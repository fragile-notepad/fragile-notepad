//! Cache the diffuse trail at the software fallback's bounded logical resolution.
//! Field rasterization precedes Iced's main pass in its existing frame encoder;
//! the main pass only linearly samples it. No extra submissions or readbacks.
//! Sharp artwork keeps its independent image draws at the window's native DPI.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use iced::Rectangle;
use iced::widget::shader::{self, Viewport};

#[derive(Debug)]
pub(super) struct Trail {
    pub instance: Instance,
    pub time: f32,
    pub opacity: f32,
    pub dark: bool,
}

/// Retain each window's field while its widget or a recorded frame needs it.
/// wgpu also retains resources referenced by encoded and submitted GPU work.
#[derive(Debug, Clone, Default)]
pub(super) struct Instance(Arc<()>);

impl Instance {
    fn key(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

struct Uniform {
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    field_parameters: Option<[f32; 4]>,
    composite_parameters: Option<[f32; 4]>,
}

impl Uniform {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        // Separate vectors in one allocation. Writing opacity must not overwrite
        // a pending field pass's parameters before the frame is submitted.
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("About trail parameters"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("About trail parameter binding"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            buffer,
            binding,
            field_parameters: None,
            composite_parameters: None,
        }
    }

    fn update(&mut self, queue: &wgpu::Queue, field: [f32; 4], composite: [f32; 4]) {
        if self.field_parameters != Some(field) {
            queue.write_buffer(&self.buffer, 0, &parameter_bytes(field));
            self.field_parameters = Some(field);
        }
        if self.composite_parameters != Some(composite) {
            queue.write_buffer(&self.buffer, 16, &parameter_bytes(composite));
            self.composite_parameters = Some(composite);
        }
    }
}

struct Field {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    binding: wgpu::BindGroup,
}

impl Field {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("About low-resolution trail"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("About trail texture binding"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        Self {
            texture,
            view,
            binding,
        }
    }

    fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }
}

struct Slot {
    owner: Weak<()>,
    field: Field,
    uniform: Option<Uniform>,
    rendered_parameters: Option<[f32; 4]>,
    #[cfg(test)]
    rasterizations: u32,
}

pub(super) struct Pipeline {
    field_raw: wgpu::RenderPipeline,
    composite_raw: wgpu::RenderPipeline,
    parameters_layout: Option<wgpu::BindGroupLayout>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    field_format: wgpu::TextureFormat,
    slots: HashMap<usize, Slot>,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let immediates = device.features().contains(wgpu::Features::IMMEDIATES)
            && device.limits().max_immediate_size >= 16;
        let parameters_layout = (!immediates).then(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("About trail parameters layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(32),
                    },
                    count: None,
                }],
            })
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("About trail texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("About trail linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let field_format = if format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let field_layouts: Vec<_> = parameters_layout.as_ref().map(Some).into_iter().collect();
        let field_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("About trail field pipeline layout"),
            bind_group_layouts: &field_layouts,
            immediate_size: if immediates { 16 } else { 0 },
        });
        let field_raw = render_pipeline(
            device,
            "About trail field pipeline",
            &field_layout,
            format!(
                "{}\nconst LINEAR_TARGET: bool = {};\n{}",
                parameter_declaration(immediates, 0, "field"),
                format.is_srgb(),
                include_str!("trail.wgsl"),
            ),
            field_format,
            None,
        );
        let mut composite_layouts = vec![Some(&texture_layout)];
        composite_layouts.extend(parameters_layout.as_ref().map(Some));
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("About trail composite pipeline layout"),
            bind_group_layouts: &composite_layouts,
            immediate_size: if immediates { 16 } else { 0 },
        });
        let composite_raw = render_pipeline(
            device,
            "About trail composite pipeline",
            &composite_layout,
            format!(
                "{}\n{}",
                parameter_declaration(immediates, 1, "composite"),
                include_str!("composite.wgsl"),
            ),
            format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        Self {
            field_raw,
            composite_raw,
            parameters_layout,
            texture_layout,
            sampler,
            field_format,
            slots: HashMap::new(),
        }
    }

    fn trim(&mut self) {
        // Paused windows retain their own slots. Once all owners disappear,
        // wgpu keeps any resources still referenced by pending GPU work alive.
        self.slots.retain(|_, slot| slot.owner.strong_count() > 0);
    }
}

impl shader::Primitive for Trail {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        let size = field_size(bounds);
        let Pipeline {
            parameters_layout,
            texture_layout,
            sampler,
            field_format,
            slots,
            ..
        } = pipeline;
        let slot = slots.entry(self.instance.key()).or_insert_with(|| Slot {
            owner: Arc::downgrade(&self.instance.0),
            field: Field::new(device, texture_layout, sampler, *field_format, size),
            uniform: parameters_layout
                .as_ref()
                .map(|layout| Uniform::new(device, layout)),
            rendered_parameters: None,
            #[cfg(test)]
            rasterizations: 0,
        });
        if slot.field.size() != size {
            slot.field = Field::new(device, texture_layout, sampler, *field_format, size);
            slot.rendered_parameters = None;
        }
        if let Some(uniform) = &mut slot.uniform {
            uniform.update(
                queue,
                self.field_parameters(size),
                self.composite_parameters(),
            );
        }
    }

    fn prepare_with_encoder(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        self.prepare(pipeline, device, queue, bounds, viewport);
        let slot = pipeline.slots.get_mut(&self.instance.key()).unwrap();
        let parameters = self.field_parameters(slot.field.size());
        if slot.rendered_parameters == Some(parameters) {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("About low-resolution trail raster"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &slot.field.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline.field_raw);
        if let Some(uniform) = &slot.uniform {
            pass.set_bind_group(0, &uniform.binding, &[]);
        } else {
            pass.set_immediates(0, &parameter_bytes(parameters));
        }
        pass.draw(0..3, 0..1);
        drop(pass);
        slot.rendered_parameters = Some(parameters);
        #[cfg(test)]
        {
            slot.rasterizations += 1;
        }
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(slot) = pipeline.slots.get(&self.instance.key()) else {
            return true;
        };
        if slot.rendered_parameters.is_none() {
            return true;
        }
        pass.set_pipeline(&pipeline.composite_raw);
        pass.set_bind_group(0, &slot.field.binding, &[]);
        if let Some(uniform) = &slot.uniform {
            pass.set_bind_group(1, &uniform.binding, &[]);
        } else {
            pass.set_immediates(0, &parameter_bytes(self.composite_parameters()));
        }
        pass.draw(0..3, 0..1);
        true
    }
}

impl Trail {
    fn field_parameters(&self, size: [u32; 2]) -> [f32; 4] {
        [
            self.time,
            u8::from(self.dark) as f32,
            size[0] as f32,
            size[1] as f32,
        ]
    }

    fn composite_parameters(&self) -> [f32; 4] {
        [self.opacity, 0.0, 0.0, 0.0]
    }
}

fn field_size(bounds: &Rectangle) -> [u32; 2] {
    // Logical size deliberately ignores DPI, just like the software field.
    [
        (bounds.width * 0.5).ceil().clamp(8.0, 160.0) as u32,
        (bounds.height * 0.5).ceil().clamp(8.0, 48.0) as u32,
    ]
}

fn parameter_declaration(immediates: bool, group: u32, member: &str) -> String {
    let (declaration, value) = if immediates {
        (
            "var<immediate> parameters: vec4<f32>;".to_owned(),
            "parameters".to_owned(),
        )
    } else {
        (
            format!(
                "struct Parameters {{ field: vec4<f32>, composite: vec4<f32>, }}\n\
                 @group({group}) @binding(0) var<uniform> parameters: Parameters;"
            ),
            format!("parameters.{member}"),
        )
    };
    format!("{declaration}\nfn {member}_parameters() -> vec4<f32> {{ return {value}; }}")
}

fn render_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    source: String,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn parameter_bytes(parameters: [f32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (value, target) in parameters.into_iter().zip(bytes.chunks_exact_mut(4)) {
        target.copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;
    use shader::{Pipeline as _, Primitive as _};

    fn device(immediates: bool) -> Option<(wgpu::Instance, wgpu::Device, wgpu::Queue)> {
        let vulkan = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let Some(adapter) =
            futures::executor::block_on(vulkan.request_adapter(&Default::default())).ok()
        else {
            eprintln!("Skipping Vulkan resource lifecycle validation: no Vulkan adapter");
            return None;
        };
        let Ok((device, queue)) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: if immediates {
                    wgpu::Features::IMMEDIATES
                } else {
                    wgpu::Features::empty()
                },
                required_limits: wgpu::Limits {
                    max_immediate_size: if immediates { 16 } else { 0 },
                    ..Default::default()
                },
                ..Default::default()
            }))
        else {
            eprintln!("Skipping Vulkan resource lifecycle validation: device unavailable");
            return None;
        };
        Some((vulkan, device, queue))
    }

    #[test]
    fn vulkan_field_cache_reuses_resources_skips_static_and_opacity_only_rasters() {
        for (immediates, format) in [
            (false, wgpu::TextureFormat::Rgba8Unorm),
            (true, wgpu::TextureFormat::Rgba8Unorm),
            (false, wgpu::TextureFormat::Rgba8UnormSrgb),
            (true, wgpu::TextureFormat::Rgba8UnormSrgb),
        ] {
            let Some((vulkan, device, queue)) = device(immediates) else {
                continue;
            };
            let baseline = vulkan.generate_report().unwrap();
            let mut pipeline = Pipeline::new(&device, &queue, format);
            let mut trail = Trail {
                instance: Instance::default(),
                time: 2.9,
                opacity: 1.0,
                dark: false,
            };
            let bounds = Rectangle::with_size(Size::new(280.0, 96.0));
            let viewport = Viewport::with_physical_size(Size::new(640, 480), 1.0);
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut prepare = |trail: &Trail,
                                   pipeline: &mut Pipeline,
                                   bounds: &Rectangle,
                                   viewport: &Viewport| {
                    trail.prepare_with_encoder(
                        pipeline,
                        &device,
                        &queue,
                        bounds,
                        viewport,
                        &mut encoder,
                    );
                };
                prepare(&trail, &mut pipeline, &bounds, &viewport);
                let key = trail.instance.key();
                let texture = pipeline.slots[&key].field.texture.clone();
                let binding = pipeline.slots[&key].field.binding.clone();
                assert_eq!(pipeline.slots[&key].field.size(), [140, 48]);
                assert_eq!(pipeline.slots[&key].field.texture.format(), format);
                assert_eq!(pipeline.slots[&key].rasterizations, 1);
                assert_eq!(pipeline.parameters_layout.is_none(), immediates);
                assert_eq!(pipeline.slots[&key].uniform.is_none(), immediates);

                // Repeating a static frame and changing DPI must reuse its field.
                for scale in [1.0, 1.5, 2.0] {
                    let viewport = Viewport::with_physical_size(Size::new(640, 480), scale);
                    prepare(&trail, &mut pipeline, &bounds, &viewport);
                    trail.opacity = 0.4;
                    prepare(&trail, &mut pipeline, &bounds, &viewport);
                    assert_eq!(pipeline.slots[&key].rasterizations, 1);
                    assert_eq!(pipeline.slots[&key].field.texture, texture);
                    assert_eq!(pipeline.slots[&key].field.binding, binding);
                }
                trail.time += 0.1;
                prepare(&trail, &mut pipeline, &bounds, &viewport);
                trail.dark = true;
                prepare(&trail, &mut pipeline, &bounds, &viewport);
                assert_eq!(pipeline.slots[&key].rasterizations, 3);
                assert_eq!(pipeline.slots[&key].field.texture, texture);
                let report = vulkan.generate_report().unwrap();
                assert_eq!(
                    report.hub.buffers.num_kept_from_user,
                    baseline.hub.buffers.num_kept_from_user + usize::from(!immediates)
                );
                assert_eq!(
                    report.hub.bind_groups.num_kept_from_user,
                    baseline.hub.bind_groups.num_kept_from_user + 1 + usize::from(!immediates)
                );
                assert_eq!(
                    report.hub.textures.num_kept_from_user,
                    baseline.hub.textures.num_kept_from_user + 1
                );
                assert_eq!(
                    report.hub.render_pipelines.num_kept_from_user,
                    baseline.hub.render_pipelines.num_kept_from_user + 2
                );

                let larger = Rectangle::with_size(Size::new(640.0, 192.0));
                prepare(&trail, &mut pipeline, &larger, &viewport);
                assert_eq!(pipeline.slots[&key].rasterizations, 4);
                assert_eq!(pipeline.slots[&key].field.size(), [160, 48]);
                assert_ne!(pipeline.slots[&key].field.texture, texture);
                let narrow = Rectangle::with_size(Size::new(16.0, 10.0));
                prepare(&trail, &mut pipeline, &narrow, &viewport);
                assert_eq!(pipeline.slots[&key].field.size(), [8, 8]);
            }
            drop(encoder);
            drop(trail);
            pipeline.trim();
            assert!(pipeline.slots.is_empty());
            drop(pipeline);
            queue.submit([]);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let released = vulkan.generate_report().unwrap();
            assert_eq!(
                released.hub.buffers.num_kept_from_user,
                baseline.hub.buffers.num_kept_from_user
            );
            assert_eq!(
                released.hub.bind_groups.num_kept_from_user,
                baseline.hub.bind_groups.num_kept_from_user
            );
            assert_eq!(
                released.hub.textures.num_kept_from_user,
                baseline.hub.textures.num_kept_from_user
            );
            assert_eq!(
                released.hub.render_pipelines.num_kept_from_user,
                baseline.hub.render_pipelines.num_kept_from_user
            );
        }
    }

    #[test]
    fn vulkan_resources_follow_widgets_recorded_frames_and_pending_gpu_work() {
        for immediates in [false, true] {
            let Some((vulkan, device, queue)) = device(immediates) else {
                continue;
            };
            let baseline = vulkan.generate_report().unwrap();
            let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            let widget = Instance::default();
            let other_widget = Instance::default();
            let mut trail = Trail {
                instance: widget.clone(),
                time: 0.0,
                opacity: 1.0,
                dark: false,
            };
            let bounds = Rectangle::with_size(Size::new(280.0, 96.0));
            let viewport = Viewport::with_physical_size(Size::new(640, 480), 1.0);
            let mut encoder = device.create_command_encoder(&Default::default());
            trail.prepare_with_encoder(
                &mut pipeline,
                &device,
                &queue,
                &bounds,
                &viewport,
                &mut encoder,
            );
            let texture = pipeline.slots[&widget.key()].field.texture.clone();
            let binding = pipeline.slots[&widget.key()].field.binding.clone();
            let buffer = pipeline.slots[&widget.key()]
                .uniform
                .as_ref()
                .map(|uniform| uniform.buffer.clone());

            // Another active window must not evict a paused window's field.
            trail.instance = other_widget.clone();
            trail.time = 7.25;
            trail.dark = true;
            trail.prepare_with_encoder(
                &mut pipeline,
                &device,
                &queue,
                &bounds,
                &viewport,
                &mut encoder,
            );
            pipeline.trim();
            assert_eq!(pipeline.slots[&widget.key()].field.texture, texture);
            assert_eq!(pipeline.slots[&widget.key()].field.binding, binding);
            assert_ne!(pipeline.slots[&other_widget.key()].field.texture, texture);
            assert_eq!(
                pipeline.slots[&widget.key()]
                    .uniform
                    .as_ref()
                    .map(|uniform| &uniform.buffer),
                buffer.as_ref()
            );
            let report = vulkan.generate_report().unwrap();
            assert_eq!(
                report.hub.buffers.num_kept_from_user,
                baseline.hub.buffers.num_kept_from_user + if immediates { 0 } else { 2 }
            );
            assert_eq!(
                report.hub.bind_groups.num_kept_from_user,
                baseline.hub.bind_groups.num_kept_from_user + if immediates { 2 } else { 4 }
            );
            assert_eq!(
                report.hub.textures.num_kept_from_user,
                baseline.hub.textures.num_kept_from_user + 2
            );

            // Record the main-pass sample too: its bindings and shared sampler
            // must remain valid after all widget/pipeline owners are released.
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("About trail lifecycle target"),
                size: wgpu::Extent3d {
                    width: 280,
                    height: 96,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let target_view = target.create_view(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                assert!(trail.draw(&pipeline, &mut pass));
            }
            drop(target_view);
            drop(target);

            drop(widget);
            drop(texture);
            drop(binding);
            drop(buffer);
            pipeline.trim();
            assert_eq!(pipeline.slots.len(), 1);
            drop(other_widget);
            // A recorded frame may outlive its widget during handoff or closing.
            pipeline.trim();
            assert_eq!(pipeline.slots.len(), 1);
            drop(trail);
            pipeline.trim();
            assert!(pipeline.slots.is_empty());
            drop(pipeline);
            // Dropping owners before submission must not invalidate encoded work.
            queue.submit([encoder.finish()]);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let released = vulkan.generate_report().unwrap();
            assert_eq!(
                released.hub.buffers.num_kept_from_user,
                baseline.hub.buffers.num_kept_from_user
            );
            assert_eq!(
                released.hub.bind_groups.num_kept_from_user,
                baseline.hub.bind_groups.num_kept_from_user
            );
            assert_eq!(
                released.hub.textures.num_kept_from_user,
                baseline.hub.textures.num_kept_from_user
            );
            assert_eq!(
                released.hub.render_pipelines.num_kept_from_user,
                baseline.hub.render_pipelines.num_kept_from_user
            );
        }
    }
}
