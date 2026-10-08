use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

pub async fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_with_check(path, contents, |_| std::future::ready(Ok(()))).await
}

pub async fn write_with_check<E, F>(
    path: &Path,
    contents: &[u8],
    before_replace: impl FnOnce(PathBuf) -> F,
) -> Result<(), E>
where
    E: From<io::Error>,
    F: Future<Output = Result<(), E>>,
{
    // Reads follow symlinks; replace the same target without replacing the link.
    // A dangling link or a loop must fail rather than silently become a file.
    let target = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => tokio::fs::canonicalize(path).await?,
        Ok(_) => path.to_owned(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_owned(),
        Err(error) => return Err(error.into()),
    };
    write_with_permissions(&target, contents, false, before_replace).await
}

pub async fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_with_permissions(path, contents, true, |_| std::future::ready(Ok(()))).await
}

async fn write_with_permissions<E, F>(
    path: &Path,
    contents: &[u8],
    _private: bool,
    before_replace: impl FnOnce(PathBuf) -> F,
) -> Result<(), E>
where
    E: From<io::Error>,
    F: Future<Output = Result<(), E>>,
{
    #[cfg(unix)]
    let permissions = if _private {
        None
    } else {
        match tokio::fs::metadata(path).await {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        }
    };
    let temp_path = temp_path(path);

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if _private || permissions.is_some() {
        // Do not expose private contents while the replacement is being written.
        options.mode(0o600);
    }
    let mut file = options.open(&temp_path).await?;
    let write_result = async {
        #[cfg(windows)]
        copy_windows_permissions(path, &temp_path)?;
        file.write_all(contents).await?;
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            // Apply after writing (which can clear mode bits), before syncing
            // and publishing the replacement. New files retain the usual umask.
            file.set_permissions(permissions).await?;
        }
        file.sync_all().await?;
        drop(file);

        // Check after staging and syncing, immediately before publishing.
        before_replace(path.to_owned()).await?;
        replace_file(&temp_path, path).await?;
        Ok(())
    }
    .await;

    if write_result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }

    write_result
}

#[cfg(windows)]
async fn replace_file(temp_path: &Path, path: &Path) -> io::Result<()> {
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

#[cfg(windows)]
fn copy_windows_permissions(path: &Path, temp_path: &Path) -> io::Result<()> {
    use std::ptr;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    let mut dacl = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    let original = wide_null(path.as_os_str());
    let error = unsafe {
        GetNamedSecurityInfoW(
            original.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if matches!(error, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        return Ok(());
    }
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    // Restrict the empty temporary file before any original contents reach it.
    // Protect the copied DACL from the directory's potentially broader rules.
    let temporary = wide_null(temp_path.as_os_str());
    let error = unsafe {
        SetNamedSecurityInfoW(
            temporary.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    unsafe {
        LocalFree(descriptor);
    }
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    Ok(())
}

#[cfg(not(windows))]
async fn replace_file(temp_path: &Path, path: &Path) -> io::Result<()> {
    tokio::fs::rename(temp_path, path).await?;
    sync_parent_dir(path).await
}

#[cfg(not(windows))]
async fn sync_parent_dir(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let dir = tokio::fs::File::open(parent).await?;
    dir.sync_all().await
}

#[cfg(windows)]
fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn temp_path(path: &Path) -> PathBuf {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("fragile-notepad");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    parent.join(format!(
        ".{file_name}.{}.{}.{}.tmp",
        std::process::id(),
        unique,
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = temp_path(&std::env::temp_dir().join("fragile-atomic-write-test"));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn run(test: impl Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(test);
    }

    #[test]
    fn new_and_existing_files_save_without_temporary_files_left_over() {
        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        run(async {
            write(&path, b"new").await.unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"new");
            write(&path, b"updated").await.unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"updated");
            assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
        });
    }

    #[cfg(windows)]
    fn windows_dacl(path: &Path) -> String {
        use std::ptr;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
            SE_FILE_OBJECT,
        };
        use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

        let mut descriptor = ptr::null_mut();
        assert_eq!(
            unsafe {
                GetNamedSecurityInfoW(
                    wide_null(path.as_os_str()).as_ptr(),
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
        let mut text = ptr::null_mut();
        let mut length = 0;
        assert_ne!(
            unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    &mut length,
                )
            },
            0
        );
        let result = String::from_utf16_lossy(unsafe {
            std::slice::from_raw_parts(text, length.saturating_sub(1) as usize)
        });
        unsafe {
            LocalFree(text.cast());
            LocalFree(descriptor);
        }
        result
    }

    #[cfg(windows)]
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
            let inherited = windows_dacl(&path);
            write(&path, b"updated").await.unwrap();
            assert_eq!(windows_dacl(&path), inherited);

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

            let restricted = windows_dacl(&path);
            write(&path, b"private update").await.unwrap();
            assert_eq!(windows_dacl(&path), restricted);
            assert_eq!(std::fs::read(&path).unwrap(), b"private update");
        });
    }

    #[cfg(windows)]
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

    #[cfg(unix)]
    #[test]
    fn existing_unix_permissions_survive_save() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        run(async {
            for mode in [0o600, 0o640, 0o755] {
                std::fs::write(&path, b"original").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
                write(&path, b"updated").await.unwrap();
                assert_eq!(std::fs::read(&path).unwrap(), b"updated");
                assert_eq!(
                    std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    mode
                );
            }
            // Recovery/settings files always remain private, even if an older
            // destination was more permissive.
            write_private(&path, b"private").await.unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn saves_through_absolute_relative_and_chained_symlinks_update_target() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = TestDirectory::new();
        let target_dir = directory.0.join("targets");
        std::fs::create_dir(&target_dir).unwrap();
        let target = target_dir.join("document.txt");
        std::fs::write(&target, b"original").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let absolute = directory.0.join("absolute.txt");
        let relative = directory.0.join("relative.txt");
        let chained = directory.0.join("chained.txt");
        symlink(&target, &absolute).unwrap();
        symlink("targets/document.txt", &relative).unwrap();
        symlink("relative.txt", &chained).unwrap();
        run(async {
            for link in [&absolute, &relative, &chained] {
                let link_target = std::fs::read_link(link).unwrap();
                std::fs::write(&target, b"original").unwrap();
                write(link, b"updated").await.unwrap();
                assert_eq!(std::fs::read_link(link).unwrap(), link_target);
                assert_eq!(std::fs::read(&target).unwrap(), b"updated");
                assert_eq!(
                    std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            assert_eq!(std::fs::read_dir(&target_dir).unwrap().count(), 1);
            assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 4);
        });
    }

    #[cfg(unix)]
    #[test]
    fn unresolved_symlinks_fail_without_replacing_links() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new();
        let dangling = directory.0.join("dangling.txt");
        let cycle = directory.0.join("cycle.txt");
        symlink("missing.txt", &dangling).unwrap();
        symlink("cycle.txt", &cycle).unwrap();
        run(async {
            for link in [&dangling, &cycle] {
                let target = std::fs::read_link(link).unwrap();
                assert!(write(link, b"updated").await.is_err());
                assert_eq!(std::fs::read_link(link).unwrap(), target);
            }
            assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 2);
        });
    }
}
