//! Optional Windows Explorer command DLL. Registration requires explicit invocation.
//! This component is deliberately independent of the editor and its build.

#[cfg(windows)]
mod launch;
#[cfg(windows)]
mod modern;
#[cfg(windows)]
mod os_version;
#[cfg(windows)]
mod registration;
#[cfg(windows)]
mod windows;
