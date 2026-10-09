//! Bounded offscreen scroll profiling with GPU timestamps, without opening a window.
//! cargo run --locked -j 1 --example profile_scroll -- target/title.csv
//! Optional positional arguments: physical width, height, scale, frame count.
//! Select an adapter with WGPU_ADAPTER_NAME (substring, case insensitive).
//! Measures command execution separately from CPU recording and submission;
//! excludes surface presentation, desktop composition, and queued atlas uploads.
//! CPU results depend on the Cargo profile; GPU shaders are the same in debug.
//! Isolated editor cases disable wrapping; full-app cases use default settings.
//! FRAGILE_SCROLL_PROFILE_CASE selects one case (e.g. app-scroll or app-drag).
//! FRAGILE_SCROLL_PROFILE_CAPTURE saves PNGs to the supplied directory.
//! FRAGILE_SCROLL_PROFILE_WRAP=0 disables wrapping in full-app cases.
//! FRAGILE_SCROLL_PROFILE_THEME=dark selects the dark full-app theme.
//! FRAGILE_SCROLL_PROFILE_MASK_CACHE=256 selects the original glyph eviction policy.

#[cfg(not(feature = "hybrid-rendering"))]
fn main() {
    eprintln!("profile_scroll requires hybrid-rendering");
}

#[cfg(feature = "hybrid-rendering")]
fn main() {
    profile::run();
}

#[cfg(feature = "hybrid-rendering")]
mod profile {
    use fragile_notepad::app::App;
    use fragile_notepad::core::{Document, DocumentId, EditorSettings};
    use fragile_notepad::editor::{AdvancedEditor, EditorAction, EditorMetrics};
    use fragile_notepad::message::Message;
    use fragile_notepad::services::types::OpenedFile;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::{self, Renderer as _};
    use iced::advanced::widget::Tree;
    use iced::advanced::{Layout, Shell, layout, mouse};
    use iced::{Background, Color, Element, Event, Point, Rectangle, Size, Theme, window};
    use iced_wgpu::graphics::{Shell as GraphicsShell, Viewport};
    use std::time::{Duration, Instant};

    pub fn run() {
        let args: Vec<_> = std::env::args().skip(1).collect();
        let path = args.first().map_or("target/title.csv", String::as_str);
        let number = |index: usize, default: u32| {
            args.get(index)
                .map_or(default, |value| value.parse().expect("numeric argument"))
        };
        let width = number(1, 2560);
        let height = number(2, 1600);
        let scale: f32 = args
            .get(3)
            .map_or(1.5, |value| value.parse().expect("scale"));
        let frames = number(4, 48).clamp(1, 240);
        assert!(width > 0 && height > 0 && scale.is_finite() && scale > 0.0);
        let source = std::fs::read_to_string(path).expect("read UTF-8 fixture");
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = if let Ok(name) = std::env::var("WGPU_ADAPTER_NAME") {
            futures::executor::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN))
                .into_iter()
                .find(|adapter| {
                    adapter
                        .get_info()
                        .name
                        .to_lowercase()
                        .contains(&name.to_lowercase())
                })
                .expect("requested Vulkan adapter")
        } else {
            futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                ..Default::default()
            }))
            .expect("Vulkan adapter")
        };
        let features =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        assert!(
            adapter.features().contains(features),
            "GPU timestamp support required"
        );
        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: features,
                ..Default::default()
            }))
            .expect("device");
        println!(
            "adapter={:?} physical={}x{} scale={} bytes={} lines={} frames={}",
            adapter.get_info(),
            width,
            height,
            scale,
            source.len(),
            source.lines().count(),
            frames
        );
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Bgra8Unorm,
            None,
            GraphicsShell::headless(),
        );
        let mut renderer = iced::Renderer::Primary(iced_wgpu::Renderer::new(
            engine,
            renderer::Settings::default(),
        ));
        renderer.hint(scale);
        if let Ok(dimension) = std::env::var("FRAGILE_SCROLL_PROFILE_MASK_CACHE") {
            let iced::Renderer::Primary(gpu_renderer) = &renderer else {
                unreachable!()
            };
            gpu_renderer.set_text_cache_retention_dimension(
                dimension.parse().expect("mask cache dimension"),
            );
        }
        let viewport = Viewport::with_physical_size(Size::new(width, height), scale);
        let size = Size::new(width as f32 / scale, height as f32 / scale);
        let bounds = Rectangle::with_size(size);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scroll profile target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("scroll profile timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp resolve"),
            size: 256,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp readback"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ascii: String = source
            .chars()
            .map(|ch| if ch.is_ascii() { ch } else { 'x' })
            .collect();
        let only_case = std::env::var("FRAGILE_SCROLL_PROFILE_CASE").ok();
        let capture =
            std::env::var_os("FRAGILE_SCROLL_PROFILE_CAPTURE").map(std::path::PathBuf::from);
        if let Some(path) = &capture {
            std::fs::create_dir_all(path).expect("capture directory");
        }
        let mut matched = false;
        for (name, text) in [
            ("clear", ""),
            ("background", ""),
            ("ascii-scroll", ascii.as_str()),
            ("fixture-scroll", source.as_str()),
            ("fixture-repeat", source.as_str()),
            ("app-scroll", source.as_str()),
            ("app-drag", source.as_str()),
            ("app-repeat", source.as_str()),
        ]
        .into_iter()
        .filter(|(name, _)| {
            only_case
                .as_deref()
                .is_none_or(|selected| selected == *name)
        }) {
            matched = true;
            let mut document = Document::from_path(DocumentId::new(1), path, text);
            let settings = EditorSettings::default();
            document.ensure_syntax_cache(settings.syntax_theme);
            document.set_word_wrap(false);
            let mut app = name.starts_with("app").then(|| {
                // Drop tasks: no windows, file writes, or background services run.
                let (mut app, _) = App::new();
                let mut settings = EditorSettings::default();
                if std::env::var("FRAGILE_SCROLL_PROFILE_WRAP").as_deref() == Ok("0") {
                    settings.word_wrap = false;
                }
                if std::env::var("FRAGILE_SCROLL_PROFILE_THEME").as_deref() == Ok("dark") {
                    settings.set_appearance(fragile_notepad::core::AppearanceMode::Dark);
                }
                let _ = app.update(Message::SettingsLoaded(Ok(Some(settings))));
                let _ = app.update(Message::FileOpened(Ok(OpenedFile {
                    path: path.into(),
                    contents: std::sync::Arc::new(fragile_notepad::core::DecodedText {
                        text: text.to_owned(),
                        encoding: fragile_notepad::core::TextEncoding::Utf8,
                        had_errors: false,
                    }),
                    disk_revision: fragile_notepad::core::FileRevision::from_bytes(text.as_bytes()),
                })));
                app
            });
            let window_id = window::Id::unique();
            let mut tree = Tree::empty();
            let mut gpu = Vec::new();
            let mut cpu = Vec::new();
            let mut warm_cache = None;
            for frame in 0..frames + 8 {
                let start = Instant::now();
                renderer.reset(bounds);
                if name != "clear" && app.is_none() {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds,
                            ..Default::default()
                        },
                        Background::Color(Color::from_rgb(0.08, 0.08, 0.08)),
                    );
                }
                if let Some(app) = &mut app {
                    record_app(
                        &mut renderer,
                        &mut tree,
                        app,
                        window_id,
                        size,
                        bounds,
                        name,
                        frame,
                    );
                } else if !text.is_empty() {
                    if name.ends_with("scroll") {
                        document.scroll.first_visible_row = frame as usize * 3;
                    }
                    record_editor(
                        &mut renderer,
                        &mut tree,
                        &mut document,
                        &settings,
                        size,
                        bounds,
                        name.ends_with("scroll"),
                    );
                }
                let mut begin = device.create_command_encoder(&Default::default());
                begin.write_timestamp(&queries, 0);
                let iced::Renderer::Primary(gpu_renderer) = &mut renderer else {
                    unreachable!()
                };
                let mut draw = gpu_renderer.draw(Some(Color::BLACK), &view, &viewport);
                draw.write_timestamp(&queries, 1);
                draw.resolve_query_set(&queries, 0..2, &resolve, 0);
                draw.copy_buffer_to_buffer(&resolve, 0, &readback, 0, 16);
                gpu_renderer.finish();
                queue.submit([begin.finish(), draw.finish()]);
                gpu_renderer.recall();
                let cpu_us = start.elapsed().as_secs_f64() * 1e6;
                let (sender, receiver) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        sender.send(result).expect("map result receiver");
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(Duration::from_secs(10)),
                    })
                    .expect("GPU completion");
                receiver
                    .recv()
                    .expect("map callback")
                    .expect("timestamp mapping");
                let mapped = readback.slice(..).get_mapped_range();
                let first = u64::from_ne_bytes(mapped[0..8].try_into().unwrap());
                let last = u64::from_ne_bytes(mapped[8..16].try_into().unwrap());
                let gpu_us = last.saturating_sub(first) as f64
                    * queue.get_timestamp_period() as f64
                    / 1000.0;
                drop(mapped);
                readback.unmap();
                if frame == 7 {
                    warm_cache = Some(gpu_renderer.text_cache_statistics());
                }
                if frame >= 8 {
                    gpu.push(gpu_us);
                    cpu.push(cpu_us);
                }
                // Keep the user's GPU responsive; never run an uncapped render loop.
                std::thread::sleep(Duration::from_millis(16).saturating_sub(start.elapsed()));
            }
            gpu.sort_by(f64::total_cmp);
            cpu.sort_by(f64::total_cmp);
            let p95 = (frames as usize * 95 / 100).min(frames as usize - 1);
            println!(
                "{name}: gpu_median_us={:.1} gpu_p95_us={:.1} cpu_median_us={:.1} cpu_p95_us={:.1}",
                gpu[gpu.len() / 2],
                gpu[p95],
                cpu[cpu.len() / 2],
                cpu[p95]
            );
            let iced::Renderer::Primary(gpu_renderer) = &renderer else {
                unreachable!()
            };
            let after = gpu_renderer.text_cache_statistics();
            let before = warm_cache.expect("warm-up cache statistics");
            println!(
                "{name}: mask_dimension={} cached_glyphs={} allocation_calls={} evictions={}",
                after.mask_size,
                after.mask_glyphs,
                after
                    .mask_allocation_calls
                    .saturating_sub(before.mask_allocation_calls),
                after.mask_evictions.saturating_sub(before.mask_evictions)
            );
            if let Some(path) = &capture {
                let iced::Renderer::Primary(gpu_renderer) = &mut renderer else {
                    unreachable!()
                };
                let pixels = gpu_renderer.screenshot(&viewport, Color::BLACK);
                tiny_skia::Pixmap::from_vec(
                    pixels,
                    tiny_skia::IntSize::from_wh(width, height).unwrap(),
                )
                .expect("RGBA screenshot")
                .save_png(path.join(format!("{name}.png")))
                .expect("capture PNG");
            }
        }
        assert!(matched, "unknown FRAGILE_SCROLL_PROFILE_CASE");
    }

    fn record_editor(
        renderer: &mut iced::Renderer,
        tree: &mut Tree,
        document: &mut Document,
        settings: &EditorSettings,
        size: Size,
        bounds: Rectangle,
        scrolling: bool,
    ) {
        let mut messages = Vec::new();
        {
            let editor = AdvancedEditor::new(
                &document.buffer,
                &document.viewport,
                &document.decorations,
                &document.syntax_cache,
                iced::highlighter::Settings {
                    token: document.render_syntax_token().to_owned(),
                    theme: settings.syntax_theme,
                },
                document.selection_set().clone(),
                |action| action,
            )
            .metrics(
                EditorMetrics {
                    line_height: 20.0,
                    character_width: 8.8,
                    ..EditorMetrics::default()
                }
                .with_line_count(document.buffer.line_count()),
            )
            .cjk_context(document.cjk_context())
            .scroll(document.scroll);
            let mut content: Element<'_, EditorAction> = editor.into();
            tree.diff(content.as_widget_mut());
            let node =
                content
                    .as_widget_mut()
                    .layout(tree, renderer, &layout::Limits::new(size, size));
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            content.as_widget_mut().update(
                tree,
                &Event::Window(window::Event::RedrawRequested(Instant::now())),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                renderer,
                &mut shell,
                &bounds,
            );
            // Exercise the real wheel fast path, without publishing changes to the fixture.
            if scrolling {
                content.as_widget_mut().update(
                    tree,
                    &Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
                    }),
                    Layout::new(&node),
                    mouse::Cursor::Available(Point::new(200.0, 100.0)),
                    renderer,
                    &mut shell,
                    &bounds,
                );
            }
            content.as_widget().draw(
                tree,
                renderer,
                &Theme::Dark,
                &renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
        }
        for message in messages {
            if let EditorAction::ViewportChanged {
                visible_rows,
                text_width,
                character_width_milli,
                font_size_milli,
                hint_factor_milli,
            } = message
            {
                document.update_viewport_geometry_with_typography(
                    visible_rows,
                    text_width as f32,
                    character_width_milli as f32 / 1000.0,
                    font_size_milli as f32 / 1000.0,
                    hint_factor_milli.map(|scale| scale as f32 / 1000.0),
                );
            }
        }
    }

    fn record_app(
        renderer: &mut iced::Renderer,
        tree: &mut Tree,
        app: &mut App,
        window_id: window::Id,
        size: Size,
        bounds: Rectangle,
        name: &str,
        frame: u32,
    ) {
        let mut messages = Vec::new();
        {
            let mut content = app.view(window_id);
            tree.diff(content.as_widget_mut());
            let node =
                content
                    .as_widget_mut()
                    .layout(tree, renderer, &layout::Limits::new(size, size));
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            content.as_widget_mut().update(
                tree,
                &Event::Window(window::Event::RedrawRequested(Instant::now())),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                renderer,
                &mut shell,
                &bounds,
            );
            if name.ends_with("scroll") {
                content.as_widget_mut().update(
                    tree,
                    &Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
                    }),
                    Layout::new(&node),
                    mouse::Cursor::Available(Point::new(200.0, size.height * 0.5)),
                    renderer,
                    &mut shell,
                    &bounds,
                );
            } else if name == "app-drag" {
                // The editor is the large leaf of the workbench layout. Use
                // its real bounds so this remains valid across DPI settings.
                fn editor_bounds(layout: Layout<'_>, size: Size) -> Option<Rectangle> {
                    let children: Vec<_> = layout.children().collect();
                    if children.is_empty() {
                        let bounds = layout.bounds();
                        return (bounds.width > size.width * 0.5
                            && bounds.height > size.height * 0.5)
                            .then_some(bounds);
                    }
                    children
                        .into_iter()
                        .find_map(|child| editor_bounds(child, size))
                }
                let editor = editor_bounds(Layout::new(&node), size).expect("editor layout");
                let progress = (frame % 120) as f32 / 119.0;
                let position = Point::new(
                    editor.x + editor.width - 6.0,
                    editor.y + 14.0 + (editor.height - 28.0) * progress,
                );
                let event = if frame == 0 {
                    mouse::Event::ButtonPressed(mouse::Button::Left)
                } else {
                    mouse::Event::CursorMoved { position }
                };
                content.as_widget_mut().update(
                    tree,
                    &Event::Mouse(event),
                    Layout::new(&node),
                    mouse::Cursor::Available(position),
                    renderer,
                    &mut shell,
                    &bounds,
                );
            }
        }
        if name == "app-drag" && frame > 0 {
            assert!(
                messages.iter().any(|message| matches!(
                    message,
                    Message::EditorAction(_, EditorAction::ScrollToRow(_))
                )),
                "scrollbar drag must publish scrolling"
            );
        }
        for message in messages {
            let _ = app.update(message);
        }
        let mut content = app.view(window_id);
        tree.diff(content.as_widget_mut());
        let node = content
            .as_widget_mut()
            .layout(tree, renderer, &layout::Limits::new(size, size));
        content.as_widget().draw(
            tree,
            renderer,
            &app.theme(window_id).unwrap_or(Theme::Light),
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &bounds,
        );
    }
}
