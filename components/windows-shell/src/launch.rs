use std::{
    ffi::{OsStr, OsString},
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
};

use ::windows::{
    Win32::{
        Foundation::{E_INVALIDARG, ERROR_FILENAME_EXCED_RANGE},
        System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS},
    },
    core::{Error, HRESULT, Result},
};

// Match the editor's single-instance IPC limit; never open a partial selection.
pub(crate) const MAX_FILES: usize = 4096;
const MAX_COMMAND_LINE: usize = 32_767; // CreateProcessW includes the NUL terminator.

/// Encode the same command line as `Command::arg`, solely to validate its length.
/// The program is quoted unconditionally, while normal arguments use the CRT
/// backslash/quote convention used by Rust's Windows process implementation.
fn command_line(program: &OsStr, paths: &[OsString]) -> Result<Vec<u16>> {
    if paths.is_empty() || paths.len() > MAX_FILES || !Path::new(program).is_absolute() {
        return Err(Error::from_hresult(E_INVALIDARG));
    }

    let mut line = Vec::new();
    append_argument(&mut line, program, true)?;
    line.push(b' ' as u16);
    append_argument(&mut line, OsStr::new("--"), false)?;
    for path in paths {
        if !Path::new(path).is_absolute() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        line.push(b' ' as u16);
        append_argument(&mut line, path, false)?;
    }
    check_length(line.len())?;
    Ok(line)
}

fn check_length(length: usize) -> Result<()> {
    if length >= MAX_COMMAND_LINE {
        Err(Error::from_hresult(HRESULT::from_win32(
            ERROR_FILENAME_EXCED_RANGE.0,
        )))
    } else {
        Ok(())
    }
}

fn append_argument(line: &mut Vec<u16>, argument: &OsStr, force_quotes: bool) -> Result<()> {
    let units: Vec<_> = argument.encode_wide().take(MAX_COMMAND_LINE).collect();
    check_length(units.len())?;
    if units.contains(&0) {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let quoted = force_quotes
        || units.is_empty()
        || units
            .iter()
            .any(|&unit| unit == b' ' as u16 || unit == b'\t' as u16);
    if quoted {
        line.push(b'"' as u16);
    }
    let mut backslashes = 0;
    for unit in units {
        if unit == b'\\' as u16 {
            backslashes += 1;
        } else {
            if unit == b'"' as u16 {
                line.extend(std::iter::repeat_n(b'\\' as u16, backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(unit);
        check_length(line.len())?;
    }
    if quoted {
        line.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        line.push(b'"' as u16);
    }
    check_length(line.len())
}

pub(crate) fn launch(program: &Path, paths: &[OsString]) -> Result<()> {
    // Validate every argument before creating any child process. Passing native
    // OsStrings to Command preserves Unicode and unpaired UTF-16 surrogates.
    command_line(program.as_os_str(), paths)?;
    let child = Command::new(program)
        .arg("--")
        .args(paths)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags((DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).0)
        .spawn()
        .map_err(|error| {
            Error::from_hresult(HRESULT::from_win32(error.raw_os_error().unwrap_or(1) as u32))
        })?;
    // The editor owns its lifetime. Dropping Child only closes our process handles.
    drop(child);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            UI::Shell::CommandLineToArgvW,
        },
        core::PCWSTR,
    };

    #[test]
    fn arguments_round_trip_through_windows_parser() {
        let program = OsStr::new(r"C:\Program Files\Fragile Notepad\fragile-notepad.exe");
        let paths = vec![
            OsString::from(r"C:\notes\ordinary.txt"),
            OsString::from(r"C:\my notes\a & b; $notes.txt"),
            OsString::from("C:\\notes\\tab\tname.txt"),
            OsString::from(r"C:\my notes\trailing\"),
            OsString::from(r"\\server\share\日本語😀.txt"),
            OsString::from_wide(&[
                b'C' as u16,
                b':' as u16,
                b'\\' as u16,
                0xD800,
                b'.' as u16,
                b't' as u16,
            ]),
        ];
        let mut line = command_line(program, &paths).unwrap();
        line.push(0);
        let mut count = 0;
        let arguments = unsafe { CommandLineToArgvW(PCWSTR(line.as_ptr()), &mut count) };
        assert!(!arguments.is_null());
        let parsed: Vec<OsString> = unsafe {
            std::slice::from_raw_parts(arguments, count as usize)
                .iter()
                .map(|value| OsString::from_wide(value.as_wide()))
                .collect()
        };
        unsafe {
            LocalFree(Some(HLOCAL(arguments.cast())));
        }
        let mut expected = vec![program.to_os_string(), OsString::from("--")];
        expected.extend(paths);
        assert_eq!(parsed, expected);
    }

    #[test]
    fn quote_escape_round_trip() {
        // Quotes cannot occur in Windows file names, but the length encoder must
        // still implement the same escaping as Command for every argument.
        let argument = OsStr::new("space \\\"quote\\\"\\\\");
        let mut line: Vec<u16> = "\"C:\\editor.exe\" ".encode_utf16().collect();
        append_argument(&mut line, argument, false).unwrap();
        line.push(0);
        let mut count = 0;
        let values = unsafe { CommandLineToArgvW(PCWSTR(line.as_ptr()), &mut count) };
        assert!(!values.is_null());
        assert_eq!(count, 2);
        let parsed = unsafe { OsString::from_wide((*values.add(1)).as_wide()) };
        unsafe {
            LocalFree(Some(HLOCAL(values.cast())));
        }
        assert_eq!(parsed, argument);
    }

    #[test]
    fn command_line_limit_includes_terminator() {
        let program = OsStr::new(r"C:\e.exe");
        let fixed = command_line(program, &[OsString::from(r"C:\")])
            .unwrap()
            .len();
        let maximum = OsString::from(format!("C:\\{}", "a".repeat(MAX_COMMAND_LINE - 1 - fixed)));
        assert_eq!(
            command_line(program, std::slice::from_ref(&maximum))
                .unwrap()
                .len(),
            MAX_COMMAND_LINE - 1
        );
        let mut too_long = maximum;
        too_long.push("a");
        assert_eq!(
            command_line(program, &[too_long]).unwrap_err().code(),
            HRESULT::from_win32(ERROR_FILENAME_EXCED_RANGE.0)
        );
    }

    #[test]
    fn relative_empty_nul_and_excessive_selections_are_rejected() {
        let program = OsStr::new(r"C:\editor.exe");
        assert!(command_line(OsStr::new("editor.exe"), &[OsString::from(r"C:\a.txt")]).is_err());
        assert!(command_line(program, &[]).is_err());
        for path in ["a.txt", "C:a.txt", r"\a.txt", "C:\\a\0.txt"] {
            assert_eq!(
                command_line(program, &[path.into()]).unwrap_err().code(),
                E_INVALIDARG
            );
        }
        assert_eq!(
            command_line(program, &vec![OsString::from(r"C:\a"); MAX_FILES + 1])
                .unwrap_err()
                .code(),
            E_INVALIDARG
        );
    }

    #[test]
    fn missing_absolute_executable_returns_error() {
        let missing = std::env::temp_dir().join(format!(
            "fragile-notepad-missing-{}.exe",
            std::process::id()
        ));
        assert!(!missing.exists());
        assert!(launch(&missing, &[OsString::from(r"C:\note.txt")]).is_err());
    }
}
