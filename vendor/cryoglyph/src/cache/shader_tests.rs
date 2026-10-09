use super::*;
use wgpu::util::DeviceExt;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    struct WakeThread(std::thread::Thread);
    impl std::task::Wake for WakeThread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(WakeThread(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

fn reference_shader() -> String {
    let mut shader = include_str!("../shader.wgsl").to_owned();
    // Restore the original normalized, nearest sampler and smooth color varying.
    // Keeping the remaining source identical makes this a focused comparison.
    for (optimized, original) in [
        (
            "@location(0) @interpolate(flat) color: vec4<f32>,",
            "@location(0) color: vec4<f32>,",
        ),
        (
            "vert_output.uv = vec2<f32>(uv);",
            "vert_output.uv = vec2<f32>(uv) / vec2<f32>(select(\n\
             textureDimensions(color_atlas_texture),\n\
             textureDimensions(mask_atlas_texture), content_type == 1u));",
        ),
        (
            "textureLoad(color_atlas_texture, vec2<i32>(in_frag.uv), 0)",
            "textureSampleLevel(color_atlas_texture, atlas_sampler, in_frag.uv, 0.0)",
        ),
        (
            "textureLoad(mask_atlas_texture, vec2<i32>(in_frag.uv), 0)",
            "textureSampleLevel(mask_atlas_texture, atlas_sampler, in_frag.uv, 0.0)",
        ),
    ] {
        assert_eq!(shader.matches(optimized).count(), 1, "{optimized}");
        shader = shader.replace(optimized, original);
    }
    shader
}

fn atlas(
    device: &Device,
    queue: &wgpu::Queue,
    format: TextureFormat,
    width: u32,
    height: u32,
    data: &[u8],
    channels: u32,
) -> TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shader comparison atlas"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * channels),
            rows_per_image: Some(height),
        },
        texture.size(),
    );
    texture.create_view(&Default::default())
}

fn glyph(pos: [i32; 2], dim: [u16; 2], uv: [u16; 2], kind: u16) -> GlyphToRender {
    GlyphToRender {
        pos,
        dim,
        uv,
        color: 0x80ca7931,
        content_type_with_srgb: [kind, 0],
        depth: 0.0,
    }
}

fn capture(
    device: &Device,
    encoder: &mut wgpu::CommandEncoder,
    cache: &Cache,
    color_atlas: &TextureView,
    mask_atlas: &TextureView,
    vertices: &Buffer,
    glyph_count: u32,
    params: &Buffer,
    format: TextureFormat,
    viewport: [f32; 4],
) -> Buffer {
    let pipeline = cache.get_or_create_pipeline(device, format, Default::default(), None);
    let atlas = cache.create_atlas_bind_group(device, color_atlas, mask_atlas);
    let uniforms = cache.create_uniforms_bind_group(device, params);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shader comparison target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &atlas, &[]);
        pass.set_bind_group(1, &uniforms, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_viewport(viewport[0], viewport[1], viewport[2], viewport[3], 0.0, 1.0);
        pass.set_scissor_rect(1, 1, WIDTH - 2, HEIGHT - 2);
        pass.draw(0..4, 0..glyph_count);
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shader comparison readback"),
        size: (WIDTH * HEIGHT * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        texture.size(),
    );
    readback
}

fn pixels(device: &Device, buffer: &Buffer) -> Vec<u8> {
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

#[test]
fn integer_atlas_loads_match_nearest_sampling_for_mask_and_color_glyphs() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference:
            wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::HighPerformance),
        ..Default::default()
    })) else {
        eprintln!("Skipping Vulkan atlas shader validation: no Vulkan adapter");
        return;
    };
    eprintln!("GLYPH_SHADER_ADAPTER {:?}", adapter.get_info());
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    let optimized = Cache::new(&device);
    let mut reference = Cache::new(&device);
    Arc::get_mut(&mut reference.0).unwrap().shader =
        device.create_shader_module(ShaderModuleDescriptor {
            label: Some("original nearest atlas shader"),
            source: ShaderSource::Wgsl(Cow::Owned(reference_shader())),
        });

    // Every texel differs from its neighbors, including at the atlas edges.
    // Different atlas dimensions also catch using the wrong texture's size.
    let mask_data: Vec<u8> = (0..16)
        .flat_map(|y| (0..16).map(move |x| ((x * 17 + y * 29 + 31) % 256) as u8))
        .collect();
    let color_data: Vec<u8> = (0..16)
        .flat_map(|y| {
            (0..32).flat_map(move |x| {
                [
                    ((x * 23 + y * 17 + 19) % 256) as u8,
                    ((x * 11 + y * 31 + 47) % 256) as u8,
                    ((x * 37 + y * 7 + 89) % 256) as u8,
                    ((x * 13 + y * 19 + 67) % 256) as u8,
                ]
            })
        })
        .collect();
    let mask = atlas(
        &device,
        &queue,
        TextureFormat::R8Unorm,
        16,
        16,
        &mask_data,
        1,
    );
    let params_bytes: Vec<u8> = [WIDTH, HEIGHT, 0, 0]
        .into_iter()
        .flat_map(u32::to_ne_bytes)
        .collect();
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("shader comparison params"),
        contents: &params_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let base_glyphs = [
        glyph([4, 4], [9, 10], [2, 3], 1),
        glyph([22, 4], [9, 9], [3, 7], 0),
        // Prepared clipping advances UVs and reduces the remaining dimensions.
        glyph([4, 22], [4, 6], [6, 8], 1),
        glyph([22, 22], [2, 3], [12, 12], 0),
        // Raster/scissor clipping must still interpolate the same texels.
        glyph([-3, 34], [8, 5], [7, 2], 1),
        glyph([59, 34], [7, 5], [2, 5], 0),
        // Single-pixel glyphs and rectangles touching the last atlas texel.
        glyph([4, 44], [1, 1], [15, 15], 1),
        glyph([22, 44], [1, 1], [31, 15], 0),
        glyph([35, 44], [3, 2], [13, 14], 1),
        glyph([44, 44], [4, 6], [28, 10], 0),
        // Overlap exercises fractional coverage and alpha blending.
        glyph([40, 4], [8, 8], [0, 0], 1),
        glyph([42, 7], [6, 7], [8, 3], 0),
    ];
    // Prepared glyphs are clipped rectangles contained in their atlas allocation.
    // Out-of-bounds UVs would compare sampler clamping against textureLoad's zero
    // result, which is outside the renderer's contract.
    for glyph in &base_glyphs {
        let atlas_width = if glyph.content_type_with_srgb[0] == 0 {
            32
        } else {
            16
        };
        assert!(glyph.dim[0] > 0 && glyph.dim[1] > 0);
        assert!(u32::from(glyph.uv[0]) + u32::from(glyph.dim[0]) <= atlas_width);
        assert!(u32::from(glyph.uv[1]) + u32::from(glyph.dim[1]) <= 16);
    }
    for srgb in [false, true] {
        let format = if srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        };
        let color = atlas(&device, &queue, format, 32, 16, &color_data, 4);
        let glyphs = base_glyphs.map(|mut glyph| {
            glyph.content_type_with_srgb[1] = u16::from(srgb);
            glyph
        });
        // GlyphToRender is repr(C), contains no padding, and uses only numeric fields.
        let vertex_bytes = unsafe {
            std::slice::from_raw_parts(glyphs.as_ptr().cast::<u8>(), mem::size_of_val(&glyphs))
        };
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shader comparison glyphs"),
            contents: vertex_bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        for viewport in [
            [0.0, 0.0, WIDTH as f32, HEIGHT as f32],
            [3.0, 5.0, 57.0, 55.0],
        ] {
            let mut encoder = device.create_command_encoder(&Default::default());
            let actual = capture(
                &device,
                &mut encoder,
                &optimized,
                &color,
                &mask,
                &vertices,
                glyphs.len() as u32,
                &params,
                format,
                viewport,
            );
            let expected = capture(
                &device,
                &mut encoder,
                &reference,
                &color,
                &mask,
                &vertices,
                glyphs.len() as u32,
                &params,
                format,
                viewport,
            );
            queue.submit([encoder.finish()]);
            let actual = pixels(&device, &actual);
            let expected = pixels(&device, &expected);
            for (offset, (&actual, &expected)) in actual.iter().zip(&expected).enumerate() {
                // Constant smooth varyings may round differently from flat varyings
                // by one UNORM step; selecting a different texel exceeds this bound.
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "srgb={srgb}, viewport={viewport:?}, pixel=({}, {}), channel={}, \
                     actual={actual}, expected={expected}",
                    offset / 4 % WIDTH as usize,
                    offset / 4 / WIDTH as usize,
                    offset % 4,
                );
            }
            if viewport[0] == 0.0 {
                // Require visible coverage from both atlas paths.
                for [x, y] in [[4, 4], [22, 4], [4, 44], [22, 44]] {
                    let start = ((y * WIDTH + x) * 4) as usize;
                    assert_ne!(&actual[start..start + 3], &[0, 0, 0]);
                }
            }
        }
    }
}
