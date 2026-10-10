#![cfg(feature = "hybrid-rendering")]

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::text::{self, Renderer as _};
use iced::{Color, Font, Pixels, Point, Rectangle, Size, Transformation, alignment};
use iced_wgpu::graphics::{Antialiasing, Shell, Viewport, color, mesh};
use iced_wgpu::primitive::{self, Renderer as _};
use mesh::Renderer as _;

const SIZE: Size<u32> = Size::new(128, 96);
const BUDGET: u64 = 32 * 1024 * 1024;
const FORMATS: [wgpu::TextureFormat; 2] = [
    wgpu::TextureFormat::Rgba8Unorm,
    wgpu::TextureFormat::Rgba8UnormSrgb,
];

// The same small headless Vulkan setup as vulkan_raster_cache.rs. Only adapter
// absence skips validation; device, rendering, and readback errors fail it.
struct Gpu {
    engine: iced_wgpu::Engine,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
}

impl Gpu {
    fn new(format: wgpu::TextureFormat, antialiasing: Option<Antialiasing>) -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let Some(adapter) =
            futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
        else {
            eprintln!("Skipping composition validation: no Vulkan adapter");
            return None;
        };
        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&Default::default()))
                .expect("create Vulkan composition test device");
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            format,
            antialiasing,
            Shell::headless(),
        );
        Some(Self {
            engine,
            device,
            queue,
            format,
        })
    }

    fn renderer(&self, enabled: bool) -> iced_wgpu::Renderer {
        let mut renderer =
            iced_wgpu::Renderer::new(self.engine.clone(), renderer::Settings::default());
        // These small fixtures must exercise retention even when production
        // heuristics would correctly choose direct painting for their cost.
        renderer.set_composition_cache_heuristics_enabled(false);
        renderer.set_composition_cache_budget(if enabled { BUDGET } else { 0 });
        renderer
    }

    fn target(&self, copy_dst: bool) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("composition presentation target"),
            size: wgpu::Extent3d {
                width: SIZE.width,
                height: SIZE.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | if copy_dst {
                    wgpu::TextureUsages::COPY_DST
                } else {
                    wgpu::TextureUsages::empty()
                },
            view_formats: &[],
        })
    }

    fn draw(
        &self,
        renderer: &mut iced_wgpu::Renderer,
        target: &wgpu::Texture,
        viewport: &Viewport,
        clear: Option<Color>,
    ) -> Vec<u8> {
        let view = target.create_view(&Default::default());
        let encoder = renderer.draw(clear, &view, viewport);
        renderer.finish();
        self.queue.submit([encoder.finish()]);
        renderer.recall();
        self.read(target)
    }

    fn read(&self, target: &wgpu::Texture) -> Vec<u8> {
        let row = SIZE.width * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composition readback"),
            size: u64::from(padded) * u64::from(SIZE.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(SIZE.height),
                },
            },
            wgpu::Extent3d {
                width: SIZE.width,
                height: SIZE.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        receiver.recv().unwrap().expect("map composition readback");
        let mapped = buffer.slice(..).get_mapped_range();
        let pixels = mapped
            .chunks_exact(padded as usize)
            .flat_map(|bytes| bytes[..row as usize].iter().copied())
            .collect();
        drop(mapped);
        buffer.unmap();
        pixels
    }
}

fn lock_vulkan_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
    Rectangle {
        x,
        y,
        width,
        height,
    }
}

fn begin(renderer: &mut iced_wgpu::Renderer, size: Size<u32>, scale: f32) -> Viewport {
    let viewport = Viewport::with_physical_size(size, scale);
    renderer.reset(Rectangle::with_size(viewport.logical_size()));
    renderer.hint(scale);
    viewport
}

fn fill(renderer: &mut iced_wgpu::Renderer, bounds: Rectangle, color: Color) {
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            snap: false,
            ..Default::default()
        },
        color,
    );
}

fn label(
    renderer: &mut iced_wgpu::Renderer,
    content: &str,
    position: Point,
    color: Color,
    clip: Rectangle,
) {
    renderer.fill_text(
        text::Text {
            content: content.to_owned(),
            bounds: Size::new(36.0, 16.0),
            size: Pixels(12.0),
            line_height: text::LineHeight::Absolute(Pixels(16.0)),
            font: Font::DEFAULT,
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: None,
        },
        position,
        color,
        clip,
    );
}

fn image(
    renderer: &mut iced_wgpu::Renderer,
    handle: &image::Handle,
    bounds: Rectangle,
    clip: Rectangle,
    opacity: f32,
) {
    renderer.draw_image(
        image::Image::new(handle.clone())
            .filter_method(image::FilterMethod::Nearest)
            .opacity(opacity),
        bounds,
        clip,
    );
}

fn icon() -> image::Handle {
    image::Handle::from_rgba(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ],
    )
}

fn assert_pixels(actual: &[u8], expected: &[u8], size: Size<u32>, context: &str) {
    assert_eq!(
        actual.len(),
        (size.width * size.height * 4) as usize,
        "{context}"
    );
    assert_eq!(actual.len(), expected.len(), "{context}");
    let (index, (&actual, &expected)) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .max_by_key(|(_, (a, b))| a.abs_diff(**b))
        .unwrap();
    assert!(
        actual.abs_diff(expected) <= 1,
        "{context}: ({}, {}) channel {}: {actual} != {expected}",
        index / 4 % size.width as usize,
        index / 4 / size.width as usize,
        index % 4
    );
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> &[u8] {
    let index = ((y * SIZE.width + x) * 4) as usize;
    &pixels[index..index + 4]
}

fn assert_reused(
    before: iced_wgpu::CompositionCacheStatistics,
    after: iced_wgpu::CompositionCacheStatistics,
) {
    assert_eq!(after.reused_frames, before.reused_frames + 1);
    assert_eq!(after.full_repaints, before.full_repaints);
    assert_eq!(after.partial_repaints, before.partial_repaints);
    assert_eq!(after.damaged_pixels, before.damaged_pixels);
}

fn assert_partial(
    before: iced_wgpu::CompositionCacheStatistics,
    after: iced_wgpu::CompositionCacheStatistics,
) {
    assert_eq!(after.partial_repaints, before.partial_repaints + 1);
    assert_eq!(after.full_repaints, before.full_repaints);
    assert_eq!(after.reused_frames, before.reused_frames);
    let damaged = after.damaged_pixels - before.damaged_pixels;
    assert!(
        damaged > 0 && damaged < u64::from(SIZE.width * SIZE.height) / 2,
        "small clipped paint damaged {damaged} pixels"
    );
}

fn scene(
    renderer: &mut iced_wgpu::Renderer,
    handle: &image::Handle,
    viewport: &Viewport,
    step: usize,
) {
    let frame = Rectangle::with_size(viewport.logical_size());
    // This earlier layer is skipped on partial frames; all three batch indices
    // must still select the later prepared layers, including after removals.
    let outside = rect(46.0, 34.0, 14.0, 12.0);
    renderer.with_layer(outside, |renderer| {
        fill(renderer, outside, Color::from_rgb(0.0, 0.0, 1.0));
        image(renderer, handle, rect(46.0, 34.0, 6.0, 6.0), outside, 1.0);
        label(renderer, "I", Point::new(52.0, 34.0), Color::WHITE, outside);
    });
    let clip = rect(4.0, 4.0, 40.0, 24.0);
    renderer.with_layer(clip, |renderer| {
        // Keep the layer present even after its changing content is removed.
        fill(renderer, rect(4.0, 4.0, 2.0, 2.0), Color::WHITE);
        for (kind, opacity_step) in [(0, 2), (1, 5), (2, 8)] {
            if step >= opacity_step + 2 && step < 12 {
                continue;
            }
            let alpha = if step >= opacity_step && step < 12 {
                0.35
            } else {
                1.0
            };
            let shift = if step == opacity_step + 1 { 6.0 } else { 0.0 };
            let bounds = rect(2.0 + shift, 6.0, 22.0, 18.0);
            match kind {
                0 => fill(renderer, bounds, Color::from_rgba(1.0, 0.0, 0.0, alpha)),
                1 => label(
                    renderer,
                    "Move",
                    Point::new(2.0 + shift, 6.0),
                    Color::from_rgba(1.0, 1.0, 1.0, alpha),
                    clip,
                ),
                _ => image(
                    renderer,
                    handle,
                    rect(20.0 + shift, 8.0, 18.0, 16.0),
                    clip,
                    alpha,
                ),
            }
        }
    });
    // A later, large clipped layer intersects every damage region and must
    // composite using its own quad, image, and text preparation indices.
    renderer.with_layer(frame, |renderer| {
        fill(
            renderer,
            rect(18.0, 12.0, 8.0, 6.0),
            Color::from_rgba(0.0, 1.0, 1.0, 0.45),
        );
        image(renderer, handle, rect(30.0, 8.0, 10.0, 10.0), frame, 0.65);
        label(renderer, "Top", Point::new(16.0, 22.0), Color::WHITE, frame);
    });
}

#[test]
fn clipped_opacity_movement_removal_and_later_batch_indices_match_uncached_pixels() {
    let _guard = lock_vulkan_test();
    for format in FORMATS {
        let Some(gpu) = Gpu::new(format, None) else {
            return;
        };
        for scale in [1.0, 1.5, 2.0] {
            let mut cached = gpu.renderer(true);
            let mut direct = gpu.renderer(false);
            let handle = icon();
            let _leases = [
                cached.load_image(&handle).unwrap(),
                direct.load_image(&handle).unwrap(),
            ];
            let clear = Color::from_rgba(0.08, 0.2, 0.4, 0.35);
            let mut previous = Vec::new();
            for step in 0..14 {
                let viewport = begin(&mut cached, SIZE, scale);
                begin(&mut direct, SIZE, scale);
                scene(&mut cached, &handle, &viewport, step);
                scene(&mut direct, &handle, &viewport, step);
                let before = cached.composition_cache_statistics();
                let actual = cached.screenshot(&viewport, clear);
                let expected = direct.screenshot(&viewport, clear);
                assert_pixels(
                    &actual,
                    &expected,
                    SIZE,
                    &format!("{format:?}, scale {scale}, step {step}"),
                );
                let after = cached.composition_cache_statistics();
                match step {
                    0 => assert_eq!(after.full_repaints, 1),
                    1 | 11 | 13 => assert_reused(before, after),
                    _ => {
                        assert_partial(before, after);
                        assert_ne!(
                            actual, previous,
                            "changed paint must change pixels at step {step}"
                        );
                    }
                }
                if step > 0 {
                    // Fractional DPI rounding and the damage AA margin still
                    // must leave the unrelated lower/right pixels untouched.
                    for y in 0..SIZE.height {
                        for x in 0..SIZE.width {
                            if x as f32 > 44.0 * scale + 2.0 || y as f32 > 28.0 * scale + 2.0 {
                                assert_eq!(pixel(&actual, x, y), pixel(&previous, x, y));
                            }
                        }
                    }
                }
                previous = actual;
                assert_eq!(after.bytes, u64::from(SIZE.width * SIZE.height) * 4);
                assert_eq!(direct.composition_cache_statistics().bytes, 0);
            }
        }
    }
}

fn simple_scene(renderer: &mut iced_wgpu::Renderer, moved: bool) {
    fill(
        renderer,
        rect(if moved { 16.0 } else { 4.0 }, 4.0, 20.0, 12.0),
        Color::from_rgba(1.0, 0.0, 0.0, 0.6),
    );
}

#[test]
fn clear_dpi_resize_and_budget_changes_invalidate_the_final_frame() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new(wgpu::TextureFormat::Rgba8UnormSrgb, None) else {
        return;
    };
    let mut cached = gpu.renderer(true);
    let mut direct = gpu.renderer(false);
    let tint = Color::from_rgba(0.2, 0.6, 0.8, 0.4);
    let bigger = Size::new(144, 104);
    for (size, scale, clear, reused) in [
        (SIZE, 1.0, Color::TRANSPARENT, false),
        (SIZE, 1.0, Color::TRANSPARENT, true),
        (SIZE, 1.0, tint, false),
        (SIZE, 1.0, tint, true),
        (SIZE, 1.5, tint, false),
        (SIZE, 1.5, tint, true),
        (SIZE, 2.0, tint, false),
        (bigger, 2.0, tint, false),
        (bigger, 2.0, tint, true),
        (SIZE, 2.0, tint, false),
    ] {
        let viewport = begin(&mut cached, size, scale);
        begin(&mut direct, size, scale);
        simple_scene(&mut cached, false);
        simple_scene(&mut direct, false);
        let before = cached.composition_cache_statistics();
        assert_pixels(
            &cached.screenshot(&viewport, clear),
            &direct.screenshot(&viewport, clear),
            size,
            "clear/DPI/resize invalidation",
        );
        let after = cached.composition_cache_statistics();
        if reused {
            assert_reused(before, after);
        } else {
            assert_eq!(after.full_repaints, before.full_repaints + 1);
            assert_eq!(after.partial_repaints, before.partial_repaints);
            assert_eq!(
                after.damaged_pixels - before.damaged_pixels,
                u64::from(size.width) * u64::from(size.height)
            );
        }
        assert_eq!(
            after.bytes,
            u64::from(size.width) * u64::from(size.height) * 4
        );
    }
    for budget in [0, u64::from(SIZE.width * SIZE.height) * 4 - 1] {
        cached.set_composition_cache_budget(budget);
        assert_eq!(cached.composition_cache_statistics().bytes, 0);
        for _ in 0..2 {
            let viewport = begin(&mut cached, SIZE, 2.0);
            begin(&mut direct, SIZE, 2.0);
            simple_scene(&mut cached, true);
            simple_scene(&mut direct, true);
            let before = cached.composition_cache_statistics();
            assert_pixels(
                &cached.screenshot(&viewport, tint),
                &direct.screenshot(&viewport, tint),
                SIZE,
                "disabled or undersized budget",
            );
            let after = cached.composition_cache_statistics();
            assert_eq!(
                (
                    after.full_repaints,
                    after.partial_repaints,
                    after.reused_frames,
                    after.damaged_pixels
                ),
                (
                    before.full_repaints,
                    before.partial_repaints,
                    before.reused_frames,
                    before.damaged_pixels
                )
            );
            assert_eq!(after.bytes, 0);
        }
    }
    cached.set_composition_cache_budget(BUDGET);
    let before = cached.composition_cache_statistics();
    let viewport = begin(&mut cached, SIZE, 2.0);
    simple_scene(&mut cached, true);
    assert_pixels(
        &cached.screenshot(&viewport, tint),
        &direct.screenshot(&viewport, tint),
        SIZE,
        "re-enabled budget",
    );
    assert_eq!(
        cached.composition_cache_statistics().full_repaints,
        before.full_repaints + 1
    );
}

#[test]
fn copy_and_shader_presentation_replace_rgba_including_transparent_pixels() {
    let _guard = lock_vulkan_test();
    for format in FORMATS {
        let Some(gpu) = Gpu::new(format, None) else {
            return;
        };
        let mut copy = gpu.renderer(true);
        let mut shader = gpu.renderer(true);
        let mut direct = gpu.renderer(false);
        let copy_target = gpu.target(true);
        let shader_target = gpu.target(false);
        let reference = gpu.target(false);
        let viewport = Viewport::with_physical_size(SIZE, 1.0);
        for step in 0..4 {
            // Fresh opaque external pixels must be replaced, even on reuse.
            begin(&mut direct, SIZE, 1.0);
            let _ = gpu.draw(&mut direct, &copy_target, &viewport, Some(Color::WHITE));
            let _ = gpu.draw(&mut direct, &shader_target, &viewport, Some(Color::WHITE));
            for renderer in [&mut copy, &mut shader, &mut direct] {
                begin(renderer, SIZE, 1.0);
                fill(renderer, rect(2.0, 2.0, 2.0, 2.0), Color::WHITE);
                if step < 2 {
                    simple_scene(renderer, false);
                }
            }
            let before = copy.composition_cache_statistics();
            let expected = gpu.draw(&mut direct, &reference, &viewport, Some(Color::TRANSPARENT));
            let dma = gpu.draw(&mut copy, &copy_target, &viewport, Some(Color::TRANSPARENT));
            let replacement = gpu.draw(
                &mut shader,
                &shader_target,
                &viewport,
                Some(Color::TRANSPARENT),
            );
            assert_eq!(
                dma, expected,
                "texture copy must preserve exact RGBA: {format:?}"
            );
            assert_pixels(
                &replacement,
                &expected,
                SIZE,
                "shader presentation replacement",
            );
            assert_eq!(pixel(&replacement, 120, 80), [0, 0, 0, 0]);
            if step == 1 || step == 3 {
                assert_reused(before, copy.composition_cache_statistics());
            }
            if step == 2 {
                assert_partial(before, copy.composition_cache_statistics());
            }
            let a = copy.composition_cache_statistics();
            let b = shader.composition_cache_statistics();
            assert_eq!(
                (
                    a.full_repaints,
                    a.partial_repaints,
                    a.reused_frames,
                    a.damaged_pixels
                ),
                (
                    b.full_repaints,
                    b.partial_repaints,
                    b.reused_frames,
                    b.damaged_pixels
                )
            );
        }
    }
}

#[test]
fn draw_without_clear_preserves_external_pixels_and_discards_retention() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new(wgpu::TextureFormat::Rgba8Unorm, None) else {
        return;
    };
    let mut cached = gpu.renderer(true);
    let mut direct = gpu.renderer(false);
    let target = gpu.target(true);
    let reference = gpu.target(false);
    let viewport = begin(&mut cached, SIZE, 1.0);
    simple_scene(&mut cached, false);
    let _ = cached.screenshot(&viewport, Color::TRANSPARENT);
    for background in [
        Color::from_rgb(0.0, 0.0, 1.0),
        Color::from_rgb(0.0, 1.0, 0.0),
    ] {
        begin(&mut direct, SIZE, 1.0);
        let _ = gpu.draw(&mut direct, &target, &viewport, Some(background));
        let _ = gpu.draw(&mut direct, &reference, &viewport, Some(background));
        begin(&mut cached, SIZE, 1.0);
        begin(&mut direct, SIZE, 1.0);
        simple_scene(&mut cached, false);
        simple_scene(&mut direct, false);
        let before = cached.composition_cache_statistics();
        let actual = gpu.draw(&mut cached, &target, &viewport, None);
        let expected = gpu.draw(&mut direct, &reference, &viewport, None);
        assert_eq!(actual, expected);
        assert_eq!(
            pixel(&actual, 120, 80),
            if background.b == 1.0 {
                [0, 0, 255, 255]
            } else {
                [0, 255, 0, 255]
            }
        );
        let after = cached.composition_cache_statistics();
        assert_eq!(after.bytes, 0);
        assert_eq!(
            (
                after.full_repaints,
                after.partial_repaints,
                after.reused_frames,
                after.damaged_pixels
            ),
            (
                before.full_repaints,
                before.partial_repaints,
                before.reused_frames,
                before.damaged_pixels
            )
        );
    }
    let before = cached.composition_cache_statistics();
    let actual = gpu.draw(&mut cached, &target, &viewport, Some(Color::TRANSPARENT));
    let expected = gpu.draw(&mut direct, &reference, &viewport, Some(Color::TRANSPARENT));
    assert_eq!(actual, expected);
    assert_eq!(
        cached.composition_cache_statistics().full_repaints,
        before.full_repaints + 1
    );
}

#[test]
fn msaa_mesh_partial_repaints_preserve_pixels_outside_the_clipped_layer() {
    let _guard = lock_vulkan_test();
    for format in FORMATS {
        let Some(gpu) = Gpu::new(format, Some(Antialiasing::MSAAx4)) else {
            return;
        };
        for scale in [1.0, 1.5, 2.0] {
            let mut cached = gpu.renderer(true);
            let mut direct = gpu.renderer(false);
            let clip = rect(4.25, 4.25, 24.5, 18.5);
            let mut previous = Vec::new();
            for step in 0..4 {
                let viewport = begin(&mut cached, SIZE, scale);
                begin(&mut direct, SIZE, scale);
                for renderer in [&mut cached, &mut direct] {
                    let frame = Rectangle::with_size(viewport.logical_size());
                    fill(renderer, frame, Color::from_rgb(0.0, 0.0, 1.0));
                    fill(
                        renderer,
                        rect(40.0, 28.0, 20.0, 16.0),
                        Color::from_rgb(1.0, 1.0, 0.0),
                    );
                    renderer.with_layer(clip, |renderer| {
                        let paint = if step < 2 {
                            Color::from_rgba(1.0, 0.0, 0.0, 0.4)
                        } else {
                            Color::from_rgba(0.0, 1.0, 0.0, 0.7)
                        };
                        renderer.draw_mesh(mesh::Mesh::Solid {
                            buffers: mesh::Indexed {
                                vertices: [[0.0, 0.0], [60.0, 0.0], [0.0, 44.0]]
                                    .map(|position| mesh::SolidVertex2D {
                                        position,
                                        color: color::pack(paint),
                                    })
                                    .into(),
                                indices: vec![0, 1, 2],
                            },
                            transformation: Transformation::IDENTITY,
                            clip_bounds: frame,
                        });
                    });
                    renderer.with_layer(frame, |renderer| {
                        fill(
                            renderer,
                            rect(18.0, 12.0, 6.0, 6.0),
                            Color::from_rgba(1.0, 1.0, 1.0, 0.5),
                        );
                    });
                }
                let before = cached.composition_cache_statistics();
                let actual = cached.screenshot(&viewport, Color::TRANSPARENT);
                let expected = direct.screenshot(&viewport, Color::TRANSPARENT);
                assert_pixels(
                    &actual,
                    &expected,
                    SIZE,
                    &format!("MSAA {format:?}, scale {scale}, step {step}"),
                );
                if step > 0 {
                    // Mesh paint is opaque to the planner: even unchanged mesh
                    // instances require a clipped partial repaint, never reuse.
                    assert_partial(before, cached.composition_cache_statistics());
                    for y in 0..SIZE.height {
                        for x in 0..SIZE.width {
                            if x as f32 > 30.0 * scale + 2.0 || y as f32 > 24.0 * scale + 2.0 {
                                assert_eq!(
                                    pixel(&actual, x, y),
                                    pixel(&previous, x, y),
                                    "MSAA resolve escaped damage"
                                );
                            }
                        }
                    }
                }
                assert_eq!(pixel(&actual, 120, 80), [0, 0, 255, 255]);
                previous = actual;
            }
        }
    }
}

#[test]
fn pending_image_pixels_cannot_freeze_the_final_frame_before_upload_completion() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new(wgpu::TextureFormat::Rgba8Unorm, None) else {
        return;
    };
    let mut cached = gpu.renderer(true);
    let mut direct = gpu.renderer(false);
    // Exactly 2 MiB selects the asynchronous RGBA upload path without codecs.
    let pending = image::Handle::from_rgba(512, 1024, [255, 0, 255, 255].repeat(512 * 1024));
    let small = icon();
    let _leases = [
        direct.load_image(&pending).unwrap(),
        direct.load_image(&small).unwrap(),
        cached.load_image(&small).unwrap(),
    ];
    let record = |renderer: &mut iced_wgpu::Renderer| {
        let clip = rect(4.0, 4.0, 40.0, 24.0);
        renderer.with_layer(clip, |renderer| {
            fill(renderer, clip, Color::WHITE);
            image(renderer, &pending, rect(8.0, 8.0, 24.0, 16.0), clip, 1.0);
        });
        let frame = Rectangle::with_size(Size::new(128.0, 96.0));
        renderer.with_layer(frame, |renderer| {
            // Later uploads may receive the earlier worker completion during
            // prepare. An earlier omission must still invalidate this frame.
            for index in 0..16 {
                image(
                    renderer,
                    &small,
                    rect(index as f32 * 8.0, 40.0, 8.0, 8.0),
                    frame,
                    1.0,
                );
            }
        });
    };
    let viewport = begin(&mut direct, SIZE, 1.0);
    record(&mut direct);
    let expected = direct.screenshot(&viewport, Color::BLACK);
    let mut complete = false;
    for attempt in 0..80 {
        cached.tick();
        begin(&mut cached, SIZE, 1.0);
        record(&mut cached);
        let actual = cached.screenshot(&viewport, Color::BLACK);
        if attempt == 0 {
            assert_eq!(pixel(&actual, 16, 16), [255, 255, 255, 255]);
        }
        if actual == expected {
            complete = true;
            break;
        }
        assert_eq!(
            cached.composition_cache_statistics().reused_frames,
            0,
            "incomplete final-frame pixels became reusable"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        complete,
        "pending image must replace omitted pixels without changing the scene"
    );
    assert!(cached.composition_cache_statistics().full_repaints >= 2);
    let before = cached.composition_cache_statistics();
    begin(&mut cached, SIZE, 1.0);
    record(&mut cached);
    assert_eq!(cached.screenshot(&viewport, Color::BLACK), expected);
    assert_reused(before, cached.composition_cache_statistics());
}

#[derive(Debug)]
struct SparsePrimitive(std::sync::Arc<std::sync::atomic::AtomicUsize>);

struct SparsePipeline;

impl primitive::Pipeline for SparsePipeline {
    fn new(_: &wgpu::Device, _: &wgpu::Queue, _: wgpu::TextureFormat) -> Self {
        Self
    }
}

impl primitive::Primitive for SparsePrimitive {
    type Pipeline = SparsePipeline;

    fn prepare(
        &self,
        _: &mut SparsePipeline,
        _: &wgpu::Device,
        _: &wgpu::Queue,
        _: &Rectangle,
        _: &Viewport,
    ) {
    }

    fn draw(&self, _: &SparsePipeline, _: &mut wgpu::RenderPass<'_>) -> bool {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        true
    }
}

#[test]
fn default_adaptive_policy_bypasses_sparse_and_scrolling_scenes_then_reuses_quiet_frames() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new(wgpu::TextureFormat::Rgba8Unorm, None) else {
        return;
    };
    // Construct directly: the ordinary harness deliberately opts out above.
    let mut adaptive = iced_wgpu::Renderer::new(gpu.engine.clone(), renderer::Settings::default());
    let mut direct = gpu.renderer(false);
    let calls = [
        std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    ];
    for (frame, alpha) in [0.2, 0.6, 0.2].into_iter().enumerate() {
        let viewport = begin(&mut adaptive, SIZE, 1.0);
        begin(&mut direct, SIZE, 1.0);
        for (renderer, calls) in [(&mut adaptive, &calls[0]), (&mut direct, &calls[1])] {
            simple_scene(renderer, false);
            fill(
                renderer,
                rect(8.0, 8.0, 8.0, 8.0),
                Color::from_rgba(0.0, 1.0, 0.0, alpha),
            );
            renderer.draw_primitive(rect(4.0, 4.0, 20.0, 12.0), SparsePrimitive(calls.clone()));
        }
        let before = adaptive.composition_cache_statistics();
        assert_pixels(
            &adaptive.screenshot(&viewport, Color::BLACK),
            &direct.screenshot(&viewport, Color::BLACK),
            SIZE,
            "default sparse bypass",
        );
        let after = adaptive.composition_cache_statistics();
        assert_eq!(after.direct_frames, before.direct_frames + 1);
        assert_eq!(after.bytes, 0);
        assert_eq!(after.full_repaints, before.full_repaints);
        assert_eq!(after.partial_repaints, before.partial_repaints);
        assert_eq!(after.reused_frames, before.reused_frames);
        assert_eq!(
            calls[0].load(std::sync::atomic::Ordering::Relaxed),
            frame + 1
        );
    }

    // A dense editor-like scene is worth retaining while quiet. Scrolling
    // changes over half the frame, so it should paint directly for that frame.
    let dense = |renderer: &mut iced_wgpu::Renderer, scroll: f32| {
        for row in 0..14 {
            for column in 0..16 {
                fill(
                    renderer,
                    rect(column as f32 * 8.0, row as f32 * 8.0 - scroll, 8.0, 8.0),
                    if (row + column) % 2 == 0 {
                        Color::from_rgb(1.0, 0.0, 0.0)
                    } else {
                        Color::from_rgb(0.0, 0.0, 1.0)
                    },
                );
            }
        }
    };
    for (frame, scroll) in [0.0, 0.0, 4.0, 4.0, 4.0].into_iter().enumerate() {
        let viewport = begin(&mut adaptive, SIZE, 1.0);
        begin(&mut direct, SIZE, 1.0);
        dense(&mut adaptive, scroll);
        dense(&mut direct, scroll);
        let before = adaptive.composition_cache_statistics();
        assert_pixels(
            &adaptive.screenshot(&viewport, Color::BLACK),
            &direct.screenshot(&viewport, Color::BLACK),
            SIZE,
            "default scroll bypass/recovery",
        );
        let after = adaptive.composition_cache_statistics();
        match frame {
            1 | 4 => assert_reused(before, after),
            2 => {
                assert_eq!(after.direct_frames, before.direct_frames + 1);
                assert_eq!(after.full_repaints, before.full_repaints);
                assert_eq!(after.partial_repaints, before.partial_repaints);
                assert_eq!(after.reused_frames, before.reused_frames);
                assert_eq!(after.damaged_pixels, before.damaged_pixels);
            }
            _ => {
                assert_eq!(after.full_repaints, before.full_repaints + 1);
                assert_eq!(after.direct_frames, before.direct_frames);
            }
        }
    }

    // One resize refreshes the retained target. A consecutive resize bypasses
    // retention, leaving an older texture whose actual dimensions must be
    // checked when the viewport settles and retention resumes.
    for (frame, size) in [
        SIZE,
        Size::new(120, 88),
        Size::new(112, 80),
        Size::new(112, 80),
        Size::new(112, 80),
    ]
    .into_iter()
    .enumerate()
    {
        let viewport = begin(&mut adaptive, size, 1.0);
        begin(&mut direct, size, 1.0);
        dense(&mut adaptive, 4.0);
        dense(&mut direct, 4.0);
        let before = adaptive.composition_cache_statistics();
        assert_pixels(
            &adaptive.screenshot(&viewport, Color::BLACK),
            &direct.screenshot(&viewport, Color::BLACK),
            size,
            "default consecutive resize bypass/recovery",
        );
        let after = adaptive.composition_cache_statistics();
        match frame {
            0 | 4 => {
                assert_reused(before, after);
                assert_eq!(after.direct_frames, before.direct_frames);
            }
            2 => {
                assert_eq!(after.direct_frames, before.direct_frames + 1);
                assert_eq!(after.full_repaints, before.full_repaints);
                assert_eq!(after.partial_repaints, before.partial_repaints);
                assert_eq!(after.reused_frames, before.reused_frames);
                assert_eq!(after.damaged_pixels, before.damaged_pixels);
                assert_eq!(after.bytes, before.bytes, "bypass must avoid reallocating");
            }
            _ => {
                assert_eq!(after.full_repaints, before.full_repaints + 1);
                assert_eq!(after.direct_frames, before.direct_frames);
                assert_eq!(after.partial_repaints, before.partial_repaints);
                assert_eq!(after.reused_frames, before.reused_frames);
                assert_eq!(
                    after.damaged_pixels - before.damaged_pixels,
                    u64::from(size.width) * u64::from(size.height),
                );
            }
        }
        if frame != 2 {
            assert_eq!(
                after.bytes,
                u64::from(size.width) * u64::from(size.height) * 4
            );
        }
    }
}

#[test]
fn partial_clear_uses_the_background_that_settled_after_direct_frames() {
    let _guard = lock_vulkan_test();
    let Some(gpu) = Gpu::new(wgpu::TextureFormat::Rgba8UnormSrgb, None) else {
        return;
    };
    let mut adaptive = iced_wgpu::Renderer::new(gpu.engine.clone(), renderer::Settings::default());
    let mut direct = gpu.renderer(false);
    let blue = Color::from_rgb(0.1, 0.2, 0.7);
    let green = Color::from_rgb(0.1, 0.7, 0.2);
    for (frame, clear) in [Color::BLACK, blue, green, green, green, green]
        .into_iter()
        .enumerate()
    {
        let viewport = begin(&mut adaptive, SIZE, 1.0);
        begin(&mut direct, SIZE, 1.0);
        for renderer in [&mut adaptive, &mut direct] {
            // Enough stable paint to retain, with a later removal exposing
            // the background inside a small damage region.
            for column in 0..32 {
                fill(
                    renderer,
                    rect(column as f32 * 3.0, 60.0, 2.0, 2.0),
                    Color::WHITE,
                );
            }
            if frame < 4 {
                simple_scene(renderer, false);
            }
        }
        let before = adaptive.composition_cache_statistics();
        assert_pixels(
            &adaptive.screenshot(&viewport, clear),
            &direct.screenshot(&viewport, clear),
            SIZE,
            "background transition bypass/recovery/partial clear",
        );
        let after = adaptive.composition_cache_statistics();
        match frame {
            2 => assert_eq!(after.direct_frames, before.direct_frames + 1),
            4 => assert_partial(before, after),
            5 => assert_reused(before, after),
            _ => assert_eq!(after.full_repaints, before.full_repaints + 1),
        }
    }
}
