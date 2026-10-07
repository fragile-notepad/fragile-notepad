//! Font advances for canonical soft wrapping, using the editor's font routes.

use iced::advanced::graphics;
use iced::advanced::text::{self, Paragraph as _};
use iced::{Font, Pixels, Size, alignment};
use unicode_segmentation::UnicodeSegmentation;

use super::cjk::{CjkContext, cjk_runs};
use super::layout::visual_width_with_tab_width;
use super::widget::{EDITOR_FONT, EDITOR_TEXT_SHAPING, editor_font_runs_from_cjk_runs};

const MAX_SHAPED_SOURCE_BYTES: usize = 4 * 1024;

/// Integer metrics make wrap configuration stable across repeated UI updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrapMeasurement {
    character_width_milli: u32,
    font_size_milli: u32,
    hint_factor_milli: Option<u32>,
}

impl WrapMeasurement {
    pub fn new(character_width: f32, font_size: f32, hint_factor: Option<f32>) -> Self {
        let milli = |value: f32| (value.max(0.001) * 1000.0).round() as u32;
        Self {
            character_width_milli: milli(character_width),
            font_size_milli: milli(font_size),
            hint_factor_milli: hint_factor
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(milli),
        }
    }

    pub fn character_width(self) -> f32 {
        self.character_width_milli as f32 / 1000.0
    }

    pub fn font_size(self) -> f32 {
        self.font_size_milli as f32 / 1000.0
    }

    pub fn hint_factor(self) -> Option<f32> {
        self.hint_factor_milli.map(|value| value as f32 / 1000.0)
    }
}

/// Bounded shaping supplies initial advances; candidate fragments can then be
/// checked with their actual shaping at the selected row boundary.
pub(crate) struct MeasuredWrapLine<'a> {
    text: &'a str,
    tab_width: usize,
    line: usize,
    context: Option<&'a CjkContext>,
    config: WrapMeasurement,
    grapheme_widths: Vec<(usize, f32)>,
}

impl<'a> MeasuredWrapLine<'a> {
    pub(crate) fn new(
        text: &'a str,
        tab_width: usize,
        line: usize,
        context: Option<&'a CjkContext>,
        config: WrapMeasurement,
    ) -> Self {
        let mut measured = Self {
            text,
            tab_width: tab_width.max(1),
            line,
            context,
            config,
            grapheme_widths: Vec::new(),
        };
        for_each_chunk(
            text,
            0,
            text.len(),
            0,
            measured.tab_width,
            &mut |fragment, start, visual_column| {
                if fragment.len() > MAX_SHAPED_SOURCE_BYTES {
                    measured.grapheme_widths.push((
                        start + fragment.len(),
                        fallback_width(fragment, visual_column, measured.tab_width, config),
                    ));
                    return;
                }
                let shaped = shape(
                    fragment,
                    start,
                    visual_column,
                    line,
                    context,
                    measured.tab_width,
                    config,
                );
                measured.grapheme_widths.extend(
                    fragment
                        .grapheme_indices(true)
                        .zip(shaped.advances)
                        .map(|((byte, grapheme), width)| (start + byte + grapheme.len(), width)),
                );
            },
        );
        measured
    }

    pub(crate) fn grapheme_widths(&self) -> &[(usize, f32)] {
        &self.grapheme_widths
    }

    /// Measures a row independently while preserving logical tab stops and
    /// script cues outside it. Ordinary rows use one paragraph. Huge fragments
    /// use bounded chunks; a single oversized grapheme uses safe cell geometry.
    pub(crate) fn width(
        &self,
        start_byte: usize,
        end_byte: usize,
        start_visual_column: usize,
    ) -> f32 {
        let mut width = 0.0;
        self.for_each_chunk(
            start_byte,
            end_byte,
            start_visual_column,
            |fragment, start, visual| {
                width += if fragment.len() > MAX_SHAPED_SOURCE_BYTES {
                    fallback_width(fragment, visual, self.tab_width, self.config)
                } else {
                    shape(
                        fragment,
                        start,
                        visual,
                        self.line,
                        self.context,
                        self.tab_width,
                        self.config,
                    )
                    .width
                };
            },
        );
        width
    }

    fn for_each_chunk(
        &self,
        start_byte: usize,
        end_byte: usize,
        start_visual_column: usize,
        mut visit: impl FnMut(&str, usize, usize),
    ) {
        for_each_chunk(
            self.text,
            start_byte,
            end_byte,
            start_visual_column,
            self.tab_width,
            &mut visit,
        );
    }
}

fn for_each_chunk(
    text: &str,
    start_byte: usize,
    end_byte: usize,
    start_visual_column: usize,
    tab_width: usize,
    visit: &mut impl FnMut(&str, usize, usize),
) {
    let source = &text[start_byte..end_byte];
    let mut chunk_start = 0;
    let mut chunk_visual = start_visual_column;
    let mut visual = start_visual_column;
    for (byte, grapheme) in source.grapheme_indices(true) {
        if byte > chunk_start && byte + grapheme.len() - chunk_start > MAX_SHAPED_SOURCE_BYTES {
            visit(
                &source[chunk_start..byte],
                start_byte + chunk_start,
                chunk_visual,
            );
            chunk_start = byte;
            chunk_visual = visual;
        }
        for ch in grapheme.chars() {
            visual = visual.saturating_add(visual_width_with_tab_width(ch, visual, tab_width));
        }
    }
    if chunk_start < source.len() {
        visit(
            &source[chunk_start..],
            start_byte + chunk_start,
            chunk_visual,
        );
    }
}

fn fallback_width(
    text: &str,
    start_visual: usize,
    tab_width: usize,
    config: WrapMeasurement,
) -> f32 {
    let end_visual = text.chars().fold(start_visual, |column, ch| {
        column.saturating_add(visual_width_with_tab_width(ch, column, tab_width))
    });
    end_visual.saturating_sub(start_visual) as f32 * config.character_width()
}

struct ShapedWidths {
    advances: Vec<f32>,
    width: f32,
}

fn shape(
    source: &str,
    start_byte: usize,
    start_visual: usize,
    line: usize,
    context: Option<&CjkContext>,
    tab_width: usize,
    config: WrapMeasurement,
) -> ShapedWidths {
    let routes = context.map_or_else(
        || cjk_runs(source, None),
        |context| context.runs_for_fragment(line, start_byte, source),
    );
    let fonts = editor_font_runs_from_cjk_runs(source, &routes);
    let mut expanded = String::with_capacity(source.len());
    let mut source_to_expanded = vec![0; source.len() + 1];
    let mut expanded_to_grapheme = Vec::with_capacity(source.len());
    let mut visual = start_visual;
    let mut grapheme_count = 0;
    for (byte, grapheme) in source.grapheme_indices(true) {
        for (offset, ch) in grapheme.char_indices() {
            source_to_expanded[byte + offset] = expanded.len();
            let before = expanded.len();
            let columns = visual_width_with_tab_width(ch, visual, tab_width);
            if ch == '\t' {
                expanded.extend(std::iter::repeat_n(' ', columns));
            } else {
                expanded.push(ch);
            }
            expanded_to_grapheme
                .extend(std::iter::repeat_n(grapheme_count, expanded.len() - before));
            visual = visual.saturating_add(columns);
            source_to_expanded[byte + offset + ch.len_utf8()] = expanded.len();
        }
        grapheme_count += 1;
    }
    let spans: Vec<text::Span<'_, (), Font>> = fonts
        .iter()
        .map(|run| {
            text::Span::new(
                &expanded[source_to_expanded[run.byte_range.start]
                    ..source_to_expanded[run.byte_range.end]],
            )
            .font(run.font)
        })
        .collect();
    let paragraph = graphics::text::Paragraph::with_spans(text::Text {
        content: spans.as_slice(),
        bounds: Size::new(f32::INFINITY, config.font_size() * 1.25),
        size: Pixels(config.font_size()),
        line_height: text::LineHeight::Absolute(Pixels(config.font_size() * 1.25)),
        font: EDITOR_FONT,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: EDITOR_TEXT_SHAPING,
        wrapping: text::Wrapping::None,
        ellipsis: text::Ellipsis::None,
        hint_factor: config.hint_factor(),
    });
    let scale = paragraph.hint_factor().unwrap_or(1.0);
    // A glyph may cover several graphemes (a ligature), or a grapheme may use
    // several glyphs. Range additions distribute each advance in linear time.
    let mut changes = vec![0.0_f32; grapheme_count + 1];
    for glyph in paragraph.buffer().layout_runs().flat_map(|run| run.glyphs) {
        if glyph.start >= glyph.end || glyph.end > expanded_to_grapheme.len() {
            continue;
        }
        let first = expanded_to_grapheme[glyph.start];
        let last = expanded_to_grapheme[glyph.end - 1];
        let advance = glyph.w / scale / (last - first + 1) as f32;
        changes[first] += advance;
        changes[last + 1] -= advance;
    }
    let mut width = 0.0_f32;
    let advances = changes[..grapheme_count]
        .iter()
        .map(|change| {
            width += change;
            width.max(0.0)
        })
        .collect();
    ShapedWidths {
        advances,
        width: paragraph.min_bounds().width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorBuffer;

    #[test]
    fn unicode_advances_follow_font_size_instead_of_configured_cell_width() {
        let source = "天地宇宙日月";
        let buffer = EditorBuffer::from_text(source);
        let context = CjkContext::from_buffer(&buffer);
        let config = WrapMeasurement::new(100.0, 24.0, None);
        let line = MeasuredWrapLine::new(source, 4, 0, Some(&context), config);
        assert_eq!(line.grapheme_widths().len(), 6);
        let advances: f32 = line.grapheme_widths().iter().map(|(_, width)| width).sum();
        assert!((advances - line.width(0, source.len(), 0)).abs() < 0.01);
        assert!(advances > 0.0 && advances < 6.0 * 100.0);
        assert_eq!(line.grapheme_widths().last().unwrap().0, source.len());
    }

    #[test]
    fn fragments_keep_logical_tab_stops_and_language_routes() {
        let source = "かな漢\t字";
        let buffer = EditorBuffer::from_text(source);
        let context = CjkContext::from_buffer(&buffer);
        let line = MeasuredWrapLine::new(
            source,
            4,
            0,
            Some(&context),
            WrapMeasurement::new(8.8, 16.0, None),
        );
        let tab = source.find('\t').unwrap();
        let two_spaces = line.width(tab, source.len(), 6);
        let four_spaces = line.width(tab, source.len(), 4);
        assert!(four_spaces > two_spaces);
        let prefix = line.width(0, tab, 0);
        assert!((prefix + two_spaces - line.width(0, source.len(), 0)).abs() < 0.01);
    }

    #[test]
    fn combining_and_joined_clusters_remain_single_graphemes() {
        let source = "a\u{301}👩\u{200d}💻ffi";
        let line = MeasuredWrapLine::new(source, 4, 0, None, WrapMeasurement::new(8.8, 16.0, None));
        let ends: Vec<_> = source
            .grapheme_indices(true)
            .map(|(byte, grapheme)| byte + grapheme.len())
            .collect();
        assert_eq!(
            line.grapheme_widths()
                .iter()
                .map(|(end, _)| *end)
                .collect::<Vec<_>>(),
            ends
        );
        assert!(
            line.grapheme_widths()
                .iter()
                .all(|(_, width)| width.is_finite() && *width >= 0.0)
        );
        let advances: f32 = line.grapheme_widths().iter().map(|(_, width)| width).sum();
        assert!((advances - line.width(0, source.len(), 0)).abs() < 0.01);
    }

    #[test]
    fn long_unicode_lines_and_oversized_graphemes_have_bounded_shaping() {
        let source = "天地".repeat(4000);
        let line =
            MeasuredWrapLine::new(&source, 4, 0, None, WrapMeasurement::new(8.8, 16.0, None));
        assert_eq!(line.grapheme_widths().len(), 8000);
        let advances: f32 = line.grapheme_widths().iter().map(|(_, width)| width).sum();
        assert!((advances - line.width(0, source.len(), 0)).abs() < 1.0);
        let cluster = format!("a{}", "\u{301}".repeat(3000));
        let line =
            MeasuredWrapLine::new(&cluster, 4, 0, None, WrapMeasurement::new(8.8, 16.0, None));
        assert_eq!(line.grapheme_widths(), [(cluster.len(), 8.8)]);
        assert_eq!(line.width(0, cluster.len(), 0), 8.8);
    }
}
