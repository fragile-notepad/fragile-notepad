//! Configure the window of your application in native platforms.

pub mod icon;

pub use icon::Icon;

pub use crate::core::window::*;
pub use crate::runtime::window::*;

// Resolve the core event-types module to the runtime gesture API.
pub use crate::runtime::window::tab_drag;
