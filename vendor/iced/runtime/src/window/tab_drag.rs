//! Continuous, cancellable tab tear-off gestures.
use crate::core::Vector;
use crate::core::window::Id;
pub use crate::core::window::tab_drag::{CancelReason, Capability, Event, SessionId};
use crate::futures::{Subscription, event};
use crate::task::{self, Task};

/// Native drag action interpreted by the window runtime.
#[derive(Debug)]
pub enum Action {
    /// Begin tracking from the original pressed window.
    Begin(Id, SessionId),
    /// Follow the pointer with a detached window. The offset is in logical
    /// pixels from the outer top-left of the detached window.
    Follow(SessionId, Id, Vector),
    /// Attach to a strip, handing capture to its visible window before hiding
    /// the old follower while retaining the original source surface.
    Attach(SessionId, Id),
    /// Roll the gesture back and release native tracking.
    Cancel(SessionId),
}

/// Starts native tracking. Keep the source window alive until release/cancel.
pub fn begin<T>(window: Id, session: SessionId) -> Task<T> {
    effect(Action::Begin(window, session))
}

/// Makes an existing detached window follow the pointer without starting a new
/// native gesture. Stale sessions and callbacks after release are ignored.
pub fn follow<T>(session: SessionId, window: Id, offset: Vector) -> Task<T> {
    effect(Action::Follow(session, window, offset))
}

/// Stops following a detached window while preserving the held gesture.
pub fn attach<T>(session: SessionId, window: Id) -> Task<T> {
    effect(Action::Attach(session, window))
}

/// Cancels tracking. A session-tagged cancellation event follows.
pub fn cancel<T>(session: SessionId) -> Task<T> {
    effect(Action::Cancel(session))
}

/// Listens for tab gestures regardless of widget event-capture status.
pub fn events() -> Subscription<Event> {
    event::listen_with(|event, _, _| match event {
        crate::core::Event::Window(crate::core::window::Event::TabDrag(event)) => Some(event),
        _ => None,
    })
}

fn effect<T>(action: Action) -> Task<T> {
    task::effect(crate::Action::Window(super::Action::TabDrag(action)))
}
