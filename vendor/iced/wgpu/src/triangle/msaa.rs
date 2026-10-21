use crate::core::{Rectangle, Size, Transformation};
use crate::graphics;

use std::num::NonZeroU64;
use std::sync::{Arc, RwLock, Weak};

#[derive(Debug, Clone)]
pub struct Pipeline {
    format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    raw: wgpu::RenderPipeline,
    constant_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sample_count: u32,
    targets: Arc<RwLock<TargetPool>>,
}

#[derive(Debug, Default)]
struct TargetPool {
    retained: Vec<Arc<Targets>>,
    shared: Vec<Weak<Targets>>,
}

impl Pipeline {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        antialiasing: graphics::Antialiasing,
    ) -> Pipeline {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());

        let constant_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iced_wgpu::triangle:msaa uniforms layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iced_wgpu::triangle::msaa texture layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iced_wgpu::triangle::msaa pipeline layout"),
            bind_group_layouts: &[Some(&constant_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iced_wgpu triangle blit_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../shader/blit.wgsl"
            ))),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iced_wgpu::triangle::msaa pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Cw,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            format,
            sampler,
            raw: pipeline,
            constant_layout,
            texture_layout,
            sample_count: antialiasing.sample_count(),
            targets: Arc::new(RwLock::new(TargetPool::default())),
        }
    }

    fn targets(&self, device: &wgpu::Device, size: Size<u32>) -> Arc<Targets> {
        let mut targets = self.targets.write().expect("Write MSAA targets");
        targets.shared.retain(|target| target.strong_count() > 0);
        let shared = targets
            .shared
            .iter()
            .filter_map(Weak::upgrade)
            .find(|target| target.size == size);
        if let Some(target) = shared {
            if let Some(index) = targets
                .retained
                .iter()
                .position(|retained| Arc::ptr_eq(retained, &target))
            {
                let target = targets.retained.remove(index);
                targets.retained.push(target);
            }
            return target;
        }
        let target = Arc::new(Targets::new(
            device,
            self.format,
            &self.texture_layout,
            self.sample_count,
            size,
        ));
        // Passes use these attachments sequentially, including across windows
        // on the shared queue. Weak entries also share oversized attachments
        // while passes own them, without retaining them beyond those passes.
        const BUDGET: u64 = 32 * 1024 * 1024;
        let bytes = |size: Size<u32>| {
            u64::from(size.width) * u64::from(size.height) * 4 * u64::from(self.sample_count + 1)
        };
        if bytes(size) <= BUDGET {
            while !targets.retained.is_empty()
                && (targets.retained.len() >= 8
                    || targets
                        .retained
                        .iter()
                        .map(|target| bytes(target.size))
                        .sum::<u64>()
                        + bytes(size)
                        > BUDGET)
            {
                let _ = targets.retained.remove(0);
            }
            targets.retained.push(target.clone());
        }
        targets.shared.push(Arc::downgrade(&target));
        target
    }
}

fn bucket_size(size: Size<u32>, viewport: Size<u32>) -> Size<u32> {
    Size::new(
        size.width.next_power_of_two().min(viewport.width),
        size.height.next_power_of_two().min(viewport.height),
    )
}

#[derive(Debug, Clone)]
struct Targets {
    attachment: wgpu::TextureView,
    resolve: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    size: Size<u32>,
}

impl Targets {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        texture_layout: &wgpu::BindGroupLayout,
        sample_count: u32,
        size: Size<u32>,
    ) -> Targets {
        let extent = wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };

        let attachment = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iced_wgpu::triangle::msaa attachment"),
            size: extent,
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        let resolve = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iced_wgpu::triangle::msaa resolve target"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let attachment = attachment.create_view(&wgpu::TextureViewDescriptor::default());

        let resolve = resolve.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iced_wgpu::triangle::msaa texture bind group"),
            layout: texture_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&resolve),
            }],
        });

        Targets {
            attachment,
            resolve,
            bind_group,
            size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Ratio {
    u: f32,
    v: f32,
    // Padding field for 16-byte alignment.
    // See https://docs.rs/wgpu/latest/wgpu/struct.DownlevelFlags.html#associatedconstant.BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED
    _padding: [f32; 2],
}

pub struct State {
    ratio: wgpu::Buffer,
    constants: wgpu::BindGroup,
    last_ratio: Option<Ratio>,
    targets: Option<Arc<Targets>>,
    bounds: Rectangle<u32>,
}

impl State {
    pub fn new(device: &wgpu::Device, pipeline: &Pipeline) -> Self {
        let ratio = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iced_wgpu::triangle::msaa ratio"),
            size: std::mem::size_of::<Ratio>() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });

        let constants = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iced_wgpu::triangle::msaa uniforms bind group"),
            layout: &pipeline.constant_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ratio.as_entire_binding(),
                },
            ],
        });

        Self {
            ratio,
            constants,
            last_ratio: None,
            targets: None,
            bounds: Rectangle::default(),
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        belt: &mut wgpu::util::StagingBelt,
        pipeline: &Pipeline,
        bounds: Rectangle<u32>,
        viewport: Size<u32>,
    ) -> Transformation {
        let region_size = Size::new(bounds.width, bounds.height);
        let size = bucket_size(region_size, viewport);
        let targets = match self.targets.as_ref() {
            Some(targets) if targets.size == size => targets.clone(),
            _ => pipeline.targets(device, size),
        };

        let ratio = Ratio {
            u: region_size.width as f32 / targets.size.width as f32,
            v: region_size.height as f32 / targets.size.height as f32,
            _padding: [0.0; 2],
        };

        if Some(ratio) != self.last_ratio {
            belt.write_buffer(
                encoder,
                &self.ratio,
                0,
                NonZeroU64::new(std::mem::size_of::<Ratio>() as u64).expect("non-empty ratio"),
            )
            .copy_from_slice(bytemuck::bytes_of(&ratio));

            self.last_ratio = Some(ratio);
        }

        let projection = Transformation::orthographic(targets.size.width, targets.size.height)
            * Transformation::translate(-(bounds.x as f32), -(bounds.y as f32));
        self.targets = Some(targets);
        self.bounds = bounds;
        projection
    }

    pub fn invalidate_ratio(&mut self) {
        self.last_ratio = None;
    }

    pub fn bounds(&self) -> Rectangle<u32> {
        self.bounds
    }

    pub fn render_pass<'a>(&self, encoder: &'a mut wgpu::CommandEncoder) -> wgpu::RenderPass<'a> {
        let targets = self.targets.as_ref().expect("Prepared MSAA target");
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iced_wgpu.triangle.render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.attachment,
                depth_slice: None,
                resolve_target: Some(&targets.resolve),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    pub fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        bounds: Rectangle,
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iced_wgpu::triangle::msaa render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        if let Some(bounds) = crate::nudge::snap(bounds) {
            render_pass.set_scissor_rect(bounds.x, bounds.y, bounds.width, bounds.height);
        } else {
            return;
        }
        render_pass.set_viewport(
            self.bounds.x as f32,
            self.bounds.y as f32,
            self.bounds.width as f32,
            self.bounds.height as f32,
            0.0,
            1.0,
        );
        render_pass.set_pipeline(&pipeline.raw);
        render_pass.set_bind_group(0, &self.constants, &[]);
        render_pass.set_bind_group(
            1,
            &self
                .targets
                .as_ref()
                .expect("Prepared MSAA target")
                .bind_group,
            &[],
        );
        render_pass.draw(0..6, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable(reason: &str) {
        const REQUIRE_GPU: &str = "FRAGILE_REQUIRE_GPU_TESTS";
        let required = match std::env::var(REQUIRE_GPU) {
            Ok(value) => match value.as_str() {
                "1" => true,
                "0" => false,
                _ => panic!("{REQUIRE_GPU} must be 0 or 1, got {value:?}"),
            },
            Err(std::env::VarError::NotPresent) => std::env::var_os("CI").is_some(),
            Err(error) => panic!("cannot read {REQUIRE_GPU}: {error}"),
        };
        assert!(
            !required,
            "GPU validation is required ({REQUIRE_GPU}=1 or CI is set): {reason}"
        );
        eprintln!("Skipping GPU validation: {reason}; set {REQUIRE_GPU}=1 to require it");
    }

    #[test]
    fn oversized_scratch_targets_share_until_released() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = match futures::executor::block_on(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        ) {
            Ok(adapter) => adapter,
            Err(error) => {
                unavailable(&format!("MSAA scratch validation: {error}"));
                return;
            }
        };
        let (device, _queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("create Vulkan MSAA scratch test device");
        let pipeline = Pipeline::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            graphics::Antialiasing::MSAAx4,
        );
        // Four samples and a resolve texture exceed the 32 MiB strong pool.
        let first = pipeline.targets(&device, Size::new(1920, 1080));
        let second = pipeline.targets(&device, Size::new(1920, 1080));
        assert!(
            Arc::ptr_eq(&first, &second),
            "equal oversized passes allocated separate scratch attachments"
        );
        assert!(
            pipeline.targets.read().unwrap().retained.is_empty(),
            "the pool retained an oversized scratch attachment"
        );
        let lifetime = Arc::downgrade(&first);
        drop(first);
        assert!(lifetime.upgrade().is_some());
        drop(second);
        assert!(
            lifetime.upgrade().is_none(),
            "scratch attachments outlived their last prepared pass"
        );
    }

    #[test]
    fn scratch_targets_fit_the_viewport() {
        let viewport = Size::new(1536, 1152);
        assert_eq!(bucket_size(Size::new(19, 23), viewport), Size::new(32, 32));
        assert_eq!(
            bucket_size(Size::new(1000, 21), viewport),
            Size::new(1024, 32)
        );
        assert_eq!(bucket_size(viewport, viewport), viewport);
    }
}
