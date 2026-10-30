use crate::Point;

use super::Button;

/// A mouse event.
///
/// _**Note:** This type is largely incomplete! If you need to track
/// additional events, feel free to [open an issue] and share your use case!_
///
/// [open an issue]: https://github.com/iced-rs/iced/issues
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The mouse cursor entered the window.
    CursorEntered,

    /// The mouse cursor left the window.
    CursorLeft,

    /// The mouse cursor was moved
    CursorMoved {
        /// The new position of the mouse cursor
        position: Point,
    },

    /// A mouse button was pressed.
    ButtonPressed(Button),

    /// A mouse button was released.
    ButtonReleased(Button),

    /// The mouse wheel was scrolled.
    WheelScrolled {
        /// The scroll movement.
        delta: ScrollDelta,

        /// The current stage of the gesture, when supplied by the platform.
        phase: ScrollPhase,
    },

    /// A two-finger pinch changed magnification.
    Pinched {
        /// The relative scale change: `0.1` magnifies by a factor of `1.1`.
        delta: f32,
        /// The current stage of the gesture.
        phase: ScrollPhase,
    },
}

/// The current stage of a wheel or touchpad gesture.
///
/// Platforms without gesture boundaries report only [`ScrollPhase::Moved`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPhase {
    /// The fingers started a gesture.
    Started,
    /// The wheel or fingers moved.
    #[default]
    Moved,
    /// Fingers left the touchpad; native momentum may follow.
    ContactEnded,
    /// The platform supplied momentum after the fingers left the touchpad.
    Momentum,
    /// The gesture ended, including any platform inertia.
    Ended,
    /// The platform cancelled the scrolling gesture.
    Cancelled,
}

/// A scroll movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// A line-based scroll movement
    Lines {
        /// The number of horizontal lines scrolled
        x: f32,

        /// The number of vertical lines scrolled
        y: f32,
    },
    /// A scroll movement measured in logical pixels.
    Pixels {
        /// The number of horizontal pixels scrolled
        x: f32,
        /// The number of vertical pixels scrolled
        y: f32,
    },
}
