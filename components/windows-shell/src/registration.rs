//! Explicit current-user registration, selecting the native menu by OS version.
#![allow(non_snake_case)]

use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path};

use ::windows::{
    Win32::{
        Foundation::{E_INVALIDARG, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, S_OK},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
            REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, RegCloseKey, RegCreateKeyExW,
            RegDeleteTreeW, RegSetValueExW,
        },
        UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify},
    },
    core::{Error, HRESULT, PCWSTR, Result},
};

use crate::{
    modern,
    os_version::{MenuGeneration, current_menu_generation},
    windows::{COMMAND_CLSID, TITLE, module_path},
};

const VERB_KEY: &str = r"*\shell\FragileNotepad.Edit";

// Registry access follows the DLL/registration host architecture. The future
// installer must distribute the native Explorer build and use a matching host.
const REGISTRY_VIEW: REG_SAM_FLAGS = if cfg!(target_pointer_width = "64") {
    KEY_WOW64_64KEY
} else {
    KEY_WOW64_32KEY
};

struct Key(HKEY);

impl Key {
    fn create(parent: HKEY, name: &str) -> Result<Self> {
        let name = wide(OsStr::new(name))?;
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                parent,
                PCWSTR(name.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS | REGISTRY_VIEW,
                None,
                &mut key,
                None,
            )
            .ok()?;
        }
        Ok(Self(key))
    }

    fn set(&self, name: &str, value: &OsStr) -> Result<()> {
        let name = wide(OsStr::new(name))?;
        let bytes: Vec<u8> = wide(value)?
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        unsafe { RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)).ok() }
    }

    fn remove(&self, name: &str) -> Result<()> {
        remove_tree(self.0, name)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut units: Vec<u16> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    units.push(0);
    Ok(units)
}

fn class_key() -> String {
    format!("CLSID\\{{{COMMAND_CLSID:?}}}")
}

fn register_at(classes: &Key, dll: &Path) -> Result<()> {
    if !dll.is_absolute() {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let executable = dll
        .parent()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?
        .join("fragile-notepad.exe");
    // Validate native strings before writing any registration values.
    wide(dll.as_os_str())?;
    wide(executable.as_os_str())?;
    let server = Key::create(classes.0, &format!("{}\\InprocServer32", class_key()))?;
    server.set("", dll.as_os_str())?;
    server.set("ThreadingModel", OsStr::new("Apartment"))?;
    let verb = Key::create(classes.0, VERB_KEY)?;
    verb.set("MUIVerb", OsStr::new(TITLE))?;
    let mut icon = OsStr::new("\"").to_os_string();
    icon.push(executable);
    icon.push("\",0");
    verb.set("Icon", &icon)?;
    verb.set(
        "ExplorerCommandHandler",
        OsStr::new(&format!("{{{COMMAND_CLSID:?}}}")),
    )?;
    verb.set("MultiSelectModel", OsStr::new("Player"))?;
    verb.set("NeverDefault", OsStr::new(""))
}

fn unregister_at(classes: &Key) -> Result<()> {
    // Attempt both removals, preserving the first error if either fails.
    let verb = classes.remove(VERB_KEY);
    let class = classes.remove(&class_key());
    verb.and(class)
}

fn register_for(
    generation: MenuGeneration,
    classes: &Key,
    dll: &Path,
    register_modern: impl FnOnce() -> Result<()>,
) -> Result<()> {
    match generation {
        MenuGeneration::Legacy => register_at(classes, dll),
        MenuGeneration::Modern => {
            register_modern()?;
            // A successful modern registration replaces the old classic verb.
            // Keep it intact if package deployment fails, and never silently
            // install a classic fallback on Windows 11.
            unregister_at(classes)
        }
    }
}

fn unregister_for(
    generation: MenuGeneration,
    classes: &Key,
    unregister_modern: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let modern = match generation {
        MenuGeneration::Legacy => Ok(()),
        MenuGeneration::Modern => unregister_modern(),
    };
    // Also clean up the component's old registry keys after an OS upgrade or
    // an earlier classic installation, even if modern removal reports an error.
    let legacy = unregister_at(classes);
    modern.and(legacy)
}

fn remove_tree(parent: HKEY, name: &str) -> Result<()> {
    let name = wide(OsStr::new(name))?;
    let status = unsafe { RegDeleteTreeW(parent, PCWSTR(name.as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        Ok(())
    } else {
        status.ok()
    }
}

fn notify_shell() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

/// Explicit current-user registration: sparse package on Windows 11+, classic
/// registry verb on earlier versions. Does not run when the DLL is loaded.
#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    let result = (|| {
        let generation = current_menu_generation()?;
        let dll = module_path()?;
        let classes = Key::create(HKEY_CURRENT_USER, r"Software\Classes")?;
        register_for(generation, &classes, &dll, || modern::register(&dll))
    })();
    match result {
        Ok(()) => {
            notify_shell();
            S_OK
        }
        Err(error) => error.code(),
    }
}

/// Explicit current-user removal with the matching OS backend and legacy cleanup.
#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    let result = (|| {
        let generation = current_menu_generation()?;
        let classes = Key::create(HKEY_CURRENT_USER, r"Software\Classes")?;
        unregister_for(generation, &classes, modern::unregister)
    })();
    match result {
        Ok(()) => {
            notify_shell();
            S_OK
        }
        Err(error) => error.code(),
    }
}

#[cfg(test)]
mod tests;
