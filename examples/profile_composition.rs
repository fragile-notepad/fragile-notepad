//! cargo run --locked -j 1 --example profile_composition -- --frames 32
//! Add --uncached for the same scenes with retained composition disabled.
//! --compare measures each uncached/cached pair consecutively on one device.
//! --help lists case, fixture, physical size, scale, and warm-up options.
//! WGPU_ADAPTER_NAME selects a Vulkan adapter by case-insensitive substring.
//!
//! Uses the runtime UserInterface: Element, Tree, and layout persist across
//! redraws; only messages or widget invalidation cause a rebuild. Redraws are
//! forced at at most 60 Hz, with real timestamps so child deadlines still fire.
//! GPU timestamps include cached-surface preparation, the low-resolution trail,
//! and main-pass composition. They exclude queued uploads, presentation, and
//! desktop composition. CPU timing includes update, recording, and submission,
//! excluding GPU completion waits, timestamp mapping, and frame pacing.
//! --uncached retains the new trail and vendor batching; the investigation
//! report is the historical baseline before those optimizations.
//! --case app-scroll-reversal sends eight wheel lines/frame, reversing every six.
//! --case app-fade deterministically closes/opens About every ten measured frames.
//! Widget and transition clocks use real redraw timestamps, paced at most 60 Hz;
//! opacity progress can differ between runs when frame intervals differ.

#[cfg(not(feature = "hybrid-rendering"))]
fn main() -> std::process::ExitCode {
    eprintln!("profile_composition requires the hybrid-rendering feature");
    std::process::ExitCode::FAILURE
}

#[cfg(feature = "hybrid-rendering")]
fn main() -> std::process::ExitCode {
    match profile::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("profile_composition: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "hybrid-rendering")]
mod profile {
    use fragile_notepad::app::App;
    use fragile_notepad::core::{
        AppearanceMode, DecodedText, EditorSettings, FileRevision, TextEncoding,
    };
    use fragile_notepad::message::Message;
    use fragile_notepad::services::types::OpenedFile;
    use fragile_notepad::ui;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::mouse;
    use iced::advanced::renderer::{self, Renderer as _};
    use iced::{Color, Element, Event, Point, Size, Theme, window};
    use iced_runtime::user_interface::{self, UserInterface};
    use iced_wgpu::graphics::{Shell as GraphicsShell, Viewport};
    use std::error::Error;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    type Result<T> = std::result::Result<T, Box<dyn Error>>;
    const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
    const HELP: &str = "Usage: profile_composition [OPTIONS]
  --case CASE      app-repeat, app-about, vfx, app-scroll-reversal, app-fade,
                   or all [all]
  --fixture PATH   UTF-8 editor fixture [embedded src/app.rs]
  --width PX       Physical target width [1536]
  --height PX      Physical target height [1152]
  --scale FACTOR   Logical-to-physical scale [1.5]
  --frames N       Measured frames per case, 1..240 [32]
  --warmup N       Unmeasured frames per case, 1..32 [8]
  --uncached       Set both raster and final-frame cache budgets to zero
  --compare        Measure uncached, then cached for each selected case
  -h, --help       Print this help";

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Case {
        Repeat,
        About,
        Vfx,
        ScrollReversal,
        Fade,
    }

    impl Case {
        const ALL: [Self; 5] = [
            Self::Repeat,
            Self::About,
            Self::Vfx,
            Self::ScrollReversal,
            Self::Fade,
        ];

        fn name(self) -> &'static str {
            match self {
                Self::Repeat => "app-repeat",
                Self::About => "app-about",
                Self::Vfx => "vfx",
                Self::ScrollReversal => "app-scroll-reversal",
                Self::Fade => "app-fade",
            }
        }
    }

    struct Options {
        case: Option<Case>,
        fixture: Option<PathBuf>,
        width: u32,
        height: u32,
        scale: f32,
        frames: u32,
        warmup: u32,
        uncached: bool,
        compare: bool,
    }

    impl Options {
        fn parse() -> std::result::Result<Option<Self>, String> {
            let mut options = Self {
                case: None,
                fixture: None,
                width: 1536,
                height: 1152,
                scale: 1.5,
                frames: 32,
                warmup: 8,
                uncached: false,
                compare: false,
            };
            let mut args = std::env::args().skip(1);
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--case" => {
                        options.case = match value(&mut args, &flag)?.as_str() {
                            "all" => None,
                            "app-repeat" => Some(Case::Repeat),
                            "app-about" => Some(Case::About),
                            "vfx" => Some(Case::Vfx),
                            "app-scroll-reversal" => Some(Case::ScrollReversal),
                            "app-fade" => Some(Case::Fade),
                            case => return Err(format!("unknown case: {case}")),
                        };
                    }
                    "--fixture" => options.fixture = Some(value(&mut args, &flag)?.into()),
                    "--width" => options.width = number(&mut args, &flag)?,
                    "--height" => options.height = number(&mut args, &flag)?,
                    "--scale" => options.scale = number(&mut args, &flag)?,
                    "--frames" => options.frames = number(&mut args, &flag)?,
                    "--warmup" => options.warmup = number(&mut args, &flag)?,
                    "--uncached" => options.uncached = true,
                    "--compare" => options.compare = true,
                    "--help" | "-h" => {
                        println!("{HELP}");
                        return Ok(None);
                    }
                    _ => return Err(format!("unknown option: {flag}; use --help")),
                }
            }
            if options.width == 0 || options.height == 0 {
                return Err("physical width and height must be positive".into());
            }
            if options.uncached && options.compare {
                return Err("choose --uncached or --compare".into());
            }
            if !options.scale.is_finite() || options.scale <= 0.0 {
                return Err("scale must be finite and positive".into());
            }
            if !(1..=240).contains(&options.frames) || !(1..=32).contains(&options.warmup) {
                return Err("frames must be 1..240 and warmup must be 1..32".into());
            }
            Ok(Some(options))
        }

        fn fixture_path(&self) -> &Path {
            self.fixture.as_deref().unwrap_or(Path::new("src/app.rs"))
        }
    }

    fn value(
        args: &mut impl Iterator<Item = String>,
        flag: &str,
    ) -> std::result::Result<String, String> {
        args.next()
            .ok_or_else(|| format!("missing value for {flag}"))
    }

    fn number<T: FromStr>(
        args: &mut impl Iterator<Item = String>,
        flag: &str,
    ) -> std::result::Result<T, String> {
        let value = value(args, flag)?;
        value
            .parse()
            .map_err(|_| format!("invalid {flag}: {value}"))
    }

    enum Scene {
        App(Box<App>),
        Vfx,
    }

    impl Scene {
        fn new(case: Case, options: &Options, source: &str) -> Self {
            if case == Case::Vfx {
                return Self::Vfx;
            }
            // Returned tasks are dropped: no real windows, file writes, settings
            // reads, or background application subscriptions run in this probe.
            let (mut app, _) = App::new();
            let mut settings = EditorSettings::default();
            settings.set_appearance(AppearanceMode::Light);
            let _ = app.update(Message::SettingsLoaded(Ok(Some(settings))));
            let _ = app.update(Message::FileOpened(Ok(OpenedFile {
                path: options.fixture_path().to_owned(),
                contents: Arc::new(DecodedText {
                    text: source.to_owned(),
                    encoding: TextEncoding::Utf8,
                    had_errors: false,
                }),
                disk_revision: FileRevision::from_bytes(source.as_bytes()),
            })));
            if matches!(case, Case::About | Case::Fade) {
                // Start with the modal fully revealed. Its InfoVfx animation
                // advances locally on RedrawRequested without rebuilding App.
                let now = Instant::now();
                let _ = app.update(Message::AboutOpened);
                let _ = app.update(Message::ChromeAnimationFrame(now));
                let _ = app.update(Message::ChromeAnimationFrame(
                    now + Duration::from_millis(200),
                ));
            }
            Self::App(Box::new(app))
        }

        fn view(&self, window_id: window::Id) -> Element<'_, Message> {
            match self {
                Self::App(app) => app.view(window_id),
                Self::Vfx => iced::widget::container(ui::info_vfx::view(1.0, true))
                    .width(544.0)
                    .height(96.0)
                    .into(),
            }
        }

        fn update(&mut self, message: Message) {
            match self {
                Self::App(app) => {
                    let _ = app.update(message);
                }
                Self::Vfx => unreachable!("isolated InfoVfx must not publish app messages"),
            }
        }

        fn theme(&self, window_id: window::Id) -> Theme {
            match self {
                Self::App(app) => app.theme(window_id).unwrap_or(Theme::Light),
                Self::Vfx => Theme::Light,
            }
        }
    }

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        engine: iced_wgpu::Engine,
        viewport: Viewport,
        _target: wgpu::Texture,
        view: wgpu::TextureView,
        queries: wgpu::QuerySet,
        resolve: wgpu::Buffer,
        readback: wgpu::Buffer,
    }

    impl Gpu {
        fn new(options: &Options) -> Result<Self> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::VULKAN,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let adapter = if let Ok(name) = std::env::var("WGPU_ADAPTER_NAME") {
                if name.trim().is_empty() {
                    return Err("WGPU_ADAPTER_NAME must not be empty".into());
                }
                futures::executor::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN))
                    .into_iter()
                    .find(|adapter| {
                        adapter
                            .get_info()
                            .name
                            .to_lowercase()
                            .contains(&name.to_lowercase())
                    })
                    .ok_or_else(|| {
                        format!("no Vulkan adapter matches WGPU_ADAPTER_NAME={name:?}")
                    })?
            } else {
                futures::executor::block_on(instance.request_adapter(
                    &wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::LowPower,
                        ..Default::default()
                    },
                ))?
            };
            let timestamps =
                wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
            if !adapter.features().contains(timestamps) {
                return Err(
                    "Vulkan adapter must support timestamps inside command encoders".into(),
                );
            }
            // Match production quad/image parameters instead of inadvertently
            // benchmarking uniform fallback on a device supporting immediates.
            let immediate_size = if adapter.features().contains(wgpu::Features::IMMEDIATES)
                && adapter.limits().max_immediate_size >= 80
            {
                80
            } else {
                0
            };
            let (device, queue) =
                futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    required_features: timestamps
                        | if immediate_size > 0 {
                            wgpu::Features::IMMEDIATES
                        } else {
                            wgpu::Features::empty()
                        },
                    required_limits: wgpu::Limits {
                        max_immediate_size: immediate_size,
                        ..Default::default()
                    },
                    ..Default::default()
                }))?;
            let limit = device.limits().max_texture_dimension_2d;
            if options.width > limit || options.height > limit {
                return Err(
                    format!("physical dimensions exceed device texture limit {limit}").into(),
                );
            }
            println!(
                "adapter={:?} immediate_bytes={immediate_size}",
                adapter.get_info()
            );
            let engine = iced_wgpu::Engine::new(
                &adapter,
                device.clone(),
                queue.clone(),
                FORMAT,
                None,
                GraphicsShell::headless(),
            );
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("composition profile target"),
                size: wgpu::Extent3d {
                    width: options.width,
                    height: options.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("composition profile timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            });
            let resolve = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("composition timestamp resolve"),
                size: 256,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("composition timestamp readback"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Ok(Self {
                device,
                queue,
                engine,
                viewport: Viewport::with_physical_size(
                    Size::new(options.width, options.height),
                    options.scale,
                ),
                _target: target,
                view,
                queries,
                resolve,
                readback,
            })
        }

        fn sample(
            &self,
            backend: &mut iced_wgpu::Renderer,
            started: Instant,
        ) -> Result<(f64, f64)> {
            let mut begin = self.device.create_command_encoder(&Default::default());
            begin.write_timestamp(&self.queries, 0);
            let mut draw = backend.draw(Some(Color::BLACK), &self.view, &self.viewport);
            draw.write_timestamp(&self.queries, 1);
            draw.resolve_query_set(&self.queries, 0..2, &self.resolve, 0);
            draw.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, 16);
            backend.finish();
            let submission = self.queue.submit([begin.finish(), draw.finish()]);
            backend.recall();
            let cpu_us = started.elapsed().as_secs_f64() * 1e6;

            let (sender, receiver) = std::sync::mpsc::channel();
            self.readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(result);
                });
            self.device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(10)),
            })?;
            receiver.recv_timeout(Duration::from_secs(10))??;
            let mapped = self.readback.slice(..).get_mapped_range();
            let first = u64::from_ne_bytes(mapped[0..8].try_into()?);
            let last = u64::from_ne_bytes(mapped[8..16].try_into()?);
            let gpu_us = last.wrapping_sub(first) as f64
                * f64::from(self.queue.get_timestamp_period())
                / 1000.0;
            drop(mapped);
            self.readback.unmap();
            Ok((gpu_us, cpu_us))
        }
    }

    #[derive(Default, Clone, Copy)]
    struct UiCounters {
        rebuilds: u64,
        layout_changed_frames: u64,
        messages: u64,
        deadline_requested_frames: u64,
        scroll_inputs: u64,
        fade_frames: u64,
        fade_toggles: u64,
        fade_close_messages: u64,
        fade_open_messages: u64,
        last_fade_toggle: Option<(u32, bool)>,
    }

    fn backend(renderer: &mut iced::Renderer) -> &mut iced_wgpu::Renderer {
        let iced::Renderer::Primary(backend) = renderer else {
            unreachable!("the benchmark always uses Vulkan")
        };
        backend
    }

    fn summarize(samples: &mut [f64]) -> (f64, f64) {
        samples.sort_by(f64::total_cmp);
        let middle = samples.len() / 2;
        let median = if samples.len() % 2 == 0 {
            (samples[middle - 1] + samples[middle]) * 0.5
        } else {
            samples[middle]
        };
        let p95 = (samples.len() * 95).div_ceil(100) - 1;
        (median, samples[p95])
    }

    fn measure(case: Case, options: &Options, source: &str, gpu: &Gpu) -> Result<()> {
        println!(
            "case={} cache={}",
            case.name(),
            if options.uncached {
                "disabled"
            } else {
                "enabled"
            },
        );
        // A fresh backend isolates retained-surface counters and cache occupancy
        // per case; engine pipelines remain shared, as they are across windows.
        let mut renderer =
            iced_wgpu::Renderer::new(gpu.engine.clone(), renderer::Settings::default());
        if options.uncached {
            renderer.set_raster_cache_budget(0);
            renderer.set_composition_cache_budget(0);
        }
        let mut renderer = iced::Renderer::Primary(renderer);
        renderer.hint(options.scale);
        let size = gpu.viewport.logical_size();
        let window_id = window::Id::unique();
        let mut scene = Scene::new(case, options, source);
        let mut interface = UserInterface::build(
            scene.view(window_id),
            size,
            user_interface::Cache::new(),
            &mut renderer,
        );
        let waker = Waker::noop();
        let cursor = if case == Case::ScrollReversal {
            // The workbench editor fills the central content area, below the
            // title bar/toolbars and above the status bar. Do not click/focus:
            // the case measures wheel scrolling without introducing a caret.
            mouse::Cursor::Available(Point::new(size.width * 0.5, size.height * 0.6))
        } else {
            mouse::Cursor::Unavailable
        };
        let mut messages = Vec::new();
        let mut events = Vec::with_capacity(2);
        let mut counters = UiCounters::default();
        let mut warm_counters = UiCounters::default();
        let mut warm_cache = backend(&mut renderer).raster_cache_statistics();
        let mut warm_composition = backend(&mut renderer).composition_cache_statistics();
        let mut gpu_samples = Vec::with_capacity(options.frames as usize);
        let mut cpu_samples = Vec::with_capacity(options.frames as usize);
        let mut next_frame = Instant::now();

        match case {
            Case::ScrollReversal => println!(
                "case=app-scroll-reversal input=widget_wheel wheel_lines_per_frame=8 reversal_frames=6 warmup=static"
            ),
            Case::Fade => println!(
                "case=app-fade transition=about_close_open transition_frames=10 scheduled_redraw_hz=60 transition_clock=real widget_clock=real warmup=fully_open"
            ),
            _ => {}
        }

        for frame in 0..options.warmup + options.frames {
            std::thread::sleep(next_frame.saturating_duration_since(Instant::now()));
            let started = Instant::now();
            next_frame = started + FRAME_INTERVAL;
            events.clear();
            let measured_frame = frame.checked_sub(options.warmup);
            let scroll_input = case == Case::ScrollReversal && measured_frame.is_some();
            if let Some(measured_frame) = measured_frame {
                match case {
                    Case::ScrollReversal => {
                        let direction = if (measured_frame / 6) % 2 == 0 {
                            -8.0
                        } else {
                            8.0
                        };
                        events.push(Event::Mouse(mouse::Event::WheelScrolled {
                            delta: mouse::ScrollDelta::Lines {
                                x: 0.0,
                                y: direction,
                            },
                        }));
                        counters.scroll_inputs += 1;
                    }
                    Case::Fade => {
                        if measured_frame % 10 == 0 {
                            messages.push(if (measured_frame / 10) % 2 == 0 {
                                Message::AboutClosed
                            } else {
                                Message::AboutOpened
                            });
                            counters.fade_toggles += 1;
                        }
                        // The toggle schedule is deterministic by frame index.
                        // Like the runtime subscription, transition progress
                        // uses actual elapsed time and may vary between runs.
                        messages.push(Message::ChromeAnimationFrame(started));
                        counters.fade_frames += 1;
                    }
                    _ => {}
                }
            }
            events.push(Event::Window(window::Event::RedrawRequested(started)));
            let (state, _) = interface.update(
                &window::Headless,
                &waker,
                &events,
                cursor,
                &mut renderer,
                &mut messages,
            );
            if scroll_input
                && !messages.iter().any(|message| {
                    matches!(
                        message,
                        Message::EditorAction(
                            _,
                            fragile_notepad::editor::EditorAction::ScrollLines(_)
                        )
                    )
                })
            {
                return Err(
                    "scroll event did not reach the editor; increase --width/--height".into(),
                );
            }
            if let user_interface::State::Updated {
                has_layout_changed,
                redraw_request,
                ..
            } = &state
            {
                counters.layout_changed_frames += u64::from(*has_layout_changed);
                counters.deadline_requested_frames +=
                    u64::from(*redraw_request != window::RedrawRequest::Wait);
            }
            if !messages.is_empty() || matches!(state, user_interface::State::Outdated) {
                // Consuming the interface releases its borrow of App, while
                // retaining the Tree. Do not move this rebuild out of the branch:
                // unconditional diff/layout would invalidate the raster caches.
                let cache = interface.into_cache();
                counters.messages += messages.len() as u64;
                for message in messages.drain(..) {
                    if case == Case::Fade
                        && let Some(measured_frame) = measured_frame
                    {
                        match &message {
                            Message::AboutClosed => {
                                counters.fade_close_messages += 1;
                                counters.last_fade_toggle = Some((measured_frame, true));
                            }
                            Message::AboutOpened => {
                                counters.fade_open_messages += 1;
                                counters.last_fade_toggle = Some((measured_frame, false));
                            }
                            _ => {}
                        }
                    }
                    scene.update(message);
                }
                interface = UserInterface::build(scene.view(window_id), size, cache, &mut renderer);
                counters.rebuilds += 1;
            }
            let theme = scene.theme(window_id);
            interface.draw(
                &mut renderer,
                &theme,
                &renderer::Style {
                    text_color: theme.palette().background.base.text,
                },
                cursor,
            );
            let (gpu_us, cpu_us) = gpu.sample(backend(&mut renderer), started)?;
            if frame + 1 == options.warmup {
                warm_cache = backend(&mut renderer).raster_cache_statistics();
                warm_composition = backend(&mut renderer).composition_cache_statistics();
                warm_counters = counters;
            } else if frame >= options.warmup {
                gpu_samples.push(gpu_us);
                cpu_samples.push(cpu_us);
            }
        }

        let (gpu_median, gpu_p95) = summarize(&mut gpu_samples);
        let (cpu_median, cpu_p95) = summarize(&mut cpu_samples);
        let after = backend(&mut renderer).raster_cache_statistics();
        let name = case.name();
        println!(
            "case={name} gpu_median_us={gpu_median:.1} gpu_p95_us={gpu_p95:.1} cpu_median_us={cpu_median:.1} cpu_p95_us={cpu_p95:.1}"
        );
        println!(
            "case={name} raster_cache_hits={} raster_cache_misses={} raster_cache_rasterizations={} raster_cache_live_fallbacks={} raster_cache_volatile_fallbacks={} raster_cache_bytes={} raster_cache_entries={} ui_rebuilds={} layout_changed_frames={} messages={} deadline_requested_frames={}",
            after.hits.saturating_sub(warm_cache.hits),
            after.misses.saturating_sub(warm_cache.misses),
            after
                .rasterizations
                .saturating_sub(warm_cache.rasterizations),
            after
                .live_fallbacks
                .saturating_sub(warm_cache.live_fallbacks),
            after
                .volatile_fallbacks
                .saturating_sub(warm_cache.volatile_fallbacks),
            after.bytes,
            after.entries,
            counters.rebuilds - warm_counters.rebuilds,
            counters.layout_changed_frames - warm_counters.layout_changed_frames,
            counters.messages - warm_counters.messages,
            counters.deadline_requested_frames - warm_counters.deadline_requested_frames,
        );
        let composition = backend(&mut renderer).composition_cache_statistics();
        println!(
            "case={name} composition_full_repaints={} composition_partial_repaints={} composition_reused_frames={} composition_direct_frames={} composition_bytes={} composition_damaged_pixels={}",
            composition
                .full_repaints
                .saturating_sub(warm_composition.full_repaints),
            composition
                .partial_repaints
                .saturating_sub(warm_composition.partial_repaints),
            composition
                .reused_frames
                .saturating_sub(warm_composition.reused_frames),
            composition
                .direct_frames
                .saturating_sub(warm_composition.direct_frames),
            composition.bytes,
            composition
                .damaged_pixels
                .saturating_sub(warm_composition.damaged_pixels),
        );
        if matches!(case, Case::ScrollReversal | Case::Fade) {
            let (last_toggle_frame, last_toggle_message) = counters.last_fade_toggle.map_or_else(
                || ("none".to_owned(), "none"),
                |(frame, closed)| {
                    (
                        frame.to_string(),
                        if closed { "AboutClosed" } else { "AboutOpened" },
                    )
                },
            );
            println!(
                "case={name} scroll_inputs={} fade_frames={} fade_toggles={} fade_close_messages={} fade_open_messages={} last_toggle_frame={last_toggle_frame} last_toggle_message={last_toggle_message}",
                counters.scroll_inputs - warm_counters.scroll_inputs,
                counters.fade_frames - warm_counters.fade_frames,
                counters.fade_toggles - warm_counters.fade_toggles,
                counters.fade_close_messages - warm_counters.fade_close_messages,
                counters.fade_open_messages - warm_counters.fade_open_messages,
            );
        }
        Ok(())
    }

    pub fn run() -> Result<()> {
        let Some(mut options) = Options::parse()? else {
            return Ok(());
        };
        let source = if options.case == Some(Case::Vfx) {
            String::new()
        } else if let Some(path) = &options.fixture {
            std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read UTF-8 fixture {}: {error}", path.display()))?
        } else {
            // A fresh checkout and a binary launched outside the repository
            // can both reproduce the default workload without ignored files.
            include_str!("../src/app.rs").to_owned()
        };
        println!(
            "physical={}x{} scale={} frames={} warmup={} cache={} debug_assertions={} forced_redraw_hz=60 theme=light",
            options.width,
            options.height,
            options.scale,
            options.frames,
            options.warmup,
            if options.compare {
                "compare"
            } else if options.uncached {
                "disabled"
            } else {
                "enabled"
            },
            cfg!(debug_assertions),
        );
        if options.case != Some(Case::Vfx) {
            println!(
                "fixture={:?} fixture_source={} bytes={} lines={} blake3={}",
                options.fixture_path(),
                if options.fixture.is_some() {
                    "file"
                } else {
                    "embedded"
                },
                source.len(),
                source.lines().count(),
                blake3::hash(source.as_bytes()),
            );
        }
        println!(
            "timing=offscreen_gpu_execution cpu=update_record_submit gpu_wait_and_pacing_excluded=true queued_uploads_and_presentation_excluded=true"
        );
        let gpu = Gpu::new(&options)?;
        let selected = options.case;
        for case in Case::ALL
            .into_iter()
            .filter(|case| selected.is_none_or(|selected| selected == *case))
        {
            if options.compare {
                options.uncached = true;
                measure(case, &options, &source, &gpu)?;
                options.uncached = false;
            }
            measure(case, &options, &source, &gpu)?;
        }
        Ok(())
    }
}
