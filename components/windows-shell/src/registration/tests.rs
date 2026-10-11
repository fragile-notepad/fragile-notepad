use super::*;
use ::windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    time::{SystemTime, UNIX_EPOCH},
};

// Exercise real Win32 serialization/removal under a unique private subtree,
// never under Software\Classes and never via the registration exports.
struct TestRegistry {
    root: Key,
    name: String,
}

impl TestRegistry {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!(
            "Software\\FragileNotepadShellTests-{}-{stamp}",
            std::process::id()
        );
        Self {
            root: Key::create(HKEY_CURRENT_USER, &name).unwrap(),
            name,
        }
    }
}

impl Drop for TestRegistry {
    fn drop(&mut self) {
        let _ = remove_tree(HKEY_CURRENT_USER, &self.name);
    }
}

fn read(root: &Key, subkey: &str, name: &str) -> Result<OsString> {
    let subkey = wide(OsStr::new(subkey))?;
    let name = wide(OsStr::new(name))?;
    let mut bytes = 0;
    unsafe {
        RegGetValueW(
            root.0,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
        .ok()?;
    }
    let mut units = vec![0u16; bytes as usize / 2];
    unsafe {
        RegGetValueW(
            root.0,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(units.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()?;
    }
    units.truncate(bytes as usize / 2);
    assert_eq!(units.pop(), Some(0));
    Ok(OsString::from_wide(&units))
}

#[test]
fn registration_updates_native_values_and_removal_preserves_other_commands() {
    let sandbox = TestRegistry::new();
    let other = Key::create(sandbox.root.0, r"*\shell\Other.Editor").unwrap();
    other.set("MUIVerb", OsStr::new("Other editor")).unwrap();
    drop(other);
    let shell = Key::create(sandbox.root.0, r"*\shell").unwrap();
    shell.set("", OsStr::new("Other.Editor")).unwrap();
    drop(shell);

    // Includes spaces, non-ASCII text, and an unpaired surrogate. REG_SZ and
    // icon construction must preserve all original UTF-16 units.
    let mut dll = OsString::from(r"C:\Editor & 日本語 ");
    dll.push(OsString::from_wide(&[0xd800]));
    dll.push(r"\fragile_notepad_shell.dll");
    let dll = Path::new(&dll);
    register_at(&sandbox.root, dll).unwrap();
    let server = format!("{}\\InprocServer32", class_key());
    assert_eq!(read(&sandbox.root, &server, "").unwrap(), dll.as_os_str());
    assert_eq!(
        read(&sandbox.root, &server, "ThreadingModel").unwrap(),
        "Apartment"
    );
    assert_eq!(read(&sandbox.root, VERB_KEY, "MUIVerb").unwrap(), TITLE);
    assert_eq!(
        read(&sandbox.root, VERB_KEY, "ExplorerCommandHandler").unwrap(),
        OsString::from(format!("{{{COMMAND_CLSID:?}}}"))
    );
    assert_eq!(
        read(&sandbox.root, VERB_KEY, "MultiSelectModel").unwrap(),
        "Player"
    );
    assert_eq!(read(&sandbox.root, VERB_KEY, "NeverDefault").unwrap(), "");
    let mut icon = OsString::from("\"");
    icon.push(dll.parent().unwrap().join("fragile-notepad.exe"));
    icon.push("\",0");
    assert_eq!(read(&sandbox.root, VERB_KEY, "Icon").unwrap(), icon);

    let moved = Path::new(r"D:\Moved Editor\fragile_notepad_shell.dll");
    register_at(&sandbox.root, moved).unwrap();
    assert_eq!(read(&sandbox.root, &server, "").unwrap(), moved.as_os_str());
    unregister_at(&sandbox.root).unwrap();
    unregister_at(&sandbox.root).unwrap();
    assert!(read(&sandbox.root, &server, "").is_err());
    assert!(read(&sandbox.root, VERB_KEY, "MUIVerb").is_err());
    assert_eq!(read(&sandbox.root, r"*\shell", "").unwrap(), "Other.Editor");
    assert_eq!(
        read(&sandbox.root, r"*\shell\Other.Editor", "MUIVerb").unwrap(),
        "Other editor"
    );
}

#[test]
fn invalid_paths_are_rejected_before_registry_changes() {
    let sandbox = TestRegistry::new();
    for dll in [Path::new("relative.dll"), Path::new("C:\\bad\0.dll")] {
        assert_eq!(
            register_at(&sandbox.root, dll).unwrap_err().code(),
            E_INVALIDARG
        );
        assert!(read(&sandbox.root, VERB_KEY, "MUIVerb").is_err());
    }
}

#[test]
fn manifest_and_registration_share_the_com_identity() {
    let manifest = include_str!("../../AppxManifest.xml");
    let identity = format!("{COMMAND_CLSID:?}");
    assert!(manifest.contains(&format!("Id=\"{identity}\"")));
    assert!(manifest.contains(&format!("Clsid=\"{identity}\"")));
}

#[test]
fn earlier_windows_registers_only_the_classic_backend() {
    let sandbox = TestRegistry::new();
    register_for(
        MenuGeneration::Legacy,
        &sandbox.root,
        Path::new(r"C:\Editor\fragile_notepad_shell.dll"),
        || panic!("earlier Windows must not deploy a modern package"),
    )
    .unwrap();
    assert_eq!(read(&sandbox.root, VERB_KEY, "MUIVerb").unwrap(), TITLE);
    unregister_for(MenuGeneration::Legacy, &sandbox.root, || {
        panic!("earlier Windows must not use modern package removal")
    })
    .unwrap();
    assert!(read(&sandbox.root, VERB_KEY, "MUIVerb").is_err());
}

#[test]
fn windows_11_replaces_classic_registration_after_package_deployment() {
    let sandbox = TestRegistry::new();
    let dll = Path::new(r"C:\Editor\fragile_notepad_shell.dll");
    register_at(&sandbox.root, dll).unwrap();
    let mut deployed = false;
    register_for(MenuGeneration::Modern, &sandbox.root, dll, || {
        // The previous verb survives until deployment succeeds.
        assert_eq!(read(&sandbox.root, VERB_KEY, "MUIVerb").unwrap(), TITLE);
        deployed = true;
        Ok(())
    })
    .unwrap();
    assert!(deployed);
    assert!(read(&sandbox.root, VERB_KEY, "MUIVerb").is_err());
    assert!(
        read(
            &sandbox.root,
            &format!("{}\\InprocServer32", class_key()),
            ""
        )
        .is_err()
    );
}

#[test]
fn modern_deployment_failure_never_installs_a_classic_fallback() {
    use ::windows::Win32::Foundation::E_FAIL;
    let sandbox = TestRegistry::new();
    let dll = Path::new(r"C:\Editor\fragile_notepad_shell.dll");
    let failed_deployment = || Err(Error::from_hresult(E_FAIL));
    assert_eq!(
        register_for(
            MenuGeneration::Modern,
            &sandbox.root,
            dll,
            failed_deployment
        )
        .unwrap_err()
        .code(),
        E_FAIL
    );
    assert!(read(&sandbox.root, VERB_KEY, "MUIVerb").is_err());
    // An existing classic installation is preserved if migration fails.
    register_at(&sandbox.root, dll).unwrap();
    assert!(
        register_for(
            MenuGeneration::Modern,
            &sandbox.root,
            dll,
            failed_deployment
        )
        .is_err()
    );
    assert_eq!(read(&sandbox.root, VERB_KEY, "MUIVerb").unwrap(), TITLE);
}

#[test]
fn modern_removal_also_cleans_old_registry_keys_and_preserves_failure() {
    use ::windows::Win32::Foundation::E_FAIL;
    let sandbox = TestRegistry::new();
    register_at(
        &sandbox.root,
        Path::new(r"C:\Editor\fragile_notepad_shell.dll"),
    )
    .unwrap();
    let mut removed = false;
    let result = unregister_for(MenuGeneration::Modern, &sandbox.root, || {
        removed = true;
        Err(Error::from_hresult(E_FAIL))
    });
    assert!(removed);
    assert_eq!(result.unwrap_err().code(), E_FAIL);
    assert!(read(&sandbox.root, VERB_KEY, "MUIVerb").is_err());
}
