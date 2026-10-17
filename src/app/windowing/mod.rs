//! Application window modules.

pub(super) mod editors;
pub(super) mod managed;
pub(super) mod operations;
pub(super) mod platform_activation;
#[cfg(test)]
mod session_tests;
pub(super) mod tabs;
pub(super) mod title;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod close_tests;
