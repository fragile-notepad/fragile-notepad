//! Retain the final composed frame and repaint a conservative damage rectangle.
//!
//! This owns a bounded root surface, not a child Renderer. The caller supplies
//! flushed/merged layers to `plan`, performs the requested paint, then calls
//! `completed` with asset readiness. Presentation copies the retained pixels
//! with replacement blending; it must not clear or alpha-blend them again.

use crate::core::{Color, Rectangle, Size, Transformation};
use crate::graphics::{self, Viewport};
use crate::{Engine, layer, raster, text};

use std::sync::Arc;

const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Cumulative root-frame repaint activity and current retained texture bytes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Statistics {
    pub full_repaints: u64,
    pub partial_repaints: u64,
    pub reused_frames: u64,
    /// Frames painted directly, including sparse scenes and broad damage.
    pub direct_frames: u64,
    pub bytes: u64,
    /// Pixels repainted in the retained surface, excluding presentation copies.
    pub damaged_pixels: u64,
}

/// The paint needed before copying the retained frame to its presentation target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Damage {
    Full,
    Partial(Rectangle<u32>),
    Reuse,
}

pub(crate) struct FramePlan {
    pub target: Arc<raster::Target>,
    /// Immutable clear resources, safe to retain across recorded GPU work.
    pub clear: raster::Clear,
    pub damage: Damage,
}

struct SurfaceTarget {
    target: Arc<raster::Target>,
    clear: raster::Clear,
}

type QuadSnapshot = Vec<(Vec<u8>, Rectangle)>;

#[derive(PartialEq)]
enum TextSnapshot {
    Group {
        transformation: Transformation,
        text: Vec<graphics::Text>,
    },
    Cached {
        transformation: Transformation,
        stamp: (text::Id, usize, usize),
        dynamic: bool,
    },
}

#[derive(PartialEq)]
struct RasterSnapshot {
    owner: u64,
    revision: u64,
    bounds: Rectangle,
}

struct LayerSnapshot {
    bounds: Rectangle,
    quads: QuadSnapshot,
    text: Vec<TextSnapshot>,
    images: Vec<graphics::Image>,
    rasters: Vec<RasterSnapshot>,
    primitive_bounds: Vec<Rectangle>,
    triangles: bool,
}

impl LayerSnapshot {
    fn new(layer: &layer::Layer) -> Self {
        Self {
            bounds: layer.bounds,
            quads: layer.quads.damage_snapshot(),
            text: layer
                .text
                .iter()
                .map(|item| match item {
                    text::Item::Group {
                        transformation,
                        text,
                    } => TextSnapshot::Group {
                        transformation: *transformation,
                        text: text.clone(),
                    },
                    text::Item::Cached {
                        transformation,
                        cache,
                    } => TextSnapshot::Cached {
                        transformation: *transformation,
                        stamp: cache.stamp(),
                        dynamic: cache.is_dynamic(),
                    },
                })
                .collect(),
            images: {
                #[cfg(any(feature = "image", feature = "svg"))]
                {
                    layer.images.clone()
                }
                #[cfg(not(any(feature = "image", feature = "svg")))]
                {
                    Vec::new()
                }
            },
            rasters: layer
                .rasters
                .iter()
                .map(|instance| RasterSnapshot {
                    owner: instance._owner.id(),
                    revision: instance.revision,
                    bounds: instance.bounds,
                })
                .collect(),
            // Never retain user primitives or their widget owners in snapshots.
            // Their paint is opaque to this planner, so both frames' bounds are
            // dirty even if the bounds and primitive identities are unchanged.
            primitive_bounds: layer
                .primitives
                .iter()
                .map(|instance| instance.bounds)
                .collect(),
            triangles: !layer.triangles.is_empty(),
        }
    }

    fn add_dynamic_damage(&self, region: &mut Region) {
        for bounds in &self.primitive_bounds {
            region.add_clipped(*bounds, self.bounds);
        }
        if self.triangles {
            region.add(self.bounds);
        }
    }
}

/// Per-renderer root-frame retention; independent of the child-layer budget.
pub(crate) struct State {
    enabled: bool,
    heuristics: bool,
    budget: u64,
    surface: Option<SurfaceTarget>,
    last_snapshot: Vec<LayerSnapshot>,
    previous_clear: Option<Color>,
    size: Option<Size<u32>>,
    scale: Option<f32>,
    fonts: Option<graphics::text::Version>,
    valid: bool,
    pending: bool,
    context_changed_last_frame: bool,
    statistics: Statistics,
}

impl Default for State {
    fn default() -> Self {
        Self {
            enabled: true,
            heuristics: true,
            budget: MAX_BYTES,
            surface: None,
            last_snapshot: Vec::new(),
            previous_clear: None,
            size: None,
            scale: None,
            fonts: None,
            valid: false,
            pending: false,
            context_changed_last_frame: false,
            statistics: Statistics::default(),
        }
    }
}

impl State {
    pub(crate) fn set_heuristics(&mut self, enabled: bool) {
        if self.heuristics != enabled {
            self.heuristics = enabled;
            self.discard();
        }
    }

    /// The root frame has its own hard 32 MiB ceiling, independent of children.
    pub(crate) fn set_budget(&mut self, bytes: u64) {
        self.budget = bytes.min(MAX_BYTES);
        if self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.target.bytes > self.budget)
        {
            self.discard();
        }
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.discard();
        }
    }

    pub(crate) fn stats(&self) -> Statistics {
        Statistics {
            bytes: self
                .surface
                .as_ref()
                .map_or(0, |surface| surface.target.bytes),
            ..self.statistics
        }
    }

    /// Plan from the complete, flushed/merged scene, before drawing its pixels.
    /// None delegates to ordinary rendering; retained pixels remain invalid.
    pub(crate) fn plan(
        &mut self,
        engine: &Engine,
        layers: &layer::Stack,
        viewport: &Viewport,
        clear: Option<Color>,
    ) -> Option<FramePlan> {
        let size = viewport.physical_size();
        let scale = viewport.scale_factor();
        let bytes = u64::from(size.width)
            .checked_mul(u64::from(size.height))
            .and_then(|pixels| pixels.checked_mul(4));
        let supported = matches!(
            engine.format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let limit = engine.device.limits().max_texture_dimension_2d;
        let Some(clear) = clear.filter(|_| {
            self.enabled
                && supported
                && size.width > 0
                && size.height > 0
                && size.width <= limit
                && size.height <= limit
                && scale.is_finite()
                && scale > 0.0
                && bytes.is_some_and(|bytes| bytes <= self.budget)
                && viewport.logical_size().width.is_finite()
                && viewport.logical_size().height.is_finite()
        }) else {
            self.discard();
            self.statistics.direct_frames += 1;
            return None;
        };
        // Copying a full window costs more bandwidth than clearing and drawing
        // a handful of changing decorations. Retain scenes with meaningful
        // reusable paint; custom primitives already own their internal caches.
        if self.heuristics && !worth_retaining(layers) {
            self.discard();
            self.statistics.direct_frames += 1;
            return None;
        }
        if self.heuristics && broad_damage(always_dirty_region(layers, viewport), size) {
            // Live editor text and large opaque primitives already guarantee
            // broad repainting. Avoid allocating/comparing a scene snapshot
            // only to choose the direct path. Quiet frames can capture afresh.
            self.discard();
            self.statistics.direct_frames += 1;
            return None;
        }
        let fonts = graphics::text::font_system()
            .read()
            .expect("Read font system")
            .version();
        let new_target = self.surface.as_ref().is_none_or(|surface| {
            surface.target._texture.width() != size.width
                || surface.target._texture.height() != size.height
                || surface.target._texture.format() != engine.format
        });
        let clear_changed = self.previous_clear != Some(clear);
        let full = !self.valid
            || new_target
            || clear_changed
            || self.scale != Some(scale)
            || self.fonts != Some(fonts);
        let snapshot: Vec<_> = layers
            .iter()
            .filter(|layer| !layer.is_empty())
            .map(LayerSnapshot::new)
            .collect();
        let changes = changed_region(&self.last_snapshot, &snapshot, viewport);
        let stable_context = self.size == Some(size)
            && self.scale == Some(scale)
            && self.fonts == Some(fonts)
            && !clear_changed;
        let context_changed = self.size.is_some() && !stable_context;
        let broad = self.heuristics
            && ((context_changed && self.context_changed_last_frame)
                || (stable_context && broad_damage(changes, size)));
        self.context_changed_last_frame = context_changed;
        let damage = if full { Damage::Full } else { changes };
        self.last_snapshot = snapshot;
        self.previous_clear = Some(clear);
        self.size = Some(size);
        self.scale = Some(scale);
        self.fonts = Some(fonts);
        if broad {
            // Scrolling or large fades should not pay for both a near-complete
            // repaint and a full-window copy. Keep the candidate scene, but
            // never reuse old pixels after painting a frame directly. Once
            // damage settles, a complete refresh makes retention valid again.
            self.valid = false;
            self.pending = false;
            self.statistics.direct_frames += 1;
            return None;
        }
        if new_target {
            let pipeline = engine.raster_pipeline();
            self.surface = Some(SurfaceTarget {
                target: Arc::new(pipeline.create_target(&engine.device, size)),
                clear: pipeline.create_clear_binding(&engine.device, &clear),
            });
        } else if full {
            // Direct frames update the observed context without touching this
            // binding. Refresh it when retention resumes, before partial clears.
            self.surface.as_mut().unwrap().clear = engine
                .raster_pipeline()
                .create_clear_binding(&engine.device, &clear);
        }
        // A plan is only reusable after the caller finishes its requested paint.
        // An abandoned plan or pending asset upload forces the next frame full.
        self.valid = false;
        self.pending = true;
        let pixels = match damage {
            Damage::Full => {
                self.statistics.full_repaints = self.statistics.full_repaints.saturating_add(1);
                u64::from(size.width) * u64::from(size.height)
            }
            Damage::Partial(bounds) => {
                self.statistics.partial_repaints =
                    self.statistics.partial_repaints.saturating_add(1);
                u64::from(bounds.width) * u64::from(bounds.height)
            }
            Damage::Reuse => {
                self.statistics.reused_frames = self.statistics.reused_frames.saturating_add(1);
                0
            }
        };
        self.statistics.damaged_pixels = self.statistics.damaged_pixels.saturating_add(pixels);
        let surface = self.surface.as_ref().unwrap();
        Some(FramePlan {
            target: surface.target.clone(),
            clear: surface.clear.clone(),
            damage,
        })
    }

    /// Call once after encoding the plan; pending images require a full repaint.
    pub(crate) fn completed(&mut self, assets_ready: bool) {
        if self.pending {
            self.valid = assets_ready;
            self.pending = false;
        } else if !assets_ready {
            self.valid = false;
        }
    }

    fn discard(&mut self) {
        self.surface = None;
        self.last_snapshot.clear();
        self.previous_clear = None;
        self.size = None;
        self.scale = None;
        self.fonts = None;
        self.valid = false;
        self.pending = false;
        self.context_changed_last_frame = false;
    }
}

fn broad_damage(damage: Damage, size: Size<u32>) -> bool {
    match damage {
        Damage::Full => true,
        Damage::Partial(bounds) => {
            u64::from(bounds.width) * u64::from(bounds.height)
                > u64::from(size.width) * u64::from(size.height) / 2
        }
        Damage::Reuse => false,
    }
}

fn always_dirty_region(layers: &layer::Stack, viewport: &Viewport) -> Damage {
    let mut region = Region::new(viewport);
    for layer in layers.iter().filter(|layer| !layer.is_empty()) {
        for primitive in &layer.primitives {
            region.add_clipped(primitive.bounds, layer.bounds);
        }
        if !layer.triangles.is_empty() {
            region.add(layer.bounds);
        }
        for item in &layer.text {
            match item {
                text::Item::Group {
                    transformation,
                    text,
                } => {
                    for text in text {
                        if matches!(
                            text,
                            graphics::Text::Editor { .. } | graphics::Text::Raw { .. }
                        ) {
                            region.add_text(text, *transformation, layer.bounds);
                        }
                    }
                }
                text::Item::Cached { cache, .. } if cache.is_dynamic() => region.add(layer.bounds),
                _ => {}
            }
        }
    }
    region.finish(viewport)
}

fn worth_retaining(layers: &layer::Stack) -> bool {
    let mut work = 0usize;
    for layer in layers.iter().filter(|layer| !layer.is_empty()) {
        if !layer.rasters.is_empty() {
            return true;
        }
        work = work.saturating_add(layer.quads.len());
        for item in &layer.text {
            work = work.saturating_add(match item {
                text::Item::Group { text, .. } => text.len().saturating_mul(4),
                text::Item::Cached { .. } => 4,
            });
        }
        if work >= 32 {
            return true;
        }
    }
    false
}

fn changed_region(
    previous: &[LayerSnapshot],
    current: &[LayerSnapshot],
    viewport: &Viewport,
) -> Damage {
    let mut region = Region::new(viewport);
    for index in 0..previous.len().max(current.len()) {
        match (previous.get(index), current.get(index)) {
            (Some(old), Some(new)) => {
                if old.bounds != new.bounds {
                    region.add(old.bounds);
                    region.add(new.bounds);
                    continue;
                }
                // Draw order is part of each snapshot. Positional comparison
                // also covers insertion, deletion, and reordering: repaint both
                // affected extents while preserving unrelated root backgrounds.
                for quad_index in 0..old.quads.len().max(new.quads.len()) {
                    let before = old.quads.get(quad_index);
                    let after = new.quads.get(quad_index);
                    if before != after {
                        if let Some((_, bounds)) = before {
                            region.add_clipped(*bounds, old.bounds);
                        }
                        if let Some((_, bounds)) = after {
                            region.add_clipped(*bounds, new.bounds);
                        }
                    }
                }
                add_text_damage(old, new, &mut region);
                for raster_index in 0..old.rasters.len().max(new.rasters.len()) {
                    let before = old.rasters.get(raster_index);
                    let after = new.rasters.get(raster_index);
                    if before != after {
                        if let Some(instance) = before {
                            region.add_clipped(instance.bounds, old.bounds);
                        }
                        if let Some(instance) = after {
                            region.add_clipped(instance.bounds, new.bounds);
                        }
                    }
                }
                // Compare images by draw order. A handle, opacity, rotation,
                // clip, addition, removal, or reordering changes just the union
                // of the affected old/new image bounds, not a large layer clip.
                for image_index in 0..old.images.len().max(new.images.len()) {
                    let before = old.images.get(image_index);
                    let after = new.images.get(image_index);
                    if before != after {
                        if let Some(image) = before {
                            region.add_image(image, old.bounds);
                        }
                        if let Some(image) = after {
                            region.add_image(image, new.bounds);
                        }
                    }
                }
                old.add_dynamic_damage(&mut region);
                new.add_dynamic_damage(&mut region);
            }
            (Some(layer), None) | (None, Some(layer)) => region.add(layer.bounds),
            (None, None) => unreachable!(),
        }
    }
    region.finish(viewport)
}

fn add_text_damage(old: &LayerSnapshot, new: &LayerSnapshot, region: &mut Region) {
    for index in 0..old.text.len().max(new.text.len()) {
        match (old.text.get(index), new.text.get(index)) {
            (
                Some(TextSnapshot::Group {
                    transformation: before_transform,
                    text: before,
                }),
                Some(TextSnapshot::Group {
                    transformation: after_transform,
                    text: after,
                }),
            ) => {
                for item in 0..before.len().max(after.len()) {
                    let previous = before.get(item);
                    let current = after.get(item);
                    let dynamic = previous.into_iter().chain(current).any(|text| {
                        matches!(
                            text,
                            graphics::Text::Editor { .. } | graphics::Text::Raw { .. }
                        )
                    });
                    if before_transform != after_transform || previous != current || dynamic {
                        if let Some(text) = previous {
                            region.add_text(text, *before_transform, old.bounds);
                        }
                        if let Some(text) = current {
                            region.add_text(text, *after_transform, new.bounds);
                        }
                    }
                }
            }
            (before, after) => {
                let dynamic = before
                    .into_iter()
                    .chain(after)
                    .any(|item| matches!(item, TextSnapshot::Cached { dynamic: true, .. }));
                if before != after || dynamic {
                    if let Some(item) = before {
                        region.add_text_item(item, old.bounds);
                    }
                    if let Some(item) = after {
                        region.add_text_item(item, new.bounds);
                    }
                }
            }
        }
    }
}

struct Region {
    viewport: Rectangle,
    bounds: Option<Rectangle>,
    full: bool,
}

impl Region {
    fn new(viewport: &Viewport) -> Self {
        Self {
            viewport: Rectangle::with_size(viewport.logical_size()),
            bounds: None,
            full: false,
        }
    }

    fn add(&mut self, bounds: Rectangle) {
        if !finite(bounds) {
            // Unknown geometry must not silently reuse potentially stale pixels.
            self.full = true;
        } else if let Some(bounds) = bounds.intersection(&self.viewport) {
            self.bounds = Some(
                self.bounds
                    .map_or(bounds, |previous| previous.union(&bounds)),
            );
        }
    }

    fn add_clipped(&mut self, bounds: Rectangle, clip: Rectangle) {
        if !finite(bounds) || (!finite(clip) && clip != Rectangle::INFINITE) {
            self.full = true;
        } else if clip == Rectangle::INFINITE {
            self.add(bounds);
        } else if let Some(bounds) = bounds.intersection(&clip) {
            self.add(bounds);
        }
    }

    fn add_image(&mut self, image: &graphics::Image, layer_bounds: Rectangle) {
        let clip = match image {
            graphics::Image::Raster { clip_bounds, .. }
            | graphics::Image::Vector { clip_bounds, .. } => *clip_bounds,
        };
        let bounds = image.bounds(); // Includes rotation's axis-aligned extent.
        if !finite(bounds) || (!finite(clip) && clip != Rectangle::INFINITE) {
            self.full = true;
        } else if clip == Rectangle::INFINITE {
            self.add_clipped(bounds, layer_bounds);
        } else if let Some(bounds) = bounds.intersection(&clip) {
            self.add_clipped(bounds, layer_bounds);
        }
    }

    fn add_text_item(&mut self, item: &TextSnapshot, layer_bounds: Rectangle) {
        match item {
            TextSnapshot::Group {
                transformation,
                text,
            } => {
                for text in text {
                    self.add_text(text, *transformation, layer_bounds);
                }
            }
            TextSnapshot::Cached { .. } => self.add(layer_bounds),
        }
    }

    fn add_text(&mut self, text: &graphics::Text, group: Transformation, layer_bounds: Rectangle) {
        // Glyph bearings, alignment anchors, and decorations can escape a text
        // layout's minimum bounds. Its actual clip is the safe damage extent.
        let (clip, transformation) = match text {
            graphics::Text::Paragraph {
                clip_bounds,
                transformation,
                ..
            }
            | graphics::Text::Editor {
                clip_bounds,
                transformation,
                ..
            } => (*clip_bounds, group * *transformation),
            graphics::Text::Cached { clip_bounds, .. } => (*clip_bounds, group),
            // Raw's clip/transformation contract is opaque to this planner.
            graphics::Text::Raw { .. } => {
                self.add(layer_bounds);
                return;
            }
        };
        if clip == Rectangle::INFINITE {
            self.add(layer_bounds);
            return;
        }
        if !finite(clip)
            || !transformation
                .as_ref()
                .iter()
                .all(|value| value.is_finite())
        {
            self.full = true;
            return;
        }
        // Transform all four corners so reflected/scaled clips remain safe.
        let corners = [
            crate::core::Point::new(clip.x, clip.y),
            crate::core::Point::new(clip.x + clip.width, clip.y),
            crate::core::Point::new(clip.x, clip.y + clip.height),
            crate::core::Point::new(clip.x + clip.width, clip.y + clip.height),
        ]
        .map(|point| point * transformation);
        if !corners
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite())
        {
            self.full = true;
            return;
        }
        let left = corners
            .iter()
            .map(|point| point.x)
            .fold(f32::INFINITY, f32::min);
        let top = corners
            .iter()
            .map(|point| point.y)
            .fold(f32::INFINITY, f32::min);
        let right = corners
            .iter()
            .map(|point| point.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = corners
            .iter()
            .map(|point| point.y)
            .fold(f32::NEG_INFINITY, f32::max);
        self.add_clipped(
            Rectangle {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            },
            layer_bounds,
        );
    }

    fn finish(self, viewport: &Viewport) -> Damage {
        if self.full {
            return Damage::Full;
        }
        let Some(bounds) = self.bounds else {
            return Damage::Reuse;
        };
        let scale = f64::from(viewport.scale_factor());
        let size = viewport.physical_size();
        // Expand by one physical pixel before rounding outwards for AA/filtering.
        let x = (f64::from(bounds.x) * scale - 1.0)
            .floor()
            .clamp(0.0, f64::from(size.width)) as u32;
        let y = (f64::from(bounds.y) * scale - 1.0)
            .floor()
            .clamp(0.0, f64::from(size.height)) as u32;
        let right = ((f64::from(bounds.x) + f64::from(bounds.width)) * scale + 1.0)
            .ceil()
            .clamp(0.0, f64::from(size.width)) as u32;
        let bottom = ((f64::from(bounds.y) + f64::from(bounds.height)) * scale + 1.0)
            .ceil()
            .clamp(0.0, f64::from(size.height)) as u32;
        let damage = Rectangle {
            x,
            y,
            width: right - x,
            height: bottom - y,
        };
        if damage.width == 0 || damage.height == 0 {
            Damage::Reuse
        } else if damage == Rectangle::with_size(size) {
            Damage::Full
        } else {
            Damage::Partial(damage)
        }
    }
}

fn finite(bounds: Rectangle) -> bool {
    [
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        bounds.x + bounds.width,
        bounds.y + bounds.height,
    ]
    .into_iter()
    .all(f32::is_finite)
        && bounds.width >= 0.0
        && bounds.height >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    fn snapshot() -> LayerSnapshot {
        LayerSnapshot {
            bounds: Rectangle::INFINITE,
            quads: vec![(vec![0, 255], rect(0.0, 0.0, 100.0, 100.0))],
            text: Vec::new(),
            images: Vec::new(),
            rasters: Vec::new(),
            primitive_bounds: Vec::new(),
            triangles: false,
        }
    }

    fn viewport() -> Viewport {
        Viewport::with_physical_size(Size::new(100, 100), 1.0)
    }

    fn partial(x: u32, y: u32, width: u32, height: u32) -> Damage {
        Damage::Partial(Rectangle {
            x,
            y,
            width,
            height,
        })
    }

    fn text(content: &str, clip_bounds: Rectangle) -> graphics::Text {
        graphics::Text::Cached {
            content: content.to_owned(),
            // Intentionally narrower than the real clip and anchored to its
            // right: damage must cover glyph bearings and alignment overflow.
            bounds: rect(clip_bounds.x + clip_bounds.width, clip_bounds.y, 1.0, 1.0),
            color: Color::BLACK,
            size: crate::core::Pixels(12.0),
            line_height: crate::core::Pixels(14.0),
            font: Default::default(),
            align_x: crate::core::text::Alignment::Right,
            align_y: crate::core::alignment::Vertical::Top,
            shaping: crate::core::text::Shaping::Basic,
            wrapping: Default::default(),
            ellipsis: Default::default(),
            clip_bounds,
        }
    }

    #[test]
    fn unchanged_static_scene_reuses_the_frame() {
        assert_eq!(
            changed_region(&[snapshot()], &[snapshot()], &viewport()),
            Damage::Reuse
        );
    }

    #[test]
    fn fading_status_quad_keeps_the_unchanged_root_background() {
        let mut before = snapshot();
        let mut after = snapshot();
        // The helper has already included the widget's shadow and its offset.
        let shadow_bounds = rect(17.0, 18.0, 20.0, 22.0);
        before.quads.push((vec![0, 200], shadow_bounds));
        after.quads.push((vec![0, 80], shadow_bounds));
        assert_eq!(
            changed_region(&[before], &[after], &viewport()),
            partial(16, 17, 22, 24)
        );
    }

    #[test]
    fn moving_or_removed_custom_paint_dirties_its_previous_extent() {
        let mut before = snapshot();
        let mut after = snapshot();
        before.primitive_bounds.push(rect(10.0, 20.0, 10.0, 10.0));
        after.primitive_bounds.push(rect(40.0, 25.0, 10.0, 10.0));
        assert_eq!(
            changed_region(&[before], &[after], &viewport()),
            partial(9, 19, 42, 17)
        );
        let mut removed = snapshot();
        removed.primitive_bounds.push(rect(10.0, 20.0, 10.0, 10.0));
        assert_eq!(
            changed_region(&[removed], &[snapshot()], &viewport()),
            partial(9, 19, 12, 12)
        );
    }

    #[test]
    fn changed_raster_revision_and_drag_dirty_only_instance_bounds() {
        let mut before = snapshot();
        let mut after = snapshot();
        before.rasters.push(RasterSnapshot {
            owner: 3,
            revision: 1,
            bounds: rect(10.0, 20.0, 10.0, 10.0),
        });
        after.rasters.push(RasterSnapshot {
            owner: 3,
            revision: 2,
            bounds: rect(40.0, 25.0, 10.0, 10.0),
        });
        assert_eq!(
            changed_region(&[before], &[after], &viewport()),
            partial(9, 19, 42, 17)
        );
    }

    #[test]
    fn scrolled_text_repaints_its_transformed_clip_including_overhang() {
        let mut before = snapshot();
        let mut after = snapshot();
        let clip = rect(10.0, 20.0, 30.0, 10.0);
        let transformation = Transformation::translate(5.0, 7.0);
        before.text.push(TextSnapshot::Group {
            transformation,
            text: vec![text("previous row", clip)],
        });
        after.text.push(TextSnapshot::Group {
            transformation,
            text: vec![text("next row", clip)],
        });
        let viewport = Viewport::with_physical_size(Size::new(150, 150), 1.5);
        assert_eq!(
            changed_region(&[before], &[after], &viewport),
            partial(21, 39, 48, 18)
        );
    }

    #[test]
    fn raw_text_dirties_the_layer_clip_even_with_unchanged_identity() {
        let raw = graphics::Text::Raw {
            raw: graphics::text::Raw {
                buffer: std::sync::Weak::new(),
                position: Default::default(),
                color: Color::BLACK,
                clip_bounds: rect(20.0, 20.0, 1.0, 1.0),
            },
            transformation: Transformation::IDENTITY,
        };
        let mut before = snapshot();
        let mut after = snapshot();
        before.bounds = rect(2.0, 3.0, 90.0, 90.0);
        after.bounds = before.bounds;
        before.text.push(TextSnapshot::Group {
            transformation: Transformation::IDENTITY,
            text: vec![raw.clone()],
        });
        after.text.push(TextSnapshot::Group {
            transformation: Transformation::IDENTITY,
            text: vec![raw],
        });
        assert_eq!(
            changed_region(&[before], &[after], &viewport()),
            partial(1, 2, 92, 92)
        );
    }

    #[test]
    fn fractional_damage_rounds_outward_after_one_physical_pixel_expansion() {
        let viewport = Viewport::with_physical_size(Size::new(150, 150), 1.5);
        let mut region = Region::new(&viewport);
        region.add(rect(10.25, 20.5, 1.0, 2.0));
        assert_eq!(region.finish(&viewport), partial(14, 29, 4, 6));
        let mut region = Region::new(&viewport);
        region.add(rect(0.0, 0.0, 0.25, 0.25));
        assert_eq!(region.finish(&viewport), partial(0, 0, 2, 2));
    }

    #[test]
    fn unknown_geometry_never_reuses_stale_pixels() {
        let mut after = snapshot();
        after.quads.push((vec![0], rect(f32::NAN, 10.0, 2.0, 2.0)));
        assert_eq!(
            changed_region(&[snapshot()], &[after], &viewport()),
            Damage::Full
        );
    }
}
