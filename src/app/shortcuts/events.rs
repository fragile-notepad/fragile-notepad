//! Keeps ordinary scrolling inside widgets while forwarding zoom shortcuts.

use super::{command_or_control, event_to_message};
use crate::message::Message;
use futures::{FutureExt, StreamExt};
use iced::advanced::subscription::{self, EventStream, Hasher, Recipe};
use iced::{Event, Subscription, keyboard, mouse, window};
use std::collections::BTreeMap;
use std::hash::Hash;

pub(in crate::app) fn subscription() -> Subscription<Message> {
    subscription::from_recipe(ShortcutEvents)
}

struct ShortcutEvents;

impl Recipe for ShortcutEvents {
    type Output = Message;

    fn hash(&self, state: &mut Hasher) {
        std::any::TypeId::of::<Self>().hash(state);
    }

    fn stream(self: Box<Self>, input: EventStream) -> futures::stream::BoxStream<'static, Message> {
        Box::pin(futures::stream::unfold(
            (input, ShortcutInput::default()),
            async |(mut input, mut state)| loop {
                let received = if let Some(waiting) = state.waiting.take() {
                    Some(waiting)
                } else {
                    input.next().await
                };
                let received = received?;
                match received {
                    subscription::Event::Interaction {
                        window,
                        event,
                        status,
                    } => {
                        match &event {
                            Event::Keyboard(keyboard::Event::ModifiersChanged(current)) => {
                                state.modifiers.insert(window, *current);
                            }
                            Event::Window(window::Event::Unfocused | window::Event::Closed) => {
                                state.modifiers.remove(&window);
                            }
                            Event::Mouse(mouse::Event::WheelScrolled { .. })
                                if !command_or_control(
                                    state.modifiers.get(&window).copied().unwrap_or_default(),
                                ) =>
                            {
                                continue;
                            }
                            _ => {}
                        }
                        if matches!(
                            &event,
                            Event::Mouse(mouse::Event::Pinched {
                                phase: mouse::ScrollPhase::Started
                                    | mouse::ScrollPhase::Moved
                                    | mouse::ScrollPhase::Momentum,
                                ..
                            })
                        ) {
                            let mut pinches = vec![(event, status, window)];
                            // Drain only packets already queued. Waiting for another frame
                            // could let a widget close command overtake the final zoom.
                            loop {
                                match input.next().now_or_never() {
                                    Some(Some(subscription::Event::Interaction {
                                        window,
                                        event:
                                            event @ Event::Mouse(mouse::Event::Pinched {
                                                phase:
                                                    mouse::ScrollPhase::Started
                                                    | mouse::ScrollPhase::Moved
                                                    | mouse::ScrollPhase::Momentum,
                                                ..
                                            }),
                                        status,
                                    })) => pinches.push((event, status, window)),
                                    Some(Some(waiting)) => {
                                        state.waiting = Some(waiting);
                                        break;
                                    }
                                    _ => break,
                                }
                            }
                            return Some((Message::RuntimeEvents(pinches), (input, state)));
                        }
                        let message = match event {
                            Event::Window(window::Event::CloseRequested) => {
                                Some(Message::WindowCloseRequested(window))
                            }
                            Event::Window(window::Event::Closed) => {
                                Some(Message::WindowClosed(window))
                            }
                            event => event_to_message(event, status, window),
                        };
                        if let Some(message) = message {
                            return Some((message, (input, state)));
                        }
                    }
                    subscription::Event::PlatformSpecific(
                        subscription::PlatformSpecific::MacOS(subscription::MacOS::QuitRequested),
                    ) => return Some((Message::NativeQuitRequested, (input, state))),
                    _ => {}
                }
            },
        ))
    }
}

#[derive(Default)]
struct ShortcutInput {
    modifiers: BTreeMap<window::Id, keyboard::Modifiers>,
    waiting: Option<subscription::Event>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::event::Status;

    fn interaction(window: window::Id, event: Event, status: Status) -> subscription::Event {
        subscription::Event::Interaction {
            window,
            event,
            status,
        }
    }

    fn wheel() -> Event {
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -16.0 },
            phase: mouse::ScrollPhase::Moved,
        })
    }

    fn modifiers(current: keyboard::Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::ModifiersChanged(current))
    }

    fn messages(events: Vec<subscription::Event>) -> Vec<Message> {
        let recipe = subscription::into_recipes(subscription()).pop().unwrap();
        futures::executor::block_on(
            recipe
                .stream(futures::stream::iter(events).boxed())
                .collect(),
        )
    }

    fn forwarded(events: Vec<subscription::Event>) -> Vec<subscription::Event> {
        messages(events)
            .into_iter()
            .flat_map(|message| match message {
                Message::RuntimeEvent(event, status, window) => vec![(event, status, window)],
                Message::RuntimeEvents(events) => events,
                Message::WindowClosed(_) => Vec::new(),
                _ => panic!("expected a runtime event"),
            })
            .map(|(event, status, window)| interaction(window, event, status))
            .collect()
    }

    fn pinch(delta: f32, phase: mouse::ScrollPhase) -> Event {
        Event::Mouse(mouse::Event::Pinched { delta, phase })
    }

    #[test]
    fn ordinary_wheel_events_stay_in_widgets_and_captured_zoom_keeps_input_order() {
        let window = window::Id::unique();
        for status in [Status::Ignored, Status::Captured] {
            for command in [keyboard::Modifiers::CTRL, keyboard::Modifiers::COMMAND] {
                let shift = interaction(window, modifiers(keyboard::Modifiers::SHIFT), status);
                let pressed = interaction(window, modifiers(command), status);
                let released = interaction(window, modifiers(keyboard::Modifiers::NONE), status);
                let zoom = interaction(window, wheel(), status);
                let input = vec![
                    zoom.clone(),
                    shift.clone(),
                    zoom.clone(),
                    pressed.clone(),
                    zoom.clone(),
                    released.clone(),
                    zoom.clone(),
                ];
                assert_eq!(forwarded(input), vec![shift, pressed, zoom, released]);
            }
        }
    }

    #[test]
    fn wheel_modifiers_belong_to_each_window_and_clear_on_unfocus_and_close() {
        let first = window::Id::unique();
        let second = window::Id::unique();
        let pressed = interaction(
            first,
            modifiers(keyboard::Modifiers::CTRL),
            Status::Captured,
        );
        let second_empty = interaction(
            second,
            modifiers(keyboard::Modifiers::NONE),
            Status::Ignored,
        );
        let zoom = interaction(first, wheel(), Status::Captured);
        let unfocused = interaction(
            first,
            Event::Window(window::Event::Unfocused),
            Status::Ignored,
        );
        let closed = interaction(first, Event::Window(window::Event::Closed), Status::Ignored);
        let output = forwarded(vec![
            pressed.clone(),
            second_empty.clone(),
            interaction(second, wheel(), Status::Captured),
            zoom.clone(),
            unfocused.clone(),
            zoom.clone(),
            pressed.clone(),
            zoom.clone(),
            closed,
            zoom.clone(),
        ]);
        assert_eq!(
            output,
            vec![
                pressed.clone(),
                second_empty,
                zoom.clone(),
                unfocused,
                pressed,
                zoom
            ]
        );
    }

    #[test]
    fn pinch_packets_share_one_update_and_terminal_phases_keep_their_order() {
        let first = window::Id::unique();
        let second = window::Id::unique();
        let packets = vec![
            (
                pinch(0.25, mouse::ScrollPhase::Started),
                Status::Captured,
                first,
            ),
            (
                pinch(0.2, mouse::ScrollPhase::Moved),
                Status::Ignored,
                second,
            ),
            (
                pinch(-0.5, mouse::ScrollPhase::Moved),
                Status::Captured,
                first,
            ),
        ];
        for phase in [
            mouse::ScrollPhase::ContactEnded,
            mouse::ScrollPhase::Ended,
            mouse::ScrollPhase::Cancelled,
        ] {
            let terminal = pinch(0.0, phase);
            let input = packets
                .iter()
                .cloned()
                .map(|(event, status, window)| interaction(window, event, status))
                .chain([interaction(first, terminal.clone(), Status::Captured)])
                .collect();
            let output = messages(input);
            assert!(matches!(output.as_slice(), [Message::RuntimeEvents(events),
                Message::RuntimeEvent(event, Status::Captured, window)]
                if events == &packets && event == &terminal && *window == first));
        }
    }

    #[test]
    fn pending_pinch_packets_flush_before_wheel_modifiers_close_and_quit() {
        let window = window::Id::unique();
        let pressed = modifiers(keyboard::Modifiers::CTRL);
        let zoom = pinch(0.25, mouse::ScrollPhase::Moved);
        let output = messages(vec![
            interaction(window, pressed.clone(), Status::Captured),
            interaction(window, zoom.clone(), Status::Captured),
            interaction(window, wheel(), Status::Captured),
            interaction(window, zoom.clone(), Status::Ignored),
            interaction(
                window,
                modifiers(keyboard::Modifiers::NONE),
                Status::Ignored,
            ),
            interaction(window, zoom.clone(), Status::Captured),
            interaction(
                window,
                Event::Window(window::Event::CloseRequested),
                Status::Ignored,
            ),
            interaction(window, zoom.clone(), Status::Captured),
            interaction(
                window,
                Event::Window(window::Event::Closed),
                Status::Ignored,
            ),
            interaction(window, zoom.clone(), Status::Captured),
            subscription::Event::PlatformSpecific(subscription::PlatformSpecific::MacOS(
                subscription::MacOS::QuitRequested,
            )),
        ]);
        assert!(matches!(output.as_slice(), [
            Message::RuntimeEvent(Event::Keyboard(keyboard::Event::ModifiersChanged(_)), ..),
            Message::RuntimeEvents(_),
            Message::RuntimeEvent(Event::Mouse(mouse::Event::WheelScrolled { .. }), Status::Captured, _),
            Message::RuntimeEvents(_),
            Message::RuntimeEvent(Event::Keyboard(keyboard::Event::ModifiersChanged(_)), ..),
            Message::RuntimeEvents(_),
            Message::WindowCloseRequested(id),
            Message::RuntimeEvents(_),
            Message::WindowClosed(closed_id),
            Message::RuntimeEvents(_),
            Message::NativeQuitRequested,
        ] if *id == window && *closed_id == window));

        let output = messages(vec![
            interaction(
                window,
                Event::Window(window::Event::CloseRequested),
                Status::Ignored,
            ),
            interaction(
                window,
                Event::Window(window::Event::Closed),
                Status::Ignored,
            ),
            subscription::Event::PlatformSpecific(subscription::PlatformSpecific::MacOS(
                subscription::MacOS::QuitRequested,
            )),
        ]);
        assert!(matches!(
            output.as_slice(),
            [
                Message::WindowCloseRequested(_),
                Message::WindowClosed(_),
                Message::NativeQuitRequested
            ]
        ));
    }

    #[test]
    fn lone_pinch_publishes_immediately_and_queued_packets_share_an_update() {
        use futures::channel::mpsc;
        let (input, stream) = mpsc::unbounded();
        let recipe = subscription::into_recipes(subscription()).pop().unwrap();
        let mut output = recipe.stream(stream.boxed());
        let window = window::Id::unique();
        for count in [1, 8] {
            for _ in 0..count {
                input
                    .unbounded_send(interaction(
                        window,
                        pinch(0.01, mouse::ScrollPhase::Moved),
                        Status::Captured,
                    ))
                    .unwrap();
            }
            let message = output.next().now_or_never();
            assert!(
                matches!(message, Some(Some(Message::RuntimeEvents(events))) if events.len() == count)
            );
            assert!(output.next().now_or_never().is_none());
        }
    }

    #[test]
    fn grouped_zoom_keeps_each_packet_clamp_and_reversal_across_windows() {
        use crate::app::App;
        let (mut app, _) = App::new();
        let _ = app.update(Message::SettingsLoaded(Ok(None)));
        let first = app.main_window_id.unwrap();
        let _ = app.update(Message::NewWindow);
        let second = app.main_window_id.unwrap();
        let packets: Vec<_> = [10.0, -0.5, -0.9, 0.25, 0.2]
            .into_iter()
            .enumerate()
            .map(|(index, delta)| {
                (
                    pinch(delta, mouse::ScrollPhase::Moved),
                    Status::Captured,
                    if index % 2 == 0 { first } else { second },
                )
            })
            .collect();
        for (event, status, window) in &packets {
            let _ = app.update(Message::RuntimeEvent(event.clone(), *status, *window));
        }
        let individual = app.settings.zoom;
        let _ = app.update(Message::ZoomReset);
        let output = messages(
            packets
                .iter()
                .cloned()
                .map(|(event, status, window)| interaction(window, event, status))
                .collect(),
        );
        assert!(
            matches!(output.as_slice(), [Message::RuntimeEvents(events)] if events == &packets)
        );
        for message in output {
            let _ = app.update(message);
        }
        assert_eq!(app.settings.zoom.to_bits(), individual.to_bits());
        assert_eq!(app.settings.zoom, 0.75);
    }
}
