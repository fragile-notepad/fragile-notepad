//! Draw meshes of triangles.
mod msaa;

use crate::Buffer;
use crate::core::{Rectangle, Size, Transformation};
use crate::graphics::Antialiasing;
use crate::graphics::mesh::{self, Mesh};

use crate::nudge;

use rustc_hash::FxHashMap;
use std::sync::{Arc, Weak};

const INITIAL_INDEX_COUNT: usize = 1_000;
const INITIAL_VERTEX_COUNT: usize = 1_000;

pub type Batch = Vec<Item>;

#[derive(Debug)]
pub enum Item {
    Group {
        transformation: Transformation,
        meshes: Vec<Mesh>,
    },
    Cached {
        transformation: Transformation,
        cache: mesh::Cache,
    },
}

#[derive(Debug)]
struct Upload {
    layer: Layer,
    owner: Weak<()>,
    batch: Arc<[Mesh]>,
    used: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UploadKey {
    id: mesh::Id,
    version: usize,
    transformation: [u32; 16],
}

impl UploadKey {
    fn new(cache: &mesh::Cache, transformation: Transformation) -> Self {
        Self {
            id: cache.id(),
            version: cache.version(),
            transformation: transformation.as_ref().map(f32::to_bits),
        }
    }
}

#[derive(Debug, Default)]
pub struct Storage {
    uploads: FxHashMap<UploadKey, Upload>,
    unused: FxHashMap<(mesh::Id, usize), Vec<UploadKey>>,
}

impl Storage {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, cache: &mesh::Cache, transformation: Transformation) -> Option<&Upload> {
        if cache.is_empty() {
            return None;
        }

        self.uploads.get(&UploadKey::new(cache, transformation))
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        belt: &mut wgpu::util::StagingBelt,
        solid: &solid::Pipeline,
        gradient: &gradient::Pipeline,
        cache: &mesh::Cache,
        new_transformation: Transformation,
    ) {
        if cache.is_empty() {
            return;
        }

        let key = UploadKey::new(cache, new_transformation);
        if let Some(upload) = self.uploads.get_mut(&key) {
            upload.used = true;
            return;
        }

        let mut reused = None;
        if let Some(unused) = self.unused.get_mut(&(key.id, key.version)) {
            while let Some(previous) = unused.pop() {
                if self
                    .uploads
                    .get(&previous)
                    .is_some_and(|upload| !upload.used)
                {
                    reused = self.uploads.remove(&previous);
                    break;
                }
            }
        }
        let mut upload = if let Some(mut upload) = reused {
            // Keep vertex/index buffers when only placement changed. Each
            // position prepared for this paint still owns separate uniforms.
            // Encoded copies follow earlier draws that used this upload.
            upload
                .layer
                .prepare_transformation(encoder, belt, &upload.batch, new_transformation);
            upload
        } else {
            let mut layer = Layer::new(device, solid, gradient);
            layer.prepare(
                device,
                encoder,
                belt,
                solid,
                gradient,
                cache.batch(),
                new_transformation,
            );
            log::debug!(
                "New mesh upload: {:?} (total: {})",
                cache.id(),
                self.uploads.len() + 1
            );
            Upload {
                layer,
                owner: cache.weak_owner(),
                batch: cache.batch().clone(),
                used: true,
            }
        };
        upload.used = true;
        let _ = self.uploads.insert(key, upload);
    }

    pub fn trim(&mut self, prepared: bool) {
        self.unused.clear();
        self.uploads.retain(|key, upload| {
            let retain = upload.owner.strong_count() > 0 && (!prepared || upload.used);
            if prepared {
                upload.used = false;
            }
            if retain && !upload.used {
                self.unused
                    .entry((key.id, key.version))
                    .or_default()
                    .push(*key);
            }
            retain
        });
    }
}

#[derive(Debug, Clone)]
pub struct Pipeline {
    msaa: Option<msaa::Pipeline>,
    solid: solid::Pipeline,
    gradient: gradient::Pipeline,
}

pub struct State {
    passes: Vec<PreparedPass>,
    prepare_pass: usize,
    render_pass: usize,
    layers: Vec<Layer>,
    prepare_layer: usize,
    storage: Storage,
}

struct PreparedPass {
    projection: Transformation,
    bounds: Option<Rectangle<u32>>,
    msaa: Option<msaa::State>,
}

impl State {
    pub fn new(_device: &wgpu::Device, _pipeline: &Pipeline) -> Self {
        Self {
            passes: Vec::new(),
            prepare_pass: 0,
            render_pass: 0,
            layers: Vec::new(),
            prepare_layer: 0,
            storage: Storage::new(),
        }
    }

    pub fn prepare(
        &mut self,
        pipeline: &Pipeline,
        device: &wgpu::Device,
        belt: &mut wgpu::util::StagingBelt,
        encoder: &mut wgpu::CommandEncoder,
        items: &[Item],
        scale: Transformation,
        target_size: Size<u32>,
        bounds: Rectangle,
    ) {
        if self.passes.len() <= self.prepare_pass {
            self.passes.push(PreparedPass {
                projection: Transformation::IDENTITY,
                bounds: None,
                msaa: pipeline
                    .msaa
                    .as_ref()
                    .map(|pipeline| msaa::State::new(device, pipeline)),
            });
        }
        let pass = &mut self.passes[self.prepare_pass];
        pass.bounds = mesh_bounds(items, scale, bounds);
        let projection = if let Some(((state, pipeline), bounds)) = pass
            .msaa
            .as_mut()
            .zip(pipeline.msaa.as_ref())
            .zip(pass.bounds)
        {
            state.prepare(device, encoder, belt, pipeline, bounds, target_size) * scale
        } else {
            Transformation::orthographic(target_size.width, target_size.height) * scale
        };
        pass.projection = projection;
        self.prepare_pass += 1;

        for item in items {
            match item {
                Item::Group {
                    transformation,
                    meshes,
                } => {
                    if self.layers.len() <= self.prepare_layer {
                        self.layers
                            .push(Layer::new(device, &pipeline.solid, &pipeline.gradient));
                    }

                    let layer = &mut self.layers[self.prepare_layer];
                    layer.prepare(
                        device,
                        encoder,
                        belt,
                        &pipeline.solid,
                        &pipeline.gradient,
                        meshes,
                        projection * *transformation,
                    );

                    self.prepare_layer += 1;
                }
                Item::Cached {
                    transformation,
                    cache,
                } => {
                    self.storage.prepare(
                        device,
                        encoder,
                        belt,
                        &pipeline.solid,
                        &pipeline.gradient,
                        cache,
                        projection * *transformation,
                    );
                }
            }
        }
    }

    /// Forces the next MSAA pass to refresh its resolve ratio uniform.
    ///
    /// A warm-up submission can complete on a different command-buffer path
    /// than the first presented frame. Refreshing this small uniform at that
    /// boundary avoids relying on the driver's visibility of the warm-up copy.
    pub fn invalidate_msaa_ratio(&mut self) {
        for pass in &mut self.passes {
            if let Some(msaa) = &mut pass.msaa {
                msaa.invalidate_ratio();
            }
        }
    }

    pub fn render(
        &mut self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        start: usize,
        batch: &Batch,
        bounds: Rectangle,
        screen_transformation: Transformation,
    ) -> usize {
        let pass = &self.passes[self.render_pass];
        self.render_pass += 1;
        if pass.bounds.is_none() {
            return batch
                .iter()
                .filter(|item| matches!(item, Item::Group { .. }))
                .count();
        }
        let mut layer_count = 0;

        let items = batch.iter().filter_map(|item| match item {
            Item::Group {
                transformation,
                meshes,
            } => {
                let layer = &self.layers[start + layer_count];
                layer_count += 1;

                Some((
                    layer,
                    meshes.as_slice(),
                    screen_transformation * *transformation,
                ))
            }
            Item::Cached {
                transformation,
                cache,
            } => {
                let upload = self.storage.get(cache, pass.projection * *transformation)?;

                Some((
                    &upload.layer,
                    &upload.batch,
                    screen_transformation * *transformation,
                ))
            }
        });

        render(
            encoder,
            target,
            pass.msaa.as_ref().zip(pipeline.msaa.as_ref()),
            &pipeline.solid,
            &pipeline.gradient,
            bounds,
            items,
        );

        layer_count
    }

    pub fn trim(&mut self) {
        self.storage.trim(self.prepare_pass > 0);
        if self.prepare_pass > 0 {
            self.passes.truncate(self.prepare_pass);
        }

        self.prepare_layer = 0;
        self.prepare_pass = 0;
        self.render_pass = 0;
    }
}

fn mesh_bounds(items: &[Item], scale: Transformation, bounds: Rectangle) -> Option<Rectangle<u32>> {
    items
        .iter()
        .flat_map(|item| {
            let (transformation, meshes) = match item {
                Item::Group {
                    transformation,
                    meshes,
                } => (transformation, meshes.as_slice()),
                Item::Cached {
                    transformation,
                    cache,
                } => (transformation, cache.batch().as_ref()),
            };
            meshes.iter().filter_map(move |mesh| {
                (mesh.clip_bounds() * (scale * *transformation)).intersection(&bounds)
            })
        })
        .reduce(|union, clip| union.union(&clip))
        .and_then(nudge::snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Color, Point};

    fn mesh() -> Mesh {
        Mesh::Solid {
            buffers: mesh::Indexed {
                vertices: [[0.0, 0.0], [20.0, 0.0], [0.0, 10.0]]
                    .map(|position| mesh::SolidVertex2D {
                        position,
                        color: crate::graphics::color::pack(Color::WHITE),
                    })
                    .to_vec(),
                indices: vec![0, 1, 2],
            },
            transformation: Transformation::IDENTITY,
            clip_bounds: Rectangle::with_size(Size::new(20.0, 10.0)),
        }
    }

    #[test]
    fn scratch_bounds_clip_transformed_meshes() {
        let cache = mesh::Cache::new(Arc::from([mesh()]));
        let items = [
            Item::Group {
                transformation: Transformation::translate(10.0, 20.0),
                meshes: vec![mesh()],
            },
            Item::Cached {
                transformation: Transformation::translate(100.0, 50.0),
                cache,
            },
        ];
        let scale = Transformation::scale(1.5);
        assert_eq!(
            mesh_bounds(
                &items,
                scale,
                Rectangle::with_size(Size::new(1500.0, 1000.0))
            ),
            Some(Rectangle {
                x: 15,
                y: 30,
                width: 165,
                height: 60,
            })
        );
        assert_eq!(
            mesh_bounds(
                &items,
                scale,
                Rectangle::new(Point::new(160.0, 80.0), Size::new(5.0, 4.0))
            ),
            Some(Rectangle {
                x: 160,
                y: 80,
                width: 5,
                height: 4,
            })
        );
        assert_eq!(
            mesh_bounds(
                &items,
                scale,
                Rectangle::new(Point::new(500.0, 500.0), Size::new(5.0, 4.0))
            ),
            None
        );
    }

    #[test]
    fn uploads_distinguish_versions_and_positions() {
        let mut current = mesh::Cache::new(Arc::from([mesh()]));
        let previous = current.clone();
        current.update(Arc::from([mesh()]));
        assert_eq!(previous.id(), current.id());

        let projection = Transformation::orthographic(32, 32);
        assert_ne!(
            UploadKey::new(&previous, projection),
            UploadKey::new(&current, projection),
            "cache versions share an upload key"
        );
        assert_ne!(
            UploadKey::new(&current, projection),
            UploadKey::new(&current, projection * Transformation::translate(8.0, 0.0)),
            "different positions share an upload key"
        );
    }

    #[test]
    fn moving_instances_reuse_mesh_buffers_and_keep_separate_uniforms() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = match futures::executor::block_on(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        ) {
            Ok(adapter) => adapter,
            Err(error) => {
                let required = match std::env::var("FRAGILE_REQUIRE_GPU_TESTS") {
                    Ok(value) if value == "1" => true,
                    Ok(value) if value == "0" => false,
                    Err(std::env::VarError::NotPresent) => std::env::var_os("CI").is_some(),
                    value => panic!("invalid FRAGILE_REQUIRE_GPU_TESTS: {value:?}"),
                };
                assert!(!required, "no Vulkan adapter for mesh reuse test: {error}");
                eprintln!("Skipping Vulkan mesh reuse test: {error}");
                return;
            }
        };
        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&Default::default()))
                .expect("create Vulkan mesh reuse test device");
        let pipeline = Pipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm, None);
        let mut storage = Storage::new();
        let mut belt =
            wgpu::util::StagingBelt::new(device.clone(), crate::buffer::STAGING_CHUNK_SIZE);
        let mut cache = mesh::Cache::new(Arc::from([
            mesh(),
            Mesh::Gradient {
                buffers: mesh::Indexed {
                    vertices: [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]]
                        .map(|position| mesh::GradientVertex2D {
                            position,
                            gradient: bytemuck::Zeroable::zeroed(),
                        })
                        .to_vec(),
                    indices: vec![0, 1, 2],
                },
                transformation: Transformation::scale(0.75),
                clip_bounds: Rectangle::with_size(Size::new(10.0, 10.0)),
            },
        ]));
        let buffers = |upload: &Upload| {
            [
                upload.layer.index_buffer.raw.clone(),
                upload.layer.solid.vertices.raw.clone(),
                upload.layer.gradient.vertices.raw.clone(),
                upload.layer.solid.uniforms.raw.clone(),
                upload.layer.gradient.uniforms.raw.clone(),
            ]
        };
        let mut initial = Vec::new();
        for frame in 0..4 {
            // Completed-frame reuse must leave the uploads available for a
            // later paint. Submit changing frames without waiting for the GPU.
            storage.trim(false);
            let mut encoder = device.create_command_encoder(&Default::default());
            for index in 0..3 {
                let transformation = Transformation::translate(
                    index as f32 * 8.0,
                    if index == 0 { 0.0 } else { frame as f32 * 0.3 },
                );
                storage.prepare(
                    &device,
                    &mut encoder,
                    &mut belt,
                    &pipeline.solid,
                    &pipeline.gradient,
                    &cache,
                    transformation,
                );
                let current = buffers(storage.get(&cache, transformation).unwrap());
                if frame == 0 {
                    initial.push(current);
                } else {
                    assert!(initial.contains(&current), "movement replaced mesh buffers");
                }
            }
            assert_eq!(storage.uploads.len(), 3);
            let active: Vec<_> = storage.uploads.values().map(buffers).collect();
            for index in 0..active.len() {
                for previous in &active[..index] {
                    assert_ne!(active[index][3], previous[3]);
                    assert_ne!(active[index][4], previous[4]);
                }
            }
            belt.finish();
            let _ = queue.submit([encoder.finish()]);
            belt.recall();
            storage.trim(true);
        }
        // Reusing old vertices after a content change would paint stale meshes.
        cache.update(Arc::from([mesh()]));
        let mut encoder = device.create_command_encoder(&Default::default());
        storage.prepare(
            &device,
            &mut encoder,
            &mut belt,
            &pipeline.solid,
            &pipeline.gradient,
            &cache,
            Transformation::IDENTITY,
        );
        assert!(!initial.contains(&buffers(
            storage.get(&cache, Transformation::IDENTITY).unwrap()
        )));
        belt.finish();
        let _ = queue.submit([encoder.finish()]);
        belt.recall();
        storage.trim(true);
        assert_eq!(storage.uploads.len(), 1);
        drop(cache);
        storage.trim(false);
        assert!(storage.uploads.is_empty());
        assert!(storage.unused.values().all(Vec::is_empty));
        let _ = device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }

    #[test]
    fn uploads_do_not_extend_cache_lifetime() {
        let mut current = mesh::Cache::new(Arc::from([mesh()]));
        let previous = current.clone();
        current.update(Arc::from([mesh()]));
        let uploads = [
            (current.batch().clone(), current.weak_owner()),
            (previous.batch().clone(), previous.weak_owner()),
            (current.batch().clone(), current.weak_owner()),
        ];
        assert!(Weak::ptr_eq(&uploads[0].1, &uploads[1].1));
        assert!(uploads.iter().all(|(_, owner)| owner.strong_count() == 2));

        drop(current);
        assert!(uploads.iter().all(|(_, owner)| owner.strong_count() == 1));
        drop(previous);
        assert!(uploads.iter().all(|(_, owner)| owner.strong_count() == 0));
        assert!(uploads.iter().all(|(batch, _)| !batch.is_empty()));
    }
}

impl Pipeline {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        antialiasing: Option<Antialiasing>,
    ) -> Pipeline {
        Pipeline {
            msaa: antialiasing.map(|a| msaa::Pipeline::new(device, format, a)),
            solid: solid::Pipeline::new(device, format, antialiasing),
            gradient: gradient::Pipeline::new(device, format, antialiasing),
        }
    }
}

fn render<'a>(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    msaa: Option<(&msaa::State, &msaa::Pipeline)>,
    solid: &solid::Pipeline,
    gradient: &gradient::Pipeline,
    bounds: Rectangle,
    group: impl Iterator<Item = (&'a Layer, &'a [Mesh], Transformation)>,
) {
    let origin = msaa.map_or((0, 0), |(state, _)| {
        let bounds = state.bounds();
        (bounds.x, bounds.y)
    });
    {
        let mut render_pass = if let Some((state, _pipeline)) = &msaa {
            state.render_pass(encoder)
        } else {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iced_wgpu.triangle.render_pass"),
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
            })
        };

        for (layer, meshes, transformation) in group {
            layer.render(
                solid,
                gradient,
                meshes,
                bounds,
                transformation,
                origin,
                &mut render_pass,
            );
        }
    }

    if let Some((state, pipeline)) = msaa {
        state.render(pipeline, encoder, target, bounds);
    }
}

#[derive(Debug)]
pub struct Layer {
    index_buffer: Buffer<u32>,
    solid: solid::Layer,
    gradient: gradient::Layer,
}

impl Layer {
    fn new(device: &wgpu::Device, solid: &solid::Pipeline, gradient: &gradient::Pipeline) -> Self {
        Self {
            index_buffer: Buffer::new(
                device,
                "iced_wgpu.triangle.index_buffer",
                INITIAL_INDEX_COUNT,
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            ),
            solid: solid::Layer::new(device, &solid.constants_layout),
            gradient: gradient::Layer::new(device, &gradient.constants_layout),
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        belt: &mut wgpu::util::StagingBelt,
        solid: &solid::Pipeline,
        gradient: &gradient::Pipeline,
        meshes: &[Mesh],
        transformation: Transformation,
    ) {
        // Count the total amount of vertices & indices we need to handle
        let count = mesh::attribute_count_of(meshes);

        // Then we ensure the current attribute buffers are big enough, resizing if necessary.
        // We are not currently using the return value of these functions as we have no system in
        // place to calculate mesh diff, or to know whether or not that would be more performant for
        // the majority of use cases. Therefore we will write GPU data every frame (for now).
        let _ = self.index_buffer.resize(device, count.indices);
        let _ = self.solid.vertices.resize(device, count.solid_vertices);
        let _ = self
            .gradient
            .vertices
            .resize(device, count.gradient_vertices);

        if self.solid.uniforms.resize(device, count.solids) {
            self.solid.constants =
                solid::Layer::bind_group(device, &self.solid.uniforms.raw, &solid.constants_layout);
        }

        if self.gradient.uniforms.resize(device, count.gradients) {
            self.gradient.constants = gradient::Layer::bind_group(
                device,
                &self.gradient.uniforms.raw,
                &gradient.constants_layout,
            );
        }

        let mut solid_vertex_offset = 0;
        let mut gradient_vertex_offset = 0;
        let mut index_offset = 0;

        for mesh in meshes {
            // Snapping vertices here would shift them when the MSAA target resizes.
            // Snap only the physical scissor rectangles.
            let indices = mesh.indices();

            index_offset += self
                .index_buffer
                .write(encoder, belt, index_offset, indices);

            match mesh {
                Mesh::Solid { buffers, .. } => {
                    solid_vertex_offset += self.solid.vertices.write(
                        encoder,
                        belt,
                        solid_vertex_offset,
                        &buffers.vertices,
                    );
                }
                Mesh::Gradient { buffers, .. } => {
                    gradient_vertex_offset += self.gradient.vertices.write(
                        encoder,
                        belt,
                        gradient_vertex_offset,
                        &buffers.vertices,
                    );
                }
            }
        }
        self.prepare_transformation(encoder, belt, meshes, transformation);
    }

    fn prepare_transformation(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        belt: &mut wgpu::util::StagingBelt,
        meshes: &[Mesh],
        transformation: Transformation,
    ) {
        let mut solid_offset = 0;
        let mut gradient_offset = 0;
        for mesh in meshes {
            let uniforms = [Uniforms::new(transformation * mesh.transformation())];
            match mesh {
                Mesh::Solid { .. } => {
                    solid_offset +=
                        self.solid
                            .uniforms
                            .write(encoder, belt, solid_offset, &uniforms);
                }
                Mesh::Gradient { .. } => {
                    gradient_offset +=
                        self.gradient
                            .uniforms
                            .write(encoder, belt, gradient_offset, &uniforms);
                }
            }
        }
    }

    fn render<'a>(
        &'a self,
        solid: &'a solid::Pipeline,
        gradient: &'a gradient::Pipeline,
        meshes: &[Mesh],
        bounds: Rectangle,
        transformation: Transformation,
        origin: (u32, u32),
        render_pass: &mut wgpu::RenderPass<'a>,
    ) {
        let mut num_solids = 0;
        let mut num_gradients = 0;
        let mut solid_offset = 0;
        let mut gradient_offset = 0;
        let mut index_offset = 0;
        let mut last_is_solid = None;

        for mesh in meshes {
            let Some(clip_bounds) = bounds
                .intersection(&(mesh.clip_bounds() * transformation))
                .and_then(nudge::snap)
            else {
                match mesh {
                    Mesh::Solid { buffers, .. } => {
                        solid_offset += buffers.vertices.len();
                        num_solids += 1;
                    }
                    Mesh::Gradient { buffers, .. } => {
                        gradient_offset += buffers.vertices.len();
                        num_gradients += 1;
                    }
                }
                index_offset += mesh.indices().len();
                continue;
            };

            render_pass.set_scissor_rect(
                clip_bounds.x - origin.0,
                clip_bounds.y - origin.1,
                clip_bounds.width,
                clip_bounds.height,
            );

            match mesh {
                Mesh::Solid { buffers, .. } => {
                    if !last_is_solid.unwrap_or(false) {
                        render_pass.set_pipeline(&solid.pipeline);

                        last_is_solid = Some(true);
                    }

                    render_pass.set_bind_group(
                        0,
                        &self.solid.constants,
                        &[(num_solids * std::mem::size_of::<Uniforms>()) as u32],
                    );

                    render_pass.set_vertex_buffer(
                        0,
                        self.solid
                            .vertices
                            .range(solid_offset, solid_offset + buffers.vertices.len()),
                    );

                    num_solids += 1;
                    solid_offset += buffers.vertices.len();
                }
                Mesh::Gradient { buffers, .. } => {
                    if last_is_solid.unwrap_or(true) {
                        render_pass.set_pipeline(&gradient.pipeline);

                        last_is_solid = Some(false);
                    }

                    render_pass.set_bind_group(
                        0,
                        &self.gradient.constants,
                        &[(num_gradients * std::mem::size_of::<Uniforms>()) as u32],
                    );

                    render_pass.set_vertex_buffer(
                        0,
                        self.gradient
                            .vertices
                            .range(gradient_offset, gradient_offset + buffers.vertices.len()),
                    );

                    num_gradients += 1;
                    gradient_offset += buffers.vertices.len();
                }
            };

            render_pass.set_index_buffer(
                self.index_buffer
                    .range(index_offset, index_offset + mesh.indices().len()),
                wgpu::IndexFormat::Uint32,
            );

            render_pass.draw_indexed(0..mesh.indices().len() as u32, 0, 0..1);

            index_offset += mesh.indices().len();
        }
    }
}

fn fragment_target(texture_format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format: texture_format,
        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    }
}

fn primitive_state() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Cw,
        ..Default::default()
    }
}

fn multisample_state(antialiasing: Option<Antialiasing>) -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: antialiasing.map(Antialiasing::sample_count).unwrap_or(1),
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
}

#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    transform: [f32; 16],
    /// Uniform values must be 256-aligned;
    /// see: [`wgpu::Limits`] `min_uniform_buffer_offset_alignment`.
    _padding: [f32; 48],
}

impl Uniforms {
    pub fn new(transform: Transformation) -> Self {
        Self {
            transform: transform.into(),
            _padding: [0.0; 48],
        }
    }

    pub fn entry() -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Self>() as u64),
            },
            count: None,
        }
    }

    pub fn min_size() -> Option<wgpu::BufferSize> {
        wgpu::BufferSize::new(std::mem::size_of::<Self>() as u64)
    }
}

mod solid {
    use crate::Buffer;
    use crate::graphics::Antialiasing;
    use crate::graphics::mesh;
    use crate::triangle;

    #[derive(Debug, Clone)]
    pub struct Pipeline {
        pub pipeline: wgpu::RenderPipeline,
        pub constants_layout: wgpu::BindGroupLayout,
    }

    #[derive(Debug)]
    pub struct Layer {
        pub vertices: Buffer<mesh::SolidVertex2D>,
        pub uniforms: Buffer<triangle::Uniforms>,
        pub constants: wgpu::BindGroup,
    }

    impl Layer {
        pub fn new(device: &wgpu::Device, constants_layout: &wgpu::BindGroupLayout) -> Self {
            let vertices = Buffer::new(
                device,
                "iced_wgpu.triangle.solid.vertex_buffer",
                triangle::INITIAL_VERTEX_COUNT,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            );

            let uniforms = Buffer::new(
                device,
                "iced_wgpu.triangle.solid.uniforms",
                1,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );

            let constants = Self::bind_group(device, &uniforms.raw, constants_layout);

            Self {
                vertices,
                uniforms,
                constants,
            }
        }

        pub fn bind_group(
            device: &wgpu::Device,
            buffer: &wgpu::Buffer,
            layout: &wgpu::BindGroupLayout,
        ) -> wgpu::BindGroup {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iced_wgpu.triangle.solid.bind_group"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer,
                        offset: 0,
                        size: triangle::Uniforms::min_size(),
                    }),
                }],
            })
        }
    }

    impl Pipeline {
        pub fn new(
            device: &wgpu::Device,
            format: wgpu::TextureFormat,
            antialiasing: Option<Antialiasing>,
        ) -> Self {
            let constants_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("iced_wgpu.triangle.solid.bind_group_layout"),
                    entries: &[triangle::Uniforms::entry()],
                });

            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("iced_wgpu.triangle.solid.pipeline_layout"),
                bind_group_layouts: &[Some(&constants_layout)],
                immediate_size: 0,
            });

            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("iced_wgpu.triangle.solid.shader"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(concat!(
                    include_str!("shader/triangle.wgsl"),
                    "\n",
                    include_str!("shader/triangle/solid.wgsl"),
                    "\n",
                    include_str!("shader/color.wgsl"),
                ))),
            });

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("iced_wgpu::triangle::solid pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("solid_vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<mesh::SolidVertex2D>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array!(
                            // Position
                            0 => Float32x2,
                            // Color
                            1 => Float32x4,
                        ),
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("solid_fs_main"),
                    targets: &[Some(triangle::fragment_target(format))],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: triangle::primitive_state(),
                depth_stencil: None,
                multisample: triangle::multisample_state(antialiasing),
                multiview_mask: None,
                cache: None,
            });

            Self {
                pipeline,
                constants_layout,
            }
        }
    }
}

mod gradient {
    use crate::Buffer;
    use crate::graphics::Antialiasing;
    use crate::graphics::mesh;
    use crate::triangle;

    #[derive(Debug, Clone)]
    pub struct Pipeline {
        pub pipeline: wgpu::RenderPipeline,
        pub constants_layout: wgpu::BindGroupLayout,
    }

    #[derive(Debug)]
    pub struct Layer {
        pub vertices: Buffer<mesh::GradientVertex2D>,
        pub uniforms: Buffer<triangle::Uniforms>,
        pub constants: wgpu::BindGroup,
    }

    impl Layer {
        pub fn new(device: &wgpu::Device, constants_layout: &wgpu::BindGroupLayout) -> Self {
            let vertices = Buffer::new(
                device,
                "iced_wgpu.triangle.gradient.vertex_buffer",
                triangle::INITIAL_VERTEX_COUNT,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            );

            let uniforms = Buffer::new(
                device,
                "iced_wgpu.triangle.gradient.uniforms",
                1,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );

            let constants = Self::bind_group(device, &uniforms.raw, constants_layout);

            Self {
                vertices,
                uniforms,
                constants,
            }
        }

        pub fn bind_group(
            device: &wgpu::Device,
            uniform_buffer: &wgpu::Buffer,
            layout: &wgpu::BindGroupLayout,
        ) -> wgpu::BindGroup {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iced_wgpu.triangle.gradient.bind_group"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniform_buffer,
                        offset: 0,
                        size: triangle::Uniforms::min_size(),
                    }),
                }],
            })
        }
    }

    impl Pipeline {
        pub fn new(
            device: &wgpu::Device,
            format: wgpu::TextureFormat,
            antialiasing: Option<Antialiasing>,
        ) -> Self {
            let constants_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("iced_wgpu.triangle.gradient.bind_group_layout"),
                    entries: &[triangle::Uniforms::entry()],
                });

            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("iced_wgpu.triangle.gradient.pipeline_layout"),
                bind_group_layouts: &[Some(&constants_layout)],
                immediate_size: 0,
            });

            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("iced_wgpu.triangle.gradient.shader"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(concat!(
                    include_str!("shader/triangle.wgsl"),
                    "\n",
                    include_str!("shader/triangle/gradient.wgsl"),
                    "\n",
                    include_str!("shader/color.wgsl"),
                    "\n",
                    include_str!("shader/color/linear_rgb.wgsl")
                ))),
            });

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("iced_wgpu.triangle.gradient.pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("gradient_vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<mesh::GradientVertex2D>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array!(
                            // Position
                            0 => Float32x2,
                            // Colors 1-2
                            1 => Uint32x4,
                            // Colors 3-4
                            2 => Uint32x4,
                            // Colors 5-6
                            3 => Uint32x4,
                            // Colors 7-8
                            4 => Uint32x4,
                            // Offsets
                            5 => Uint32x4,
                            // Direction
                            6 => Float32x4
                        ),
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("gradient_fs_main"),
                    targets: &[Some(triangle::fragment_target(format))],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: triangle::primitive_state(),
                depth_stencil: None,
                multisample: triangle::multisample_state(antialiasing),
                multiview_mask: None,
                cache: None,
            });

            Self {
                pipeline,
                constants_layout,
            }
        }
    }
}
