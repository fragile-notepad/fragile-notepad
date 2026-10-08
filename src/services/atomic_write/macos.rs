use std::ffi::{CStr, CString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::ptr::NonNull;

pub(super) async fn restrict_staged_file(destination: &tokio::fs::File) -> io::Result<()> {
    let destination = destination.try_clone().await?.into_std().await;
    tokio::task::spawn_blocking(move || {
        let security = FileSecurity::new()?;
        security.remove_acl()?;
        security.apply(destination.as_raw_fd())
    })
    .await
    .map_err(io::Error::other)?
}

pub(super) async fn copy_metadata(path: &Path, destination: &tokio::fs::File) -> io::Result<()> {
    let path = path.to_owned();
    let destination = destination.try_clone().await?.into_std().await;
    tokio::task::spawn_blocking(move || {
        let source = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let security = FileSecurity::read(&source)?;
        let names = list_attributes(source.as_raw_fd())?;
        for name in list_attributes(destination.as_raw_fd())? {
            if !names.contains(&name)
                && unsafe { libc::fremovexattr(destination.as_raw_fd(), name.as_ptr(), 0) } != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        for name in names {
            copy_attribute(source.as_raw_fd(), destination.as_raw_fd(), &name)?;
        }
        // Apply the POSIX owner/group/mode and the exact ACL together, after
        // copying attributes, while the staged file is still private. Unlike
        // fcopyfile, this does not merge ACLs or ignore permission-copy errors.
        security.apply(destination.as_raw_fd())
    })
    .await
    .map_err(io::Error::other)?
}

struct FileSecurity {
    raw: NonNull<libc::c_void>,
    has_acl: bool,
}

impl FileSecurity {
    fn new() -> io::Result<Self> {
        NonNull::new(unsafe { native::filesec_init() })
            .map(|raw| Self {
                raw,
                has_acl: false,
            })
            .ok_or_else(io::Error::last_os_error)
    }

    fn read(source: &File) -> io::Result<Self> {
        let mut security = Self::new()?;
        let metadata = source.metadata()?;
        security.set(native::FILESEC_OWNER, &metadata.uid())?;
        security.set(native::FILESEC_GROUP, &metadata.gid())?;
        security.set(native::FILESEC_MODE, &(metadata.mode() as libc::mode_t))?;
        if let Some(acl) = Acl::read(source.as_raw_fd())? {
            security.set(native::FILESEC_ACL, &acl.0.as_ptr())?;
            security.has_acl = true;
        } else {
            security.remove_acl()?;
        }
        Ok(security)
    }

    fn set<T>(&self, property: libc::c_int, value: &T) -> io::Result<()> {
        // Each caller supplies the native type required by this property.
        // filesec_set_property copies the value (including the ACL contents).
        let result = unsafe {
            native::filesec_set_property(
                self.raw.as_ptr(),
                property,
                std::ptr::from_ref(value).cast(),
            )
        };
        check(result)
    }

    fn remove_acl(&self) -> io::Result<()> {
        // _FILESEC_REMOVE_ACL is the documented sentinel (void *)1, rather
        // than an allocated ACL or a pointer that the function dereferences.
        check(unsafe {
            native::filesec_set_property(
                self.raw.as_ptr(),
                native::FILESEC_ACL,
                std::ptr::without_provenance(1),
            )
        })
    }

    fn apply(&self, file: RawFd) -> io::Result<()> {
        let result = unsafe { native::fchmodx_np(file, self.raw.as_ptr()) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOTSUP) || self.has_acl {
            return Err(error);
        }
        // XNU rejects even ACL removal on volumes without extended security,
        // before applying mode/ownership. With no source ACL, retry only those
        // POSIX fields. A real ACL must never be discarded by this fallback.
        // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/kpi_vfs.c
        check(unsafe {
            native::filesec_set_property(self.raw.as_ptr(), native::FILESEC_ACL, std::ptr::null())
        })?;
        check(unsafe { native::fchmodx_np(file, self.raw.as_ptr()) })
    }
}

impl Drop for FileSecurity {
    fn drop(&mut self) {
        unsafe { native::filesec_free(self.raw.as_ptr()) };
    }
}

struct Acl(NonNull<libc::c_void>);

impl Acl {
    fn read(file: RawFd) -> io::Result<Option<Self>> {
        match NonNull::new(unsafe { native::acl_get_fd(file) }) {
            Some(acl) => Ok(Some(Self(acl))),
            None => {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ENOENT) {
                    Ok(None)
                } else {
                    Err(error)
                }
            }
        }
    }
}

impl Drop for Acl {
    fn drop(&mut self) {
        unsafe { native::acl_free(self.0.as_ptr()) };
    }
}

fn check(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn list_attributes(file: RawFd) -> io::Result<Vec<CString>> {
    // Normal xattr visibility excludes backing storage for compressed data.
    // Edited text must not inherit a stale compressed copy of its old contents.
    let names = match read_attribute_buffer(|buffer, length| unsafe {
        libc::flistxattr(file, buffer.cast(), length, 0)
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
        libc::fgetxattr(file, name.as_ptr(), buffer, length, 0, 0)
    })
}

fn set_attribute(file: RawFd, name: &CStr, value: &[u8], position: u32) -> io::Result<()> {
    check(unsafe {
        libc::fsetxattr(
            file,
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            position,
            0,
        )
    })
}

fn copy_attribute(source: RawFd, destination: RawFd, name: &CStr) -> io::Result<()> {
    if name != c"com.apple.ResourceFork" {
        return set_attribute(destination, name, &get_attribute(source, name)?, 0);
    }
    // Resource forks can be much larger than ordinary attributes. Copy them
    // with the documented position argument using a bounded buffer.
    let size = unsafe { libc::fgetxattr(source, name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    // A position-zero write overwrites/extends a fork without truncating it.
    // Clear any staged fork first so a shorter source cannot leave a stale tail.
    if unsafe { libc::fremovexattr(destination, name.as_ptr(), 0) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOATTR) {
            return Err(error);
        }
    }
    if size == 0 {
        return set_attribute(destination, name, &[], 0);
    }
    let mut buffer = vec![0; 64 * 1024];
    let mut position = 0_usize;
    while position < size as usize {
        let offset = u32::try_from(position).map_err(io::Error::other)?;
        let length = buffer.len().min(size as usize - position);
        let read = unsafe {
            libc::fgetxattr(
                source,
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                length,
                offset,
                0,
            )
        };
        if read < 0 {
            return Err(io::Error::last_os_error());
        }
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Resource fork changed while saving",
            ));
        }
        set_attribute(destination, name, &buffer[..read as usize], offset)?;
        position += read as usize;
    }
    let current_size =
        unsafe { libc::fgetxattr(source, name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
    if current_size < 0 {
        return Err(io::Error::last_os_error());
    }
    if current_size != size {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Resource fork changed while saving",
        ));
    }
    Ok(())
}

fn read_attribute_buffer(
    mut read: impl FnMut(*mut libc::c_void, usize) -> libc::ssize_t,
) -> io::Result<Vec<u8>> {
    for _ in 0..4 {
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

mod native {
    // Signatures and property values from Apple's public sys/acl.h and
    // sys/fcntl.h. libc does not currently expose these security APIs.
    // https://github.com/apple-oss-distributions/Libc/blob/main/include/sys/acl.h
    // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/fcntl.h
    pub(super) const FILESEC_OWNER: libc::c_int = 1;
    pub(super) const FILESEC_GROUP: libc::c_int = 2;
    pub(super) const FILESEC_MODE: libc::c_int = 4;
    pub(super) const FILESEC_ACL: libc::c_int = 5;

    unsafe extern "C" {
        pub(super) fn filesec_init() -> *mut libc::c_void;
        pub(super) fn filesec_free(security: *mut libc::c_void);
        pub(super) fn filesec_set_property(
            security: *mut libc::c_void,
            property: libc::c_int,
            value: *const libc::c_void,
        ) -> libc::c_int;
        pub(super) fn fchmodx_np(file: libc::c_int, security: *mut libc::c_void) -> libc::c_int;
        pub(super) fn acl_get_fd(file: libc::c_int) -> *mut libc::c_void;
        pub(super) fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
}
