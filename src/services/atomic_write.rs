use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

#[cfg(target_os = "linux")]
#[path = "atomic_write/linux.rs"]
mod linux;

#[cfg(target_os = "macos")]
#[path = "atomic_write/macos.rs"]
mod macos;

#[cfg(windows)]
#[path = "atomic_write/windows.rs"]
mod windows;

#[cfg(windows)]
use windows::replace_file;

pub async fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_with_check(path, contents, |_| std::future::ready(Ok(()))).await
}

/// The check must be repeatable: metadata-preserving saves check both before
/// copying metadata and after the replacement has finished syncing.
pub async fn write_with_check<E, F>(
    path: &Path,
    contents: &[u8],
    before_replace: impl FnMut(PathBuf) -> F,
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
    mut before_replace: impl FnMut(PathBuf) -> F,
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
    #[cfg(target_os = "macos")]
    let mut file = if _private || permissions.is_some() {
        // Exclude inherited ACL grants at creation, so nobody can open the
        // empty replacement and retain access when contents are written.
        macos::create_restricted_staged_file(&temp_path).await?
    } else {
        options.open(&temp_path).await?
    };
    #[cfg(not(target_os = "macos"))]
    let mut file = options.open(&temp_path).await?;
    let write_result = async {
        #[cfg(windows)]
        windows::copy_permissions(path, &temp_path)?;
        file.write_all(contents).await?;
        #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
        if let Some(permissions) = permissions {
            // Apply after writing (which can clear mode bits), before syncing
            // and publishing the replacement. New files retain the usual umask.
            file.set_permissions(permissions).await?;
        }
        file.sync_all().await?;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        drop(file);

        // Refuse stale contents before starting a potentially costly metadata copy.
        before_replace(path.to_owned()).await?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if !_private {
                // Read the current metadata after the conflict check. Refuse
                // publication if ownership, ACLs or attributes cannot be kept.
                #[cfg(target_os = "linux")]
                linux::copy_metadata(path, &file).await?;
                #[cfg(target_os = "macos")]
                macos::copy_metadata(path, &file).await?;
                file.sync_all().await?;
            }
            drop(file);
            if !_private {
                // Resource forks and attributes can take time to copy. Check
                // again after that work so concurrent edits are not lost.
                before_replace(path.to_owned()).await?;
            }
        }
        replace_file(&temp_path, path).await?;
        Ok(())
    }
    .await;

    if write_result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }

    write_result
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

    pub(super) struct TestDirectory(pub(super) PathBuf);

    impl TestDirectory {
        pub(super) fn new() -> Self {
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

    pub(super) fn run(test: impl Future<Output = ()>) {
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
