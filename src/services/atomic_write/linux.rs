use std::ffi::{CStr, CString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(super) async fn copy_metadata(path: &Path, destination: &tokio::fs::File) -> io::Result<()> {
    let path = path.to_owned();
    let destination = destination.try_clone().await?.into_std().await;
    tokio::task::spawn_blocking(move || {
        let source = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        copy_file_metadata(&source, &destination)
    })
    .await
    .map_err(io::Error::other)?
}

fn copy_file_metadata(source: &File, destination: &File) -> io::Result<()> {
    let metadata = source.metadata()?;
    let attributes = list_attributes(source.as_raw_fd())?
        .into_iter()
        .map(|name| get_attribute(source.as_raw_fd(), &name).map(|value| (name, value)))
        .collect::<io::Result<Vec<_>>>()?;
    let destination_metadata = destination.metadata()?;
    if (metadata.uid(), metadata.gid()) != (destination_metadata.uid(), destination_metadata.gid())
    {
        // Both descriptors remain open throughout the copy. Change ownership
        // first because chown can clear set-ID bits and security attributes.
        if unsafe { libc::fchown(destination.as_raw_fd(), metadata.uid(), metadata.gid()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // A staged file may inherit an ACL or a security label from its directory.
    // Match the original inode instead of retaining extra inherited attributes.
    let destination_attributes = list_attributes(destination.as_raw_fd())?;
    for name in &destination_attributes {
        if !attributes.iter().any(|(original, _)| original == name)
            && unsafe { libc::fremovexattr(destination.as_raw_fd(), name.as_ptr()) } != 0
        {
            return Err(io::Error::last_os_error());
        }
    }

    // Install the original ACL before chmod widens the staged file's mask.
    // Otherwise inherited named entries could temporarily grant extra access.
    if let Some((name, value)) = attributes
        .iter()
        .find(|(name, _)| name.as_c_str() == c"system.posix_acl_access")
    {
        set_attribute(destination.as_raw_fd(), name, value)?;
    }
    destination.set_permissions(metadata.permissions())?;

    for (name, value) in attributes {
        if name.as_c_str() == c"system.posix_acl_access" {
            continue;
        }
        if destination_attributes.contains(&name)
            && get_attribute(destination.as_raw_fd(), &name)? == value
        {
            continue;
        }
        set_attribute(destination.as_raw_fd(), &name, &value)?;
    }
    Ok(())
}

fn list_attributes(file: RawFd) -> io::Result<Vec<CString>> {
    let names = match read_attribute_buffer(|buffer, length| unsafe {
        libc::flistxattr(file, buffer.cast(), length)
    }) {
        Ok(names) => names,
        Err(error) if error.raw_os_error() == Some(libc::ENOTSUP) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| CString::new(name).map_err(io::Error::other))
        .collect()
}

fn get_attribute(file: RawFd, name: &CStr) -> io::Result<Vec<u8>> {
    read_attribute_buffer(|buffer, length| unsafe {
        libc::fgetxattr(file, name.as_ptr(), buffer, length)
    })
}

fn set_attribute(file: RawFd, name: &CStr, value: &[u8]) -> io::Result<()> {
    // The name is NUL-terminated and the value is valid for its byte length,
    // including empty values and attributes with binary data.
    if unsafe { libc::fsetxattr(file, name.as_ptr(), value.as_ptr().cast(), value.len(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_attribute_buffer(
    mut read: impl FnMut(*mut libc::c_void, usize) -> libc::ssize_t,
) -> io::Result<Vec<u8>> {
    for _ in 0..4 {
        // A zero-length query returns the required size without dereferencing
        // the pointer. The second call receives a buffer of exactly that size.
        let size = read(std::ptr::null_mut(), 0);
        if size < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0; size as usize];
        if buffer.is_empty() {
            return Ok(buffer);
        }
        let size = read(buffer.as_mut_ptr().cast(), buffer.len());
        if size >= 0 {
            buffer.truncate(size as usize);
            return Ok(buffer);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ERANGE) {
            return Err(error);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "File metadata changed while saving",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::atomic_write::tests::{TestDirectory, run};
    use crate::services::atomic_write::{write, write_private, write_with_check};
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn access_acl() -> Vec<u8> {
        let mut acl = 2_u32.to_le_bytes().to_vec();
        // Give one other user read access, with an explicit ACL mask.
        for (tag, permissions, id) in [
            (1_u16, 6_u16, u32::MAX),
            (2, 4, unsafe { libc::geteuid() }.wrapping_add(1)),
            (4, 0, u32::MAX),
            (16, 4, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            acl.extend_from_slice(&tag.to_le_bytes());
            acl.extend_from_slice(&permissions.to_le_bytes());
            acl.extend_from_slice(&id.to_le_bytes());
        }
        acl
    }

    #[test]
    fn replacements_preserve_binary_empty_attributes_and_ownership() {
        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        std::fs::write(&path, b"original").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let source = File::open(&path).unwrap();
        let original = source.metadata().unwrap();
        let non_utf8_name = CString::new(b"user.\xff").unwrap();
        set_attribute(source.as_raw_fd(), c"user.binary", b"\0metadata\xff").unwrap();
        set_attribute(source.as_raw_fd(), c"user.empty", b"").unwrap();
        set_attribute(source.as_raw_fd(), &non_utf8_name, b"raw name").unwrap();
        run(async {
            for contents in [b"updated".as_slice(), b"updated again"] {
                write(&path, contents).await.unwrap();
                let saved = File::open(&path).unwrap();
                let metadata = saved.metadata().unwrap();
                assert_eq!(
                    (metadata.uid(), metadata.gid()),
                    (original.uid(), original.gid())
                );
                assert_eq!(metadata.mode(), original.mode());
                assert_eq!(
                    get_attribute(saved.as_raw_fd(), c"user.binary").unwrap(),
                    b"\0metadata\xff"
                );
                assert_eq!(
                    get_attribute(saved.as_raw_fd(), c"user.empty").unwrap(),
                    b""
                );
                assert_eq!(
                    get_attribute(saved.as_raw_fd(), &non_utf8_name).unwrap(),
                    b"raw name"
                );
                assert_eq!(std::fs::read(&path).unwrap(), contents);
                assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
            }
        });
    }

    #[test]
    fn symlink_saves_preserve_acl_and_attributes_changed_while_staging() {
        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        let link = directory.0.join("link.txt");
        std::fs::write(&path, b"original").unwrap();
        symlink("document.txt", &link).unwrap();
        let source = File::open(&path).unwrap();
        set_attribute(
            source.as_raw_fd(),
            c"system.posix_acl_access",
            &access_acl(),
        )
        .unwrap();
        let acl = get_attribute(source.as_raw_fd(), c"system.posix_acl_access").unwrap();
        let mode = source.metadata().unwrap().mode();
        run(async {
            write_with_check(&link, b"updated", |_| async {
                set_attribute(source.as_raw_fd(), c"user.changed", b"current metadata")
            })
            .await
            .unwrap();
        });
        let saved = File::open(&path).unwrap();
        assert_eq!(
            get_attribute(saved.as_raw_fd(), c"system.posix_acl_access").unwrap(),
            acl
        );
        assert_eq!(
            get_attribute(saved.as_raw_fd(), c"user.changed").unwrap(),
            b"current metadata"
        );
        assert_eq!(saved.metadata().unwrap().mode(), mode);
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            Path::new("document.txt")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"updated");
    }

    #[test]
    fn replacement_removes_inherited_acl_absent_from_original() {
        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        std::fs::write(&path, b"original").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let parent = File::open(&directory.0).unwrap();
        set_attribute(
            parent.as_raw_fd(),
            c"system.posix_acl_default",
            &access_acl(),
        )
        .unwrap();
        run(async { write(&path, b"updated").await.unwrap() });
        let saved = File::open(&path).unwrap();
        assert_eq!(saved.metadata().unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            get_attribute(saved.as_raw_fd(), c"system.posix_acl_access")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ENODATA)
        );
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn metadata_read_failure_keeps_original_and_removes_staged_file() {
        // Root bypasses the permission denial this regression exercises.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let directory = TestDirectory::new();
        let path = directory.0.join("document.txt");
        std::fs::write(&path, b"original").unwrap();
        let permissions = std::fs::metadata(&path).unwrap().permissions();
        run(async {
            let result = write_with_check(&path, b"updated", |_| async {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0))
            })
            .await;
            std::fs::set_permissions(&path, permissions).unwrap();
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        });
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn private_replacements_do_not_copy_original_access_rules() {
        let directory = TestDirectory::new();
        let path = directory.0.join("recovery.json");
        std::fs::write(&path, b"original").unwrap();
        let source = File::open(&path).unwrap();
        set_attribute(
            source.as_raw_fd(),
            c"system.posix_acl_access",
            &access_acl(),
        )
        .unwrap();
        run(async { write_private(&path, b"private").await.unwrap() });
        let saved = File::open(&path).unwrap();
        assert_eq!(saved.metadata().unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            get_attribute(saved.as_raw_fd(), c"system.posix_acl_access")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ENODATA)
        );
    }
}
