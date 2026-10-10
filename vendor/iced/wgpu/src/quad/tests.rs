use super::*;
use crate::core::{Color, gradient};

mod batching {
    use super::*;

    fn batch(kinds: &[Kind], start: usize) -> Batch {
        let mut batch = Batch::default();
        let color = Color::from_rgba(0.2, 0.4, 0.6, 0.5);
        let gradient = gradient::Linear::new(0.0)
            .add_stop(0.0, color)
            .add_stop(1.0, Color::from_rgba(0.6, 0.4, 0.2, 0.5));

        for (index, kind) in kinds.iter().enumerate() {
            let quad = Quad {
                // Overlapping translucent quads make order significant.
                position: [(start + index) as f32, 0.0],
                size: [32.0, 32.0],
                ..Quad::zeroed()
            };
            let background = match kind {
                Kind::Solid => Background::Color(color),
                Kind::Gradient => Background::Gradient(gradient.into()),
            };
            batch.add(quad, &background);
        }

        batch
    }

    fn sequence(batch: &Batch) -> Vec<(Kind, f32)> {
        let mut solids = batch.solids.iter();
        let mut gradients = batch.gradients.iter();
        let mut sequence = Vec::new();

        for (kind, count) in &batch.order {
            for _ in 0..*count {
                let quad = match kind {
                    Kind::Solid => &solids.next().expect("solid run instance").quad,
                    Kind::Gradient => &gradients.next().expect("gradient run instance").quad,
                };
                sequence.push((*kind, quad.position[0]));
            }
        }

        assert!(solids.next().is_none(), "all solid instances drawn");
        assert!(gradients.next().is_none(), "all gradient instances drawn");
        sequence
    }

    fn assert_drained(batch: &Batch) {
        assert!(batch.is_empty());
        assert!(batch.solids.is_empty());
        assert!(batch.gradients.is_empty());
        assert!(batch.order.is_empty());
    }

    #[test]
    fn append_coalesces_matching_solid_and_gradient_boundaries() {
        for (kind, other) in [(Kind::Solid, Kind::Gradient), (Kind::Gradient, Kind::Solid)] {
            let mut target = batch(&[other, kind, kind], 0);
            let mut donor = batch(&[kind, kind, other], 3);
            let expected = [sequence(&target), sequence(&donor)].concat();
            let solids: Vec<u8> = [
                bytemuck::cast_slice(&target.solids),
                bytemuck::cast_slice(&donor.solids),
            ]
            .concat();
            let gradients: Vec<u8> = [
                bytemuck::cast_slice(&target.gradients),
                bytemuck::cast_slice(&donor.gradients),
            ]
            .concat();

            target.append(&mut donor);

            assert_eq!(target.order, [(other, 1), (kind, 4), (other, 1)]);
            assert_eq!(sequence(&target), expected);
            assert_eq!(bytemuck::cast_slice::<_, u8>(&target.solids), solids);
            assert_eq!(bytemuck::cast_slice::<_, u8>(&target.gradients), gradients);
            assert_drained(&donor);
        }
    }

    #[test]
    fn append_preserves_alternating_translucent_draw_order() {
        let kinds = [Kind::Solid, Kind::Gradient, Kind::Solid, Kind::Gradient];
        let mut target = batch(&kinds, 0);
        let mut donor = batch(&kinds, 4);
        let expected = [sequence(&target), sequence(&donor)].concat();

        target.append(&mut donor);

        assert_eq!(target.order.len(), 8);
        assert!(target.order.iter().all(|(_, count)| *count == 1));
        assert_eq!(sequence(&target), expected);
        assert_drained(&donor);
    }

    #[test]
    fn append_handles_empty_batches_and_retains_donor_allocations() {
        let mut target = Batch::default();
        let mut donor = batch(&[Kind::Solid, Kind::Gradient], 0);
        let expected = sequence(&donor);
        let capacities = (
            donor.solids.capacity(),
            donor.gradients.capacity(),
            donor.order.capacity(),
        );

        target.append(&mut donor);
        target.append(&mut donor);

        assert_eq!(sequence(&target), expected);
        assert_drained(&donor);
        assert_eq!(
            (
                donor.solids.capacity(),
                donor.gradients.capacity(),
                donor.order.capacity(),
            ),
            capacities
        );

        target.clear();
        target.append(&mut donor);
        assert_drained(&target);
    }

    #[test]
    fn repeated_appends_coalesce_only_adjacent_runs() {
        let mut target = Batch::default();
        let mut expected = Vec::new();
        let chunks: &[&[Kind]] = &[
            &[Kind::Solid, Kind::Solid],
            &[Kind::Solid],
            &[],
            &[Kind::Gradient, Kind::Gradient],
            &[Kind::Gradient, Kind::Solid],
            &[Kind::Solid, Kind::Gradient],
        ];

        for chunk in chunks {
            let mut donor = batch(chunk, expected.len());
            expected.extend(sequence(&donor));
            target.append(&mut donor);
            assert_eq!(sequence(&target), expected);
            assert_drained(&donor);
        }

        assert_eq!(
            target.order,
            [
                (Kind::Solid, 3),
                (Kind::Gradient, 3),
                (Kind::Solid, 2),
                (Kind::Gradient, 1),
            ]
        );
    }
}

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    static INSTANCE: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();
    let instance = INSTANCE.get_or_init(|| {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    });
    let adapter = futures::executor::block_on(
        instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
    )
    .ok()?;
    futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

fn capture(device: &wgpu::Device, queue: &wgpu::Queue, background: Background) -> Vec<u8> {
    let pipeline = Pipeline::new(device, wgpu::TextureFormat::Rgba8Unorm);
    capture_quad(
        device,
        queue,
        &pipeline,
        background,
        Quad {
            position: [16.4998, 16.4998],
            size: [20.0, 20.0],
            border_color: color::pack(Color::TRANSPARENT),
            border_radius: [0.0; 4],
            border_width: 0.0,
            shadow_color: color::pack(Color::from_rgba(1.0, 0.0, 0.0, 0.75)),
            shadow_offset: [8.0, 8.0],
            shadow_blur_radius: 0.0,
            snap: 1,
        },
        1.0,
    )
}

fn capture_quad(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &Pipeline,
    background: Background,
    quad: Quad,
    scale: f32,
) -> Vec<u8> {
    let mut state = State::new();
    let mut batch = Batch::default();
    batch.add(quad, &background);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut belt = wgpu::util::StagingBelt::new(device.clone(), 1024);
    state.prepare(
        pipeline,
        device,
        &mut belt,
        &mut encoder,
        &batch,
        Transformation::orthographic(64, 64),
        scale,
    );
    belt.finish();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("quad regression target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..wgpu::RenderPassDescriptor::default()
        });
        state.render(
            pipeline,
            0,
            Rectangle::with_size(crate::core::Size::new(64, 64)),
            &batch,
            &mut pass,
        );
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("quad regression readback"),
        size: 64 * 64 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(64),
            },
        },
        target.size(),
    );
    let _ = queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    let _ = device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    readback.slice(..).get_mapped_range().to_vec()
}

#[test]
fn gradient_quads_match_solid_shadow_and_snapped_edges() {
    let Some((device, queue)) = device() else {
        eprintln!("Skipping Vulkan gradient validation: adapter unavailable");
        return;
    };
    let solid = capture(&device, &queue, Color::WHITE.into());
    let gradient = gradient::Linear::new(0.0)
        .add_stop(0.0, Color::WHITE)
        .add_stop(1.0, Color::WHITE);
    let actual = capture(&device, &queue, Background::Gradient(gradient.into()));
    for (index, (actual, expected)) in actual
        .chunks_exact(4)
        .zip(solid.chunks_exact(4))
        .enumerate()
    {
        assert_eq!(actual[3], expected[3], "alpha at pixel {index}");
        for channel in 0..3 {
            assert!(
                actual[channel].abs_diff(expected[channel]) <= 1,
                "color at pixel {index}, channel {channel}: {actual:?} vs {expected:?}"
            );
        }
    }
    assert_eq!(actual[(40 * 64 + 40) * 4 + 3], 191, "shadow outside quad");
    assert_eq!(actual[(20 * 64 + 16) * 4 + 3], 0, "edge snaps up");
}

#[test]
fn single_stop_gradient_keeps_its_color_across_the_quad() {
    let Some((device, queue)) = device() else {
        eprintln!("Skipping Vulkan gradient validation: adapter unavailable");
        return;
    };
    let gradient = gradient::Linear::new(0.0).add_stop(0.5, Color::WHITE);
    let actual = capture(&device, &queue, Background::Gradient(gradient.into()));
    for (x, y) in [(20, 20), (32, 32)] {
        let offset = (y * 64 + x) * 4;
        assert_eq!(&actual[offset..offset + 4], &[255, 255, 255, 255]);
    }
}

#[test]
fn solid_interiors_preserve_rounded_borders_shadows_and_fractional_edges() {
    let Some((device, queue)) = device() else {
        eprintln!("Skipping Vulkan solid validation: adapter unavailable");
        return;
    };
    let pipeline = Pipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    for scale in [0.75, 1.0, 1.5] {
        for case in 0..6 {
            let color = Color::from_rgba(0.25, 0.6, 0.85, if case % 2 == 0 { 1.0 } else { 0.45 });
            let quad = Quad {
                position: [7.25, 8.75],
                size: if case == 5 { [2.5, 3.0] } else { [38.0, 37.5] },
                border_color: color::pack(Color::from_rgba(0.9, 0.2, 0.1, 0.7)),
                border_radius: if case < 2 {
                    [0.0; 4]
                } else {
                    [2.0, 8.0, 4.0, 12.0]
                },
                border_width: match case {
                    0 | 1 => 0.0,
                    2 | 3 => 1.25,
                    _ => 7.0,
                },
                shadow_color: color::pack(if case < 3 {
                    Color::TRANSPARENT
                } else {
                    Color::from_rgba(0.1, 0.3, 0.2, 0.5)
                }),
                shadow_offset: [2.0, -3.0],
                shadow_blur_radius: if case == 4 { 3.0 } else { 0.0 },
                snap: case % 2,
            };
            let solid = capture_quad(&device, &queue, &pipeline, color.into(), quad, scale);
            // The constant gradient uses the original distance-field path at
            // every pixel, including interiors skipped by the solid shader.
            let gradient = gradient::Linear::new(0.0)
                .add_stop(0.0, color)
                .add_stop(1.0, color);
            let reference = capture_quad(
                &device,
                &queue,
                &pipeline,
                Background::Gradient(gradient.into()),
                quad,
                scale,
            );
            for (index, (actual, expected)) in solid.iter().zip(&reference).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "scale={scale} case={case} byte={index}: {actual} vs {expected}"
                );
            }
        }
    }
}
