//! Native presentation with synthetic 60 Hz pixel touchpad packets.
//! cargo run --release --locked -j 1 --example profile_touchpad -- path/to/fixture.txt
//! Set FRAGILE_PERF_TRACE=1 and a fresh FRAGILE_PERF_TRACE_DIR before launching.
//! Use isolated configuration directories; startup tasks run normally.

use fragile_notepad::{app::App, message, perf_trace, startup};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Task, Theme, Vector, window};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const START_DELAY: Duration = Duration::from_secs(4);
const CONTACT_SECONDS: f64 = 6.0;
const MOMENTUM_SECONDS: f64 = 1.0;
const INPUT_HZ: f64 = 60.0;
const PIXELS_PER_PACKET: f32 = -16.0;

#[derive(Debug, Clone)]
enum Message {
    App(message::Message),
    Finished { packets: u32, captured_packets: u32 },
    Exit,
}

struct NativeReplay {
    app: App,
    completed: Arc<AtomicBool>,
}

impl NativeReplay {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::App(message) => self.app.update(message).map(Message::App),
            Message::Finished {
                packets,
                captured_packets,
            } => {
                self.completed.store(
                    packets > 0 && packets == captured_packets,
                    Ordering::Relaxed,
                );
                Task::perform(
                    async { tokio::time::sleep(Duration::from_millis(500)).await },
                    |_| Message::Exit,
                )
            }
            Message::Exit => iced::exit(),
        }
    }

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        Element::new(TouchpadReplay(self.app.view(id).map(Message::App)))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "src/editor/widget/interaction.rs".into());
    let path = std::path::absolute(path)?;
    if !std::fs::metadata(&path)?.is_file() {
        return Err(format!("fixture is not a file: {}", path.display()).into());
    }
    println!(
        "synthetic touchpad replay: fixture={} input_hz={INPUT_HZ} contact_seconds={CONTACT_SECONDS} momentum_seconds={MOMENTUM_SECONDS} pixels_per_packet={PIXELS_PER_PACKET}",
        path.display()
    );
    fragile_notepad::app::initialize_platform();
    let ui_scale = startup::ui_scale_factor();
    let completed = Arc::new(AtomicBool::new(false));
    let app_completed = Arc::clone(&completed);
    iced::daemon(
        move || {
            let (app, task) = App::new_with_options(startup::StartupOptions {
                files: vec![path.clone()],
                restore_session: false,
            });
            let timeout = Task::perform(
                async { tokio::time::sleep(Duration::from_secs(13)).await },
                |_| Message::Exit,
            );
            (
                NativeReplay {
                    app,
                    completed: Arc::clone(&app_completed),
                },
                Task::batch([task.map(Message::App), timeout]),
            )
        },
        NativeReplay::update,
        NativeReplay::view,
    )
    .settings(startup::iced_settings())
    .subscription(|state| state.app.subscription().map(Message::App))
    .title(|state: &NativeReplay, id| state.app.title(id))
    .theme(|state: &NativeReplay, id| state.app.theme(id))
    .scale_factor(move |_, _| ui_scale)
    .run()?;
    perf_trace::flush()?;
    if !completed.load(Ordering::Relaxed) {
        return Err("touchpad replay did not finish with every input received".into());
    }
    Ok(())
}

struct TouchpadReplay<'a>(Element<'a, Message>);

#[derive(Default)]
struct ReplayState {
    starts_at: Option<Instant>,
    next_packet: u32,
    packets: u32,
    captured_packets: u32,
    contact_ended: bool,
    finished: bool,
}

impl Widget<Message, Theme, Renderer> for TouchpadReplay<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ReplayState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(ReplayState::default())
    }
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.0));
    }
    fn size(&self) -> Size<Length> {
        self.0.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.0
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.0
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let replay = tree.state.downcast_mut::<ReplayState>();
            let was_finished = replay.finished;
            let starts_at = *replay.starts_at.get_or_insert(*now + START_DELAY);
            let cursor = mouse::Cursor::Available(iced::Point::new(
                layout.bounds().center_x(),
                layout.bounds().y + layout.bounds().height * 0.6,
            ));
            let mut send = |phase, y| {
                // Capturing a synthetic wheel event must not capture the native redraw.
                let mut messages = Vec::new();
                let mut local = shell.local(&mut messages);
                self.0.as_widget_mut().update(
                    &mut tree.children[0],
                    &Event::Mouse(mouse::Event::WheelScrolled {
                        phase,
                        delta: mouse::ScrollDelta::Pixels { x: 0.0, y },
                    }),
                    layout,
                    cursor,
                    renderer,
                    &mut local,
                    viewport,
                );
                let captured = local.is_event_captured();
                shell.request_redraw_at(local.redraw_request());
                if let Some(diff) = local.is_layout_invalid() {
                    shell.invalidate_layout_with(diff);
                }
                if local.are_widgets_invalid() {
                    shell.invalidate_widgets();
                }
                drop(local);
                for message in messages {
                    shell.publish(message);
                }
                captured
            };
            if *now >= starts_at && !replay.finished {
                let elapsed = now.duration_since(starts_at).as_secs_f64();
                if replay.packets == 0 {
                    perf_trace::event("touchpad_replay_begin", "synthetic=true input_hz=60");
                }
                if elapsed >= CONTACT_SECONDS && !replay.contact_ended {
                    send(mouse::ScrollPhase::ContactEnded, 0.0);
                    perf_trace::event("touchpad_replay_contact_end", replay.packets);
                    replay.contact_ended = true;
                }
                if elapsed >= CONTACT_SECONDS + MOMENTUM_SECONDS {
                    send(mouse::ScrollPhase::Ended, 0.0);
                    perf_trace::event(
                        "touchpad_replay_end",
                        format_args!(
                            "packets={} captured_packets={}",
                            replay.packets, replay.captured_packets
                        ),
                    );
                    println!(
                        "touchpad_replay_end packets={} captured_packets={} synthetic=true",
                        replay.packets, replay.captured_packets
                    );
                    replay.finished = true;
                } else if elapsed * INPUT_HZ >= f64::from(replay.next_packet) {
                    let phase = if replay.packets == 0 {
                        mouse::ScrollPhase::Started
                    } else if replay.contact_ended {
                        mouse::ScrollPhase::Momentum
                    } else {
                        mouse::ScrollPhase::Moved
                    };
                    let decay = if replay.contact_ended {
                        (1.0 - (elapsed - CONTACT_SECONDS) / MOMENTUM_SECONDS).powi(2) as f32
                    } else {
                        1.0
                    };
                    let captured = send(phase, PIXELS_PER_PACKET * decay);
                    replay.captured_packets += u32::from(captured);
                    perf_trace::event(
                        "touchpad_replay_packet",
                        format_args!(
                            "index={} phase={phase:?} captured={captured}",
                            replay.packets
                        ),
                    );
                    replay.packets += 1;
                    // Skip late slots instead of injecting a burst after a slow frame.
                    replay.next_packet = (elapsed * INPUT_HZ).floor() as u32 + 1;
                }
            }
            if replay.finished && !was_finished {
                shell.publish(Message::Finished {
                    packets: replay.packets,
                    captured_packets: replay.captured_packets,
                });
            } else if !replay.finished {
                shell.request_redraw_at(
                    starts_at + Duration::from_secs_f64(f64::from(replay.next_packet) / INPUT_HZ),
                );
            }
        }
        self.0.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.0.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.0
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.0.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
