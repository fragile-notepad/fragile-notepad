#![cfg(feature = "hybrid-rendering")]

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::renderer::{self, Renderer as _};
use iced::{Color, Rectangle, Size, Transformation};
use iced_wgpu::graphics::{Shell, Viewport};

fn poll_initialization(device: &wgpu::Device, limit: u32, stage: &str) {
    let result = device.poll(wgpu::PollType::Poll);
    eprintln!("TRANSFORM_INIT_POLL limit={limit} stage={stage} result={result:?}");
    result.unwrap_or_else(|error| panic!("GPU initialization poll failed at {stage}: {error}"));
}

fn record(renderer: &mut iced_wgpu::Renderer, image: &image::Handle, phase: u32) {
    let bounds = Rectangle::with_size(Size::new(256.0, 128.0));
    renderer.reset(bounds);
    for layer in 0..3 {
        renderer.with_layer(
            Rectangle {
                x: layer as f32 * 78.0,
                y: 8.0,
                width: 68.0,
                height: 100.0,
            },
            |renderer| {
                renderer.with_transformation(
                    Transformation::translate(layer as f32 * 78.0 + phase as f32, 12.0)
                        * Transformation::scale(0.9 + phase as f32 * 0.02),
                    |renderer| {
                        let fill = if layer % 2 == 0 {
                            iced::Background::Color(Color::from_rgb(
                                0.3 + phase as f32 * 0.1,
                                0.6,
                                0.8,
                            ))
                        } else {
                            iced::gradient::Linear::new(iced::Radians(0.7))
                                .add_stop(
                                    0.0,
                                    Color::from_rgba(1.0, 0.0, 0.0, 0.8 - phase as f32 * 0.1),
                                )
                                .add_stop(1.0, Color::from_rgb(0.0, 0.0, 1.0))
                                .into()
                        };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: 0.0,
                                    y: 0.0,
                                    width: 72.0,
                                    height: 86.0,
                                },
                                border: iced::Border {
                                    color: Color::WHITE,
                                    width: 1.5,
                                    radius: 7.0.into(),
                                },
                                ..Default::default()
                            },
                            fill,
                        );
                        for (index, filter) in
                            [image::FilterMethod::Nearest, image::FilterMethod::Linear]
                                .into_iter()
                                .enumerate()
                        {
                            renderer.draw_image(
                                image::Image::new(image.clone())
                                    .filter_method(filter)
                                    .opacity(0.6 + phase as f32 * 0.03)
                                    .rotation(iced::Radians(0.2)),
                                Rectangle {
                                    x: 4.0 + index as f32 * 23.0,
                                    y: 18.0,
                                    width: 29.0,
                                    height: 53.0,
                                },
                                Rectangle {
                                    x: 6.0,
                                    y: 10.0,
                                    width: 48.0,
                                    height: 62.0,
                                },
                            );
                        }
                    },
                );
            },
        );
    }
}

#[test]
fn vulkan_transform_parameters_match_uniforms_at_each_device_limit() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Some(adapter) =
        futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference:
                wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::HighPerformance),
            ..Default::default()
        }))
        .ok()
    else {
        eprintln!("Skipping Vulkan transform validation: no Vulkan adapter");
        return;
    };
    if !adapter.features().contains(wgpu::Features::IMMEDIATES)
        || adapter.limits().max_immediate_size < 80
    {
        eprintln!("Skipping Vulkan transform validation: immediate uniforms unavailable");
        return;
    }
    eprintln!("TRANSFORM_ADAPTER {:?}", adapter.get_info());
    let mut reference = None;
    let mut baseline_counts = None;
    for limit in [0, 16, 64, 80] {
        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: if limit == 0 {
                    wgpu::Features::empty()
                } else {
                    wgpu::Features::IMMEDIATES
                },
                required_limits: wgpu::Limits {
                    max_immediate_size: limit,
                    ..Default::default()
                },
                ..Default::default()
            }))
            .unwrap();
        device.set_device_lost_callback(move |reason, message| {
            eprintln!("TRANSFORM_DEVICE_LOST limit={limit} reason={reason:?} message={message}");
        });
        poll_initialization(&device, limit, "device_requested");
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::R8Unorm,
        ] {
            eprintln!(
                "TRANSFORM_INIT_TEXTURE limit={limit} format={format:?} stage=create_texture"
            );
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("transform initialization texture probe"),
                size: wgpu::Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            eprintln!("TRANSFORM_INIT_TEXTURE limit={limit} format={format:?} stage=create_view");
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            eprintln!("TRANSFORM_INIT_TEXTURE limit={limit} format={format:?} stage=ready");
            poll_initialization(&device, limit, "texture_probe_ready");
            drop(view);
            drop(texture);
        }
        eprintln!("TRANSFORM_INIT_STAGE limit={limit} stage=engine_begin");
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device.clone(),
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            None,
            Shell::headless(),
        );
        eprintln!("TRANSFORM_INIT_STAGE limit={limit} stage=engine_ready");
        poll_initialization(&device, limit, "engine_ready");
        let mut active = iced_wgpu::Renderer::new(engine.clone(), renderer::Settings::default());
        let mut idle = iced_wgpu::Renderer::new(engine.clone(), renderer::Settings::default());
        let image = image::Handle::from_rgba(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
        );
        let leases = [
            active.load_image(&image).unwrap(),
            idle.load_image(&image).unwrap(),
        ];
        let mut pixels = Vec::new();
        for scale in [1.0, 1.5, 2.0] {
            let viewport = Viewport::with_physical_size(
                Size::new((256.0 * scale) as u32, (128.0 * scale) as u32),
                scale,
            );
            record(&mut idle, &image, 4);
            let idle_pixels = idle.screenshot(&viewport, Color::BLACK);
            for phase in [0, 0, 1, 2, 2, 0] {
                record(&mut active, &image, phase);
                let frame = active.screenshot(&viewport, Color::BLACK);
                assert!(frame.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
                // A new renderer has no previous instance data to reuse. Match
                // every pixel after unchanged, changed, and reverted frames.
                let mut fresh =
                    iced_wgpu::Renderer::new(engine.clone(), renderer::Settings::default());
                let _lease = fresh.load_image(&image).unwrap();
                record(&mut fresh, &image, phase);
                assert_eq!(
                    frame,
                    fresh.screenshot(&viewport, Color::BLACK),
                    "retained instances differ at phase {phase}, scale {scale}, limit {limit}"
                );
                pixels.push(frame);
                assert_eq!(
                    idle.screenshot(&viewport, Color::BLACK),
                    idle_pixels,
                    "active window must not overwrite idle window transforms/samplers"
                );
            }
            pixels.push(idle_pixels);
            active.reset(Rectangle::with_size(Size::new(256.0, 128.0)));
            assert!(
                active
                    .screenshot(&viewport, Color::BLACK)
                    .chunks_exact(4)
                    .all(|p| p == [0, 0, 0, 255])
            );
        }
        assert_ne!(
            pixels[0], pixels[2],
            "changed transforms must affect pixels"
        );
        if let Some(reference) = &reference {
            assert_eq!(
                &pixels, reference,
                "pixel mismatch with immediate limit {limit}"
            );
        } else {
            reference = Some(pixels);
        }
        let report = instance.generate_report().unwrap().hub;
        let counts = (
            report.buffers.num_kept_from_user,
            report.bind_groups.num_kept_from_user,
        );
        eprintln!(
            "TRANSFORM_RESOURCES limit={limit} buffers={} bindings={}",
            counts.0, counts.1
        );
        match limit {
            0 => baseline_counts = Some(counts),
            16 => assert_eq!(
                Some(counts),
                baseline_counts,
                "insufficient limits retain uniform resources"
            ),
            _ => {
                let baseline = baseline_counts.unwrap();
                assert!(
                    counts.0 + 6 <= baseline.0,
                    "image transforms need no per-layer buffer"
                );
                assert!(
                    counts.1 + 10 <= baseline.1,
                    "sampler bindings are shared across six layers"
                );
            }
        }
        drop(leases);
        drop(active);
        drop(idle);
        drop(engine);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let report = instance.generate_report().unwrap().hub;
        assert_eq!(report.buffers.num_kept_from_user, 0);
        assert_eq!(report.bind_groups.num_kept_from_user, 0);
    }
}
