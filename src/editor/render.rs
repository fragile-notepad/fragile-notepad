use super::buffer::EditorBuffer;
use super::decoration::{DecorationModel, HiddenLineSpan, IndentGuide, LineDecoration};
use super::fold::{FoldDelimiter, FoldRange};
use super::fold_projection::{FoldProjection, ProjectionFragment};
use super::layout::{
    EditorLayout, EditorMetrics, GUTTER_RIGHT_MARGIN, byte_column_for, row_y,
    scrolled_text_origin_x, visual_column_for, visual_column_for_with_offset,
    visual_width_with_tab_width, x_for_visual_column,
};
use super::position::{
    EditorPosition, EditorSelection, ProjectedSelectionLine, SelectionRange, SelectionSet,
    SelectionShape,
};
use super::viewport::{RowSegment, ViewportModel};
use iced::{Color, Rectangle, highlighter};
use std::{collections::HashMap, ops::Range};

pub(crate) mod syntax;
pub use syntax::{SyntaxLineCache, SyntaxParseResult};

const DEFAULT_SYNTAX_TOKEN: &str = "txt";

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPlan {
    pub rows: Vec<RowRenderPlan>,
    pub selections: Vec<SelectionRenderPlan>,
    pub carets: Vec<CaretRenderPlan>,
    pub caret: Option<CaretRenderPlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowRenderPlan {
    pub visible_row: usize,
    pub line: usize,
    pub start_column: usize,
    pub start_visual_column: usize,
    pub y: f32,
    pub text_x: f32,
    pub text: String,
    pub line_number: Option<usize>,
    pub is_active_line: bool,
    pub fold: Option<FoldRenderPlan>,
    pub hidden_lines: Option<HiddenLineRenderPlan>,
    pub whitespace: Vec<WhitespaceRenderPlan>,
    pub eol: Option<EolRenderPlan>,
    pub indent_guides: Vec<IndentGuideRenderPlan>,
    pub syntax_spans: Vec<SyntaxRenderSpan>,
    pub projection: Vec<ProjectionFragment>,
}

impl RowRenderPlan {
    pub(crate) fn display_column_for_source(&self, position: EditorPosition) -> Option<usize> {
        if self.projection.is_empty() {
            return (position.line == self.line)
                .then_some(position.column.saturating_sub(self.start_column));
        }
        for fragment in &self.projection {
            match fragment {
                ProjectionFragment::Source {
                    display_range,
                    source_start,
                } if position.line == source_start.line
                    && position.column >= source_start.column
                    && position.column <= source_start.column + display_range.len() =>
                {
                    return Some(display_range.start + position.column - source_start.column);
                }
                ProjectionFragment::Placeholder {
                    display_range,
                    range,
                    delimiter,
                } => {
                    if position == EditorPosition::new(range.start_line, delimiter.opening_column) {
                        return Some(display_range.start);
                    }
                    if position == EditorPosition::new(range.end_line, delimiter.closing_column)
                        || (position.line == range.start_line
                            && position.column > delimiter.opening_column)
                    {
                        return Some(display_range.end);
                    }
                }
                _ => {}
            }
        }
        if let Some(ProjectionFragment::Source {
            display_range,
            source_start,
        }) = self.projection.last()
            && position.line == source_start.line
            && position.column > source_start.column + display_range.len()
        {
            return Some(
                display_range.end + position.column - source_start.column - display_range.len(),
            );
        }
        None
    }

    pub fn collapsed_delimiter(&self) -> Option<FoldDelimiter> {
        self.hidden_lines.and_then(|hidden| hidden.delimiter)
    }

    pub fn collapsed_indicator_column(&self) -> usize {
        self.collapsed_delimiter()
            .map_or(self.text.len(), |delimiter| delimiter.opening_column)
    }

    /// Returns the inline indicator only when this row hides a collapsed block.
    /// Measure `collapsed_indicator_column` in the row's full source geometry.
    pub fn collapsed_indicator_bounds(
        &self,
        metrics: EditorMetrics,
        measured_anchor_x: f32,
    ) -> Option<Rectangle> {
        self.hidden_lines
            .filter(|hidden| hidden.hidden_line_count > 0)
            .map(|hidden| {
                if hidden.delimiter.is_some() {
                    collapsed_delimiter_indicator_bounds(metrics, self.y, measured_anchor_x)
                } else if hidden.condition_placeholder.is_some() {
                    collapsed_condition_indicator_bounds(
                        metrics,
                        self.y,
                        measured_anchor_x,
                        self.eol.is_some(),
                    )
                } else {
                    collapsed_fold_indicator_bounds(
                        metrics,
                        self.y,
                        measured_anchor_x,
                        self.eol.is_some(),
                    )
                }
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxRenderSpan {
    pub range: Range<usize>,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRenderPlan {
    pub line: usize,
    pub collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiddenLineRenderPlan {
    pub first_hidden_line: usize,
    pub last_hidden_line: usize,
    pub hidden_line_count: usize,
    pub delimiter: Option<FoldDelimiter>,
    pub condition_placeholder: Option<&'static str>,
}

pub(crate) fn condition_fold_placeholder(
    buffer: &EditorBuffer,
    first: usize,
    last: usize,
) -> Option<&'static str> {
    let continuation = (first..=last)
        .filter_map(|line| buffer.line(line))
        .find(|line| !line.trim().is_empty())?;
    let continuation = continuation.trim_start();
    if continuation.starts_with("&&") {
        Some("&&...")
    } else if continuation.starts_with("||") {
        Some("||...")
    } else {
        None
    }
}

pub(crate) fn fold_delimiter_for_fragment(
    mut delimiter: FoldDelimiter,
    start_column: usize,
    text: &str,
) -> Option<FoldDelimiter> {
    let column = delimiter.opening_column.checked_sub(start_column)?;
    if !text
        .get(column..)?
        .strip_prefix(delimiter.opening)?
        .trim()
        .is_empty()
    {
        return None;
    }
    delimiter.opening_column = column;
    Some(delimiter)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitespaceKind {
    Space,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhitespaceRenderPlan {
    pub line: usize,
    pub column: usize,
    pub x: f32,
    pub kind: WhitespaceKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EolRenderPlan {
    pub line: usize,
    pub x: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndentGuideRenderPlan {
    pub line: usize,
    pub depth: usize,
    pub x: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRenderPlan {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub start_visual_column: usize,
    pub end_visual_column: usize,
    pub start_virtual_column: Option<usize>,
    pub end_virtual_column: Option<usize>,
    pub y: f32,
    pub x: f32,
    pub width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretRenderPlan {
    pub position: EditorPosition,
    pub visual_column: Option<usize>,
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

/// Estimates the number of text draw calls needed for editor row text.
///
/// This helper deliberately counts only text primitives. It does not include
/// quads for selections, active-line backgrounds, indentation guides, carets,
/// scrollbars, or decorative visible-space dots.
///
/// Fast scrolling can batch tab-free ASCII rows without syntax spans or paired
/// fold placeholders into one clipped text item. Highlighted and projected
/// rows remain separate to preserve their colors and source geometry.
pub fn planned_text_draws(plan: &RenderPlan, fast_text: bool) -> usize {
    if fast_text
        && plan.rows.iter().all(|row| {
            row.projection.is_empty()
                && row.collapsed_delimiter().is_none()
                && row.syntax_spans.is_empty()
                && row
                    .text
                    .bytes()
                    .all(|byte| byte.is_ascii() && byte != b'\t')
        })
    {
        return usize::from(!plan.rows.is_empty());
    }

    plan.rows.len()
}

/// Estimates text draw calls after optional whitespace marker rendering.
///
/// Visible space markers are intentionally excluded because they render as tiny
/// solid quads on the software renderer fast path. Tab and end-of-line markers
/// render as raster Heroicons, so they do not add text draw calls.
///
/// The function is used by performance tests and profiling examples as a stable
/// guard against accidentally returning decorative spaces to per-glyph text
/// rendering.
pub fn planned_text_draws_with_markers(plan: &RenderPlan, fast_text: bool) -> usize {
    planned_text_draws(plan, fast_text)
}

/// Returns the left edge used for right-aligned line numbers.
///
/// The editor gutter anchors line numbers to a stable right edge so the text
/// does not jitter as the visible line range changes from one digit count to
/// another. `text_width` should be the measured or monospace-estimated width of
/// the concrete line number.
pub fn line_number_left_x(metrics: EditorMetrics, text_width: f32) -> f32 {
    line_number_text_x(metrics) - text_width
}

/// Returns the right-edge text anchor for line numbers.
///
/// The value is expressed in editor-local coordinates, not absolute widget
/// coordinates. Callers should add the widget bounds origin before drawing.
pub fn line_number_text_x(metrics: EditorMetrics) -> f32 {
    metrics.padding_left + metrics.line_number_width - GUTTER_RIGHT_MARGIN
}

/// Computes the baseline offset used by all editor text draws.
///
/// The editor renders text in a fixed-height row, but iced text positioning is
/// top-based. This offset vertically centers the configured text size inside
/// `metrics.line_height`; callers add it to the row's top before drawing text.
pub fn text_baseline_offset(metrics: EditorMetrics) -> f32 {
    (metrics.line_height - text_size(metrics)) / 2.0
}

/// Computes the editor text size derived from row metrics.
///
/// Keeping this calculation centralized ensures plain text, rich text, line
/// numbers, markers, hit-test measurement, and profiler fixtures all agree on
/// the same font size.
pub fn text_size(metrics: EditorMetrics) -> f32 {
    (metrics.line_height / 1.25).max(8.0)
}

/// Returns the inline ellipsis bounds after a collapsed block's header text.
///
/// Coordinates are editor-local and already include horizontal scrolling in
/// `measured_text_end_x`. Sharing this geometry with pointer handling keeps the
/// clickable area aligned with measured Unicode text, tabs, and zoom. Callers
/// clip the result to the text area; a long header never moves the indicator
/// over its own text. An enabled end-of-line marker retains its own text cell.
pub fn collapsed_fold_indicator_bounds(
    metrics: EditorMetrics,
    row_y: f32,
    measured_text_end_x: f32,
    show_eol_markers: bool,
) -> Rectangle {
    let (gap, width) = collapsed_fold_indicator_dimensions(metrics.character_width);
    let height = metrics.line_height.max(0.0);
    let marker_width = if show_eol_markers {
        end_of_line_marker_reservation(metrics.character_width)
    } else {
        0.0
    };

    Rectangle {
        x: measured_text_end_x + marker_width + gap,
        y: row_y,
        width,
        height,
    }
}

/// Bounds for a matched delimiter and its hidden contents, at the opener.
pub fn collapsed_delimiter_indicator_bounds(
    metrics: EditorMetrics,
    row_y: f32,
    measured_opener_x: f32,
) -> Rectangle {
    let height = metrics.line_height;
    Rectangle {
        x: measured_opener_x,
        y: row_y,
        width: collapsed_delimiter_indicator_width(metrics.character_width),
        height,
    }
}

pub fn collapsed_condition_indicator_bounds(
    metrics: EditorMetrics,
    row_y: f32,
    measured_text_end_x: f32,
    show_eol_markers: bool,
) -> Rectangle {
    let anchor =
        collapsed_fold_indicator_bounds(metrics, row_y, measured_text_end_x, show_eol_markers);
    collapsed_delimiter_indicator_bounds(metrics, row_y, anchor.x)
}

/// Horizontal space needed after a header, excluding any EOL marker.
pub fn collapsed_fold_indicator_reservation(character_width: f32) -> f32 {
    let (gap, width) = collapsed_fold_indicator_dimensions(character_width);
    (gap + width)
        .max(gap + collapsed_delimiter_indicator_width(character_width))
        .max(
            collapsed_delimiter_indicator_width(character_width) - character_width
                + character_width * 0.25,
        )
}

fn collapsed_delimiter_indicator_width(character_width: f32) -> f32 {
    (character_width * 5.4).max(24.0)
}

fn collapsed_fold_indicator_dimensions(character_width: f32) -> (f32, f32) {
    (
        (character_width * 0.6).max(4.0),
        (character_width * 2.8).max(20.0),
    )
}

/// Keep the marker's minimum icon width available when text is zoomed out.
pub fn end_of_line_marker_reservation(character_width: f32) -> f32 {
    character_width.max(7.0)
}

/// Returns the visible visual-column range for marker rendering.
///
/// `text_origin_x` is the absolute x coordinate of visual column zero after
/// horizontal scrolling. `clip_bounds` is the text clip rectangle in absolute
/// widget coordinates. The result is inclusive and intentionally rounded outward
/// so partially visible markers are still drawn.
pub fn visible_marker_columns(
    text_origin_x: f32,
    character_width: f32,
    clip_bounds: Rectangle,
) -> Option<(usize, usize)> {
    if character_width <= 0.0 || clip_bounds.width <= 0.0 {
        return None;
    }

    let first = ((clip_bounds.x - text_origin_x) / character_width)
        .floor()
        .max(0.0) as usize;
    let last = ((clip_bounds.x + clip_bounds.width - text_origin_x) / character_width)
        .ceil()
        .max(0.0) as usize;

    Some((first, last))
}

/// Returns the rectangle for a decorative visible-space dot.
///
/// The rectangle is in absolute widget coordinates. Space markers use this
/// geometry so they can render as tiny solid quads instead of one-character text
/// items. That keeps the common visible-spaces setting on the renderer's solid
/// rectangle fast path while preserving centered dot placement in each
/// monospace cell.
pub fn space_marker_bounds(
    bounds: Rectangle,
    layout: EditorLayout,
    decorations: &DecorationModel,
    row: &RowRenderPlan,
    visual_column: usize,
) -> Rectangle {
    let metrics = layout.metrics;
    let dot_size = space_marker_size(metrics);
    let x = bounds.x
        + scrolled_text_origin_x(layout, decorations)
        + visual_column as f32 * metrics.character_width
        + (metrics.character_width - dot_size) / 2.0;
    let y = bounds.y + row.y + (metrics.line_height - dot_size) / 2.0;

    Rectangle {
        x,
        y,
        width: dot_size,
        height: dot_size,
    }
}

/// Returns the side length of a visible-space dot in physical editor units.
///
/// The value is capped so dots remain visible at small font sizes and do not
/// become visually dominant at larger zoom levels.
pub fn space_marker_size(metrics: EditorMetrics) -> f32 {
    (metrics.character_width / 4.0).clamp(1.0, 2.0)
}

pub fn build_render_plan(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    selection: EditorSelection,
    layout: EditorLayout,
    syntax_settings: &highlighter::Settings,
) -> RenderPlan {
    let syntax_cache = SyntaxLineCache::rebuild(buffer, syntax_settings);

    build_render_plan_for_selection_set_with_cache(
        buffer,
        viewport,
        decorations,
        SelectionSet::single(selection),
        layout,
        &syntax_cache,
    )
}

pub fn build_render_plan_with_cache(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    selection: EditorSelection,
    layout: EditorLayout,
    syntax_cache: &SyntaxLineCache,
) -> RenderPlan {
    build_render_plan_for_selection_set_with_cache(
        buffer,
        viewport,
        decorations,
        SelectionSet::single(selection),
        layout,
        syntax_cache,
    )
}

pub fn build_render_plan_for_selection_set_with_cache(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    selections: SelectionSet,
    layout: EditorLayout,
    syntax_cache: &SyntaxLineCache,
) -> RenderPlan {
    build_render_plan_for_selection_set_with_cache_and_caret_row(
        buffer,
        viewport,
        decorations,
        selections,
        layout,
        syntax_cache,
        None,
    )
}

pub fn build_render_plan_for_selection_set_with_cache_and_caret_row(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    selections: SelectionSet,
    layout: EditorLayout,
    syntax_cache: &SyntaxLineCache,
    caret_row: Option<usize>,
) -> RenderPlan {
    let caret = caret_row.map(|row| (selections.main().cursor, row));
    build_render_plan_for_selection_set_with_cache_and_caret_rows(
        buffer,
        viewport,
        decorations,
        selections,
        layout,
        syntax_cache,
        caret.as_slice(),
    )
}

pub fn build_render_plan_for_selection_set_with_cache_and_caret_rows(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    selections: SelectionSet,
    mut layout: EditorLayout,
    syntax_cache: &SyntaxLineCache,
    caret_rows: &[(EditorPosition, usize)],
) -> RenderPlan {
    if viewport.wrap_columns().is_some() {
        layout.scroll.horizontal_px = 0.0;
    }
    let main_selection = selections.main();
    let active_display_line = viewport
        .display_position(main_selection.cursor)
        .map(|position| position.line);
    let caret_row = |position| {
        caret_rows
            .iter()
            .find_map(|(caret, row)| (*caret == position).then_some(*row))
    };
    let mut rows = Vec::new();
    let mut selection_plans = Vec::new();
    let mut carets = Vec::new();
    let mut cached_line = None;
    let mut line_text = String::new();
    let mut line_spans = Vec::new();
    let mut projected = Vec::new();
    let text_x = scrolled_text_origin_x(layout, decorations);
    let max_row = layout
        .scroll
        .first_visible_row
        .saturating_add(layout.visible_row_capacity());
    let lookups = RenderDecorationLookups::new(
        decorations,
        (layout.scroll.first_visible_row..=max_row)
            .filter_map(|row| viewport.visible_row_to_document_line(row)),
    );
    for visible_row in layout.scroll.first_visible_row..=max_row {
        let Some(line) = viewport.visible_row_to_document_line(visible_row) else {
            break;
        };
        let Some(segment) = viewport.row_segment(visible_row, buffer) else {
            break;
        };
        if cached_line != Some(line) {
            cached_line = Some(line);
            line_text = viewport.display_text(line, buffer).into_owned();
            line_spans = viewport.projection(line).map_or_else(
                || {
                    syntax_cache
                        .spans_for_text(line, &line_text)
                        .unwrap_or_default()
                },
                |projection| projected_syntax_spans(projection, buffer, syntax_cache),
            );
            projected.clear();
            for selection in selections.ranges() {
                if let Some(projection) = viewport.projection(line) {
                    projected.extend(project_folded_selection(
                        *selection,
                        line,
                        buffer,
                        projection,
                        decorations.settings.indent_width,
                    ));
                    for source_line in viewport.source_lines(line) {
                        let source_text = buffer.line(source_line).unwrap_or_default();
                        if (selection.is_caret() || selection.is_rectangular())
                            && let Some(source_selection) = project_selection_line(
                                *selection,
                                source_line,
                                buffer,
                                &source_text,
                                decorations.settings.indent_width,
                            )
                            && let Some(caret) = caret_plan_from_projected(
                                buffer,
                                viewport,
                                decorations,
                                source_selection,
                                &source_text,
                                layout,
                                caret_row(source_selection.end),
                            )
                        {
                            carets.push(caret);
                        }
                    }
                    continue;
                }
                if let Some(projection) = project_selection_line(
                    *selection,
                    line,
                    buffer,
                    &line_text,
                    decorations.settings.indent_width,
                ) {
                    if (selection.is_caret() || selection.is_rectangular())
                        && let Some(caret) = caret_plan_from_projected(
                            buffer,
                            viewport,
                            decorations,
                            projection,
                            &line_text,
                            layout,
                            caret_row(projection.end),
                        )
                    {
                        carets.push(caret);
                    }
                    projected.push(projection);
                }
            }
        }
        let text = &line_text[segment.start_column..segment.end_column];
        let y = row_y(visible_row, layout);
        let line_decoration = lookups.line_decoration(line);
        let fold = line_decoration
            .filter(|decoration| decoration.has_fold_control && !segment.is_continuation())
            .map(|decoration| FoldRenderPlan {
                line,
                collapsed: decoration.is_fold_collapsed,
            });
        let hidden_lines = lookups
            .hidden_line_span(line)
            .filter(|_| segment.is_last && viewport.projection(line).is_none())
            .map(|span| HiddenLineRenderPlan {
                condition_placeholder: condition_fold_placeholder(
                    buffer,
                    span.first_hidden_line,
                    span.last_hidden_line,
                ),
                first_hidden_line: span.first_hidden_line,
                last_hidden_line: span.last_hidden_line,
                hidden_line_count: span.hidden_line_count(),
                delimiter: line_decoration
                    .filter(|decoration| {
                        decoration.fold_range == Some(FoldRange::new(line, span.last_hidden_line))
                    })
                    .and_then(|decoration| decoration.fold_delimiter)
                    .and_then(|delimiter| {
                        fold_delimiter_for_fragment(delimiter, segment.start_column, text)
                    }),
            });
        let first_span = line_spans.partition_point(|span| span.range.end <= segment.start_column);
        let syntax_spans = line_spans[first_span..]
            .iter()
            .take_while(|span| span.range.start < segment.end_column)
            .filter_map(|span| {
                let start = span.range.start.max(segment.start_column);
                let end = span.range.end.min(segment.end_column);
                (start < end).then(|| SyntaxRenderSpan {
                    range: start - segment.start_column..end - segment.start_column,
                    color: span.color,
                })
            })
            .collect();

        for projection in &projected {
            if let Some(selection) = selection_plan_from_projected(
                decorations,
                *projection,
                segment,
                visible_row,
                layout,
            ) {
                selection_plans.push(selection);
            }
        }

        rows.push(RowRenderPlan {
            visible_row,
            line,
            start_column: segment.start_column,
            start_visual_column: segment.start_visual_column,
            y,
            text_x,
            text: text.to_owned(),
            line_number: line_decoration
                .filter(|_| !segment.is_continuation())
                .and_then(|decoration| decoration.line_number),
            is_active_line: active_display_line == Some(line),
            fold,
            hidden_lines,
            whitespace: whitespace_plan(
                line,
                text,
                segment.start_visual_column,
                decorations,
                layout,
            ),
            eol: segment
                .is_last
                .then(|| eol_plan(line, text, segment.start_visual_column, decorations, layout))
                .flatten(),
            indent_guides: if segment.is_continuation() {
                Vec::new()
            } else {
                indent_guide_plan(line, lookups.indent_guides(line), decorations, layout)
            },
            syntax_spans,
            projection: viewport
                .projection(line)
                .map_or_else(Vec::new, |projection| {
                    clipped_projection_fragments(
                        projection,
                        segment.start_column..segment.end_column,
                    )
                }),
        });
    }

    let caret = selections
        .main_range()
        .is_caret()
        .then(|| {
            caret_plan_for_position(
                buffer,
                viewport,
                decorations,
                selections.main_range().cursor,
                selections.main_range().cursor_virtual_column,
                &buffer.line(main_selection.cursor.line).unwrap_or_default(),
                layout,
                caret_row(main_selection.cursor),
            )
        })
        .flatten();

    RenderPlan {
        rows,
        selections: selection_plans,
        carets,
        caret,
    }
}

pub(crate) fn clipped_projection_fragments(
    projection: &FoldProjection,
    clip: Range<usize>,
) -> Vec<ProjectionFragment> {
    projection
        .fragments
        .iter()
        .filter_map(|fragment| {
            let range = match fragment {
                ProjectionFragment::Source { display_range, .. }
                | ProjectionFragment::Placeholder { display_range, .. } => display_range,
            };
            let start = range.start.max(clip.start);
            let end = range.end.min(clip.end);
            if start >= end {
                return None;
            }
            let display_range = start - clip.start..end - clip.start;
            Some(match fragment {
                ProjectionFragment::Source { source_start, .. } => ProjectionFragment::Source {
                    display_range,
                    source_start: EditorPosition::new(
                        source_start.line,
                        source_start.column + start - range.start,
                    ),
                },
                ProjectionFragment::Placeholder {
                    range, delimiter, ..
                } => ProjectionFragment::Placeholder {
                    display_range,
                    range: *range,
                    delimiter: *delimiter,
                },
            })
        })
        .collect()
}

fn projected_syntax_spans(
    projection: &FoldProjection,
    buffer: &EditorBuffer,
    syntax_cache: &SyntaxLineCache,
) -> Vec<SyntaxRenderSpan> {
    let mut spans = Vec::new();
    for fragment in &projection.fragments {
        if let ProjectionFragment::Source {
            display_range,
            source_start,
        } = fragment
        {
            let source = buffer.line(source_start.line).unwrap_or_default();
            for span in syntax_cache
                .spans_for_text(source_start.line, &source)
                .unwrap_or_default()
            {
                let start = span.range.start.max(source_start.column);
                let end = span
                    .range
                    .end
                    .min(source_start.column + display_range.len());
                if start < end {
                    spans.push(SyntaxRenderSpan {
                        range: display_range.start + start - source_start.column
                            ..display_range.start + end - source_start.column,
                        color: span.color,
                    });
                }
            }
        }
    }
    spans
}

pub(crate) fn project_folded_selection(
    selection: SelectionRange,
    owner: usize,
    buffer: &EditorBuffer,
    projection: &FoldProjection,
    tab_width: usize,
) -> Vec<ProjectedSelectionLine> {
    let range = buffer.clamp_range(selection.range());
    let mut selected = Vec::new();
    for fragment in &projection.fragments {
        let columns = match fragment {
            ProjectionFragment::Source {
                display_range,
                source_start,
            } => {
                let source = buffer.line(source_start.line).unwrap_or_default();
                let Some(line) = project_selection_line(
                    selection,
                    source_start.line,
                    buffer,
                    &source,
                    tab_width,
                ) else {
                    continue;
                };
                let start = line.start.column.max(source_start.column);
                let end = line
                    .end
                    .column
                    .min(source_start.column + display_range.len());
                if start >= end {
                    continue;
                }
                display_range.start + start - source_start.column
                    ..display_range.start + end - source_start.column
            }
            ProjectionFragment::Placeholder {
                display_range,
                range: fold,
                delimiter,
            } => {
                let start = EditorPosition::new(fold.start_line, delimiter.opening_column);
                let end = EditorPosition::new(fold.end_line, delimiter.closing_column);
                if range.start >= end || range.end <= start {
                    continue;
                }
                if selection.is_rectangular() {
                    let source = buffer.line(fold.start_line).unwrap_or_default();
                    let Some(line) = project_selection_line(
                        selection,
                        fold.start_line,
                        buffer,
                        &source,
                        tab_width,
                    ) else {
                        continue;
                    };
                    if line.end.column <= delimiter.opening_column {
                        continue;
                    }
                }
                display_range.clone()
            }
        };
        selected.push(ProjectedSelectionLine {
            line: owner,
            start: EditorPosition::new(owner, columns.start),
            end: EditorPosition::new(owner, columns.end),
            start_visual_column: visual_column_for(&projection.text, columns.start, tab_width),
            end_visual_column: visual_column_for(&projection.text, columns.end, tab_width),
            start_virtual_column: None,
            end_virtual_column: None,
        });
    }
    selected
}

fn project_selection_line(
    selection: SelectionRange,
    line: usize,
    buffer: &EditorBuffer,
    text: &str,
    tab_width: usize,
) -> Option<ProjectedSelectionLine> {
    match selection.shape {
        SelectionShape::Linear => {
            project_linear_selection_line(selection, line, buffer, text, tab_width)
        }
        SelectionShape::Rectangular(rectangular) => {
            let anchor = buffer.clamp_position(selection.anchor);
            let cursor = buffer.clamp_position(selection.cursor);
            let (first_line, last_line) = if anchor.line <= cursor.line {
                (anchor.line, cursor.line)
            } else {
                (cursor.line, anchor.line)
            };
            if !(first_line..=last_line).contains(&line) {
                return None;
            }

            let (start_visual_column, end_visual_column) = rectangular.visual_columns();
            let line_visual_width = visual_column_for(text, text.len(), tab_width);
            let start_column = byte_column_for(text, start_visual_column, tab_width);
            let end_column = byte_column_for(text, end_visual_column, tab_width);

            Some(ProjectedSelectionLine {
                line,
                start: EditorPosition::new(line, start_column),
                end: EditorPosition::new(line, end_column),
                start_visual_column,
                end_visual_column,
                start_virtual_column: (start_visual_column > line_visual_width)
                    .then_some(start_visual_column),
                end_virtual_column: (end_visual_column > line_visual_width)
                    .then_some(end_visual_column),
            })
        }
    }
}

fn project_linear_selection_line(
    selection: SelectionRange,
    line: usize,
    buffer: &EditorBuffer,
    text: &str,
    tab_width: usize,
) -> Option<ProjectedSelectionLine> {
    let range = buffer.clamp_range(selection.range());
    if !(range.start.line..=range.end.line).contains(&line) {
        return None;
    }

    let start_column = if line == range.start.line {
        range.start.column
    } else {
        0
    };
    let end_column = if line == range.end.line {
        range.end.column
    } else {
        text.len()
    };
    let start = buffer.clamp_position(EditorPosition::new(line, start_column));
    let end = buffer.clamp_position(EditorPosition::new(line, end_column));

    let start_visual_column = selection
        .anchor_virtual_column
        .filter(|_| start == selection.anchor)
        .unwrap_or_else(|| visual_column_for(text, start.column, tab_width));
    let mut end_visual_column = selection
        .cursor_virtual_column
        .filter(|_| end == selection.cursor)
        .unwrap_or_else(|| visual_column_for(text, end.column, tab_width));

    if start_visual_column == end_visual_column && line < range.end.line {
        end_visual_column = end_visual_column.saturating_add(1);
    }

    Some(ProjectedSelectionLine {
        line,
        start,
        end,
        start_visual_column,
        end_visual_column,
        start_virtual_column: selection
            .anchor_virtual_column
            .filter(|_| start == selection.anchor),
        end_virtual_column: selection
            .cursor_virtual_column
            .filter(|_| end == selection.cursor),
    })
}

fn selection_plan_from_projected(
    decorations: &DecorationModel,
    selection: ProjectedSelectionLine,
    segment: RowSegment,
    visible_row: usize,
    layout: EditorLayout,
) -> Option<SelectionRenderPlan> {
    if selection.start_visual_column == selection.end_visual_column {
        return None;
    }

    let start_visual_column = selection
        .start_visual_column
        .min(selection.end_visual_column)
        .max(segment.start_visual_column);
    let mut end_visual_column = selection
        .start_visual_column
        .max(selection.end_visual_column);
    if !segment.is_last {
        end_visual_column = end_visual_column.min(segment.end_visual_column);
    }
    if start_visual_column >= end_visual_column {
        return None;
    }
    let start_column = selection
        .start
        .column
        .clamp(segment.start_column, segment.end_column);
    let end_column = selection
        .end
        .column
        .clamp(segment.start_column, segment.end_column);

    Some(SelectionRenderPlan {
        line: selection.line,
        start_column,
        end_column,
        start_visual_column,
        end_visual_column,
        start_virtual_column: selection.start_virtual_column.filter(|_| segment.is_last),
        end_virtual_column: selection
            .end_virtual_column
            .filter(|_| segment.is_last)
            .or_else(|| {
                (end_visual_column > segment.end_visual_column).then_some(end_visual_column)
            }),
        y: row_y(visible_row, layout),
        x: x_for_visual_column(
            start_visual_column - segment.start_visual_column,
            layout,
            decorations,
        ),
        width: end_visual_column.saturating_sub(start_visual_column) as f32
            * layout.metrics.character_width,
    })
}

fn caret_plan_from_projected(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    selection: ProjectedSelectionLine,
    line_text: &str,
    layout: EditorLayout,
    caret_row: Option<usize>,
) -> Option<CaretRenderPlan> {
    if selection.start != selection.end
        || selection.start_visual_column != selection.end_visual_column
    {
        return None;
    }

    caret_plan_for_position(
        buffer,
        viewport,
        decorations,
        selection.end,
        selection
            .end_virtual_column
            .or(Some(selection.end_visual_column)),
        line_text,
        layout,
        caret_row,
    )
}

fn caret_plan_for_position(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    position: EditorPosition,
    virtual_column: Option<usize>,
    line_text: &str,
    layout: EditorLayout,
    caret_row: Option<usize>,
) -> Option<CaretRenderPlan> {
    let display_position = viewport.display_position(position)?;
    let display_text = viewport.display_text(display_position.line, buffer);
    let visible_row = caret_row
        .filter(|row| {
            viewport.visible_row_to_document_line(*row) == Some(display_position.line)
                && viewport.row_segment(*row, buffer).is_some_and(|segment| {
                    (segment.start_column..=segment.end_column).contains(&display_position.column)
                })
        })
        .or_else(|| viewport.position_to_visible_row(position))?;
    if visible_row < layout.scroll.first_visible_row
        || visible_row
            > layout
                .scroll
                .first_visible_row
                .saturating_add(layout.visible_row_capacity())
    {
        return None;
    }
    let segment = viewport.row_segment(visible_row, buffer)?;
    let source_visual = visual_column_for(
        line_text,
        position.column,
        decorations.settings.indent_width,
    );
    let display_visual = visual_column_for(
        &display_text,
        display_position.column,
        decorations.settings.indent_width,
    );
    let visual_column = virtual_column
        .filter(|column| *column > source_visual)
        .map(|column| display_visual + column - source_visual);

    Some(CaretRenderPlan {
        position,
        visual_column,
        x: x_for_visual_column(
            visual_column
                .unwrap_or_else(|| {
                    visual_column_for(
                        &display_text,
                        display_position.column,
                        decorations.settings.indent_width,
                    )
                })
                .saturating_sub(segment.start_visual_column),
            layout,
            decorations,
        ),
        y: row_y(visible_row, layout),
        height: layout.metrics.line_height,
    })
}

struct RenderDecorationLookups<'a> {
    decorations: &'a DecorationModel,
    visible: HashMap<usize, VisibleLineDecorations<'a>>,
    #[cfg(test)]
    metadata_visits: usize,
}

#[derive(Default)]
struct VisibleLineDecorations<'a> {
    hidden: Option<&'a HiddenLineSpan>,
    guides: Vec<&'a IndentGuide>,
}

impl<'a> RenderDecorationLookups<'a> {
    fn new(decorations: &'a DecorationModel, visible_lines: impl Iterator<Item = usize>) -> Self {
        let mut result = Self {
            decorations,
            visible: if decorations.hidden_line_spans.is_empty()
                && (!decorations.settings.show_indentation_guides
                    || decorations.indent_guides.is_empty())
            {
                HashMap::new()
            } else {
                visible_lines
                    .map(|line| (line, VisibleLineDecorations::default()))
                    .collect()
            },
            #[cfg(test)]
            metadata_visits: 0,
        };
        if result.visible.is_empty() {
            return result;
        }
        let first = *result.visible.keys().min().expect("visible line");
        let last = *result.visible.keys().max().expect("visible line");
        // Metadata can arrive unordered, and brace/indentation folds may share
        // a header. Scan it once per frame and retain only visible logical lines;
        // repeated wrapped rows reuse the same small lookup without rescanning
        // every guide in the document for each row.
        for span in &decorations.hidden_line_spans {
            #[cfg(test)]
            {
                result.metadata_visits += 1;
            }
            if span.header_line < first || span.header_line > last {
                continue;
            }
            if let Some(line) = result.visible.get_mut(&span.header_line)
                && line
                    .hidden
                    .is_none_or(|previous| previous.last_hidden_line <= span.last_hidden_line)
            {
                line.hidden = Some(span);
            }
        }
        if decorations.settings.show_indentation_guides {
            for guide in &decorations.indent_guides {
                #[cfg(test)]
                {
                    result.metadata_visits += 1;
                }
                if guide.line < first || guide.line > last {
                    continue;
                }
                if let Some(line) = result.visible.get_mut(&guide.line) {
                    line.guides.push(guide);
                }
            }
        }
        result
    }

    fn line_decoration(&self, line: usize) -> Option<&'a LineDecoration> {
        self.decorations.line_decorations.get(line)
    }

    fn hidden_line_span(&self, line: usize) -> Option<&'a HiddenLineSpan> {
        self.visible.get(&line).and_then(|line| line.hidden)
    }

    fn indent_guides(&self, line: usize) -> impl Iterator<Item = &'a IndentGuide> {
        self.visible
            .get(&line)
            .into_iter()
            .flat_map(|line| line.guides.iter().copied())
    }
}

#[cfg(test)]
mod decoration_lookup_tests {
    use super::*;
    use crate::editor::{DecorationSettings, FoldModel, ScrollOffset};

    #[test]
    fn visible_decoration_index_matches_unordered_metadata_with_one_scan() {
        let mut decorations = DecorationModel::new(DecorationSettings::default());
        decorations.indent_guides = (0..10_000)
            .rev()
            .flat_map(|line| [3, 1, 2].map(|depth| IndentGuide { line, depth }))
            .collect();
        decorations.hidden_line_spans = [
            (700, 730),
            (701, 710),
            (9_999, 10_002),
            (700, 725),
            (700, 740),
            (700, 740),
        ]
        .map(|(header_line, last_hidden_line)| HiddenLineSpan {
            header_line,
            first_hidden_line: header_line + 1,
            last_hidden_line,
        })
        .to_vec();
        let visible = [700, 701, 701, 703, 708, 708];
        let lookups = RenderDecorationLookups::new(&decorations, visible.into_iter());
        let visits = decorations.indent_guides.len() + decorations.hidden_line_spans.len();
        assert_eq!(lookups.metadata_visits, visits);
        assert_eq!(
            lookups.visible.len(),
            4,
            "wrap continuations share one logical-line index"
        );
        for _ in 0..40 {
            for line in visible {
                assert_eq!(
                    lookups.indent_guides(line).collect::<Vec<_>>(),
                    decorations
                        .indent_guides
                        .iter()
                        .filter(|guide| guide.line == line)
                        .collect::<Vec<_>>(),
                    "original depth order must be preserved",
                );
                let expected = decorations
                    .hidden_line_spans
                    .iter()
                    .filter(|span| span.header_line == line)
                    .max_by_key(|span| span.last_hidden_line);
                assert_eq!(lookups.hidden_line_span(line), expected);
                if let Some(expected) = expected {
                    assert!(
                        std::ptr::eq(lookups.hidden_line_span(line).unwrap(), expected),
                        "equal-length folds keep the original last-match preference"
                    );
                }
            }
        }
        assert_eq!(
            lookups.metadata_visits, visits,
            "row lookup must not rescan document metadata"
        );
        assert!(lookups.indent_guides(9_999).next().is_none());
        assert!(lookups.hidden_line_span(9_999).is_none());
    }

    #[test]
    fn decoration_index_preserves_wrapped_fold_rows_during_scroll_reversal() {
        let buffer = EditorBuffer::from_text("    漢字\tcontinuation text\n".repeat(120));
        let folds = FoldModel::with_collapsed(
            vec![
                FoldRange::new(5, 8),
                FoldRange::new(5, 10),
                FoldRange::new(60, 64),
            ],
            [
                FoldRange::new(5, 8),
                FoldRange::new(5, 10),
                FoldRange::new(60, 64),
            ]
            .into_iter()
            .collect(),
        );
        let viewport = ViewportModel::new_wrapped(&buffer, &folds, 12, 4);
        let decorations = DecorationModel::from_folds(
            DecorationSettings::default(),
            buffer.line_count(),
            &folds,
            (0..buffer.line_count())
                .rev()
                .flat_map(|line| [2, 1].map(|depth| IndentGuide { line, depth }))
                .collect(),
        );
        let mut saw_fold = false;
        let mut saw_continuation = false;
        for first_visible_row in [0, 4, 12, 28, 12, 4, 0] {
            let layout = EditorLayout::new(
                EditorMetrics::default(),
                ScrollOffset {
                    first_visible_row,
                    horizontal_px: 0.0,
                },
                320.0,
                150.0,
            );
            let plan = build_render_plan_with_cache(
                &buffer,
                &viewport,
                &decorations,
                EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
                layout,
                &SyntaxLineCache::default(),
            );
            for row in plan.rows {
                let segment = viewport.row_segment(row.visible_row, &buffer).unwrap();
                let expected_depths = if segment.is_continuation() {
                    vec![]
                } else {
                    decorations
                        .indent_guides
                        .iter()
                        .filter(|guide| guide.line == row.line)
                        .map(|guide| guide.depth)
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    row.indent_guides
                        .iter()
                        .map(|guide| guide.depth)
                        .collect::<Vec<_>>(),
                    expected_depths
                );
                let expected_hidden = decorations
                    .hidden_line_spans
                    .iter()
                    .filter(|span| span.header_line == row.line && segment.is_last)
                    .max_by_key(|span| span.last_hidden_line);
                assert_eq!(
                    row.hidden_lines
                        .map(|span| (span.first_hidden_line, span.last_hidden_line)),
                    expected_hidden.map(|span| (span.first_hidden_line, span.last_hidden_line))
                );
                saw_fold |= row.hidden_lines.is_some();
                saw_continuation |= segment.is_continuation();
            }
        }
        assert!(
            saw_fold && saw_continuation,
            "exercise both fold headers and wrapped CJK fragments"
        );
    }

    #[test]
    fn disabled_guides_and_empty_viewports_do_not_scan_unused_metadata() {
        let mut decorations = DecorationModel::new(DecorationSettings {
            show_indentation_guides: false,
            ..DecorationSettings::default()
        });
        decorations.indent_guides = (0..10_000)
            .map(|line| IndentGuide { line, depth: 1 })
            .collect();
        let disabled = RenderDecorationLookups::new(&decorations, [10, 11].into_iter());
        assert_eq!(disabled.metadata_visits, 0);
        assert!(disabled.indent_guides(10).next().is_none());
        decorations.settings.show_indentation_guides = true;
        let empty = RenderDecorationLookups::new(&decorations, std::iter::empty());
        assert_eq!(empty.metadata_visits, 0);
        assert!(empty.visible.is_empty());
    }
}

fn whitespace_plan(
    line: usize,
    text: &str,
    start_visual_column: usize,
    decorations: &DecorationModel,
    layout: EditorLayout,
) -> Vec<WhitespaceRenderPlan> {
    if !decorations.settings.show_spaces && !decorations.settings.show_tabs {
        return Vec::new();
    }

    let mut visual_column = start_visual_column;

    text.char_indices()
        .filter_map(|(column, ch)| {
            let kind = match ch {
                ' ' if decorations.settings.show_spaces => WhitespaceKind::Space,
                '\t' if decorations.settings.show_tabs => WhitespaceKind::Tab,
                _ => {
                    visual_column += visual_width_with_tab_width(
                        ch,
                        visual_column,
                        decorations.settings.indent_width,
                    );
                    return None;
                }
            };
            let x = x_for_visual_column(visual_column - start_visual_column, layout, decorations);

            visual_column +=
                visual_width_with_tab_width(ch, visual_column, decorations.settings.indent_width);

            Some(WhitespaceRenderPlan {
                line,
                column,
                x,
                kind,
            })
        })
        .collect()
}

fn eol_plan(
    line: usize,
    text: &str,
    start_visual_column: usize,
    decorations: &DecorationModel,
    layout: EditorLayout,
) -> Option<EolRenderPlan> {
    decorations
        .settings
        .show_end_of_line_markers
        .then(|| EolRenderPlan {
            line,
            x: x_for_visual_column(
                visual_column_for_with_offset(
                    text,
                    text.len(),
                    decorations.settings.indent_width,
                    start_visual_column,
                ) - start_visual_column,
                layout,
                decorations,
            ),
        })
}

fn indent_guide_plan<'a>(
    line: usize,
    indent_guides: impl Iterator<Item = &'a IndentGuide>,
    decorations: &DecorationModel,
    layout: EditorLayout,
) -> Vec<IndentGuideRenderPlan> {
    if !decorations.settings.show_indentation_guides {
        return Vec::new();
    }

    indent_guides
        .map(|guide| IndentGuideRenderPlan {
            line,
            depth: guide.depth,
            x: x_for_visual_column(
                guide.depth * decorations.settings.indent_width.max(1),
                layout,
                decorations,
            ),
        })
        .collect()
}
