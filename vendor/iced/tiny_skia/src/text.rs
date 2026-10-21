use crate::core::alignment;
use crate::core::text::Paragraph as _;
use crate::core::text::{Alignment, Ellipsis, Shaping, Wrapping};
use crate::core::{Color, Font, Pixels, Point, Rectangle, Transformation};
use crate::graphics::text::cache::{self, Cache};
use crate::graphics::text::editor;
use crate::graphics::text::font_system;
use crate::graphics::text::paragraph;

use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::hash_map;

#[derive(Debug)]
pub struct Pipeline {
    glyph_cache: GlyphCache,
    paragraph_raster_cache: ParagraphRasterCache,
    swash_cache: cosmic_text::SwashCache,
    cache: RefCell<Cache>,
    clip_scratch: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PipelineStats {
    pub paragraph_raster_hits: u64,
    pub paragraph_raster_misses: u64,
    pub paragraph_raster_bypasses: u64,
    pub glyph_hits: u64,
    pub glyph_misses: u64,
}

impl PipelineStats {
    pub fn saturating_delta(self, previous: Self) -> Self {
        Self {
            paragraph_raster_hits: self
                .paragraph_raster_hits
                .saturating_sub(previous.paragraph_raster_hits),
            paragraph_raster_misses: self
                .paragraph_raster_misses
                .saturating_sub(previous.paragraph_raster_misses),
            paragraph_raster_bypasses: self
                .paragraph_raster_bypasses
                .saturating_sub(previous.paragraph_raster_bypasses),
            glyph_hits: self.glyph_hits.saturating_sub(previous.glyph_hits),
            glyph_misses: self.glyph_misses.saturating_sub(previous.glyph_misses),
        }
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Pipeline {
            glyph_cache: GlyphCache::new(),
            paragraph_raster_cache: ParagraphRasterCache::new(),
            swash_cache: cosmic_text::SwashCache::new(),
            cache: RefCell::new(Cache::new()),
            clip_scratch: Vec::new(),
        }
    }

    // TODO: Shared engine
    #[allow(dead_code)]
    pub fn load_font(&mut self, bytes: Cow<'static, [u8]>) {
        font_system()
            .write()
            .expect("Write font system")
            .load_font(bytes);

        self.cache = RefCell::new(Cache::new());
        self.swash_cache = cosmic_text::SwashCache::new();
    }

    pub fn draw_paragraph(
        &mut self,
        paragraph: &paragraph::Weak,
        position: Point,
        color: Color,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: Option<&tiny_skia::Mask>,
        transformation: Transformation,
        clip_bounds: Option<Rectangle>,
    ) {
        let Some(paragraph) = paragraph.upgrade() else {
            return;
        };

        let mut font_system = font_system().write().expect("Write font system");
        if let Some(raster) = self.paragraph_raster_cache.allocate(
            &paragraph,
            color,
            transformation.scale_factor(),
            font_system.raw(),
            &mut self.glyph_cache,
            &mut self.swash_cache,
            &mut self.clip_scratch,
        ) {
            let position = position * transformation;

            draw_pixmap_clipped(
                pixels,
                position.x.round() as i32 + raster.left as i32,
                position.y.round() as i32 + raster.top as i32,
                raster.pixmap,
                &tiny_skia::PixmapPaint::default(),
                clip_mask,
                clip_bounds,
                &mut self.clip_scratch,
            );
        } else {
            draw(
                font_system.raw(),
                &mut self.glyph_cache,
                &mut self.swash_cache,
                paragraph.buffer(),
                position,
                color,
                pixels,
                clip_mask,
                transformation,
                clip_bounds,
                &mut self.clip_scratch,
            );
        }
    }

    pub fn draw_editor(
        &mut self,
        editor: &editor::Weak,
        position: Point,
        color: Color,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: Option<&tiny_skia::Mask>,
        transformation: Transformation,
        clip_bounds: Option<Rectangle>,
    ) {
        let Some(editor) = editor.upgrade() else {
            return;
        };

        let mut font_system = font_system().write().expect("Write font system");

        draw(
            font_system.raw(),
            &mut self.glyph_cache,
            &mut self.swash_cache,
            editor.buffer(),
            position,
            color,
            pixels,
            clip_mask,
            transformation,
            clip_bounds,
            &mut self.clip_scratch,
        );
    }

    pub fn draw_cached(
        &mut self,
        content: &str,
        bounds: Rectangle,
        color: Color,
        size: Pixels,
        line_height: Pixels,
        font: Font,
        align_x: Alignment,
        align_y: alignment::Vertical,
        shaping: Shaping,
        wrapping: Wrapping,
        ellipsis: Ellipsis,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: Option<&tiny_skia::Mask>,
        transformation: Transformation,
        clip_bounds: Option<Rectangle>,
    ) {
        let line_height = f32::from(line_height);

        let mut font_system = font_system().write().expect("Write font system");
        let font_system = font_system.raw();

        let key = cache::Key {
            bounds: bounds.size(),
            content,
            font,
            size: size.into(),
            line_height,
            shaping,
            wrapping,
            ellipsis,
            align_x,
        };

        let (_, entry) = self.cache.get_mut().allocate(font_system, key);

        let width = entry.min_bounds.width;
        let height = entry.min_bounds.height;

        let x = match align_x {
            Alignment::Default | Alignment::Left | Alignment::Justified => bounds.x,
            Alignment::Center => bounds.x - width / 2.0,
            Alignment::Right => bounds.x - width,
        };

        let y = match align_y {
            alignment::Vertical::Top => bounds.y,
            alignment::Vertical::Center => bounds.y - height / 2.0,
            alignment::Vertical::Bottom => bounds.y - height,
        };

        draw(
            font_system,
            &mut self.glyph_cache,
            &mut self.swash_cache,
            &entry.buffer,
            Point::new(x, y),
            color,
            pixels,
            clip_mask,
            transformation,
            clip_bounds,
            &mut self.clip_scratch,
        );
    }

    pub fn draw_raw(
        &mut self,
        buffer: &cosmic_text::Buffer,
        position: Point,
        color: Color,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: Option<&tiny_skia::Mask>,
        transformation: Transformation,
        clip_bounds: Option<Rectangle>,
    ) {
        let mut font_system = font_system().write().expect("Write font system");

        draw(
            font_system.raw(),
            &mut self.glyph_cache,
            &mut self.swash_cache,
            buffer,
            position,
            color,
            pixels,
            clip_mask,
            transformation,
            clip_bounds,
            &mut self.clip_scratch,
        );
    }

    pub fn trim_cache(&mut self) {
        self.cache.get_mut().trim();
        self.paragraph_raster_cache.trim();
        self.glyph_cache.trim();
    }

    pub(crate) fn stats(&self) -> PipelineStats {
        PipelineStats {
            paragraph_raster_hits: self.paragraph_raster_cache.hits,
            paragraph_raster_misses: self.paragraph_raster_cache.misses,
            paragraph_raster_bypasses: self.paragraph_raster_cache.bypasses,
            glyph_hits: self.glyph_cache.hits,
            glyph_misses: self.glyph_cache.misses,
        }
    }
}

fn draw(
    font_system: &mut cosmic_text::FontSystem,
    glyph_cache: &mut GlyphCache,
    swash_cache: &mut cosmic_text::SwashCache,
    buffer: &cosmic_text::Buffer,
    position: Point,
    color: Color,
    pixels: &mut tiny_skia::PixmapMut<'_>,
    clip_mask: Option<&tiny_skia::Mask>,
    transformation: Transformation,
    clip_bounds: Option<Rectangle>,
    clip_scratch: &mut Vec<u8>,
) {
    let position = position * transformation;
    let scale = transformation.scale_factor();

    for run in buffer.layout_runs() {
        if let Some(clip_bounds) = clip_bounds {
            let line_y = position.y + run.line_y * scale;
            let line_height = run.line_height * scale;

            if line_y + line_height < clip_bounds.y
                || line_y - line_height > clip_bounds.y + clip_bounds.height
            {
                continue;
            }
        }

        for glyph in run.glyphs {
            if let Some(clip_bounds) = clip_bounds {
                let glyph_left = position.x + (glyph.x + glyph.font_size * glyph.x_offset) * scale;
                let glyph_right = glyph_left + glyph.w.max(1.0) * scale;

                if glyph_right < clip_bounds.x || glyph_left > clip_bounds.x + clip_bounds.width {
                    continue;
                }
            }

            let physical_glyph = glyph.physical((position.x, position.y), scale);

            if let Some((buffer, placement)) = glyph_cache.allocate(
                physical_glyph.cache_key,
                glyph.color_opt.map(from_color).unwrap_or(color),
                font_system,
                swash_cache,
            ) {
                let draw_x = physical_glyph.x + placement.left;
                let draw_y = physical_glyph.y - placement.top + (run.line_y * scale).round() as i32;

                if let Some(clip_bounds) = clip_bounds {
                    let glyph_bounds = Rectangle {
                        x: draw_x as f32,
                        y: draw_y as f32,
                        width: placement.width as f32,
                        height: placement.height as f32,
                    };

                    if !glyph_bounds.intersects(&clip_bounds) {
                        continue;
                    }
                }

                let pixmap =
                    tiny_skia::PixmapRef::from_bytes(buffer, placement.width, placement.height)
                        .expect("Create glyph pixel map");

                let opacity =
                    color.a * glyph.color_opt.map(|c| c.a() as f32 / 255.0).unwrap_or(1.0);

                draw_pixmap_clipped(
                    pixels,
                    draw_x,
                    draw_y,
                    pixmap,
                    &tiny_skia::PixmapPaint {
                        opacity,
                        ..tiny_skia::PixmapPaint::default()
                    },
                    clip_mask,
                    clip_bounds,
                    clip_scratch,
                );
            }
        }
    }
}

fn draw_pixmap_clipped(
    pixels: &mut tiny_skia::PixmapMut<'_>,
    x: i32,
    y: i32,
    pixmap: tiny_skia::PixmapRef<'_>,
    paint: &tiny_skia::PixmapPaint,
    clip_mask: Option<&tiny_skia::Mask>,
    clip_bounds: Option<Rectangle>,
    clip_scratch: &mut Vec<u8>,
) {
    if clip_bounds.is_none() && x >= 0 && y >= 0 {
        pixels.draw_pixmap(
            x,
            y,
            pixmap,
            paint,
            tiny_skia::Transform::identity(),
            clip_mask,
        );
        return;
    }

    // Tiny-skia rounds negative rectangle origins toward zero, repeating an
    // edge row or column. Crop to the destination before drawing, including
    // glyphs with negative bearings painted into an unclipped paragraph raster.
    let canvas_bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: pixels.width() as f32,
        height: pixels.height() as f32,
    };
    let Some(clip_bounds) = clip_bounds
        .unwrap_or(canvas_bounds)
        .intersection(&canvas_bounds)
    else {
        return;
    };

    let Some(source) = clipped_source_rect(x, y, pixmap.width(), pixmap.height(), clip_bounds)
    else {
        return;
    };

    if source.x() == 0
        && source.y() == 0
        && source.width() == pixmap.width()
        && source.height() == pixmap.height()
    {
        pixels.draw_pixmap(
            x,
            y,
            pixmap,
            paint,
            tiny_skia::Transform::identity(),
            clip_mask,
        );
        return;
    }

    let row_bytes = source.width() as usize * 4;
    if source.x() == 0 && source.width() == pixmap.width() {
        let start = source.y() as usize * row_bytes;
        let end = start + source.height() as usize * row_bytes;
        let cropped = tiny_skia::PixmapRef::from_bytes(
            &pixmap.data()[start..end],
            source.width(),
            source.height(),
        )
        .expect("contiguous cropped pixel buffer dimensions");
        pixels.draw_pixmap(
            x,
            y + source.y(),
            cropped,
            paint,
            tiny_skia::Transform::identity(),
            clip_mask,
        );
        return;
    }
    clip_scratch.clear();
    clip_scratch.reserve(row_bytes * source.height() as usize);
    for row in source.y() as usize..source.bottom() as usize {
        let start = (row * pixmap.width() as usize + source.x() as usize) * 4;
        clip_scratch.extend_from_slice(&pixmap.data()[start..start + row_bytes]);
    }
    let cropped = tiny_skia::PixmapRef::from_bytes(clip_scratch, source.width(), source.height())
        .expect("cropped pixel buffer dimensions");

    pixels.draw_pixmap(
        x + source.x(),
        y + source.y(),
        cropped,
        paint,
        tiny_skia::Transform::identity(),
        clip_mask,
    );
}

fn clipped_source_rect(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    clip_bounds: Rectangle,
) -> Option<tiny_skia::IntRect> {
    let width = i32::try_from(width).ok()?;
    let height = i32::try_from(height).ok()?;

    let left = (clip_bounds.x.ceil() as i32).max(x);
    let top = (clip_bounds.y.ceil() as i32).max(y);
    let right = ((clip_bounds.x + clip_bounds.width).floor() as i32).min(x.checked_add(width)?);
    let bottom = ((clip_bounds.y + clip_bounds.height).floor() as i32).min(y.checked_add(height)?);

    tiny_skia::IntRect::from_ltrb(
        left.checked_sub(x)?,
        top.checked_sub(y)?,
        right.checked_sub(x)?,
        bottom.checked_sub(y)?,
    )
}

fn from_color(color: cosmic_text::Color) -> Color {
    let [r, g, b, a] = color.as_rgba();

    Color::from_rgba8(r, g, b, a as f32 / 255.0)
}

#[derive(Debug, Clone)]
struct ParagraphRaster {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    left: u32,
    top: u32,
}

impl ParagraphRaster {
    fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    fn as_ref(&self) -> ParagraphRasterRef<'_> {
        ParagraphRasterRef {
            pixmap: self.pixmap(),
            left: self.left,
            top: self.top,
        }
    }

    fn pixmap(&self) -> tiny_skia::PixmapRef<'_> {
        tiny_skia::PixmapRef::from_bytes(
            bytemuck::cast_slice(self.pixels.as_slice()),
            self.width,
            self.height,
        )
        .expect("Build paragraph raster pixmap")
    }
}

#[derive(Debug, Clone, Copy)]
struct ParagraphRasterRef<'a> {
    pixmap: tiny_skia::PixmapRef<'a>,
    left: u32,
    top: u32,
}

fn crop_transparent_padding(pixmap: &tiny_skia::Pixmap) -> ParagraphRaster {
    let width = pixmap.width();
    let height = pixmap.height();
    let pixels = pixmap.data();
    let stride = width as usize;
    let mut left = width;
    let mut top = height;
    let mut right = 0;
    let mut bottom = 0;

    for y in 0..height {
        let row_start = y as usize * stride;

        for x in 0..width {
            let alpha = pixels[(row_start + x as usize) * 4 + 3];

            if alpha == 0 {
                continue;
            }

            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }

    if left >= right || top >= bottom {
        return ParagraphRaster {
            pixels: vec![0],
            width: 1,
            height: 1,
            left: 0,
            top: 0,
        };
    }

    let cropped_width = right - left;
    let cropped_height = bottom - top;
    let mut pixels_out = Vec::with_capacity(cropped_width as usize * cropped_height as usize);
    let source_pixels = bytemuck::cast_slice::<u8, u32>(pixels);

    for y in top..bottom {
        let start = y as usize * stride + left as usize;
        let end = start + cropped_width as usize;
        pixels_out.extend_from_slice(&source_pixels[start..end]);
    }

    ParagraphRaster {
        pixels: pixels_out,
        width: cropped_width,
        height: cropped_height,
        left,
        top,
    }
}

#[derive(Debug, Clone)]
struct ParagraphRasterEntry {
    raster: ParagraphRaster,
    last_used: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ParagraphRasterKey {
    paragraph: u64,
    color: [u8; 4],
    scale: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Default)]
struct ParagraphRasterCache {
    entries: FxHashMap<ParagraphRasterKey, ParagraphRasterEntry>,
    clock: u64,
    total_pixels: usize,
    hits: u64,
    misses: u64,
    bypasses: u64,
}

impl ParagraphRasterCache {
    const CAPACITY_LIMIT: usize = 512;
    const PIXEL_LIMIT: usize = 16 * 1024 * 1024;
    const MAX_RASTER_PIXELS: usize = 1024 * 1024;

    fn new() -> Self {
        Self::default()
    }

    fn allocate(
        &mut self,
        paragraph: &paragraph::Paragraph,
        color: Color,
        scale: f32,
        font_system: &mut cosmic_text::FontSystem,
        glyph_cache: &mut GlyphCache,
        swash_cache: &mut cosmic_text::SwashCache,
        clip_scratch: &mut Vec<u8>,
    ) -> Option<ParagraphRasterRef<'_>> {
        if scale <= 0.0 || !scale.is_finite() {
            self.bypasses = self.bypasses.saturating_add(1);
            return None;
        }

        let min_bounds = paragraph.min_bounds();
        let width = (min_bounds.width * scale).ceil().max(1.0) as u32;
        let height = (min_bounds.height * scale).ceil().max(1.0) as u32;
        let raster_pixels = width as usize * height as usize;

        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || raster_pixels > Self::MAX_RASTER_PIXELS
        {
            self.bypasses = self.bypasses.saturating_add(1);
            return None;
        }

        let key = ParagraphRasterKey {
            paragraph: paragraph.cache_key(),
            color: color.into_rgba8(),
            scale: scale.to_bits(),
            width,
            height,
        };

        if self.entries.contains_key(&key) {
            self.hits = self.hits.saturating_add(1);
            self.mark_used(&key);
            return self.entries.get(&key).map(|entry| entry.raster.as_ref());
        }

        let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
        self.misses = self.misses.saturating_add(1);
        draw(
            font_system,
            glyph_cache,
            swash_cache,
            paragraph.buffer(),
            Point::ORIGIN,
            color,
            &mut pixmap.as_mut(),
            None,
            Transformation::scale(scale),
            None,
            clip_scratch,
        );
        let raster = crop_transparent_padding(&pixmap);
        let raster_pixels = raster.pixel_count();
        let last_used = self.next_lru_tick();

        if let Some(previous) = self
            .entries
            .insert(key, ParagraphRasterEntry { raster, last_used })
        {
            self.total_pixels = self
                .total_pixels
                .saturating_sub(previous.raster.pixel_count());
        }

        self.total_pixels = self.total_pixels.saturating_add(raster_pixels);
        self.evict_until_within_capacity();

        self.entries.get(&key).map(|entry| entry.raster.as_ref())
    }

    fn trim(&mut self) {
        self.evict_until_within_capacity();
        self.entries.shrink_to(Self::CAPACITY_LIMIT);
    }

    fn mark_used(&mut self, key: &ParagraphRasterKey) {
        let last_used = self.next_lru_tick();

        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = last_used;
        }
    }

    fn next_lru_tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);

        if self.clock == 0 {
            self.renormalize_lru_clock();
            self.clock = self.entries.len() as u64 + 1;
        }

        self.clock
    }

    fn renormalize_lru_clock(&mut self) {
        let mut entries = self
            .entries
            .iter()
            .map(|(key, entry)| (*key, entry.last_used))
            .collect::<Vec<_>>();

        entries.sort_by_key(|(_, last_used)| *last_used);

        for (index, (key, _)) in entries.into_iter().enumerate() {
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.last_used = index as u64 + 1;
            }
        }
    }

    fn evict_until_within_capacity(&mut self) {
        while self.entries.len() > Self::CAPACITY_LIMIT || self.total_pixels > Self::PIXEL_LIMIT {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key)
            else {
                return;
            };

            if let Some(entry) = self.entries.remove(&key) {
                self.total_pixels = self.total_pixels.saturating_sub(entry.raster.pixel_count());
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct GlyphCache {
    entries: FxHashMap<(cosmic_text::CacheKey, [u8; 3]), (Vec<u32>, cosmic_text::Placement)>,
    recently_used: FxHashSet<(cosmic_text::CacheKey, [u8; 3])>,
    trim_count: usize,
    hits: u64,
    misses: u64,
}

impl GlyphCache {
    const TRIM_INTERVAL: usize = 300;
    const CAPACITY_LIMIT: usize = 16 * 1024;

    fn new() -> Self {
        GlyphCache::default()
    }

    fn allocate(
        &mut self,
        cache_key: cosmic_text::CacheKey,
        color: Color,
        font_system: &mut cosmic_text::FontSystem,
        swash: &mut cosmic_text::SwashCache,
    ) -> Option<(&[u8], cosmic_text::Placement)> {
        let [r, g, b, _a] = color.into_rgba8();
        let key = (cache_key, [r, g, b]);

        if let hash_map::Entry::Vacant(entry) = self.entries.entry(key) {
            self.misses = self.misses.saturating_add(1);
            // TODO: Outline support
            let image = swash.get_image_uncached(font_system, cache_key)?;

            let glyph_size = image.placement.width as usize * image.placement.height as usize;

            if glyph_size == 0 {
                return None;
            }

            let mut buffer = vec![0u32; glyph_size];

            match image.content {
                cosmic_text::SwashContent::Mask => {
                    let mut i = 0;

                    // TODO: Blend alpha

                    for _y in 0..image.placement.height {
                        for _x in 0..image.placement.width {
                            buffer[i] = bytemuck::cast(
                                tiny_skia::ColorU8::from_rgba(b, g, r, image.data[i]).premultiply(),
                            );

                            i += 1;
                        }
                    }
                }
                cosmic_text::SwashContent::Color => {
                    let mut i = 0;

                    for _y in 0..image.placement.height {
                        for _x in 0..image.placement.width {
                            // TODO: Blend alpha
                            buffer[i >> 2] = bytemuck::cast(
                                tiny_skia::ColorU8::from_rgba(
                                    image.data[i + 2],
                                    image.data[i + 1],
                                    image.data[i],
                                    image.data[i + 3],
                                )
                                .premultiply(),
                            );

                            i += 4;
                        }
                    }
                }
                cosmic_text::SwashContent::SubpixelMask => {
                    // TODO
                }
            }

            let _ = entry.insert((buffer, image.placement));
        } else {
            self.hits = self.hits.saturating_add(1);
        }

        let _ = self.recently_used.insert(key);

        self.entries
            .get(&key)
            .map(|(buffer, placement)| (bytemuck::cast_slice(buffer.as_slice()), *placement))
    }

    pub fn trim(&mut self) {
        if self.trim_count > Self::TRIM_INTERVAL || self.recently_used.len() >= Self::CAPACITY_LIMIT
        {
            self.entries
                .retain(|key, _| self.recently_used.contains(key));

            self.recently_used.clear();

            self.entries.shrink_to(Self::CAPACITY_LIMIT);
            self.recently_used.shrink_to(Self::CAPACITY_LIMIT);

            self.trim_count = 0;
        } else {
            self.trim_count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_pixmap_origins_match_a_translated_reference() {
        let mut source = tiny_skia::Pixmap::new(9, 7).unwrap();
        for (index, pixel) in source.pixels_mut().iter_mut().enumerate() {
            *pixel = tiny_skia::ColorU8::from_rgba(
                (index * 7) as u8,
                (index * 13) as u8,
                (index * 3) as u8,
                160,
            )
            .premultiply();
        }

        let mut mask = tiny_skia::Mask::new(16, 12).unwrap();
        mask.data_mut().fill(128);
        let mut reference_mask = tiny_skia::Mask::new(32, 28).unwrap();
        reference_mask.data_mut().fill(128);
        let mut scratch = Vec::new();
        for clip in [
            None,
            Some(Rectangle::new(
                Point::new(-10.0, -10.0),
                crate::core::Size::new(50.0, 50.0),
            )),
        ] {
            for masked in [false, true] {
                for (x, y) in [(-1, 2), (3, -1), (-1, -1), (-5, -3)] {
                    let mut actual = tiny_skia::Pixmap::new(16, 12).unwrap();
                    let mut reference = tiny_skia::Pixmap::new(32, 28).unwrap();
                    reference.draw_pixmap(
                        x + 8,
                        y + 8,
                        source.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::identity(),
                        masked.then_some(&reference_mask),
                    );
                    let expected = reference
                        .clone_rect(tiny_skia::IntRect::from_xywh(8, 8, 16, 12).unwrap())
                        .unwrap();
                    draw_pixmap_clipped(
                        &mut actual.as_mut(),
                        x,
                        y,
                        source.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        masked.then_some(&mask),
                        clip,
                        &mut scratch,
                    );
                    assert!(
                        actual.data() == expected.data(),
                        "position={x},{y}, clip={clip:?}, masked={masked}",
                    );
                }
            }
        }
    }

    #[test]
    fn clipped_draw_matches_allocating_reference_with_transparency_and_opacity() {
        let mut source = tiny_skia::Pixmap::new(13, 9).unwrap();
        for (index, pixel) in source.pixels_mut().iter_mut().enumerate() {
            *pixel = tiny_skia::ColorU8::from_rgba(
                (index * 7) as u8,
                (index * 13) as u8,
                (index * 3) as u8,
                (index * 11) as u8,
            )
            .premultiply();
        }
        let mut scratch = Vec::new();
        for opacity in [0.0, 0.37, 1.0] {
            for (x, y) in [(-3, -2), (2, 3), (20, 20)] {
                for clip in [
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: 30.0,
                        height: 30.0,
                    },
                    Rectangle {
                        x: 4.2,
                        y: 4.3,
                        width: 7.5,
                        height: 5.2,
                    },
                    Rectangle {
                        x: 0.0,
                        y: 5.0,
                        width: 30.0,
                        height: 2.0,
                    },
                ] {
                    let mut expected = tiny_skia::Pixmap::new(30, 30).unwrap();
                    expected.fill(tiny_skia::Color::from_rgba8(31, 47, 63, 127));
                    let mut actual = expected.clone();
                    let paint = tiny_skia::PixmapPaint {
                        opacity,
                        ..Default::default()
                    };
                    if let Some(rect) =
                        clipped_source_rect(x, y, source.width(), source.height(), clip)
                    {
                        let cropped = source.clone_rect(rect).unwrap();
                        expected.as_mut().draw_pixmap(
                            x + rect.x(),
                            y + rect.y(),
                            cropped.as_ref(),
                            &paint,
                            tiny_skia::Transform::identity(),
                            None,
                        );
                    }
                    draw_pixmap_clipped(
                        &mut actual.as_mut(),
                        x,
                        y,
                        source.as_ref(),
                        &paint,
                        None,
                        Some(clip),
                        &mut scratch,
                    );
                    assert_eq!(
                        actual.data(),
                        expected.data(),
                        "opacity={opacity}, position={x},{y}, clip={clip:?}"
                    );
                }
            }
        }
        let capacity = scratch.capacity();
        assert!(capacity > 0);
        let mut target = tiny_skia::Pixmap::new(30, 30).unwrap();
        draw_pixmap_clipped(
            &mut target.as_mut(),
            0,
            0,
            source.as_ref(),
            &Default::default(),
            None,
            Some(Rectangle {
                x: 1.0,
                y: 1.0,
                width: 2.0,
                height: 2.0,
            }),
            &mut scratch,
        );
        assert_eq!(
            scratch.capacity(),
            capacity,
            "repeated clipping reuses allocation"
        );
    }

    #[test]
    fn crop_transparent_padding_keeps_only_nontransparent_pixels() {
        let mut pixmap = tiny_skia::Pixmap::new(5, 4).expect("create pixmap");
        let data = pixmap.data_mut();

        for (x, y) in [(2usize, 1usize), (3, 2)] {
            let offset = (y * 5 + x) * 4;
            data[offset] = 8;
            data[offset + 1] = 16;
            data[offset + 2] = 24;
            data[offset + 3] = 255;
        }

        let raster = crop_transparent_padding(&pixmap);

        assert_eq!(raster.left, 2);
        assert_eq!(raster.top, 1);
        assert_eq!(raster.width, 2);
        assert_eq!(raster.height, 2);
        assert_eq!(raster.pixels.len(), 4);
    }

    #[test]
    fn crop_transparent_padding_keeps_empty_rasters_drawable() {
        let pixmap = tiny_skia::Pixmap::new(5, 4).expect("create pixmap");
        let raster = crop_transparent_padding(&pixmap);

        assert_eq!(raster.left, 0);
        assert_eq!(raster.top, 0);
        assert_eq!(raster.width, 1);
        assert_eq!(raster.height, 1);
        assert_eq!(raster.pixels.len(), 1);
    }

    #[test]
    fn clipped_source_rect_keeps_visible_subrectangle() {
        let clip_bounds = Rectangle {
            x: 8.0,
            y: 0.0,
            width: 24.0,
            height: 18.0,
        };

        let source = clipped_source_rect(4, -4, 64, 32, clip_bounds).expect("visible source rect");

        assert_eq!(source.x(), 4);
        assert_eq!(source.y(), 4);
        assert_eq!(source.width(), 24);
        assert_eq!(source.height(), 18);
    }

    #[test]
    fn clipped_source_rect_keeps_full_pixmap_when_inside_clip() {
        let clip_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let source = clipped_source_rect(8, 12, 24, 18, clip_bounds).expect("visible source rect");

        assert_eq!(source.x(), 0);
        assert_eq!(source.y(), 0);
        assert_eq!(source.width(), 24);
        assert_eq!(source.height(), 18);
    }

    #[test]
    fn clipped_source_rect_drops_non_intersecting_pixmap() {
        let clip_bounds = Rectangle {
            x: 80.0,
            y: 80.0,
            width: 10.0,
            height: 10.0,
        };

        assert!(clipped_source_rect(8, 12, 24, 18, clip_bounds).is_none());
    }
}
