//! Build window-based GUI applications.
pub mod icon;
pub mod screenshot;
pub mod settings;
pub mod tab_drag;

mod direction;
mod event;
mod id;
mod level;
mod mode;
mod position;
mod redraw_request;
mod user_attention;

pub use direction::Direction;
pub use event::Event;
pub use icon::Icon;
pub use id::Id;
pub use level::Level;
pub use mode::Mode;
pub use position::Position;
pub use redraw_request::RedrawRequest;
pub use screenshot::Screenshot;
pub use settings::Settings;
pub use user_attention::UserAttention;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static MAX_FPS: AtomicU32 = AtomicU32::new(120);
static VSYNC: AtomicBool = AtomicBool::new(true);

/// The target time between frames, shared by window animations.
pub fn frame_interval() -> crate::time::Duration {
    crate::time::Duration::from_nanos(1_000_000_000 / u64::from(MAX_FPS.load(Ordering::Relaxed)))
}

/// Changes the shared animation timing. The native runtime also updates surfaces.
#[doc(hidden)]
pub fn update_frame_settings(max_fps: u32, vsync: bool) {
    MAX_FPS.store(max_fps.clamp(1, 1000), Ordering::Relaxed);
    VSYNC.store(vsync, Ordering::Relaxed);
}

/// The synchronization preference from startup or the latest frame settings.
#[doc(hidden)]
pub fn vsync() -> bool {
    VSYNC.load(Ordering::Relaxed)
}

pub use raw_window_handle;

/// A window managed by iced.
///
/// It implements both [`HasWindowHandle`] and [`HasDisplayHandle`].
pub trait Window: HasWindowHandle + HasDisplayHandle + Debug {}

impl<T> Window for T where T: HasWindowHandle + HasDisplayHandle + Debug {}

/// A headless window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Headless;

impl HasWindowHandle for Headless {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::NotSupported)
    }
}

impl HasDisplayHandle for Headless {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::NotSupported)
    }
}
