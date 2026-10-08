use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

pub(super) async fn replace_file(temp_path: &Path, path: &Path) -> io::Result<()> {
    replace_windows_file_with(temp_path, path, replace_windows_file)
}

fn replace_windows_file_with(
    temp_path: &Path,
    path: &Path,
    replace: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    // Read immediately before publication so a permission edit made while
    // staging is preserved along with the original file's other metadata.
    let permissions = WindowsDacl::read(path)?;
    replace(temp_path, path)?;
    if let Some(permissions) = permissions {
        // ReplaceFileW can convert legacy inherited ACEs into explicit entries
        // and append inherited copies. Apply the original descriptor after its
        // metadata merge to preserve both the rules and their inheritance.
        permissions.apply(path)?;
    }
    Ok(())
}

fn replace_windows_file(temp_path: &Path, path: &Path) -> io::Result<()> {
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    };

    let from = wide_null(temp_path.as_os_str());
    let to = wide_null(path.as_os_str());
    // Merge the original DACL, named streams, encryption, compression, and
    // creation time. Do not ignore merge failures or fall back to an overwrite.
    let result = unsafe {
        ReplaceFileW(
            to.as_ptr(),
            from.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_FILE_NOT_FOUND as i32) {
        return Err(error);
    }
    // A new destination has no metadata to preserve. Without REPLACE_EXISTING,
    // a file created by somebody else in the meantime cannot be overwritten.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn copy_permissions(path: &Path, temp_path: &Path) -> io::Result<()> {
    if let Some(permissions) = WindowsDacl::read(path)? {
        // Restrict the staged file before writing any source contents.
        permissions.apply(temp_path)?;
    }
    Ok(())
}

struct WindowsDacl(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

impl WindowsDacl {
    fn read(path: &Path) -> io::Result<Option<Self>> {
        use std::ptr;
        use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

        let mut descriptor = ptr::null_mut();
        let original = wide_null(path.as_os_str());
        let error = unsafe {
            GetNamedSecurityInfoW(
                original.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if matches!(error, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
            return Ok(None);
        }
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(Some(Self(descriptor)))
    }

    fn apply(&self, path: &Path) -> io::Result<()> {
        use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, SetFileSecurityW};

        // Apply the DACL descriptor without forcing protection or recomputing
        // inherited entries from the parent directory.
        let name = wide_null(path.as_os_str());
        if unsafe { SetFileSecurityW(name.as_ptr(), DACL_SECURITY_INFORMATION, self.0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for WindowsDacl {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::atomic_write::tests::{TestDirectory, run};
    use crate::services::atomic_write::{write, write_with_check};
    use std::path::PathBuf;

    #[derive(Debug, PartialEq, Eq)]
    struct WindowsPermissions {
        dacl_present: bool,
        dacl_protected: bool,
        entries: Option<Vec<Vec<u8>>>,
    }

    fn windows_permissions(path: &Path) -> WindowsPermissions {
        use std::ptr;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            ACE_HEADER, DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorControl,
            SE_DACL_PRESENT, SE_DACL_PROTECTED,
        };

        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        assert_eq!(
            unsafe {
                GetNamedSecurityInfoW(
                    wide_null(path.as_os_str()).as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut dacl,
                    ptr::null_mut(),
                    &mut descriptor,
                )
            },
            0
        );
        let mut control = 0;
        let mut revision = 0;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0
        );
        // Compare the access rules themselves, including their order, masks,
        // SIDs and inherited flags. A null DACL differs from an empty DACL.
        // ReplaceFileW may update automatic-inheritance bookkeeping without
        // changing any of these permissions.
        let entries = (!dacl.is_null()).then(|| {
            (0..unsafe { (*dacl).AceCount })
                .map(|index| {
                    let mut ace = ptr::null_mut();
                    assert_ne!(unsafe { GetAce(dacl, u32::from(index), &mut ace) }, 0);
                    let size = unsafe { (*ace.cast::<ACE_HEADER>()).AceSize };
                    unsafe { std::slice::from_raw_parts(ace.cast::<u8>(), usize::from(size)) }
                        .to_vec()
                })
                .collect()
        });
        let result = WindowsPermissions {
            dacl_present: control & SE_DACL_PRESENT != 0,
            dacl_protected: control & SE_DACL_PROTECTED != 0,
            entries,
        };
        unsafe {
            LocalFree(descriptor);
        }
        result
    }

    async fn write_and_check_windows_permissions(path: &Path, contents: &[u8]) {
        let expected = windows_permissions(path);
        write_with_check(path, contents, |_| async {
            let temporary = std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|candidate| candidate != path)
                .expect("Staged replacement");
            assert_eq!(
                windows_permissions(&temporary),
                expected,
                "Staged permissions"
            );
            Ok::<(), io::Error>(())
        })
        .await
        .unwrap();
        assert_eq!(windows_permissions(path), expected, "Published permissions");
        assert_eq!(std::fs::read(path).unwrap(), contents);
    }

    fn set_windows_dacl(path: &Path, sddl: &str) {
        use std::ptr;
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;

        let mut descriptor = ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide_null(OsStr::new(sddl)).as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            },
            0
        );
        WindowsDacl(descriptor).apply(path).unwrap();
    }

    #[test]
    fn windows_replacement_restores_inherited_entries_after_metadata_merge() {
        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        let temporary = directory.0.join("replacement.tmp");
        std::fs::write(&path, b"original").unwrap();
        std::fs::write(&temporary, b"updated").unwrap();
        set_windows_dacl(&path, "D:(A;ID;FA;;;OW)");
        let expected = windows_permissions(&path);
        assert!(!expected.dacl_protected);
        assert_eq!(expected.entries.as_ref().unwrap().len(), 1);
        copy_permissions(&path, &temporary).unwrap();
        assert_eq!(windows_permissions(&temporary), expected);

        replace_windows_file_with(&temporary, &path, |temporary, path| {
            replace_windows_file(temporary, path)?;
            // Reproduce the hosted runner's result on Windows builds whose
            // ReplaceFileW does not duplicate these inherited entries itself.
            set_windows_dacl(path, "D:(A;;FA;;;OW)(A;ID;FA;;;OW)");
            let merged = windows_permissions(path);
            assert_eq!(merged.entries.as_ref().unwrap().len(), 2);
            assert_ne!(merged, expected);
            Ok(())
        })
        .unwrap();

        assert_eq!(
            windows_permissions(&path),
            expected,
            "Published permissions"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"updated");
        assert!(!temporary.exists());
    }

    #[test]
    fn windows_replacement_preserves_permissions_changed_while_staging() {
        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        std::fs::write(&path, b"original").unwrap();
        run(async {
            write_with_check(&path, b"updated", |_| async {
                set_windows_dacl(&path, "D:P(A;;FA;;;OW)");
                Ok::<(), io::Error>(())
            })
            .await
            .unwrap();
        });
        let published = windows_permissions(&path);
        assert!(published.dacl_protected);
        assert_eq!(published.entries.as_ref().unwrap().len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"updated");
    }

    #[test]
    fn existing_windows_permissions_survive_save() {
        use std::ptr;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT,
            SetNamedSecurityInfoW,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        };

        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        std::fs::write(&path, b"original").unwrap();
        run(async {
            write_and_check_windows_permissions(&path, b"updated").await;

            let sddl = wide_null(OsStr::new("D:P(A;;FA;;;OW)"));
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        sddl.as_ptr(),
                        1,
                        &mut descriptor,
                        ptr::null_mut(),
                    )
                },
                0
            );
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = ptr::null_mut();
            assert_ne!(
                unsafe {
                    GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
                },
                0
            );
            assert_eq!(
                unsafe {
                    SetNamedSecurityInfoW(
                        wide_null(path.as_os_str()).as_ptr(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        dacl,
                        ptr::null(),
                    )
                },
                0
            );
            unsafe {
                LocalFree(descriptor);
            }

            write_and_check_windows_permissions(&path, b"private update").await;
        });
    }

    #[test]
    fn existing_windows_legacy_inherited_permissions_survive_save() {
        use std::ptr;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, SE_DACL_AUTO_INHERIT_REQ, SE_DACL_AUTO_INHERITED,
            SetFileSecurityW, SetSecurityDescriptorControl,
        };

        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        std::fs::write(&path, b"original").unwrap();
        let name = wide_null(path.as_os_str());
        let mut descriptor = ptr::null_mut();
        assert_eq!(
            unsafe {
                GetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut descriptor,
                )
            },
            0
        );
        assert_ne!(
            unsafe {
                SetSecurityDescriptorControl(
                    descriptor,
                    SE_DACL_AUTO_INHERITED | SE_DACL_AUTO_INHERIT_REQ,
                    0,
                )
            },
            0
        );
        // Hosted runners can expose inherited ACEs without the modern automatic
        // inheritance control bit. Use the legacy API to recreate that state.
        assert_ne!(
            unsafe { SetFileSecurityW(name.as_ptr(), DACL_SECURITY_INFORMATION, descriptor) },
            0
        );
        unsafe {
            LocalFree(descriptor);
        }
        run(async {
            write_and_check_windows_permissions(&path, b"updated").await;
        });
    }

    #[test]
    fn existing_windows_streams_and_creation_time_survive_save() {
        use std::os::windows::fs::MetadataExt;

        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        let stream = PathBuf::from(format!("{}:metadata", path.display()));
        std::fs::write(&path, b"original").unwrap();
        std::fs::write(&stream, b"application metadata").unwrap();
        let created = std::fs::metadata(&path).unwrap().creation_time();
        run(async {
            write(&path, b"updated").await.unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"updated");
            assert_eq!(std::fs::read(&stream).unwrap(), b"application metadata");
            assert_eq!(std::fs::metadata(&path).unwrap().creation_time(), created);
        });
    }
}
