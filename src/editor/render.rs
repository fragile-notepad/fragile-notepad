use super::buffer::EditorBuffer;
use super::decoration::{DecorationModel, HiddenLineSpan, IndentGuide, LineDecoration};
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
use std::ops::Range;

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
}

impl RowRenderPlan {
    /// Returns the inline indicator only when this row hides a collapsed block.
    /// `measured_text_end_x` is the editor-local endpoint of the row's text.
    pub fn collapsed_indicator_bounds(
        &self,
        metrics: EditorMetrics,
        measured_text_end_x: f32,
    ) -> Option<Rectangle> {
        self.hidden_lines
            .filter(|hidden| hidden.hidden_line_count > 0)
            .map(|_| {
                collapsed_fold_indicator_bounds(
                    metrics,
                    self.y,
                    measured_text_end_x,
                    self.eol.is_some(),
                )
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
/// When `fast_text` is enabled and every visible row is tab-free ASCII text
/// without syntax spans, the widget can batch all visible rows into a single
/// clipped multiline text item. Highlighted rows are kept separate so syntax
/// colors remain visible during active scrolling.
pub fn planned_text_draws(plan: &RenderPlan, fast_text: bool) -> usize {
    if fast_text
        && plan.rows.iter().all(|row| {
            row.syntax_spans.is_empty()
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

/// Returns the boxed ellipsis bounds after a collapsed block's header text.
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
    let height = (metrics.line_height * 0.75)
        .max(8.0)
        .min(metrics.line_height.max(0.0));
    let marker_width = if show_eol_markers {
        end_of_line_marker_reservation(metrics.character_width)
    } else {
        0.0
    };

    Rectangle {
        x: measured_text_end_x + marker_width + gap,
        y: row_y + (metrics.line_height - height) / 2.0,
        width,
        height,
    }
}

/// Horizontal space needed after a header, excluding any EOL marker.
pub fn collapsed_fold_indicator_reservation(character_width: f32) -> f32 {
    let (gap, width) = collapsed_fold_indicator_dimensions(character_width);
    gap + width
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
    let caret_row = |position| {
        caret_rows
            .iter()
            .find_map(|(caret, row)| (*caret == position).then_some(*row))
    };
    let active_line = main_selection.cursor.line;
    let mut rows = Vec::new();
    let mut selection_plans = Vec::new();
    let mut carets = Vec::new();
    let mut cached_line = None;
    let mut line_text = String::new();
    let mut line_spans = Vec::new();
    let mut projected = Vec::new();
    let lookups = RenderDecorationLookups::new(decorations);
    let text_x = scrolled_text_origin_x(layout, decorations);
    let max_row = layout
        .scroll
        .first_visible_row
        .saturating_add(layout.visible_row_capacity());
    for visible_row in layout.scroll.first_visible_row..=max_row {
        let Some(line) = viewport.visible_row_to_document_line(visible_row) else {
            break;
        };
        let Some(segment) = viewport.row_segment(visible_row, buffer) else {
            break;
        };
        if cached_line != Some(line) {
            cached_line = Some(line);
            line_text = buffer.line(line).unwrap_or_default();
            line_spans = syntax_cache
                .spans_for_text(line, &line_text)
                .unwrap_or_default();
            projected.clear();
            for selection in selections.ranges() {
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
            .filter(|_| segment.is_last)
            .map(|span| HiddenLineRenderPlan {
                first_hidden_line: span.first_hidden_line,
                last_hidden_line: span.last_hidden_line,
                hidden_line_count: span.hidden_line_count(),
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
            is_active_line: line == active_line,
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
    let visible_row = caret_row
        .filter(|row| {
            viewport.visible_row_to_document_line(*row) == Some(position.line)
                && viewport.row_segment(*row, buffer).is_some_and(|segment| {
                    (segment.start_column..=segment.end_column).contains(&position.column)
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
    let visual_column = virtual_column.filter(|column| {
        *column
            > visual_column_for(
                line_text,
                position.column,
                decorations.settings.indent_width,
            )
    });

    Some(CaretRenderPlan {
        position,
        visual_column,
        x: x_for_visual_column(
            visual_column
                .unwrap_or_else(|| {
                    visual_column_for(
                        line_text,
                        position.column,
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
}

impl<'a> RenderDecorationLookups<'a> {
    fn new(decorations: &'a DecorationModel) -> Self {
        Self { decorations }
    }

    fn line_decoration(&self, line: usize) -> Option<&'a LineDecoration> {
        self.decorations.line_decorations.get(line)
    }

    fn hidden_line_span(&self, line: usize) -> Option<&'a HiddenLineSpan> {
        self.decorations
            .hidden_line_spans
            .iter()
            .find(|span| span.header_line == line)
    }

    fn indent_guides(&self, line: usize) -> impl Iterator<Item = &'a IndentGuide> {
        self.decorations
            .indent_guides
            .iter()
            .filter(move |guide| guide.line == line)
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
