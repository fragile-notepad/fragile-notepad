#![cfg(feature = "hybrid-rendering")]

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::text::{self, Renderer as _};
use iced::{Color, Font, Pixels, Point, Rectangle, Size, Transformation, alignment};
use iced_wgpu::graphics::{Shell, Viewport};
use std::cell::Cell;

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;
const DEFAULT_BUDGET: u64 = 32 * 1024 * 1024;

#[test]
fn asynchronous_image_completion_repaints_same_key_before_a_surface_can_be_reused() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let token = renderer::Cache::new();
    // Exactly 2 MiB takes the asynchronous upload path, without requiring codecs.
    let image = image::Handle::from_rgba(512, 1024, [255, 0, 255, 255].repeat(512 * 1024));
    let bounds = rect(8.0, 8.0, 32.0, 24.0);
    let calls = Cell::new(0);
    let mut complete = false;
    for frame in 0..40 {
        cached.tick();
        let viewport = begin_frame(&mut cached, 1.0);
        cached.with_cached_layer(&token, 1, bounds, |renderer| {
            calls.set(calls.get() + 1);
            fill(renderer, bounds, Color::WHITE);
            renderer.draw_image(
                iced::advanced::graphics::core::Image::new(image.clone()),
                bounds,
                bounds,
            );
        });
        let actual = cached.screenshot(&viewport, Color::BLACK);
        if frame == 0 {
            assert_eq!(
                pixel(&actual, 16, 16),
                [255, 255, 255, 255],
                "pending image omitted on first paint"
            );
        }
        if pixel(&actual, 16, 16) == [255, 0, 255, 255] {
            complete = true;
            break;
        }
        assert_eq!(
            cached.raster_cache_statistics().hits,
            0,
            "incomplete pixels cannot become a cache hit"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        complete,
        "asynchronous image must replace incomplete cached pixels"
    );
    assert!(
        calls.get() >= 2,
        "same caller key must repaint after upload completion"
    );
    let before = cached.raster_cache_statistics();
    let count = calls.get();
    let viewport = begin_frame(&mut cached, 1.0);
    cached.with_cached_layer(&token, 1, bounds, |_| calls.set(calls.get() + 1));
    let actual = cached.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&actual, 16, 16), [255, 0, 255, 255]);
    assert_eq!(
        calls.get(),
        count,
        "complete image paint should be retained"
    );
    assert_eq!(cached.raster_cache_statistics().hits, before.hits + 1);
}

fn lock_vulkan_test() -> std::sync::MutexGuard<'static, ()> {
    // Keep device initialization and destruction serialized, including when
    // this test binary is run without the recommended --test-threads=1.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Gpu {
    engine: iced_wgpu::Engine,
    device: wgpu::Device,
    instance: wgpu::Instance,
}

impl Gpu {
    fn new() -> Option<Self> {
        Self::with_format(wgpu::TextureFormat::Rgba8Unorm)
    }

    fn with_format(format: wgpu::TextureFormat) -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let Some(adapter) =
            futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
        else {
            eprintln!("Skipping raster cache validation: no Vulkan adapter");
            return None;
        };
        // An unavailable adapter may skip; device or rendering failures must fail.
        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&Default::default()))
                .expect("create Vulkan raster cache test device");
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device.clone(),
            queue,
            format,
            None,
            Shell::headless(),
        );
        Some(Self {
            engine,
            device,
            instance,
        })
    }

    fn renderer(&self) -> iced_wgpu::Renderer {
        iced_wgpu::Renderer::new(self.engine.clone(), renderer::Settings::default())
    }
}

fn paint_text_and_image(renderer: &mut iced_wgpu::Renderer, image: &image::Handle, y: f32) {
    let clip = rect(8.0, y, 112.0, 32.0);
    renderer.draw_image(
        image::Image::new(image.clone()).filter_method(image::FilterMethod::Nearest),
        rect(80.0, y + 4.0, 24.0, 24.0),
        clip,
    );
    renderer.fill_text(
        text::Text {
            content: "Cache".to_owned(),
            bounds: Size::new(60.0, 24.0),
            size: Pixels(18.0),
            line_height: text::LineHeight::Absolute(Pixels(24.0)),
            font: Font::DEFAULT,
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: None,
        },
        Point::new(16.0, y + 4.0),
        Color::from_rgba(1.0, 1.0, 1.0, 0.75),
        clip,
    );
}

#[test]
fn text_and_synchronous_rgba_images_match_direct_pixels_in_two_cached_surfaces() {
    let _guard = lock_vulkan_test();
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let Some(gpu) = Gpu::with_format(format) else {
            return;
        };
        let mut cached = gpu.renderer();
        let mut direct = gpu.renderer();
        let image = image::Handle::from_rgba(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
        );
        // Load synchronously at the root before either cache child is created.
        // Both children and the live root must use its shared atlas allocation.
        let _cached_lease = cached.load_image(&image).expect("load root RGBA image");
        let _direct_lease = direct
            .load_image(&image)
            .expect("load reference RGBA image");
        let tokens = [renderer::Cache::new(), renderer::Cache::new()];
        let calls = Cell::new(0);

        for frame in 0..2 {
            let viewport = begin_frame(&mut cached, 1.0);
            begin_frame(&mut direct, 1.0);
            for (index, y) in [8.0, 48.0].into_iter().enumerate() {
                let bounds = rect(8.0, y, 112.0, 32.0);
                cached.with_cached_layer(&tokens[index], 1, bounds, |renderer| {
                    calls.set(calls.get() + 1);
                    paint_text_and_image(renderer, &image, y);
                });
                direct.with_layer(bounds, |renderer| paint_text_and_image(renderer, &image, y));
            }
            for renderer in [&mut cached, &mut direct] {
                renderer.with_layer(rect(0.0, 84.0, 12.0, 12.0), |renderer| {
                    renderer.draw_image(
                        image::Image::new(image.clone())
                            .filter_method(image::FilterMethod::Nearest),
                        rect(0.0, 84.0, 12.0, 12.0),
                        rect(0.0, 84.0, 12.0, 12.0),
                    );
                });
            }
            let actual = cached.screenshot(&viewport, Color::BLACK);
            let expected = direct.screenshot(&viewport, Color::BLACK);
            let context = format!("text/RGBA images: {format:?}");
            if format == wgpu::TextureFormat::Rgba8UnormSrgb {
                assert_srgb_pixels_match(&actual, &expected, &context);
            } else {
                assert_pixels_match(&actual, &expected, &context);
            }
            for y in [8, 48] {
                assert_eq!(pixel(&actual, 84, y + 8), [255, 0, 0, 255]);
                assert_eq!(pixel(&actual, 98, y + 8), [0, 255, 0, 255]);
                assert_eq!(pixel(&actual, 84, y + 22), [0, 0, 255, 255]);
                assert_eq!(pixel(&actual, 98, y + 22), [255, 255, 0, 255]);
                assert!(
                    (y + 4..y + 28).any(|row| (16..72).any(|x| pixel(&actual, x, row)[0] > 0)),
                    "text must produce visible glyphs: {format:?}"
                );
            }
            assert_eq!(pixel(&actual, 3, 87), [255, 0, 0, 255]);
            assert_eq!(calls.get(), 2);
            let statistics = cached.raster_cache_statistics();
            assert_eq!(statistics.hits, frame * 2);
            assert_eq!(statistics.misses, 2);
            assert_eq!(statistics.rasterizations, 2);
            assert_eq!(statistics.live_fallbacks, 0);
            assert_eq!(statistics.entries, 2);
            assert!(statistics.bytes <= DEFAULT_BUDGET);
        }
    }
}

#[test]
fn srgb_cached_transparency_matches_direct_rendering_on_cold_and_hit_frames() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::with_format(wgpu::TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let mut cached = gpu.renderer();
    let mut direct = gpu.renderer();
    let token = renderer::Cache::new();
    let calls = Cell::new(0);
    let direct_calls = Cell::new(0);
    for (frame, backdrop) in [Color::from_rgb(0.0, 0.0, 1.0), Color::WHITE]
        .into_iter()
        .enumerate()
    {
        let viewport = begin_frame(&mut cached, 1.0);
        begin_frame(&mut direct, 1.0);
        transparency_scene(&mut cached, Some(&token), backdrop, &calls);
        transparency_scene(&mut direct, None, backdrop, &direct_calls);
        let actual = cached.screenshot(&viewport, Color::BLACK);
        let expected = direct.screenshot(&viewport, Color::BLACK);
        assert_srgb_pixels_match(&actual, &expected, "sRGB transparency");
        assert_eq!(pixel(&actual, 52, 32), [0, 255, 255, 255]);
        assert!(pixel(&actual, 20, 20)[0] > 0);
        assert_eq!(calls.get(), 1);
        let statistics = cached.raster_cache_statistics();
        assert_eq!(statistics.hits, frame as u64);
        assert_eq!(statistics.misses, 1);
        assert_eq!(statistics.rasterizations, 1);
    }
}

#[test]
fn scale_translation_and_transform_changes_invalidate_retained_pixels() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let mut direct = gpu.renderer();
    let token = renderer::Cache::new();
    let calls = Cell::new(0);
    let bounds = rect(4.0, 4.0, 20.0, 12.0);
    let translated = Transformation::translate(12.0, 8.0);
    let transformed = translated * Transformation::scale(1.5);
    let paint = |renderer: &mut iced_wgpu::Renderer| {
        fill(renderer, bounds, Color::from_rgb(1.0, 0.0, 0.0));
        fill(
            renderer,
            rect(8.0, 8.0, 8.0, 4.0),
            Color::from_rgb(0.0, 1.0, 0.0),
        );
    };
    let mut previous = Vec::new();

    for (frame, (scale, transformation)) in [
        (1.0, Transformation::IDENTITY),
        (1.0, Transformation::IDENTITY),
        (1.0, translated),
        (1.0, transformed),
        (2.0, transformed),
        (2.0, transformed),
    ]
    .into_iter()
    .enumerate()
    {
        let viewport = begin_frame(&mut cached, scale);
        begin_frame(&mut direct, scale);
        cached.with_transformation(transformation, |renderer| {
            renderer.with_cached_layer(&token, 1, bounds, |renderer| {
                calls.set(calls.get() + 1);
                paint(renderer);
            });
        });
        direct.with_transformation(transformation, |renderer| {
            renderer.with_layer(bounds, paint)
        });
        let actual = cached.screenshot(&viewport, Color::BLACK);
        let expected = direct.screenshot(&viewport, Color::BLACK);
        assert_pixels_match(&actual, &expected, "scale/transformation invalidation");
        assert!(actual.chunks_exact(4).any(|p| p == [255, 0, 0, 255]));
        assert!(actual.chunks_exact(4).any(|p| p == [0, 255, 0, 255]));
        if (2..=4).contains(&frame) {
            assert_ne!(actual, previous, "changed transform/scale must move pixels");
        }
        previous = actual;
        assert_eq!(calls.get(), [1, 1, 2, 3, 4, 4][frame]);
        let statistics = cached.raster_cache_statistics();
        assert_eq!(statistics.hits, [0, 1, 1, 1, 1, 2][frame]);
        assert_eq!(statistics.misses, [1, 1, 2, 3, 4, 4][frame]);
        assert_eq!(statistics.rasterizations, [1, 1, 2, 3, 4, 4][frame]);
        assert_eq!(statistics.live_fallbacks, 0);
        assert_eq!(statistics.entries, 1);
    }
}

#[test]
fn reset_clears_frame_paint_and_last_token_drop_releases_cached_gpu_resources() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let token = renderer::Cache::new();
    let clone = token.clone();
    let owner = token.downgrade();
    let calls = Cell::new(0);
    let bounds = rect(8.0, 8.0, 32.0, 24.0);
    let paint = |renderer: &mut iced_wgpu::Renderer| {
        calls.set(calls.get() + 1);
        fill(renderer, bounds, Color::from_rgb(1.0, 0.0, 0.0));
    };
    let viewport = begin_frame(&mut cached, 1.0);
    cached.with_cached_layer(&token, 1, bounds, paint);
    let first = cached.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&first, 12, 12), [255, 0, 0, 255]);
    let occupied_textures = gpu
        .instance
        .generate_report()
        .unwrap()
        .hub
        .textures
        .num_kept_from_user;

    begin_frame(&mut cached, 1.0);
    let empty = cached.screenshot(&viewport, Color::BLACK);
    assert!(empty.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    assert_eq!(cached.raster_cache_statistics().entries, 1);
    drop(token);
    assert!(owner.upgrade().is_some(), "a clone keeps the entry alive");
    cached.with_cached_layer(&clone, 1, bounds, paint);
    assert_pixels_match(
        &cached.screenshot(&viewport, Color::BLACK),
        &first,
        "cloned token hit",
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(cached.raster_cache_statistics().hits, 1);

    drop(clone);
    begin_frame(&mut cached, 1.0);
    assert!(owner.upgrade().is_none());
    let empty = cached.screenshot(&viewport, Color::BLACK);
    assert!(empty.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    let statistics = cached.raster_cache_statistics();
    assert_eq!(statistics.entries, 0);
    assert_eq!(statistics.bytes, 0);
    assert_eq!(statistics.rasterizations, 1);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(
        gpu.instance
            .generate_report()
            .unwrap()
            .hub
            .textures
            .num_kept_from_user
            < occupied_textures
    );

    drop(cached);
    let Gpu {
        engine,
        device,
        instance,
    } = gpu;
    drop(engine);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let report = instance.generate_report().unwrap().hub;
    assert_eq!(report.textures.num_kept_from_user, 0);
    assert_eq!(report.buffers.num_kept_from_user, 0);
    assert_eq!(report.bind_groups.num_kept_from_user, 0);
    assert_eq!(report.render_pipelines.num_kept_from_user, 0);
}

#[test]
fn nested_cached_layer_stays_live_and_only_outer_surface_is_retained() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let mut direct = gpu.renderer();
    let outer = renderer::Cache::new();
    let inner = renderer::Cache::new();
    let outer_calls = Cell::new(0);
    let inner_calls = Cell::new(0);
    let bounds = rect(8.0, 8.0, 72.0, 48.0);
    let inner_bounds = rect(24.0, 20.0, 24.0, 16.0);

    for (frame, (key, color)) in [
        (1, Color::from_rgb(1.0, 0.0, 0.0)),
        (2, Color::from_rgb(0.0, 1.0, 0.0)),
        (2, Color::from_rgb(0.0, 1.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let viewport = begin_frame(&mut cached, 1.0);
        begin_frame(&mut direct, 1.0);
        cached.with_cached_layer(&outer, key, bounds, |renderer| {
            outer_calls.set(outer_calls.get() + 1);
            fill(renderer, bounds, Color::from_rgb(0.0, 0.0, 1.0));
            renderer.with_cached_layer(&inner, 99, inner_bounds, |renderer| {
                inner_calls.set(inner_calls.get() + 1);
                fill(renderer, inner_bounds, color);
            });
        });
        direct.with_layer(bounds, |renderer| {
            fill(renderer, bounds, Color::from_rgb(0.0, 0.0, 1.0));
            renderer.with_layer(inner_bounds, |renderer| fill(renderer, inner_bounds, color));
        });
        let actual = cached.screenshot(&viewport, Color::BLACK);
        assert_pixels_match(
            &actual,
            &direct.screenshot(&viewport, Color::BLACK),
            "nested live layer",
        );
        assert_eq!(pixel(&actual, 12, 12), [0, 0, 255, 255]);
        assert_eq!(
            pixel(&actual, 30, 24),
            if frame == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        );
        assert_eq!(outer_calls.get(), [1, 2, 2][frame]);
        assert_eq!(inner_calls.get(), [1, 2, 2][frame]);
        let statistics = cached.raster_cache_statistics();
        assert_eq!(statistics.hits, [0, 0, 1][frame]);
        assert_eq!(statistics.misses, [1, 2, 2][frame]);
        assert_eq!(statistics.rasterizations, [1, 2, 2][frame]);
        assert_eq!(statistics.live_fallbacks, [1, 2, 2][frame]);
        assert_eq!(statistics.entries, 1);
        assert_eq!(statistics.bytes, 72 * 48 * 4);
    }
}

#[test]
fn different_keys_for_same_token_in_one_frame_keep_first_paint_and_draw_second_live() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let mut direct = gpu.renderer();
    let token = renderer::Cache::new();
    let calls = Cell::new(0);
    let bounds = rect(8.0, 8.0, 80.0, 48.0);
    for frame in 0..2 {
        let viewport = begin_frame(&mut cached, 1.0);
        begin_frame(&mut direct, 1.0);
        for (key, painted, color) in [
            (
                1,
                rect(8.0, 8.0, 32.0, 48.0),
                Color::from_rgb(1.0, 0.0, 0.0),
            ),
            (
                2,
                rect(56.0, 8.0, 32.0, 48.0),
                Color::from_rgb(0.0, 1.0, 0.0),
            ),
        ] {
            cached.with_cached_layer(&token, key, bounds, |renderer| {
                calls.set(calls.get() + 1);
                fill(renderer, painted, color);
            });
            direct.with_layer(bounds, |renderer| fill(renderer, painted, color));
        }
        let actual = cached.screenshot(&viewport, Color::BLACK);
        assert_pixels_match(
            &actual,
            &direct.screenshot(&viewport, Color::BLACK),
            "same-frame key conflict",
        );
        assert_eq!(pixel(&actual, 16, 16), [255, 0, 0, 255]);
        assert_eq!(pixel(&actual, 64, 16), [0, 255, 0, 255]);
        assert_eq!(pixel(&actual, 48, 16), [0, 0, 0, 255]);
        assert_eq!(calls.get(), frame + 2);
        let statistics = cached.raster_cache_statistics();
        assert_eq!(statistics.hits, frame as u64);
        assert_eq!(statistics.misses, 1);
        assert_eq!(statistics.rasterizations, 1);
        assert_eq!(statistics.live_fallbacks, frame as u64 + 1);
        assert_eq!(statistics.entries, 1);
    }
}

#[test]
fn reduced_budget_evicts_old_surfaces_and_falls_back_live_when_frame_is_pinned() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let tokens = [renderer::Cache::new(), renderer::Cache::new()];
    let calls = [Cell::new(0), Cell::new(0)];
    let budget = 32 * 24 * 4;
    cached.set_raster_cache_budget(budget);
    let paint = |renderer: &mut iced_wgpu::Renderer, index: usize| {
        calls[index].set(calls[index].get() + 1);
        fill(
            renderer,
            rect(8.0 + index as f32 * 48.0, 8.0, 32.0, 24.0),
            if index == 0 {
                Color::from_rgb(1.0, 0.0, 0.0)
            } else {
                Color::from_rgb(0.0, 1.0, 0.0)
            },
        );
    };
    for (frame, index) in [0, 1, 0].into_iter().enumerate() {
        let viewport = begin_frame(&mut cached, 1.0);
        let bounds = rect(8.0 + index as f32 * 48.0, 8.0, 32.0, 24.0);
        cached.with_cached_layer(&tokens[index], 1, bounds, |renderer| paint(renderer, index));
        let actual = cached.screenshot(&viewport, Color::BLACK);
        assert_eq!(
            pixel(&actual, 12 + index as u32 * 48, 12),
            if index == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        );
        let statistics = cached.raster_cache_statistics();
        assert_eq!(statistics.hits, 0);
        assert_eq!(statistics.misses, frame as u64 + 1);
        assert_eq!(statistics.rasterizations, frame as u64 + 1);
        assert_eq!(statistics.entries, 1);
        assert_eq!(statistics.bytes, budget);
    }
    let viewport = begin_frame(&mut cached, 1.0);
    for index in 0..2 {
        cached.with_cached_layer(
            &tokens[index],
            1,
            rect(8.0 + index as f32 * 48.0, 8.0, 32.0, 24.0),
            |renderer| paint(renderer, index),
        );
    }
    let actual = cached.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&actual, 12, 12), [255, 0, 0, 255]);
    assert_eq!(pixel(&actual, 60, 12), [0, 255, 0, 255]);
    assert_eq!((calls[0].get(), calls[1].get()), (2, 2));
    let statistics = cached.raster_cache_statistics();
    assert_eq!(statistics.hits, 1);
    assert_eq!(statistics.misses, 3);
    assert_eq!(statistics.rasterizations, 3);
    assert_eq!(statistics.live_fallbacks, 1);
    assert_eq!(statistics.entries, 1);
    assert!(statistics.bytes <= budget);

    begin_frame(&mut cached, 1.0);
    cached.set_raster_cache_budget(0);
    cached.with_cached_layer(&tokens[0], 1, rect(8.0, 8.0, 32.0, 24.0), |renderer| {
        paint(renderer, 0)
    });
    let actual = cached.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&actual, 12, 12), [255, 0, 0, 255]);
    assert_eq!(calls[0].get(), 3);
    let statistics = cached.raster_cache_statistics();
    assert_eq!(statistics.entries, 0);
    assert_eq!(statistics.bytes, 0);
    assert_eq!(statistics.rasterizations, 3);
    assert_eq!(statistics.live_fallbacks, 2);
}

#[test]
fn software_renderer_keeps_cached_layer_closure_live_for_same_key() {
    let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("headless software renderer");
    assert!(renderer.name().contains("tiny-skia"));
    let token = renderer::Cache::new();
    let calls = Cell::new(0);
    for (frame, color) in [
        Color::from_rgb(1.0, 0.0, 0.0),
        Color::from_rgb(0.0, 1.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        renderer.reset(rect(0.0, 0.0, WIDTH as f32, HEIGHT as f32));
        renderer.hint(1.0);
        renderer.with_cached_layer(&token, 1, rect(8.0, 8.0, 32.0, 24.0), |renderer| {
            calls.set(calls.get() + 1);
            fill(renderer, rect(8.0, 8.0, 32.0, 24.0), color);
        });
        let actual = renderer.screenshot(Size::new(WIDTH, HEIGHT), 1.0, Color::BLACK);
        assert_eq!(
            pixel(&actual, 12, 12),
            if frame == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        );
        assert_eq!(calls.get(), frame + 1);
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
    Rectangle {
        x,
        y,
        width,
        height,
    }
}

fn begin_frame(renderer: &mut iced_wgpu::Renderer, scale: f32) -> Viewport {
    let viewport = Viewport::with_physical_size(Size::new(WIDTH, HEIGHT), scale);
    renderer.reset(Rectangle::with_size(viewport.logical_size()));
    // Cache lookup happens while recording, before draw sees the viewport.
    renderer.hint(scale);
    viewport
}

fn fill(renderer: &mut impl renderer::Renderer, bounds: Rectangle, color: Color) {
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            snap: false,
            ..Default::default()
        },
        color,
    );
}

fn pixel(bytes: &[u8], x: u32, y: u32) -> &[u8] {
    let offset = ((y * WIDTH + x) * 4) as usize;
    &bytes[offset..offset + 4]
}

fn assert_pixels_match(actual: &[u8], expected: &[u8], context: &str) {
    // Compositing an RGBA8 intermediate can introduce one rounding unit.
    assert_pixel_difference_at_most(actual, expected, context, 1);
}

fn assert_srgb_pixels_match(actual: &[u8], expected: &[u8], context: &str) {
    // The retained sRGB surface adds an 8-bit RGB encode/decode round trip and
    // alpha quantization before final blending. These scenes allow at most two
    // readback units for that extra rounding; RGBA8 keeps its one-unit bound.
    assert_pixel_difference_at_most(actual, expected, context, 2);
}

fn assert_pixel_difference_at_most(actual: &[u8], expected: &[u8], context: &str, tolerance: u8) {
    assert_eq!(actual.len(), (WIDTH * HEIGHT * 4) as usize, "{context}");
    assert_eq!(actual.len(), expected.len(), "{context}");
    let (maximum, index, actual, expected) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .map(|(index, (&actual, &expected))| (actual.abs_diff(expected), index, actual, expected))
        .max_by_key(|(difference, ..)| *difference)
        .expect("nonempty readback");
    assert!(
        maximum <= tolerance,
        "{context}: maximum channel difference {maximum} exceeds {tolerance} at pixel ({}, {}), channel {}: {actual} != {expected}",
        index / 4 % WIDTH as usize,
        index / 4 / WIDTH as usize,
        index % 4,
    );
}

#[test]
fn cold_hit_and_changed_key_skip_only_unchanged_paint_and_match_direct_pixels() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let mut direct = gpu.renderer();
    let token = renderer::Cache::new();
    let calls = Cell::new(0);
    let bounds = rect(16.0, 12.0, 48.0, 32.0);
    let red = Color::from_rgb(1.0, 0.0, 0.0);
    let green = Color::from_rgb(0.0, 1.0, 0.0);

    for (frame, (key, color)) in [(7, red), (7, red), (8, green)].into_iter().enumerate() {
        let viewport = begin_frame(&mut cached, 1.0);
        begin_frame(&mut direct, 1.0);
        cached.with_cached_layer(&token, key, bounds, |renderer| {
            calls.set(calls.get() + 1);
            fill(renderer, bounds, color);
        });
        direct.with_layer(bounds, |renderer| fill(renderer, bounds, color));
        if frame == 0 {
            assert_eq!(cached.raster_cache_statistics().rasterizations, 0);
        }

        let actual = cached.screenshot(&viewport, Color::BLACK);
        let expected = direct.screenshot(&viewport, Color::BLACK);
        assert_pixels_match(&actual, &expected, "cold/hit/key change");
        assert_eq!(
            pixel(&actual, 24, 20),
            if frame == 2 {
                [0, 255, 0, 255]
            } else {
                [255, 0, 0, 255]
            }
        );
        assert_eq!(pixel(&actual, 4, 4), [0, 0, 0, 255]);
        assert_eq!(calls.get(), [1, 1, 2][frame]);
        let statistics = cached.raster_cache_statistics();
        assert_eq!(statistics.hits, [0, 1, 1][frame]);
        assert_eq!(statistics.misses, [1, 1, 2][frame]);
        assert_eq!(statistics.rasterizations, [1, 1, 2][frame]);
        assert_eq!(statistics.live_fallbacks, 0);
        assert_eq!(statistics.entries, 1);
        assert_eq!(statistics.bytes, 48 * 32 * 4);
        assert!(statistics.bytes <= DEFAULT_BUDGET);
    }
}

fn transparency_scene(
    renderer: &mut iced_wgpu::Renderer,
    token: Option<&renderer::Cache>,
    backdrop: Color,
    calls: &Cell<u32>,
) {
    let frame = rect(0.0, 0.0, WIDTH as f32, HEIGHT as f32);
    let parent = rect(16.0, 12.0, 64.0, 48.0);
    renderer.with_layer(frame, |renderer| fill(renderer, frame, backdrop));
    renderer.with_layer(parent, |renderer| {
        let paint = |renderer: &mut iced_wgpu::Renderer| {
            calls.set(calls.get() + 1);
            fill(
                renderer,
                rect(8.0, 4.0, 64.0, 40.0),
                Color::from_rgba(1.0, 0.0, 0.0, 0.5),
            );
            fill(
                renderer,
                rect(40.0, 24.0, 56.0, 48.0),
                Color::from_rgba(0.0, 1.0, 0.0, 0.5),
            );
            renderer.with_layer(rect(48.0, 28.0, 12.0, 12.0), |renderer| {
                fill(renderer, frame, Color::from_rgb(0.0, 1.0, 1.0));
            });
        };
        if let Some(token) = token {
            // Requested bounds exceed the parent. Retained geometry must crop
            // to it; the direct reference explicitly uses that intersection.
            renderer.with_cached_layer(token, 41, rect(8.0, 4.0, 88.0, 72.0), paint);
        } else {
            renderer.with_layer(parent, paint);
        }
    });
    renderer.with_layer(frame, |renderer| {
        fill(
            renderer,
            rect(56.0, 36.0, 16.0, 16.0),
            Color::from_rgba(1.0, 1.0, 0.0, 0.5),
        );
    });
}

#[test]
fn retained_transparency_painter_order_and_parent_clipping_match_direct_rendering() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new() else { return };
    let mut cached = gpu.renderer();
    let mut direct = gpu.renderer();
    let token = renderer::Cache::new();
    let calls = Cell::new(0);
    let direct_calls = Cell::new(0);

    for (frame, backdrop) in [
        Color::from_rgb(0.0, 0.0, 1.0),
        Color::WHITE,
        Color::TRANSPARENT,
    ]
    .into_iter()
    .enumerate()
    {
        let viewport = begin_frame(&mut cached, 1.0);
        begin_frame(&mut direct, 1.0);
        transparency_scene(&mut cached, Some(&token), backdrop, &calls);
        transparency_scene(&mut direct, None, backdrop, &direct_calls);
        let actual = cached.screenshot(&viewport, Color::TRANSPARENT);
        let expected = direct.screenshot(&viewport, Color::TRANSPARENT);
        assert_pixels_match(&actual, &expected, "transparency/order/parent clipping");
        assert_eq!(pixel(&actual, 52, 32), [0, 255, 255, 255]);
        assert_eq!(pixel(&actual, 12, 20), pixel(&expected, 120, 80));
        assert_eq!(pixel(&actual, 20, 56), pixel(&expected, 120, 80));
        assert!(
            pixel(&actual, 20, 20)[0] > 0,
            "red translucent paint visible"
        );
        assert!(pixel(&actual, 44, 32)[1] > 0, "green overlap visible");
        assert!(
            pixel(&actual, 60, 40)[0] > pixel(&actual, 44, 32)[0],
            "later yellow layer paints above cache"
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(direct_calls.get(), frame as u32 + 1);
        let statistics = cached.raster_cache_statistics();
        assert_eq!(statistics.hits, frame as u64);
        assert_eq!(statistics.misses, 1);
        assert_eq!(statistics.rasterizations, 1);
        assert_eq!(statistics.live_fallbacks, 0);
        assert_eq!(statistics.entries, 1);
        assert_eq!(statistics.bytes, 64 * 48 * 4);
    }
}
