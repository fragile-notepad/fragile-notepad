//! A windowing shell for Iced, on top of [`winit`].
//!
//! ![The native path of the Iced ecosystem](https://github.com/iced-rs/iced/blob/0525d76ff94e828b7b21634fa94a747022001c83/docs/graphs/native.png?raw=true)
//!
//! `iced_winit` offers some convenient abstractions on top of [`iced_runtime`]
//! to quickstart development when using [`winit`].
//!
//! It exposes a renderer-agnostic [`Program`] trait that can be implemented
//! and then run with a simple call. The use of this trait is optional.
//!
//! Additionally, a [`conversion`] module is available for users that decide to
//! implement a custom event loop.
//!
//! [`iced_runtime`]: https://github.com/iced-rs/iced/tree/master/runtime
//! [`winit`]: https://github.com/rust-windowing/winit
//! [`conversion`]: crate::conversion
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/iced-rs/iced/9ab6923e943f784985e9ef9ca28b10278297225d/docs/logo.svg"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]
pub use iced_debug as debug;
pub use iced_program as program;
pub use iced_runtime as runtime;
pub use program::core;
pub use program::graphics;
pub use runtime::futures;
pub use winit;

pub mod clipboard;
pub mod conversion;

mod error;
mod proxy;
mod trace;
mod window;

pub use clipboard::Clipboard;
pub use error::Error;
pub use proxy::Proxy;

use crate::core::backend;
use crate::core::mouse;
use crate::core::renderer;
use crate::core::theme;
use crate::core::time::Instant;
use crate::core::widget::operation;
use crate::core::{Point, Size};
use crate::futures::futures::channel::mpsc;
use crate::futures::futures::channel::oneshot;
use crate::futures::futures::stream;
use crate::futures::futures::task;
use crate::futures::futures::{Future, StreamExt};
use crate::futures::subscription;
use crate::futures::{Executor, Runtime};
use crate::graphics::Viewport;
use crate::graphics::{Compositor, Shell, compositor};
use crate::runtime::font;
use crate::runtime::image;
use crate::runtime::system;
use crate::runtime::user_interface::{self, UserInterface};
use crate::runtime::{Action, Task};

use program::Program;

use rustc_hash::FxHashMap;
use std::borrow::Cow;
use std::mem::ManuallyDrop;
use std::slice;
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

/// Runs a [`Program`] with the provided settings.
pub fn run<P>(program: P) -> Result<(), Error>
where
    P: Program + 'static,
    P::Theme: theme::Base,
    <P::Renderer as compositor::Default>::Compositor: Send,
{
    use winit::event_loop::EventLoop;

    let boot_span = debug::boot();
    let settings = program.settings();
    let window_settings = program.window();

    let event_loop = EventLoop::with_user_event()
        .build()
        .expect("Create event loop");

    let backend_settings = backend::Settings::from(&settings);
    let renderer_settings = renderer::Settings::from(&settings);
    let display_handle = event_loop.owned_display_handle();

    let (proxy, worker) = Proxy::new(event_loop.create_proxy());

    #[cfg(feature = "debug")]
    {
        let proxy = proxy.clone();

        debug::on_hotpatch(move || {
            proxy.send_action(Action::Reload);
        });
    }

    let mut runtime = {
        let executor = P::Executor::new().map_err(Error::ExecutorCreationFailed)?;
        executor.spawn(worker);

        Runtime::new(executor, proxy.clone())
    };

    let (program, task) = runtime.enter(|| program::Instance::new(program));
    let is_daemon = window_settings.is_none();

    let task = if let Some(window_settings) = window_settings {
        let mut task = Some(task);

        let (_id, open) = runtime::window::open(window_settings);

        open.then(move |_| task.take().unwrap_or_else(Task::none))
    } else {
        task
    };

    if let Some(stream) = runtime::task::into_stream(task) {
        runtime.run(stream);
    }

    runtime.track(subscription::into_recipes(
        runtime.enter(|| program.subscription().map(Action::Output)),
    ));

    let (event_sender, event_receiver) = mpsc::unbounded();
    let (control_sender, control_receiver) = mpsc::unbounded();
    let (system_theme_sender, system_theme_receiver) = oneshot::channel();

    let instance = Box::pin(run_instance::<P>(
        program,
        runtime,
        proxy.clone(),
        event_receiver,
        control_sender,
        display_handle,
        is_daemon,
        backend_settings,
        renderer_settings,
        settings.fonts,
        system_theme_receiver,
    ));

    let context = task::Context::from_waker(task::noop_waker_ref());

    struct Runner<Message: 'static, F> {
        instance: std::pin::Pin<Box<F>>,
        context: task::Context<'static>,
        id: Option<String>,
        sender: mpsc::UnboundedSender<Event<Action<Message>>>,
        receiver: mpsc::UnboundedReceiver<Control>,
        error: Option<Error>,
        system_theme: Option<oneshot::Sender<theme::Mode>>,

        #[cfg(target_arch = "wasm32")]
        canvas: Option<web_sys::HtmlCanvasElement>,
    }

    let runner = Runner {
        instance,
        context,
        id: settings.id,
        sender: event_sender,
        receiver: control_receiver,
        error: None,
        system_theme: Some(system_theme_sender),

        #[cfg(target_arch = "wasm32")]
        canvas: None,
    };

    boot_span.finish();

    impl<Message, F> winit::application::ApplicationHandler<Action<Message>> for Runner<Message, F>
    where
        F: Future<Output = ()>,
    {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            if let Some(sender) = self.system_theme.take() {
                let _ = sender.send(
                    event_loop
                        .system_theme()
                        .map(conversion::theme_mode)
                        .unwrap_or_default(),
                );
            }
        }

        fn new_events(
            &mut self,
            event_loop: &winit::event_loop::ActiveEventLoop,
            cause: winit::event::StartCause,
        ) {
            self.process_event(
                event_loop,
                Event::EventLoopAwakened(winit::event::Event::NewEvents(cause)),
            );
        }

        fn window_event(
            &mut self,
            event_loop: &winit::event_loop::ActiveEventLoop,
            window_id: winit::window::WindowId,
            event: winit::event::WindowEvent,
        ) {
            #[cfg(target_os = "windows")]
            let is_move_or_resize = matches!(
                event,
                winit::event::WindowEvent::Resized(_) | winit::event::WindowEvent::Moved(_)
            );

            self.process_event(
                event_loop,
                Event::EventLoopAwakened(winit::event::Event::WindowEvent { window_id, event }),
            );

            // TODO: Remove when unnecessary
            // On Windows, we emulate an `AboutToWait` event after every `Resized` event
            // since the event loop does not resume during resize interaction.
            // More details: https://github.com/rust-windowing/winit/issues/3272
            #[cfg(target_os = "windows")]
            {
                if is_move_or_resize {
                    self.process_event(
                        event_loop,
                        Event::EventLoopAwakened(winit::event::Event::AboutToWait),
                    );
                }
            }
        }

        fn user_event(
            &mut self,
            event_loop: &winit::event_loop::ActiveEventLoop,
            action: Action<Message>,
        ) {
            self.process_event(
                event_loop,
                Event::EventLoopAwakened(winit::event::Event::UserEvent(action)),
            );
        }

        fn received_url(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, url: String) {
            self.process_event(
                event_loop,
                Event::EventLoopAwakened(winit::event::Event::PlatformSpecific(
                    winit::event::PlatformSpecific::MacOS(winit::event::MacOS::ReceivedUrl(url)),
                )),
            );
        }

        fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            self.process_event(
                event_loop,
                Event::EventLoopAwakened(winit::event::Event::AboutToWait),
            );
        }
    }

    impl<Message, F> Runner<Message, F>
    where
        F: Future<Output = ()>,
    {
        fn process_event(
            &mut self,
            event_loop: &winit::event_loop::ActiveEventLoop,
            event: Event<Action<Message>>,
        ) {
            if event_loop.exiting() {
                return;
            }

            self.sender.start_send(event).expect("Send event");

            loop {
                let poll = self.instance.as_mut().poll(&mut self.context);

                match poll {
                    task::Poll::Pending => match self.receiver.try_recv() {
                        Ok(control) => match control {
                            Control::ChangeFlow(flow) => {
                                use winit::event_loop::ControlFlow;

                                match (event_loop.control_flow(), flow) {
                                    (
                                        ControlFlow::WaitUntil(current),
                                        ControlFlow::WaitUntil(new),
                                    ) if current > Instant::now() && current < new => {}
                                    (ControlFlow::WaitUntil(target), ControlFlow::Wait)
                                        if target > Instant::now() => {}
                                    _ => {
                                        event_loop.set_control_flow(flow);
                                    }
                                }
                            }
                            Control::CreateWindow {
                                id,
                                settings,
                                title,
                                scale_factor,
                                monitor,
                                on_open,
                            } => {
                                let exit_on_close_request = settings.exit_on_close_request;

                                let visible = settings.visible;

                                #[cfg(target_arch = "wasm32")]
                                let target = settings.platform_specific.target.clone();

                                let window_attributes = conversion::window_attributes(
                                    settings,
                                    &title,
                                    scale_factor,
                                    monitor.or(event_loop.primary_monitor()),
                                    self.id.clone(),
                                )
                                .with_visible(false);

                                #[cfg(target_arch = "wasm32")]
                                let window_attributes = {
                                    use winit::platform::web::WindowAttributesExtWebSys;
                                    window_attributes.with_canvas(self.canvas.take())
                                };

                                log::info!(
                                    "Window attributes for id `{id:#?}`: {window_attributes:#?}"
                                );

                                // On macOS, the `position` in `WindowAttributes` represents the "inner"
                                // position of the window; while on other platforms it's the "outer" position.
                                // We fix the inconsistency on macOS by positioning the window after creation.
                                #[cfg(target_os = "macos")]
                                let mut window_attributes = window_attributes;

                                #[cfg(target_os = "macos")]
                                let position = window_attributes.position.take();

                                let window = event_loop
                                    .create_window(window_attributes)
                                    .expect("Create window");

                                #[cfg(target_os = "macos")]
                                if let Some(position) = position {
                                    window.set_outer_position(position);
                                }

                                #[cfg(target_arch = "wasm32")]
                                {
                                    use winit::platform::web::WindowExtWebSys;

                                    let canvas = window.canvas().expect("Get window canvas");

                                    let _ = canvas.set_attribute(
                                        "style",
                                        "display: block; width: 100%; height: 100%",
                                    );

                                    let window = web_sys::window().unwrap();
                                    let document = window.document().unwrap();
                                    let body = document.body().unwrap();

                                    let target = target.and_then(|target| {
                                        body.query_selector(&format!("#{target}"))
                                            .ok()
                                            .unwrap_or(None)
                                    });

                                    match target {
                                        Some(node) => {
                                            let _ = node.replace_with_with_node_1(&canvas).expect(
                                                &format!("Could not replace #{}", node.id()),
                                            );
                                        }
                                        None => {
                                            let _ = body
                                                .append_child(&canvas)
                                                .expect("Append canvas to HTML body");
                                        }
                                    };
                                }

                                self.process_event(
                                    event_loop,
                                    Event::WindowCreated {
                                        id,
                                        window: Arc::new(window),
                                        exit_on_close_request,
                                        make_visible: visible,
                                        on_open,
                                    },
                                );
                            }
                            Control::Exit => {
                                self.process_event(event_loop, Event::Exit);
                                event_loop.exit();
                                break;
                            }
                            Control::Crash(error) => {
                                self.error = Some(error);
                                event_loop.exit();
                            }
                            Control::SetAutomaticWindowTabbing(_enabled) => {
                                #[cfg(target_os = "macos")]
                                {
                                    use winit::platform::macos::ActiveEventLoopExtMacOS;
                                    event_loop.set_allows_automatic_window_tabbing(_enabled);
                                }
                            }
                        },
                        _ => {
                            break;
                        }
                    },
                    task::Poll::Ready(_) => {
                        event_loop.exit();
                        break;
                    }
                };
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut runner = runner;
        event_loop
            .run_app(&mut runner)
            .map_err(Error::EventLoopFailed)?;

        runner.error.map(Err).unwrap_or(Ok(()))
    }

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        let _ = event_loop.spawn_app(runner);

        Ok(())
    }
}

#[derive(Debug)]
enum Event<Message: 'static> {
    WindowCreated {
        id: window::Id,
        window: Arc<winit::window::Window>,
        exit_on_close_request: bool,
        make_visible: bool,
        on_open: oneshot::Sender<window::Id>,
    },
    EventLoopAwakened(winit::event::Event<Message>),
    Exit,
}

#[derive(Debug)]
enum Control {
    ChangeFlow(winit::event_loop::ControlFlow),
    Exit,
    Crash(Error),
    CreateWindow {
        id: window::Id,
        settings: window::Settings,
        title: String,
        monitor: Option<winit::monitor::MonitorHandle>,
        on_open: oneshot::Sender<window::Id>,
        scale_factor: f32,
    },
    SetAutomaticWindowTabbing(bool),
}

struct PendingBackendHandoff<C: Compositor> {
    compositor: C,
    renderers: FxHashMap<window::Id, C::Renderer>,
    sender: oneshot::Sender<backend::StrictHandoffOutcome>,
    phase: backend::StrictHandoffPhase,
    commit_after: Option<StdInstant>,
}

struct PreparingBackendHandoff<C: Compositor> {
    receiver: oneshot::Receiver<Result<C, backend::Error>>,
    sender: oneshot::Sender<backend::StrictHandoffOutcome>,
    warming: Option<WarmingBackendHandoff<C>>,
}

struct WarmingBackendHandoff<C: Compositor> {
    compositor: C,
    renderers: FxHashMap<window::Id, C::Renderer>,
    remaining: Vec<window::Id>,
    started: StdInstant,
}

struct RetainedBackendHandoff<P, C>
where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    compositor: C,
    windows: Vec<(window::Id, window::RenderingState<P, C>)>,
}

struct AwaitingFirstPresentBackendHandoff<P, C>
where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    sender: oneshot::Sender<backend::StrictHandoffOutcome>,
    windows: Vec<window::Id>,
    evidence: Vec<backend::StrictHandoffWindowEvidence>,
    retained: RetainedBackendHandoff<P, C>,
    first_present_started_at: StdInstant,
    missing_progress_passes: u8,
}

enum BackendHandoffAfterPresent<P, C>
where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    Complete {
        awaiting: AwaitingFirstPresentBackendHandoff<P, C>,
        evidence: backend::StrictHandoffWindowEvidence,
    },
    Failed {
        awaiting: AwaitingFirstPresentBackendHandoff<P, C>,
        evidence: backend::StrictHandoffWindowEvidence,
        message: String,
    },
}

const RENDER_INJECT_FAILURE_ENV: &str = "FRAGILE_NOTEPAD_RENDER_INJECT_FAILURE";
const RENDER_INJECT_PREPARE_DELAY_MS_ENV: &str = "FRAGILE_NOTEPAD_RENDER_PREPARE_DELAY_MS";
const RENDER_COMMIT_PENDING_DELAY_MS_ENV: &str = "FRAGILE_NOTEPAD_RENDER_COMMIT_PENDING_DELAY_MS";
const RENDER_CLOSE_DURING_COMMIT_PENDING_ENV: &str =
    "FRAGILE_NOTEPAD_RENDER_CLOSE_DURING_COMMIT_PENDING";
const AWAITING_FIRST_PRESENT_MISSING_TIMEOUT_MS: u64 = 3_000;

fn injected_backend_failure(phase: &str) -> Option<backend::Error> {
    let requested = std::env::var(RENDER_INJECT_FAILURE_ENV).ok()?;

    (requested == phase).then(|| {
        backend::Error::BackendError(format!("phase={phase} injected backend handoff failure"))
    })
}

fn maybe_delay_backend_prepare() {
    let Some(delay_ms) = std::env::var(RENDER_INJECT_PREPARE_DELAY_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return;
    };

    if delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
}

fn backend_commit_pending_delay_deadline() -> Option<StdInstant> {
    let Some(delay_ms) = std::env::var(RENDER_COMMIT_PENDING_DELAY_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return None;
    };

    if delay_ms > 0 {
        Some(StdInstant::now() + Duration::from_millis(delay_ms))
    } else {
        None
    }
}

fn should_close_during_commit_pending() -> bool {
    std::env::var_os(RENDER_CLOSE_DURING_COMMIT_PENDING_ENV).is_some()
}

fn strict_handoff_error(
    phase: backend::StrictHandoffPhase,
    category: backend::StrictHandoffFailureCategory,
    rollback: backend::StrictRollbackStatus,
    message: impl Into<String>,
) -> backend::StrictHandoffError {
    strict_handoff_error_with_windows(phase, category, rollback, Vec::new(), message)
}

fn strict_handoff_error_with_windows(
    phase: backend::StrictHandoffPhase,
    category: backend::StrictHandoffFailureCategory,
    rollback: backend::StrictRollbackStatus,
    windows: Vec<backend::StrictHandoffWindowEvidence>,
    message: impl Into<String>,
) -> backend::StrictHandoffError {
    backend::StrictHandoffError {
        phase,
        category,
        rollback,
        windows,
        message: message.into(),
    }
}

fn strict_backend_error(
    phase: backend::StrictHandoffPhase,
    category: backend::StrictHandoffFailureCategory,
    rollback: backend::StrictRollbackStatus,
    error: backend::Error,
) -> backend::StrictHandoffError {
    strict_handoff_error(phase, category, rollback, error.to_string())
}

fn strict_surface_failure(error: &compositor::SurfaceError) -> backend::SurfaceFailure {
    match error {
        compositor::SurfaceError::Timeout => backend::SurfaceFailure::Timeout,
        compositor::SurfaceError::Outdated => backend::SurfaceFailure::Outdated,
        compositor::SurfaceError::Lost => backend::SurfaceFailure::Lost,
        compositor::SurfaceError::OutOfMemory => backend::SurfaceFailure::OutOfMemory,
        compositor::SurfaceError::Occluded => backend::SurfaceFailure::Occluded,
        compositor::SurfaceError::Other => backend::SurfaceFailure::Other,
    }
}

fn strict_present_evidence(
    window: window::Id,
    frame_sequence: u64,
    renderer_family: backend::RendererFamily,
    adapter: Option<String>,
    backend: Option<String>,
    status: backend::PresentStatus,
    phase: backend::StrictHandoffPhase,
) -> backend::StrictHandoffWindowEvidence {
    backend::StrictHandoffWindowEvidence {
        window,
        frame_sequence,
        renderer_family,
        adapter,
        backend,
        status,
        phase,
    }
}

fn strict_present_evidence_from_information(
    window: window::Id,
    frame_sequence: u64,
    information: &compositor::Information,
    status: backend::PresentStatus,
    phase: backend::StrictHandoffPhase,
) -> backend::StrictHandoffWindowEvidence {
    strict_present_evidence(
        window,
        frame_sequence,
        strict_renderer_family(information),
        Some(information.adapter.clone()),
        Some(information.backend.clone()),
        status,
        phase,
    )
}

fn strict_renderer_family(information: &compositor::Information) -> backend::RendererFamily {
    match information.backend.as_str() {
        "tiny-skia" => backend::RendererFamily::TinySkia,
        "Vulkan" | "Metal" | "Dx12" | "Dx11" | "Gl" | "BrowserWebGpu" | "Noop" => {
            backend::RendererFamily::Wgpu
        }
        "Null" => backend::RendererFamily::Null,
        _ => backend::RendererFamily::Unknown,
    }
}

fn next_present_frame_sequence(
    frame_sequences: &mut FxHashMap<window::Id, u64>,
    window: window::Id,
) -> u64 {
    let sequence = frame_sequences.entry(window).or_insert(0);
    *sequence += 1;
    *sequence
}

fn trace_strict_present_evidence(
    evidence: &backend::StrictHandoffWindowEvidence,
    elapsed_us: u128,
) {
    if !trace::enabled() {
        return;
    }

    trace::event(
        "backend_handoff_present_evidence",
        elapsed_us,
        format_args!(
            "window={} frame_sequence={} renderer_family={:?} adapter={} backend={} status={:?} phase={:?}",
            evidence.window,
            evidence.frame_sequence,
            evidence.renderer_family,
            evidence.adapter.as_deref().unwrap_or("unknown"),
            evidence.backend.as_deref().unwrap_or("unknown"),
            evidence.status,
            evidence.phase,
        ),
    );
}

fn trace_offscreen_warm_up_evidence(
    event: &'static str,
    phase: backend::StrictHandoffPhase,
    evidence: &compositor::OffscreenWarmUpEvidence,
) {
    if !trace::enabled() {
        return;
    }

    trace::event(
        event,
        evidence.elapsed_us,
        format_args!(
            "phase={phase:?} renderer_family={:?} adapter={} backend={} width={} height={} passes={} elapsed_us={} submission_completed={}",
            evidence.renderer_family,
            evidence.adapter.as_deref().unwrap_or("unknown"),
            evidence.backend.as_deref().unwrap_or("unknown"),
            evidence.width,
            evidence.height,
            evidence.passes,
            evidence.elapsed_us,
            evidence.submission_completed,
        ),
    );
}

fn trace_offscreen_warm_up_failure(
    phase: backend::StrictHandoffPhase,
    renderer_family: backend::RendererFamily,
    adapter: Option<&str>,
    backend: Option<&str>,
    width: u32,
    height: u32,
    elapsed_us: u128,
    error: &compositor::OffscreenWarmUpError,
) {
    if !trace::enabled() {
        return;
    }

    trace::event(
        "backend_handoff_warm_failed",
        elapsed_us,
        format_args!(
            "phase={phase:?} renderer_family={renderer_family:?} adapter={} backend={} width={width} height={height} passes=0 elapsed_us={elapsed_us} submission_completed=false error={error:?}",
            adapter.unwrap_or("unknown"),
            backend.unwrap_or("unknown"),
        ),
    );
}

fn trace_awaiting_first_present_missing(
    missing_windows: &[window::Id],
    progress_passes: u8,
    elapsed_ms: u128,
    timeout_ms: u64,
    exhausted: bool,
) {
    if !trace::enabled() {
        return;
    }

    let missing = missing_windows
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("|");

    trace::event(
        "backend_handoff_first_present_missing",
        0,
        format_args!(
            "phase={:?} missing_windows={} missing_count={} progress_passes={} elapsed_ms={} timeout_ms={} exhausted={}",
            backend::StrictHandoffPhase::AwaitingFirstPresent,
            missing,
            missing_windows.len(),
            progress_passes,
            elapsed_ms,
            timeout_ms,
            exhausted,
        ),
    );
}

fn awaiting_first_present_missing_timeout<P, C>(
    awaiting: &AwaitingFirstPresentBackendHandoff<P, C>,
) -> (u128, bool)
where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let elapsed_ms = awaiting.first_present_started_at.elapsed().as_millis();
    let exhausted = elapsed_ms >= u128::from(AWAITING_FIRST_PRESENT_MISSING_TIMEOUT_MS);

    (elapsed_ms, exhausted)
}

fn missing_first_present_windows<P, C>(
    awaiting: &AwaitingFirstPresentBackendHandoff<P, C>,
    window_manager: &window::Manager<P, C>,
) -> Option<Vec<window::Id>>
where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let live_required_windows = awaiting
        .windows
        .iter()
        .copied()
        .filter(|window| window_manager.get(*window).is_some())
        .collect::<Vec<_>>();

    if live_required_windows.is_empty() {
        return None;
    }

    Some(
        live_required_windows
            .into_iter()
            .filter(|window| {
                !awaiting.evidence.iter().any(|evidence| {
                    evidence.window == *window
                        && evidence.renderer_family == backend::RendererFamily::Wgpu
                        && evidence.status == backend::PresentStatus::Presented
                })
            })
            .collect(),
    )
}

fn missing_first_present_evidence(
    windows: &[window::Id],
) -> Vec<backend::StrictHandoffWindowEvidence> {
    windows
        .iter()
        .copied()
        .map(|window| {
            strict_present_evidence(
                window,
                0,
                backend::RendererFamily::Unknown,
                None,
                None,
                backend::PresentStatus::Failed(backend::SurfaceFailure::Other),
                backend::StrictHandoffPhase::AwaitingFirstPresent,
            )
        })
        .collect()
}

fn strict_closed_before_proof_evidence(
    windows: &[window::Id],
) -> Vec<backend::StrictHandoffWindowEvidence> {
    windows
        .iter()
        .copied()
        .map(|window| {
            strict_present_evidence(
                window,
                0,
                backend::RendererFamily::Unknown,
                None,
                None,
                backend::PresentStatus::ClosedBeforeProof,
                backend::StrictHandoffPhase::AwaitingFirstPresent,
            )
        })
        .collect()
}

async fn maybe_delay_backend_prepare_async() {
    maybe_delay_backend_prepare();
}

async fn run_instance<P>(
    mut program: program::Instance<P>,
    mut runtime: Runtime<P::Executor, Proxy<P::Message>, Action<P::Message>>,
    mut proxy: Proxy<P::Message>,
    mut event_receiver: mpsc::UnboundedReceiver<Event<Action<P::Message>>>,
    mut control_sender: mpsc::UnboundedSender<Control>,
    display_handle: winit::event_loop::OwnedDisplayHandle,
    is_daemon: bool,
    backend_settings: backend::Settings,
    mut renderer_settings: renderer::Settings,
    default_fonts: Vec<Cow<'static, [u8]>>,
    mut _system_theme: oneshot::Receiver<theme::Mode>,
) where
    P: Program + 'static,
    P::Theme: theme::Base,
    <P::Renderer as compositor::Default>::Compositor: Send,
{
    use winit::event;
    use winit::event_loop::ControlFlow;

    let mut window_manager = window::Manager::new();
    let mut is_window_opening = !is_daemon;

    let mut compositor = None;
    let mut preparing_backend_handoff = None;
    let mut pending_backend_handoff = None;
    let mut awaiting_first_present_backend_handoff = None;
    let mut present_frame_sequences = FxHashMap::default();
    let mut events = Vec::new();
    let mut messages = Vec::new();
    let mut actions = 0;

    let mut ui_caches = FxHashMap::default();
    let mut user_interfaces = ManuallyDrop::new(FxHashMap::default());
    let mut clipboard = Clipboard::new();

    #[cfg(all(feature = "linux-theme-detection", target_os = "linux"))]
    let mut system_theme = {
        let to_mode = |color_scheme| match color_scheme {
            mundy::ColorScheme::NoPreference => theme::Mode::None,
            mundy::ColorScheme::Light => theme::Mode::Light,
            mundy::ColorScheme::Dark => theme::Mode::Dark,
        };

        runtime.run(
            mundy::Preferences::stream(mundy::Interest::ColorScheme)
                .map(move |preferences| {
                    Action::System(system::Action::NotifyTheme(to_mode(
                        preferences.color_scheme,
                    )))
                })
                .boxed(),
        );

        runtime
            .enter(|| {
                mundy::Preferences::once_blocking(
                    mundy::Interest::ColorScheme,
                    core::time::Duration::from_millis(200),
                )
            })
            .map(|preferences| to_mode(preferences.color_scheme))
            .unwrap_or_default()
    };

    #[cfg(not(all(feature = "linux-theme-detection", target_os = "linux")))]
    let mut system_theme = _system_theme.try_recv().ok().flatten().unwrap_or_default();

    log::info!("System theme: {system_theme:?}");

    'next_event: loop {
        // Empty the queue if possible
        let event = if let Ok(event) = event_receiver.try_recv() {
            Some(event)
        } else {
            event_receiver.next().await
        };

        let Some(event) = event else {
            break;
        };

        drive_preparing_backend_handoff(
            &mut preparing_backend_handoff,
            &mut pending_backend_handoff,
            &mut window_manager,
            renderer_settings,
            &mut events,
            &mut user_interfaces,
        );

        match event {
            Event::WindowCreated {
                id,
                window,
                exit_on_close_request,
                make_visible,
                on_open,
            } => {
                if compositor.is_none() {
                    let (compositor_sender, compositor_receiver) = oneshot::channel();

                    let create_compositor = {
                        let window = window.clone();
                        let backend_settings = backend_settings.clone();
                        let display_handle = display_handle.clone();
                        let proxy = proxy.clone();
                        let default_fonts = default_fonts.clone();

                        async move {
                            let shell = Shell::new(proxy.clone());

                            let mut compositor =
                                <P::Renderer as compositor::Default>::Compositor::new(
                                    backend_settings,
                                    display_handle,
                                    window,
                                    shell,
                                )
                                .await;

                            if let Ok(compositor) = &mut compositor {
                                for font in default_fonts {
                                    compositor.load_font(font.clone());
                                }
                            }

                            compositor_sender
                                .send(compositor)
                                .ok()
                                .expect("Send compositor");

                            // HACK! Send a proxy event on completion to trigger
                            // a runtime re-poll
                            // TODO: Send compositor through proxy (?)
                            {
                                let (sender, _receiver) = oneshot::channel();

                                proxy.send_action(Action::Window(
                                    runtime::window::Action::GetLatest(sender),
                                ));
                            }
                        }
                    };

                    #[cfg(target_arch = "wasm32")]
                    wasm_bindgen_futures::spawn_local(create_compositor);

                    #[cfg(not(target_arch = "wasm32"))]
                    runtime.block_on(create_compositor);

                    match compositor_receiver.await.expect("Wait for compositor") {
                        Ok(new_compositor) => {
                            compositor = Some(new_compositor);
                        }
                        Err(error) => {
                            let _ = control_sender.start_send(Control::Crash(error.into()));
                            continue;
                        }
                    }
                }

                // On Linux, keep the desktop preference supplied by mundy.
                // X11 reports no window theme; Wayland reports its decoration theme.
                #[cfg(not(all(feature = "linux-theme-detection", target_os = "linux")))]
                {
                    let window_theme = window
                        .theme()
                        .map(conversion::theme_mode)
                        .unwrap_or_default();

                    if system_theme != window_theme {
                        system_theme = window_theme;

                        runtime.broadcast(subscription::Event::SystemThemeChanged(window_theme));
                    }
                }

                let is_first = window_manager.is_empty();
                let window = window_manager.insert(
                    id,
                    window,
                    &program,
                    compositor.as_mut().expect("Compositor must be initialized"),
                    proxy.clone(),
                    renderer_settings,
                    exit_on_close_request,
                    system_theme,
                );

                window
                    .raw
                    .set_theme(conversion::window_theme(window.state.theme_mode()));

                debug::theme_changed(|| {
                    if is_first {
                        theme::Base::seed(window.state.theme())
                    } else {
                        None
                    }
                });

                let logical_size = window.state.logical_size();

                #[cfg(feature = "hinting")]
                window.renderer.hint(window.state.scale_factor());

                let _ = user_interfaces.insert(
                    id,
                    build_user_interface(
                        &program,
                        user_interface::Cache::default(),
                        &mut window.renderer,
                        logical_size,
                        id,
                    ),
                );
                let _ = ui_caches.insert(id, user_interface::Cache::default());

                // Prepare actual pixels while the HWND is hidden. The native
                // paint handler supplies them during ShowWindow's opening fade,
                // before the ordinary compositor gets its first visible redraw.
                let physical_size = window.state.physical_size();
                if window.first_frame.needs_prepare()
                    && physical_size.width > 0
                    && physical_size.height > 0
                {
                    let started = StdInstant::now();
                    user_interfaces
                        .get_mut(&id)
                        .expect("Get initial user interface")
                        .draw(
                            &mut window.renderer,
                            window.state.theme(),
                            &renderer::Style {
                                text_color: window.state.text_color(),
                            },
                            window.state.cursor(),
                        );
                    let pixels = compositor
                        .as_mut()
                        .expect("Compositor must be initialized")
                        .screenshot(
                            &mut window.renderer,
                            window.state.viewport(),
                            window.state.background_color(),
                        );
                    window
                        .first_frame
                        .prepare(&window.raw, physical_size, pixels);
                    trace::event(
                        "winit_first_frame_prepared",
                        started.elapsed().as_micros(),
                        format_args!("window={id}"),
                    );
                }

                if make_visible {
                    window.raw.set_visible(true);
                }

                events.push((
                    id,
                    core::Event::Window(window::Event::Opened {
                        position: window.position(),
                        size: window.state.logical_size(),
                        scale_factor: window.raw.scale_factor() as f32,
                    }),
                ));

                let _ = on_open.send(id);
                is_window_opening = false;
            }
            Event::EventLoopAwakened(event) => {
                match event {
                    event::Event::NewEvents(event::StartCause::Init) => {
                        for (_id, window) in window_manager.iter_mut() {
                            window.raw.request_redraw();
                        }
                    }
                    event::Event::NewEvents(event::StartCause::ResumeTimeReached { .. }) => {
                        let now = Instant::now();

                        for (_id, window) in window_manager.iter_mut() {
                            if let Some(redraw_at) = window.redraw_at
                                && redraw_at <= now
                            {
                                window.raw.request_redraw();
                                window.redraw_at = None;
                            }
                        }

                        if let Some(redraw_at) = window_manager.redraw_at() {
                            let _ = control_sender
                                .start_send(Control::ChangeFlow(ControlFlow::WaitUntil(redraw_at)));
                        } else {
                            let _ =
                                control_sender.start_send(Control::ChangeFlow(ControlFlow::Wait));
                        }
                    }
                    event::Event::PlatformSpecific(event::PlatformSpecific::MacOS(
                        event::MacOS::ReceivedUrl(url),
                    )) => {
                        runtime.broadcast(subscription::Event::PlatformSpecific(
                            subscription::PlatformSpecific::MacOS(
                                subscription::MacOS::ReceivedUrl(url),
                            ),
                        ));
                    }
                    event::Event::UserEvent(action) => {
                        run_action(
                            action,
                            &program,
                            &proxy,
                            &mut runtime,
                            &mut compositor,
                            &mut preparing_backend_handoff,
                            &mut pending_backend_handoff,
                            &mut awaiting_first_present_backend_handoff,
                            &mut events,
                            &mut messages,
                            &mut clipboard,
                            &mut control_sender,
                            &mut user_interfaces,
                            &mut window_manager,
                            &mut ui_caches,
                            &mut is_window_opening,
                            &mut system_theme,
                            &mut renderer_settings,
                        );
                        actions += 1;
                    }
                    event::Event::WindowEvent {
                        window_id: id,
                        event: event::WindowEvent::RedrawRequested,
                        ..
                    } => {
                        let Some(mut current_compositor) = compositor.as_mut() else {
                            continue;
                        };

                        let Some((id, mut window)) = window_manager.get_mut_alias(id) else {
                            continue;
                        };

                        let physical_size = window.state.physical_size();
                        let mut logical_size = window.state.logical_size();

                        if physical_size.width == 0 || physical_size.height == 0 {
                            continue;
                        }

                        let redraw_started = StdInstant::now();
                        let redraw_delta_us = trace::frame_delta_us(id, redraw_started);
                        if trace::enabled() {
                            trace::event(
                                "winit_redraw_start",
                                0,
                                format_args!(
                                    "window={id} frame_delta_us={} frame_fps={:.1} physical={}x{} logical={:.1}x{:.1} scale={:.2}",
                                    redraw_delta_us
                                        .map(|delta| delta.to_string())
                                        .unwrap_or_else(|| String::from("first")),
                                    redraw_delta_us
                                        .map(|delta| 1_000_000.0 / delta.max(1) as f64)
                                        .unwrap_or(0.0),
                                    physical_size.width,
                                    physical_size.height,
                                    logical_size.width,
                                    logical_size.height,
                                    window.state.scale_factor(),
                                ),
                            );
                        }

                        // Window was resized between redraws
                        if window.surface_version != window.state.surface_version() {
                            #[cfg(feature = "hinting")]
                            window.renderer.hint(window.state.scale_factor());

                            let ui = user_interfaces.remove(&id).expect("Remove user interface");

                            let layout_span = debug::layout(id);
                            let _ = user_interfaces
                                .insert(id, ui.relayout(logical_size, &mut window.renderer));
                            layout_span.finish();

                            current_compositor.configure_surface(
                                &mut window.surface,
                                physical_size.width,
                                physical_size.height,
                            );

                            window.surface_version = window.state.surface_version();
                        }

                        let redraw_event =
                            core::Event::Window(window::Event::RedrawRequested(Instant::now()));

                        let cursor = window.state.cursor();

                        let mut interface =
                            user_interfaces.get_mut(&id).expect("Get user interface");

                        let interact_span = debug::interact(id);
                        let interact_started = StdInstant::now();
                        let mut redraw_count = 0;

                        let state = loop {
                            let message_count = messages.len();
                            let (state, _) = interface.update(
                                &window.raw,
                                &window.waker,
                                slice::from_ref(&redraw_event),
                                cursor,
                                &mut window.renderer,
                                &mut messages,
                            );

                            if message_count == messages.len() && !state.has_layout_changed() {
                                break state;
                            }

                            if redraw_count >= 2 {
                                log::warn!(
                                    "More than 3 consecutive RedrawRequested events \
                                    produced layout invalidation"
                                );

                                break state;
                            }

                            redraw_count += 1;

                            if !messages.is_empty()
                                || matches!(state, user_interface::State::Outdated)
                            {
                                let caches: FxHashMap<_, _> =
                                    ManuallyDrop::into_inner(user_interfaces)
                                        .into_iter()
                                        .map(|(id, interface)| (id, interface.into_cache()))
                                        .collect();

                                let actions = update(&mut program, &mut runtime, &mut messages);

                                user_interfaces = ManuallyDrop::new(build_user_interfaces(
                                    &program,
                                    &mut window_manager,
                                    caches,
                                    &mut proxy,
                                ));

                                for action in actions {
                                    // Defer all window actions to avoid compositor
                                    // race conditions while redrawing
                                    if let Action::Window(_) = action {
                                        proxy.send_action(action);
                                        continue;
                                    }

                                    run_action(
                                        action,
                                        &program,
                                        &proxy,
                                        &mut runtime,
                                        &mut compositor,
                                        &mut preparing_backend_handoff,
                                        &mut pending_backend_handoff,
                                        &mut awaiting_first_present_backend_handoff,
                                        &mut events,
                                        &mut messages,
                                        &mut clipboard,
                                        &mut control_sender,
                                        &mut user_interfaces,
                                        &mut window_manager,
                                        &mut ui_caches,
                                        &mut is_window_opening,
                                        &mut system_theme,
                                        &mut renderer_settings,
                                    );
                                }

                                for (window_id, window) in window_manager.iter_mut() {
                                    // We are already redrawing this window
                                    if window_id == id {
                                        continue;
                                    }

                                    window.raw.request_redraw();
                                }

                                let Some(next_compositor) = compositor.as_mut() else {
                                    continue 'next_event;
                                };

                                current_compositor = next_compositor;
                                window = window_manager.get_mut(id).unwrap();

                                // Window scale factor changed during a redraw request
                                if logical_size != window.state.logical_size() {
                                    logical_size = window.state.logical_size();

                                    log::debug!(
                                        "Window scale factor changed during a redraw request"
                                    );

                                    let ui =
                                        user_interfaces.remove(&id).expect("Remove user interface");

                                    let layout_span = debug::layout(id);
                                    let _ = user_interfaces.insert(
                                        id,
                                        ui.relayout(logical_size, &mut window.renderer),
                                    );
                                    layout_span.finish();
                                }

                                interface = user_interfaces.get_mut(&id).unwrap();
                            }
                        };
                        interact_span.finish();
                        let interact_us = interact_started.elapsed().as_micros();
                        if trace::enabled() {
                            trace::event(
                                "winit_redraw_interact",
                                interact_us,
                                format_args!("window={id} redraw_loops={}", redraw_count + 1),
                            );
                        }

                        let draw_span = debug::draw(id);
                        let draw_started = StdInstant::now();
                        interface.draw(
                            &mut window.renderer,
                            window.state.theme(),
                            &renderer::Style {
                                text_color: window.state.text_color(),
                            },
                            cursor,
                        );
                        draw_span.finish();
                        let draw_us = draw_started.elapsed().as_micros();
                        if trace::enabled() {
                            trace::event("winit_redraw_draw", draw_us, format_args!("window={id}"));
                        }

                        if let user_interface::State::Updated {
                            redraw_request,
                            input_method,
                            mouse_interaction,
                            clipboard: clipboard_requests,
                            ..
                        } = state
                        {
                            window.request_redraw(redraw_request);
                            window.request_input_method(input_method);
                            window.update_mouse(mouse_interaction);

                            run_clipboard(&mut proxy, &mut clipboard, clipboard_requests, id);
                        }

                        runtime.broadcast(subscription::Event::Interaction {
                            window: id,
                            event: redraw_event,
                            status: core::event::Status::Ignored,
                        });

                        window.draw_preedit();

                        let present_span = debug::present(id);
                        let present_started = StdInstant::now();
                        let present_frame_sequence =
                            next_present_frame_sequence(&mut present_frame_sequences, id);
                        let strict_present_phase = awaiting_first_present_backend_handoff
                            .as_ref()
                            .filter(|awaiting| awaiting.windows.contains(&id))
                            .map(|_| backend::StrictHandoffPhase::AwaitingFirstPresent);
                        let strict_present_information =
                            strict_present_phase.map(|_| current_compositor.information());
                        let present_result = current_compositor.present(
                            &mut window.renderer,
                            &mut window.surface,
                            window.state.viewport(),
                            window.state.background_color(),
                            || window.raw.pre_present_notify(),
                        );
                        let present_us = present_started.elapsed().as_micros();
                        let strict_present_evidence = strict_present_information
                            .as_ref()
                            .zip(strict_present_phase)
                            .map(|(information, phase)| {
                                let status = match &present_result {
                                    Ok(()) => backend::PresentStatus::Presented,
                                    Err(error) => backend::PresentStatus::Failed(
                                        strict_surface_failure(error),
                                    ),
                                };

                                strict_present_evidence_from_information(
                                    id,
                                    present_frame_sequence,
                                    information,
                                    status,
                                    phase,
                                )
                            });

                        if let Some(evidence) = &strict_present_evidence {
                            trace_strict_present_evidence(evidence, present_us);
                        }

                        if trace::enabled() {
                            let status = present_result
                                .as_ref()
                                .map(|_| String::from("ok"))
                                .unwrap_or_else(|error| format!("{error:?}"));
                            trace::event(
                                "winit_redraw_frame",
                                redraw_started.elapsed().as_micros(),
                                format_args!(
                                    "window={id} frame_delta_us={} frame_fps={:.1} interact_us={interact_us} draw_us={draw_us} present_us={present_us} status={status}",
                                    redraw_delta_us
                                        .map(|delta| delta.to_string())
                                        .unwrap_or_else(|| String::from("first")),
                                    redraw_delta_us
                                        .map(|delta| 1_000_000.0 / delta.max(1) as f64)
                                        .unwrap_or(0.0),
                                ),
                            );
                        }

                        let mut commit_pending_handoff = false;
                        let mut backend_handoff_after_present = None;

                        match present_result {
                            Ok(()) => {
                                window.first_frame.presented(&window.raw);
                                present_span.finish();

                                if let Some(evidence) = strict_present_evidence {
                                    if let Some(awaiting) =
                                        awaiting_first_present_backend_handoff.take()
                                    {
                                        backend_handoff_after_present =
                                            Some(BackendHandoffAfterPresent::Complete {
                                                awaiting,
                                                evidence,
                                            });
                                    }
                                }

                                commit_pending_handoff = pending_backend_handoff.is_some();
                            }
                            Err(error) => {
                                if let Some((awaiting, evidence)) = strict_present_evidence
                                    .and_then(|evidence| {
                                        awaiting_first_present_backend_handoff
                                            .take()
                                            .map(|awaiting| (awaiting, evidence))
                                    })
                                {
                                    let message = format!(
                                        "first present failed after backend handoff commit: {error}"
                                    );

                                    present_span.finish();

                                    backend_handoff_after_present =
                                        Some(BackendHandoffAfterPresent::Failed {
                                            awaiting,
                                            evidence,
                                            message,
                                        });
                                } else {
                                    match error {
                                        compositor::SurfaceError::OutOfMemory => {
                                            // This is an unrecoverable error.
                                            panic!("{error:?}");
                                        }
                                        compositor::SurfaceError::Occluded => {
                                            present_span.finish();

                                            // Do nothing and wait for window to become visible again
                                        }
                                        compositor::SurfaceError::Timeout => {
                                            present_span.finish();

                                            window.raw.request_redraw();
                                        }
                                        compositor::SurfaceError::Lost
                                        | compositor::SurfaceError::Outdated
                                        | compositor::SurfaceError::Other => {
                                            present_span.finish();

                                            // Reconfigure or recreate at most once a second,
                                            // and do not request a redraw in between, so a
                                            // surface that keeps failing cannot spin the loop.
                                            let due = window.surface_error_at.is_none_or(|at| {
                                                at.elapsed() > Duration::from_secs(1)
                                            });

                                            if due {
                                                window.surface_error_at = Some(Instant::now());

                                                log::warn!(
                                                    "Error {error:?} when presenting surface. Recovering it."
                                                );

                                                let physical_size = window.state.physical_size();

                                                if matches!(
                                                    error,
                                                    compositor::SurfaceError::Outdated
                                                ) {
                                                    current_compositor.configure_surface(
                                                        &mut window.surface,
                                                        physical_size.width,
                                                        physical_size.height,
                                                    );
                                                } else {
                                                    window.surface = current_compositor
                                                        .create_surface(
                                                            window.raw.clone(),
                                                            physical_size.width,
                                                            physical_size.height,
                                                        );
                                                }

                                                window.raw.request_redraw();
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(after_present) = backend_handoff_after_present {
                            match after_present {
                                BackendHandoffAfterPresent::Complete { awaiting, evidence } => {
                                    complete_awaiting_first_present_backend_handoff(
                                        awaiting,
                                        evidence,
                                        &mut awaiting_first_present_backend_handoff,
                                        &mut compositor,
                                        &mut window_manager,
                                        renderer_settings,
                                    );
                                }
                                BackendHandoffAfterPresent::Failed {
                                    awaiting,
                                    evidence,
                                    message,
                                } => {
                                    fail_awaiting_first_present_backend_handoff(
                                        awaiting,
                                        evidence,
                                        message,
                                        &mut compositor,
                                        &mut window_manager,
                                        renderer_settings,
                                    );
                                }
                            }
                        }

                        if commit_pending_handoff {
                            if pending_backend_handoff
                                .as_ref()
                                .and_then(|pending| pending.commit_after)
                                .is_some_and(|commit_after| StdInstant::now() < commit_after)
                            {
                                for (_id, window) in window_manager.iter_mut() {
                                    let delay = pending_backend_handoff
                                        .as_ref()
                                        .and_then(|pending| pending.commit_after)
                                        .map(|deadline| {
                                            deadline.saturating_duration_since(StdInstant::now())
                                        })
                                        .unwrap_or_default();
                                    let next = Instant::now() + delay;
                                    if window.redraw_at.is_none_or(|scheduled| scheduled > next) {
                                        window.request_redraw(window::RedrawRequest::At(next));
                                    }
                                }
                            } else if let Some(pending) = pending_backend_handoff.take() {
                                commit_backend_handoff(
                                    pending,
                                    &mut compositor,
                                    &mut window_manager,
                                    renderer_settings,
                                    &mut awaiting_first_present_backend_handoff,
                                );
                            }
                        }
                    }
                    event::Event::WindowEvent {
                        event: window_event,
                        window_id,
                    } => {
                        if !is_daemon
                            && matches!(window_event, winit::event::WindowEvent::Destroyed)
                            && !is_window_opening
                            && window_manager.is_empty()
                        {
                            cancel_backend_handoffs(
                                &mut preparing_backend_handoff,
                                &mut pending_backend_handoff,
                                &mut awaiting_first_present_backend_handoff,
                                &mut compositor,
                                &mut window_manager,
                                renderer_settings,
                                "backend handoff cancelled by last-window close",
                            );

                            control_sender
                                .start_send(Control::Exit)
                                .expect("Send control action");

                            continue;
                        }

                        let Some((id, window)) = window_manager.get_mut_alias(window_id) else {
                            continue;
                        };

                        match window_event {
                            winit::event::WindowEvent::Resized(_)
                            | winit::event::WindowEvent::Occluded(false) => {
                                window.raw.request_redraw();
                            }
                            winit::event::WindowEvent::ThemeChanged(theme) => {
                                let mode = conversion::theme_mode(theme);

                                if mode != system_theme {
                                    system_theme = mode;

                                    runtime
                                        .broadcast(subscription::Event::SystemThemeChanged(mode));
                                }
                            }
                            _ => {}
                        }

                        if matches!(window_event, winit::event::WindowEvent::CloseRequested)
                            && window.exit_on_close_request
                        {
                            run_action(
                                Action::Window(runtime::window::Action::Close(id)),
                                &program,
                                &proxy,
                                &mut runtime,
                                &mut compositor,
                                &mut preparing_backend_handoff,
                                &mut pending_backend_handoff,
                                &mut awaiting_first_present_backend_handoff,
                                &mut events,
                                &mut messages,
                                &mut clipboard,
                                &mut control_sender,
                                &mut user_interfaces,
                                &mut window_manager,
                                &mut ui_caches,
                                &mut is_window_opening,
                                &mut system_theme,
                                &mut renderer_settings,
                            );
                        } else {
                            window.state.update(&program, &window.raw, &window_event);

                            if let Some(event) = conversion::window_event(
                                window_event,
                                window.state.scale_factor(),
                                window.state.modifiers(),
                            ) {
                                events.push((id, event));
                            }
                        }
                    }
                    event::Event::AboutToWait => {
                        if actions > 0 {
                            proxy.free_slots(actions);
                            actions = 0;
                        }

                        progress_awaiting_first_present_backend_handoff(
                            &mut awaiting_first_present_backend_handoff,
                            &mut compositor,
                            &mut window_manager,
                            &mut present_frame_sequences,
                            &mut user_interfaces,
                            renderer_settings,
                        );

                        if awaiting_first_present_backend_handoff.is_none()
                            && events.is_empty()
                            && messages.is_empty()
                            && window_manager.is_idle()
                        {
                            continue;
                        }

                        let mut uis_stale = false;

                        for (id, window) in window_manager.iter_mut() {
                            let interact_span = debug::interact(id);
                            let mut window_events = vec![];

                            events.retain(|(window_id, event)| {
                                if *window_id == id {
                                    window_events.push(event.clone());
                                    false
                                } else {
                                    true
                                }
                            });

                            if window_events.is_empty() {
                                continue;
                            }

                            let (ui_state, statuses) = user_interfaces
                                .get_mut(&id)
                                .expect("Get user interface")
                                .update(
                                    &window.raw,
                                    &window.waker,
                                    &window_events,
                                    window.state.cursor(),
                                    &mut window.renderer,
                                    &mut messages,
                                );

                            #[cfg(feature = "unconditional-rendering")]
                            window.request_redraw(window::RedrawRequest::NextFrame);

                            match ui_state {
                                user_interface::State::Updated {
                                    redraw_request: _redraw_request,
                                    mouse_interaction,
                                    clipboard: clipboard_requests,
                                    ..
                                } => {
                                    window.update_mouse(mouse_interaction);

                                    #[cfg(not(feature = "unconditional-rendering"))]
                                    window.request_redraw(_redraw_request);

                                    run_clipboard(
                                        &mut proxy,
                                        &mut clipboard,
                                        clipboard_requests,
                                        id,
                                    );
                                }
                                user_interface::State::Outdated => {
                                    uis_stale = true;
                                }
                            }

                            for (event, status) in window_events.into_iter().zip(statuses) {
                                runtime.broadcast(subscription::Event::Interaction {
                                    window: id,
                                    event,
                                    status,
                                });
                            }

                            interact_span.finish();
                        }

                        for (id, event) in events.drain(..) {
                            runtime.broadcast(subscription::Event::Interaction {
                                window: id,
                                event,
                                status: core::event::Status::Ignored,
                            });
                        }

                        if !messages.is_empty() || uis_stale {
                            let cached_interfaces: FxHashMap<_, _> =
                                ManuallyDrop::into_inner(user_interfaces)
                                    .into_iter()
                                    .map(|(id, ui)| (id, ui.into_cache()))
                                    .collect();

                            let actions = update(&mut program, &mut runtime, &mut messages);

                            user_interfaces = ManuallyDrop::new(build_user_interfaces(
                                &program,
                                &mut window_manager,
                                cached_interfaces,
                                &mut proxy,
                            ));

                            for action in actions {
                                run_action(
                                    action,
                                    &program,
                                    &proxy,
                                    &mut runtime,
                                    &mut compositor,
                                    &mut preparing_backend_handoff,
                                    &mut pending_backend_handoff,
                                    &mut awaiting_first_present_backend_handoff,
                                    &mut events,
                                    &mut messages,
                                    &mut clipboard,
                                    &mut control_sender,
                                    &mut user_interfaces,
                                    &mut window_manager,
                                    &mut ui_caches,
                                    &mut is_window_opening,
                                    &mut system_theme,
                                    &mut renderer_settings,
                                );
                            }

                            for (_id, window) in window_manager.iter_mut() {
                                window.raw.request_redraw();
                            }
                        }

                        if let Some(redraw_at) = window_manager.redraw_at() {
                            let _ = control_sender
                                .start_send(Control::ChangeFlow(ControlFlow::WaitUntil(redraw_at)));
                        } else {
                            let _ =
                                control_sender.start_send(Control::ChangeFlow(ControlFlow::Wait));
                        }
                    }
                    _ => {}
                }
            }
            Event::Exit => {
                cancel_backend_handoffs(
                    &mut preparing_backend_handoff,
                    &mut pending_backend_handoff,
                    &mut awaiting_first_present_backend_handoff,
                    &mut compositor,
                    &mut window_manager,
                    renderer_settings,
                    "backend handoff cancelled by event loop exit",
                );

                break;
            }
        }
    }

    let _ = ManuallyDrop::into_inner(user_interfaces);
}

fn commit_backend_handoff<P, C>(
    pending: PendingBackendHandoff<C>,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
    awaiting_first_present_backend_handoff: &mut Option<AwaitingFirstPresentBackendHandoff<P, C>>,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let PendingBackendHandoff {
        compositor: mut prepared_compositor,
        mut renderers,
        sender,
        phase,
        commit_after: _,
    } = pending;

    log::info!("Committing prepared backend handoff: {phase:?}");

    if trace::enabled() {
        trace::event(
            "backend_handoff_commit_start",
            0,
            format_args!("phase={phase:?}"),
        );
    }

    if let Some(error) = injected_backend_failure("commit") {
        if trace::enabled() {
            trace::event(
                "backend_handoff_commit_failed",
                0,
                format_args!("phase={phase:?} error={error:?}"),
            );
        }

        let _ = sender.send(Err(strict_backend_error(
            phase,
            backend::StrictHandoffFailureCategory::Commit,
            backend::StrictRollbackStatus::NotNeeded,
            error,
        )));
        return;
    }

    let awaiting_windows = window_manager
        .iter_mut()
        .map(|(id, _window)| id)
        .collect::<Vec<_>>();

    if awaiting_windows.is_empty() {
        let _ = sender.send(Err(strict_handoff_error(
            backend::StrictHandoffPhase::CommitPending,
            backend::StrictHandoffFailureCategory::NoActiveWindow,
            backend::StrictRollbackStatus::NotNeeded,
            "cannot commit backend handoff without an active window",
        )));

        return;
    }

    let Some(retained_compositor) = compositor.take() else {
        let _ = sender.send(Err(strict_handoff_error(
            phase,
            backend::StrictHandoffFailureCategory::Commit,
            backend::StrictRollbackStatus::NotAvailable,
            "cannot retain active compositor for backend handoff rollback",
        )));
        return;
    };

    graphics::cache::invalidate_all();

    let mut retained_windows = Vec::new();

    window_manager.replace_with_id(|id, window| {
        let size = window.state.physical_size();

        let rendering_state = window::RenderingState {
            renderer: renderers
                .remove(&id)
                .unwrap_or_else(|| prepared_compositor.create_renderer(renderer_settings)),
            surface: prepared_compositor.create_surface(
                window.raw.clone(),
                size.width,
                size.height,
            ),
            surface_version: window.state.surface_version(),
        };

        let (window, retained_window) = window.replace_rendering_state(rendering_state);
        retained_windows.push((id, retained_window));
        window
    });

    *compositor = Some(prepared_compositor);

    let mut windows_redrawn = 0;

    for (_id, window) in window_manager.iter_mut() {
        window.request_redraw(window::RedrawRequest::NextFrame);
        windows_redrawn += 1;
    }

    if trace::enabled() {
        trace::event(
            "backend_handoff_commit_complete",
            0,
            format_args!("phase={phase:?} windows_redrawn={windows_redrawn}"),
        );
    }

    *awaiting_first_present_backend_handoff = Some(AwaitingFirstPresentBackendHandoff {
        sender,
        windows: awaiting_windows,
        evidence: Vec::new(),
        retained: RetainedBackendHandoff {
            compositor: retained_compositor,
            windows: retained_windows,
        },
        first_present_started_at: StdInstant::now(),
        missing_progress_passes: 0,
    });
}

fn drive_preparing_backend_handoff<P, C>(
    preparing_backend_handoff: &mut Option<PreparingBackendHandoff<C>>,
    pending_backend_handoff: &mut Option<PendingBackendHandoff<C>>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
    events: &mut Vec<(window::Id, core::Event)>,
    user_interfaces: &mut FxHashMap<
        window::Id,
        UserInterface<'_, P::Message, P::Theme, P::Renderer>,
    >,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let Some(preparing) = preparing_backend_handoff else {
        return;
    };

    if preparing.warming.is_some() {
        drive_warming_backend_handoff(
            preparing_backend_handoff,
            pending_backend_handoff,
            window_manager,
            events,
        );
        return;
    }

    let result = match preparing.receiver.try_recv() {
        Ok(Some(result)) => result,
        Ok(None) => return,
        Err(_) => {
            let preparing = preparing_backend_handoff.take().expect("preparing handoff");
            let _ = preparing.sender.send(Err(strict_handoff_error(
                backend::StrictHandoffPhase::Preparing,
                backend::StrictHandoffFailureCategory::Prepare,
                backend::StrictRollbackStatus::NotNeeded,
                "backend preparation worker stopped without a result",
            )));
            return;
        }
    };

    let Some(preparing) = preparing_backend_handoff.take() else {
        return;
    };

    let phase = backend::StrictHandoffPhase::Warming;

    log::info!("Warming backend handoff: {phase:?}");

    let mut prepared_compositor = match result {
        Ok(compositor) => compositor,
        Err(error) => {
            if trace::enabled() {
                trace::event(
                    "backend_handoff_prepare_failed",
                    0,
                    format_args!(
                        "phase={:?} error={error:?}",
                        backend::StrictHandoffPhase::Preparing
                    ),
                );
            }

            let _ = preparing.sender.send(Err(strict_backend_error(
                backend::StrictHandoffPhase::Preparing,
                backend::StrictHandoffFailureCategory::Prepare,
                backend::StrictRollbackStatus::NotNeeded,
                error,
            )));
            return;
        }
    };

    let information = prepared_compositor.information();
    let renderer_family = strict_renderer_family(&information);
    let (viewport, _background_color) = window_manager
        .first()
        .map(|window| {
            let physical_size = window.state.physical_size();
            let width = physical_size.width.max(1);
            let height = physical_size.height.max(1);
            let scale_factor = window.state.scale_factor();
            let viewport = Viewport::with_physical_size(Size::new(width, height), scale_factor);

            (viewport, window.state.background_color())
        })
        .unwrap_or_else(|| {
            (
                Viewport::with_physical_size(Size::new(1, 1), 1.0),
                core::Color::TRANSPARENT,
            )
        });

    let warm_size = viewport.physical_size();

    if trace::enabled() {
        trace::event(
            "backend_handoff_warm_start",
            0,
            format_args!(
                "phase={phase:?} renderer_family={renderer_family:?} adapter={} backend={} width={} height={} passes=0 elapsed_us=0 submission_completed=false",
                information.adapter, information.backend, warm_size.width, warm_size.height,
            ),
        );
    }

    if let Some(error) = injected_backend_failure("warm") {
        if trace::enabled() {
            trace::event(
                "backend_handoff_warm_failed",
                0,
                format_args!(
                    "phase={phase:?} renderer_family={renderer_family:?} adapter={} backend={} width={} height={} passes=0 elapsed_us=0 submission_completed=false error={error:?}",
                    information.adapter, information.backend, warm_size.width, warm_size.height,
                ),
            );
        }

        let _ = preparing.sender.send(Err(strict_backend_error(
            phase,
            backend::StrictHandoffFailureCategory::WarmUp,
            backend::StrictRollbackStatus::NotNeeded,
            error,
        )));
        return;
    }

    let warm_started = StdInstant::now();
    let mut renderers = FxHashMap::default();
    // Geometry caches encode the renderer family. Isolate the temporary GPU
    // draw from the software frames that continue until commit.
    graphics::cache::invalidate_all();
    let warm_result = (|| {
        for (id, window) in window_manager.iter_mut() {
            let mut renderer = prepared_compositor.create_renderer(renderer_settings);
            #[cfg(feature = "hinting")]
            {
                use crate::core::Renderer as _;
                renderer.hint(window.state.scale_factor());
            }
            let interface = user_interfaces.get_mut(&id).ok_or_else(|| {
                compositor::OffscreenWarmUpError::Failed(
                    "window UI is unavailable for warm-up".into(),
                )
            })?;
            interface.draw(
                &mut renderer,
                window.state.theme(),
                &renderer::Style {
                    text_color: window.state.text_color(),
                },
                window.state.cursor(),
            );
            let size = window.state.physical_size();
            let viewport = Viewport::with_physical_size(
                Size::new(size.width.max(1), size.height.max(1)),
                window.state.scale_factor(),
            );
            prepared_compositor.begin_warm_up_offscreen(
                &mut renderer,
                &viewport,
                window.state.background_color(),
            )?;
            let _ = renderers.insert(id, renderer);
        }
        Ok::<_, compositor::OffscreenWarmUpError>(())
    })();
    graphics::cache::invalidate_all();
    if let Err(error) = warm_result {
        let elapsed_us = warm_started.elapsed().as_micros();
        trace_offscreen_warm_up_failure(
            phase,
            renderer_family,
            Some(&information.adapter),
            Some(&information.backend),
            warm_size.width,
            warm_size.height,
            elapsed_us,
            &error,
        );

        let _ = preparing.sender.send(Err(strict_handoff_error(
            phase,
            backend::StrictHandoffFailureCategory::WarmUp,
            backend::StrictRollbackStatus::NotNeeded,
            error.to_string(),
        )));
        return;
    }

    *preparing_backend_handoff = Some(PreparingBackendHandoff {
        receiver: preparing.receiver,
        sender: preparing.sender,
        warming: Some(WarmingBackendHandoff {
            compositor: prepared_compositor,
            remaining: renderers.keys().copied().collect(),
            renderers,
            started: warm_started,
        }),
    });
    drive_warming_backend_handoff(
        preparing_backend_handoff,
        pending_backend_handoff,
        window_manager,
        events,
    );
}

fn drive_warming_backend_handoff<P, C>(
    preparing_backend_handoff: &mut Option<PreparingBackendHandoff<C>>,
    pending_backend_handoff: &mut Option<PendingBackendHandoff<C>>,
    window_manager: &mut window::Manager<P, C>,
    events: &mut Vec<(window::Id, core::Event)>,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let Some(preparing) = preparing_backend_handoff.as_mut() else {
        return;
    };
    let Some(warming) = preparing.warming.as_mut() else {
        return;
    };
    let phase = backend::StrictHandoffPhase::Warming;
    match poll_warm_renderers(&mut warming.renderers, &mut warming.remaining, |renderer| {
        warming.compositor.poll_warm_up_offscreen(renderer)
    }) {
        Ok(true) => {}
        Ok(false) => {
            for (_, window) in window_manager.iter_mut() {
                let next = Instant::now() + std::time::Duration::from_millis(16);
                if window.redraw_at.is_none_or(|scheduled| scheduled > next) {
                    window.request_redraw(window::RedrawRequest::At(next));
                }
            }
            return;
        }
        Err(error) => {
            let information = warming.compositor.information();
            let size = warming
                .remaining
                .last()
                .and_then(|id| window_manager.get(*id))
                .map(|window| window.state.physical_size())
                .unwrap_or(Size::new(0, 0));
            trace_offscreen_warm_up_failure(
                phase,
                strict_renderer_family(&information),
                Some(&information.adapter),
                Some(&information.backend),
                size.width,
                size.height,
                warming.started.elapsed().as_micros(),
                &error,
            );
            let preparing = preparing_backend_handoff.take().expect("preparing handoff");
            let _ = preparing.sender.send(Err(strict_handoff_error(
                phase,
                backend::StrictHandoffFailureCategory::WarmUp,
                backend::StrictRollbackStatus::NotNeeded,
                error.to_string(),
            )));
            return;
        }
    }
    let preparing = preparing_backend_handoff.take().expect("preparing handoff");
    let warming = preparing.warming.expect("warmed renderers");

    let phase = backend::StrictHandoffPhase::CommitPending;

    if trace::enabled() {
        trace::event(
            "backend_handoff_commit_pending",
            0,
            format_args!("phase={phase:?}"),
        );
    }

    *pending_backend_handoff = Some(PendingBackendHandoff {
        compositor: warming.compositor,
        renderers: warming.renderers,
        sender: preparing.sender,
        phase,
        commit_after: backend_commit_pending_delay_deadline(),
    });

    if should_close_during_commit_pending() {
        let extra_window = window_manager.iter_mut().nth(1).map(|(id, _window)| id);

        if let Some(id) = extra_window {
            let removed = window_manager.remove(id).is_some();

            if removed {
                if trace::enabled() {
                    trace::event(
                        "winit_window_close",
                        0,
                        format_args!("window={id} source=commit-pending-probe"),
                    );
                }

                events.push((id, core::Event::Window(core::window::Event::Closed)));
            }
        }
    }

    for (_id, window) in window_manager.iter_mut() {
        window.raw.request_redraw();
    }
}

fn poll_warm_renderers<R>(
    renderers: &mut FxHashMap<window::Id, R>,
    remaining: &mut Vec<window::Id>,
    mut poll: impl FnMut(
        &mut R,
    ) -> Result<
        Option<compositor::OffscreenWarmUpEvidence>,
        compositor::OffscreenWarmUpError,
    >,
) -> Result<bool, compositor::OffscreenWarmUpError> {
    while let Some(id) = remaining.last().copied() {
        let renderer = renderers.get_mut(&id).expect("warm renderer");
        let Some(evidence) = poll(renderer)? else {
            return Ok(false);
        };
        if !evidence.submission_completed
            || evidence.renderer_family != backend::RendererFamily::Wgpu
        {
            return Err(compositor::OffscreenWarmUpError::Failed(
                "offscreen warm-up did not prove a completed GPU submission".into(),
            ));
        }
        trace_offscreen_warm_up_evidence(
            "backend_handoff_warm_complete",
            backend::StrictHandoffPhase::Warming,
            &evidence,
        );
        let _ = remaining.pop();
    }
    Ok(true)
}

#[cfg(test)]
mod warm_handoff_tests {
    use super::*;

    fn evidence() -> compositor::OffscreenWarmUpEvidence {
        compositor::OffscreenWarmUpEvidence {
            renderer_family: backend::RendererFamily::Wgpu,
            adapter: None,
            backend: None,
            width: 100,
            height: 80,
            passes: 1,
            elapsed_us: 1,
            submission_completed: true,
        }
    }

    #[test]
    fn pending_warmup_preserves_renderers_and_completed_windows_for_commit() {
        let first = window::Id::unique();
        let second = window::Id::unique();
        let mut renderers = FxHashMap::from_iter([(first, 0), (second, 0)]);
        let mut remaining = vec![first, second];
        let mut polls = 0;
        assert!(
            !poll_warm_renderers(&mut renderers, &mut remaining, |state| {
                polls += 1;
                *state += 1;
                Ok((polls == 1).then(evidence))
            })
            .unwrap()
        );
        assert_eq!(remaining, vec![first]);
        assert_eq!(renderers.len(), 2);
        assert_eq!(renderers[&first], 1);
        assert_eq!(renderers[&second], 1);
        assert!(
            poll_warm_renderers(&mut renderers, &mut remaining, |state| {
                *state += 1;
                Ok(Some(evidence()))
            })
            .unwrap()
        );
        assert!(remaining.is_empty());
        assert_eq!(renderers[&first], 2);
        assert_eq!(
            renderers[&second], 1,
            "completed renderer must not be recreated or polled twice"
        );
    }

    #[test]
    fn warmup_timeout_and_unproven_submission_do_not_allow_commit() {
        let id = window::Id::unique();
        let mut renderers = FxHashMap::from_iter([(id, ())]);
        let mut remaining = vec![id];
        assert_eq!(
            poll_warm_renderers(&mut renderers, &mut remaining, |_| Err(
                compositor::OffscreenWarmUpError::Timeout
            )),
            Err(compositor::OffscreenWarmUpError::Timeout)
        );
        let mut unproven = evidence();
        unproven.submission_completed = false;
        assert!(
            poll_warm_renderers(&mut renderers, &mut remaining, |_| Ok(Some(
                unproven.clone()
            )))
            .is_err()
        );
        assert_eq!(remaining, vec![id]);
        assert_eq!(renderers.len(), 1);
    }
}

fn cancel_preparing_backend_handoff<C: Compositor>(
    preparing_backend_handoff: &mut Option<PreparingBackendHandoff<C>>,
    message: &str,
) {
    if let Some(preparing) = preparing_backend_handoff.take() {
        let phase = if preparing.warming.is_some() {
            backend::StrictHandoffPhase::Warming
        } else {
            backend::StrictHandoffPhase::Preparing
        };
        let _ = preparing.sender.send(Err(strict_handoff_error(
            phase,
            backend::StrictHandoffFailureCategory::Cancelled,
            backend::StrictRollbackStatus::NotNeeded,
            message,
        )));
    }
}

fn cancel_pending_backend_handoff<C: Compositor>(
    pending_backend_handoff: &mut Option<PendingBackendHandoff<C>>,
    message: &str,
) {
    if let Some(pending) = pending_backend_handoff.take() {
        let _ = pending.sender.send(Err(strict_handoff_error(
            pending.phase,
            backend::StrictHandoffFailureCategory::Cancelled,
            backend::StrictRollbackStatus::NotNeeded,
            message,
        )));
    }
}

fn restore_retained_backend_handoff<P, C>(
    retained: RetainedBackendHandoff<P, C>,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
) -> backend::StrictRollbackStatus
where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let RetainedBackendHandoff {
        compositor: mut retained_compositor,
        windows: mut retained_windows,
    } = retained;

    window_manager.replace_with_id(|id, window| {
        let rendering_state = retained_windows
            .iter()
            .position(|(window_id, _)| *window_id == id)
            .map(|index| retained_windows.swap_remove(index).1)
            .unwrap_or_else(|| {
                let size = window.state.physical_size();

                window::RenderingState {
                    renderer: retained_compositor.create_renderer(renderer_settings),
                    surface: retained_compositor.create_surface(
                        window.raw.clone(),
                        size.width,
                        size.height,
                    ),
                    surface_version: window.state.surface_version(),
                }
            });

        let (window, _released_rendering_state) = window.replace_rendering_state(rendering_state);
        window
    });

    *compositor = Some(retained_compositor);
    graphics::cache::invalidate_all();

    for (_id, window) in window_manager.iter_mut() {
        window.raw.request_redraw();
    }

    backend::StrictRollbackStatus::Restored
}

#[allow(dead_code)]
fn release_retained_backend_handoff<P, C>(
    _retained: RetainedBackendHandoff<P, C>,
) -> backend::StrictRollbackStatus
where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    backend::StrictRollbackStatus::ReleasedAfterSuccess
}

fn cancel_awaiting_first_present_backend_handoff<P, C>(
    awaiting_first_present_backend_handoff: &mut Option<AwaitingFirstPresentBackendHandoff<P, C>>,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
    message: &str,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    if let Some(awaiting) = awaiting_first_present_backend_handoff.take() {
        let rollback = restore_retained_backend_handoff(
            awaiting.retained,
            compositor,
            window_manager,
            renderer_settings,
        );

        let _ = awaiting.sender.send(Err(strict_handoff_error_with_windows(
            backend::StrictHandoffPhase::AwaitingFirstPresent,
            backend::StrictHandoffFailureCategory::Cancelled,
            rollback,
            strict_closed_before_proof_evidence(&awaiting.windows),
            message,
        )));
    }
}

fn cancel_backend_handoffs<P, C>(
    preparing_backend_handoff: &mut Option<PreparingBackendHandoff<C>>,
    pending_backend_handoff: &mut Option<PendingBackendHandoff<C>>,
    awaiting_first_present_backend_handoff: &mut Option<AwaitingFirstPresentBackendHandoff<P, C>>,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
    message: &str,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    cancel_preparing_backend_handoff(preparing_backend_handoff, message);
    cancel_pending_backend_handoff(pending_backend_handoff, message);
    cancel_awaiting_first_present_backend_handoff(
        awaiting_first_present_backend_handoff,
        compositor,
        window_manager,
        renderer_settings,
        message,
    );
}

fn complete_awaiting_first_present_backend_handoff<P, C>(
    mut awaiting: AwaitingFirstPresentBackendHandoff<P, C>,
    mut evidence: backend::StrictHandoffWindowEvidence,
    awaiting_first_present_backend_handoff: &mut Option<AwaitingFirstPresentBackendHandoff<P, C>>,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    if let Some(error) = injected_backend_failure("first-present") {
        evidence.status = backend::PresentStatus::Failed(backend::SurfaceFailure::Other);

        let rollback = restore_retained_backend_handoff(
            awaiting.retained,
            compositor,
            window_manager,
            renderer_settings,
        );

        let _ = awaiting.sender.send(Err(strict_handoff_error_with_windows(
            backend::StrictHandoffPhase::AwaitingFirstPresent,
            backend::StrictHandoffFailureCategory::FirstPresent,
            rollback,
            vec![evidence],
            error.to_string(),
        )));

        return;
    }

    if evidence.renderer_family != backend::RendererFamily::Wgpu {
        let renderer_family = evidence.renderer_family;
        let rollback = restore_retained_backend_handoff(
            awaiting.retained,
            compositor,
            window_manager,
            renderer_settings,
        );

        let _ = awaiting.sender.send(Err(strict_handoff_error_with_windows(
            backend::StrictHandoffPhase::AwaitingFirstPresent,
            backend::StrictHandoffFailureCategory::RendererEvidenceMissing,
            rollback,
            vec![evidence],
            format!(
                "strict handoff first present was not produced by a proven wgpu renderer: {renderer_family:?}"
            ),
        )));

        return;
    }

    if !awaiting
        .evidence
        .iter()
        .any(|existing| existing.window == evidence.window)
    {
        awaiting.evidence.push(evidence);
        awaiting.missing_progress_passes = 0;
    }

    let Some(missing_windows) = missing_first_present_windows(&awaiting, window_manager) else {
        let rollback = restore_retained_backend_handoff(
            awaiting.retained,
            compositor,
            window_manager,
            renderer_settings,
        );

        let _ = awaiting.sender.send(Err(strict_handoff_error_with_windows(
            backend::StrictHandoffPhase::AwaitingFirstPresent,
            backend::StrictHandoffFailureCategory::NoActiveWindow,
            rollback,
            strict_closed_before_proof_evidence(&awaiting.windows),
            "strict handoff lost all required windows before first present proof completed",
        )));

        return;
    };

    if !missing_windows.is_empty() {
        awaiting.missing_progress_passes = awaiting.missing_progress_passes.saturating_add(1);
        let (elapsed_ms, exhausted) = awaiting_first_present_missing_timeout(&awaiting);

        trace_awaiting_first_present_missing(
            &missing_windows,
            awaiting.missing_progress_passes,
            elapsed_ms,
            AWAITING_FIRST_PRESENT_MISSING_TIMEOUT_MS,
            exhausted,
        );

        if exhausted {
            fail_missing_first_present_backend_handoff(
                awaiting,
                &missing_windows,
                compositor,
                window_manager,
                renderer_settings,
            );
            return;
        }

        for window_id in missing_windows {
            if let Some(window) = window_manager.get_mut(window_id) {
                window.request_redraw(window::RedrawRequest::NextFrame);
            }
        }

        *awaiting_first_present_backend_handoff = Some(awaiting);
        return;
    }

    let rollback = release_retained_backend_handoff(awaiting.retained);

    let _ = awaiting.sender.send(Ok(backend::StrictHandoffResult {
        completed_phase: backend::StrictHandoffPhase::Completed,
        rollback,
        windows: awaiting.evidence,
    }));
}

fn fail_missing_first_present_backend_handoff<P, C>(
    awaiting: AwaitingFirstPresentBackendHandoff<P, C>,
    missing_windows: &[window::Id],
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let mut windows = awaiting.evidence;
    windows.extend(missing_first_present_evidence(missing_windows));

    let rollback = restore_retained_backend_handoff(
        awaiting.retained,
        compositor,
        window_manager,
        renderer_settings,
    );

    let _ = awaiting.sender.send(Err(strict_handoff_error_with_windows(
        backend::StrictHandoffPhase::AwaitingFirstPresent,
        backend::StrictHandoffFailureCategory::RendererEvidenceMissing,
        rollback,
        windows,
        format!(
            "strict handoff first present evidence missing for required live windows: {}",
            missing_windows
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
    )));
}

fn present_missing_first_present_window<P, C>(
    window_id: window::Id,
    awaiting_first_present_backend_handoff: &mut Option<AwaitingFirstPresentBackendHandoff<P, C>>,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    present_frame_sequences: &mut FxHashMap<window::Id, u64>,
    user_interfaces: &mut FxHashMap<
        window::Id,
        UserInterface<'_, P::Message, P::Theme, P::Renderer>,
    >,
    renderer_settings: renderer::Settings,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let Some(current_compositor) = compositor.as_mut() else {
        return;
    };

    let Some(window) = window_manager.get_mut(window_id) else {
        return;
    };

    let physical_size = window.state.physical_size();

    if physical_size.width == 0 || physical_size.height == 0 {
        return;
    }

    let logical_size = window.state.logical_size();

    if window.surface_version != window.state.surface_version() {
        #[cfg(feature = "hinting")]
        window.renderer.hint(window.state.scale_factor());

        let ui = user_interfaces
            .remove(&window_id)
            .expect("Remove user interface");

        let layout_span = debug::layout(window_id);
        let _ = user_interfaces.insert(window_id, ui.relayout(logical_size, &mut window.renderer));
        layout_span.finish();

        current_compositor.configure_surface(
            &mut window.surface,
            physical_size.width,
            physical_size.height,
        );

        window.surface_version = window.state.surface_version();
    }

    let cursor = window.state.cursor();
    let Some(interface) = user_interfaces.get_mut(&window_id) else {
        return;
    };

    interface.draw(
        &mut window.renderer,
        window.state.theme(),
        &renderer::Style {
            text_color: window.state.text_color(),
        },
        cursor,
    );

    window.draw_preedit();

    let present_started = StdInstant::now();
    let present_frame_sequence = next_present_frame_sequence(present_frame_sequences, window_id);
    let information = current_compositor.information();
    let present_result = current_compositor.present(
        &mut window.renderer,
        &mut window.surface,
        window.state.viewport(),
        window.state.background_color(),
        || window.raw.pre_present_notify(),
    );
    let present_us = present_started.elapsed().as_micros();
    if present_result.is_ok() {
        window.first_frame.presented(&window.raw);
    }
    let status = match &present_result {
        Ok(()) => backend::PresentStatus::Presented,
        Err(error) => backend::PresentStatus::Failed(strict_surface_failure(error)),
    };
    let evidence = strict_present_evidence_from_information(
        window_id,
        present_frame_sequence,
        &information,
        status,
        backend::StrictHandoffPhase::AwaitingFirstPresent,
    );

    trace_strict_present_evidence(&evidence, present_us);

    let Some(awaiting) = awaiting_first_present_backend_handoff.take() else {
        return;
    };

    match present_result {
        Ok(()) => complete_awaiting_first_present_backend_handoff(
            awaiting,
            evidence,
            awaiting_first_present_backend_handoff,
            compositor,
            window_manager,
            renderer_settings,
        ),
        Err(error) => fail_awaiting_first_present_backend_handoff(
            awaiting,
            evidence,
            format!("first present failed after backend handoff commit: {error}"),
            compositor,
            window_manager,
            renderer_settings,
        ),
    }
}

fn progress_awaiting_first_present_backend_handoff<P, C>(
    awaiting_first_present_backend_handoff: &mut Option<AwaitingFirstPresentBackendHandoff<P, C>>,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    present_frame_sequences: &mut FxHashMap<window::Id, u64>,
    user_interfaces: &mut FxHashMap<
        window::Id,
        UserInterface<'_, P::Message, P::Theme, P::Renderer>,
    >,
    renderer_settings: renderer::Settings,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    enum Progress {
        Pending {
            missing_windows: Vec<window::Id>,
            progress_passes: u8,
            elapsed_ms: u128,
            exhausted: bool,
        },
        NoLiveWindows,
    }

    let Some(awaiting) = awaiting_first_present_backend_handoff.as_mut() else {
        return;
    };

    let progress = match missing_first_present_windows(awaiting, window_manager) {
        Some(missing_windows) if missing_windows.is_empty() => return,
        Some(missing_windows) => {
            awaiting.missing_progress_passes = awaiting.missing_progress_passes.saturating_add(1);
            let (elapsed_ms, exhausted) = awaiting_first_present_missing_timeout(awaiting);

            Progress::Pending {
                missing_windows,
                progress_passes: awaiting.missing_progress_passes,
                elapsed_ms,
                exhausted,
            }
        }
        None => Progress::NoLiveWindows,
    };

    match progress {
        Progress::Pending {
            missing_windows,
            progress_passes,
            elapsed_ms,
            exhausted,
        } => {
            trace_awaiting_first_present_missing(
                &missing_windows,
                progress_passes,
                elapsed_ms,
                AWAITING_FIRST_PRESENT_MISSING_TIMEOUT_MS,
                exhausted,
            );

            if exhausted {
                let Some(awaiting) = awaiting_first_present_backend_handoff.take() else {
                    return;
                };

                fail_missing_first_present_backend_handoff(
                    awaiting,
                    &missing_windows,
                    compositor,
                    window_manager,
                    renderer_settings,
                );

                return;
            }

            let next_missing_window = missing_windows.first().copied();

            for window_id in missing_windows {
                if let Some(window) = window_manager.get_mut(window_id) {
                    window.request_redraw(window::RedrawRequest::NextFrame);
                }
            }

            if let Some(window_id) = next_missing_window {
                present_missing_first_present_window(
                    window_id,
                    awaiting_first_present_backend_handoff,
                    compositor,
                    window_manager,
                    present_frame_sequences,
                    user_interfaces,
                    renderer_settings,
                );
            }
        }
        Progress::NoLiveWindows => {
            let Some(awaiting) = awaiting_first_present_backend_handoff.take() else {
                return;
            };

            let rollback = restore_retained_backend_handoff(
                awaiting.retained,
                compositor,
                window_manager,
                renderer_settings,
            );

            let _ = awaiting.sender.send(Err(strict_handoff_error_with_windows(
                backend::StrictHandoffPhase::AwaitingFirstPresent,
                backend::StrictHandoffFailureCategory::NoActiveWindow,
                rollback,
                strict_closed_before_proof_evidence(&awaiting.windows),
                "strict handoff lost all required windows before first present proof completed",
            )));
        }
    }
}

fn fail_awaiting_first_present_backend_handoff<P, C>(
    awaiting: AwaitingFirstPresentBackendHandoff<P, C>,
    evidence: backend::StrictHandoffWindowEvidence,
    message: String,
    compositor: &mut Option<C>,
    window_manager: &mut window::Manager<P, C>,
    renderer_settings: renderer::Settings,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    let rollback = restore_retained_backend_handoff(
        awaiting.retained,
        compositor,
        window_manager,
        renderer_settings,
    );

    let _ = awaiting.sender.send(Err(strict_handoff_error_with_windows(
        backend::StrictHandoffPhase::AwaitingFirstPresent,
        backend::StrictHandoffFailureCategory::FirstPresent,
        rollback,
        vec![evidence],
        message,
    )));
}

/// Builds a window's [`UserInterface`] for the [`Program`].
fn build_user_interface<'a, P: Program>(
    program: &'a program::Instance<P>,
    cache: user_interface::Cache,
    renderer: &mut P::Renderer,
    size: Size,
    id: window::Id,
) -> UserInterface<'a, P::Message, P::Theme, P::Renderer>
where
    P::Theme: theme::Base,
{
    let build_started = StdInstant::now();
    let view_span = debug::view(id);
    let view_started = StdInstant::now();
    let view = program.view(id);
    let view_us = view_started.elapsed().as_micros();
    view_span.finish();

    let layout_span = debug::layout(id);
    let layout_started = StdInstant::now();
    let user_interface = UserInterface::build(view, size, cache, renderer);
    let layout_us = layout_started.elapsed().as_micros();
    layout_span.finish();

    if trace::enabled() {
        trace::event(
            "winit_build_ui",
            build_started.elapsed().as_micros(),
            format_args!(
                "window={id} view_us={view_us} layout_us={layout_us} size={:.1}x{:.1}",
                size.width, size.height
            ),
        );
    }

    user_interface
}

fn update<P: Program, E: Executor>(
    program: &mut program::Instance<P>,
    runtime: &mut Runtime<E, Proxy<P::Message>, Action<P::Message>>,
    messages: &mut Vec<P::Message>,
) -> Vec<Action<P::Message>>
where
    P::Theme: theme::Base,
{
    use futures::futures;

    let mut actions = Vec::new();
    let mut outputs = Vec::new();

    while !messages.is_empty() {
        for message in messages.drain(..) {
            let task = runtime.enter(|| program.update(message));

            if let Some(mut stream) = runtime::task::into_stream(task) {
                let waker = futures::task::noop_waker_ref();
                let mut context = futures::task::Context::from_waker(waker);

                // Run immediately available actions synchronously (e.g. widget operations)
                loop {
                    match runtime.enter(|| stream.poll_next_unpin(&mut context)) {
                        futures::task::Poll::Ready(Some(Action::Output(output))) => {
                            outputs.push(output);
                        }
                        futures::task::Poll::Ready(Some(action)) => {
                            actions.push(action);
                        }
                        futures::task::Poll::Ready(None) => {
                            break;
                        }
                        futures::task::Poll::Pending => {
                            runtime.run(stream);
                            break;
                        }
                    }
                }
            }
        }

        messages.append(&mut outputs);
    }

    let subscription = runtime.enter(|| program.subscription());
    let recipes = subscription::into_recipes(subscription.map(Action::Output));

    runtime.track(recipes);

    actions
}

fn run_action<'a, P, C>(
    action: Action<P::Message>,
    program: &'a program::Instance<P>,
    _proxy: &Proxy<P::Message>,
    runtime: &mut Runtime<P::Executor, Proxy<P::Message>, Action<P::Message>>,
    compositor: &mut Option<C>,
    preparing_backend_handoff: &mut Option<PreparingBackendHandoff<C>>,
    pending_backend_handoff: &mut Option<PendingBackendHandoff<C>>,
    awaiting_first_present_backend_handoff: &mut Option<AwaitingFirstPresentBackendHandoff<P, C>>,
    events: &mut Vec<(window::Id, core::Event)>,
    messages: &mut Vec<P::Message>,
    clipboard: &mut Clipboard,
    control_sender: &mut mpsc::UnboundedSender<Control>,
    interfaces: &mut FxHashMap<window::Id, UserInterface<'a, P::Message, P::Theme, P::Renderer>>,
    window_manager: &mut window::Manager<P, C>,
    ui_caches: &mut FxHashMap<window::Id, user_interface::Cache>,
    is_window_opening: &mut bool,
    system_theme: &mut theme::Mode,
    renderer_settings: &mut renderer::Settings,
) where
    P: Program,
    C: Compositor<Renderer = P::Renderer> + Send + 'static,
    P::Theme: theme::Base,
{
    use crate::core::Renderer as _;
    use crate::runtime::backend;
    use crate::runtime::clipboard;
    use crate::runtime::window;

    match action {
        Action::Output(message) => {
            messages.push(message);
        }
        Action::Clipboard(action) => match action {
            clipboard::Action::Read { kind, channel } => {
                clipboard.read(kind, move |result| {
                    let _ = channel.send(result);
                });
            }
            clipboard::Action::Write { content, channel } => {
                clipboard.write(content, move |result| {
                    let _ = channel.send(result);
                });
            }
        },
        Action::Window(action) => match action {
            window::Action::Open(id, settings, channel) => {
                let monitor = window_manager.last_monitor();

                control_sender
                    .start_send(Control::CreateWindow {
                        id,
                        settings,
                        title: program.title(id),
                        scale_factor: program.scale_factor(id),
                        monitor,
                        on_open: channel,
                    })
                    .expect("Send control action");

                *is_window_opening = true;
            }
            window::Action::Close(id) => {
                let _ = ui_caches.remove(&id);
                let _ = interfaces.remove(&id);

                if window_manager.remove(id).is_some() {
                    cancel_backend_handoffs(
                        preparing_backend_handoff,
                        pending_backend_handoff,
                        awaiting_first_present_backend_handoff,
                        compositor,
                        window_manager,
                        *renderer_settings,
                        "backend handoff cancelled by programmatic window close",
                    );

                    if trace::enabled() {
                        trace::event(
                            "winit_window_close",
                            0,
                            format_args!("window={id} source=programmatic"),
                        );
                    }

                    events.push((id, core::Event::Window(core::window::Event::Closed)));
                }

                if window_manager.is_empty() {
                    *compositor = None;
                }
            }
            window::Action::GetOldest(channel) => {
                let id = window_manager.iter_mut().next().map(|(id, _window)| id);

                let _ = channel.send(id);
            }
            window::Action::GetLatest(channel) => {
                let id = window_manager.iter_mut().last().map(|(id, _window)| id);

                let _ = channel.send(id);
            }
            window::Action::Drag(id) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = window.raw.drag_window();
                }
            }
            window::Action::DragResize(id, direction) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = window
                        .raw
                        .drag_resize_window(conversion::resize_direction(direction));
                }
            }
            window::Action::Resize(id, size) => {
                if let Some(window) = window_manager.get_mut(id) {
                    if let Some(size) = window.raw.request_inner_size(
                        winit::dpi::LogicalSize {
                            width: size.width,
                            height: size.height,
                        }
                        .to_physical::<f32>(f64::from(window.state.scale_factor())),
                    ) {
                        // Some platforms (notably Wayland) apply this size
                        // immediately without emitting a later Resized event.
                        // Use the actual returned size, which may be clamped.
                        let event = winit::event::WindowEvent::Resized(size);
                        window.state.update(program, &window.raw, &event);
                        if let Some(event) = conversion::window_event(
                            event,
                            window.state.scale_factor(),
                            window.state.modifiers(),
                        ) {
                            events.push((id, event));
                        }
                        window.raw.request_redraw();
                    }
                }
            }
            window::Action::SetMinSize(id, size) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_min_inner_size(size.map(|size| {
                        winit::dpi::LogicalSize {
                            width: size.width,
                            height: size.height,
                        }
                        .to_physical::<f32>(f64::from(window.state.scale_factor()))
                    }));
                }
            }
            window::Action::SetMaxSize(id, size) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_max_inner_size(size.map(|size| {
                        winit::dpi::LogicalSize {
                            width: size.width,
                            height: size.height,
                        }
                        .to_physical::<f32>(f64::from(window.state.scale_factor()))
                    }));
                }
            }
            window::Action::SetResizeIncrements(id, increments) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_resize_increments(increments.map(|size| {
                        winit::dpi::LogicalSize {
                            width: size.width,
                            height: size.height,
                        }
                        .to_physical::<f32>(f64::from(window.state.scale_factor()))
                    }));
                }
            }
            window::Action::SetResizable(id, resizable) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_resizable(resizable);
                }
            }
            window::Action::GetSize(id, channel) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let size = window.state.logical_size();
                    let _ = channel.send(Size::new(size.width, size.height));
                }
            }
            window::Action::GetMaximized(id, channel) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = channel.send(window.raw.is_maximized());
                }
            }
            window::Action::Maximize(id, maximized) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_maximized(maximized);
                }
            }
            window::Action::GetMinimized(id, channel) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = channel.send(window.raw.is_minimized());
                }
            }
            window::Action::Minimize(id, minimized) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_minimized(minimized);
                }
            }
            window::Action::GetPosition(id, channel) => {
                if let Some(window) = window_manager.get(id) {
                    let position = window
                        .raw
                        .outer_position()
                        .map(|position| {
                            let position = position.to_logical::<f32>(window.raw.scale_factor());

                            Point::new(position.x, position.y)
                        })
                        .ok();

                    let _ = channel.send(position);
                }
            }
            window::Action::GetScaleFactor(id, channel) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let scale_factor = window.raw.scale_factor();

                    let _ = channel.send(scale_factor as f32);
                }
            }
            window::Action::Move(id, position) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_outer_position(winit::dpi::LogicalPosition {
                        x: position.x,
                        y: position.y,
                    });
                }
            }
            window::Action::SetMode(id, mode) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_visible(conversion::visible(mode));
                    window
                        .raw
                        .set_fullscreen(conversion::fullscreen(window.raw.current_monitor(), mode));
                }
            }
            window::Action::SetIcon(id, icon) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_window_icon(conversion::icon(icon));
                }
            }
            window::Action::GetMode(id, channel) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let mode = if window.raw.is_visible().unwrap_or(true) {
                        conversion::mode(window.raw.fullscreen())
                    } else {
                        core::window::Mode::Hidden
                    };

                    let _ = channel.send(mode);
                }
            }
            window::Action::ToggleMaximize(id) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_maximized(!window.raw.is_maximized());
                }
            }
            window::Action::ToggleDecorations(id) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_decorations(!window.raw.is_decorated());
                }
            }
            window::Action::RequestUserAttention(id, attention_type) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window
                        .raw
                        .request_user_attention(attention_type.map(conversion::user_attention));
                }
            }
            window::Action::GainFocus(id) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.focus_window();
                }
            }
            window::Action::SetLevel(id, level) => {
                if let Some(window) = window_manager.get_mut(id) {
                    window.raw.set_window_level(conversion::window_level(level));
                }
            }
            window::Action::ShowSystemMenu(id) => {
                if let Some(window) = window_manager.get_mut(id)
                    && let mouse::Cursor::Available(point) = window.state.cursor()
                {
                    window.raw.show_window_menu(winit::dpi::LogicalPosition {
                        x: point.x,
                        y: point.y,
                    });
                }
            }
            window::Action::GetRawId(id, channel) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = channel.send(window.raw.id().into());
                }
            }
            window::Action::Run(id, f) => {
                if let Some(window) = window_manager.get_mut(id) {
                    f(&window.raw);
                }
            }
            window::Action::Screenshot(id, channel) => {
                if let Some(window) = window_manager.get_mut(id)
                    && let Some(compositor) = compositor
                {
                    let bytes = compositor.screenshot(
                        &mut window.renderer,
                        window.state.viewport(),
                        window.state.background_color(),
                    );

                    let _ = channel.send(core::window::Screenshot::new(
                        bytes,
                        window.state.physical_size(),
                        window.state.scale_factor(),
                    ));
                }
            }
            window::Action::EnableMousePassthrough(id) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = window.raw.set_cursor_hittest(false);
                }
            }
            window::Action::DisableMousePassthrough(id) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = window.raw.set_cursor_hittest(true);
                }
            }
            window::Action::GetMonitorSize(id, channel) => {
                if let Some(window) = window_manager.get(id) {
                    let size = window.raw.current_monitor().map(|monitor| {
                        let scale = window.state.scale_factor();
                        let size = monitor.size().to_logical(f64::from(scale));

                        Size::new(size.width, size.height)
                    });

                    let _ = channel.send(size);
                }
            }
            window::Action::SetAllowAutomaticTabbing(enabled) => {
                control_sender
                    .start_send(Control::SetAutomaticWindowTabbing(enabled))
                    .expect("Send control action");
            }
            window::Action::RedrawAll => {
                for (_id, window) in window_manager.iter_mut() {
                    window.raw.request_redraw();
                }
            }
            window::Action::RelayoutAll => {
                for (id, window) in window_manager.iter_mut() {
                    if let Some(ui) = interfaces.remove(&id) {
                        let _ = interfaces.insert(
                            id,
                            ui.relayout(window.state.logical_size(), &mut window.renderer),
                        );
                    }

                    window.raw.request_redraw();
                }
            }
        },
        Action::System(action) => match action {
            system::Action::GetInformation(_channel) => {
                #[cfg(feature = "sysinfo")]
                {
                    if let Some(compositor) = compositor {
                        let graphics_info = compositor.information();

                        let _ = std::thread::spawn(move || {
                            let information = system_information(graphics_info);

                            let _ = _channel.send(information);
                        });
                    }
                }
            }
            system::Action::GetTheme(channel) => {
                let _ = channel.send(*system_theme);
            }
            system::Action::NotifyTheme(mode) => {
                if mode != *system_theme {
                    *system_theme = mode;

                    runtime.broadcast(subscription::Event::SystemThemeChanged(mode));
                }

                let Some(theme) = conversion::window_theme(mode) else {
                    return;
                };

                for (_id, window) in window_manager.iter_mut() {
                    window.state.update(
                        program,
                        &window.raw,
                        &winit::event::WindowEvent::ThemeChanged(theme),
                    );
                }
            }
        },
        Action::Font(action) => match action {
            font::Action::Load { bytes, channel } => {
                if let Some(compositor) = compositor {
                    let result = compositor.load_font(bytes.clone());
                    let _ = channel.send(result);
                }
            }
            font::Action::List { channel } => {
                if let Some(compositor) = compositor {
                    let fonts = compositor.list_fonts();
                    let _ = channel.send(fonts);
                }
            }
            font::Action::SetDefaults { font, text_size } => {
                renderer_settings.default_font = font;
                renderer_settings.default_text_size = text_size;

                let Some(compositor) = compositor else {
                    return;
                };

                // Recreate renderers and relayout all windows
                for (id, window) in window_manager.iter_mut() {
                    window.renderer = compositor.create_renderer(*renderer_settings);

                    let Some(ui) = interfaces.remove(&id) else {
                        continue;
                    };

                    let size = window.state.logical_size();
                    let ui = ui.relayout(size, &mut window.renderer);
                    let _ = interfaces.insert(id, ui);

                    window.raw.request_redraw();
                }
            }
        },
        Action::Widget(operation) => {
            let mut current_operation = Some(operation);

            while let Some(mut operation) = current_operation.take() {
                for (id, ui) in interfaces.iter_mut() {
                    if let Some(window) = window_manager.get_mut(*id) {
                        ui.operate(&window.renderer, operation.as_mut());
                    }
                }

                match operation.finish() {
                    operation::Outcome::None => {}
                    operation::Outcome::Some(()) => {}
                    operation::Outcome::Chain(next) => {
                        current_operation = Some(next);
                    }
                }
            }

            // Redraw all windows
            for (_, window) in window_manager.iter_mut() {
                window.raw.request_redraw();
            }
        }
        Action::Image(action) => match action {
            image::Action::Allocate(handle, sender) => {
                // TODO: Shared image cache in compositor
                if let Some((_id, window)) = window_manager.iter_mut().next() {
                    window.renderer.allocate_image(&handle, move |allocation| {
                        let _ = sender.send(allocation);
                    });
                }
            }
        },
        Action::Backend(action) => match action {
            #[cfg(not(target_arch = "wasm32"))]
            backend::Action::Configure(settings, sender) => {
                cancel_backend_handoffs(
                    preparing_backend_handoff,
                    pending_backend_handoff,
                    awaiting_first_present_backend_handoff,
                    compositor,
                    window_manager,
                    *renderer_settings,
                    "backend handoff cancelled by backend configure",
                );

                let shell = Shell::new(_proxy.clone());

                let mut new_compositor = if let Some(window) = window_manager.first() {
                    match runtime.block_on(C::new(
                        settings,
                        window.raw.clone(),
                        window.raw.clone(),
                        shell,
                    )) {
                        Ok(compositor) => compositor,
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            return;
                        }
                    }
                } else {
                    let _ = sender.send(Err(crate::core::backend::Error::BackendError(
                        "cannot configure backend without an active window".to_owned(),
                    )));
                    return;
                };

                graphics::cache::invalidate_all();

                window_manager.replace_with(|mut window| {
                    let size = window.state.physical_size();

                    drop(window.renderer);
                    drop(window.surface);

                    window.renderer = new_compositor.create_renderer(*renderer_settings);
                    window.surface =
                        new_compositor.create_surface(window.raw.clone(), size.width, size.height);

                    window
                });

                *compositor = Some(new_compositor);

                for (_id, window) in window_manager.iter_mut() {
                    window.raw.request_redraw();
                }

                let _ = sender.send(Ok(()));
            }
            #[cfg(all(not(target_arch = "wasm32"), feature = "wgpu"))]
            backend::Action::PrepareWarmAndCommit(settings, sender) => {
                if preparing_backend_handoff.is_some() {
                    let _ = sender.send(Err(strict_handoff_error(
                        crate::core::backend::StrictHandoffPhase::Preparing,
                        crate::core::backend::StrictHandoffFailureCategory::AlreadyInProgress,
                        crate::core::backend::StrictRollbackStatus::NotNeeded,
                        "a backend handoff is already preparing",
                    )));
                    return;
                }

                if pending_backend_handoff.is_some() {
                    let _ = sender.send(Err(strict_handoff_error(
                        crate::core::backend::StrictHandoffPhase::CommitPending,
                        crate::core::backend::StrictHandoffFailureCategory::AlreadyInProgress,
                        crate::core::backend::StrictRollbackStatus::NotNeeded,
                        "a backend handoff is already pending",
                    )));
                    return;
                }

                if awaiting_first_present_backend_handoff.is_some() {
                    let _ = sender.send(Err(strict_handoff_error(
                        crate::core::backend::StrictHandoffPhase::AwaitingFirstPresent,
                        crate::core::backend::StrictHandoffFailureCategory::AlreadyInProgress,
                        crate::core::backend::StrictRollbackStatus::Retained,
                        "a backend handoff is awaiting first present evidence",
                    )));
                    return;
                }

                let Some(window) = window_manager.first() else {
                    let _ = sender.send(Err(strict_handoff_error(
                        crate::core::backend::StrictHandoffPhase::Preparing,
                        crate::core::backend::StrictHandoffFailureCategory::NoActiveWindow,
                        crate::core::backend::StrictRollbackStatus::NotNeeded,
                        "cannot prepare backend handoff without an active window",
                    )));
                    return;
                };

                let phase = crate::core::backend::StrictHandoffPhase::Preparing;

                log::info!("Preparing backend handoff: {phase:?}");

                if trace::enabled() {
                    trace::event(
                        "backend_handoff_prepare_start",
                        0,
                        format_args!("phase={phase:?}"),
                    );
                }

                if let Some(error) = injected_backend_failure("prepare") {
                    if trace::enabled() {
                        trace::event(
                            "backend_handoff_prepare_failed",
                            0,
                            format_args!("phase={phase:?} error={error:?}"),
                        );
                    }

                    let _ = sender.send(Err(strict_backend_error(
                        phase,
                        crate::core::backend::StrictHandoffFailureCategory::Prepare,
                        crate::core::backend::StrictRollbackStatus::NotNeeded,
                        error,
                    )));
                    return;
                }

                let shell = Shell::new(_proxy.clone());
                let raw_window = window.raw.clone();
                let (prepared_sender, prepared_receiver) = oneshot::channel();

                *preparing_backend_handoff = Some(PreparingBackendHandoff {
                    receiver: prepared_receiver,
                    sender,
                    warming: None,
                });

                runtime.run(
                    stream::once(async move {
                        maybe_delay_backend_prepare_async().await;

                        let prepared_compositor =
                            C::new(settings, raw_window.clone(), raw_window, shell).await;

                        let _ = prepared_sender.send(prepared_compositor);

                        let (sender, _receiver) = oneshot::channel();

                        Action::Window(crate::runtime::window::Action::GetLatest(sender))
                    })
                    .boxed(),
                );

                for (_id, window) in window_manager.iter_mut() {
                    window.raw.request_redraw();
                }
            }
            #[cfg(target_arch = "wasm32")]
            backend::Action::Configure(_, _) => {}
            #[cfg(any(target_arch = "wasm32", not(feature = "wgpu")))]
            backend::Action::PrepareWarmAndCommit(_, sender) => {
                let _ = sender.send(Err(strict_handoff_error(
                    crate::core::backend::StrictHandoffPhase::Preparing,
                    crate::core::backend::StrictHandoffFailureCategory::Unsupported,
                    crate::core::backend::StrictRollbackStatus::NotNeeded,
                    "backend warm handoff is not supported on this target",
                )));
            }
        },
        Action::Event { window, event } => {
            events.push((window, event));
        }
        Action::Tick => {
            for (_id, window) in window_manager.iter_mut() {
                window.renderer.tick();
            }
        }
        Action::Reload => {
            for (id, window) in window_manager.iter_mut() {
                let Some(ui) = interfaces.remove(&id) else {
                    continue;
                };

                let cache = ui.into_cache();
                let size = window.state.logical_size();

                let _ = interfaces.insert(
                    id,
                    build_user_interface(program, cache, &mut window.renderer, size, id),
                );

                window.raw.request_redraw();
            }
        }
        Action::Exit => {
            cancel_backend_handoffs(
                preparing_backend_handoff,
                pending_backend_handoff,
                awaiting_first_present_backend_handoff,
                compositor,
                window_manager,
                *renderer_settings,
                "backend handoff cancelled by action exit",
            );

            control_sender
                .start_send(Control::Exit)
                .expect("Send control action");
        }
    }
}

/// Build the user interface for every window.
pub fn build_user_interfaces<'a, P: Program, C>(
    program: &'a program::Instance<P>,
    window_manager: &mut window::Manager<P, C>,
    mut cached_user_interfaces: FxHashMap<window::Id, user_interface::Cache>,
    proxy: &mut Proxy<P::Message>,
) -> FxHashMap<window::Id, UserInterface<'a, P::Message, P::Theme, P::Renderer>>
where
    C: Compositor<Renderer = P::Renderer>,
    P::Theme: theme::Base,
{
    for (id, window) in window_manager.iter_mut() {
        let old_size = window.state.logical_size();

        window.state.synchronize(program, id, &window.raw);

        let new_size = window.state.logical_size();

        if old_size != new_size {
            proxy.send_action(Action::Event {
                window: id,
                event: core::Event::Window(window::Event::Resized(new_size)),
            });
        }

        #[cfg(feature = "hinting")]
        window.renderer.hint(window.state.scale_factor());
    }

    debug::theme_changed(|| {
        window_manager
            .first()
            .and_then(|window| theme::Base::seed(window.state.theme()))
    });

    cached_user_interfaces
        .drain()
        .filter_map(|(id, cache)| {
            let window = window_manager.get_mut(id)?;

            Some((
                id,
                build_user_interface(
                    program,
                    cache,
                    &mut window.renderer,
                    window.state.logical_size(),
                    id,
                ),
            ))
        })
        .collect()
}

/// Returns true if the provided event should cause a [`Program`] to
/// exit.
pub fn user_force_quit(
    event: &winit::event::WindowEvent,
    _modifiers: winit::keyboard::ModifiersState,
) -> bool {
    match event {
        #[cfg(target_os = "macos")]
        winit::event::WindowEvent::KeyboardInput {
            event:
                winit::event::KeyEvent {
                    logical_key: winit::keyboard::Key::Character(c),
                    state: winit::event::ElementState::Pressed,
                    ..
                },
            ..
        } if c == "q" && _modifiers.super_key() => true,
        _ => false,
    }
}

#[cfg(feature = "sysinfo")]
fn system_information(graphics: compositor::Information) -> system::Information {
    use sysinfo::{Process, System};

    let mut system = System::new_all();
    system.refresh_all();

    let cpu_brand = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_default();

    let memory_used = sysinfo::get_current_pid()
        .and_then(|pid| system.process(pid).ok_or("Process not found"))
        .map(Process::memory)
        .ok();

    system::Information {
        system_name: System::name(),
        system_kernel: System::kernel_version(),
        system_version: System::long_os_version(),
        system_short_version: System::os_version(),
        cpu_brand,
        cpu_cores: system.physical_core_count(),
        memory_total: system.total_memory(),
        memory_used,
        graphics_adapter: graphics.adapter,
        graphics_backend: graphics.backend,
    }
}

fn run_clipboard<Message: Send>(
    proxy: &mut Proxy<Message>,
    clipboard: &mut Clipboard,
    requests: core::Clipboard,
    window: window::Id,
) {
    for kind in requests.reads {
        let proxy = proxy.clone();

        clipboard.read(kind, move |result| {
            proxy.send_action(Action::Event {
                window,
                event: core::Event::Clipboard(core::clipboard::Event::Read(result.map(Arc::new))),
            });
        });
    }

    if let Some(content) = requests.write {
        let proxy = proxy.clone();

        clipboard.write(content, move |result| {
            proxy.send_action(Action::Event {
                window,
                event: core::Event::Clipboard(core::clipboard::Event::Written(result)),
            });
        });
    }
}
