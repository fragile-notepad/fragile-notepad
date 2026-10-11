#![allow(non_snake_case)] // COM method and export names are fixed by Windows.

use std::{
    ffi::{OsStr, OsString, c_void},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

use ::windows::{
    Win32::{
        Foundation::{
            CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_INVALIDARG, E_NOTIMPL,
            E_OUTOFMEMORY, E_POINTER, E_UNEXPECTED, ERROR_BAD_PATHNAME, ERROR_FILENAME_EXCED_RANGE,
            HMODULE, S_FALSE, S_OK,
        },
        System::{
            Com::{CoTaskMemAlloc, CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl},
            LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
                GetModuleHandleExW,
            },
            SystemServices::{SFGAO_FILESYSTEM, SFGAO_FOLDER},
        },
        UI::Shell::{
            ECF_DEFAULT, ECS_ENABLED, ECS_HIDDEN, IEnumExplorerCommand, IExplorerCommand,
            IExplorerCommand_Impl, IShellItemArray, SIGDN_FILESYSPATH,
        },
    },
    core::{
        BOOL, Error, GUID, HRESULT, IUnknown, Interface, PCWSTR, PWSTR, Ref, Result, implement,
    },
};

use crate::launch::{MAX_FILES, launch};

pub(crate) const COMMAND_CLSID: GUID = GUID::from_u128(0x90d3a9d8_913c_4ea4_ab25_0e0f68c08978);
pub(crate) const TITLE: &str = "Edit with Fragile Notepad";
const EXECUTABLE: &str = "fragile-notepad.exe";

static LIVE_OBJECTS: AtomicUsize = AtomicUsize::new(0);
static SERVER_LOCKS: AtomicUsize = AtomicUsize::new(0);

/// Counts allocated COM identities, rather than interface pointers/references.
/// The generated implementation drops this guard after its final Release.
struct ObjectLifetime;

impl ObjectLifetime {
    fn new() -> Self {
        LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for ObjectLifetime {
    fn drop(&mut self) {
        LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[implement(IClassFactory, Agile = false)]
struct CommandFactory {
    _lifetime: ObjectLifetime,
}

impl CommandFactory {
    fn new() -> Self {
        Self {
            _lifetime: ObjectLifetime::new(),
        }
    }
}

impl IClassFactory_Impl for CommandFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        if object.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        // COM requires a null output on every unsuccessful query.
        unsafe {
            object.write(ptr::null_mut());
        }
        if iid.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        if !outer.is_null() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let command: IExplorerCommand = ExplorerCommand::new().into();
        unsafe { command.query(iid, object).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            SERVER_LOCKS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        } else {
            // A mismatched unlock must not wrap around and pin the DLL forever.
            SERVER_LOCKS
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                    count.checked_sub(1)
                })
                .map(|_| ())
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))
        }
    }
}

#[implement(IExplorerCommand, Agile = false)]
struct ExplorerCommand {
    _lifetime: ObjectLifetime,
}

impl ExplorerCommand {
    fn new() -> Self {
        Self {
            _lifetime: ObjectLifetime::new(),
        }
    }
}

impl IExplorerCommand_Impl for ExplorerCommand_Impl {
    fn GetTitle(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        task_string(OsStr::new(TITLE))
    }

    fn GetIcon(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        let mut icon = adjacent_executable()?.into_os_string();
        icon.push(",0");
        task_string(&icon)
    }

    fn GetToolTip(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        task_string(OsStr::new(TITLE))
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(COMMAND_CLSID)
    }

    fn GetState(&self, selection: Ref<IShellItemArray>, _: BOOL) -> Result<u32> {
        // No file contents, executable probing, process launch, or registry work
        // during menu construction. Invalid/unavailable selections are hidden.
        let enabled = selection
            .as_ref()
            .is_some_and(|items| valid_selection(items).is_ok());
        Ok(if enabled {
            ECS_ENABLED.0 as u32
        } else {
            ECS_HIDDEN.0 as u32
        })
    }

    fn Invoke(&self, selection: Ref<IShellItemArray>, _: Ref<IBindCtx>) -> Result<()> {
        let items = selection
            .as_ref()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let count = valid_selection(items)?;
        let mut paths = Vec::with_capacity(count as usize);
        // Resolve the whole selection before launch, so a virtual or failed item
        // can never result in opening only a subset of the selected files.
        for index in 0..count {
            let item = unsafe { items.GetItemAt(index)? };
            let name = TaskString(unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? });
            if name.0.is_null() {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            let path = unsafe { OsString::from_wide(name.0.as_wide()) };
            if !Path::new(&path).is_absolute() {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            paths.push(path);
        }
        launch(&adjacent_executable()?, &paths)
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

fn valid_selection(items: &IShellItemArray) -> Result<u32> {
    let count = unsafe { items.GetCount()? };
    if count == 0 || count as usize > MAX_FILES {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    // Checking each item catches mixed file/folder or file/virtual selections.
    // Aggregated AND attributes alone would miss a folder in a file selection.
    for index in 0..count {
        let item = unsafe { items.GetItemAt(index)? };
        let attributes = unsafe { item.GetAttributes(SFGAO_FILESYSTEM | SFGAO_FOLDER)? };
        if attributes.0 & SFGAO_FILESYSTEM.0 == 0 || attributes.0 & SFGAO_FOLDER.0 != 0 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
    }
    Ok(count)
}

struct TaskString(PWSTR);

impl Drop for TaskString {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0.0.cast()));
        }
    }
}

fn task_string(value: &OsStr) -> Result<PWSTR> {
    let mut units: Vec<u16> = value.encode_wide().collect();
    units.push(0);
    let allocation = unsafe { CoTaskMemAlloc(units.len() * size_of::<u16>()) }.cast::<u16>();
    if allocation.is_null() {
        return Err(Error::from_hresult(E_OUTOFMEMORY));
    }
    unsafe {
        ptr::copy_nonoverlapping(units.as_ptr(), allocation, units.len());
    }
    Ok(PWSTR(allocation))
}

fn adjacent_executable() -> Result<PathBuf> {
    executable_beside(&module_path()?)
}

pub(crate) fn module_path() -> Result<PathBuf> {
    let mut module = HMODULE::default();
    // Resolve this DLL by an address inside it. Explorer's executable, current
    // directory, and PATH must never determine which editor gets launched.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(DllGetClassObject as *const () as *const u16),
            &mut module,
        )?;
    }
    let mut units = vec![0u16; 512];
    loop {
        let length = unsafe { GetModuleFileNameW(Some(module), &mut units) } as usize;
        if length == 0 {
            return Err(Error::from_thread());
        }
        if length < units.len() {
            let path = PathBuf::from(OsString::from_wide(&units[..length]));
            return Ok(path);
        }
        if units.len() >= 32_768 {
            return Err(Error::from_hresult(HRESULT::from_win32(
                ERROR_FILENAME_EXCED_RANGE.0,
            )));
        }
        units.resize((units.len() * 2).min(32_768), 0);
    }
}

fn executable_beside(module: &Path) -> Result<PathBuf> {
    if !module.is_absolute() {
        return Err(Error::from_hresult(HRESULT::from_win32(
            ERROR_BAD_PATHNAME.0,
        )));
    }
    module
        .parent()
        .map(|directory| directory.join(EXECUTABLE))
        .ok_or_else(|| Error::from_hresult(HRESULT::from_win32(ERROR_BAD_PATHNAME.0)))
}

/// Standard COM activation entry point; registration requires separate exports.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class_id: *const GUID,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if object.is_null() {
        return E_POINTER;
    }
    unsafe {
        object.write(ptr::null_mut());
    }
    if class_id.is_null() || iid.is_null() {
        return E_POINTER;
    }
    if unsafe { *class_id } != COMMAND_CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = CommandFactory::new().into();
    unsafe { factory.query(iid, object) }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE_OBJECTS.load(Ordering::SeqCst) == 0 && SERVER_LOCKS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests;
