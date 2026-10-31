//! Runtime event sources and single-instance admission.

use super::{App, shortcuts};
use crate::ipc::{PrimaryInstance, Signal};
use crate::message::Message;
use iced::advanced::subscription::{self, EventStream, Hasher, Recipe};
use iced::{Subscription, event, stream, window};
use std::collections::BTreeMap;
use std::hash::Hash;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::Instant;

static SINGLE_INSTANCE: OnceLock<PrimaryInstance> = OnceLock::new();

impl App {
    pub(super) fn subscriptions(&self) -> Subscription<Message> {
        let chrome_animation = if self.needs_animation_frames() {
            window::frames().map(Message::ChromeAnimationFrame)
        } else {
            Subscription::none()
        };
        let disk_checks = if self.workspace.documents().iter().any(|document| {
            document.path.is_some()
                && document.disk_revision.is_some()
                && document.has_complete_text_index()
        }) {
            iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::CheckDiskFiles)
        } else {
            Subscription::none()
        };

        Subscription::batch([
            single_instance_subscription(),
            macos_menu_subscription(),
            event::listen_reopen().map(|()| Message::ApplicationReopened),
            iced::system::theme_changes().map(Message::SystemColorModeChanged),
            shortcuts::subscription(),
            subscription::from_recipe(WindowPlacement),
            event::listen_with(|event, _, id| match event {
                iced::Event::Window(window::Event::OpenFailed { error }) => {
                    Some(Message::WindowOpenFailed(id, error))
                }
                _ => None,
            }),
            window::tab_drag::events().map(Message::TabDragEvent),
            disk_checks,
            chrome_animation,
        ])
    }
}

struct WindowPlacement;

impl Recipe for WindowPlacement {
    type Output = Message;

    fn hash(&self, state: &mut Hasher) {
        std::any::TypeId::of::<Self>().hash(state);
    }

    fn stream(self: Box<Self>, input: EventStream) -> futures::stream::BoxStream<'static, Message> {
        use futures::{FutureExt, StreamExt};

        Box::pin(futures::stream::unfold(
            (input, WindowMoves::default()),
            async |(mut input, mut moves)| loop {
                let event = if let Some(deadline) = moves.next_deadline() {
                    let delay = tokio::time::sleep_until(deadline).fuse();
                    futures::pin_mut!(delay);
                    futures::select_biased! {
                        event = input.next().fuse() => event,
                        _ = delay => {
                            if let Some(id) = moves.pop_due(Instant::now()) {
                                return Some((Message::WindowPlacementChanged(id), (input, moves)));
                            }
                            continue;
                        }
                    }
                } else {
                    input.next().await
                };
                let event = event?;
                if let subscription::Event::Interaction {
                    window,
                    event: iced::Event::Window(event),
                    ..
                } = event
                    && let Some(id) = moves.update(window, &event, Instant::now())
                {
                    return Some((Message::WindowPlacementChanged(id), (input, moves)));
                }
            },
        ))
    }
}

#[derive(Default)]
struct WindowMoves {
    pending: BTreeMap<window::Id, Instant>,
}

impl WindowMoves {
    const DELAY: Duration = Duration::from_millis(150);

    fn update(
        &mut self,
        id: window::Id,
        event: &window::Event,
        now: Instant,
    ) -> Option<window::Id> {
        match event {
            window::Event::Moved(_) => {
                // Saving placement does not change pixels. Wait until movement
                // stops before publishing a message that rebuilds every view.
                self.pending.insert(id, now + Self::DELAY);
                None
            }
            window::Event::Rescaled(_) => {
                self.pending.remove(&id);
                Some(id)
            }
            window::Event::CloseRequested | window::Event::Unfocused => {
                self.pending.remove(&id).map(|_| id)
            }
            window::Event::Resized(_) | window::Event::Closed => {
                // Resize already captures position and size through shortcuts.
                self.pending.remove(&id);
                None
            }
            _ => None,
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().copied().min()
    }

    fn pop_due(&mut self, now: Instant) -> Option<window::Id> {
        let id = self
            .pending
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .min_by_key(|(_, deadline)| **deadline)
            .map(|(id, _)| *id)?;
        self.pending.remove(&id);
        Some(id)
    }
}

fn macos_menu_subscription() -> Subscription<Message> {
    #[cfg(target_os = "macos")]
    {
        super::macos::subscription()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Subscription::none()
    }
}

pub fn register_single_instance(instance: PrimaryInstance) {
    let _ = SINGLE_INSTANCE.set(instance);
}

fn single_instance_subscription() -> Subscription<Message> {
    SINGLE_INSTANCE
        .get()
        .filter(|instance| instance.supports_signals())
        .map(|_| Subscription::run(single_instance_signals))
        .unwrap_or_else(Subscription::none)
}

fn single_instance_signals() -> impl iced::futures::Stream<Item = Message> {
    stream::channel(8, async |mut output| {
        let Some(instance) = SINGLE_INSTANCE.get() else {
            return;
        };

        std::thread::spawn(move || {
            loop {
                let mut disconnected = false;
                let accepted = instance.accept_signal_with(|signal| {
                    use iced::futures::SinkExt;
                    let receipt = crate::ipc::AdmissionReceipt::new();
                    let (paths, request) = match signal {
                        Signal::Show(request) => (Vec::new(), request.clone()),
                        Signal::OpenFiles(paths, request) => (paths.clone(), request.clone()),
                    };
                    if futures::executor::block_on(output.send(Message::ForwardedFiles(
                        paths,
                        request,
                        receipt.clone(),
                    )))
                    .is_err()
                    {
                        disconnected = true;
                        return false;
                    }
                    receipt.wait_for_acceptance()
                });
                if disconnected {
                    break;
                }
                match accepted {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_error) => break,
                }
            }
        });

        std::future::pending::<()>().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc;
    use futures::{FutureExt, StreamExt};

    fn moved() -> window::Event {
        window::Event::Moved(iced::Point::new(100.0, 200.0))
    }

    fn interaction(id: window::Id, event: window::Event) -> subscription::Event {
        subscription::Event::Interaction {
            window: id,
            event: iced::Event::Window(event),
            status: iced::event::Status::Ignored,
        }
    }

    fn placement_stream() -> (
        mpsc::UnboundedSender<subscription::Event>,
        futures::stream::BoxStream<'static, Message>,
    ) {
        let (sender, input) = mpsc::unbounded();
        let recipe = subscription::into_recipes(subscription::from_recipe(WindowPlacement))
            .pop()
            .unwrap();
        (sender, recipe.stream(input.boxed()))
    }

    fn next_window(output: &mut futures::stream::BoxStream<'static, Message>) -> window::Id {
        let Some(Some(Message::WindowPlacementChanged(id))) = output.next().now_or_never() else {
            panic!("expected a window placement message");
        };
        id
    }

    #[test]
    fn placement_waits_after_the_last_move_independently_for_each_window() {
        let mut moves = WindowMoves::default();
        let first = window::Id::unique();
        let second = window::Id::unique();
        let now = Instant::now();
        let later = now + Duration::from_millis(100);

        assert_eq!(moves.update(first, &moved(), now), None);
        assert_eq!(moves.update(second, &moved(), now), None);
        assert_eq!(moves.update(first, &moved(), later), None);
        assert_eq!(moves.next_deadline(), Some(now + WindowMoves::DELAY));
        assert_eq!(
            moves.pop_due(now + WindowMoves::DELAY - Duration::from_nanos(1)),
            None
        );
        assert_eq!(moves.pop_due(now + WindowMoves::DELAY), Some(second));
        assert_eq!(moves.next_deadline(), Some(later + WindowMoves::DELAY));
        assert_eq!(moves.pop_due(later + WindowMoves::DELAY), Some(first));
        assert_eq!(moves.next_deadline(), None);
    }

    #[test]
    fn placement_subscription_coalesces_move_events_before_publishing() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        let (input, mut output) = placement_stream();
        let id = window::Id::unique();

        for x in 0..200 {
            input
                .unbounded_send(interaction(
                    id,
                    window::Event::Moved(iced::Point::new(x as f32, 100.0)),
                ))
                .unwrap();
        }
        assert!(output.next().now_or_never().is_none());
        drop(_entered);
        let message = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), output.next())
                .await
                .expect("window movement must publish after it stops")
        });
        assert!(matches!(message, Some(Message::WindowPlacementChanged(window)) if window == id));
        let _entered = runtime.enter();
        assert!(output.next().now_or_never().is_none());
    }

    #[test]
    fn placement_subscription_keeps_scale_and_close_events_immediate() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        let (input, mut output) = placement_stream();
        let first = window::Id::unique();
        let second = window::Id::unique();

        input.unbounded_send(interaction(first, moved())).unwrap();
        input.unbounded_send(interaction(second, moved())).unwrap();
        input
            .unbounded_send(interaction(first, window::Event::Rescaled(1.5)))
            .unwrap();
        assert_eq!(next_window(&mut output), first);
        input
            .unbounded_send(interaction(second, window::Event::CloseRequested))
            .unwrap();
        assert_eq!(next_window(&mut output), second);
        input.unbounded_send(interaction(first, moved())).unwrap();
        input
            .unbounded_send(interaction(first, window::Event::Unfocused))
            .unwrap();
        assert_eq!(next_window(&mut output), first);
        assert!(output.next().now_or_never().is_none());
    }

    #[test]
    fn placement_subscription_drops_pending_moves_after_resize_or_close() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        let (input, mut output) = placement_stream();
        let first = window::Id::unique();
        let second = window::Id::unique();

        for id in [first, second] {
            input.unbounded_send(interaction(id, moved())).unwrap();
        }
        input
            .unbounded_send(interaction(first, window::Event::Closed))
            .unwrap();
        input
            .unbounded_send(interaction(
                second,
                window::Event::Resized(iced::Size::new(800.0, 600.0)),
            ))
            .unwrap();
        for id in [first, second] {
            input
                .unbounded_send(interaction(id, window::Event::Unfocused))
                .unwrap();
        }
        assert!(output.next().now_or_never().is_none());
    }
}
