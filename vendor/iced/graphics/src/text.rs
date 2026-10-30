//! Draw text.
pub mod cache;
pub mod editor;
pub mod paragraph;

#[cfg(any(target_os = "linux", test))]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

pub use cache::Cache;
pub use editor::Editor;
pub use paragraph::Paragraph;

pub use cosmic_text;

use crate::core::alignment;
use crate::core::font::{self, Font};
use crate::core::text::{Alignment, Ellipsis, Shaping, Wrapping};
use crate::core::{Color, Pixels, Point, Rectangle, Size, Transformation, Vector};

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock, RwLock, Weak};

/// A text primitive.
#[derive(Debug, Clone, PartialEq)]
pub enum Text {
    /// A paragraph.
    #[allow(missing_docs)]
    Paragraph {
        paragraph: paragraph::Weak,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    },
    /// An editor.
    #[allow(missing_docs)]
    Editor {
        editor: editor::Weak,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    },
    /// Some cached text.
    Cached {
        /// The contents of the text.
        content: String,
        /// The bounds of the text.
        bounds: Rectangle,
        /// The color of the text.
        color: Color,
        /// The size of the text in logical pixels.
        size: Pixels,
        /// The line height of the text.
        line_height: Pixels,
        /// The font of the text.
        font: Font,
        /// The horizontal alignment of the text.
        align_x: Alignment,
        /// The vertical alignment of the text.
        align_y: alignment::Vertical,
        /// The shaping strategy of the text.
        shaping: Shaping,
        /// The wrapping strategy of the text.
        wrapping: Wrapping,
        /// The ellipsis strategy of the text.
        ellipsis: Ellipsis,
        /// The clip bounds of the text.
        clip_bounds: Rectangle,
    },
    /// Some raw text.
    #[allow(missing_docs)]
    Raw {
        raw: Raw,
        transformation: Transformation,
    },
}

impl Text {
    /// Returns the visible bounds of the [`Text`].
    pub fn visible_bounds(&self) -> Option<Rectangle> {
        match self {
            Text::Paragraph {
                position,
                paragraph,
                clip_bounds,
                transformation,
                ..
            } => Rectangle::new(*position, paragraph.min_bounds)
                .intersection(clip_bounds)
                .map(|bounds| bounds * *transformation),
            Text::Editor {
                editor,
                position,
                clip_bounds,
                transformation,
                ..
            } => Rectangle::new(*position, editor.bounds)
                .intersection(clip_bounds)
                .map(|bounds| bounds * *transformation),
            Text::Cached {
                bounds,
                clip_bounds,
                ..
            } => bounds.intersection(clip_bounds),
            Text::Raw { raw, .. } => Some(raw.clip_bounds),
        }
    }
}

/// Detects a consistent text-only scroll delta between two ordered text lists.
pub fn scroll_delta(
    previous: &[&Text],
    current: &[&Text],
    max_mismatched_items: usize,
) -> Option<Vector> {
    if previous.len() < 2 || current.len() < 2 {
        return None;
    }
    // In particular, repeated identical rows must not manufacture a scroll
    // from an otherwise unchanged scene.
    if previous == current {
        return None;
    }

    let shared_len = previous.len().min(current.len());
    let required_matches = shared_len
        .saturating_sub(max_mismatched_items)
        .max(shared_len.saturating_mul(2) / 3)
        .max(2);
    let mut best = None;
    // Scroll copying is optional. Bound detection work and let the normal
    // damage path redraw when a scene cannot be matched cheaply.
    let mut remaining_comparisons = 65_536usize;

    for previous_start in 0..previous.len() {
        for current_start in 0..current.len() {
            let possible = (previous.len() - previous_start).min(current.len() - current_start);
            let minimum = best.map_or(required_matches, |(matches, _)| matches + 1);
            if possible < minimum {
                break;
            }
            remaining_comparisons = remaining_comparisons.checked_sub(1)?;
            let Some(delta) = delta(previous[previous_start], current[current_start]) else {
                continue;
            };

            if delta.x.abs() > 0.5 || delta.y.abs() < 0.5 {
                continue;
            }

            let mut matches = 0usize;
            while previous_start + matches < previous.len()
                && current_start + matches < current.len()
            {
                remaining_comparisons = remaining_comparisons.checked_sub(1)?;
                if !has_delta(
                    previous[previous_start + matches],
                    current[current_start + matches],
                    delta,
                ) {
                    break;
                }
                matches += 1;
            }

            if matches >= required_matches {
                match best {
                    Some((best_matches, _)) if best_matches >= matches => {}
                    _ => best = Some((matches, delta)),
                }
            }
        }
    }

    best.map(|(_, delta)| delta)
}

fn has_delta(previous: &Text, current: &Text, delta: Vector) -> bool {
    let Some(candidate) = self::delta(previous, current) else {
        return false;
    };

    (candidate.x - delta.x).abs() <= 0.5 && (candidate.y - delta.y).abs() <= 0.5
}

fn delta(previous: &Text, current: &Text) -> Option<Vector> {
    if !same_content(previous, current) {
        return None;
    }

    let previous = position(previous)?;
    let current = position(current)?;

    Some(Vector::new(current.x - previous.x, current.y - previous.y))
}

fn position(text: &Text) -> Option<Point> {
    match text {
        Text::Paragraph {
            position,
            transformation,
            ..
        } => Some(*position * *transformation),
        Text::Cached { bounds, .. } => Some(bounds.position()),
        Text::Raw {
            raw,
            transformation,
        } => Some(raw.position * *transformation),
        Text::Editor {
            position,
            transformation,
            ..
        } => Some(*position * *transformation),
    }
}

fn same_content(previous: &Text, current: &Text) -> bool {
    match (previous, current) {
        (
            Text::Paragraph {
                paragraph: paragraph_a,
                color: color_a,
                clip_bounds: clip_bounds_a,
                transformation: transformation_a,
                ..
            },
            Text::Paragraph {
                paragraph: paragraph_b,
                color: color_b,
                clip_bounds: clip_bounds_b,
                transformation: transformation_b,
                ..
            },
        ) => {
            paragraph_a == paragraph_b
                && color_a == color_b
                && clip_bounds_a == clip_bounds_b
                && transformation_a == transformation_b
        }
        (
            Text::Cached {
                content: content_a,
                bounds: bounds_a,
                color: color_a,
                size: size_a,
                line_height: line_height_a,
                font: font_a,
                align_x: align_x_a,
                align_y: align_y_a,
                shaping: shaping_a,
                wrapping: wrapping_a,
                ellipsis: ellipsis_a,
                clip_bounds: clip_bounds_a,
            },
            Text::Cached {
                content: content_b,
                bounds: bounds_b,
                color: color_b,
                size: size_b,
                line_height: line_height_b,
                font: font_b,
                align_x: align_x_b,
                align_y: align_y_b,
                shaping: shaping_b,
                wrapping: wrapping_b,
                ellipsis: ellipsis_b,
                clip_bounds: clip_bounds_b,
            },
        ) => {
            content_a == content_b
                && bounds_a.size() == bounds_b.size()
                && color_a == color_b
                && size_a == size_b
                && line_height_a == line_height_b
                && font_a == font_b
                && align_x_a == align_x_b
                && align_y_a == align_y_b
                && shaping_a == shaping_b
                && wrapping_a == wrapping_b
                && ellipsis_a == ellipsis_b
                && clip_bounds_a == clip_bounds_b
        }
        _ => false,
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    fn row(content: usize, y: f32) -> Text {
        Text::Cached {
            content: content.to_string(),
            bounds: Rectangle::new(Point::new(0.0, y), Size::new(80.0, 18.0)),
            color: Color::BLACK,
            size: Pixels(14.0),
            line_height: Pixels(18.0),
            font: Font::MONOSPACE,
            align_x: Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping: Shaping::Basic,
            wrapping: Wrapping::None,
            ellipsis: Ellipsis::None,
            clip_bounds: Rectangle::new(Point::ORIGIN, Size::new(200.0, 2000.0)),
        }
    }

    fn reference(previous: &[&Text], current: &[&Text], allowance: usize) -> Option<Vector> {
        let shared = previous.len().min(current.len());
        let required = shared.saturating_sub(allowance).max(shared * 2 / 3).max(2);
        let mut best = None;
        for p in 0..previous.len() {
            for c in 0..current.len() {
                let Some(offset) = delta(previous[p], current[c]) else {
                    continue;
                };
                if offset.x.abs() > 0.5 || offset.y.abs() < 0.5 {
                    continue;
                }
                let count = previous[p..]
                    .iter()
                    .zip(&current[c..])
                    .take_while(|(a, b)| has_delta(a, b, offset))
                    .count();
                if count >= required && best.is_none_or(|(n, _)| count > n) {
                    best = Some((count, offset));
                }
            }
        }
        best.map(|(_, offset)| offset)
    }

    #[test]
    fn optimized_scroll_matches_reference_for_changed_scenes() {
        for count in [3, 8, 20, 70] {
            for repeats in [1, 2, 7, 1000] {
                for offset in [-3, -1, 1, 3] {
                    let previous = (0..count)
                        .map(|i| row(i % repeats, i as f32 * 18.0))
                        .collect::<Vec<_>>();
                    let current = (0..count)
                        .map(|i| row(i % repeats, i as f32 * 18.0 + offset as f32 * 18.0))
                        .collect::<Vec<_>>();
                    let a = previous.iter().collect::<Vec<_>>();
                    let b = current.iter().collect::<Vec<_>>();
                    assert_eq!(scroll_delta(&a, &b, 10), reference(&a, &b, 10));
                }
            }
        }
    }

    #[test]
    fn unchanged_repeated_rows_do_not_produce_scroll() {
        let rows = (0..100)
            .map(|i| row(0, i as f32 * 18.0))
            .collect::<Vec<_>>();
        let rows = rows.iter().collect::<Vec<_>>();
        assert_eq!(scroll_delta(&rows, &rows, 10), None);
    }

    #[test]
    fn scroll_matches_reference_with_entering_rows_edits_and_fractional_positions() {
        for count in [8, 35, 90] {
            for shift in [-3i32, -1, 1, 3] {
                for changed in [false, true] {
                    let previous = (0..count)
                        .map(|i| row(i, i as f32 * 18.0))
                        .collect::<Vec<_>>();
                    let current = (0..count)
                        .map(|i| {
                            let content = usize::try_from(i as i32 + shift).unwrap_or(5001);
                            let content = if changed && i == count / 2 {
                                5000
                            } else {
                                content
                            };
                            row(
                                content,
                                i as f32 * 18.0 + if i % 2 == 0 { 0.25 } else { 0.0 },
                            )
                        })
                        .collect::<Vec<_>>();
                    let a = previous.iter().collect::<Vec<_>>();
                    let b = current.iter().collect::<Vec<_>>();
                    assert_eq!(scroll_delta(&a, &b, 10), reference(&a, &b, 10));
                }
            }
        }
    }

    #[test]
    fn ambiguous_large_scene_falls_back_without_unbounded_search() {
        let previous = (0..1000)
            .map(|i| row(i, i as f32 * 18.0))
            .collect::<Vec<_>>();
        let current = (0..1000)
            .map(|i| row(i + 1000, i as f32 * 18.0))
            .collect::<Vec<_>>();
        assert_eq!(
            scroll_delta(
                &previous.iter().collect::<Vec<_>>(),
                &current.iter().collect::<Vec<_>>(),
                1000
            ),
            None
        );
    }
}

/// The regular variant of the [Fira Sans] font.
///
/// It is loaded as part of the default fonts when the `fira-sans`
/// feature is enabled.
///
/// [Fira Sans]: https://mozilla.github.io/Fira/
#[cfg(feature = "fira-sans")]
pub const FIRA_SANS_REGULAR: &[u8] = include_bytes!("../fonts/FiraSans-Regular.ttf").as_slice();

/// Returns the global [`FontSystem`].
pub fn font_system() -> &'static RwLock<FontSystem> {
    static FONT_SYSTEM: OnceLock<RwLock<FontSystem>> = OnceLock::new();

    FONT_SYSTEM.get_or_init(|| {
        #[allow(unused_mut)]
        let mut raw = cosmic_text::FontSystem::new_with_fonts([
            cosmic_text::fontdb::Source::Binary(Arc::new(
                include_bytes!("../fonts/Iced-Icons.ttf").as_slice(),
            )),
            #[cfg(feature = "fira-sans")]
            cosmic_text::fontdb::Source::Binary(Arc::new(
                include_bytes!("../fonts/FiraSans-Regular.ttf").as_slice(),
            )),
        ]);

        #[cfg(feature = "fira-sans")]
        raw.db_mut().set_sans_serif_family("Fira Sans");

        #[cfg(target_os = "macos")]
        macos::configure(&mut raw);

        #[cfg(target_os = "linux")]
        linux::configure(raw.db_mut());

        #[cfg(target_os = "windows")]
        {
            #[cfg(not(feature = "fira-sans"))]
            raw.db_mut().set_sans_serif_family("Segoe UI");
            raw.db_mut().set_serif_family("Times New Roman");
            raw.db_mut().set_monospace_family("Consolas");
        }

        RwLock::new(FontSystem {
            raw,
            loaded_fonts: HashSet::new(),
            version: Version::default(),
        })
    })
}

/// A set of system fonts.
pub struct FontSystem {
    raw: cosmic_text::FontSystem,
    loaded_fonts: HashSet<usize>,
    version: Version,
}

impl FontSystem {
    /// Returns the raw [`cosmic_text::FontSystem`].
    pub fn raw(&mut self) -> &mut cosmic_text::FontSystem {
        &mut self.raw
    }

    /// Loads a font from its bytes.
    pub fn load_font(&mut self, bytes: Cow<'static, [u8]>) {
        if let Cow::Borrowed(bytes) = bytes {
            let address = bytes.as_ptr() as usize;

            if !self.loaded_fonts.insert(address) {
                return;
            }
        }

        let _ = self
            .raw
            .db_mut()
            .load_font_source(cosmic_text::fontdb::Source::Binary(Arc::new(
                bytes.into_owned(),
            )));

        self.version = Version(self.version.0 + 1);
    }

    /// Returns an iterator over the family names of all font faces
    /// in the font database.
    pub fn families(&self) -> impl Iterator<Item = &str> {
        self.raw
            .db()
            .faces()
            .filter_map(|face| face.families.first())
            .map(|(name, _)| name.as_str())
    }

    /// Returns the current [`Version`] of the [`FontSystem`].
    ///
    /// Loading a font will increase the version of a [`FontSystem`].
    pub fn version(&self) -> Version {
        self.version
    }
}

/// A version number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Version(u32);

/// A weak reference to a [`cosmic_text::Buffer`] that can be drawn.
#[derive(Debug, Clone)]
pub struct Raw {
    /// A weak reference to a [`cosmic_text::Buffer`].
    pub buffer: Weak<cosmic_text::Buffer>,
    /// The position of the text.
    pub position: Point,
    /// The color of the text.
    pub color: Color,
    /// The clip bounds of the text.
    pub clip_bounds: Rectangle,
}

impl PartialEq for Raw {
    fn eq(&self, _other: &Self) -> bool {
        // TODO: There is no proper way to compare raw buffers
        // For now, no two instances of `Raw` text will be equal.
        // This should be fine, but could trigger unnecessary redraws
        // in the future.
        false
    }
}

/// Measures the dimensions of the given [`cosmic_text::Buffer`].
pub fn measure(buffer: &cosmic_text::Buffer) -> (Size, bool) {
    let (width, height, has_rtl) =
        buffer
            .layout_runs()
            .fold((0.0, 0.0, false), |(width, height, has_rtl), run| {
                (
                    run.line_w.max(width),
                    height + run.line_height,
                    has_rtl || run.rtl,
                )
            });

    (Size::new(width, height), has_rtl)
}

/// Aligns the given [`cosmic_text::Buffer`] with the given [`Alignment`]
/// and returns its minimum [`Size`].
pub fn align(
    buffer: &mut cosmic_text::Buffer,
    font_system: &mut cosmic_text::FontSystem,
    alignment: Alignment,
) -> Size {
    let (min_bounds, has_rtl) = measure(buffer);
    let mut needs_relayout = has_rtl;

    if let Some(align) = to_align(alignment) {
        let has_multiple_lines = buffer.lines.len() > 1
            || buffer
                .lines
                .first()
                .is_some_and(|line| line.layout_opt().is_some_and(|layout| layout.len() > 1));

        if has_multiple_lines {
            for line in &mut buffer.lines {
                let _ = line.set_align(Some(align));
            }

            needs_relayout = true;
        } else if let Some(line) = buffer.lines.first_mut() {
            needs_relayout |= line.set_align(None);
        }
    }

    // TODO: Avoid relayout with some changes to `cosmic-text` (?)
    if needs_relayout {
        log::trace!("Relayouting paragraph...");

        buffer.set_size(Some(min_bounds.width), Some(min_bounds.height));
        buffer.shape_until_scroll(font_system, false);
    }

    min_bounds
}

/// Returns the attributes of the given [`Font`].
pub fn to_attributes(font: Font) -> cosmic_text::Attrs<'static> {
    let weight = to_weight(font.weight);
    #[cfg(target_os = "macos")]
    let weight = macos::ui_weight(font.family, weight);

    cosmic_text::Attrs::new()
        .family(to_family(font.family))
        .weight(weight)
        .stretch(to_stretch(font.stretch))
        .style(to_style(font.style))
}

fn to_family(family: font::Family) -> cosmic_text::Family<'static> {
    match family {
        font::Family::Name(name) => cosmic_text::Family::Name(name),
        font::Family::SansSerif => cosmic_text::Family::SansSerif,
        font::Family::Serif => cosmic_text::Family::Serif,
        font::Family::Cursive => cosmic_text::Family::Cursive,
        font::Family::Fantasy => cosmic_text::Family::Fantasy,
        font::Family::Monospace => cosmic_text::Family::Monospace,
    }
}

fn to_weight(weight: font::Weight) -> cosmic_text::Weight {
    match weight {
        font::Weight::Thin => cosmic_text::Weight::THIN,
        font::Weight::ExtraLight => cosmic_text::Weight::EXTRA_LIGHT,
        font::Weight::Light => cosmic_text::Weight::LIGHT,
        font::Weight::Normal => cosmic_text::Weight::NORMAL,
        font::Weight::Medium => cosmic_text::Weight::MEDIUM,
        font::Weight::Semibold => cosmic_text::Weight::SEMIBOLD,
        font::Weight::Bold => cosmic_text::Weight::BOLD,
        font::Weight::ExtraBold => cosmic_text::Weight::EXTRA_BOLD,
        font::Weight::Black => cosmic_text::Weight::BLACK,
    }
}

fn to_stretch(stretch: font::Stretch) -> cosmic_text::Stretch {
    match stretch {
        font::Stretch::UltraCondensed => cosmic_text::Stretch::UltraCondensed,
        font::Stretch::ExtraCondensed => cosmic_text::Stretch::ExtraCondensed,
        font::Stretch::Condensed => cosmic_text::Stretch::Condensed,
        font::Stretch::SemiCondensed => cosmic_text::Stretch::SemiCondensed,
        font::Stretch::Normal => cosmic_text::Stretch::Normal,
        font::Stretch::SemiExpanded => cosmic_text::Stretch::SemiExpanded,
        font::Stretch::Expanded => cosmic_text::Stretch::Expanded,
        font::Stretch::ExtraExpanded => cosmic_text::Stretch::ExtraExpanded,
        font::Stretch::UltraExpanded => cosmic_text::Stretch::UltraExpanded,
    }
}

fn to_style(style: font::Style) -> cosmic_text::Style {
    match style {
        font::Style::Normal => cosmic_text::Style::Normal,
        font::Style::Italic => cosmic_text::Style::Italic,
        font::Style::Oblique => cosmic_text::Style::Oblique,
    }
}

fn to_align(alignment: Alignment) -> Option<cosmic_text::Align> {
    match alignment {
        Alignment::Default => None,
        Alignment::Left => Some(cosmic_text::Align::Left),
        Alignment::Center => Some(cosmic_text::Align::Center),
        Alignment::Right => Some(cosmic_text::Align::Right),
        Alignment::Justified => Some(cosmic_text::Align::Justified),
    }
}

/// Converts some [`Shaping`] strategy to a [`cosmic_text::Shaping`] strategy.
pub fn to_shaping(shaping: Shaping, text: &str) -> cosmic_text::Shaping {
    match shaping {
        Shaping::Auto => {
            // San Francisco's proportional and variable faces need shaping
            // for kerning and weight-correct advances, including ASCII UI.
            if text.is_ascii() && !cfg!(target_os = "macos") {
                cosmic_text::Shaping::Basic
            } else {
                cosmic_text::Shaping::Advanced
            }
        }
        Shaping::Basic => cosmic_text::Shaping::Basic,
        Shaping::Advanced => cosmic_text::Shaping::Advanced,
    }
}

/// Converts some [`Wrapping`] strategy to a [`cosmic_text::Wrap`] strategy.
pub fn to_wrap(wrapping: Wrapping) -> cosmic_text::Wrap {
    match wrapping {
        Wrapping::None => cosmic_text::Wrap::None,
        Wrapping::Word => cosmic_text::Wrap::Word,
        Wrapping::Glyph => cosmic_text::Wrap::Glyph,
        Wrapping::WordOrGlyph => cosmic_text::Wrap::WordOrGlyph,
    }
}

/// Converts some [`Ellipsis`] strategy to a [`cosmic_text::Ellipsize`] strategy.
pub fn to_ellipsize(ellipsis: Ellipsis, max_height: f32) -> cosmic_text::Ellipsize {
    let limit = cosmic_text::EllipsizeHeightLimit::Height(max_height);

    match ellipsis {
        Ellipsis::None => cosmic_text::Ellipsize::None,
        Ellipsis::Start => cosmic_text::Ellipsize::Start(limit),
        Ellipsis::Middle => cosmic_text::Ellipsize::Middle(limit),
        Ellipsis::End => cosmic_text::Ellipsize::End(limit),
    }
}

/// Converts some [`Color`] to a [`cosmic_text::Color`].
pub fn to_color(color: Color) -> cosmic_text::Color {
    let [r, g, b, a] = color.into_rgba8();

    cosmic_text::Color::rgba(r, g, b, a)
}

/// Returns the ideal hint factor given the size and scale factor of some text.
pub fn hint_factor(_size: Pixels, _scale_factor: Option<f32>) -> Option<f32> {
    // TODO: Fix hinting in `cosmic-text`
    // const MAX_HINTING_SIZE: f32 = 18.0;

    // let hint_factor = scale_factor?;

    // if size.0 * hint_factor < MAX_HINTING_SIZE {
    //     Some(hint_factor)
    // } else {
    //     None
    // }

    None // Disable all text hinting for now
}

/// A text renderer coupled to `iced_graphics`.
pub trait Renderer {
    /// Draws the given [`Raw`] text.
    fn fill_raw(&mut self, raw: Raw);
}
