use super::*;
use ::windows::{
    Win32::{
        Foundation::{E_FAIL, E_NOINTERFACE, PROPERTYKEY},
        System::SystemServices::SFGAO_FLAGS,
        UI::Shell::{
            IEnumShellItems, IShellItem, IShellItem_Impl, IShellItemArray_Impl,
            PropertiesSystem::GETPROPERTYSTOREFLAGS, SIATTRIBFLAGS, SIGDN,
        },
    },
    core::IUnknown,
};
use std::sync::{Arc, Mutex};

// COM object counts are process-wide; keep tests that create commands serialized.
static COM_TESTS: Mutex<()> = Mutex::new(());

fn factory() -> IClassFactory {
    let mut object = ptr::null_mut();
    unsafe {
        assert_eq!(
            DllGetClassObject(&COMMAND_CLSID, &IClassFactory::IID, &mut object),
            S_OK
        );
        IClassFactory::from_raw(object)
    }
}

#[test]
fn com_factory_queries_and_lifetimes_control_unload() {
    let _guard = COM_TESTS.lock().unwrap();
    assert_eq!(DllCanUnloadNow(), S_OK);
    let factory = factory();
    assert_eq!(DllCanUnloadNow(), S_FALSE);
    let unknown: IUnknown = factory.cast().unwrap();
    let factory_again: IClassFactory = unknown.cast().unwrap();
    let command: IExplorerCommand = unsafe { factory.CreateInstance(None).unwrap() };
    let command_again: IExplorerCommand = command.cast::<IUnknown>().unwrap().cast().unwrap();
    assert_eq!(
        unsafe { command.GetCanonicalName().unwrap() },
        COMMAND_CLSID
    );
    assert_eq!(unsafe { command.GetFlags().unwrap() }, ECF_DEFAULT.0 as u32);
    drop(factory_again);
    drop(unknown);
    drop(factory);
    assert_eq!(DllCanUnloadNow(), S_FALSE);
    drop(command);
    assert_eq!(DllCanUnloadNow(), S_FALSE);
    drop(command_again);
    assert_eq!(DllCanUnloadNow(), S_OK);
}

#[test]
fn server_locks_outlive_factory_and_reject_unbalanced_unlock() {
    let _guard = COM_TESTS.lock().unwrap();
    let first = factory();
    assert_eq!(
        unsafe { first.LockServer(false).unwrap_err().code() },
        E_UNEXPECTED
    );
    unsafe {
        first.LockServer(true).unwrap();
    }
    drop(first);
    assert_eq!(DllCanUnloadNow(), S_FALSE);
    let next = factory();
    unsafe {
        next.LockServer(false).unwrap();
    }
    drop(next);
    assert_eq!(DllCanUnloadNow(), S_OK);
}

#[test]
fn invalid_class_interface_pointers_and_aggregation_fail_cleanly() {
    let _guard = COM_TESTS.lock().unwrap();
    let unsupported = GUID::from_u128(0x9014b4ba_09cf_48c8_bb27_000000000001);
    let mut object = ptr::dangling_mut::<c_void>();
    unsafe {
        assert_eq!(
            DllGetClassObject(&unsupported, &IClassFactory::IID, &mut object),
            CLASS_E_CLASSNOTAVAILABLE
        );
        assert!(object.is_null());
        assert_eq!(
            DllGetClassObject(&COMMAND_CLSID, &unsupported, &mut object),
            E_NOINTERFACE
        );
        assert!(object.is_null());
        assert_eq!(
            DllGetClassObject(ptr::null(), &IClassFactory::IID, &mut object),
            E_POINTER
        );
        assert_eq!(
            DllGetClassObject(&COMMAND_CLSID, ptr::null(), &mut object),
            E_POINTER
        );
        assert_eq!(
            DllGetClassObject(&COMMAND_CLSID, &IClassFactory::IID, ptr::null_mut()),
            E_POINTER
        );
    }
    assert_eq!(DllCanUnloadNow(), S_OK);
    let factory = factory();
    let outer: IUnknown = factory.cast().unwrap();
    assert_eq!(
        unsafe {
            factory
                .CreateInstance::<_, IExplorerCommand>(&outer)
                .unwrap_err()
                .code()
        },
        CLASS_E_NOAGGREGATION
    );
    assert_eq!(
        unsafe {
            factory
                .CreateInstance::<_, IClassFactory>(None)
                .unwrap_err()
                .code()
        },
        E_NOINTERFACE
    );
    let vtable = Interface::vtable(&factory);
    unsafe {
        assert_eq!(
            (vtable.CreateInstance)(factory.as_raw(), ptr::null_mut(), ptr::null(), &mut object),
            E_POINTER
        );
        assert!(object.is_null());
        assert_eq!(
            (vtable.CreateInstance)(
                factory.as_raw(),
                ptr::null_mut(),
                &IExplorerCommand::IID,
                ptr::null_mut()
            ),
            E_POINTER
        );
    }
    drop(outer);
    drop(factory);
    assert_eq!(DllCanUnloadNow(), S_OK);
}

#[test]
fn strings_are_task_allocated_and_executable_is_beside_module() {
    let _guard = COM_TESTS.lock().unwrap();
    let command: IExplorerCommand = ExplorerCommand::new().into();
    let title = TaskString(unsafe { command.GetTitle(None).unwrap() });
    let tooltip = TaskString(unsafe { command.GetToolTip(None).unwrap() });
    assert_eq!(unsafe { OsString::from_wide(title.0.as_wide()) }, TITLE);
    assert_eq!(unsafe { OsString::from_wide(tooltip.0.as_wide()) }, TITLE);
    let icon = TaskString(unsafe { command.GetIcon(None).unwrap() });
    let mut expected = adjacent_executable().unwrap().into_os_string();
    expected.push(",0");
    assert_eq!(unsafe { OsString::from_wide(icon.0.as_wide()) }, expected);
    assert_eq!(
        executable_beside(Path::new(r"C:\Editor Folder\fragile_notepad_shell.dll")).unwrap(),
        Path::new(r"C:\Editor Folder\fragile-notepad.exe")
    );
    assert!(executable_beside(Path::new("relative.dll")).is_err());
    assert_eq!(
        unsafe { command.EnumSubCommands().unwrap_err().code() },
        E_NOTIMPL
    );
}

#[implement(IShellItem)]
struct FakeItem {
    attributes: SFGAO_FLAGS,
    fail_attributes: bool,
    name: OsString,
    display_name_queries: Arc<AtomicUsize>,
}

impl IShellItem_Impl for FakeItem_Impl {
    fn BindToHandler(
        &self,
        _: Ref<IBindCtx>,
        _: *const GUID,
        _: *const GUID,
        _: *mut *mut c_void,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn GetParent(&self) -> Result<IShellItem> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn GetDisplayName(&self, mode: SIGDN) -> Result<PWSTR> {
        assert_eq!(mode, SIGDN_FILESYSPATH);
        self.display_name_queries.fetch_add(1, Ordering::SeqCst);
        task_string(&self.name)
    }
    fn GetAttributes(&self, mask: SFGAO_FLAGS) -> Result<SFGAO_FLAGS> {
        if self.fail_attributes {
            Err(Error::from_hresult(E_FAIL))
        } else {
            Ok(self.attributes & mask)
        }
    }
    fn Compare(&self, _: Ref<IShellItem>, _: u32) -> Result<i32> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

#[implement(IShellItemArray)]
struct FakeArray {
    items: Vec<IShellItem>,
    fail_count: bool,
    reported_count: Option<u32>,
}

impl IShellItemArray_Impl for FakeArray_Impl {
    fn BindToHandler(
        &self,
        _: Ref<IBindCtx>,
        _: *const GUID,
        _: *const GUID,
        _: *mut *mut c_void,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn GetPropertyStore(
        &self,
        _: GETPROPERTYSTOREFLAGS,
        _: *const GUID,
        _: *mut *mut c_void,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn GetPropertyDescriptionList(
        &self,
        _: *const PROPERTYKEY,
        _: *const GUID,
        _: *mut *mut c_void,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn GetAttributes(&self, _: SIATTRIBFLAGS, _: SFGAO_FLAGS) -> Result<SFGAO_FLAGS> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn GetCount(&self) -> Result<u32> {
        if self.fail_count {
            Err(Error::from_hresult(E_FAIL))
        } else {
            Ok(self.reported_count.unwrap_or(self.items.len() as u32))
        }
    }
    fn GetItemAt(&self, index: u32) -> Result<IShellItem> {
        self.items
            .get(index as usize)
            .cloned()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))
    }
    fn EnumItems(&self) -> Result<IEnumShellItems> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

fn selection(attributes: &[SFGAO_FLAGS]) -> (IShellItemArray, Arc<AtomicUsize>) {
    let queries = Arc::new(AtomicUsize::new(0));
    let items = attributes
        .iter()
        .map(|&attributes| {
            FakeItem {
                attributes,
                fail_attributes: false,
                name: OsString::from(r"C:\notes\a.txt"),
                display_name_queries: queries.clone(),
            }
            .into()
        })
        .collect();
    (
        FakeArray {
            items,
            fail_count: false,
            reported_count: None,
        }
        .into(),
        queries,
    )
}

#[test]
fn state_only_accepts_nonempty_selections_of_filesystem_files() {
    let _guard = COM_TESTS.lock().unwrap();
    let command: IExplorerCommand = ExplorerCommand::new().into();
    assert_eq!(
        unsafe { command.GetState(None, false).unwrap() },
        ECS_HIDDEN.0 as u32
    );
    for (attributes, state) in [
        (vec![], ECS_HIDDEN),
        (vec![SFGAO_FILESYSTEM], ECS_ENABLED),
        (vec![SFGAO_FILESYSTEM, SFGAO_FILESYSTEM], ECS_ENABLED),
        (vec![SFGAO_FILESYSTEM | SFGAO_FOLDER], ECS_HIDDEN),
        (vec![SFGAO_FLAGS(0)], ECS_HIDDEN),
        (
            vec![SFGAO_FILESYSTEM, SFGAO_FOLDER | SFGAO_FILESYSTEM],
            ECS_HIDDEN,
        ),
        (vec![SFGAO_FILESYSTEM, SFGAO_FLAGS(0)], ECS_HIDDEN),
    ] {
        let (items, names) = selection(&attributes);
        for slow_allowed in [false, true] {
            assert_eq!(
                unsafe { command.GetState(&items, slow_allowed).unwrap() },
                state.0 as u32
            );
        }
        assert_eq!(
            names.load(Ordering::SeqCst),
            0,
            "GetState must not resolve file paths"
        );
        if state == ECS_HIDDEN {
            assert!(unsafe { command.Invoke(&items, None) }.is_err());
            assert_eq!(names.load(Ordering::SeqCst), 0);
        }
    }
    assert!(unsafe { command.Invoke(None, None) }.is_err());
    for (fail_count, reported_count) in [
        (true, None),
        (false, Some(1)),
        (false, Some(MAX_FILES as u32 + 1)),
    ] {
        let items: IShellItemArray = FakeArray {
            items: vec![],
            fail_count,
            reported_count,
        }
        .into();
        assert_eq!(
            unsafe { command.GetState(&items, false).unwrap() },
            ECS_HIDDEN.0 as u32
        );
        assert!(unsafe { command.Invoke(&items, None) }.is_err());
    }
    let queries = Arc::new(AtomicUsize::new(0));
    let item: IShellItem = FakeItem {
        attributes: SFGAO_FILESYSTEM,
        fail_attributes: true,
        name: OsString::new(),
        display_name_queries: queries.clone(),
    }
    .into();
    let items: IShellItemArray = FakeArray {
        items: vec![item],
        fail_count: false,
        reported_count: None,
    }
    .into();
    assert_eq!(
        unsafe { command.GetState(&items, false).unwrap() },
        ECS_HIDDEN.0 as u32
    );
    assert_eq!(queries.load(Ordering::SeqCst), 0);
}

#[test]
fn invoke_rejects_a_relative_shell_path_before_launch() {
    let _guard = COM_TESTS.lock().unwrap();
    let command: IExplorerCommand = ExplorerCommand::new().into();
    let item: IShellItem = FakeItem {
        attributes: SFGAO_FILESYSTEM,
        fail_attributes: false,
        name: OsString::from("relative.txt"),
        display_name_queries: Arc::new(AtomicUsize::new(0)),
    }
    .into();
    let items: IShellItemArray = FakeArray {
        items: vec![item],
        fail_count: false,
        reported_count: None,
    }
    .into();
    assert_eq!(
        unsafe { command.Invoke(&items, None).unwrap_err().code() },
        E_INVALIDARG
    );
}
