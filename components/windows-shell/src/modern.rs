//! Explicit current-user deployment for the Windows 11 context menu.
//! Package registration is installer work and never runs in Explorer callbacks.

use std::{
    ffi::OsString,
    io,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, Prefix},
    thread,
};

use ::windows::{
    Foundation::Uri,
    Management::Deployment::{AddPackageOptions, DeploymentResult, PackageManager},
    Win32::{
        Foundation::{
            E_INVALIDARG, E_UNEXPECTED, ERROR_FILE_NOT_FOUND, ERROR_FILENAME_EXCED_RANGE,
            ERROR_GEN_FAILURE,
        },
        System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        UI::Shell::UrlCreateFromPathW,
    },
    core::{Error, HRESULT, HSTRING, PCWSTR, PWSTR, Result},
};

pub(crate) const PACKAGE_NAME: &str = "FragileNotepad.ShellIntegration";
pub(crate) const PACKAGE_PUBLISHER: &str = match option_env!("FRAGILE_SHELL_PUBLISHER") {
    Some(publisher) => publisher,
    None => "CN=Fragile Notepad",
};
const PACKAGE_FILE: &str = "FragileNotepad.ShellIntegration.msix";
const MAX_NATIVE_PATH_UNITS: usize = 32_767;

struct Apartment;

impl Apartment {
    fn multithreaded() -> Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

fn io_error(error: io::Error, context: &str) -> Error {
    let code = error.raw_os_error().unwrap_or(ERROR_GEN_FAILURE.0 as i32);
    Error::new(
        HRESULT::from_win32(code as u32),
        format!("{context}: {error}"),
    )
}

// WinRT asynchronous deployment uses a blocking completion wait. A fresh MTA
// avoids depending on a registration host's STA or its message pump.
fn on_mta(action: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    thread::Builder::new()
        .name("fragile-shell-deployment".into())
        .spawn(move || {
            let _apartment = Apartment::multithreaded()?;
            action()
        })
        .map_err(|error| io_error(error, "Could not start shell package deployment"))?
        .join()
        .map_err(|_| Error::new(E_UNEXPECTED, "Shell package deployment worker panicked"))?
}

fn check_deployment(result: DeploymentResult) -> Result<()> {
    let code = result.ExtendedErrorCode()?;
    if code.is_err() {
        let message = result.ErrorText().unwrap_or_default();
        return Err(Error::new(code, message.to_string_lossy()));
    }
    Ok(())
}

// UrlCreateFromPathW accepts native UTF-16 but not verbatim path prefixes.
// Only strip prefixes with equivalent ordinary drive/UNC path semantics.
fn normal_path_units(path: &Path) -> Result<Vec<u16>> {
    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) || !path.is_absolute() {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let verbatim = units.starts_with(&[92, 92, 63, 92]);
    if verbatim {
        if units.contains(&(b'/' as u16)) {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        units.drain(..4);
        if units.len() >= 4
            && units[..3]
                .iter()
                .zip([85u16, 78, 67])
                .all(|(&unit, letter)| unit == letter || unit == letter + 32)
            && units[3] == 92
        {
            units.splice(..4, [92, 92]);
        }
    }
    let ordinary = OsString::from_wide(&units);
    let ordinary = Path::new(&ordinary);
    let unc = match ordinary.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => false,
            Prefix::UNC(_, _) => true,
            _ => return Err(Error::from_hresult(E_INVALIDARG)),
        },
        _ => return Err(Error::from_hresult(E_INVALIDARG)),
    };
    if !ordinary.is_absolute() {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    if verbatim {
        // Inspect raw components: Path::components can hide literal dot segments.
        // Ordinary Win32 parsing must not trim names, interpret separators, or
        // redirect verbatim files to DOS devices. A UNC host may end in a DNS dot.
        let tail = &units[if unc { 2 } else { 3 }..];
        let mut components = tail.split(|&unit| unit == b'\\' as u16).peekable();
        let mut index = 0;
        while let Some(component) = components.next() {
            let host = unc && index == 0;
            let filename = !unc || index >= 2;
            if (component.is_empty() && components.peek().is_some())
                || component.iter().any(|&unit| {
                    unit < 32 || matches!(unit, 34 | 42 | 47 | 58 | 60 | 62 | 63 | 124)
                })
                || component
                    .last()
                    .is_some_and(|&unit| unit == b' ' as u16 || (unit == b'.' as u16 && !host))
                || (filename && is_dos_device(component))
            {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            index += 1;
        }
    }
    if units.len() > MAX_NATIVE_PATH_UNITS {
        return Err(Error::from_hresult(HRESULT::from_win32(
            ERROR_FILENAME_EXCED_RANGE.0,
        )));
    }
    Ok(units)
}

fn is_dos_device(component: &[u16]) -> bool {
    let stem = component.split(|&unit| unit == b'.' as u16).next().unwrap();
    // Only classify reserved names; the original UTF-16 path stays unchanged.
    let Ok(stem) = String::from_utf16(stem) else {
        return false;
    };
    let stem = stem.trim_end_matches(' ').to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

fn file_uri_string(path: &Path) -> Result<HSTRING> {
    let mut input = normal_path_units(path)?;
    // ASCII escaping needs at most three output units per input unit, plus
    // the file scheme and terminating NUL. Unicode stays in native UTF-16.
    let mut output = vec![0u16; input.len() * 3 + 16];
    let mut capacity = output.len() as u32;
    input.push(0);
    unsafe {
        UrlCreateFromPathW(
            PCWSTR(input.as_ptr()),
            PWSTR(output.as_mut_ptr()),
            &mut capacity,
            0,
        )?;
    }
    let length = output
        .iter()
        .position(|&unit| unit == 0)
        .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))?;
    if length == 0 {
        return Err(Error::from_hresult(E_UNEXPECTED));
    }
    Ok(HSTRING::from_wide(&output[..length]))
}

fn file_uri(path: &Path) -> Result<Uri> {
    Uri::CreateUri(&file_uri_string(path)?)
}

pub(crate) fn register(dll: &Path) -> Result<()> {
    // Validate paths and the required signed package before starting WinRT or
    // making any system changes. Package signing/trust belongs to packaging.
    normal_path_units(dll)?;
    let external = dll
        .parent()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?
        .to_path_buf();
    let package = external.join(PACKAGE_FILE);
    let metadata = package.metadata().map_err(|error| {
        io_error(
            error,
            "Windows 11 shell registration requires the adjacent signed FragileNotepad.ShellIntegration.msix",
        )
    })?;
    if !metadata.is_file() {
        return Err(Error::new(
            HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0),
            "The adjacent shell identity package is not a file",
        ));
    }
    on_mta(move || {
        let manager = PackageManager::new()?;
        let options = AddPackageOptions::new()?;
        options.SetExternalLocationUri(&file_uri(&external)?)?;
        check_deployment(
            manager
                .AddPackageByUriAsync(&file_uri(&package)?, &options)?
                .join()?,
        )
    })
}

pub(crate) fn unregister() -> Result<()> {
    on_mta(|| {
        let manager = PackageManager::new()?;
        // Empty SID means the calling user. Exact name and signing publisher
        // keep removal scoped to this component, across package versions.
        let packages = manager.FindPackagesByUserSecurityIdNamePublisher(
            &HSTRING::new(),
            &HSTRING::from(PACKAGE_NAME),
            &HSTRING::from(PACKAGE_PUBLISHER),
        )?;
        let iterator = packages.First()?;
        let mut names = Vec::new();
        while iterator.HasCurrent()? {
            names.push(iterator.Current()?.Id()?.FullName()?);
            iterator.MoveNext()?;
        }
        // Snapshot before removal: mutating the deployment set must not affect
        // enumeration, and all enumeration errors must reach the caller.
        for name in names {
            check_deployment(manager.RemovePackageAsync(&name)?.join()?)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::windows::Win32::{Foundation::ERROR_PATH_NOT_FOUND, UI::Shell::PathCreateFromUrlW};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn from_uri(uri: &HSTRING) -> OsString {
        let mut output = vec![0u16; MAX_NATIVE_PATH_UNITS + 1];
        let mut capacity = output.len() as u32;
        unsafe {
            PathCreateFromUrlW(
                PCWSTR(uri.as_ptr()),
                PWSTR(output.as_mut_ptr()),
                &mut capacity,
                0,
            )
            .unwrap();
        }
        let length = output.iter().position(|&unit| unit == 0).unwrap();
        OsString::from_wide(&output[..length])
    }

    #[test]
    fn file_uris_escape_metacharacters_and_preserve_unicode() {
        let path = Path::new(r"C:\Editor space # 100% 日本語\FragileNotepad.ShellIntegration.msix");
        let uri = file_uri_string(path).unwrap();
        let text = uri.to_string_lossy();
        assert!(text.starts_with("file:///C:/"));
        assert!(text.contains("%20"));
        assert!(text.contains("%23"));
        assert!(text.contains("%25"));
        assert!(text.contains("日本語"));
        assert_eq!(from_uri(&uri), path.as_os_str());
    }

    #[test]
    fn file_uris_normalize_verbatim_drive_and_unc_prefixes() {
        for (verbatim, ordinary) in [
            (r"\\?\C:\Editor #\shell.msix", r"C:\Editor #\shell.msix"),
            (
                r"\\?\C:\.hidden\COM10\shell.msix",
                r"C:\.hidden\COM10\shell.msix",
            ),
            (
                r"\\?\UNC\server\share\Editor #\shell.msix",
                r"\\server\share\Editor #\shell.msix",
            ),
            (
                r"\\?\UNC\server.\share\Editor #\shell.msix",
                r"\\server.\share\Editor #\shell.msix",
            ),
        ] {
            assert_eq!(
                file_uri_string(Path::new(verbatim)).unwrap(),
                file_uri_string(Path::new(ordinary)).unwrap(),
            );
        }
    }

    #[test]
    fn verbatim_paths_that_change_meaning_without_the_prefix_are_rejected() {
        for path in [
            r"\\?\C:\Editor.\shell.msix",
            r"\\?\C:\Editor \shell.msix",
            r"\\?\C:\Editor\shell.msix.",
            r"\\?\C:\Editor\shell.msix ",
            r"\\?\C:\Editor\.\shell.msix",
            r"\\?\C:\Editor\..\shell.msix",
            r"\\?\C:\Editor/subdir\shell.msix",
            r"\\?\C:/Editor\shell.msix",
            r"\\?\C:\Editor\\subdir\shell.msix",
            r"\\?\C:\Editor:stream\shell.msix",
            r"\\?\C:\Editor?\shell.msix",
            r"\\?\C:\Editor|\shell.msix",
            r"\\?\C:\NUL\shell.msix",
            r"\\?\C:\nul.txt\shell.msix",
            r"\\?\C:\NUL .txt\shell.msix",
            r"\\?\C:\COM1\shell.msix",
            r"\\?\C:\LPT².txt\shell.msix",
            r"\\?\UNC\server\share\Editor.\shell.msix",
            r"\\?\UNC\server\share\Editor \shell.msix",
            r"\\?\UNC\server \share\Editor\shell.msix",
            r"\\?\UNC\server\share\..\shell.msix",
        ] {
            assert_eq!(
                file_uri_string(Path::new(path)).unwrap_err().code(),
                E_INVALIDARG,
                "{path}"
            );
            // Rejection must happen before inspecting or deploying a package.
            assert_eq!(register(Path::new(path)).unwrap_err().code(), E_INVALIDARG);
        }
    }

    #[test]
    fn invalid_uri_paths_are_rejected() {
        for path in [
            Path::new("relative.msix"),
            Path::new("C:\\bad\0.msix"),
            Path::new(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\shell.msix"),
            Path::new(r"\\?\UNC\.\share\shell.msix"),
            Path::new(r"\\.\C:\Editor\shell.msix"),
        ] {
            assert_eq!(file_uri_string(path).unwrap_err().code(), E_INVALIDARG);
        }
    }

    #[test]
    fn external_location_uri_has_no_query_or_fragment() {
        on_mta(|| {
            let uri = file_uri(Path::new(r"C:\Editor # 100% 日本語"))?;
            assert_eq!(uri.SchemeName()?, "file");
            assert!(uri.Query()?.is_empty());
            assert!(uri.Fragment()?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn missing_package_fails_before_any_deployment() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dll = std::env::temp_dir()
            .join(format!(
                "FragileNotepadShellMissing-{}-{stamp}",
                std::process::id()
            ))
            .join("fragile_notepad_shell.dll");
        let code = register(&dll).unwrap_err().code();
        assert!(
            code == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
                || code == HRESULT::from_win32(ERROR_PATH_NOT_FOUND.0)
        );
    }
}
