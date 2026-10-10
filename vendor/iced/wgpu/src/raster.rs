//! Composite retained layer pixels into a render target.

use crate::core::{self, Rectangle, Size};
use crate::graphics::color;

use wgpu::util::DeviceExt;

use std::sync::Arc;

/// A pipeline for compositing premultiplied cached pixels.
#[derive(Debug, Clone)]
pub struct Pipeline {
    raw: wgpu::RenderPipeline,
    replace: wgpu::RenderPipeline,
    clear_raw: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    clear_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

/// An immutable premultiplied clear color binding.
#[derive(Debug, Clone)]
pub struct Clear {
    _buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// A single-sample texture retaining a painted layer.
#[derive(Debug)]
pub struct Target {
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
    /// The texture's texel storage size, excluding backend allocation overhead.
    pub(crate) bytes: u64,
}

/// A cached layer to composite at pixel-aligned logical bounds.
#[derive(Debug, Clone)]
pub struct Instance {
    pub target: Arc<Target>,
    pub bounds: Rectangle<f32>,
    pub _owner: core::renderer::Cache,
    pub revision: u64,
}

impl Pipeline {
    /// Creates a pipeline using the parent render target's format.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iced_wgpu::raster sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iced_wgpu::raster texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iced_wgpu::raster pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iced_wgpu::raster shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("raster.wgsl").into()),
        });

        let raw = create_pipeline(
            device,
            format,
            &pipeline_layout,
            &shader,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            "iced_wgpu::raster pipeline",
        );
        let replace = create_pipeline(
            device,
            format,
            &pipeline_layout,
            &shader,
            None,
            "iced_wgpu::raster replacement pipeline",
        );

        let clear_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iced_wgpu::raster clear color layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(16),
                },
                count: None,
            }],
        });

        let clear_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("iced_wgpu::raster clear pipeline layout"),
                bind_group_layouts: &[Some(&clear_layout)],
                immediate_size: 0,
            });
        let clear_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iced_wgpu::raster clear shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("raster_clear.wgsl").into()),
        });
        let clear_raw = create_pipeline(
            device,
            format,
            &clear_pipeline_layout,
            &clear_shader,
            None,
            "iced_wgpu::raster clear pipeline",
        );

        Self {
            raw,
            replace,
            clear_raw,
            format,
            layout,
            clear_layout,
            sampler,
        }
    }

    /// Allocates a nonempty target in physical pixels, using the parent format.
    pub fn create_target(&self, device: &wgpu::Device, size: Size<u32>) -> Target {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iced_wgpu::raster target"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iced_wgpu::raster target binding"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let bytes = u64::from(size.width)
            * u64::from(size.height)
            * u64::from(
                self.format
                    .block_copy_size(None)
                    .expect("Cached layers use a color render target format"),
            );

        Target {
            _texture: texture,
            view,
            bind_group,
            bytes,
        }
    }

    /// Creates an immutable binding matching the scene's premultiplied clear color.
    ///
    /// Reuse this binding for damage rectangles and frames until the color changes.
    /// The uniform is initialized once and does not permit queue writes.
    pub fn create_clear_binding(&self, device: &wgpu::Device, color: &core::Color) -> Clear {
        let [r, g, b, a] = color::pack(*color).components();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("iced_wgpu::raster clear color"),
            contents: bytemuck::bytes_of(&[r * a, g * a, b * a, a]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iced_wgpu::raster clear binding"),
            layout: &self.clear_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Clear {
            _buffer: buffer,
            bind_group,
        }
    }

    /// Composites a target at `bounds` in physical pixel coordinates.
    ///
    /// Integer-aligned bounds matching the target's size provide exact 1:1
    /// nearest sampling. The caller supplies clipping through the pass's scissor
    /// rectangle and restores the viewport before subsequent ordinary drawing.
    pub fn draw<'a>(
        &'a self,
        target: &'a Target,
        bounds: Rectangle<f32>,
        pass: &mut wgpu::RenderPass<'a>,
    ) {
        pass.set_viewport(bounds.x, bounds.y, bounds.width, bounds.height, 0.0, 1.0);
        pass.set_pipeline(&self.raw);
        pass.set_bind_group(0, &target.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Replaces the destination's complete RGBA pixels with a retained target.
    ///
    /// Bounds are physical pixels, with the same sampling and clipping rules as
    /// [`draw`](Self::draw). Blending is disabled, including for transparent pixels.
    pub fn draw_replace<'a>(
        &'a self,
        target: &'a Target,
        bounds: Rectangle<f32>,
        pass: &mut wgpu::RenderPass<'a>,
    ) {
        pass.set_viewport(bounds.x, bounds.y, bounds.width, bounds.height, 0.0, 1.0);
        pass.set_pipeline(&self.replace);
        pass.set_bind_group(0, &target.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Replaces pixels with a clear color within the current viewport and scissor.
    pub fn clear<'a>(&'a self, clear: &'a Clear, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.clear_raw);
        pass.set_bind_group(0, &clear.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    blend: Option<wgpu::BlendState>,
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
