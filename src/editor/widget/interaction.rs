use iced::advanced::{InputMethod, Shell, input_method, mouse, text};
use iced::keyboard;
use iced::time::{Duration, Instant};
use iced::{Event, Font, Pixels, Point, Rectangle, window};

use crate::core::ShortcutMap;
use crate::editor::action::EditorAction;
use crate::editor::buffer::EditorBuffer;
use crate::editor::cjk::CjkContext;
use crate::editor::decoration::DecorationModel;
use crate::editor::layout::{EditorLayout, HitTarget, hit_test, hit_visible_row};
use crate::editor::position::{EditorPosition, EditorSelection, SelectionRange, SelectionSet};
use crate::editor::render::{project_folded_selection, text_size};
use crate::editor::viewport::ViewportModel;

use super::actions::key_action;
use super::font::{editor_font_runs_for_display_fragment, editor_font_runs_for_fragment};
use super::line_cache::{
    LineGeometry, measured_position_point_with_context, measured_text_hit_target_with_context,
    measured_virtual_caret_x,
};
use super::scrollbar::{scrollbar_row_for_position, vertical_scrollbar_geometry};
use super::state::{AdvancedEditorState, CARET_BLINK_INTERVAL_MS, TextDrag};

const FAST_SCROLL_SETTLE_MS: u64 = 120;
const DRAG_SCROLL_INTERVAL_MS: u64 = 50;
const MAX_DRAG_SCROLL_LINES: i32 = 8;
const TEXT_DRAG_THRESHOLD: f32 = 4.0;

pub(super) struct InteractionContext<'a, Message> {
    pub(super) buffer: &'a EditorBuffer,
    pub(super) viewport: &'a ViewportModel,
    pub(super) decorations: &'a DecorationModel,
    pub(super) selections: &'a SelectionSet,
    pub(super) metrics: crate::editor::layout::EditorMetrics,
    pub(super) caret_row: Option<usize>,
    pub(super) scroll_speed: f32,
    pub(super) viewport_key: u64,
    pub(super) cjk_context: Option<&'a CjkContext>,
    pub(super) shortcuts: &'a ShortcutMap,
    pub(super) on_action: &'a dyn Fn(EditorAction) -> Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UpdateOutcome {
    pub(super) perf_event: &'static str,
    pub(super) should_capture: bool,
}

impl Default for UpdateOutcome {
    fn default() -> Self {
        Self {
            perf_event: "editor_update",
            should_capture: false,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_event<Message, Renderer>(
    context: InteractionContext<'_, Message>,
    state: &mut AdvancedEditorState<Renderer::Paragraph>,
    event: &Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    editor_layout: EditorLayout,
    renderer: &Renderer,
    shell: &mut Shell<'_, Message>,
) -> UpdateOutcome
where
    Message: Clone,
    Renderer: iced::advanced::Renderer + text::Renderer<Font = Font>,
{
    let mut outcome = UpdateOutcome::default();
    if state
        .viewport_geometry
        .is_some_and(|geometry| geometry.0 != context.viewport_key)
        || state
            .text_drag
            .as_ref()
            .is_some_and(|drag| &drag.source != context.selections)
    {
        state.cancel_pointer_drag();
    }
    // Rendering includes a partially visible bottom row; caret navigation must
    // count only complete rows so the insertion point cannot remain clipped.
    let visible_rows = editor_layout.complete_visible_row_capacity().max(1);
    let text_width =
        (bounds.width - context.metrics.text_origin_x(context.decorations) - 14.0).max(1.0) as u32;
    let character_width_milli = (context.metrics.character_width * 1000.0).max(1.0) as u32;
    let font_size_milli = (text_size(context.metrics) * 1000.0).round().max(1.0) as u32;
    let hint_factor_milli = renderer
        .scale_factor()
        .map(|scale| (scale * 1000.0).round().max(1.0) as u32);
    let geometry = (
        context.viewport_key,
        visible_rows,
        text_width,
        character_width_milli,
        font_size_milli,
        hint_factor_milli,
    );
    if state.viewport_geometry != Some(geometry) {
        state.viewport_geometry = Some(geometry);
        shell.publish((context.on_action)(EditorAction::ViewportChanged {
            visible_rows,
            text_width,
            character_width_milli,
            font_size_milli,
            hint_factor_milli,
        }));
    }

    match event {
        Event::Window(window::Event::Unfocused) => {
            state.is_window_focused = false;
            state.cancel_pointer_drag();
            state.clear_text_click();
            shell.request_redraw();
        }
        Event::Window(window::Event::Focused) => {
            state.is_window_focused = true;
            state.reset_caret_blink();
            shell.request_redraw();
        }
        Event::Window(window::Event::RedrawRequested(now)) => {
            state.caret_now.set(*now);
            update_drag_selection(
                &context,
                state,
                bounds,
                editor_layout,
                renderer,
                shell,
                *now,
            );
            request_caret_blink_frame(state, shell);
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            state.cancel_pointer_drag();
            let Some(position) = cursor.position_in(bounds) else {
                state.is_focused = false;
                state.clear_text_click();
                state.preedit = None;
                shell.request_redraw();
                shell.request_input_method(&input_method(
                    &context,
                    state,
                    editor_layout,
                    bounds,
                    renderer,
                ));
                return outcome;
            };

            if let Some(scrollbar) =
                vertical_scrollbar_geometry(editor_layout, context.viewport.visible_row_count())
                && scrollbar.track.contains(position)
            {
                if scrollbar.thumb.contains(position) {
                    state.scrollbar_grab_offset_y = Some(position.y - scrollbar.thumb.y);
                } else {
                    let target_row = scrollbar_row_for_position(
                        position.y,
                        0.0,
                        editor_layout,
                        context.viewport.visible_row_count(),
                    );
                    shell.publish((context.on_action)(EditorAction::ScrollToRow(target_row)));
                    state.scrollbar_grab_offset_y =
                        Some((scrollbar.thumb.height / 2.0).min(position.y));
                }

                state.is_focused = true;
                state.reset_caret_blink();
                state.clear_text_click();
                shell.publish((context.on_action)(EditorAction::Focus));
                shell.request_redraw();
                outcome.should_capture = true;
                shell.capture_event();
                return outcome;
            }

            let clicked_row =
                hit_visible_row(position.y, editor_layout, context.viewport).map(|(row, _)| row);
            let clicked_row_start = clicked_row
                .and_then(|row| {
                    let line = context.viewport.visible_row_to_document_line(row)?;
                    let segment = context.viewport.row_segment(row, context.buffer)?;
                    Some(
                        context
                            .viewport
                            .source_position(EditorPosition::new(line, segment.start_column))
                            .column,
                    )
                })
                .unwrap_or(0);
            let hit = hit_test(
                position.x,
                position.y,
                editor_layout,
                context.buffer,
                context.viewport,
                context.decorations,
            );
            let hit = match hit {
                HitTarget::Text(_) => measured_text_hit_target_with_context(
                    position,
                    editor_layout,
                    context.buffer,
                    context.viewport,
                    context.decorations,
                    renderer,
                    context.cjk_context,
                ),
                other => other,
            };

            match hit {
                HitTarget::FoldControl { range, .. } => {
                    state.is_focused = true;
                    state.reset_caret_blink();
                    state.clear_text_click();
                    shell.publish((context.on_action)(EditorAction::Focus));
                    shell.publish((context.on_action)(EditorAction::ToggleFold(range)));
                    shell.request_redraw();
                }
                HitTarget::Text(position) => {
                    state.is_focused = true;
                    state.reset_caret_blink();
                    let is_double_click = state.record_text_click(position, state.caret_now.get());
                    if !is_double_click
                        && selection_contains_pointer(
                            &context,
                            position,
                            cursor.position_in(bounds).unwrap(),
                            editor_layout,
                            renderer,
                        )
                    {
                        state.text_drag = Some(TextDrag {
                            source: context.selections.clone(),
                            buffer: context.buffer.clone(),
                            pressed_at: cursor.position().unwrap(),
                            clicked_position: position,
                            clicked_row,
                            target: None,
                            started: false,
                        });
                        state.preedit = None;
                        shell.request_input_method(&InputMethod::<&str>::Disabled);
                        shell.publish((context.on_action)(EditorAction::Focus));
                        shell.request_redraw();
                        outcome.should_capture = true;
                        shell.capture_event();
                        return outcome;
                    }
                    state.drag_anchor = (!is_double_click).then_some(position);
                    shell.publish((context.on_action)(EditorAction::Focus));
                    shell.publish((context.on_action)(place_caret_action(
                        context.viewport,
                        position,
                        clicked_row,
                    )));
                    if is_double_click {
                        state.clear_text_click();
                        let action = if position.column == clicked_row_start {
                            EditorAction::SelectRegion(whole_line_selection(
                                context.buffer,
                                position.line,
                            ))
                        } else {
                            EditorAction::SelectWordAt(position)
                        };
                        shell.publish((context.on_action)(action));
                    }
                    shell.request_redraw();
                }
                HitTarget::GutterLine { line } | HitTarget::HiddenLineIndicator { line } => {
                    state.is_focused = true;
                    state.reset_caret_blink();
                    let position = EditorPosition::new(line, 0);
                    let is_double_click = state.record_text_click(position, state.caret_now.get());
                    if is_double_click {
                        state.clear_text_click();
                    }
                    state.drag_anchor = (!is_double_click).then_some(position);
                    shell.publish((context.on_action)(EditorAction::Focus));
                    shell.publish((context.on_action)(if is_double_click {
                        EditorAction::SelectRegion(whole_line_selection(context.buffer, line))
                    } else {
                        EditorAction::PlaceCaret(position)
                    }));
                    shell.request_redraw();
                }
                HitTarget::Outside => {
                    state.is_focused = true;
                    state.reset_caret_blink();
                    state.clear_text_click();
                    state.drag_anchor = None;
                    shell.publish((context.on_action)(EditorAction::Focus));
                    shell.publish((context.on_action)(EditorAction::PlaceCaret(
                        last_line_end_position(context.buffer),
                    )));
                    shell.request_redraw();
                }
            }

            outcome.should_capture = true;
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
            if let Some(position) = cursor.position_in(bounds)
                && position.x >= context.metrics.text_origin_x(context.decorations)
                && !vertical_scrollbar_geometry(editor_layout, context.viewport.visible_row_count())
                    .is_some_and(|scrollbar| scrollbar.track.contains(position))
            {
                state.is_focused = true;
                state.reset_caret_blink();
                state.cancel_pointer_drag();
                state.clear_text_click();
                state.preedit = None;

                let text_position =
                    Point::new(position.x, position.y.max(context.metrics.padding_top));
                let hit = measured_text_hit_target_with_context(
                    text_position,
                    editor_layout,
                    context.buffer,
                    context.viewport,
                    context.decorations,
                    renderer,
                    context.cjk_context,
                );
                let target = match hit {
                    HitTarget::Text(target) => target,
                    _ => last_line_end_position(context.buffer),
                };
                shell.publish((context.on_action)(EditorAction::Focus));
                if !matches!(hit, HitTarget::Text(_))
                    || !selection_contains_pointer(
                        &context,
                        target,
                        position,
                        editor_layout,
                        renderer,
                    )
                {
                    shell.publish((context.on_action)(place_caret_action(
                        context.viewport,
                        target,
                        hit_visible_row(text_position.y, editor_layout, context.viewport)
                            .map(|(row, _)| row),
                    )));
                }
                shell.request_redraw();
                outcome.should_capture = true;
            }
        }
        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
            if let Some(grab_offset_y) = state.scrollbar_grab_offset_y {
                if let Some(position) = cursor.position() {
                    let target_row = scrollbar_row_for_position(
                        position.y - bounds.y,
                        grab_offset_y,
                        editor_layout,
                        context.viewport.visible_row_count(),
                    );
                    shell.publish((context.on_action)(EditorAction::ScrollToRow(target_row)));
                    shell.request_redraw();
                    outcome.should_capture = true;
                }
            } else if (state.drag_anchor.is_some() || state.text_drag.is_some())
                && let Some(screen_position) = cursor.position()
            {
                outcome.perf_event = "editor_drag_select";
                state.drag_position = Some(screen_position);
                if let Some(drag) = state.text_drag.as_mut() {
                    drag.started |=
                        screen_position.distance(drag.pressed_at) >= TEXT_DRAG_THRESHOLD;
                }
                update_drag_selection(
                    &context,
                    state,
                    bounds,
                    editor_layout,
                    renderer,
                    shell,
                    Instant::now(),
                );
                outcome.should_capture = true;
            }
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            outcome.should_capture = state.drag_anchor.is_some()
                || state.text_drag.is_some()
                || state.scrollbar_grab_offset_y.is_some();
            if state.text_drag.is_some() {
                state.drag_position = cursor.position();
                update_drag_selection(
                    &context,
                    state,
                    bounds,
                    editor_layout,
                    renderer,
                    shell,
                    Instant::now(),
                );
                if let Some(drag) = state.text_drag.take()
                    && drag.buffer == *context.buffer
                {
                    if !drag.started {
                        shell.publish((context.on_action)(place_caret_action(
                            context.viewport,
                            drag.clicked_position,
                            drag.clicked_row,
                        )));
                    } else if let Some(pointer) = cursor.position_in(bounds)
                        && pointer.x >= context.metrics.text_origin_x(context.decorations)
                        && !vertical_scrollbar_geometry(
                            editor_layout,
                            context.viewport.visible_row_count(),
                        )
                        .is_some_and(|bar| bar.track.contains(pointer))
                        && let Some((target, _)) = drag.target
                    {
                        shell.publish((context.on_action)(EditorAction::MoveSelection {
                            source: drag.source,
                            target,
                        }));
                    }
                }
                shell.request_redraw();
            }
            state.cancel_pointer_drag();
        }
        Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
            outcome.perf_event = "editor_wheel";
            let lines =
                scroll_delta_lines(*delta, context.scroll_speed) + state.partial_scroll_lines;
            let whole_lines = lines.trunc() as i32;
            state.partial_scroll_lines = lines.fract();

            if whole_lines != 0 {
                shell.publish((context.on_action)(EditorAction::ScrollLines(whole_lines)));
                mark_scroll_fast(state, shell);
            }
            outcome.should_capture = true;
        }
        Event::InputMethod(input_method::Event::Opened) if state.is_focused => {
            state.preedit = Some(input_method::Preedit::new());
            state.reset_caret_blink();
            shell.request_redraw();
            outcome.should_capture = true;
        }
        Event::InputMethod(input_method::Event::Closed) if state.is_focused => {
            state.preedit = None;
            state.reset_caret_blink();
            shell.request_redraw();
            outcome.should_capture = true;
        }
        Event::InputMethod(input_method::Event::Preedit(content, selection))
            if state.is_focused =>
        {
            state.reset_caret_blink();
            state.preedit = Some(input_method::Preedit {
                content: content.clone(),
                selection: selection.clone(),
                text_size: Some(Pixels(text_size(context.metrics))),
            });
            shell.request_redraw();
            outcome.should_capture = true;
        }
        Event::InputMethod(input_method::Event::Commit(content)) if state.is_focused => {
            state.cancel_pointer_drag();
            state.preedit = None;
            if !content.is_empty() {
                state.reset_caret_blink();
                shell.publish((context.on_action)(EditorAction::InsertText(
                    content.clone(),
                )));
            }
            shell.request_redraw();
            outcome.should_capture = true;
        }
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            text,
            ..
        }) if state.is_focused => {
            if state.text_drag.is_some() {
                state.cancel_pointer_drag();
                shell.request_redraw();
                if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                    shell.capture_event();
                    outcome.should_capture = true;
                    return outcome;
                }
            }
            if let Some(action) = key_action(
                key,
                modified_key,
                *modifiers,
                text.as_deref(),
                context.shortcuts,
            ) {
                state.reset_caret_blink();
                shell.publish((context.on_action)(action));
                shell.request_redraw();
                outcome.should_capture = true;
            }
        }
        _ => {}
    }

    if outcome.should_capture {
        shell.capture_event();
    }

    shell.request_input_method(&input_method(
        &context,
        state,
        editor_layout,
        bounds,
        renderer,
    ));

    outcome
}

fn whole_line_selection(buffer: &EditorBuffer, line: usize) -> EditorSelection {
    let start = buffer.clamp_position(EditorPosition::new(line, 0));
    let end = if start.line + 1 < buffer.line_count() {
        EditorPosition::new(start.line + 1, 0)
    } else {
        last_line_end_position(buffer)
    };

    EditorSelection::new(start, end)
}

fn place_caret_action(
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

fn update_drag_selection<Message, Renderer>(
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
    if target_row != layout.scroll.first_visible_row {
        let scroll_at = state.drag_scroll_at.unwrap_or(now);
        if now >= scroll_at {
            layout.scroll.first_visible_row = target_row;
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
        position.y.clamp(layout.metrics.padding_top, last_row_y),
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

fn selection_contains_pointer<Message, Renderer>(
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
            super::line_cache::fold_needs_measured_geometry(
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

fn mark_scroll_fast<Paragraph, Message>(
    state: &AdvancedEditorState<Paragraph>,
    shell: &mut Shell<'_, Message>,
) {
    let settle_at = Instant::now() + Duration::from_millis(FAST_SCROLL_SETTLE_MS);
    state.scroll_fast_until.set(Some(settle_at));
    shell.request_redraw_at(settle_at);
}

fn last_line_end_position(buffer: &EditorBuffer) -> EditorPosition {
    let line = buffer.line_count().saturating_sub(1);
    let column = buffer.line(line).map(|text| text.len()).unwrap_or(0);

    buffer.clamp_position(EditorPosition::new(line, column))
}

fn request_caret_blink_frame<Message, Paragraph>(
    state: &AdvancedEditorState<Paragraph>,
    shell: &mut Shell<'_, Message>,
) {
    if !state.is_focused || !state.is_window_focused {
        return;
    }

    let now = state.caret_now.get();
    let elapsed = (now - state.caret_updated_at).as_millis();
    let millis_until_redraw = CARET_BLINK_INTERVAL_MS - elapsed % CARET_BLINK_INTERVAL_MS;

    shell.request_redraw_at(now + Duration::from_millis(millis_until_redraw as u64));
}

fn input_method<'a, Message, Renderer>(
    context: &InteractionContext<'_, Message>,
    state: &'a AdvancedEditorState<Renderer::Paragraph>,
    editor_layout: EditorLayout,
    bounds: Rectangle,
    renderer: &Renderer,
) -> InputMethod<&'a str>
where
    Renderer: text::Renderer<Font = Font>,
{
    if !state.is_focused || state.text_drag.is_some() {
        return InputMethod::Disabled;
    }

    let cursor = context
        .buffer
        .clamp_position(context.selections.main().cursor);
    let point = measured_position_point_with_context(
        context.buffer,
        context.viewport,
        context.decorations,
        editor_layout,
        cursor,
        context.caret_row,
        renderer,
        context.cjk_context,
    );

    InputMethod::Enabled {
        cursor: Rectangle {
            x: bounds.x + point.x,
            y: bounds.y + point.y,
            width: 1.0,
            height: context.metrics.line_height,
        },
        purpose: input_method::Purpose::Normal,
        preedit: state.preedit.as_ref().map(input_method::Preedit::as_ref),
    }
}

pub(super) fn scroll_delta_lines(delta: mouse::ScrollDelta, scroll_speed: f32) -> f32 {
    let lines = match delta {
        mouse::ScrollDelta::Lines { y, .. } => -y,
        mouse::ScrollDelta::Pixels { y, .. } => -y / 16.0,
    };

    lines * scroll_speed.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::decoration::DecorationSettings;
    use crate::editor::fold::{FoldModel, FoldRange};
    use crate::editor::layout::{EditorMetrics, ScrollOffset};
    use iced::advanced::graphics::core::shell::Waker;

    struct TestEditor {
        buffer: EditorBuffer,
        viewport: ViewportModel,
        decorations: DecorationModel,
        selections: SelectionSet,
        caret_row: Option<usize>,
        shortcuts: ShortcutMap,
        state: AdvancedEditorState<()>,
        layout: EditorLayout,
        bounds: Rectangle,
        last_redraw: window::RedrawRequest,
    }

    impl TestEditor {
        fn new(text: &str) -> Self {
            Self::with_folds(text, FoldModel::default())
        }

        fn with_folds(text: &str, folds: FoldModel) -> Self {
            let buffer = EditorBuffer::from_text(text);
            let viewport = ViewportModel::new(buffer.line_count(), &folds);
            let decorations = DecorationModel::from_folds(
                DecorationSettings::default(),
                buffer.line_count(),
                &folds,
                vec![],
            );
            let layout =
                EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 400.0, 94.0);

            Self {
                buffer,
                viewport,
                decorations,
                selections: SelectionSet::new(EditorSelection::new(
                    EditorPosition::new(0, 0),
                    EditorPosition::new(0, 0),
                )),
                caret_row: None,
                shortcuts: ShortcutMap::default(),
                state: AdvancedEditorState::default(),
                layout,
                bounds: Rectangle {
                    x: 10.0,
                    y: 20.0,
                    width: layout.width,
                    height: layout.height,
                },
                last_redraw: window::RedrawRequest::Wait,
            }
        }

        fn text_point(&self, row: usize, column: usize) -> Point {
            Point::new(
                self.bounds.x
                    + self.layout.metrics.text_origin_x(&self.decorations)
                    + column as f32 * self.layout.metrics.character_width
                    + 0.5,
                self.bounds.y
                    + self.layout.metrics.padding_top
                    + row as f32 * self.layout.metrics.line_height
                    + 1.0,
            )
        }

        fn dispatch(&mut self, event: Event, cursor: mouse::Cursor) -> Vec<EditorAction> {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            handle_event(
                InteractionContext {
                    buffer: &self.buffer,
                    viewport: &self.viewport,
                    decorations: &self.decorations,
                    selections: &self.selections,
                    metrics: self.layout.metrics,
                    caret_row: self.caret_row,
                    scroll_speed: 1.5,
                    viewport_key: 0,
                    cjk_context: None,
                    shortcuts: &self.shortcuts,
                    on_action: &std::convert::identity,
                },
                &mut self.state,
                &event,
                self.bounds,
                cursor,
                self.layout,
                &(),
                &mut shell,
            );
            self.last_redraw = shell.redraw_request();
            for action in &messages {
                match action {
                    EditorAction::PlaceCaret(position) => {
                        self.caret_row = None;
                        self.selections =
                            SelectionSet::new(EditorSelection::new(*position, *position));
                    }
                    EditorAction::SelectRegion(selection) => {
                        self.caret_row = None;
                        self.selections = SelectionSet::new(*selection);
                    }
                    EditorAction::PlaceCaretOnRow { position, row } => {
                        self.caret_row = Some(*row);
                        self.selections =
                            SelectionSet::new(EditorSelection::new(*position, *position));
                    }
                    EditorAction::SelectRegionOnRow { selection, row } => {
                        self.caret_row = Some(*row);
                        self.selections = SelectionSet::new(*selection);
                    }
                    EditorAction::ScrollToRow(row) => self.layout.scroll.first_visible_row = *row,
                    _ => {}
                }
            }

            messages
        }

        fn press(&mut self, button: mouse::Button, point: Point) -> Vec<EditorAction> {
            self.dispatch(
                Event::Mouse(mouse::Event::ButtonPressed(button)),
                mouse::Cursor::Available(point),
            )
        }

        fn drag_to(&mut self, point: Point) -> Vec<EditorAction> {
            self.dispatch(
                Event::Mouse(mouse::Event::CursorMoved { position: point }),
                mouse::Cursor::Available(point),
            )
        }
    }

    #[test]
    fn blank_editor_area_targets_end_of_last_line() {
        let buffer = EditorBuffer::from_text("alpha\nbeta");

        assert_eq!(
            last_line_end_position(&buffer),
            EditorPosition::new(1, "beta".len())
        );
    }

    fn selected_editor() -> TestEditor {
        let mut editor = TestEditor::new("abcdefgh\nijklmnop");
        editor.selections = SelectionSet::new(EditorSelection::new(
            EditorPosition::new(0, 1),
            EditorPosition::new(0, 4),
        ));
        editor
    }

    #[test]
    fn text_drag_preserves_highlight_and_moves_only_on_release() {
        let mut editor = selected_editor();
        let source = editor.selections.clone();
        editor.press(mouse::Button::Left, editor.text_point(0, 2));
        assert_eq!(editor.selections, source);
        let target = editor.text_point(1, 6);
        let messages = editor.drag_to(target);
        assert_eq!(editor.selections, source);
        assert!(!messages.iter().any(|action| action.mutates_document()));
        assert_eq!(
            editor.state.text_drag.as_ref().unwrap().target,
            Some((EditorPosition::new(1, 6), Some(1)))
        );
        // A stationary drag inside the editor must not request frames forever.
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(Instant::now())),
            mouse::Cursor::Available(target),
        );
        assert_ne!(editor.last_redraw, window::RedrawRequest::NextFrame);
        let messages = editor.dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Available(target),
        );
        assert!(messages.contains(&EditorAction::MoveSelection {
            source,
            target: EditorPosition::new(1, 6),
        }));
        assert!(editor.state.text_drag.is_none());
    }

    #[test]
    fn text_drag_can_start_immediately_after_double_click_selection() {
        let mut editor = TestEditor::new("abcdefgh\nijklmnop");
        let start = editor.text_point(0, 0);
        for _ in 0..2 {
            editor.press(mouse::Button::Left, start);
            editor.dispatch(
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                mouse::Cursor::Available(start),
            );
        }
        assert!(!editor.selections.main().is_caret());
        let source = editor.selections.clone();
        editor.press(mouse::Button::Left, start);
        editor.drag_to(editor.text_point(1, 5));
        assert!(editor.state.text_drag.as_ref().unwrap().started);
        assert_eq!(editor.selections, source);
    }

    #[test]
    fn clicking_highlight_with_small_pointer_jitter_places_caret() {
        let mut editor = selected_editor();
        let start = editor.text_point(0, 2);
        editor.press(mouse::Button::Left, start);
        let end = Point::new(start.x + 1.0, start.y + 1.0);
        editor.drag_to(end);
        let messages = editor.dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Available(end),
        );
        assert!(messages.contains(&EditorAction::PlaceCaret(EditorPosition::new(0, 2))));
        assert!(!messages.iter().any(|action| action.mutates_document()));
    }

    #[test]
    fn text_drag_cancels_on_escape_focus_loss_outside_drop_and_changed_buffer() {
        for ending in 0..5 {
            let mut editor = selected_editor();
            let source = editor.selections.clone();
            editor.press(mouse::Button::Left, editor.text_point(0, 2));
            let end = editor.text_point(1, 6);
            editor.drag_to(end);
            let cursor = match ending {
                0 => {
                    editor.dispatch(
                        Event::Keyboard(keyboard::Event::KeyPressed {
                            key: keyboard::Key::Named(keyboard::key::Named::Escape),
                            modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
                            physical_key: keyboard::key::Physical::Code(
                                keyboard::key::Code::Escape,
                            ),
                            location: keyboard::Location::Standard,
                            modifiers: keyboard::Modifiers::default(),
                            text: None,
                            repeat: false,
                        }),
                        mouse::Cursor::Available(end),
                    );
                    mouse::Cursor::Available(end)
                }
                1 => {
                    editor.dispatch(
                        Event::Window(window::Event::Unfocused),
                        mouse::Cursor::Available(end),
                    );
                    mouse::Cursor::Available(end)
                }
                2 => mouse::Cursor::Available(Point::new(editor.bounds.x - 5.0, end.y)),
                3 => {
                    editor.buffer = EditorBuffer::from_text("ABCDEFGH\nIJKLMNOP");
                    mouse::Cursor::Available(end)
                }
                _ => mouse::Cursor::Unavailable,
            };
            let messages = editor.dispatch(
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                cursor,
            );
            assert!(
                !messages.iter().any(|action| action.mutates_document()),
                "ending {ending}"
            );
            assert_eq!(editor.selections, source);
            assert!(editor.state.text_drag.is_none());
            assert!(editor.state.drag_scroll_at.is_none());
        }
    }

    #[test]
    fn text_drag_tracks_wrapped_destinations_and_scrolls_without_reselecting() {
        let mut editor = selected_editor();
        editor.viewport = ViewportModel::new_wrapped(&editor.buffer, &FoldModel::default(), 4, 4);
        let source = editor.selections.clone();
        editor.press(mouse::Button::Left, editor.text_point(0, 2));
        editor.drag_to(editor.text_point(1, 2));
        assert_eq!(
            editor.state.text_drag.as_ref().unwrap().target,
            Some((EditorPosition::new(0, 6), Some(1)))
        );
        assert_eq!(editor.selections, source);

        let mut editor = TestEditor::new(&"abcdefgh\n".repeat(30));
        editor.layout.scroll.first_visible_row = 10;
        editor.selections = SelectionSet::new(EditorSelection::new(
            EditorPosition::new(11, 1),
            EditorPosition::new(11, 4),
        ));
        let source = editor.selections.clone();
        editor.press(mouse::Button::Left, editor.text_point(1, 2));
        let outside = Point::new(editor.text_point(1, 2).x, editor.bounds.y - 5.0);
        let messages = editor.drag_to(outside);
        assert!(
            messages
                .iter()
                .any(|action| matches!(action, EditorAction::ScrollToRow(_)))
        );
        let due = editor.state.drag_scroll_at.unwrap();
        let row = editor.layout.scroll.first_visible_row;
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(due)),
            mouse::Cursor::Unavailable,
        );
        assert!(editor.layout.scroll.first_visible_row < row);
        assert_eq!(editor.selections, source);
    }

    #[test]
    fn blank_editor_area_targets_empty_trailing_line() {
        let buffer = EditorBuffer::from_text("alpha\n");

        assert_eq!(last_line_end_position(&buffer), EditorPosition::new(1, 0));
    }

    #[test]
    fn whole_line_selection_includes_line_endings_and_handles_empty_last_lines() {
        for (text, line, expected) in [
            ("alpha\nbeta", 0, "alpha\n"),
            ("alpha\r\nbeta", 0, "alpha\r\n"),
            ("alpha\rbeta", 0, "alpha\r"),
            ("alpha\n\rbeta", 0, "alpha\n\r"),
            ("alpha\n\nbeta", 1, "\n"),
            ("alpha\ncaf\u{e9}", 1, "caf\u{e9}"),
            ("alpha\n", 1, ""),
            ("", 0, ""),
        ] {
            let buffer = EditorBuffer::from_text(text);
            let selection = whole_line_selection(&buffer, line);

            assert_eq!(buffer.slice_text(selection.range()), expected, "{text:?}");
            assert_eq!(selection.anchor, EditorPosition::new(line, 0));
        }
    }

    #[test]
    fn double_click_at_text_start_selects_line_and_elsewhere_selects_word() {
        for column in [0, 2] {
            let mut editor = TestEditor::new("alpha beta\nnext");
            let point = editor.text_point(0, column);
            editor.press(mouse::Button::Left, point);
            editor.dispatch(
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                mouse::Cursor::Available(point),
            );
            let messages = editor.press(mouse::Button::Left, point);
            let expected = if column == 0 {
                EditorAction::SelectRegion(EditorSelection::new(
                    EditorPosition::new(0, 0),
                    EditorPosition::new(1, 0),
                ))
            } else {
                EditorAction::SelectWordAt(EditorPosition::new(0, column))
            };

            assert!(messages.contains(&expected));
            assert!(editor.state.drag_anchor.is_none());
        }
    }

    #[test]
    fn wrapped_clicks_use_fragment_columns_and_double_click_selects_logical_line() {
        let mut editor = TestEditor::new("abcdefghijkl\nnext");
        editor.viewport = ViewportModel::new_wrapped(&editor.buffer, &FoldModel::default(), 4, 4);
        let point = editor.text_point(1, 0);
        let messages = editor.press(mouse::Button::Left, point);
        assert!(messages.contains(&EditorAction::PlaceCaretOnRow {
            position: EditorPosition::new(0, 4),
            row: 1,
        }));
        let messages = editor.press(mouse::Button::Left, point);
        assert!(
            messages.contains(&EditorAction::SelectRegion(EditorSelection::new(
                EditorPosition::new(0, 0),
                EditorPosition::new(1, 0),
            )))
        );
        assert!(editor.state.drag_anchor.is_none());
    }

    #[test]
    fn wrapped_row_end_click_and_drag_keep_upstream_row_affinity() {
        let mut editor = TestEditor::new("abcdefghijkl\nnext");
        editor.viewport = ViewportModel::new_wrapped(&editor.buffer, &FoldModel::default(), 4, 4);
        let end = editor.text_point(0, 8);
        let messages = editor.press(mouse::Button::Left, end);
        assert!(messages.contains(&EditorAction::PlaceCaretOnRow {
            position: EditorPosition::new(0, 4),
            row: 0,
        }));
        let messages = editor.drag_to(editor.text_point(1, 8));
        assert!(messages.contains(&EditorAction::SelectRegionOnRow {
            selection: EditorSelection::new(EditorPosition::new(0, 4), EditorPosition::new(0, 8)),
            row: 1,
        }));
    }

    #[test]
    fn wrapped_drag_autoscroll_counts_visual_rows_within_one_logical_line() {
        let mut editor = TestEditor::new(&"a".repeat(120));
        editor.viewport = ViewportModel::new_wrapped(&editor.buffer, &FoldModel::default(), 4, 4);
        editor.layout.scroll.first_visible_row = 5;
        editor.press(mouse::Button::Left, editor.text_point(1, 1));
        let outside = Point::new(editor.text_point(0, 1).x, editor.bounds.y - 1.0);
        let messages = editor.drag_to(outside);
        assert!(messages.contains(&EditorAction::ScrollToRow(4)));
        assert!(messages.contains(&EditorAction::SelectRegionOnRow {
            selection: EditorSelection::new(EditorPosition::new(0, 25), EditorPosition::new(0, 17)),
            row: 4,
        }));
    }

    #[test]
    fn wrapped_right_click_preserves_rectangular_selection_on_continuation() {
        let mut editor = TestEditor::new("abcdefghijkl\nnext");
        editor.viewport = ViewportModel::new_wrapped(&editor.buffer, &FoldModel::default(), 4, 4);
        let selection =
            SelectionRange::rectangular(EditorPosition::new(0, 5), EditorPosition::new(0, 7), 5, 7);
        editor.selections = SelectionSet::from_selection_ranges(vec![selection], 0);
        let original = editor.selections.clone();
        let messages = editor.press(mouse::Button::Right, editor.text_point(1, 2));
        assert_eq!(editor.selections, original);
        assert!(!messages.iter().any(|message| matches!(
            message,
            EditorAction::PlaceCaret(_) | EditorAction::PlaceCaretOnRow { .. }
        )));
    }

    #[test]
    fn gutter_double_click_selects_the_logical_line_after_a_collapsed_block() {
        let range = FoldRange::new(0, 2);
        let mut folds = FoldModel::new(vec![range]);
        folds.set_collapsed(range, true);
        let mut editor = TestEditor::with_folds("header\nchild\nend\nafter\nlast", folds);
        let point = Point::new(editor.bounds.x + 10.0, editor.text_point(1, 0).y);

        editor.press(mouse::Button::Left, point);
        let messages = editor.press(mouse::Button::Left, point);

        assert!(
            messages.contains(&EditorAction::SelectRegion(EditorSelection::new(
                EditorPosition::new(3, 0),
                EditorPosition::new(4, 0),
            )))
        );
    }

    #[test]
    fn stationary_drag_scrolls_both_directions_and_tracks_the_destination_rows() {
        let text = (0..40).map(|_| "abcdefgh").collect::<Vec<_>>().join("\n");
        for upwards in [true, false] {
            let mut editor = TestEditor::new(&text);
            editor.layout.scroll.first_visible_row = 10;
            let start = editor.text_point(2, 2);
            editor.press(mouse::Button::Left, start);
            let outside = Point::new(
                start.x,
                if upwards {
                    editor.bounds.y - 5.0
                } else {
                    editor.bounds.y + editor.bounds.height + 5.0
                },
            );
            editor.drag_to(outside);

            assert_eq!(
                editor.layout.scroll.first_visible_row,
                if upwards { 9 } else { 11 }
            );
            assert_eq!(editor.selections.main().anchor, EditorPosition::new(12, 2));
            assert_eq!(
                editor.selections.main().cursor,
                EditorPosition::new(if upwards { 9 } else { 15 }, 2),
            );

            let due = editor
                .state
                .drag_scroll_at
                .expect("drag should schedule another frame");
            let early = editor.dispatch(
                Event::Window(window::Event::RedrawRequested(
                    due - Duration::from_millis(1),
                )),
                mouse::Cursor::Unavailable,
            );
            assert!(
                !early
                    .iter()
                    .any(|action| matches!(action, EditorAction::ScrollToRow(_)))
            );
            assert_eq!(editor.last_redraw, window::RedrawRequest::At(due));

            editor.dispatch(
                Event::Window(window::Event::RedrawRequested(due)),
                mouse::Cursor::Unavailable,
            );
            assert_eq!(
                editor.layout.scroll.first_visible_row,
                if upwards { 8 } else { 12 }
            );
            assert_eq!(
                editor.selections.main().cursor,
                EditorPosition::new(if upwards { 8 } else { 16 }, 2),
            );
        }
    }

    #[test]
    fn drag_stops_at_document_ends_and_selects_the_remaining_text() {
        let text = (0..10).map(|_| "abcdefgh").collect::<Vec<_>>().join("\n");
        for upwards in [true, false] {
            let mut editor = TestEditor::new(&text);
            editor.layout.scroll.first_visible_row = if upwards { 1 } else { 4 };
            let start = editor.text_point(2, 2);
            editor.press(mouse::Button::Left, start);
            editor.drag_to(Point::new(
                start.x,
                if upwards {
                    editor.bounds.y - 500.0
                } else {
                    editor.bounds.y + editor.bounds.height + 500.0
                },
            ));

            assert_eq!(
                editor.layout.scroll.first_visible_row,
                if upwards { 0 } else { 5 }
            );
            assert_eq!(
                editor.selections.main().cursor,
                if upwards {
                    EditorPosition::new(0, 0)
                } else {
                    EditorPosition::new(9, 8)
                }
            );
            editor.dispatch(
                Event::Window(window::Event::RedrawRequested(
                    Instant::now() + Duration::from_millis(100),
                )),
                mouse::Cursor::Unavailable,
            );
            assert!(editor.state.drag_scroll_at.is_none());
        }
    }

    #[test]
    fn release_and_window_unfocus_cancel_pending_drag_scroll() {
        let text = (0..20).map(|_| "abcdefgh").collect::<Vec<_>>().join("\n");
        for ending in [
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Event::Window(window::Event::Unfocused),
        ] {
            let mut editor = TestEditor::new(&text);
            editor.layout.scroll.first_visible_row = 10;
            let start = editor.text_point(2, 2);
            editor.press(mouse::Button::Left, start);
            editor.drag_to(Point::new(start.x, editor.bounds.y - 5.0));
            let due = editor.state.drag_scroll_at.expect("scheduled drag scroll");

            editor.dispatch(ending, mouse::Cursor::Unavailable);
            let messages = editor.dispatch(
                Event::Window(window::Event::RedrawRequested(due)),
                mouse::Cursor::Unavailable,
            );

            assert!(editor.state.drag_anchor.is_none());
            assert!(editor.state.drag_position.is_none());
            assert!(editor.state.drag_scroll_at.is_none());
            assert!(
                !messages
                    .iter()
                    .any(|action| matches!(action, EditorAction::ScrollToRow(_)))
            );
        }
    }

    #[test]
    fn right_click_preserves_multiple_selections_and_repositions_outside_them() {
        let mut editor = TestEditor::new("abcdefgh\nabcdefgh");
        editor.selections = SelectionSet::from_ranges(
            vec![
                EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 2)),
                EditorSelection::new(EditorPosition::new(1, 1), EditorPosition::new(1, 4)),
            ],
            0,
        );
        let original = editor.selections.clone();
        let inside = editor.text_point(1, 2);

        let messages = editor.press(mouse::Button::Right, inside);

        assert!(messages.contains(&EditorAction::Focus));
        assert!(
            !messages
                .iter()
                .any(|action| matches!(action, EditorAction::PlaceCaret(_)))
        );
        assert_eq!(editor.selections, original);
        let outside = editor.text_point(1, 6);
        let messages = editor.press(mouse::Button::Right, outside);
        assert!(messages.contains(&EditorAction::PlaceCaret(EditorPosition::new(1, 6))));
    }

    #[test]
    fn right_click_preserves_rectangular_virtual_space_only_inside_the_rectangle() {
        let mut editor = TestEditor::new("abcdefgh\nx\nabcdefgh");
        editor.selections =
            SelectionSet::rectangular(EditorPosition::new(0, 3), EditorPosition::new(2, 6), 3, 6);
        let original = editor.selections.clone();
        let inside = editor.text_point(1, 4);
        editor.press(mouse::Button::Right, inside);
        assert_eq!(editor.selections, original);

        let outside = editor.text_point(1, 7);
        let messages = editor.press(mouse::Button::Right, outside);
        assert!(messages.contains(&EditorAction::PlaceCaret(EditorPosition::new(1, 1))));
    }
}
