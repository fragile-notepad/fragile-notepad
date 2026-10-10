#![cfg(feature = "hybrid-rendering")]

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::renderer::{self, Renderer as _};
use iced::{Background, Color, Rectangle, Size, Transformation, gradient};
use iced_wgpu::graphics::{Antialiasing, Shell, Viewport, color, mesh};
use mesh::Renderer as _;

const WIDTH: u32 = 256;
const HEIGHT: u32 = 128;
const CELLS: usize = 64 * 64;

fn resource_renderer(engine: iced_wgpu::Engine) -> iced_wgpu::Renderer {
    let mut renderer = iced_wgpu::Renderer::new(engine, renderer::Settings::default());
    // These counters isolate image, quad, and mesh storage. Final-frame surface
    // allocation and damage rendering have their own pixel/resource tests.
    renderer.set_composition_cache_budget(0);
    renderer
}

fn lock_vulkan_test() -> std::sync::MutexGuard<'static, ()> {
    // Serialize Vulkan device and image-worker lifetimes, including teardown.
    // The parallel suite has segfaulted under MoltenVK in macOS CI; keep these
    // tests independent without overlapping driver initialization/destruction.
    // Declare the guard first in each test so it is dropped after GPU resources.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn vulkan_atlas_limits_spill_and_recover_without_losing_existing_images() {
    let _guard = lock_vulkan_test();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Some(adapter) =
        futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
    else {
        eprintln!("Skipping Vulkan atlas validation: no Vulkan adapter");
        return;
    };
    let Ok((device, queue)) =
        futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_texture_array_layers: 2,
                max_texture_dimension_2d: 2048,
                ..Default::default()
            },
            ..Default::default()
        }))
    else {
        eprintln!("Skipping Vulkan atlas validation: device unavailable");
        return;
    };
    let engine = iced_wgpu::Engine::new(
        &adapter,
        device.clone(),
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        None,
        Shell::headless(),
    );
    let mut renderer = resource_renderer(engine.clone());
    let rgba = |size: u32, color: [u8; 4]| {
        image::Handle::from_rgba(size, size, color.repeat((size * size) as usize))
    };
    let oversized = rgba(1536, [0, 255, 255, 255]);
    let full = [rgba(1024, [255, 0, 0, 255]), rgba(1024, [0, 0, 255, 255])];
    // The 1024-square shared atlas cannot fit this fragmented image in two
    // layers, but an independent 1536-square texture can. Rollback must preserve
    // the existing full page and free the failed image's partial reservations
    // so the following full page still fits in the shared atlas.
    let mut leases = vec![renderer.load_image(&full[0]).unwrap()];
    leases.push(renderer.load_image(&oversized).unwrap());
    let bindings = instance
        .generate_report()
        .unwrap()
        .hub
        .bind_groups
        .num_kept_from_user;
    leases.push(renderer.load_image(&full[1]).unwrap());
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .bind_groups
            .num_kept_from_user,
        bindings,
        "second full page must grow the shared texture, not spill due to leaked fragments"
    );

    let mut icons = resource_renderer(engine.clone());
    let icon_handles: Vec<_> = (0..12)
        .map(|index| rgba(96, [20 + index * 15, 80, 100, 255]))
        .collect();
    for handle in &icon_handles {
        leases.push(icons.load_image(handle).unwrap());
    }
    // The ninth 96-square icon exceeds two 256-square pages and uses the main
    // atlas. In the first renderer both shared pools fill, so it also exercises
    // an independent small texture after its eighth icon.
    for handle in icon_handles.iter().take(8) {
        leases.push(renderer.load_image(handle).unwrap());
    }
    let bounds = Rectangle::with_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let viewport = Viewport::with_physical_size(Size::new(WIDTH, HEIGHT), 1.0);
    for renderer in [&mut renderer, &mut icons] {
        renderer.reset(bounds);
        for (index, handle) in icon_handles.iter().enumerate() {
            renderer.draw_image(
                image::Image::new(handle.clone()),
                Rectangle {
                    x: index as f32 * 16.0,
                    y: 0.0,
                    width: 16.0,
                    height: 16.0,
                },
                bounds,
            );
        }
        let pixels = renderer.screenshot(&viewport, Color::BLACK);
        for index in 0..12 {
            assert_eq!(
                pixel(&pixels, index * 16 + 4, 4),
                [20 + index as u8 * 15, 80, 100, 255]
            );
        }
    }
    renderer.reset(bounds);
    for (index, handle) in [&oversized, &full[0], &full[1]].into_iter().enumerate() {
        renderer.draw_image(
            image::Image::new(handle.clone()),
            Rectangle {
                x: index as f32 * 16.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            },
            bounds,
        );
    }
    let pixels = renderer.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&pixels, 4, 4), [0, 255, 255, 255]);
    assert_eq!(pixel(&pixels, 20, 4), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 36, 4), [0, 0, 255, 255]);

    // Even a dedicated texture needs four 2048-square layers for this image.
    // Both APIs must report an allocation error rather than issue invalid GPU
    // commands or leave an asynchronous callback pending forever.
    let too_big = rgba(4096, [255, 255, 255, 255]);
    assert!(matches!(
        renderer.load_image(&too_big),
        Err(image::Error::OutOfMemory)
    ));
    let (sender, receiver) = std::sync::mpsc::channel();
    renderer.allocate_image(&too_big, move |result| {
        sender.send(result).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        renderer.tick();
        if let Ok(result) = receiver.try_recv() {
            assert!(matches!(result, Err(image::Error::OutOfMemory)));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "failed allocation callback timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(renderer.screenshot(&viewport, Color::BLACK), pixels);
    drop(leases);
    drop(renderer);
    drop(icons);
    drop(engine);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .bind_groups
            .num_kept_from_user,
        0
    );
}

fn solid() -> Background {
    Color::from_rgb(1.0, 0.0, 0.0).into()
}

fn gradient() -> Background {
    gradient::Linear::new(0.0)
        .add_stop(0.0, Color::from_rgb(0.0, 1.0, 0.0))
        .add_stop(1.0, Color::from_rgb(0.0, 0.0, 1.0))
        .into()
}

fn record(renderer: &mut iced_wgpu::Renderer, count: usize, gradients: bool, reverse: bool) {
    renderer.reset(Rectangle::with_size(Size::new(WIDTH as f32, HEIGHT as f32)));
    for index in 0..count {
        let x = (index % 64) as f32 * 4.0;
        let y = (index / 64) as f32 * 2.0;
        for side in 0..if gradients { 2 } else { 1 } {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: x + side as f32 * 2.0,
                        y,
                        width: 2.0,
                        height: 2.0,
                    },
                    ..Default::default()
                },
                if (side == 1) != reverse {
                    gradient()
                } else {
                    solid()
                },
            );
        }
    }
}

fn pixel(bytes: &[u8], x: u32, y: u32) -> &[u8] {
    let offset = ((y * WIDTH + x) * 4) as usize;
    &bytes[offset..offset + 4]
}

#[test]
fn vulkan_quad_buffers_grow_reuse_and_keep_windows_and_layers_independent() {
    let _guard = lock_vulkan_test();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Some(adapter) =
        futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
    else {
        eprintln!("Skipping Vulkan quad resource validation: no Vulkan adapter");
        return;
    };
    let Ok((device, queue)) =
        futures::executor::block_on(adapter.request_device(&Default::default()))
    else {
        eprintln!("Skipping Vulkan quad resource validation: device unavailable");
        return;
    };
    let engine = iced_wgpu::Engine::new(
        &adapter,
        device.clone(),
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        None,
        Shell::headless(),
    );
    let mut active = resource_renderer(engine.clone());
    let mut idle = resource_renderer(engine.clone());
    let pipeline_count = || {
        instance
            .generate_report()
            .unwrap()
            .hub
            .render_pipelines
            .num_kept_from_user
    };
    let initial_pipelines = pipeline_count();
    let viewport = Viewport::with_physical_size(Size::new(WIDTH, HEIGHT), 1.0);

    // Start with solids only, then introduce gradients into the same layer.
    record(&mut active, 1, false, false);
    let first_solid = active.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&first_solid, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&first_solid, 2, 0), [0, 0, 0, 255]);
    assert_eq!(pipeline_count(), initial_pipelines);
    record(&mut active, 1, true, false);
    let reference = active.screenshot(&viewport, Color::BLACK);
    assert_eq!(
        pipeline_count(),
        initial_pipelines + 1,
        "compile gradients only at first use"
    );
    let gradient_pixel = pixel(&reference, 2, 0);
    assert_eq!(gradient_pixel[0], 0);
    assert!(gradient_pixel[1] > 0 && gradient_pixel[2] > 0);

    record(&mut idle, 1, true, true);
    let idle_reference = idle.screenshot(&viewport, Color::BLACK);
    assert_eq!(
        pipeline_count(),
        initial_pipelines + 1,
        "engine clones share the gradient pipeline"
    );

    // Each type grows to 4,096 instances, beyond the previous fixed capacity.
    for reverse in [false, true, false] {
        record(&mut active, CELLS, true, reverse);
        let large = active.screenshot(&viewport, Color::BLACK);
        for index in 0..CELLS {
            let x = (index % 64) as u32 * 4;
            let y = (index / 64) as u32 * 2;
            let (solid_x, gradient_x) = if reverse { (x + 2, x) } else { (x, x + 2) };
            assert_eq!(pixel(&large, solid_x, y), [255, 0, 0, 255]);
            assert_eq!(pixel(&large, gradient_x, y), gradient_pixel);
        }
        assert_eq!(idle.screenshot(&viewport, Color::BLACK), idle_reference);

        record(&mut active, 1, false, false);
        assert_eq!(active.screenshot(&viewport, Color::BLACK), first_solid);
        record(&mut active, 0, false, false);
        assert!(
            active
                .screenshot(&viewport, Color::BLACK)
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
    }

    // A second clipped layer needs independent storage and must not overwrite
    // the first layer's uniforms/instances while the encoder is being built.
    record(&mut active, CELLS, true, false);
    active.with_layer(
        Rectangle {
            x: 4.0,
            y: 4.0,
            width: 4.0,
            height: 4.0,
        },
        |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::with_size(Size::new(WIDTH as f32, HEIGHT as f32)),
                    ..Default::default()
                },
                Color::WHITE,
            );
        },
    );
    let clipped = active.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&clipped, 4, 4), [255; 4]);
    assert_eq!(pixel(&clipped, 8, 4), [255, 0, 0, 255]);
    assert_eq!(pixel(&clipped, 2, 4), gradient_pixel);

    drop(active);
    assert_eq!(idle.screenshot(&viewport, Color::BLACK), idle_reference);
    drop(idle);
    drop(engine);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let report = instance.generate_report().unwrap().hub;
    assert_eq!(report.buffers.num_kept_from_user, 0);
    assert_eq!(report.bind_groups.num_kept_from_user, 0);
    assert_eq!(report.render_pipelines.num_kept_from_user, 0);
}

#[test]
fn vulkan_mesh_pipelines_are_lazy_shared_and_preserve_msaa_after_resize() {
    let _guard = lock_vulkan_test();
    for antialiasing in [None, Some(Antialiasing::MSAAx4)] {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let Some(adapter) =
            futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
        else {
            eprintln!("Skipping Vulkan mesh resource validation: no Vulkan adapter");
            return;
        };
        let Ok((device, queue)) =
            futures::executor::block_on(adapter.request_device(&Default::default()))
        else {
            eprintln!("Skipping Vulkan mesh resource validation: device unavailable");
            return;
        };
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device.clone(),
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            antialiasing,
            Shell::headless(),
        );
        let pipelines = || {
            instance
                .generate_report()
                .unwrap()
                .hub
                .render_pipelines
                .num_kept_from_user
        };
        let initial = pipelines();
        let mut active = resource_renderer(engine.clone());
        let mut other = resource_renderer(engine.clone());
        let viewport = Viewport::with_physical_size(Size::new(WIDTH, HEIGHT), 1.0);
        record(&mut active, 1, false, false);
        active.screenshot(&viewport, Color::BLACK);
        assert_eq!(
            pipelines(),
            initial,
            "quad-only warm-up must not compile meshes"
        );

        let bounds = Rectangle::with_size(Size::new(WIDTH as f32, HEIGHT as f32));
        let Background::Gradient(fill) = gradient() else {
            unreachable!()
        };
        let packed = iced_wgpu::graphics::gradient::pack(
            &fill,
            Rectangle {
                x: 32.0,
                y: 0.0,
                width: 32.0,
                height: 32.0,
            },
        );
        let meshes = vec![
            mesh::Mesh::Solid {
                buffers: mesh::Indexed {
                    vertices: [[32.0, 32.0], [32.0, 0.0], [0.0, 32.0], [0.0, 0.0]]
                        .map(|position| mesh::SolidVertex2D {
                            position,
                            color: color::pack(Color::from_rgb(1.0, 0.0, 0.0)),
                        })
                        .into(),
                    indices: vec![3, 1, 2, 1, 0, 2],
                },
                transformation: Transformation::IDENTITY,
                clip_bounds: bounds,
            },
            mesh::Mesh::Gradient {
                buffers: mesh::Indexed {
                    vertices: [[32.0, 0.0], [64.0, 0.0], [32.0, 32.0]]
                        .map(|position| mesh::GradientVertex2D {
                            position,
                            gradient: packed,
                        })
                        .into(),
                    indices: vec![0, 1, 2],
                },
                transformation: Transformation::IDENTITY,
                clip_bounds: bounds,
            },
        ];
        active.reset(bounds);
        for mesh in &meshes {
            active.draw_mesh(mesh.clone());
        }
        let warm = active.warm_up_offscreen(&viewport, Color::BLACK).unwrap();
        assert!(warm.submission_completed);
        let compiled = pipelines();
        assert_eq!(
            compiled - initial,
            if antialiasing.is_some() { 3 } else { 2 }
        );
        let first = active.screenshot(&viewport, Color::BLACK);
        assert_eq!(
            pipelines(),
            compiled,
            "first draw reuses warmed mesh pipelines"
        );
        assert_eq!(
            pixel(&first, 2, 2),
            [255, 0, 0, 255],
            "first mesh draw: antialiasing={antialiasing:?}, adapter={:?}, gradient_pixel={:?}",
            adapter.get_info(),
            pixel(&first, 34, 2),
        );
        // The second triangle reuses vertices and has a different index count
        // from the gradient mesh that follows it in the shared index buffer.
        assert_eq!(pixel(&first, 30, 30), [255, 0, 0, 255]);
        let gradient_pixel = pixel(&first, 34, 2);
        assert_eq!(gradient_pixel[0], 0);
        assert!(gradient_pixel[1] > 0 && gradient_pixel[2] > 0);
        assert_eq!(pixel(&first, 60, 60), [0, 0, 0, 255]);

        // Grow the shared index allocation with degenerate [0, 0, 0] triangles.
        // Repeating visible triangles would accumulate the gradient shader's
        // dithered alpha through blending and change the expected pixels.
        // Both draws still have fewer than 65536 indices, so on Apple's virtual
        // GPU the buffer binding offset must continue to select wide encoding.
        active.reset(bounds);
        for mesh in &meshes {
            let mut grown = mesh.clone();
            match &mut grown {
                mesh::Mesh::Solid { buffers, .. } => {
                    buffers.indices.resize(buffers.indices.len() * 1024, 0);
                }
                mesh::Mesh::Gradient { buffers, .. } => {
                    buffers.indices.resize(buffers.indices.len() * 1024, 0);
                }
            }
            active.draw_mesh(grown);
        }
        assert!(
            active.screenshot(&viewport, Color::BLACK) == first,
            "mesh pixels changed after index-buffer growth: antialiasing={antialiasing:?}, adapter={:?}",
            adapter.get_info(),
        );
        active.reset(bounds);
        for mesh in &meshes {
            active.draw_mesh(mesh.clone());
        }
        assert!(
            active.screenshot(&viewport, Color::BLACK) == first,
            "mesh pixels changed after restoring original indices: antialiasing={antialiasing:?}, adapter={:?}",
            adapter.get_info(),
        );

        // A renderer cloned before first mesh use must share the new pipelines.
        other.reset(bounds);
        other.draw_mesh_cache(mesh::Cache::new(meshes.into()));
        assert_eq!(other.screenshot(&viewport, Color::BLACK), first);
        assert_eq!(pipelines(), compiled);
        let resized = Viewport::with_physical_size(Size::new(WIDTH * 2, HEIGHT * 2), 2.0);
        let large = active.screenshot(&resized, Color::BLACK);
        let offset = ((4 * WIDTH * 2 + 4) * 4) as usize;
        assert_eq!(&large[offset..offset + 4], [255, 0, 0, 255]);
        assert_eq!(active.screenshot(&viewport, Color::BLACK), first);
        drop(active);
        assert_eq!(other.screenshot(&viewport, Color::BLACK), first);
        drop(other);
        drop(engine);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(pipelines(), 0);
    }
}

#[test]
fn vulkan_image_storage_is_lazy_and_preserves_async_allocations_and_atlas_growth() {
    let _guard = lock_vulkan_test();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Some(adapter) =
        futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
    else {
        eprintln!("Skipping Vulkan image validation: no Vulkan adapter");
        return;
    };
    let Ok((device, queue)) =
        futures::executor::block_on(adapter.request_device(&Default::default()))
    else {
        eprintln!("Skipping Vulkan image validation: device unavailable");
        return;
    };
    let engine = iced_wgpu::Engine::new(
        &adapter,
        device.clone(),
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        None,
        Shell::headless(),
    );
    let initial_textures = instance.generate_report().unwrap().hub.textures;
    let mut renderer = resource_renderer(engine.clone());
    let mut other = resource_renderer(engine.clone());
    let pipeline_count = || {
        instance
            .generate_report()
            .unwrap()
            .hub
            .render_pipelines
            .num_kept_from_user
    };
    let initial_pipelines = pipeline_count();
    let bounds = Rectangle::with_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let viewport = Viewport::with_physical_size(Size::new(WIDTH, HEIGHT), 1.0);
    renderer.reset(bounds);
    renderer.tick();
    renderer.screenshot(&viewport, Color::BLACK);
    assert_eq!(pipeline_count(), initial_pipelines);
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .textures
            .num_kept_from_user,
        initial_textures.num_kept_from_user,
        "empty renderer must not allocate image textures"
    );

    let rgba = |size: u32, color: [u8; 4]| {
        image::Handle::from_rgba(size, size, color.repeat((size * size) as usize))
    };
    let small = rgba(4, [255, 0, 0, 255]);
    assert_eq!(renderer.measure_image(&small), Some(Size::new(4, 4)));
    assert_eq!(pipeline_count(), initial_pipelines + 1);
    assert_eq!(other.measure_image(&small), Some(Size::new(4, 4)));
    assert_eq!(
        pipeline_count(),
        initial_pipelines + 1,
        "engine clones share the image pipeline"
    );
    other.tick();
    drop(other);
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .textures
            .num_kept_from_user,
        initial_textures.num_kept_from_user,
        "RGBA measurement must not allocate atlas storage"
    );

    // Explicit asynchronous allocations use their own texture. Duplicate requests
    // must both complete without constructing the unused shared atlas binding.
    let bindings = instance
        .generate_report()
        .unwrap()
        .hub
        .bind_groups
        .num_kept_from_user;
    let (sender, receiver) = std::sync::mpsc::channel();
    for _ in 0..2 {
        let sender = sender.clone();
        renderer.allocate_image(&small, move |result| {
            sender.send(result).unwrap();
        });
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut leases = Vec::new();
    while leases.len() < 2 {
        renderer.tick();
        while let Ok(result) = receiver.try_recv() {
            leases.push(result.unwrap());
        }
        assert!(
            std::time::Instant::now() < deadline,
            "image allocation callbacks timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(leases.iter().all(|lease| lease.size() == Size::new(4, 4)));
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .bind_groups
            .num_kept_from_user,
        bindings + 1
    );
    renderer.draw_image(
        image::Image::new(small.clone()),
        Rectangle {
            x: 0.0,
            y: 0.0,
            width: 16.0,
            height: 16.0,
        },
        bounds,
    );
    let first = renderer.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&first, 4, 4), [255, 0, 0, 255]);

    // The first shared upload itself can span multiple atlas pages. The lazy
    // allocation must create every required layer without an old texture to copy.
    let mut pixels = Vec::with_capacity(1100 * 4 * 4);
    for _ in 0..4 {
        for x in 0..1100 {
            pixels.extend_from_slice(if x < 1024 {
                &[0, 255, 255, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    let fragmented = image::Handle::from_rgba(1100, 4, pixels);
    leases.push(renderer.load_image(&fragmented).unwrap());
    renderer.reset(bounds);
    renderer.draw_image(
        image::Image::new(fragmented).filter_method(image::FilterMethod::Nearest),
        Rectangle {
            x: 0.0,
            y: 0.0,
            width: 110.0,
            height: 4.0,
        },
        bounds,
    );
    let pixels = renderer.screenshot(&viewport, Color::BLACK);
    assert_eq!(pixel(&pixels, 50, 1), [0, 255, 255, 255]);
    assert_eq!(pixel(&pixels, 108, 1), [0, 0, 255, 255]);

    // Three 600-square images force shared-atlas growth beyond its first page.
    // Earlier uploads must survive copying into the expanded texture.
    let colors = [[0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255]];
    let handles: Vec<_> = colors.iter().map(|color| rgba(600, *color)).collect();
    for handle in &handles {
        leases.push(renderer.load_image(handle).unwrap());
    }
    for _ in 0..2 {
        renderer.reset(bounds);
        for (index, handle) in std::iter::once(&small).chain(&handles).enumerate() {
            renderer.draw_image(
                image::Image::new(handle.clone()),
                Rectangle {
                    x: index as f32 * 16.0,
                    y: 0.0,
                    width: 16.0,
                    height: 16.0,
                },
                bounds,
            );
        }
        let pixels = renderer.screenshot(&viewport, Color::BLACK);
        assert_eq!(pixel(&pixels, 4, 4), [255, 0, 0, 255]);
        for (index, color) in colors.iter().enumerate() {
            assert_eq!(pixel(&pixels, (index as u32 + 1) * 16 + 4, 4), color);
        }
    }
    // A large RGBA handle takes the ordinary asynchronous-upload path without
    // an explicit allocation, and eventually replaces its initially blank draw.
    let large = rgba(800, [255, 0, 255, 255]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        renderer.reset(bounds);
        renderer.draw_image(
            image::Image::new(large.clone()),
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            },
            bounds,
        );
        let pixels = renderer.screenshot(&viewport, Color::BLACK);
        if pixel(&pixels, 4, 4) == [255, 0, 255, 255] {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "large asynchronous image timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Shared icons use a smaller pool. Exercise both upload entry points,
    // growth, eviction, and reuse while large artwork and a pinned icon stay
    // alive. Reused atlas coordinates must never refer to the other pool.
    let pinned = rgba(96, [0, 255, 255, 255]);
    leases.push(renderer.load_image(&pinned).unwrap());
    for phase in 0..3 {
        let icons: Vec<_> = (0..20)
            .map(|index| {
                let color = [30 + phase * 60, 20 + index * 10, 100, 255];
                (rgba(96, color), color)
            })
            .collect();
        for (handle, _) in icons.iter().take(10) {
            drop(renderer.load_image(handle).unwrap());
        }
        renderer.reset(bounds);
        let images: Vec<_> = std::iter::once((&small, [255, 0, 0, 255]))
            .chain(handles.iter().zip(colors))
            .chain(std::iter::once((&pinned, [0, 255, 255, 255])))
            .chain(icons.iter().map(|(handle, color)| (handle, *color)))
            .collect();
        for (index, (handle, _)) in images.iter().enumerate() {
            renderer.draw_image(
                image::Image::new((*handle).clone()).filter_method(image::FilterMethod::Nearest),
                Rectangle {
                    x: (index % 20) as f32 * 12.0,
                    y: (index / 20) as f32 * 12.0,
                    width: 12.0,
                    height: 12.0,
                },
                bounds,
            );
        }
        let pixels = renderer.screenshot(&viewport, Color::BLACK);
        for (index, (_, color)) in images.iter().enumerate() {
            assert_eq!(
                pixel(
                    &pixels,
                    (index % 20) as u32 * 12 + 4,
                    (index / 20) as u32 * 12 + 4
                ),
                color,
                "mixed atlas phase {phase}, image {index}"
            );
        }
    }
    drop(leases);
    drop(renderer);
    drop(engine);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let report = instance.generate_report().unwrap().hub;
    assert_eq!(report.textures.num_kept_from_user, 0);
    assert_eq!(report.bind_groups.num_kept_from_user, 0);
}
