//! A [`wgpu`] renderer for [Iced].
//!
//! ![The native path of the Iced ecosystem](https://github.com/iced-rs/iced/blob/0525d76ff94e828b7b21634fa94a747022001c83/docs/graphs/native.png?raw=true)
//!
//! [`wgpu`] supports most modern graphics backends: Vulkan, Metal, DX11, and
//! DX12 (OpenGL and WebGL are still WIP). Additionally, it will support the
//! incoming [WebGPU API].
//!
//! Currently, `iced_wgpu` supports the following primitives:
//! - Text, which is rendered using [`glyphon`].
//! - Quads or rectangles, with rounded borders and a solid background color.
//! - Clip areas, useful to implement scrollables or hide overflowing content.
//! - Images and SVG, loaded from memory or the file system.
//! - Meshes of triangles, useful to draw geometry freely.
//!
//! [Iced]: https://github.com/iced-rs/iced
//! [`wgpu`]: https://github.com/gfx-rs/wgpu-rs
//! [WebGPU API]: https://gpuweb.github.io/gpuweb/
//! [`glyphon`]: https://github.com/grovesNL/glyphon
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/iced-rs/iced/9ab6923e943f784985e9ef9ca28b10278297225d/docs/logo.svg"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(missing_docs)]
pub mod layer;
pub mod primitive;
pub mod window;

#[cfg(feature = "geometry")]
pub mod geometry;

mod buffer;
mod color;
mod composition;
mod engine;
mod nudge;
mod quad;
mod raster;
mod raster_cache;
mod text;
mod triangle;

#[cfg(any(feature = "image", feature = "svg"))]
#[path = "image/mod.rs"]
mod image;

#[cfg(not(any(feature = "image", feature = "svg")))]
#[path = "image/null.rs"]
mod image;

use buffer::Buffer;

use iced_debug as debug;
pub use iced_graphics as graphics;
pub use iced_graphics::core;

pub use wgpu;

pub use composition::Statistics as CompositionCacheStatistics;
pub use engine::Engine;
pub use layer::Layer;
pub use primitive::Primitive;
pub use raster_cache::Statistics as RasterCacheStatistics;

#[cfg(feature = "geometry")]
pub use geometry::Geometry;

use crate::core::renderer;
use crate::core::{Background, Color, Font, Pixels, Point, Rectangle, Size, Transformation};
use crate::graphics::compositor::{OffscreenWarmUpError, OffscreenWarmUpEvidence};
use crate::graphics::mesh;
use crate::graphics::text::{Editor, Paragraph};
use crate::graphics::{Shell, Viewport};

/// A [`wgpu`] graphics renderer for [`iced`].
///
/// [`wgpu`]: https://github.com/gfx-rs/wgpu-rs
/// [`iced`]: https://github.com/iced-rs/iced
pub struct Renderer {
    engine: Engine,
    settings: renderer::Settings,

    layers: layer::Stack,
    scale_factor: Option<f32>,

    quad: quad::State,
    triangle: Option<triangle::State>,
    text: text::State,
    text_viewport: text::Viewport,
    raster_cache: raster_cache::State,
    composition: composition::State,

    #[cfg(any(feature = "svg", feature = "image"))]
    image: image::State,

    // TODO: Centralize all the image feature handling
    #[cfg(any(feature = "svg", feature = "image"))]
    image_cache: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<image::Cache>>>,

    staging_belt: wgpu::util::StagingBelt,
    offscreen_warm_up: Option<OffscreenWarmUp>,
}

const OFFSCREEN_WARM_UP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

struct OffscreenWarmUp {
    _texture: wgpu::Texture,
    submission: wgpu::SubmissionIndex,
    completed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: std::time::Instant,
    evidence: OffscreenWarmUpEvidence,
}

fn warm_up_completed(
    completed: bool,
    elapsed: std::time::Duration,
) -> Result<bool, OffscreenWarmUpError> {
    if completed {
        Ok(true)
    } else if elapsed >= OFFSCREEN_WARM_UP_TIMEOUT {
        Err(OffscreenWarmUpError::Timeout)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod warm_up_tests {
    use super::*;

    #[test]
    fn gpu_completion_must_be_observed_before_warmup_succeeds() {
        assert_eq!(
            warm_up_completed(false, std::time::Duration::ZERO),
            Ok(false)
        );
        assert_eq!(
            warm_up_completed(false, OFFSCREEN_WARM_UP_TIMEOUT),
            Err(OffscreenWarmUpError::Timeout)
        );
        assert_eq!(warm_up_completed(true, OFFSCREEN_WARM_UP_TIMEOUT), Ok(true));
    }
}

impl Renderer {
    pub fn new(engine: Engine, settings: renderer::Settings) -> Self {
        Self {
            settings,
            layers: layer::Stack::new(),
            scale_factor: None,

            quad: quad::State::new(),
            triangle: None,
            text: text::State::new(),
            text_viewport: engine.text_pipeline.create_viewport(&engine.device),
            raster_cache: raster_cache::State::default(),
            composition: composition::State::default(),

            #[cfg(any(feature = "svg", feature = "image"))]
            image: image::State::new(),

            #[cfg(any(feature = "svg", feature = "image"))]
            image_cache: std::sync::Arc::new(std::sync::OnceLock::new()),

            // Small instance updates use a small chunk. The belt allocates
            // larger chunks on demand for writes up to MAX_WRITE_SIZE.
            staging_belt: wgpu::util::StagingBelt::new(
                engine.device.clone(),
                buffer::STAGING_CHUNK_SIZE,
            ),
            offscreen_warm_up: None,

            engine,
        }
    }

    /// Returns shared mask glyph cache occupancy and allocation/eviction counters.
    pub fn text_cache_statistics(&self) -> cryoglyph::CacheStatistics {
        self.engine.text_pipeline.cache_statistics()
    }

    /// Returns retained-surface counters and current texture occupancy.
    pub fn raster_cache_statistics(&self) -> RasterCacheStatistics {
        self.raster_cache.statistics()
    }

    /// Sets the retained texture budget (32 MiB by default). Zero disables caching.
    /// Current frame surfaces are released at the next reset when necessary.
    pub fn set_raster_cache_budget(&mut self, bytes: u64) {
        self.raster_cache.set_budget(bytes);
    }

    /// Returns final-frame repaint activity and retained texture bytes.
    pub fn composition_cache_statistics(&self) -> CompositionCacheStatistics {
        self.composition.stats()
    }

    /// Sets the final-frame texture budget (32 MiB by default). Zero disables it.
    pub fn set_composition_cache_budget(&mut self, bytes: u64) {
        self.composition.set_budget(bytes);
    }

    /// Enables retained final-frame painting and conservative damage tracking.
    pub fn set_composition_cache_enabled(&mut self, enabled: bool) {
        self.composition.set_enabled(enabled);
    }

    /// Sets the shared mask atlas size to retain before evicting older glyphs.
    ///
    /// The atlas grows on demand. This does not allocate immediately or impose
    /// a hard size limit when the visible glyphs need more space.
    pub fn set_text_cache_retention_dimension(&self, dimension: u32) {
        self.engine.text_pipeline.set_mask_cache_target(dimension);
    }

    /// Record commands that draw the current primitives to the target texture view.
    ///
    /// You must call [`finish`](Self::finish) and [`recall`](Self::recall) when submitting
    /// the resulting [`wgpu::CommandEncoder`].
    pub fn draw(
        &mut self,
        clear_color: Option<Color>,
        target: &wgpu::TextureView,
        viewport: &Viewport,
    ) -> wgpu::CommandEncoder {
        let mut encoder =
            self.engine
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("iced_wgpu encoder"),
                });

        let retained_ready = self.raster_cache.prepare(&mut encoder);
        self.layers.merge();
        let texture = target.texture();
        let eligible = texture.width() == viewport.physical_width()
            && texture.height() == viewport.physical_height()
            && texture.sample_count() == 1
            && texture.format() == self.engine.format;
        if let Some(plan) = self.composition.plan(
            &self.engine,
            &self.layers,
            viewport,
            if eligible { clear_color } else { None },
        ) {
            let complete = match plan.damage {
                composition::Damage::Full => {
                    self.encode(&mut encoder, clear_color, &plan.target.view, viewport)
                }
                composition::Damage::Partial(damage) => {
                    let complete = self.prepare(&mut encoder, viewport, Some(damage));
                    self.render(
                        &mut encoder,
                        &plan.target.view,
                        None,
                        viewport,
                        Some((damage, &plan.clear)),
                    );
                    self.trim();
                    complete
                }
                composition::Damage::Reuse => true,
            };
            self.composition.completed(complete && retained_ready);
            self.copy_composition(&mut encoder, &plan.target, target, viewport);
        } else {
            let _ = self.encode(&mut encoder, clear_color, target, viewport);
        }
        // Shared glyph and primitive generations advance once per complete scene,
        // after all cached surfaces and the main scene have been prepared.
        self.engine.trim();
        #[cfg(any(feature = "svg", feature = "image"))]
        if let Some(cache) = self.image_cache.get() {
            cache.lock().expect("Lock image cache").trim();
        }
        encoder
    }

    fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        clear_color: Option<Color>,
        target: &wgpu::TextureView,
        viewport: &Viewport,
    ) -> bool {
        let retained_ready = self.raster_cache.prepare(encoder);
        // Accumulate omitted images across every layer. Later worker completion
        // must not make an earlier incomplete paint eligible for reuse.
        let assets_ready = self.prepare(encoder, viewport, None);
        self.render(encoder, target, clear_color, viewport, None);
        self.trim();
        assets_ready && retained_ready
    }

    fn copy_composition(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &raster::Target,
        target: &wgpu::TextureView,
        viewport: &Viewport,
    ) {
        let texture = target.texture();
        if texture.usage().contains(wgpu::TextureUsages::COPY_DST)
            && texture.mip_level_count() == 1
            && texture.depth_or_array_layers() == 1
        {
            encoder.copy_texture_to_texture(
                source._texture.as_image_copy(),
                texture.as_image_copy(),
                wgpu::Extent3d {
                    width: viewport.physical_width(),
                    height: viewport.physical_height(),
                    depth_or_array_layers: 1,
                },
            );
        } else {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iced_wgpu retained frame presentation"),
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
            self.engine.raster_pipeline().draw_replace(
                source,
                Rectangle::<f32>::from(Rectangle::with_size(viewport.physical_size())),
                &mut pass,
            );
        }
    }

    fn trim(&mut self) {
        self.quad.trim();
        if let Some(triangle) = &mut self.triangle {
            triangle.trim();
        }
        self.text.trim();

        #[cfg(any(feature = "svg", feature = "image"))]
        {
            self.image.trim();
        }
    }

    pub fn present(
        &mut self,
        clear_color: Option<Color>,
        _format: wgpu::TextureFormat,
        frame: &wgpu::TextureView,
        viewport: &Viewport,
    ) -> wgpu::SubmissionIndex {
        let encoder = self.draw(clear_color, frame, viewport);

        self.finish();
        let submission = self.engine.queue.submit([encoder.finish()]);
        self.recall();
        submission
    }

    /// Submits the current primitives to an offscreen texture and waits for the
    /// GPU work to complete.
    pub fn warm_up_offscreen(
        &mut self,
        viewport: &Viewport,
        background_color: Color,
    ) -> Result<OffscreenWarmUpEvidence, OffscreenWarmUpError> {
        self.begin_warm_up_offscreen(viewport, background_color)?;
        let warm_up = self.offscreen_warm_up.as_ref().expect("started warm-up");
        let result = self.engine.device.poll(wgpu::PollType::Wait {
            submission_index: Some(warm_up.submission.clone()),
            timeout: Some(OFFSCREEN_WARM_UP_TIMEOUT),
        });
        if let Err(error) = result {
            self.offscreen_warm_up = None;
            return Err(OffscreenWarmUpError::Failed(error.to_string()));
        }
        self.poll_warm_up_offscreen()?
            .ok_or(OffscreenWarmUpError::Timeout)
    }

    /// Submits warm-up primitives while leaving completion to nonblocking polling.
    pub fn begin_warm_up_offscreen(
        &mut self,
        viewport: &Viewport,
        background_color: Color,
    ) -> Result<(), OffscreenWarmUpError> {
        if self.offscreen_warm_up.is_some() {
            return Err(OffscreenWarmUpError::Failed(
                "warm-up already pending".into(),
            ));
        }
        let size = viewport.physical_size();

        if size.width == 0 || size.height == 0 {
            return Err(OffscreenWarmUpError::InvalidDimensions);
        }

        let started = std::time::Instant::now();
        let passes = self.offscreen_passes();

        let texture = self.engine.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iced_wgpu.offscreen.warm_up_texture"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.engine.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let encoder = self.draw(Some(background_color), &view, viewport);

        self.finish();
        let submission = self.engine.queue.submit([encoder.finish()]);
        self.recall();

        let completed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = completed.clone();
        self.engine.queue.on_submitted_work_done(move || {
            signal.store(true, std::sync::atomic::Ordering::Release);
        });
        self.offscreen_warm_up = Some(OffscreenWarmUp {
            _texture: texture,
            submission,
            completed,
            started,
            evidence: OffscreenWarmUpEvidence {
                renderer_family: crate::core::backend::RendererFamily::Wgpu,
                adapter: None,
                backend: None,
                width: size.width,
                height: size.height,
                passes,
                elapsed_us: 0,
                submission_completed: false,
            },
        });
        Ok(())
    }

    /// Polls the device without waiting and rejects stalled warm-ups after a deadline.
    pub fn poll_warm_up_offscreen(
        &mut self,
    ) -> Result<Option<OffscreenWarmUpEvidence>, OffscreenWarmUpError> {
        if let Err(error) = self.engine.device.poll(wgpu::PollType::Poll) {
            self.offscreen_warm_up = None;
            return Err(OffscreenWarmUpError::Failed(error.to_string()));
        }
        let Some(warm_up) = self.offscreen_warm_up.as_ref() else {
            return Err(OffscreenWarmUpError::Failed("no pending warm-up".into()));
        };
        match warm_up_completed(
            warm_up.completed.load(std::sync::atomic::Ordering::Acquire),
            warm_up.started.elapsed(),
        ) {
            Ok(true) => {
                let mut warm_up = self.offscreen_warm_up.take().expect("pending warm-up");
                warm_up.evidence.elapsed_us = warm_up.started.elapsed().as_micros();
                warm_up.evidence.submission_completed = true;
                if let Some(triangle) = &mut self.triangle {
                    triangle.invalidate_msaa_ratio();
                }
                Ok(Some(warm_up.evidence))
            }
            Ok(false) => Ok(None),
            Err(error) => {
                self.offscreen_warm_up = None;
                Err(error)
            }
        }
    }

    /// Renders the current surface to an offscreen buffer.
    ///
    /// Returns RGBA bytes of the texture data.
    pub fn screenshot(&mut self, viewport: &Viewport, background_color: Color) -> Vec<u8> {
        #[derive(Clone, Copy, Debug)]
        struct BufferDimensions {
            width: u32,
            height: u32,
            unpadded_bytes_per_row: usize,
            padded_bytes_per_row: usize,
        }

        impl BufferDimensions {
            fn new(size: Size<u32>) -> Self {
                let unpadded_bytes_per_row = size.width as usize * 4; //slice of buffer per row; always RGBA
                let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize; //256
                let padded_bytes_per_row_padding =
                    (alignment - unpadded_bytes_per_row % alignment) % alignment;
                let padded_bytes_per_row = unpadded_bytes_per_row + padded_bytes_per_row_padding;

                Self {
                    width: size.width,
                    height: size.height,
                    unpadded_bytes_per_row,
                    padded_bytes_per_row,
                }
            }
        }

        let dimensions = BufferDimensions::new(viewport.physical_size());

        let texture_extent = wgpu::Extent3d {
            width: dimensions.width,
            height: dimensions.height,
            depth_or_array_layers: 1,
        };

        let texture = self.engine.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iced_wgpu.offscreen.source_texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.engine.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.draw(Some(background_color), &view, viewport);

        let texture = crate::color::convert(
            &self.engine.device,
            &mut encoder,
            texture,
            if graphics::color::GAMMA_CORRECTION {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
        );

        let output_buffer = self.engine.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iced_wgpu.offscreen.output_texture_buffer"),
            size: (dimensions.padded_bytes_per_row * dimensions.height as usize) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(dimensions.padded_bytes_per_row as u32),
                    rows_per_image: None,
                },
            },
            texture_extent,
        );

        self.finish();
        let index = self.engine.queue.submit([encoder.finish()]);
        self.recall();

        let slice = output_buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});

        let _ = self.engine.device.poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: None,
        });

        let mapped_buffer = slice.get_mapped_range();

        mapped_buffer
            .chunks(dimensions.padded_bytes_per_row)
            .fold(vec![], |mut acc, row| {
                acc.extend(&row[..dimensions.unpadded_bytes_per_row]);
                acc
            })
    }

    #[cfg(any(feature = "image", feature = "svg"))]
    fn image_cache(&self) -> &std::sync::Mutex<image::Cache> {
        self.image_cache
            .get_or_init(|| std::sync::Mutex::new(self.engine.create_image_cache()))
    }

    fn prepare(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        viewport: &Viewport,
        damage: Option<Rectangle<u32>>,
    ) -> bool {
        #[allow(unused_mut)]
        let mut complete = true;
        let scale_factor = viewport.scale_factor();

        self.text_viewport
            .update(&self.engine.queue, viewport.physical_size());

        let physical_bounds = Rectangle::<f32>::from(
            damage.unwrap_or_else(|| Rectangle::with_size(viewport.physical_size())),
        );

        self.layers.merge();

        for layer in self.layers.iter() {
            let clip_bounds = layer.bounds * scale_factor;

            if physical_bounds
                .intersection(&clip_bounds)
                .and_then(nudge::snap)
                .is_none()
            {
                continue;
            }

            if !layer.quads.is_empty() {
                let prepare_span = debug::prepare(debug::Primitive::Quad);

                self.quad.prepare(
                    &self.engine.quad_pipeline,
                    &self.engine.device,
                    &mut self.staging_belt,
                    encoder,
                    &layer.quads,
                    viewport.projection(),
                    scale_factor,
                );

                prepare_span.finish();
            }

            if !layer.triangles.is_empty() {
                let prepare_span = debug::prepare(debug::Primitive::Triangle);

                let pipeline = self.engine.triangle_pipeline();
                let triangle = self
                    .triangle
                    .get_or_insert_with(|| triangle::State::new(&self.engine.device, pipeline));
                triangle.prepare(
                    pipeline,
                    &self.engine.device,
                    &mut self.staging_belt,
                    encoder,
                    &layer.triangles,
                    Transformation::scale(scale_factor),
                    viewport.physical_size(),
                );

                prepare_span.finish();
            }

            if !layer.primitives.is_empty() {
                let prepare_span = debug::prepare(debug::Primitive::Shader);

                let mut primitive_storage = self
                    .engine
                    .primitive_storage
                    .write()
                    .expect("Write primitive storage");

                for instance in &layer.primitives {
                    instance.primitive.prepare(
                        &mut primitive_storage,
                        &self.engine.device,
                        &self.engine.queue,
                        self.engine.format,
                        &instance.bounds,
                        viewport,
                        encoder,
                    );
                }

                prepare_span.finish();
            }

            #[cfg(any(feature = "svg", feature = "image"))]
            if !layer.images.is_empty() {
                let prepare_span = debug::prepare(debug::Primitive::Image);

                complete &= self.image.prepare(
                    self.engine.image_pipeline(),
                    &self.engine.device,
                    &mut self.staging_belt,
                    encoder,
                    &mut self
                        .image_cache
                        .get_or_init(|| std::sync::Mutex::new(self.engine.create_image_cache()))
                        .lock()
                        .expect("Lock image cache"),
                    &layer.images,
                    viewport.projection(),
                    scale_factor,
                );

                prepare_span.finish();
            }

            if !layer.text.is_empty() {
                let prepare_span = debug::prepare(debug::Primitive::Text);

                self.text.prepare(
                    &self.engine.text_pipeline,
                    &self.engine.device,
                    &self.engine.queue,
                    &self.text_viewport,
                    encoder,
                    &layer.text,
                    layer.bounds,
                    Transformation::scale(scale_factor),
                );

                prepare_span.finish();
            }
        }
        complete
    }

    fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        clear_color: Option<Color>,
        viewport: &Viewport,
        damage: Option<(Rectangle<u32>, &raster::Clear)>,
    ) {
        use std::mem::ManuallyDrop;

        let mut render_pass =
            ManuallyDrop::new(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iced_wgpu render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match clear_color {
                            Some(background_color) => wgpu::LoadOp::Clear({
                                let [r, g, b, a] =
                                    graphics::color::pack(background_color).components();

                                wgpu::Color {
                                    r: f64::from(r * a),
                                    g: f64::from(g * a),
                                    b: f64::from(b * a),
                                    a: f64::from(a),
                                }
                            }),
                            None => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));

        if let Some((bounds, clear)) = damage {
            render_pass.set_scissor_rect(bounds.x, bounds.y, bounds.width, bounds.height);
            self.engine.raster_pipeline().clear(clear, &mut render_pass);
        }

        let mut quad_layer = 0;
        let mut mesh_layer = 0;
        let mut text_layer = 0;

        #[cfg(any(feature = "svg", feature = "image"))]
        let mut image_layer = 0;

        let scale_factor = viewport.scale_factor();
        let physical_bounds = Rectangle::<f32>::from(
            damage
                .map(|(bounds, _)| bounds)
                .unwrap_or_else(|| Rectangle::with_size(viewport.physical_size())),
        );

        let scale = Transformation::scale(scale_factor);

        for layer in self.layers.iter() {
            let Some(physical_bounds) =
                physical_bounds.intersection(&(layer.bounds * scale_factor))
            else {
                continue;
            };

            if nudge::snap(physical_bounds).is_none() {
                continue;
            }

            let Some(scissor_rect) = nudge::snap(physical_bounds) else {
                continue;
            };

            if !layer.quads.is_empty() {
                let render_span = debug::render(debug::Primitive::Quad);
                self.quad.render(
                    &self.engine.quad_pipeline,
                    quad_layer,
                    scissor_rect,
                    &layer.quads,
                    &mut render_pass,
                );
                render_span.finish();

                quad_layer += 1;
            }

            if !layer.triangles.is_empty() {
                let _ = ManuallyDrop::into_inner(render_pass);

                let render_span = debug::render(debug::Primitive::Triangle);
                mesh_layer += self.triangle.as_mut().expect("prepared meshes").render(
                    self.engine.triangle_pipeline(),
                    encoder,
                    frame,
                    mesh_layer,
                    &layer.triangles,
                    physical_bounds,
                    scale,
                );
                render_span.finish();

                render_pass =
                    ManuallyDrop::new(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("iced_wgpu render pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: frame,
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
                    }));
            }

            if !layer.primitives.is_empty() {
                let render_span = debug::render(debug::Primitive::Shader);

                let primitive_storage = self
                    .engine
                    .primitive_storage
                    .read()
                    .expect("Read primitive storage");

                let mut need_render = Vec::new();

                for instance in &layer.primitives {
                    let bounds = instance.bounds * scale;

                    if let Some(clip_bounds) = (instance.bounds * scale)
                        .intersection(&physical_bounds)
                        .and_then(nudge::snap)
                    {
                        render_pass.set_viewport(
                            bounds.x,
                            bounds.y,
                            bounds.width,
                            bounds.height,
                            0.0,
                            1.0,
                        );

                        render_pass.set_scissor_rect(
                            clip_bounds.x,
                            clip_bounds.y,
                            clip_bounds.width,
                            clip_bounds.height,
                        );

                        let drawn = instance
                            .primitive
                            .draw(&primitive_storage, &mut render_pass);

                        if !drawn {
                            need_render.push((instance, clip_bounds));
                        }
                    }
                }

                render_pass.set_viewport(
                    0.0,
                    0.0,
                    viewport.physical_width() as f32,
                    viewport.physical_height() as f32,
                    0.0,
                    1.0,
                );

                render_pass.set_scissor_rect(
                    0,
                    0,
                    viewport.physical_width(),
                    viewport.physical_height(),
                );

                if !need_render.is_empty() {
                    let _ = ManuallyDrop::into_inner(render_pass);

                    for (instance, clip_bounds) in need_render {
                        instance
                            .primitive
                            .render(&primitive_storage, encoder, frame, &clip_bounds);
                    }

                    render_pass =
                        ManuallyDrop::new(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("iced_wgpu render pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: frame,
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
                        }));
                }

                render_span.finish();
            }

            if !layer.rasters.is_empty() {
                render_pass.set_scissor_rect(
                    scissor_rect.x,
                    scissor_rect.y,
                    scissor_rect.width,
                    scissor_rect.height,
                );
                for instance in &layer.rasters {
                    self.engine.raster_pipeline().draw(
                        &instance.target,
                        instance.bounds * scale_factor,
                        &mut render_pass,
                    );
                }
                render_pass.set_viewport(
                    0.0,
                    0.0,
                    viewport.physical_width() as f32,
                    viewport.physical_height() as f32,
                    0.0,
                    1.0,
                );
            }

            #[cfg(any(feature = "svg", feature = "image"))]
            if !layer.images.is_empty() {
                let render_span = debug::render(debug::Primitive::Image);
                self.image.render(
                    self.engine.image_pipeline(),
                    image_layer,
                    scissor_rect,
                    &mut render_pass,
                );
                render_span.finish();

                image_layer += 1;
            }

            if !layer.text.is_empty() {
                let render_span = debug::render(debug::Primitive::Text);
                text_layer += self.text.render(
                    &self.engine.text_pipeline,
                    &self.text_viewport,
                    text_layer,
                    &layer.text,
                    scissor_rect,
                    &mut render_pass,
                );
                render_span.finish();
            }
        }

        let _ = ManuallyDrop::into_inner(render_pass);

        debug::layers_rendered(|| {
            self.layers
                .iter()
                .filter(|layer| {
                    !layer.is_empty()
                        && physical_bounds
                            .intersection(&(layer.bounds * scale_factor))
                            .is_some_and(|viewport| nudge::snap(viewport).is_some())
                })
                .count()
        });
    }

    fn offscreen_passes(&self) -> u32 {
        let mut passes = 0;

        for layer in self.layers.iter().filter(|layer| !layer.is_empty()) {
            let mut layer_passes = 0;

            if !layer.quads.is_empty()
                || !layer.primitives.is_empty()
                || !layer.images.is_empty()
                || !layer.text.is_empty()
            {
                layer_passes += 1;
            }

            if !layer.triangles.is_empty() {
                layer_passes += 1;
            }

            passes += layer_passes.max(1);
        }

        passes.max(1)
    }

    /// Prepares currently mapped buffers for use in a submission.
    ///
    /// Usually, this method is only needed if you are calling [`Renderer::draw`] directly,
    /// instead of relying on [`Renderer::present`].
    ///
    /// You must call this method _before_ submitting the resulting [`wgpu::CommandEncoder`]
    /// of [`Renderer::draw`] to a [`wgpu::Queue`].
    pub fn finish(&mut self) {
        self.staging_belt.finish();
        for entry in self.raster_cache.entries.values_mut() {
            entry.child.finish();
        }
    }

    /// Recalls all of the closed buffers back to be reused.
    ///
    /// Usually, this method is only needed if you are calling [`Renderer::draw`] directly,
    /// instead of relying on [`Renderer::present`] to a [`wgpu::Queue`].
    ///
    /// You must call this method _after_ submitting the resulting [`wgpu::CommandEncoder`]
    /// of [`Renderer::draw`] to a [`wgpu::Queue`].
    pub fn recall(&mut self) {
        self.staging_belt.recall();
        for entry in self.raster_cache.entries.values_mut() {
            entry.child.recall();
        }
    }
}

impl core::Renderer for Renderer {
    fn start_cached_layer(&mut self, cache: &renderer::Cache, key: u64, bounds: Rectangle) -> bool {
        let mut state = std::mem::take(&mut self.raster_cache);
        let record = state.begin(self, cache, key, bounds);
        self.raster_cache = state;
        record
    }

    fn end_cached_layer(&mut self) {
        let mut state = std::mem::take(&mut self.raster_cache);
        state.end(self);
        self.raster_cache = state;
    }

    fn start_layer(&mut self, bounds: Rectangle) {
        self.layers.push_clip(bounds);
    }

    fn end_layer(&mut self) {
        self.layers.pop_clip();
    }

    fn start_transformation(&mut self, transformation: Transformation) {
        self.layers.push_transformation(transformation);
    }

    fn end_transformation(&mut self) {
        self.layers.pop_transformation();
    }

    fn fill_quad(&mut self, quad: core::renderer::Quad, background: impl Into<Background>) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_quad(quad, background.into(), transformation);
    }

    fn allocate_image(
        &mut self,
        _handle: &core::image::Handle,
        _callback: impl FnOnce(Result<core::image::Allocation, core::image::Error>) + Send + 'static,
    ) {
        #[cfg(feature = "image")]
        self.image_cache()
            .lock()
            .expect("Lock image cache")
            .allocate_image(_handle, _callback);
    }

    fn hint(&mut self, scale_factor: f32) {
        self.scale_factor = Some(scale_factor);
    }

    fn scale_factor(&self) -> Option<f32> {
        Some(self.scale_factor? * self.layers.transformation().scale_factor())
    }

    fn tick(&mut self) {
        #[cfg(feature = "image")]
        if let Some(cache) = self.image_cache.get() {
            cache.lock().expect("Lock image cache").receive();
        }
    }

    fn reset(&mut self, new_bounds: Rectangle) {
        self.layers.reset(new_bounds);
        self.raster_cache.reset();
    }
}

impl core::text::Renderer for Renderer {
    type Font = Font;
    type Paragraph = Paragraph;
    type Editor = Editor;

    const ICON_FONT: Font = Font::new("Iced-Icons");
    const CHECKMARK_ICON: char = '\u{f00c}';
    const ARROW_DOWN_ICON: char = '\u{e800}';
    const ICED_LOGO: char = '\u{e801}';
    const SCROLL_UP_ICON: char = '\u{e802}';
    const SCROLL_DOWN_ICON: char = '\u{e803}';
    const SCROLL_LEFT_ICON: char = '\u{e804}';
    const SCROLL_RIGHT_ICON: char = '\u{e805}';

    fn default_font(&self) -> Self::Font {
        self.settings.default_font
    }

    fn default_size(&self) -> Pixels {
        self.settings.default_text_size
    }

    fn fill_paragraph(
        &mut self,
        text: &Self::Paragraph,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();

        layer.draw_paragraph(text, position, color, clip_bounds, transformation);
    }

    fn fill_editor(
        &mut self,
        editor: &Self::Editor,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_editor(editor, position, color, clip_bounds, transformation);
    }

    fn fill_text(
        &mut self,
        text: core::Text,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_text(text, position, color, clip_bounds, transformation);
    }
}

impl graphics::text::Renderer for Renderer {
    fn fill_raw(&mut self, raw: graphics::text::Raw) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_text_raw(raw, transformation);
    }
}

#[cfg(feature = "image")]
impl core::image::Renderer for Renderer {
    type Handle = core::image::Handle;

    fn load_image(
        &self,
        handle: &Self::Handle,
    ) -> Result<core::image::Allocation, core::image::Error> {
        self.image_cache()
            .lock()
            .expect("Lock image cache")
            .load_image(&self.engine.device, &self.engine.queue, handle)
    }

    fn measure_image(&self, handle: &Self::Handle) -> Option<core::Size<u32>> {
        self.image_cache()
            .lock()
            .expect("Lock image cache")
            .measure_image(handle)
    }

    fn draw_image(&mut self, image: core::Image, bounds: Rectangle, clip_bounds: Rectangle) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_raster(image, bounds, clip_bounds, transformation);
    }
}

#[cfg(feature = "svg")]
impl core::svg::Renderer for Renderer {
    fn measure_svg(&self, handle: &core::svg::Handle) -> core::Size<u32> {
        self.image_cache()
            .lock()
            .expect("Lock image cache")
            .measure_svg(handle)
    }

    fn draw_svg(&mut self, svg: core::Svg, bounds: Rectangle, clip_bounds: Rectangle) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_svg(svg, bounds, clip_bounds, transformation);
    }
}

impl graphics::mesh::Renderer for Renderer {
    fn draw_mesh(&mut self, mesh: graphics::Mesh) {
        debug_assert!(
            !mesh.indices().is_empty(),
            "Mesh must not have empty indices"
        );

        debug_assert!(
            mesh.indices().len().is_multiple_of(3),
            "Mesh indices length must be a multiple of 3"
        );

        let (layer, transformation) = self.layers.current_mut();
        layer.draw_mesh(mesh, transformation);
    }

    fn draw_mesh_cache(&mut self, cache: mesh::Cache) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_mesh_cache(cache, transformation);
    }
}

#[cfg(feature = "geometry")]
impl graphics::geometry::Renderer for Renderer {
    type Geometry = Geometry;
    type Frame = geometry::Frame;

    fn new_frame(&self, bounds: Rectangle) -> Self::Frame {
        geometry::Frame::new(bounds)
    }

    fn draw_geometry(&mut self, geometry: Self::Geometry) {
        let (layer, transformation) = self.layers.current_mut();

        match geometry {
            Geometry::Live {
                meshes,
                images,
                text,
            } => {
                layer.draw_mesh_group(meshes, transformation);

                for image in images {
                    layer.draw_image(image, transformation);
                }

                layer.draw_text_group(text, transformation);
            }
            Geometry::Cached(cache) => {
                if let Some(meshes) = cache.meshes {
                    layer.draw_mesh_cache(meshes, transformation);
                }

                if let Some(images) = cache.images {
                    for image in images.iter().cloned() {
                        layer.draw_image(image, transformation);
                    }
                }

                if let Some(text) = cache.text {
                    layer.draw_text_cache(text, transformation);
                }
            }
        }
    }
}

impl primitive::Renderer for Renderer {
    fn draw_primitive(&mut self, bounds: Rectangle, primitive: impl Primitive) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_primitive(bounds, primitive, transformation);
    }
}

impl graphics::compositor::Default for crate::Renderer {
    type Compositor = window::Compositor;
}

impl renderer::Headless for Renderer {
    async fn new(settings: renderer::Settings, backend: Option<&str>) -> Option<Self> {
        if backend.is_some_and(|backend| backend != "wgpu") {
            return None;
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            flags: wgpu::InstanceFlags::empty(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .ok()?;

        let immediate_size = crate::engine::immediate_size(&adapter);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("iced_wgpu [headless]"),
                required_features: if immediate_size > 0 {
                    wgpu::Features::IMMEDIATES
                } else {
                    wgpu::Features::empty()
                },
                required_limits: wgpu::Limits {
                    max_bind_groups: 2,
                    max_immediate_size: immediate_size,
                    ..wgpu::Limits::default()
                },
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            })
            .await
            .ok()?;

        let engine = Engine::new(
            &adapter,
            device,
            queue,
            if graphics::color::GAMMA_CORRECTION {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            Some(graphics::Antialiasing::MSAAx4),
            Shell::headless(),
        );

        Some(Self::new(engine, settings))
    }

    fn name(&self) -> String {
        "wgpu".to_owned()
    }

    fn screenshot(
        &mut self,
        size: Size<u32>,
        scale_factor: f32,
        background_color: Color,
    ) -> Vec<u8> {
        self.screenshot(
            &Viewport::with_physical_size(size, scale_factor),
            background_color,
        )
    }
}
