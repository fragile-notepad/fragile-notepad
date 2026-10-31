//! Selection hit tests and timed scrolling while a drag leaves the viewport.

use iced::advanced::{Shell, text};
use iced::time::{Duration, Instant};
use iced::{Font, Point, Rectangle};

use crate::editor::action::EditorAction;
use crate::editor::buffer::EditorBuffer;
use crate::editor::layout::{EditorLayout, HitTarget, hit_visible_row};
use crate::editor::position::{EditorPosition, EditorSelection, SelectionRange};
use crate::editor::render::project_folded_selection;
use crate::editor::viewport::ViewportModel;

use super::super::font::{editor_font_runs_for_display_fragment, editor_font_runs_for_fragment};
use super::super::line_cache::{
    LineGeometry, measured_text_hit_target_with_context, measured_virtual_caret_x,
};
use super::super::state::AdvancedEditorState;
use super::InteractionContext;

const DRAG_SCROLL_INTERVAL_MS: u64 = 50;
const MAX_DRAG_SCROLL_LINES: i32 = 8;

/// Includes the line ending when a following logical line exists.
pub(super) fn whole_line_selection(buffer: &EditorBuffer, line: usize) -> EditorSelection {
    let start = buffer.clamp_position(EditorPosition::new(line, 0));
    let end = if start.line + 1 < buffer.line_count() {
        EditorPosition::new(start.line + 1, 0)
    } else {
        last_line_end_position(buffer)
    };

    EditorSelection::new(start, end)
}

/// Retains the clicked visual row at a wrapped segment boundary.
pub(super) fn place_caret_action(
    viewport: &ViewportModel,
    position: EditorPosition,
    row: Option<usize>,
) -> EditorAction {
    match row.filter(|_| viewport.wrap_columns().is_some()) {
        Some(row) => EditorAction::PlaceCaretOnRow { position, row },
        None => EditorAction::PlaceCaret(position),
    }
}

fn drag_scroll_lines(y: f32, layout: EditorLayout) -> i32 {
    let distance = if y < 0.0 {
        y
    } else if y > layout.height {
        y - layout.height
    } else {
        return 0;
    };
    let lines = (1 + (distance.abs() / (layout.metrics.line_height.max(1.0) * 2.0)) as i32)
        .min(MAX_DRAG_SCROLL_LINES);

    if distance.is_sign_negative() {
        -lines
    } else {
        lines
    }
}

fn drag_scroll_row(y: f32, layout: EditorLayout, row_count: usize) -> usize {
    let first_row = layout.scroll.first_visible_row;
    let lines = drag_scroll_lines(y, layout);
    let last_page = row_count.saturating_sub(layout.complete_visible_row_capacity().max(1));

    if lines.is_negative() {
        first_row.saturating_sub(lines.unsigned_abs() as usize)
    } else if lines > 0 && first_row < last_page {
        first_row.saturating_add(lines as usize).min(last_page)
    } else {
        first_row
    }
}

/// Advances a selection or text-drop target and schedules continued edge scrolling.
pub(super) fn update_drag_selection<Message, Renderer>(
    context: &InteractionContext<'_, Message>,
    state: &mut AdvancedEditorState<Renderer::Paragraph>,
    bounds: Rectangle,
    mut layout: EditorLayout,
    renderer: &Renderer,
    shell: &mut Shell<'_, Message>,
    now: Instant,
) where
    Renderer: text::Renderer<Font = Font>,
{
    let Some(screen_position) = state.drag_position else {
        return;
    };
    if state.text_drag.as_ref().is_some_and(|drag| !drag.started)
        || (state.drag_anchor.is_none() && state.text_drag.is_none())
    {
        return;
    }
    if !state.is_focused || !state.is_window_focused {
        state.cancel_pointer_drag();
        return;
    }

    let position = Point::new(screen_position.x - bounds.x, screen_position.y - bounds.y);
    let target_row = drag_scroll_row(position.y, layout, context.viewport.visible_row_count());
    if target_row != layout.scroll.first_visible_row
        || position.y < 0.0 && layout.vertical_offset_px > 0.0
    {
        let scroll_at = state.drag_scroll_at.unwrap_or(now);
        if now >= scroll_at {
            layout.scroll.first_visible_row = target_row;
            layout.vertical_offset_px = 0.0;
            state.smooth_scroll = None;
            shell.publish((context.on_action)(EditorAction::ScrollToRow(target_row)));
            state.drag_scroll_at = Some(now + Duration::from_millis(DRAG_SCROLL_INTERVAL_MS));
        }
        if let Some(scroll_at) = state.drag_scroll_at {
            shell.request_redraw_at(scroll_at);
        }
    } else {
        state.drag_scroll_at = None;
    }

    // Hit-test the destination viewport, including its top padding. Otherwise an
    // upward drag becomes Outside and the selection stops at the previous row.
    let (cursor, row) = drag_selection_position(position, layout, context, renderer);
    if let Some(drag) = state.text_drag.as_mut() {
        let target = Some((cursor, row));
        if drag.target != target {
            drag.target = target;
            shell.request_redraw();
        }
        state.clear_text_click();
        return;
    }
    let Some(anchor) = state.drag_anchor else {
        return;
    };
    let selection = EditorSelection::new(anchor, cursor);
    if context.selections.main() != selection
        || context.viewport.wrap_columns().is_some() && row != context.caret_row
    {
        state.reset_caret_blink();
        state.clear_text_click();
        let action = match row.filter(|_| context.viewport.wrap_columns().is_some()) {
            Some(row) => EditorAction::SelectRegionOnRow { selection, row },
            None => EditorAction::SelectRegion(selection),
        };
        shell.publish((context.on_action)(action));
        shell.request_redraw();
    }
}

fn drag_selection_position<Message, Renderer>(
    position: Point,
    layout: EditorLayout,
    context: &InteractionContext<'_, Message>,
    renderer: &Renderer,
) -> (EditorPosition, Option<usize>)
where
    Renderer: text::Renderer<Font = Font>,
{
    let last_page = context
        .viewport
        .visible_row_count()
        .saturating_sub(layout.complete_visible_row_capacity().max(1));
    if position.y < 0.0 && layout.scroll.first_visible_row == 0 {
        return (EditorPosition::new(0, 0), Some(0));
    }
    if position.y > layout.height && layout.scroll.first_visible_row >= last_page {
        let position = last_line_end_position(context.buffer);
        return (position, context.viewport.position_to_visible_row(position));
    }

    let last_row_y = layout.metrics.padding_top
        + (layout.complete_visible_row_capacity().max(1) as f32 - 0.5)
            * layout.metrics.line_height.max(1.0);
    let position = Point::new(
        position.x.clamp(0.0, layout.width.max(0.0)),
        if (0.0..layout.height).contains(&position.y) {
            position
                .y
                .max((layout.metrics.padding_top - layout.vertical_offset_px).max(0.0))
        } else {
            position.y.clamp(layout.metrics.padding_top, last_row_y)
        },
    );

    let row = hit_visible_row(position.y, layout, context.viewport).map(|(row, _)| row);
    let target = match measured_text_hit_target_with_context(
        position,
        layout,
        context.buffer,
        context.viewport,
        context.decorations,
        renderer,
        context.cjk_context,
    ) {
        HitTarget::Text(position) => position,
        _ => last_line_end_position(context.buffer),
    };
    (target, row)
}

/// Tests the painted selection, including rectangular columns beyond line ends.
pub(super) fn selection_contains_pointer<Message, Renderer>(
    context: &InteractionContext<'_, Message>,
    position: EditorPosition,
    pointer: Point,
    layout: EditorLayout,
    renderer: &Renderer,
) -> bool
where
    Renderer: text::Renderer<Font = Font>,
{
    context.selections.ranges().iter().any(|selection| {
        let range = selection.range();
        if position.line < range.start.line || position.line > range.end.line {
            return false;
        }
        if !selection.is_rectangular() {
            return range.start <= position && position < range.end;
        }

        // Project only the clicked row so a large rectangular selection does
        // not allocate geometry for the rest of the document on right click.
        let row = SelectionRange {
            anchor: EditorPosition::new(position.line, 0),
            cursor: EditorPosition::new(position.line, 0),
            ..*selection
        }
        .projected_lines(context.buffer, context.decorations.settings.indent_width);
        let Some(row) = row.first() else {
            return false;
        };
        let Some((visible_row, owner)) = hit_visible_row(pointer.y, layout, context.viewport)
        else {
            return false;
        };
        if let Some(projection) = context.viewport.projection(owner) {
            let Some(segment) = context.viewport.row_segment(visible_row, context.buffer) else {
                return false;
            };
            let fragment = &projection.text[segment.start_column..segment.end_column];
            let geometry = LineGeometry::new_with_font_runs(
                fragment,
                layout.metrics,
                renderer,
                segment.start_visual_column,
                context.decorations.settings.indent_width,
                &editor_font_runs_for_display_fragment(
                    fragment,
                    context.cjk_context,
                    owner,
                    segment.start_column,
                    context.viewport,
                ),
                true,
            );
            // A collapsed fold can combine several source lines in one display
            // row. Project this source line before comparing measured x offsets.
            let source_selection = SelectionRange {
                anchor: EditorPosition::new(position.line, 0),
                cursor: EditorPosition::new(position.line, 0),
                ..*selection
            };
            return project_folded_selection(
                source_selection,
                owner,
                context.buffer,
                projection,
                context.decorations.settings.indent_width,
            )
            .iter()
            .any(|selected| {
                let start = selected.start.column.max(segment.start_column);
                let end = selected.end.column.min(segment.end_column);
                if start >= end {
                    return false;
                }
                let start_x = measured_virtual_caret_x(
                    &geometry,
                    start - segment.start_column,
                    None,
                    layout,
                    context.decorations,
                );
                let end_x = measured_virtual_caret_x(
                    &geometry,
                    end - segment.start_column,
                    None,
                    layout,
                    context.decorations,
                );
                pointer.x >= start_x.min(end_x) && pointer.x < start_x.max(end_x)
            });
        }
        let text = context.buffer.line(position.line).unwrap_or_default();
        let Some(segment) = hit_visible_row(pointer.y, layout, context.viewport)
            .and_then(|(visible_row, _)| context.viewport.row_segment(visible_row, context.buffer))
        else {
            return false;
        };
        let fragment = &text[segment.start_column..segment.end_column];
        let geometry = LineGeometry::new_with_font_runs(
            fragment,
            layout.metrics,
            renderer,
            segment.start_visual_column,
            context.decorations.settings.indent_width,
            &editor_font_runs_for_fragment(
                fragment,
                context.cjk_context,
                position.line,
                segment.start_column,
            ),
            super::super::line_cache::fold_needs_measured_geometry(
                position.line,
                context.viewport,
                context.decorations,
            ),
        );
        let start_x = measured_virtual_caret_x(
            &geometry,
            row.start.column.saturating_sub(segment.start_column),
            row.start_virtual_column.map(|column| {
                if segment.is_last {
                    column
                } else {
                    column.min(segment.end_visual_column)
                }
            }),
            layout,
            context.decorations,
        );
        let end_x = measured_virtual_caret_x(
            &geometry,
            row.end.column.saturating_sub(segment.start_column),
            row.end_virtual_column.map(|column| {
                if segment.is_last {
                    column
                } else {
                    column.min(segment.end_visual_column)
                }
            }),
            layout,
            context.decorations,
        );

        pointer.x >= start_x.min(end_x) && pointer.x < start_x.max(end_x)
    })
}

pub(super) fn last_line_end_position(buffer: &EditorBuffer) -> EditorPosition {
    let line = buffer.line_count().saturating_sub(1);
    let column = buffer.line(line).map(|text| text.len()).unwrap_or(0);

    buffer.clamp_position(EditorPosition::new(line, column))
}
