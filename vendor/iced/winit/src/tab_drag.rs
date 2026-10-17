//! Tab gestures live on the existing GUI event loop, retaining original capture.
use crate::core::window::Id;
use crate::core::{self, Point, Vector};
use crate::runtime::window::tab_drag::{Action, CancelReason, Capability, Event, SessionId};
use std::sync::Arc;
use winit::dpi::{LogicalPosition, PhysicalPosition};

#[derive(Default)]
pub struct Tracker {
    active: Option<Session>,
    // Remember the real press. A delayed Begin must not revive a released drag
    // on a surface-only backend where a global physical-button query is absent.
    pressed: Option<Id>,
    press_position: Option<(Id, Point)>,
    cursor: Option<(Id, Point)>,
}

struct Session {
    id: SessionId,
    origin: Id,
    source: Arc<winit::window::Window>,
    capture: (Id, Arc<winit::window::Window>),
    capability: Capability,
    following: Option<(Id, Arc<winit::window::Window>, Vector)>,
    last_motion: Option<Event>,
    start_position: Point,
    surface_started: bool,
}

pub type WindowInfo = (Id, Arc<winit::window::Window>, f32);

impl Tracker {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn action(
        &mut self,
        action: Action,
        windows: &[WindowInfo],
        events: &mut Vec<(Id, core::Event)>,
    ) {
        match action {
            Action::Begin(origin, id) => {
                self.stop(CancelReason::Requested, events);
                let Some((_, source, _)) = windows.iter().find(|(window, _, _)| *window == origin)
                else {
                    emit(
                        origin,
                        Event::Cancelled {
                            session: id,
                            reason: CancelReason::WindowClosed,
                        },
                        events,
                    );
                    return;
                };
                let pointer = winit::tab_drag::pointer(source);
                if pointer.is_some_and(|pointer| !pointer.pressed)
                    || (pointer.is_none() && self.pressed != Some(origin))
                {
                    emit(origin, Event::Released { session: id }, events);
                    return;
                }
                let capability = if winit::tab_drag::begin(source) {
                    Capability::Desktop
                } else if winit::tab_drag::surface_capability(source) == Some(true) {
                    Capability::Compositor
                } else {
                    Capability::SurfaceOnly
                };
                let start_position = self
                    .press_position
                    .filter(|(id, _)| *id == origin)
                    .or_else(|| self.cursor.filter(|(id, _)| *id == origin))
                    .map(|(_, position)| position)
                    .unwrap_or(Point::ORIGIN);
                let current_position = self
                    .cursor
                    .filter(|(id, _)| *id == origin)
                    .map(|(_, position)| position);
                let surface_started = capability != Capability::Desktop
                    && current_position
                        .is_some_and(|position| position.distance(start_position) >= 10.0)
                    && winit::tab_drag::begin_surface(source, id.0).is_some();
                self.active = Some(Session {
                    id,
                    origin,
                    source: source.clone(),
                    capture: (origin, source.clone()),
                    capability,
                    following: None,
                    last_motion: None,
                    start_position,
                    surface_started,
                });
                emit(
                    origin,
                    Event::Started {
                        session: id,
                        window: origin,
                        capability,
                    },
                    events,
                );
                if capability != Capability::Desktop {
                    if let Some(position) = current_position {
                        emit(
                            origin,
                            Event::Motion {
                                session: id,
                                window: origin,
                                position,
                                desktop: None,
                                hovered: Some((origin, position)),
                            },
                            events,
                        );
                    }
                }
                self.poll(windows, events);
            }
            Action::Follow(id, target, offset) => {
                if self.active.as_ref().is_none_or(|session| session.id != id) {
                    return;
                }
                // Sampling release before considering a newly created window is
                // essential: its open task may resolve after the user lets go.
                self.poll(windows, events);
                if let Some(session) = self.active.as_mut().filter(|session| session.id == id) {
                    if let Some((_, window, _)) =
                        windows.iter().find(|(window, _, _)| *window == target)
                    {
                        if session.capability == Capability::Compositor {
                            let _ = winit::tab_drag::follow_surface(
                                window,
                                id.0,
                                LogicalPosition::new(f64::from(offset.x), f64::from(offset.y)),
                            );
                        }
                        if session.capability != Capability::SurfaceOnly {
                            session.following = Some((target, window.clone(), offset));
                            session.last_motion = None;
                        }
                    }
                }
                self.poll(windows, events);
            }
            Action::Attach(id, target) => {
                let mut handoff_failed = false;
                if let Some(session) = self.active.as_mut().filter(|session| session.id == id) {
                    if session.capability == Capability::Desktop && session.capture.0 != target {
                        if let Some((_, raw, _)) = windows.iter().find(|(window, _, _)| *window == target) {
                            // Transfer before the application hides the old
                            // followed window. No release/press is synthesized.
                            if winit::tab_drag::begin(raw) {
                                session.capture = (target, raw.clone());
                            } else { handoff_failed = true; }
                        } else { handoff_failed = true; }
                    }
                    session.following = None;
                    session.last_motion = None;
                }
                if handoff_failed {
                    self.poll(windows, events);
                    self.stop(CancelReason::NativeFailure, events);
                }
            }
            Action::Cancel(id) => {
                if self.active.as_ref().is_some_and(|session| session.id == id) {
                    self.stop(CancelReason::Requested, events);
                }
            }
        }
    }

    pub fn window_event(
        &mut self,
        id: Id,
        event: &winit::event::WindowEvent,
        scale: f32,
        events: &mut Vec<(Id, core::Event)>,
    ) {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        match event {
            WindowEvent::TabDrag(native) => {
                let Some(session) = self.active.as_ref() else {
                    return;
                };
                let token = session.id;
                match native {
                    winit::tab_drag::Event::Motion { session, position } if *session == token.0 => {
                        let position =
                            Point::new(position.x as f32 / scale, position.y as f32 / scale);
                        self.cursor = Some((id, position));
                        emit(
                            self.active.as_ref().unwrap().origin,
                            Event::Motion {
                                session: token,
                                window: id,
                                position,
                                desktop: None,
                                hovered: Some((id, position)),
                            },
                            events,
                        );
                    }
                    winit::tab_drag::Event::Left { session } if *session == token.0 => {
                        let (window, position) = self.cursor.unwrap_or((id, Point::ORIGIN));
                        emit(
                            self.active.as_ref().unwrap().origin,
                            Event::Motion {
                                session: token,
                                window,
                                position,
                                desktop: None,
                                hovered: None,
                            },
                            events,
                        );
                    }
                    winit::tab_drag::Event::Released { session } if *session == token.0 => {
                        self.release(events)
                    }
                    winit::tab_drag::Event::Cancelled { session } if *session == token.0 => {
                        self.stop(CancelReason::Requested, events)
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => {
                match state {
                    ElementState::Pressed => {
                        self.pressed = Some(id);
                        self.press_position = self.cursor.filter(|(window, _)| *window == id);
                    }
                    ElementState::Released => {
                        self.pressed = None;
                        self.press_position = None;
                        // On desktop backends trust the physical state instead
                        // of synthesized releases from toolkit/native handoff.
                        if self.active.as_ref().is_some_and(|session| {
                            session.capability != Capability::Desktop && !session.surface_started
                        }) {
                            self.release(events);
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((
                    id,
                    Point::new(position.x as f32 / scale, position.y as f32 / scale),
                ));
                if let Some(session) = self
                    .active
                    .as_mut()
                    .filter(|session| session.capability != Capability::Desktop)
                {
                    let position = self.cursor.unwrap().1;
                    if !session.surface_started
                        && id == session.origin
                        && position.distance(session.start_position) >= 10.0
                    {
                        session.surface_started =
                            winit::tab_drag::begin_surface(&session.source, session.id.0).is_some();
                    }
                    emit(
                        session.origin,
                        Event::Motion {
                            session: session.id,
                            window: id,
                            position,
                            desktop: None,
                            hovered: Some((id, position)),
                        },
                        events,
                    );
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key
                        == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) =>
            {
                self.stop(CancelReason::Requested, events)
            }
            WindowEvent::Destroyed => self.window_closed(id, events),
            _ => {}
        }
    }

    pub fn native_failure(&mut self, id: SessionId, events: &mut Vec<(Id, core::Event)>) {
        if self.active.as_ref().is_some_and(|session| session.id == id) {
            self.stop(CancelReason::NativeFailure, events);
        }
    }

    pub fn window_closed(&mut self, id: Id, events: &mut Vec<(Id, core::Event)>) {
        if self.pressed == Some(id) {
            self.pressed = None;
            self.press_position = None;
        }
        if self.active.as_ref().is_some_and(|session| {
            session.origin == id || session.capture.0 == id
                || session
                    .following
                    .as_ref()
                    .is_some_and(|(target, _, _)| *target == id)
        }) {
            self.stop(CancelReason::WindowClosed, events);
        }
    }

    pub fn poll(&mut self, windows: &[WindowInfo], events: &mut Vec<(Id, core::Event)>) {
        let Some(session) = self.active.as_mut() else {
            return;
        };
        if session.capability != Capability::Desktop {
            return;
        }
        let Some(pointer) = winit::tab_drag::pointer(&session.capture.1) else {
            self.stop(CancelReason::NativeFailure, events);
            return;
        };
        match pointer_terminal(pointer) {
            Some(Terminal::Released) => {
                self.release(events);
                return;
            }
            Some(Terminal::CaptureLost) => {
                self.stop(CancelReason::CaptureLost, events);
                return;
            }
            None => {}
        }
        let ignore = session
            .following
            .as_ref()
            .map(|(_, window, _)| window.as_ref());
        let hovered = windows.iter().find_map(|(id, raw, scale)| {
            if !winit::tab_drag::is_pointer_target(raw, ignore) {
                return None;
            }
            let local = winit::tab_drag::client_position(raw, pointer.position)?;
            let position = Point::new(local.x as f32 / scale, local.y as f32 / scale);
            let size = raw.inner_size();
            (local.x >= 0.0
                && local.y >= 0.0
                && local.x < f64::from(size.width)
                && local.y < f64::from(size.height))
            .then_some((*id, position))
        });
        let scale = windows
            .iter()
            .find(|(id, _, _)| *id == session.origin)
            .map(|(_, _, scale)| *scale)
            .unwrap_or(1.0);
        let local = winit::tab_drag::client_position(&session.source, pointer.position)
            .unwrap_or(PhysicalPosition::new(0.0, 0.0));
        let motion = Event::Motion {
            session: session.id,
            window: session.origin,
            position: Point::new(local.x as f32 / scale, local.y as f32 / scale),
            desktop: Some(Point::new(
                pointer.position.x as f32,
                pointer.position.y as f32,
            )),
            hovered,
        };
        if session.last_motion.as_ref() != Some(&motion) {
            emit(session.origin, motion.clone(), events);
            session.last_motion = Some(motion);
        }
        if let Some((_, following, offset)) = &session.following {
            winit::tab_drag::move_window(
                following,
                pointer.position,
                LogicalPosition::new(f64::from(offset.x), f64::from(offset.y)),
            );
        }
    }

    fn release(&mut self, events: &mut Vec<(Id, core::Event)>) {
        if let Some(session) = self.active.take() {
            winit::tab_drag::end(&session.capture.1);
            emit(
                session.origin,
                Event::Released {
                    session: session.id,
                },
                events,
            );
        }
    }

    fn stop(&mut self, reason: CancelReason, events: &mut Vec<(Id, core::Event)>) {
        if let Some(session) = self.active.take() {
            winit::tab_drag::end(&session.capture.1);
            winit::tab_drag::cancel_surface(&session.source, session.id.0);
            emit(
                session.origin,
                Event::Cancelled {
                    session: session.id,
                    reason,
                },
                events,
            );
        }
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        if let Some(session) = self.active.take() {
            winit::tab_drag::end(&session.capture.1);
            winit::tab_drag::cancel_surface(&session.source, session.id.0);
        }
    }
}

fn emit(window: Id, event: Event, events: &mut Vec<(Id, core::Event)>) {
    events.push((
        window,
        core::Event::Window(core::window::Event::TabDrag(event)),
    ));
}

#[derive(Debug, PartialEq)]
enum Terminal {
    Released,
    CaptureLost,
}

fn pointer_terminal(pointer: winit::tab_drag::Pointer) -> Option<Terminal> {
    // Native release normally drops capture before winit delivers its input
    // event. Give actual button release priority over capture loss.
    if !pointer.pressed {
        Some(Terminal::Released)
    } else if pointer.capture_lost {
        Some(Terminal::CaptureLost)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_release_commits_even_when_native_release_also_drops_capture() {
        let position = PhysicalPosition::new(-1200.0, 75.0);
        assert_eq!(
            pointer_terminal(winit::tab_drag::Pointer {
                position,
                pressed: false,
                capture_lost: true
            }),
            Some(Terminal::Released)
        );
        assert_eq!(
            pointer_terminal(winit::tab_drag::Pointer {
                position,
                pressed: false,
                capture_lost: false
            }),
            Some(Terminal::Released)
        );
    }

    #[test]
    fn external_capture_loss_cancels_a_still_held_gesture() {
        let position = PhysicalPosition::new(0.0, 0.0);
        assert_eq!(
            pointer_terminal(winit::tab_drag::Pointer {
                position,
                pressed: true,
                capture_lost: true
            }),
            Some(Terminal::CaptureLost)
        );
        assert_eq!(
            pointer_terminal(winit::tab_drag::Pointer {
                position,
                pressed: true,
                capture_lost: false
            }),
            None
        );
    }

    #[test]
    fn delayed_follow_attach_and_cancel_after_release_are_ignored() {
        let mut tracker = Tracker::default();
        let mut events = Vec::new();
        let window = Id::unique();
        tracker.action(
            Action::Follow(SessionId(41), window, Vector::new(20.0, 10.0)),
            &[],
            &mut events,
        );
        tracker.action(Action::Attach(SessionId(41), window), &[], &mut events);
        tracker.action(Action::Cancel(SessionId(41)), &[], &mut events);
        assert!(!tracker.is_active());
        assert!(events.is_empty());
    }

    #[test]
    fn closing_the_original_pressed_window_drops_the_remembered_press() {
        let mut tracker = Tracker::default();
        let mut events = Vec::new();
        let source = Id::unique();
        tracker.pressed = Some(source);
        tracker.window_closed(source, &mut events);
        assert_eq!(tracker.pressed, None);
        assert!(events.is_empty());
    }
}
