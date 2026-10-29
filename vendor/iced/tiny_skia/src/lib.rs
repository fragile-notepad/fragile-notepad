#![allow(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
pub mod window;

mod engine;
mod layer;
mod primitive;
mod text;
mod trace;

#[cfg(feature = "image")]
mod raster;

#[cfg(feature = "svg")]
mod vector;

#[cfg(feature = "geometry")]
pub mod geometry;

use iced_debug as debug;
pub use iced_graphics as graphics;
pub use iced_graphics::core;

pub use layer::Layer;
pub use primitive::Primitive;

#[cfg(feature = "geometry")]
pub use geometry::Geometry;

use crate::core::renderer;
use crate::core::{Background, Color, Font, Pixels, Point, Rectangle, Size, Transformation};
use crate::engine::{ClipMaskStats, Engine};
use crate::graphics::Viewport;
use crate::graphics::compositor;
use crate::graphics::text::{Editor, Paragraph};
use crate::text::PipelineStats;
/// A [`tiny-skia`] graphics renderer for [`iced`].
///
/// [`tiny-skia`]: https://github.com/RazrFalcon/tiny-skia
/// [`iced`]: https://github.com/iced-rs/iced
#[derive(Debug)]
pub struct Renderer {
    settings: renderer::Settings,
    layers: layer::Stack,
    engine: Engine, // TODO: Shared engine
    opacity_recordings: Vec<OpacityRecording>,
    opacity_pixels: Vec<OpacityPixels>,
}

#[derive(Debug)]
struct OpacityRecording {
    parent: layer::Stack,
    bounds: Rectangle,
    opacity: f32,
}

#[derive(Debug)]
struct OpacityPixels {
    pixels: tiny_skia::Pixmap,
    mask: tiny_skia::Mask,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DrawStats {
    pub damage_regions: usize,
    pub layer_visits: usize,
    pub quads: usize,
    pub primitives: usize,
    pub images: usize,
    pub text_groups: usize,
    pub text_items: usize,
    pub clip_mask: ClipMaskStats,
    pub text_pipeline: PipelineStats,
}

impl Renderer {
    pub fn new(settings: renderer::Settings) -> Self {
        Self {
            settings,
            layers: layer::Stack::new(),
            engine: Engine::new(),
            opacity_recordings: Vec::new(),
            opacity_pixels: Vec::new(),
        }
    }

    pub fn layers(&mut self) -> &[Layer] {
        self.layers.flush();
        self.layers.as_slice()
    }

    pub fn draw(
        &mut self,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut tiny_skia::Mask,
        viewport: &Viewport,
        damage: &[Rectangle],
        background_color: Color,
    ) {
        let _ = self.draw_with_scroll(pixels, clip_mask, viewport, damage, background_color, &[]);
    }

    pub(crate) fn draw_with_scroll(
        &mut self,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut tiny_skia::Mask,
        viewport: &Viewport,
        damage: &[Rectangle],
        background_color: Color,
        scrolls: &[layer::Scroll],
    ) -> DrawStats {
        let scale_factor = viewport.scale_factor();
        self.layers.flush();
        let text_stats_before = self.engine.text_stats();
        let mut stats = DrawStats::default();

        for scroll in scrolls {
            copy_scroll_region(pixels, *scroll, scale_factor);
        }

        let damage_regions = coalesce_overlapping_regions(
            damage
                .iter()
                .map(|bounds| pixel_aligned(*bounds * scale_factor))
                .filter(|bounds| bounds.width >= 1.0 && bounds.height >= 1.0),
        );
        stats.damage_regions = damage_regions.len();

        for (damage_index, damage_bounds) in damage_regions.iter().enumerate() {
            if trace::enabled() {
                trace::event(
                    "tiny_skia_draw_region",
                    0,
                    format_args!(
                        "index={damage_index} bounds={}",
                        format_rect(*damage_bounds)
                    ),
                );
            }

            pixels.fill_rect(
                tiny_skia::Rect::from_xywh(
                    damage_bounds.x,
                    damage_bounds.y,
                    damage_bounds.width,
                    damage_bounds.height,
                )
                .expect("Create damage rectangle"),
                &tiny_skia::Paint {
                    shader: tiny_skia::Shader::SolidColor(engine::into_color(background_color)),
                    anti_alias: false,
                    blend_mode: tiny_skia::BlendMode::Source,
                    ..Default::default()
                },
                tiny_skia::Transform::identity(),
                None,
            );
        }

        if damage_regions.is_empty() {
            self.engine.trim();
            stats.clip_mask = ClipMaskStats::default();
            stats.text_pipeline = self.engine.text_stats().saturating_delta(text_stats_before);
            return stats;
        }

        let mut clip_mask = engine::ClipMask::new(clip_mask);

        draw_layers(
            &mut self.engine,
            self.layers.as_slice(),
            pixels,
            &mut clip_mask,
            &damage_regions,
            scale_factor,
            &mut self.opacity_pixels,
            &mut stats,
        );
        if !self
            .layers
            .as_slice()
            .iter()
            .any(|layer| layer.opacity.is_some())
        {
            self.opacity_pixels.clear();
        }
        self.engine.trim();
        stats.clip_mask = clip_mask.stats();
        stats.text_pipeline = self.engine.text_stats().saturating_delta(text_stats_before);
        trace::flush();
        stats
    }
}

fn draw_layers(
    engine: &mut Engine,
    layers: &[Layer],
    pixels: &mut tiny_skia::PixmapMut<'_>,
    clip_mask: &mut engine::ClipMask<'_>,
    damage_regions: &[Rectangle],
    scale_factor: f32,
    scratch: &mut Vec<OpacityPixels>,
    stats: &mut DrawStats,
) {
    let scale = Transformation::scale(scale_factor);
    let damage_bounds = damage_regions
        .iter()
        .copied()
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Rectangle::default());
    for (layer_index, layer) in layers.iter().enumerate() {
        let layer_physical_bounds = layer.bounds * scale_factor;
        let layer_regions = collect_clip_regions(&damage_regions, layer_physical_bounds);

        if layer_regions.is_empty() {
            if trace::enabled() {
                trace::event(
                    "tiny_skia_layer_skip",
                    0,
                    format_args!(
                        "layer={layer_index} reason=no_intersection damage={} layer={}",
                        format_rect(damage_bounds),
                        format_rect(layer_physical_bounds)
                    ),
                );
            }
            continue;
        };

        let layer_bounds = layer_regions
            .iter()
            .copied()
            .reduce(|a, b| a.union(&b))
            .unwrap_or(damage_bounds);
        let multi_region_layer = layer_regions.len() > 1;
        stats.layer_visits += 1;

        if trace::enabled() {
            trace::event(
                "tiny_skia_layer_visit",
                0,
                format_args!(
                    "layer={layer_index} layer_bounds={} clipped={} regions={}",
                    format_rect(layer_physical_bounds),
                    format_rect(layer_bounds),
                    layer_regions.len()
                ),
            );
        }

        if let Some(group) = &layer.opacity {
            if group.opacity <= 0.0 {
                continue;
            }
            let regions = collect_clip_regions(&layer_regions, group.bounds * scale_factor);
            if regions.is_empty() {
                continue;
            }
            let mut offscreen = scratch
                .pop()
                .filter(|buffer| {
                    buffer.pixels.width() == pixels.width()
                        && buffer.pixels.height() == pixels.height()
                })
                .unwrap_or_else(|| OpacityPixels {
                    pixels: tiny_skia::Pixmap::new(pixels.width(), pixels.height())
                        .expect("Create opacity pixels"),
                    mask: tiny_skia::Mask::new(pixels.width(), pixels.height())
                        .expect("Create opacity clip mask"),
                });
            offscreen.pixels.fill(tiny_skia::Color::TRANSPARENT);
            let mut inner_mask = engine::ClipMask::new(&mut offscreen.mask);
            draw_layers(
                engine,
                &group.layers,
                &mut offscreen.pixels.as_mut(),
                &mut inner_mask,
                &regions,
                scale_factor,
                scratch,
                stats,
            );
            for region in regions {
                pixels.draw_pixmap(
                    0,
                    0,
                    offscreen.pixels.as_ref(),
                    &tiny_skia::PixmapPaint {
                        opacity: group.opacity,
                        ..Default::default()
                    },
                    tiny_skia::Transform::identity(),
                    Some(clip_mask.set(region)),
                );
            }
            scratch.push(offscreen);
            continue;
        }

        if !layer.quads.is_empty() {
            let render_span = debug::render(debug::Primitive::Quad);
            for (quad_index, (quad, background)) in layer.quads.iter().enumerate() {
                let visible_bounds = quad_visible_bounds(quad) * scale_factor;
                if !visible_bounds.intersects(&layer_bounds) {
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_quad_skip",
                            0,
                            format_args!(
                                "layer={layer_index} quad={quad_index} reason=no_intersection visible={} clip={}",
                                format_rect(visible_bounds),
                                format_rect(layer_bounds)
                            ),
                        );
                    }
                    continue;
                }

                if multi_region_layer {
                    for_each_clip_region(
                        &layer_regions,
                        visible_bounds,
                        |clip_index, clip_bounds| {
                            stats.quads += 1;
                            if trace::enabled() {
                                trace::event(
                                    "tiny_skia_quad_draw",
                                    0,
                                    format_args!(
                                        "clip_index={clip_index} layer={layer_index} quad={quad_index} bounds={} visible={} clip={}",
                                        format_rect(quad.bounds * scale_factor),
                                        format_rect(visible_bounds),
                                        format_rect(clip_bounds)
                                    ),
                                );
                            }
                            engine.draw_quad(
                                quad,
                                background,
                                scale,
                                pixels,
                                clip_mask,
                                clip_bounds,
                            );
                        },
                    );
                } else {
                    stats.quads += 1;
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_quad_draw",
                            0,
                            format_args!(
                                "layer={layer_index} quad={quad_index} bounds={} visible={} clip={}",
                                format_rect(quad.bounds * scale_factor),
                                format_rect(visible_bounds),
                                format_rect(layer_bounds)
                            ),
                        );
                    }
                    engine.draw_quad(quad, background, scale, pixels, clip_mask, layer_bounds);
                }
            }
            render_span.finish();
        }

        if !layer.primitives.is_empty() {
            let render_span = debug::render(debug::Primitive::Triangle);

            for group in &layer.primitives {
                let group_transformation = scale * group.transformation();
                let Some(group_bounds) =
                    (group.clip_bounds() * scale_factor).intersection(&layer_bounds)
                else {
                    continue;
                };

                for primitive in group.as_slice() {
                    let primitive_bounds = primitive.visible_bounds() * group_transformation;

                    if !primitive_bounds.intersects(&group_bounds) {
                        continue;
                    }

                    let Some(visible_bounds) = primitive_bounds.intersection(&group_bounds) else {
                        continue;
                    };

                    if multi_region_layer {
                        for_each_clip_region(&layer_regions, visible_bounds, |_, clip_bounds| {
                            stats.primitives += 1;
                            engine.draw_primitive(
                                primitive,
                                group_transformation,
                                pixels,
                                clip_mask,
                                clip_bounds,
                            );
                        });
                    } else {
                        stats.primitives += 1;
                        engine.draw_primitive(
                            primitive,
                            group_transformation,
                            pixels,
                            clip_mask,
                            group_bounds,
                        );
                    }
                }
            }

            render_span.finish();
        }

        if !layer.images.is_empty() {
            let render_span = debug::render(debug::Primitive::Image);

            for image in &layer.images {
                let visible_bounds = image.bounds() * scale;
                if !visible_bounds.intersects(&layer_bounds) {
                    continue;
                }

                if multi_region_layer {
                    for_each_clip_region(&layer_regions, visible_bounds, |_, clip_bounds| {
                        stats.images += 1;
                        engine.draw_image(image, scale, pixels, clip_mask, clip_bounds);
                    });
                } else {
                    stats.images += 1;
                    engine.draw_image(image, scale, pixels, clip_mask, layer_bounds);
                }
            }

            render_span.finish();
        }

        if !layer.text.is_empty() {
            let render_span = debug::render(debug::Primitive::Image);
            stats.text_groups += layer.text.len();

            for (group_index, group) in layer.text.iter().enumerate() {
                let group_transformation = group.transformation();
                let scaled_group_transformation = scale * group_transformation;
                let Some(group_bounds) =
                    (group.clip_bounds() * scale_factor).intersection(&layer_bounds)
                else {
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_text_group_skip",
                            0,
                            format_args!(
                                "layer={layer_index} group={group_index} reason=no_group_intersection group_clip={} layer_clip={}",
                                format_rect(group.clip_bounds() * scale_factor),
                                format_rect(layer_bounds)
                            ),
                        );
                    }
                    continue;
                };

                for (text_index, text) in group.as_slice().iter().enumerate() {
                    let Some(text_bounds) = text
                        .visible_bounds()
                        .map(|bounds| bounds * group_transformation * scale_factor)
                    else {
                        if trace::enabled() {
                            trace::event(
                                "tiny_skia_text_skip",
                                0,
                                format_args!(
                                    "layer={layer_index} group={group_index} text={text_index} reason=no_visible_bounds group_clip={}",
                                    format_rect(group_bounds)
                                ),
                            );
                        };
                        continue;
                    };

                    if !text_bounds.intersects(&group_bounds) {
                        if trace::enabled() {
                            trace::event(
                                "tiny_skia_text_skip",
                                0,
                                format_args!(
                                    "layer={layer_index} group={group_index} text={text_index} reason=no_intersection visible={} clip={}",
                                    format_rect(text_bounds),
                                    format_rect(group_bounds)
                                ),
                            );
                        }
                        continue;
                    }

                    let Some(visible_bounds) = text_bounds.intersection(&group_bounds) else {
                        continue;
                    };

                    if multi_region_layer {
                        for_each_clip_region(
                            &layer_regions,
                            visible_bounds,
                            |clip_index, clip_bounds| {
                                stats.text_items += 1;
                                if trace::enabled() {
                                    trace::event(
                                        "tiny_skia_text_draw",
                                        0,
                                        format_args!(
                                            "clip_index={clip_index} layer={layer_index} group={group_index} text={text_index} visible={} clip={}",
                                            format_rect(text_bounds),
                                            format_rect(clip_bounds)
                                        ),
                                    );
                                }
                                engine.draw_text(
                                    text,
                                    scaled_group_transformation,
                                    pixels,
                                    clip_mask,
                                    clip_bounds,
                                );
                            },
                        );
                    } else {
                        stats.text_items += 1;
                        if trace::enabled() {
                            trace::event(
                                "tiny_skia_text_draw",
                                0,
                                format_args!(
                                    "layer={layer_index} group={group_index} text={text_index} visible={} clip={}",
                                    format_rect(text_bounds),
                                    format_rect(group_bounds)
                                ),
                            );
                        }
                        engine.draw_text(
                            text,
                            scaled_group_transformation,
                            pixels,
                            clip_mask,
                            group_bounds,
                        );
                    }
                }
            }

            render_span.finish();
        }
    }
}

fn collect_clip_regions(regions: &[Rectangle], bounds: Rectangle) -> Vec<Rectangle> {
    coalesce_overlapping_regions(
        regions
            .iter()
            .filter_map(|region| region.intersection(&bounds))
            .filter(|region| region.width >= 1.0 && region.height >= 1.0),
    )
}

fn coalesce_overlapping_regions(regions: impl IntoIterator<Item = Rectangle>) -> Vec<Rectangle> {
    let mut coalesced: Vec<Rectangle> = Vec::new();

    for region in regions {
        let mut current = region;
        let mut index = 0;

        while index < coalesced.len() {
            if overlaps(current, coalesced[index]) {
                current = current.union(&coalesced.swap_remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }

        coalesced.push(current);
    }

    coalesced
}

fn overlaps(a: Rectangle, b: Rectangle) -> bool {
    a.intersection(&b)
        .is_some_and(|region| region.width >= 1.0 && region.height >= 1.0)
}

fn for_each_clip_region(
    regions: &[Rectangle],
    bounds: Rectangle,
    mut f: impl FnMut(usize, Rectangle),
) {
    for (index, region) in regions.iter().enumerate() {
        let Some(region) = region.intersection(&bounds) else {
            continue;
        };

        if region.width >= 1.0 && region.height >= 1.0 {
            f(index, region);
        }
    }
}

fn quad_visible_bounds(quad: &renderer::Quad) -> Rectangle {
    if quad.shadow.color.a > 0.0 {
        quad.bounds.expand(
            quad.shadow.offset.x.abs().max(quad.shadow.offset.y.abs()) + quad.shadow.blur_radius,
        )
    } else {
        quad.bounds
    }
}

fn format_rect(rect: Rectangle) -> String {
    format!(
        "[{:.1},{:.1},{:.1},{:.1}]",
        rect.x, rect.y, rect.width, rect.height
    )
}

fn pixel_aligned(bounds: Rectangle) -> Rectangle {
    let left = bounds.x.floor();
    let top = bounds.y.floor();
    let right = (bounds.x + bounds.width).ceil();
    let bottom = (bounds.y + bounds.height).ceil();

    Rectangle {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

fn copy_scroll_region(
    pixels: &mut tiny_skia::PixmapMut<'_>,
    scroll: layer::Scroll,
    scale_factor: f32,
) {
    let bounds = scroll.bounds * scale_factor;
    let delta_x = (scroll.delta.x * scale_factor).round() as i32;
    let delta_y = (scroll.delta.y * scale_factor).round() as i32;

    if delta_x != 0 || delta_y == 0 {
        return;
    }

    let width = bounds.width.round().max(0.0) as u32;
    let height = bounds.height.round().max(0.0) as u32;
    let x = bounds.x.round() as i32;
    let y = bounds.y.round() as i32;
    let copy_height = height.saturating_sub(delta_y.unsigned_abs());

    if width == 0 || copy_height == 0 {
        return;
    }

    let source_y = if delta_y < 0 { y - delta_y } else { y };
    let target_y = if delta_y < 0 { y } else { y + delta_y };
    let Some(source) = tiny_skia::IntRect::from_xywh(x, source_y, width, copy_height) else {
        return;
    };

    // A window with an opaque background can scroll in place. Keep the original
    // SourceOver path for translucent pixels and clipped rectangles: a raw copy
    // would change their composition over the destination.
    if x >= 0
        && source_y >= 0
        && target_y >= 0
        && x as u64 + width as u64 <= pixels.width() as u64
        && source_y as u64 + copy_height as u64 <= pixels.height() as u64
        && target_y as u64 + copy_height as u64 <= pixels.height() as u64
    {
        let stride = pixels.width() as usize * 4;
        let row_bytes = width as usize * 4;
        let source_start = source_y as usize * stride + x as usize * 4;
        let target_start = target_y as usize * stride + x as usize * 4;
        let opaque = (0..copy_height as usize).all(|row| {
            pixels.as_ref().data()
                [source_start + row * stride..source_start + row * stride + row_bytes]
                .chunks_exact(4)
                .all(|pixel| pixel[3] == 255)
        });
        if opaque {
            let data = pixels.data_mut();
            if x == 0 && row_bytes == stride {
                data.copy_within(
                    source_start..source_start + copy_height as usize * stride,
                    target_start,
                );
            } else if target_start < source_start {
                for row in 0..copy_height as usize {
                    let start = source_start + row * stride;
                    data.copy_within(start..start + row_bytes, target_start + row * stride);
                }
            } else {
                for row in (0..copy_height as usize).rev() {
                    let start = source_start + row * stride;
                    data.copy_within(start..start + row_bytes, target_start + row * stride);
                }
            }
            return;
        }
    }

    let Some(snapshot) = pixels.as_ref().clone_rect(source) else {
        return;
    };

    pixels.draw_pixmap(
        x,
        target_y,
        snapshot.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
}

impl core::Renderer for Renderer {
    fn start_layer(&mut self, bounds: Rectangle) {
        self.layers.push_clip(bounds);
    }

    fn end_layer(&mut self) {
        self.layers.pop_clip();
    }

    fn start_opacity_layer(&mut self, bounds: Rectangle, opacity: f32) {
        self.layers.flush();
        let transformation = self.layers.transformation();
        let parent_bounds = self.layers.current_mut().0.bounds;
        let bounds = (bounds * transformation)
            .intersection(&parent_bounds)
            .unwrap_or_default();
        let mut layers = layer::Stack::new();
        layers.reset(bounds);
        layers.push_transformation(transformation);
        self.opacity_recordings.push(OpacityRecording {
            parent: std::mem::replace(&mut self.layers, layers),
            bounds,
            opacity: opacity.clamp(0.0, 1.0),
        });
    }

    fn end_opacity_layer(&mut self) {
        self.layers.flush();
        let recording = self.opacity_recordings.pop().expect("End opacity layer");
        let group = layer::OpacityGroup {
            layers: self.layers.as_slice().to_vec().into(),
            bounds: recording.bounds,
            opacity: recording.opacity,
        };
        self.layers = recording.parent;
        self.layers.next_layer();
        self.layers.current_mut().0.opacity = Some(group);
        self.layers.next_layer();
    }

    fn start_transformation(&mut self, transformation: Transformation) {
        self.layers.push_transformation(transformation);
    }

    fn end_transformation(&mut self) {
        self.layers.pop_transformation();
    }

    fn fill_quad(&mut self, quad: renderer::Quad, background: impl Into<Background>) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_quad(quad, background.into(), transformation);
    }

    fn allocate_image(
        &mut self,
        _handle: &core::image::Handle,
        callback: impl FnOnce(Result<core::image::Allocation, core::image::Error>) + Send + 'static,
    ) {
        #[cfg(feature = "image")]
        #[allow(unsafe_code)]
        // TODO: Concurrency
        callback(self.engine.raster_pipeline.load(_handle));

        #[cfg(not(feature = "image"))]
        callback(Err(core::image::Error::Unsupported));
    }

    fn hint(&mut self, _scale_factor: f32) {
        // TODO: No hinting supported
        // We'll replace `tiny-skia` with `vello_cpu` soon
    }

    fn scale_factor(&self) -> Option<f32> {
        None
    }

    fn reset(&mut self, new_bounds: Rectangle) {
        self.opacity_recordings.clear();
        self.layers.reset(new_bounds);
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
                primitives,
                images,
                text,
                clip_bounds,
            } => {
                layer.draw_primitive_group(primitives, clip_bounds, transformation);

                for image in images {
                    layer.draw_image(image, transformation);
                }

                layer.draw_text_group(text, clip_bounds, transformation);
            }
            Geometry::Cache(cache) => {
                layer.draw_primitive_cache(cache.primitives, cache.clip_bounds, transformation);

                for image in cache.images.iter() {
                    layer.draw_image(image.clone(), transformation);
                }

                layer.draw_text_cache(cache.text, cache.clip_bounds, transformation);
            }
        }
    }
}

impl graphics::mesh::Renderer for Renderer {
    fn draw_mesh(&mut self, _mesh: graphics::Mesh) {
        log::warn!("iced_tiny_skia does not support drawing meshes");
    }

    fn draw_mesh_cache(&mut self, _cache: iced_graphics::mesh::Cache) {
        log::warn!("iced_tiny_skia does not support drawing meshes");
    }
}

#[cfg(feature = "image")]
impl core::image::Renderer for Renderer {
    type Handle = core::image::Handle;

    fn load_image(
        &self,
        handle: &Self::Handle,
    ) -> Result<core::image::Allocation, core::image::Error> {
        self.engine.raster_pipeline.load(handle)
    }

    fn measure_image(&self, handle: &Self::Handle) -> Option<crate::core::Size<u32>> {
        self.engine.raster_pipeline.dimensions(handle)
    }

    fn draw_image(&mut self, image: core::Image, bounds: Rectangle, clip_bounds: Rectangle) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_raster(image, bounds, clip_bounds, transformation);
    }
}

#[cfg(feature = "svg")]
impl core::svg::Renderer for Renderer {
    fn measure_svg(&self, handle: &core::svg::Handle) -> crate::core::Size<u32> {
        self.engine.vector_pipeline.viewport_dimensions(handle)
    }

    fn draw_svg(&mut self, svg: core::Svg, bounds: Rectangle, clip_bounds: Rectangle) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_svg(svg, bounds, clip_bounds, transformation);
    }
}

impl compositor::Default for Renderer {
    type Compositor = window::Compositor;
}

impl renderer::Headless for Renderer {
    async fn new(settings: renderer::Settings, backend: Option<&str>) -> Option<Self> {
        if backend.is_some_and(|backend| !["tiny-skia", "tiny_skia", "software"].contains(&backend))
        {
            return None;
        }

        Some(Self::new(settings))
    }

    fn name(&self) -> String {
        "tiny-skia".to_owned()
    }

    fn screenshot(
        &mut self,
        size: Size<u32>,
        scale_factor: f32,
        background_color: Color,
    ) -> Vec<u8> {
        let viewport = Viewport::with_physical_size(size, scale_factor);

        window::compositor::screenshot(self, &viewport, background_color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(renderer: &mut Renderer, bounds: Rectangle, color: Color) {
        use crate::core::Renderer as _;
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..Default::default()
            },
            color,
        );
    }

    fn opacity_pixels(renderer: &mut Renderer) -> Vec<u8> {
        use crate::core::renderer::Headless as _;
        renderer.screenshot(Size::new(16, 16), 1.0, Color::from_rgb(0.0, 0.0, 1.0))
    }

    fn pixel(bytes: &[u8], x: usize, y: usize) -> &[u8] {
        &bytes[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]
    }

    #[test]
    fn opacity_is_applied_once_after_overlapping_content_and_repaints_live() {
        use crate::core::Renderer as _;
        let bounds = Rectangle::with_size(Size::new(16.0, 16.0));
        let left = Rectangle::with_size(Size::new(8.0, 16.0));
        let mut renderer = Renderer::new(renderer::Settings::default());
        for (color, expected) in [
            (Color::from_rgb(0.0, 1.0, 0.0), [0, 128, 127, 255]),
            (Color::WHITE, [128, 128, 255, 255]),
        ] {
            renderer.reset(bounds);
            renderer.with_opacity_layer(bounds, 0.5, |renderer| {
                fill(renderer, bounds, Color::from_rgb(1.0, 0.0, 0.0));
                renderer.with_layer(left, |renderer| fill(renderer, left, color));
            });
            let actual = opacity_pixels(&mut renderer);
            for (actual, expected) in pixel(&actual, 4, 4).iter().zip(expected) {
                assert!((*actual as i16 - expected as i16).abs() <= 1);
            }
            assert_eq!(renderer.opacity_pixels.len(), 1);
        }
        renderer.reset(bounds);
        let _ = opacity_pixels(&mut renderer);
        assert!(
            renderer.opacity_pixels.is_empty(),
            "idle frames release offscreen pixels"
        );
    }

    #[test]
    fn opacity_groups_preserve_nesting_clipping_transforms_and_following_draws() {
        use crate::core::Renderer as _;
        let bounds = Rectangle::with_size(Size::new(16.0, 16.0));
        let mut renderer = Renderer::new(renderer::Settings::default());
        renderer.reset(bounds);
        renderer.with_transformation(Transformation::translate(4.0, 0.0), |renderer| {
            let clip = Rectangle::with_size(Size::new(8.0, 16.0));
            renderer.with_opacity_layer(clip, 0.5, |renderer| {
                fill(renderer, bounds, Color::from_rgb(1.0, 0.0, 0.0));
                renderer.with_opacity_layer(clip, 0.5, |renderer| {
                    fill(renderer, bounds, Color::from_rgb(0.0, 1.0, 0.0));
                });
            });
        });
        fill(
            &mut renderer,
            Rectangle {
                x: 8.0,
                y: 0.0,
                width: 2.0,
                height: 16.0,
            },
            Color::WHITE,
        );
        let actual = opacity_pixels(&mut renderer);
        assert_eq!(pixel(&actual, 2, 4), &[0, 0, 255, 255]);
        assert_eq!(pixel(&actual, 13, 4), &[0, 0, 255, 255]);
        assert_eq!(pixel(&actual, 8, 4), &[255, 255, 255, 255]);
        let blended = pixel(&actual, 6, 4);
        assert!((blended[0] as i16 - 64).abs() <= 1);
        assert!((blended[1] as i16 - 64).abs() <= 1);
        assert!((blended[2] as i16 - 127).abs() <= 1);
        assert_eq!(blended[3], 255);
        assert_eq!(renderer.opacity_pixels.len(), 2);
    }

    #[test]
    fn in_place_scroll_matches_snapshot_composition_exactly() {
        for alpha in [127, 255] {
            for scale in [1.0, 1.5, 2.0] {
                for delta in [-4.0, 4.0] {
                    for bounds in [
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: 32.0,
                            height: 32.0,
                        },
                        Rectangle {
                            x: 3.0,
                            y: 2.0,
                            width: 20.0,
                            height: 24.0,
                        },
                        Rectangle {
                            x: -2.0,
                            y: 0.0,
                            width: 20.0,
                            height: 24.0,
                        },
                    ] {
                        let mut actual = tiny_skia::Pixmap::new(64, 64).unwrap();
                        for (i, pixel) in actual.pixels_mut().iter_mut().enumerate() {
                            *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                                (i % 97) as u8,
                                (i % 113) as u8,
                                (i % 127) as u8,
                                alpha,
                            )
                            .unwrap();
                        }
                        let mut expected = actual.clone();
                        let physical = bounds * scale;
                        let dy = (delta * scale).round() as i32;
                        let x = physical.x.round() as i32;
                        let y = physical.y.round() as i32;
                        let h = physical.height.round() as u32 - dy.unsigned_abs();
                        let source = tiny_skia::IntRect::from_xywh(
                            x,
                            if dy < 0 { y - dy } else { y },
                            physical.width.round() as u32,
                            h,
                        )
                        .unwrap();
                        if let Some(snapshot) = expected.as_ref().clone_rect(source) {
                            expected.draw_pixmap(
                                x,
                                if dy < 0 { y } else { y + dy },
                                snapshot.as_ref(),
                                &tiny_skia::PixmapPaint::default(),
                                tiny_skia::Transform::identity(),
                                None,
                            );
                        }
                        copy_scroll_region(
                            &mut actual.as_mut(),
                            layer::Scroll {
                                bounds,
                                delta: core::Vector::new(0.0, delta),
                            },
                            scale,
                        );
                        assert_eq!(
                            actual.data(),
                            expected.data(),
                            "alpha={alpha} scale={scale} delta={delta} bounds={bounds:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fractional_damage_bounds_are_expanded_to_physical_pixels() {
        assert_eq!(
            pixel_aligned(Rectangle {
                x: 58.8,
                y: 130.5,
                width: 13.2,
                height: 30.0,
            }),
            Rectangle {
                x: 58.0,
                y: 130.0,
                width: 14.0,
                height: 31.0,
            }
        );
    }

    #[test]
    fn integer_damage_bounds_are_unchanged() {
        assert_eq!(
            pixel_aligned(Rectangle {
                x: 4.0,
                y: 0.0,
                width: 372.0,
                height: 329.0,
            }),
            Rectangle {
                x: 4.0,
                y: 0.0,
                width: 372.0,
                height: 329.0,
            }
        );
    }

    #[test]
    fn collect_clip_regions_keeps_only_intersecting_regions() {
        let regions = vec![
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            Rectangle {
                x: 20.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            Rectangle {
                x: 40.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        ];

        assert_eq!(
            collect_clip_regions(
                &regions,
                Rectangle {
                    x: 5.0,
                    y: 0.0,
                    width: 25.0,
                    height: 8.0,
                }
            ),
            vec![
                Rectangle {
                    x: 5.0,
                    y: 0.0,
                    width: 5.0,
                    height: 8.0,
                },
                Rectangle {
                    x: 20.0,
                    y: 0.0,
                    width: 10.0,
                    height: 8.0,
                },
            ]
        );
    }

    #[test]
    fn collect_clip_regions_merges_overlapping_regions() {
        let regions = vec![
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            Rectangle {
                x: 5.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        ];

        assert_eq!(
            collect_clip_regions(
                &regions,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 10.0,
                }
            ),
            vec![Rectangle {
                x: 0.0,
                y: 0.0,
                width: 15.0,
                height: 10.0,
            },]
        );
    }

    #[test]
    fn coalesce_overlapping_regions_merges_transitive_clusters() {
        assert_eq!(
            coalesce_overlapping_regions([
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                Rectangle {
                    x: 9.0,
                    y: 0.0,
                    width: 4.0,
                    height: 10.0,
                },
                Rectangle {
                    x: 20.0,
                    y: 0.0,
                    width: 5.0,
                    height: 10.0,
                },
            ]),
            vec![
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 13.0,
                    height: 10.0,
                },
                Rectangle {
                    x: 20.0,
                    y: 0.0,
                    width: 5.0,
                    height: 10.0,
                },
            ]
        );
    }

    #[test]
    fn for_each_clip_region_preserves_source_region_index() {
        let regions = vec![
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            Rectangle {
                x: 20.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        ];
        let mut visited = Vec::new();

        for_each_clip_region(
            &regions,
            Rectangle {
                x: 18.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            |index, region| visited.push((index, region)),
        );

        assert_eq!(
            visited,
            vec![(
                1,
                Rectangle {
                    x: 20.0,
                    y: 0.0,
                    width: 6.0,
                    height: 8.0,
                }
            )]
        );
    }
}
