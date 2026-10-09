use super::*;
use crate::{Attrs, Cache, Color, Metrics, Resolution, Shaping, TextBounds};

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    struct WakeThread(std::thread::Thread);
    impl std::task::Wake for WakeThread {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(WakeThread(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

fn text(fonts: &mut FontSystem, content: &str, size: f32) -> crate::Buffer {
    let mut buffer = crate::Buffer::new(fonts, Metrics::new(size, size * 1.2));
    buffer.set_size(Some(512.0), Some(256.0));
    buffer.set_text(content, &Attrs::new(), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    buffer
}

fn area(buffer: &crate::Buffer, phase: usize) -> TextArea<'_> {
    TextArea {
        text: buffer.layout_runs(),
        left: if phase == 2 { 7.0 } else { 0.0 },
        top: 0.0,
        scale: 1.0,
        bounds: TextBounds {
            left: 0,
            top: 0,
            right: if phase == 3 { 100 } else { 512 },
            bottom: 256,
        },
        default_color: if phase == 1 {
            Color::rgb(200, 50, 80)
        } else {
            Color::rgb(240, 240, 240)
        },
    }
}

fn capture(
    device: &Device,
    encoder: &mut CommandEncoder,
    renderer: &TextRenderer,
    atlas: &TextAtlas,
    viewport: &Viewport,
) -> Buffer {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: Extent3d {
            width: 512,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        renderer.render(atlas, viewport, &mut pass).unwrap();
    }
    let readback = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 512 * 256 * 4,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512 * 4),
                rows_per_image: Some(256),
            },
        },
        texture.size(),
    );
    readback
}

fn pixels(device: &Device, readback: &Buffer) -> Vec<u8> {
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    let bytes = readback.slice(..).get_mapped_range().to_vec();
    readback.unmap();
    bytes
}

#[test]
fn retained_text_matches_fresh_uploads_and_recovers_after_atlas_full() {
    let instance = instance();
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference:
            wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::HighPerformance),
        ..Default::default()
    })) else {
        // Hosted Windows runners may have no Vulkan implementation.
        eprintln!("Skipping Vulkan text validation: no Vulkan adapter");
        return;
    };
    eprintln!("GLYPH_ADAPTER {:?}", adapter.get_info());
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    let cache = Cache::new(&device);
    let mut atlas = TextAtlas::new(&device, &queue, &cache, wgpu::TextureFormat::Rgba8Unorm);
    let mut viewport = Viewport::new(&device, &cache);
    viewport.update(
        &queue,
        Resolution {
            width: 512,
            height: 256,
        },
    );
    let mut renderer = TextRenderer::new(&mut atlas, &device, Default::default(), None);
    let mut fonts = FontSystem::new();
    let mut swash = SwashCache::new();
    let small = text(
        &mut fonts,
        "Retained text: café e\u{301} العربية 中文 🐇",
        18.0,
    );
    let large = text(
        &mut fonts,
        &"ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789\n".repeat(14),
        14.0,
    );
    let empty = text(&mut fonts, "", 14.0);

    for phase in [0, 0, 1, 2, 3, 4, 5, 0, 4, 4] {
        let buffer = match phase {
            4 => &large,
            5 => &empty,
            _ => &small,
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer
            .prepare(
                &device,
                &queue,
                &mut encoder,
                &mut fonts,
                &mut atlas,
                &viewport,
                [area(buffer, phase)],
                &mut swash,
            )
            .unwrap();
        let actual = capture(&device, &mut encoder, &renderer, &atlas, &viewport);
        let mut fresh = TextRenderer::new(&mut atlas, &device, Default::default(), None);
        fresh
            .prepare(
                &device,
                &queue,
                &mut encoder,
                &mut fonts,
                &mut atlas,
                &viewport,
                [area(buffer, phase)],
                &mut swash,
            )
            .unwrap();
        let reference = capture(&device, &mut encoder, &fresh, &atlas, &viewport);
        queue.submit([encoder.finish()]);
        let actual = pixels(&device, &actual);
        assert_eq!(actual, pixels(&device, &reference), "phase {phase}");
        if phase != 5 {
            assert!(actual.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
        }
        atlas.trim();
    }

    // Overflow after a changed prefix has already overwritten the CPU vector.
    // Returning to the same text must not trust that unsubmitted prefix.
    atlas.mask_atlas.max_texture_dimension_2d = atlas.mask_atlas.size;
    let huge = text(&mut fonts, "W", 1000.0);
    let mut encoder = device.create_command_encoder(&Default::default());
    let result = renderer.prepare(
        &device,
        &queue,
        &mut encoder,
        &mut fonts,
        &mut atlas,
        &viewport,
        [area(&small, 1), area(&huge, 0)],
        &mut swash,
    );
    assert!(matches!(result, Err(PrepareError::AtlasFull)));
    assert!(!renderer.vertices_valid);
    let failed = capture(&device, &mut encoder, &renderer, &atlas, &viewport);
    queue.submit([encoder.finish()]);
    assert!(
        pixels(&device, &failed)
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 0, 0, 255])
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer
        .prepare(
            &device,
            &queue,
            &mut encoder,
            &mut fonts,
            &mut atlas,
            &viewport,
            [area(&small, 1)],
            &mut swash,
        )
        .unwrap();
    let actual = capture(&device, &mut encoder, &renderer, &atlas, &viewport);
    let mut fresh = TextRenderer::new(&mut atlas, &device, Default::default(), None);
    fresh
        .prepare(
            &device,
            &queue,
            &mut encoder,
            &mut fonts,
            &mut atlas,
            &viewport,
            [area(&small, 1)],
            &mut swash,
        )
        .unwrap();
    let reference = capture(&device, &mut encoder, &fresh, &atlas, &viewport);
    queue.submit([encoder.finish()]);
    assert_eq!(pixels(&device, &actual), pixels(&device, &reference));
}

#[test]
fn vertex_growth_keeps_pending_draws_alive() {
    let instance = instance();
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference:
            wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::HighPerformance),
        ..Default::default()
    })) else {
        // Hosted Windows runners may have no Vulkan implementation.
        eprintln!("Skipping Vulkan text validation: no Vulkan adapter");
        return;
    };
    eprintln!("GLYPH_GROWTH_ADAPTER {:?}", adapter.get_info());
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    let cache = Cache::new(&device);
    let mut atlas = TextAtlas::new(&device, &queue, &cache, wgpu::TextureFormat::Rgba8Unorm);
    let mut viewport = Viewport::new(&device, &cache);
    viewport.update(
        &queue,
        Resolution {
            width: 512,
            height: 256,
        },
    );
    let mut renderer = TextRenderer::new(&mut atlas, &device, Default::default(), None);
    let mut fonts = FontSystem::new();
    let mut swash = SwashCache::new();
    // Both uploads exceed the current capacity and initialize mapped buffers;
    // neither uses a staging chunk that requires an intervening submission.
    let small = text(
        &mut fonts,
        &"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n".repeat(8),
        14.0,
    );
    let large = text(
        &mut fonts,
        &"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n".repeat(14),
        14.0,
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer
        .prepare(
            &device,
            &queue,
            &mut encoder,
            &mut fonts,
            &mut atlas,
            &viewport,
            [area(&small, 0)],
            &mut swash,
        )
        .unwrap();
    let old_size = renderer.vertex_buffer_size;
    let first = capture(&device, &mut encoder, &renderer, &atlas, &viewport);
    renderer
        .prepare(
            &device,
            &queue,
            &mut encoder,
            &mut fonts,
            &mut atlas,
            &viewport,
            [area(&large, 0)],
            &mut swash,
        )
        .unwrap();
    assert!(renderer.vertex_buffer_size > old_size);
    let second = capture(&device, &mut encoder, &renderer, &atlas, &viewport);
    queue.submit([encoder.finish()]);
    let first = pixels(&device, &first);
    let second = pixels(&device, &second);
    assert_ne!(first, second);
    assert!(first.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
    assert!(second.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
}

fn instance() -> &'static wgpu::Instance {
    static INSTANCE: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    })
}

#[test]
fn cached_glyphs_remain_protected_until_the_next_trim() {
    let Ok(adapter) = block_on(instance().request_adapter(&Default::default())) else {
        eprintln!("Skipping Vulkan glyph generation validation: no Vulkan adapter");
        return;
    };
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    let cache = Cache::new(&device);
    let mut atlas = TextAtlas::new(&device, &queue, &cache, wgpu::TextureFormat::Rgba8Unorm);
    let mut viewport = Viewport::new(&device, &cache);
    viewport.update(
        &queue,
        Resolution {
            width: 512,
            height: 256,
        },
    );
    let mut renderer = TextRenderer::new(&mut atlas, &device, Default::default(), None);
    let mut fonts = FontSystem::new();
    let mut swash = SwashCache::new();
    let buffer = text(&mut fonts, "Cached glyph protection", 18.0);
    for _ in 0..2 {
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer
            .prepare(
                &device,
                &queue,
                &mut encoder,
                &mut fonts,
                &mut atlas,
                &viewport,
                [area(&buffer, 0)],
                &mut swash,
            )
            .unwrap();
        queue.submit([encoder.finish()]);
        let entries = atlas.mask_atlas.glyph_cache.len();
        assert!(entries > 0);
        let size = atlas.mask_atlas.size as usize;
        assert!(atlas.mask_atlas.try_allocate(size, size).is_none());
        assert_eq!(atlas.mask_atlas.glyph_cache.len(), entries);
        atlas.trim();
    }
    let size = atlas.mask_atlas.size as usize;
    assert!(atlas.mask_atlas.try_allocate(size, size).is_some());
    assert!(atlas.mask_atlas.glyph_cache.is_empty());
}

#[test]
fn mask_retention_grows_lazily_then_resumes_lru_without_evicting_pinned_glyphs() {
    let Ok(adapter) = block_on(instance().request_adapter(&Default::default())) else {
        eprintln!("Skipping Vulkan mask retention validation: no Vulkan adapter");
        return;
    };
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    let cache = Cache::new(&device);
    let mut atlas = TextAtlas::new(&device, &queue, &cache, wgpu::TextureFormat::Rgba8Unorm);
    let initial_size = atlas.cache_statistics().mask_size;
    atlas.set_mask_cache_retention_size(1024);
    assert_eq!(atlas.cache_statistics().mask_size, initial_size);

    let mut viewport = Viewport::new(&device, &cache);
    viewport.update(
        &queue,
        Resolution {
            width: 512,
            height: 256,
        },
    );
    let mut renderer = TextRenderer::new(&mut atlas, &device, Default::default(), None);
    let mut fonts = FontSystem::new();
    let mut swash = SwashCache::new();
    let buffer = text(&mut fonts, "Retained glyphs: café 日本語", 18.0);
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer
        .prepare(
            &device,
            &queue,
            &mut encoder,
            &mut fonts,
            &mut atlas,
            &viewport,
            [area(&buffer, 0)],
            &mut swash,
        )
        .unwrap();
    let expected = capture(&device, &mut encoder, &renderer, &atlas, &viewport);
    queue.submit([encoder.finish()]);
    let expected = pixels(&device, &expected);
    let coordinates: Vec<_> = atlas
        .mask_atlas
        .glyph_cache
        .iter()
        .filter_map(|(key, glyph)| match glyph.gpu_cache {
            GpuCacheStatus::InAtlas { x, y, .. } => Some((*key, x, y)),
            GpuCacheStatus::SkipRasterization => None,
        })
        .collect();
    assert!(!coordinates.is_empty());
    atlas.trim();
    let before_growth = atlas.cache_statistics();

    while atlas.mask_atlas.size < 1024 {
        let size = atlas.mask_atlas.size as usize;
        // A full-side request cannot fit beside any live allocation. Historical
        // entries must survive this failed request so prepare can grow instead.
        assert!(atlas.mask_atlas.try_allocate(size, size).is_none());
        assert_eq!(
            atlas.cache_statistics().mask_evictions,
            before_growth.mask_evictions
        );
        assert_eq!(
            atlas.cache_statistics().mask_glyphs,
            before_growth.mask_glyphs
        );
        assert!(atlas.grow(&device, &queue, &mut fonts, &mut swash, ContentType::Mask));
        for &(key, expected_x, expected_y) in &coordinates {
            let glyph = atlas.mask_atlas.glyph_cache.peek(&key).unwrap();
            let GpuCacheStatus::InAtlas { x, y, .. } = glyph.gpu_cache else {
                panic!("retained glyph lost its atlas allocation");
            };
            assert_eq!((x, y), (expected_x, expected_y));
        }
    }
    assert_eq!(atlas.cache_statistics().mask_size, 1024);

    // Preparing the same text pins every retained mask glyph in the new frame.
    // Rendering must still read the same texels after growth and atlas rebinding.
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer
        .prepare(
            &device,
            &queue,
            &mut encoder,
            &mut fonts,
            &mut atlas,
            &viewport,
            [area(&buffer, 0)],
            &mut swash,
        )
        .unwrap();
    let actual = capture(&device, &mut encoder, &renderer, &atlas, &viewport);
    queue.submit([encoder.finish()]);
    assert_eq!(pixels(&device, &actual), expected);
    let size = atlas.mask_atlas.size as usize;
    assert!(atlas.mask_atlas.try_allocate(size, size).is_none());
    assert_eq!(
        atlas.cache_statistics().mask_evictions,
        before_growth.mask_evictions
    );
    assert_eq!(
        atlas.cache_statistics().mask_glyphs,
        before_growth.mask_glyphs
    );

    // At the retention target, advancing the frame restores ordinary eviction.
    atlas.trim();
    assert!(atlas.mask_atlas.try_allocate(size, size).is_some());
    let after_eviction = atlas.cache_statistics();
    assert_eq!(after_eviction.mask_size, 1024);
    assert_eq!(after_eviction.mask_glyphs, 0);
    assert_eq!(
        after_eviction.mask_evictions,
        before_growth.mask_evictions + before_growth.mask_glyphs as u64
    );
    assert!(after_eviction.mask_allocation_calls > before_growth.mask_allocation_calls);
}
