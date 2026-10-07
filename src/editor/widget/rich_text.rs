use iced::advanced::text::{self, Paragraph};
use iced::{Color, Font, Pixels, Point, Rectangle, Size, alignment};
use std::ops::Range;

use crate::editor::decoration::DecorationModel;
use crate::editor::layout::{
    EditorMetrics, byte_column_for, byte_column_for_with_offset, visual_column_for,
    visual_column_for_with_offset, visual_width_with_tab_width,
};
use crate::editor::render::RowRenderPlan;

use super::cache::{RichParagraphCache, SyntaxSpanKey};
use super::font::{
    EDITOR_FONT, EDITOR_TEXT_SHAPING, EditorFontRun, clipped_font_runs, remap_font_runs,
};
use super::style::EditorStyle;

const MAX_SYNTAX_SPANS_PER_ROW: usize = 256;
const LONG_LINE_VISIBLE_MARGIN_COLUMNS: usize = 8;
const LONG_STYLE_RUN_SUBDIVISION_COLUMNS: usize = 100;
const MAX_UNCLIPPED_UNICODE_BYTES: usize = 4 * 1024;

pub(super) fn can_batch_fast_text(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii() && byte != b'\t')
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_row_text<Renderer>(
    renderer: &mut Renderer,
    row: &RowRenderPlan,
    text_origin_x: f32,
    baseline_y: f32,
    metrics: EditorMetrics,
    decorations: &DecorationModel,
    style: EditorStyle,
    clip_bounds: Rectangle,
    frame_id: u64,
    rich_paragraphs: &mut RichParagraphCache<Renderer::Paragraph>,
    font_runs: &[EditorFontRun],
    draw_plain_text: impl FnOnce(
        &mut Renderer,
        String,
        Point,
        Size,
        Color,
        text::Alignment,
        EditorMetrics,
        Rectangle,
    ),
) where
    Renderer: text::Renderer<Font = Font>,
{
    if contains_unsafe_render_controls(&row.text) {
        draw_clipped_plain_text(
            renderer,
            row.text.clone(),
            text_origin_x,
            baseline_y,
            metrics,
            clip_bounds,
            style.syntax_fallback_text,
            draw_plain_text,
        );
        return;
    }

    if !row.text.is_ascii() {
        draw_adaptive_row_text(
            renderer,
            row,
            text_origin_x,
            baseline_y,
            metrics,
            decorations,
            style,
            clip_bounds,
            frame_id,
            rich_paragraphs,
            font_runs,
        );
        return;
    }

    if (row.syntax_spans.is_empty() || row.syntax_spans.len() > MAX_SYNTAX_SPANS_PER_ROW)
        && let Some((text, offset)) = visible_expanded_tabs(
            &row.text,
            decorations.settings.indent_width,
            row.start_visual_column,
            text_origin_x,
            metrics,
            clip_bounds,
        )
    {
        let width = visual_column_for(&text, text.len(), 1);
        draw_plain_text(
            renderer,
            text,
            Point::new(
                text_origin_x + offset as f32 * metrics.character_width,
                baseline_y,
            ),
            Size::new(
                (width as f32 * metrics.character_width).max(metrics.character_width),
                metrics.line_height,
            ),
            style.syntax_fallback_text,
            text::Alignment::Left,
            metrics,
            clip_bounds,
        );
        return;
    }

    if let Some(expanded) = expand_tabs_for_rendering_with_offset(
        &row.text,
        decorations.settings.indent_width,
        row.start_visual_column,
    ) {
        if row.syntax_spans.is_empty() || row.syntax_spans.len() > MAX_SYNTAX_SPANS_PER_ROW {
            draw_clipped_plain_text(
                renderer,
                expanded.text,
                text_origin_x,
                baseline_y,
                metrics,
                clip_bounds,
                style.syntax_fallback_text,
                draw_plain_text,
            );
            return;
        }

        let expanded_row = RowRenderPlan {
            text: expanded.text,
            syntax_spans: remap_syntax_spans(&row.syntax_spans, &expanded.byte_offsets),
            ..row.clone()
        };

        if expanded_row.syntax_spans.is_empty() || !syntax_spans_are_valid(&expanded_row) {
            draw_clipped_plain_text(
                renderer,
                expanded_row.text,
                text_origin_x,
                baseline_y,
                metrics,
                clip_bounds,
                style.syntax_fallback_text,
                draw_plain_text,
            );
            return;
        }

        draw_rich_row_text(
            renderer,
            &expanded_row,
            Point::new(text_origin_x, baseline_y),
            metrics.line_height,
            metrics,
            style,
            clip_bounds,
            frame_id,
            rich_paragraphs,
            draw_plain_text,
        );
        return;
    }

    if row.syntax_spans.is_empty() || row.syntax_spans.len() > MAX_SYNTAX_SPANS_PER_ROW {
        draw_clipped_plain_text(
            renderer,
            row.text.clone(),
            text_origin_x,
            baseline_y,
            metrics,
            clip_bounds,
            style.syntax_fallback_text,
            draw_plain_text,
        );
        return;
    }

    draw_rich_row_text(
        renderer,
        row,
        Point::new(text_origin_x, baseline_y),
        metrics.line_height,
        metrics,
        style,
        clip_bounds,
        frame_id,
        rich_paragraphs,
        draw_plain_text,
    );
}

/// Shape font and color runs together so fallback glyph advances are identical
/// to those used for caret/selection geometry. Ordinary Unicode lines retain
/// their real origin; column-based cropping is reserved for very long lines.
#[allow(clippy::too_many_arguments)]
fn draw_adaptive_row_text<Renderer>(
    renderer: &mut Renderer,
    row: &RowRenderPlan,
    text_origin_x: f32,
    baseline_y: f32,
    metrics: EditorMetrics,
    decorations: &DecorationModel,
    style: EditorStyle,
    clip_bounds: Rectangle,
    frame_id: u64,
    cache: &mut RichParagraphCache<Renderer::Paragraph>,
    font_runs: &[EditorFontRun],
) where
    Renderer: text::Renderer<Font = Font>,
{
    let source_is_long = row.text.len() > MAX_UNCLIPPED_UNICODE_BYTES;
    let clipped;
    let clipped_fonts;
    let mut fonts = font_runs;
    let mut text_origin_x = text_origin_x;
    // A minified/tabbed Unicode line can be megabytes long. Restrict the
    // source before allocating an expanded string and its syntax byte map.
    let row = if source_is_long
        && let Some((range, offset)) = visible_tab_source_range(
            &row.text,
            decorations.settings.indent_width,
            row.start_visual_column,
            text_origin_x,
            metrics,
            clip_bounds,
        ) {
        clipped_fonts = clipped_font_runs(fonts, range.clone());
        fonts = clipped_fonts.as_slice();
        let syntax_spans = row
            .syntax_spans
            .iter()
            .filter_map(|span| {
                let start = span.range.start.max(range.start);
                let end = span.range.end.min(range.end);
                (start < end && row.text.is_char_boundary(start) && row.text.is_char_boundary(end))
                    .then_some(crate::editor::render::SyntaxRenderSpan {
                        range: start - range.start..end - range.start,
                        color: span.color,
                    })
            })
            .collect();
        clipped = RowRenderPlan {
            text: row.text[range].to_owned(),
            syntax_spans,
            start_visual_column: row.start_visual_column + offset,
            ..row.clone()
        };
        text_origin_x += offset as f32 * metrics.character_width;
        &clipped
    } else {
        row
    };
    let expanded;
    let expanded_fonts;
    let row = if let Some(tabs) = expand_tabs_for_rendering_with_offset(
        &row.text,
        decorations.settings.indent_width,
        row.start_visual_column,
    ) {
        expanded_fonts = remap_font_runs(fonts, &tabs.byte_offsets);
        fonts = expanded_fonts.as_slice();
        expanded = RowRenderPlan {
            text: tabs.text,
            syntax_spans: remap_syntax_spans(&row.syntax_spans, &tabs.byte_offsets),
            ..row.clone()
        };
        &expanded
    } else {
        row
    };
    let range = if source_is_long {
        if syntax_spans_are_valid(row) {
            visible_styled_text_range(row, text_origin_x, metrics, clip_bounds)
        } else {
            visible_text_range(&row.text, text_origin_x, metrics, clip_bounds)
        }
    } else {
        0..row.text.len()
    };
    let content = &row.text[range.clone()];
    let fonts = clipped_font_runs(fonts, range.clone());
    let colors = adaptive_span_keys(row, range.clone(), &fonts, style.syntax_fallback_text);
    let bounds = Size::new(f32::INFINITY, metrics.line_height);
    let size = Pixels((metrics.line_height / 1.25).max(8.0));
    let scale = renderer.scale_factor();
    let paragraph = cache.get_or_insert_with_fonts(
        row.visible_row,
        content,
        &colors,
        &fonts,
        range.start,
        bounds,
        size,
        metrics.line_height,
        scale,
        frame_id,
        || {
            let spans: Vec<text::Span<'_, (), Font>> = colors
                .iter()
                .map(|color| {
                    let font = fonts
                        .iter()
                        .find(|font| font.byte_range.contains(&color.start))
                        .map_or(EDITOR_FONT, |font| font.font);
                    text::Span::new(&content[color.start..color.end])
                        .font(font)
                        .color_maybe(color.color)
                })
                .collect();
            Renderer::Paragraph::with_spans(text::Text {
                content: spans.as_slice(),
                bounds,
                size,
                line_height: text::LineHeight::Absolute(Pixels(metrics.line_height)),
                font: EDITOR_FONT,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: EDITOR_TEXT_SHAPING,
                wrapping: text::Wrapping::None,
                ellipsis: text::Ellipsis::None,
                hint_factor: scale,
            })
        },
    );
    let offset = visual_column_for(&row.text, range.start, 1) as f32 * metrics.character_width;
    renderer.fill_paragraph(
        paragraph,
        Point::new(text_origin_x + offset, baseline_y),
        style.syntax_fallback_text,
        clip_bounds,
    );
}

/// Partition at both color and font boundaries. Font ranges are already
/// relative to this slice; syntax ranges still use the row's original offsets.
fn adaptive_span_keys(
    row: &RowRenderPlan,
    range: Range<usize>,
    fonts: &[EditorFontRun],
    fallback_color: Color,
) -> Vec<SyntaxSpanKey> {
    let content = &row.text[range.clone()];
    let mut boundaries = vec![0, content.len()];
    for run in fonts {
        boundaries.extend([run.byte_range.start, run.byte_range.end]);
    }
    let valid_syntax =
        row.syntax_spans.len() <= MAX_SYNTAX_SPANS_PER_ROW && syntax_spans_are_valid(row);
    if valid_syntax {
        for span in &row.syntax_spans {
            if span.range.end > range.start && span.range.start < range.end {
                boundaries.extend([
                    span.range.start.max(range.start) - range.start,
                    span.range.end.min(range.end) - range.start,
                ]);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
        .windows(2)
        .map(|pair| {
            let byte = range.start + pair[0];
            let color = valid_syntax
                .then(|| {
                    row.syntax_spans
                        .iter()
                        .find(|span| span.range.contains(&byte))
                        .and_then(|span| span.color)
                })
                .flatten()
                .unwrap_or(fallback_color);
            SyntaxSpanKey {
                start: pair[0],
                end: pair[1],
                color: Some(color),
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn draw_rich_row_text<Renderer>(
    renderer: &mut Renderer,
    row: &RowRenderPlan,
    position: Point,
    height: f32,
    metrics: EditorMetrics,
    style: EditorStyle,
    clip_bounds: Rectangle,
    frame_id: u64,
    rich_paragraphs: &mut RichParagraphCache<Renderer::Paragraph>,
    draw_plain_text: impl FnOnce(
        &mut Renderer,
        String,
        Point,
        Size,
        Color,
        text::Alignment,
        EditorMetrics,
        Rectangle,
    ),
) where
    Renderer: text::Renderer<Font = Font>,
{
    let visible_range = visible_rich_text_range(row, position.x, metrics, clip_bounds);
    let visible_text = &row.text[visible_range.clone()];
    let visible_start_column = visual_column_for(&row.text, visible_range.start, 1);
    let visible_width = visual_column_for(visible_text, visible_text.len(), 1);
    let visible_position = Point::new(
        position.x + visible_start_column as f32 * metrics.character_width,
        position.y,
    );
    let visible_bounds = Size::new(
        (visible_width as f32 * metrics.character_width).max(metrics.character_width),
        height,
    );
    let mut span_keys = Vec::new();
    let mut cursor = visible_range.start;
    let first_span = first_visible_syntax_span(row, visible_range.start);
    let fallback_color = style.syntax_fallback_text;

    for syntax_span in &row.syntax_spans[first_span..] {
        if syntax_span.range.start >= visible_range.end {
            break;
        }

        let start = syntax_span.range.start.max(visible_range.start);
        let end = syntax_span.range.end.min(visible_range.end);

        if start >= end
            || end > row.text.len()
            || !row.text.is_char_boundary(start)
            || !row.text.is_char_boundary(end)
        {
            continue;
        }

        if cursor < start {
            let local_start = cursor - visible_range.start;
            let local_end = start - visible_range.start;

            push_syntax_span_key(
                &mut span_keys,
                SyntaxSpanKey {
                    start: local_start,
                    end: local_end,
                    color: Some(fallback_color),
                },
            );
        }

        push_syntax_span_key(
            &mut span_keys,
            SyntaxSpanKey {
                start: start - visible_range.start,
                end: end - visible_range.start,
                color: syntax_span.color.or(Some(fallback_color)),
            },
        );
        cursor = end;
    }

    if cursor < visible_range.end {
        push_syntax_span_key(
            &mut span_keys,
            SyntaxSpanKey {
                start: cursor - visible_range.start,
                end: visible_range.end - visible_range.start,
                color: Some(fallback_color),
            },
        );
    }

    if span_keys.is_empty() {
        draw_plain_text(
            renderer,
            visible_text.to_owned(),
            visible_position,
            visible_bounds,
            style.syntax_fallback_text,
            text::Alignment::Left,
            metrics,
            clip_bounds,
        );
        return;
    }

    let size = Pixels((metrics.line_height / 1.25).max(8.0));
    let scale_factor = renderer.scale_factor();
    let paragraph = rich_paragraphs.get_or_insert_with(
        row.visible_row,
        visible_text,
        &span_keys,
        visible_range.start,
        visible_bounds,
        size,
        metrics.line_height,
        scale_factor,
        frame_id,
        || {
            let mut spans: Vec<text::Span<'_, (), Font>> = Vec::with_capacity(span_keys.len());
            for key in &span_keys {
                spans.push(
                    text::Span::new(&visible_text[key.start..key.end]).color_maybe(key.color),
                );
            }

            Renderer::Paragraph::with_spans(text::Text {
                content: spans.as_slice(),
                bounds: visible_bounds,
                size,
                line_height: text::LineHeight::Absolute(Pixels(metrics.line_height)),
                font: EDITOR_FONT,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: EDITOR_TEXT_SHAPING,
                wrapping: text::Wrapping::None,
                ellipsis: text::Ellipsis::None,
                hint_factor: scale_factor,
            })
        },
    );

    renderer.fill_paragraph(
        paragraph,
        visible_position,
        style.syntax_fallback_text,
        clip_bounds,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_clipped_plain_text<Renderer>(
    renderer: &mut Renderer,
    text: String,
    text_origin_x: f32,
    baseline_y: f32,
    metrics: EditorMetrics,
    clip_bounds: Rectangle,
    color: Color,
    draw_plain_text: impl FnOnce(
        &mut Renderer,
        String,
        Point,
        Size,
        Color,
        text::Alignment,
        EditorMetrics,
        Rectangle,
    ),
) {
    let visible_range = visible_text_range(&text, text_origin_x, metrics, clip_bounds);
    let visible_text = &text[visible_range.clone()];
    let render_text = if contains_unsafe_render_controls(&text) {
        safe_control_heavy_text(visible_text)
    } else {
        visible_text.to_owned()
    };
    let visible_start_column = visual_column_for(&text, visible_range.start, 1);
    let visible_width = visual_column_for(visible_text, visible_text.len(), 1);

    draw_plain_text(
        renderer,
        render_text,
        Point::new(
            text_origin_x + visible_start_column as f32 * metrics.character_width,
            baseline_y,
        ),
        Size::new(
            (visible_width as f32 * metrics.character_width).max(metrics.character_width),
            metrics.line_height,
        ),
        color,
        text::Alignment::Left,
        metrics,
        clip_bounds,
    );
}

fn contains_unsafe_render_controls(text: &str) -> bool {
    text.chars().any(|ch| ch != '\t' && ch.is_control())
}

fn safe_control_heavy_text(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_graphic() || ch == ' ' {
                ch
            } else {
                '.'
            }
        })
        .collect()
}

pub(super) fn visible_rich_text_range(
    row: &RowRenderPlan,
    text_origin_x: f32,
    metrics: EditorMetrics,
    clip_bounds: Rectangle,
) -> Range<usize> {
    visible_styled_text_range(row, text_origin_x, metrics, clip_bounds)
}

pub(super) fn push_syntax_span_key(span_keys: &mut Vec<SyntaxSpanKey>, key: SyntaxSpanKey) {
    if key.start >= key.end {
        return;
    }

    if let Some(previous) = span_keys.last_mut()
        && previous.end == key.start
        && previous.color == key.color
    {
        previous.end = key.end;
        return;
    }

    span_keys.push(key);
}

pub(super) fn visible_styled_text_range(
    row: &RowRenderPlan,
    text_origin_x: f32,
    metrics: EditorMetrics,
    clip_bounds: Rectangle,
) -> Range<usize> {
    if !syntax_spans_are_valid(row) {
        return 0..row.text.len();
    }

    let range = visible_text_range(&row.text, text_origin_x, metrics, clip_bounds);
    let start = style_boundary_start(row, range.start);

    start..range.end
}

fn visible_text_range(
    text: &str,
    text_origin_x: f32,
    metrics: EditorMetrics,
    clip_bounds: Rectangle,
) -> Range<usize> {
    if text.is_empty() || metrics.character_width <= 0.0 {
        return 0..text.len();
    }

    let first = ((clip_bounds.x - text_origin_x) / metrics.character_width)
        .floor()
        .max(0.0) as usize;
    let last = ((clip_bounds.x + clip_bounds.width - text_origin_x) / metrics.character_width)
        .ceil()
        .max(0.0) as usize;
    let start_column = first.saturating_sub(LONG_LINE_VISIBLE_MARGIN_COLUMNS);
    let end_column = last.saturating_add(LONG_LINE_VISIBLE_MARGIN_COLUMNS);
    let start = byte_column_for(text, start_column, 1);
    let end = byte_column_for(text, end_column, 1).max(start);

    start..end
}

fn syntax_spans_are_valid(row: &RowRenderPlan) -> bool {
    let mut previous_end = 0;

    row.syntax_spans.iter().all(|span| {
        let valid = previous_end <= span.range.start
            && span.range.start < span.range.end
            && span.range.end <= row.text.len()
            && row.text.is_char_boundary(span.range.start)
            && row.text.is_char_boundary(span.range.end);

        previous_end = span.range.end;
        valid
    })
}

fn style_boundary_start(row: &RowRenderPlan, start: usize) -> usize {
    let boundary = row
        .syntax_spans
        .iter()
        .filter(|span| span.range.start < start && start < span.range.end)
        .map(|span| span.range.start)
        .max()
        .unwrap_or(start);

    let mut start = if start.saturating_sub(boundary) > LONG_STYLE_RUN_SUBDIVISION_COLUMNS {
        start - (start - boundary) % LONG_STYLE_RUN_SUBDIVISION_COLUMNS
    } else {
        boundary
    };
    // Subdivision is measured in bytes; its rounded boundary may fall inside
    // a multi-byte CJK character. Back up at most three bytes before slicing.
    while !row.text.is_char_boundary(start) {
        start -= 1;
    }
    start
}

pub(super) fn first_visible_syntax_span(row: &RowRenderPlan, visible_start: usize) -> usize {
    row.syntax_spans
        .partition_point(|span| span.range.end <= visible_start)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExpandedTabs {
    pub(super) text: String,
    pub(super) byte_offsets: Vec<usize>,
}

fn visible_tab_source_range(
    text: &str,
    tab_width: usize,
    start_visual_column: usize,
    text_origin_x: f32,
    metrics: EditorMetrics,
    clip_bounds: Rectangle,
) -> Option<(Range<usize>, usize)> {
    if !text.contains('\t') || metrics.character_width <= 0.0 {
        return None;
    }
    let first = ((clip_bounds.x - text_origin_x) / metrics.character_width)
        .floor()
        .max(0.0) as usize;
    let last = ((clip_bounds.x + clip_bounds.width - text_origin_x) / metrics.character_width)
        .ceil()
        .max(0.0) as usize;
    let start = byte_column_for_with_offset(
        text,
        start_visual_column.saturating_add(first.saturating_sub(LONG_LINE_VISIBLE_MARGIN_COLUMNS)),
        tab_width,
        start_visual_column,
    );
    let end = byte_column_for_with_offset(
        text,
        start_visual_column
            .saturating_add(last)
            .saturating_add(LONG_LINE_VISIBLE_MARGIN_COLUMNS + 1),
        tab_width,
        start_visual_column,
    )
    .max(start);
    let end = end + text[end..].chars().next().map_or(0, char::len_utf8);
    let offset = visual_column_for_with_offset(text, start, tab_width, start_visual_column)
        - start_visual_column;
    Some((start..end, offset))
}

// Plain text needs no syntax byte map. Expand only the visible fragment, using
// the logical column at its start so wrapped rows preserve their tab stops.
fn visible_expanded_tabs(
    text: &str,
    tab_width: usize,
    start_visual_column: usize,
    text_origin_x: f32,
    metrics: EditorMetrics,
    clip_bounds: Rectangle,
) -> Option<(String, usize)> {
    let (range, offset) = visible_tab_source_range(
        text,
        tab_width,
        start_visual_column,
        text_origin_x,
        metrics,
        clip_bounds,
    )?;
    let first = ((clip_bounds.x - text_origin_x) / metrics.character_width)
        .floor()
        .max(0.0) as usize;
    let last = ((clip_bounds.x + clip_bounds.width - text_origin_x) / metrics.character_width)
        .ceil()
        .max(0.0) as usize;
    let mut column = start_visual_column + offset;
    let mut expanded = String::with_capacity(range.len());
    for ch in text[range].chars() {
        let width = visual_width_with_tab_width(ch, column, tab_width);
        if ch == '\t' {
            expanded.extend(std::iter::repeat_n(' ', width));
        } else {
            expanded.push(ch);
        }
        column += width;
    }
    // Clip in visual columns relative to the original origin. Applying a new
    // floating-point origin before clipping can round a fractional cell twice.
    let start = byte_column_for(
        &expanded,
        first
            .saturating_sub(LONG_LINE_VISIBLE_MARGIN_COLUMNS)
            .saturating_sub(offset),
        1,
    );
    let end = byte_column_for(
        &expanded,
        last.saturating_add(LONG_LINE_VISIBLE_MARGIN_COLUMNS)
            .saturating_sub(offset),
        1,
    )
    .max(start);
    let column = offset + visual_column_for(&expanded, start, 1);
    expanded.truncate(end);
    drop(expanded.drain(..start));
    Some((expanded, column))
}

#[cfg(test)]
fn expand_tabs_for_rendering(text: &str, tab_width: usize) -> Option<ExpandedTabs> {
    expand_tabs_for_rendering_with_offset(text, tab_width, 0)
}

pub(super) fn expand_tabs_for_rendering_with_offset(
    text: &str,
    tab_width: usize,
    start_visual_column: usize,
) -> Option<ExpandedTabs> {
    if !text.contains('\t') {
        return None;
    }

    let mut expanded = String::with_capacity(text.len());
    let mut byte_offsets = vec![0; text.len() + 1];
    let mut visual_column = start_visual_column;
    let mut expanded_offset = 0usize;

    for (offset, ch) in text.char_indices() {
        byte_offsets[offset] = expanded_offset;

        if ch == '\t' {
            let spaces = visual_width_with_tab_width(ch, visual_column, tab_width);
            expanded.extend(std::iter::repeat_n(' ', spaces));
            visual_column += spaces;
            expanded_offset += spaces;
        } else {
            expanded.push(ch);
            visual_column += visual_width_with_tab_width(ch, visual_column, tab_width);
            expanded_offset += ch.len_utf8();
        }

        byte_offsets[offset + ch.len_utf8()] = expanded_offset;
    }

    byte_offsets[text.len()] = expanded_offset;

    Some(ExpandedTabs {
        text: expanded,
        byte_offsets,
    })
}

fn remap_syntax_spans(
    spans: &[crate::editor::render::SyntaxRenderSpan],
    byte_offsets: &[usize],
) -> Vec<crate::editor::render::SyntaxRenderSpan> {
    spans
        .iter()
        .filter_map(|span| {
            let start = *byte_offsets.get(span.range.start)?;
            let end = *byte_offsets.get(span.range.end)?;

            (start < end).then_some(crate::editor::render::SyntaxRenderSpan {
                range: start..end,
                color: span.color,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::render::SyntaxRenderSpan;

    #[test]
    fn clipped_tab_expansion_matches_full_line_at_scroll_and_wrap_boundaries() {
        let long = "a\tb\tend\t".repeat(300);
        let long_cjk = "中\t骨直令\t".repeat(1000);
        for character_width in [7.2, 8.35, 9.25, 13.6] {
            let metrics = EditorMetrics {
                character_width,
                ..EditorMetrics::default()
            };
            for source in [
                "a\tb\tend",
                "\t中e\u{301}\t🐇 cafe\t",
                long.as_str(),
                long_cjk.as_str(),
            ] {
                for tab_width in [1, 4, 8] {
                    for start_column in [0, 3, 9] {
                        let full =
                            expand_tabs_for_rendering_with_offset(source, tab_width, start_column)
                                .unwrap()
                                .text;
                        for scroll in [0.0, 3.5, 9.25, 81.0, 790.0, 4000.0] {
                            for width in [1.0, 25.0, 132.0] {
                                let clip = Rectangle {
                                    x: 37.0,
                                    y: 0.0,
                                    width,
                                    height: 20.0,
                                };
                                let origin = 37.0 - scroll;
                                let (visible, offset) = visible_expanded_tabs(
                                    source,
                                    tab_width,
                                    start_column,
                                    origin,
                                    metrics,
                                    clip,
                                )
                                .unwrap();
                                let expected = visible_text_range(&full, origin, metrics, clip);
                                assert_eq!(
                                    visible,
                                    full[expected.clone()],
                                    "cell={character_width}, tab={tab_width}, start={start_column}, scroll={scroll}, width={width}"
                                );
                                assert_eq!(offset, visual_column_for(&full, expected.start, 1));
                                assert!(
                                    visible.len() <= 256,
                                    "offscreen tail must not be expanded"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn syntax_span_keys_merge_adjacent_equal_styles() {
        let color = Some(Color::from_rgb(0.8, 0.2, 0.1));
        let mut keys = Vec::new();

        push_syntax_span_key(
            &mut keys,
            SyntaxSpanKey {
                start: 0,
                end: 3,
                color,
            },
        );
        push_syntax_span_key(
            &mut keys,
            SyntaxSpanKey {
                start: 3,
                end: 7,
                color,
            },
        );
        push_syntax_span_key(
            &mut keys,
            SyntaxSpanKey {
                start: 7,
                end: 9,
                color: Some(Color::from_rgb(0.2, 0.4, 0.6)),
            },
        );

        assert_eq!(
            keys,
            vec![
                SyntaxSpanKey {
                    start: 0,
                    end: 7,
                    color,
                },
                SyntaxSpanKey {
                    start: 7,
                    end: 9,
                    color: Some(Color::from_rgb(0.2, 0.4, 0.6)),
                },
            ]
        );
    }

    #[test]
    fn tab_expansion_uses_configured_tab_stops_for_render_text() {
        let expanded = expand_tabs_for_rendering("a\tb", 8).expect("tabbed text");

        assert_eq!(expanded.text, "a       b");
        assert_eq!(expanded.byte_offsets[0], 0);
        assert_eq!(expanded.byte_offsets[1], 1);
        assert_eq!(expanded.byte_offsets[2], 8);
        assert_eq!(expanded.byte_offsets[3], 9);
    }

    #[test]
    fn wrapped_tab_expansion_retains_stops_from_the_logical_line() {
        let expanded = expand_tabs_for_rendering_with_offset("x\ty ", 4, 5).unwrap();
        assert_eq!(expanded.text, "x  y ");
        assert_eq!(expanded.byte_offsets, [0, 1, 3, 4, 5]);
    }

    #[test]
    fn control_heavy_rows_use_bounded_ascii_display_fallback() {
        assert!(!contains_unsafe_render_controls("caf\u{00e9}\ttext"));
        assert!(contains_unsafe_render_controls("caf\u{00e9}\0text"));
        assert_eq!(safe_control_heavy_text("A\0\u{00e9} Z"), "A.. Z");
    }

    #[test]
    fn syntax_spans_remap_to_expanded_tab_text() {
        let expanded = expand_tabs_for_rendering("a\tbc", 4).expect("tabbed text");
        let spans = vec![
            SyntaxRenderSpan {
                range: 0..1,
                color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
            },
            SyntaxRenderSpan {
                range: 1..4,
                color: Some(Color::from_rgb(0.0, 1.0, 0.0)),
            },
        ];

        assert_eq!(
            remap_syntax_spans(&spans, &expanded.byte_offsets),
            vec![
                SyntaxRenderSpan {
                    range: 0..1,
                    color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
                },
                SyntaxRenderSpan {
                    range: 1..6,
                    color: Some(Color::from_rgb(0.0, 1.0, 0.0)),
                },
            ]
        );
    }

    #[test]
    fn rich_text_visible_range_limits_long_ascii_lines_to_clip_columns() {
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: "x".repeat(1_000),
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans: Vec::new(),
        };
        let metrics = EditorMetrics {
            character_width: 10.0,
            ..EditorMetrics::default()
        };
        let range = visible_rich_text_range(
            &row,
            0.0,
            metrics,
            Rectangle {
                x: 100.0,
                y: 0.0,
                width: 50.0,
                height: 20.0,
            },
        );

        assert_eq!(range, 2..23);
    }

    #[test]
    fn rich_text_visible_range_backs_up_to_syntax_boundary() {
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: "a".repeat(120),
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans: vec![
                SyntaxRenderSpan {
                    range: 0..16,
                    color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
                },
                SyntaxRenderSpan {
                    range: 16..80,
                    color: Some(Color::from_rgb(0.0, 1.0, 0.0)),
                },
                SyntaxRenderSpan {
                    range: 80..120,
                    color: Some(Color::from_rgb(0.0, 0.0, 1.0)),
                },
            ],
        };
        let metrics = EditorMetrics {
            character_width: 10.0,
            ..EditorMetrics::default()
        };
        let range = visible_styled_text_range(
            &row,
            0.0,
            metrics,
            Rectangle {
                x: 330.0,
                y: 0.0,
                width: 60.0,
                height: 20.0,
            },
        );

        assert_eq!(range, 16..47);
    }

    #[test]
    fn rich_text_visible_range_subdivides_long_syntax_runs() {
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: "a".repeat(1_000),
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans: vec![SyntaxRenderSpan {
                range: 0..1_000,
                color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
            }],
        };
        let metrics = EditorMetrics {
            character_width: 10.0,
            ..EditorMetrics::default()
        };
        let range = visible_styled_text_range(
            &row,
            0.0,
            metrics,
            Rectangle {
                x: 3_330.0,
                y: 0.0,
                width: 60.0,
                height: 20.0,
            },
        );

        assert_eq!(range, 300..347);
    }

    #[test]
    fn first_visible_syntax_span_skips_offscreen_spans() {
        let syntax_spans = (0..1_000)
            .map(|index| SyntaxRenderSpan {
                range: index * 8..index * 8 + 8,
                color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
            })
            .collect();
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: "a".repeat(8_000),
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans,
        };

        assert_eq!(first_visible_syntax_span(&row, 3_200), 400);
    }

    #[test]
    fn rich_text_visible_range_clips_tabbed_lines_to_visible_columns() {
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: format!("{}\t{}", "a".repeat(40), "b".repeat(40)),
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans: Vec::new(),
        };
        let metrics = EditorMetrics {
            character_width: 10.0,
            ..EditorMetrics::default()
        };

        assert_eq!(
            visible_styled_text_range(
                &row,
                0.0,
                metrics,
                Rectangle {
                    x: 100.0,
                    y: 0.0,
                    width: 50.0,
                    height: 20.0,
                },
            ),
            2..23
        );
    }

    #[test]
    fn rich_text_visible_range_clips_non_ascii_lines_on_utf8_boundaries() {
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: format!("{}\u{6f20}\u{7958}{}", "a".repeat(40), "b".repeat(40)),
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans: Vec::new(),
        };
        let metrics = EditorMetrics {
            character_width: 10.0,
            ..EditorMetrics::default()
        };

        assert_eq!(
            visible_styled_text_range(
                &row,
                0.0,
                metrics,
                Rectangle {
                    x: 100.0,
                    y: 0.0,
                    width: 50.0,
                    height: 20.0,
                },
            ),
            2..23
        );
    }

    #[test]
    fn long_cjk_style_subdivision_preserves_utf8_when_scrolled() {
        let content = "漢".repeat(2_000);
        let length = content.len();
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: content,
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans: vec![SyntaxRenderSpan {
                range: 0..length,
                color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
            }],
        };
        assert!(row.text.len() > 4096);
        assert_eq!(style_boundary_start(&row, 1005), 999);
        let metrics = EditorMetrics {
            character_width: 10.0,
            ..EditorMetrics::default()
        };
        let range = visible_styled_text_range(
            &row,
            0.0,
            metrics,
            Rectangle {
                x: 6780.0,
                y: 0.0,
                width: 60.0,
                height: 20.0,
            },
        );
        assert_eq!(range.start, 999);
        assert!(row.text.is_char_boundary(range.end));
        assert!(!row.text[range].is_empty());
    }

    #[test]
    fn rich_text_visible_range_falls_back_for_invalid_syntax_spans() {
        let row = RowRenderPlan {
            visible_row: 0,
            line: 0,
            start_column: 0,
            start_visual_column: 0,
            y: 0.0,
            text_x: 0.0,
            text: "a".repeat(120),
            line_number: None,
            is_active_line: false,
            fold: None,
            hidden_lines: None,
            whitespace: Vec::new(),
            eol: None,
            indent_guides: Vec::new(),
            projection: Vec::new(),
            syntax_spans: vec![SyntaxRenderSpan {
                range: 12..128,
                color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
            }],
        };
        let metrics = EditorMetrics {
            character_width: 10.0,
            ..EditorMetrics::default()
        };

        assert_eq!(
            visible_styled_text_range(
                &row,
                0.0,
                metrics,
                Rectangle {
                    x: 100.0,
                    y: 0.0,
                    width: 50.0,
                    height: 20.0,
                },
            ),
            0..row.text.len()
        );
    }
}
