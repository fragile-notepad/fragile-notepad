//! Events for a continuous tab tear-off gesture.
use super::Id;
use crate::Point;

/// Application-allocated token. Never reuse a token while callbacks may remain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// Native drag support for the current display connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Unconfined pointer tracking and cancellable window positioning.
    Desktop,
    /// The compositor supports data-device dragging and moving detached toplevels.
    /// Pointer positions remain surface-local.
    Compositor,
    /// Only surface-local input is available. Offer Move to New Window as a
    /// fallback when the compositor cannot provide a toplevel drag protocol.
    SurfaceOnly,
}

/// Why a gesture was rolled back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// Escape or an application cancellation.
    Requested,
    /// Another application took pointer capture.
    CaptureLost,
    /// The originating native window was destroyed.
    WindowClosed,
    /// Native pointer tracking could not be started.
    NativeFailure,
}

/// Session-tagged events. Focus changes and attachment do not end a gesture.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Native tracking has started.
    Started {
        /// Gesture identity.
        session: SessionId,
        /// Original pressed window.
        window: Id,
        /// Native support available on this display connection.
        capability: Capability,
    },
    /// Position of the pointer. `position` is logical client coordinates of
    /// `window`; `hovered` identifies the native topmost app window and its local
    /// logical coordinates. `desktop` is physical desktop pixels on Windows/X11
    /// and desktop points on macOS. It is unavailable on Wayland.
    Motion {
        /// Gesture identity.
        session: SessionId,
        /// Window whose client coordinates describe `position`.
        window: Id,
        /// Logical client position, including coordinates outside the window.
        position: Point,
        /// Native desktop position, when supported.
        desktop: Option<Point>,
        /// Topmost application window under the pointer and its client position.
        hovered: Option<(Id, Point)>,
    },
    /// The physical primary button was released.
    Released {
        /// Gesture identity.
        session: SessionId,
    },
    /// The gesture must be rolled back.
    Cancelled {
        /// Gesture identity.
        session: SessionId,
        /// Cancellation cause.
        reason: CancelReason,
    },
}
