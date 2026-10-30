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
    LineGeometry, measured_position_point_with_cache, measured_text_hit_target_with_context,
    measured_virtual_caret_x,
};
use super::scroll::{ScrollAnimation, horizontal_offset_milli};
use super::scrollbar::{
    horizontal_offset_for_position, horizontal_scrollbar_geometry, scrollbar_row_for_position,
    vertical_scrollbar_geometry,
};
use super::state::{AdvancedEditorState, CARET_BLINK_INTERVAL_MS, TextDrag};

const FAST_SCROLL_SETTLE_MS: u64 = 120;
const DRAG_SCROLL_INTERVAL_MS: u64 = 50;
const MAX_DRAG_SCROLL_LINES: i32 = 8;
const TEXT_DRAG_THRESHOLD: f32 = 4.0;

pub(super) struct InteractionContext<'a, Message> {
    pub(super) now: Instant,
    pub(super) buffer: &'a EditorBuffer,
    pub(super) viewport: &'a ViewportModel,
    pub(super) decorations: &'a DecorationModel,
    pub(super) selections: &'a SelectionSet,
    pub(super) metrics: crate::editor::layout::EditorMetrics,
    pub(super) caret_row: Option<usize>,
    pub(super) scroll_speed: f32,
    pub(super) smooth_scrolling: bool,
    pub(super) max_horizontal_px: f32,
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
    if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
        state.wheel_input = Default::default();
    }
    let switched_document = state
        .viewport_geometry
        .is_some_and(|geometry| geometry.0 != context.viewport_key);
    if switched_document {
        state.wheel_input = Default::default();
        state.partial_scroll_lines = 0.0;
    }
    if switched_document
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

    // A horizontal bar can appear between precise updates. Keep the fractional
    // row when only its reserved lane changes the text height.
    if let Some(scroll) = state.smooth_scroll.as_mut()
        && !scroll.animating(context.now)
        && scroll.viewport_height() == bounds.height
    {
        scroll.set_text_height(editor_layout.height);
    }
    if state.smooth_scroll.as_mut().is_some_and(|scroll| {
        !scroll.observe_document_scroll(editor_layout.scroll)
            || (!context.smooth_scrolling && scroll.animating(context.now))
            || scroll.viewport_height() != bounds.height
            || !scroll.matches(
                editor_layout,
                context.viewport_key,
                context.viewport.visible_row_count(),
                context.viewport.wrap_columns(),
            )
    }) {
        state.smooth_scroll = None;
        state.partial_scroll_lines = 0.0;
        shell.request_redraw();
    }
    // Navigation and typing reveal the caret immediately. Pointer input instead
    // stops at the visible text so a click cannot land at the wheel destination.
    if matches!(
        event,
        Event::Keyboard(keyboard::Event::KeyPressed { .. })
            | Event::InputMethod(input_method::Event::Commit(_))
    ) {
        state.wheel_input = Default::default();
        if state.finish_scrolling() {
            shell.request_redraw();
        }
    } else if matches!(
        event,
        Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Window(window::Event::Unfocused)
    ) && let Some(scroll) = state.smooth_scroll.as_mut()
    {
        let previous_row = scroll.target_row();
        let previous_horizontal = scroll.target_horizontal_milli();
        scroll.stop(context.now);
        if scroll.target_row() != previous_row {
            shell.publish((context.on_action)(EditorAction::ScrollToRow(
                scroll.target_row(),
            )));
        }
        if scroll.target_horizontal_milli() != previous_horizontal {
            shell.publish((context.on_action)(EditorAction::ScrollToHorizontal {
                offset_milli: scroll.target_horizontal_milli(),
            }));
        }
        shell.request_redraw();
    }
    let editor_layout = state
        .smooth_scroll
        .map_or(editor_layout, |scroll| scroll.display_layout(context.now));
    if state
        .smooth_scroll
        .is_some_and(|scroll| scroll.animating(context.now))
    {
        shell.request_redraw_at(context.now + Duration::from_millis(16));
    }

    match event {
        Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            if state.modifiers.shift() != modifiers.shift() {
                state.wheel_input = Default::default();
            }
            state.modifiers = *modifiers;
        }
        Event::Window(window::Event::Unfocused) => {
            state.wheel_input = Default::default();
            state.modifiers = keyboard::Modifiers::empty();
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

            if let Some(scrollbar) = horizontal_scrollbar_geometry(
                editor_layout,
                context.max_horizontal_px,
                context.metrics.text_origin_x(context.decorations),
            ) && scrollbar.track.contains(position)
            {
                let grab_offset = if scrollbar.thumb.contains(position) {
                    position.x - scrollbar.thumb.x
                } else {
                    scrollbar.thumb.width / 2.0
                };
                state.scrollbar_grab_offset_x = Some((grab_offset, context.max_horizontal_px));
                let offset = horizontal_offset_for_position(
                    position.x,
                    grab_offset,
                    editor_layout,
                    context.max_horizontal_px,
                    context.metrics.text_origin_x(context.decorations),
                );
                let offset_milli = horizontal_offset_milli(offset);
                if let Some(scroll) = state.smooth_scroll.as_mut() {
                    scroll.set_horizontal_offset(offset_milli);
                }
                shell.publish((context.on_action)(EditorAction::ScrollToHorizontal {
                    offset_milli,
                }));
                state.is_focused = true;
                state.reset_caret_blink();
                state.clear_text_click();
                shell.publish((context.on_action)(EditorAction::Focus));
                shell.capture_event();
                shell.request_redraw();
                outcome.should_capture = true;
                return outcome;
            }

            if position.y >= editor_layout.height {
                shell.capture_event();
                outcome.should_capture = true;
                return outcome;
            }

            if let Some(scrollbar) =
                vertical_scrollbar_geometry(editor_layout, context.viewport.visible_row_count())
                && scrollbar.track.contains(position)
            {
                state.smooth_scroll = None;
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
                    let is_double_click = state.record_text_click(position, context.now);
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
                    let is_double_click = state.record_text_click(position, context.now);
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
                && position.y < editor_layout.height
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
            if let Some((grab_offset_x, max_horizontal_px)) = state.scrollbar_grab_offset_x {
                if let Some(position) = cursor.position() {
                    let offset = horizontal_offset_for_position(
                        position.x - bounds.x,
                        grab_offset_x,
                        editor_layout,
                        max_horizontal_px,
                        context.metrics.text_origin_x(context.decorations),
                    );
                    let offset_milli = horizontal_offset_milli(offset);
                    if let Some(scroll) = state.smooth_scroll.as_mut() {
                        scroll.set_horizontal_offset(offset_milli);
                    }
                    shell.publish((context.on_action)(EditorAction::ScrollToHorizontal {
                        offset_milli,
                    }));
                    shell.request_redraw();
                    outcome.should_capture = true;
                }
            } else if let Some(grab_offset_y) = state.scrollbar_grab_offset_y {
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
                    context.now,
                );
                outcome.should_capture = true;
            }
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            outcome.should_capture = state.drag_anchor.is_some()
                || state.text_drag.is_some()
                || state.scrollbar_grab_offset_y.is_some();
            outcome.should_capture |= state.scrollbar_grab_offset_x.is_some();
            if state.text_drag.is_some() {
                state.drag_position = cursor.position();
                update_drag_selection(
                    &context,
                    state,
                    bounds,
                    editor_layout,
                    renderer,
                    shell,
                    context.now,
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
                        && pointer.y < editor_layout.height
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
        Event::Mouse(mouse::Event::WheelScrolled { delta, phase })
            if cursor.is_over(bounds)
                || matches!(
                    phase,
                    mouse::ScrollPhase::ContactEnded
                        | mouse::ScrollPhase::Ended
                        | mouse::ScrollPhase::Cancelled
                ) =>
        {
            outcome.perf_event = "editor_wheel";
            let animate_wheel =
                state
                    .wheel_input
                    .should_animate_with_phase(*delta, *phase, context.now);
            if !cursor.is_over(bounds) || *phase == mouse::ScrollPhase::Cancelled {
                return outcome;
            }
            let delta = *delta;
            if matches!(
                delta,
                mouse::ScrollDelta::Lines { x: 0.0, y: 0.0 }
                    | mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 }
            ) {
                shell.capture_event();
                outcome.should_capture = true;
                return outcome;
            }
            let (lines, horizontal_px) = scroll_deltas(
                delta,
                state.modifiers,
                context.metrics.line_height,
                context.scroll_speed,
            );
            let max_horizontal_px = if context.viewport.wrap_columns().is_none() {
                context.max_horizontal_px
            } else {
                0.0
            };
            let new_scroll = || {
                ScrollAnimation::new(
                    editor_layout,
                    context.viewport_key,
                    context.viewport.visible_row_count(),
                    context.viewport.wrap_columns(),
                    bounds.height,
                    context.now,
                )
            };
            if !animate_wheel || context.smooth_scrolling {
                let scroll = state.smooth_scroll.get_or_insert_with(new_scroll);
                let previous_row = scroll.target_row();
                let previous_horizontal = scroll.target_horizontal_milli();
                if animate_wheel {
                    scroll.scroll_by(lines, horizontal_px, max_horizontal_px, context.now);
                } else {
                    scroll.scroll_direct(lines, horizontal_px, max_horizontal_px, context.now);
                }
                if scroll.target_row() != previous_row {
                    shell.publish((context.on_action)(EditorAction::ScrollToRow(
                        scroll.target_row(),
                    )));
                }
                if scroll.target_horizontal_milli() != previous_horizontal {
                    shell.publish((context.on_action)(EditorAction::ScrollToHorizontal {
                        offset_milli: scroll.target_horizontal_milli(),
                    }));
                }
                mark_scroll_fast(state, shell, context.now);
                shell.request_redraw();
            } else {
                let lines = lines + state.partial_scroll_lines;
                let whole_lines = lines.trunc() as i32;
                state.partial_scroll_lines = lines.fract();

                if whole_lines != 0 {
                    shell.publish((context.on_action)(EditorAction::ScrollLines(whole_lines)));
                    mark_scroll_fast(state, shell, context.now);
                }
                if horizontal_px != 0.0 && max_horizontal_px > 0.0 {
                    let scroll = state.smooth_scroll.get_or_insert_with(new_scroll);
                    scroll.scroll_direct(0.0, horizontal_px, max_horizontal_px, context.now);
                    shell.publish((context.on_action)(EditorAction::ScrollToHorizontal {
                        offset_milli: scroll.target_horizontal_milli(),
                    }));
                    mark_scroll_fast(state, shell, context.now);
                    shell.request_redraw();
                }
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
    now: Instant,
) {
    let settle_at = now + Duration::from_millis(FAST_SCROLL_SETTLE_MS);
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
    let point = measured_position_point_with_cache(
        context.buffer,
        context.viewport,
        context.decorations,
        editor_layout,
        cursor,
        context.caret_row,
        renderer,
        context.cjk_context,
        &mut state.line_geometries.borrow_mut(),
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

pub(super) fn scroll_delta_lines(
    delta: mouse::ScrollDelta,
    scroll_speed: f32,
    line_height: f32,
) -> f32 {
    match delta {
        mouse::ScrollDelta::Lines { y, .. } => -y * scroll_speed.max(0.0),
        mouse::ScrollDelta::Pixels { y, .. } => -y / line_height,
    }
}

fn scroll_deltas(
    delta: mouse::ScrollDelta,
    modifiers: keyboard::Modifiers,
    line_height: f32,
    scroll_speed: f32,
) -> (f32, f32) {
    let (mut x, mut y, pixels) = match delta {
        mouse::ScrollDelta::Lines { x, y } => (x, y, false),
        mouse::ScrollDelta::Pixels { x, y } => (x, y, true),
    };
    if modifiers.shift() {
        // Some platforms already turn Shift+wheel into horizontal input.
        x = if x == 0.0 { y } else { x };
        y = 0.0;
    }
    let vertical = if pixels {
        mouse::ScrollDelta::Pixels { x: 0.0, y }
    } else {
        mouse::ScrollDelta::Lines { x: 0.0, y }
    };
    (
        scroll_delta_lines(vertical, scroll_speed, line_height),
        -x * if pixels {
            1.0
        } else {
            scroll_speed.max(0.0) * line_height
        },
    )
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
        now: Instant,
        layout: EditorLayout,
        bounds: Rectangle,
        last_redraw: window::RedrawRequest,
        last_input_method: InputMethod,
        smooth_scrolling: bool,
        scroll_speed: f32,
        max_horizontal_px: f32,
        viewport_key: u64,
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
                now: Instant::now(),
                layout,
                bounds: Rectangle {
                    x: 10.0,
                    y: 20.0,
                    width: layout.width,
                    height: layout.height,
                },
                last_redraw: window::RedrawRequest::Wait,
                last_input_method: InputMethod::Disabled,
                smooth_scrolling: false,
                scroll_speed: 1.5,
                max_horizontal_px: 0.0,
                viewport_key: 0,
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

        fn displayed_layout(&self) -> EditorLayout {
            self.state
                .smooth_scroll
                .map_or(self.layout, |scroll| scroll.display_layout(self.now))
        }

        fn dispatch_without_app_update(
            &mut self,
            event: Event,
            cursor: mouse::Cursor,
        ) -> Vec<EditorAction> {
            // Simulated input timing must not depend on font loading or runner load.
            self.now = match &event {
                Event::Window(window::Event::RedrawRequested(now)) => *now,
                _ => self.now + Duration::from_millis(16),
            };
            let mut messages = Vec::new();
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            handle_event(
                InteractionContext {
                    now: self.now,
                    buffer: &self.buffer,
                    viewport: &self.viewport,
                    decorations: &self.decorations,
                    selections: &self.selections,
                    metrics: self.layout.metrics,
                    caret_row: self.caret_row,
                    scroll_speed: self.scroll_speed,
                    smooth_scrolling: self.smooth_scrolling,
                    max_horizontal_px: self.max_horizontal_px,
                    viewport_key: self.viewport_key,
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
            self.last_input_method = shell.input_method().clone();
            messages
        }

        fn apply_messages(&mut self, messages: &[EditorAction]) {
            for action in messages {
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
                    EditorAction::ScrollToHorizontal { offset_milli } => {
                        self.layout.scroll.horizontal_px = *offset_milli as f32 / 1000.0;
                    }
                    EditorAction::ScrollLines(lines) => {
                        let max_row = self.viewport.visible_row_count().saturating_sub(1);
                        self.layout.scroll.first_visible_row = self
                            .layout
                            .scroll
                            .first_visible_row
                            .saturating_add_signed(*lines as isize)
                            .min(max_row);
                    }
                    _ => {}
                }
            }
        }

        fn dispatch(&mut self, event: Event, cursor: mouse::Cursor) -> Vec<EditorAction> {
            let messages = self.dispatch_without_app_update(event, cursor);
            self.apply_messages(&messages);
            messages
        }

        fn press(&mut self, button: mouse::Button, point: Point) -> Vec<EditorAction> {
            self.dispatch(
                Event::Mouse(mouse::Event::ButtonPressed(button)),
                mouse::Cursor::Available(point),
            )
        }

        fn wheel(&mut self, delta: mouse::ScrollDelta) -> Vec<EditorAction> {
            self.dispatch(
                Event::Mouse(mouse::Event::WheelScrolled {
                    delta,
                    phase: mouse::ScrollPhase::Moved,
                }),
                mouse::Cursor::Available(self.text_point(0, 0)),
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
    fn precise_events_in_one_batch_accumulate_before_document_scroll_updates() {
        for smooth in [false, true] {
            let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(50)).repeat(100));
            editor.max_horizontal_px = 10_000.0;
            editor.smooth_scrolling = smooth;
            let cursor = mouse::Cursor::Available(editor.text_point(0, 0));
            let mut messages = Vec::new();
            for _ in 0..2 {
                messages.extend(editor.dispatch_without_app_update(
                    Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Pixels { x: -12.0, y: -12.0 },
                        phase: mouse::ScrollPhase::Moved,
                    }),
                    cursor,
                ));
                assert_eq!(editor.layout.scroll, ScrollOffset::ZERO);
            }

            let displayed = editor.displayed_layout();
            assert_eq!(displayed.scroll.horizontal_px, 24.0);
            assert_eq!(displayed.scroll.first_visible_row, 1);
            assert!((displayed.vertical_offset_px - 6.0).abs() < 0.001);
            editor.apply_messages(&messages);
            editor.dispatch(
                Event::Window(window::Event::RedrawRequested(
                    editor.now + Duration::from_millis(16),
                )),
                mouse::Cursor::Unavailable,
            );
            assert_eq!(editor.displayed_layout(), displayed);

            // Once the document has applied the target, returning to the old
            // offset is an external jump rather than another queued input.
            editor.layout.scroll = ScrollOffset::ZERO;
            editor.dispatch(
                Event::Window(window::Event::RedrawRequested(
                    editor.now + Duration::from_millis(16),
                )),
                mouse::Cursor::Unavailable,
            );
            assert!(editor.state.smooth_scroll.is_none());
            assert_eq!(editor.layout.scroll, ScrollOffset::ZERO);
        }
    }

    #[test]
    fn notch_events_in_one_batch_accumulate_with_smoothing_enabled_or_disabled() {
        for smooth in [false, true] {
            let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(50)).repeat(100));
            editor.max_horizontal_px = 10_000.0;
            editor.smooth_scrolling = smooth;
            let cursor = mouse::Cursor::Available(editor.text_point(0, 0));
            let mut messages = Vec::new();
            for _ in 0..2 {
                messages.extend(editor.dispatch_without_app_update(
                    Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Lines { x: -1.0, y: -1.0 },
                        phase: mouse::ScrollPhase::Moved,
                    }),
                    cursor,
                ));
                assert_eq!(editor.layout.scroll, ScrollOffset::ZERO);
            }
            assert_eq!(
                editor
                    .state
                    .smooth_scroll
                    .unwrap()
                    .target_horizontal_milli(),
                54_000
            );
            assert_eq!(
                editor.state.smooth_scroll.unwrap().animating(editor.now),
                smooth
            );
            editor.apply_messages(&messages);
            assert_eq!(editor.layout.scroll.first_visible_row, 3);
            assert_eq!(editor.layout.scroll.horizontal_px, 54.0);
            editor.dispatch(
                Event::Window(window::Event::RedrawRequested(
                    editor.now + super::super::scroll::SCROLL_DURATION,
                )),
                mouse::Cursor::Unavailable,
            );
            assert_eq!(editor.displayed_layout().scroll, editor.layout.scroll);
        }
    }

    #[test]
    fn precise_pixel_scroll_follows_input_at_every_zoom_without_a_tail() {
        for zoom in [0.5, 1.0, 2.0] {
            for smooth in [false, true] {
                for speed in [1.0, 1.5] {
                    let text = format!("{}\n", "abcdefgh".repeat(50)).repeat(200);
                    let mut editor = TestEditor::new(&text);
                    editor.layout.metrics.line_height = 20.0 * zoom;
                    editor.max_horizontal_px = 10_000.0;
                    editor.smooth_scrolling = smooth;
                    editor.scroll_speed = speed;
                    for packet in 1..=12 {
                        editor.wheel(mouse::ScrollDelta::Pixels { x: -8.0, y: -8.0 });
                        let displayed = editor.displayed_layout();
                        let expected = packet as f32 * 8.0;
                        let y = displayed.scroll.first_visible_row as f32
                            * displayed.metrics.line_height
                            + displayed.vertical_offset_px;
                        assert!((displayed.scroll.horizontal_px - expected).abs() < 0.001);
                        assert!((y - expected).abs() < 0.001);
                        assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));
                    }
                    let last_input = editor.now;
                    let last_position = editor.displayed_layout();
                    for elapsed_ms in [16, 160] {
                        editor.dispatch(
                            Event::Window(window::Event::RedrawRequested(
                                last_input + Duration::from_millis(elapsed_ms),
                            )),
                            mouse::Cursor::Unavailable,
                        );
                        assert_eq!(editor.displayed_layout(), last_position);
                        assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));
                    }
                }
            }
        }
    }

    #[test]
    fn precise_pixels_take_over_notch_animation_from_the_displayed_position() {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(50)).repeat(100));
        editor.max_horizontal_px = 10_000.0;
        editor.smooth_scrolling = true;
        editor.wheel(mouse::ScrollDelta::Lines { x: -4.0, y: -4.0 });
        let now = editor.now + Duration::from_millis(16);
        let motion = editor.state.smooth_scroll.unwrap();
        assert!(motion.animating(now));
        let before = motion.display_layout(now);
        let before_y = before.scroll.first_visible_row as f32 * before.metrics.line_height
            + before.vertical_offset_px;

        editor.wheel(mouse::ScrollDelta::Pixels { x: -8.0, y: -8.0 });

        let after = editor.displayed_layout();
        let after_y = after.scroll.first_visible_row as f32 * after.metrics.line_height
            + after.vertical_offset_px;
        assert!((after.scroll.horizontal_px - before.scroll.horizontal_px - 8.0).abs() < 0.001);
        assert!((after_y - before_y - 8.0).abs() < 0.001);
        assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));
    }

    #[test]
    fn fractional_line_input_preserves_both_axes_and_keeps_later_packets_direct() {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(50)).repeat(100));
        editor.max_horizontal_px = 10_000.0;
        editor.smooth_scrolling = true;

        editor.wheel(mouse::ScrollDelta::Lines { x: -0.25, y: -1.0 });

        let displayed = editor.displayed_layout();
        assert_eq!(displayed.scroll.horizontal_px, 6.75);
        assert_eq!(displayed.scroll.first_visible_row, 1);
        assert_eq!(displayed.vertical_offset_px, 9.0);
        assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));

        editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 });

        let displayed = editor.displayed_layout();
        assert_eq!(displayed.scroll.horizontal_px, 6.75);
        assert_eq!(displayed.scroll.first_visible_row, 3);
        assert_eq!(displayed.vertical_offset_px, 0.0);
        assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));
    }

    #[test]
    fn native_cancellation_discards_movement_and_restores_wheel_easing() {
        let mut editor = TestEditor::new(&"line\n".repeat(100));
        editor.smooth_scrolling = true;
        let cursor = mouse::Cursor::Available(editor.text_point(0, 0));
        let before = editor.displayed_layout();
        editor.dispatch(
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 },
                phase: mouse::ScrollPhase::Started,
            }),
            cursor,
        );
        editor.dispatch(
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x: -8.0, y: -8.0 },
                phase: mouse::ScrollPhase::Cancelled,
            }),
            cursor,
        );
        assert_eq!(editor.displayed_layout(), before);
        editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 });
        assert!(editor.state.smooth_scroll.unwrap().animating(editor.now));
    }

    #[test]
    fn precise_wheel_preserves_native_components_when_the_direction_changes() {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(20));
        editor.max_horizontal_px = 500.0;
        editor.smooth_scrolling = false;

        editor.wheel(mouse::ScrollDelta::Pixels { x: -1.0, y: -10.0 });
        assert_eq!(editor.layout.scroll.horizontal_px, 1.0);
        assert_eq!(editor.layout.scroll.first_visible_row, 0);
        assert!((editor.displayed_layout().vertical_offset_px - 10.0).abs() < 0.001);

        editor.wheel(mouse::ScrollDelta::Pixels { x: -10.0, y: -10.0 });
        assert_eq!(editor.layout.scroll.horizontal_px, 11.0);
        assert_eq!(editor.layout.scroll.first_visible_row, 1);
        assert!((editor.displayed_layout().vertical_offset_px - 2.0).abs() < 0.001);

        editor.wheel(mouse::ScrollDelta::Pixels { x: -1.0, y: -10.0 });
        assert_eq!(editor.layout.scroll.horizontal_px, 12.0);
        assert_eq!(editor.layout.scroll.first_visible_row, 1);
        assert!((editor.displayed_layout().vertical_offset_px - 12.0).abs() < 0.001);

        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(
                editor.now + Duration::from_millis(250),
            )),
            mouse::Cursor::Unavailable,
        );
        editor.wheel(mouse::ScrollDelta::Pixels { x: -1.0, y: -10.0 });
        assert_eq!(editor.layout.scroll.horizontal_px, 13.0);
        assert_eq!(editor.layout.scroll.first_visible_row, 2);
        assert!((editor.displayed_layout().vertical_offset_px - 4.0).abs() < 0.001);
    }

    #[test]
    fn horizontal_wheel_and_shift_wheel_preserve_vertical_scroll_and_selection() {
        for (delta, shift, expected_px) in [
            (mouse::ScrollDelta::Pixels { x: -8.0, y: 0.0 }, false, 8.0),
            (mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 }, true, 54.0),
            (mouse::ScrollDelta::Lines { x: -2.0, y: -2.0 }, true, 54.0),
        ] {
            let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(20));
            editor.max_horizontal_px = 500.0;
            editor.layout.scroll.first_visible_row = 3;
            let selections = editor.selections.clone();
            if shift {
                editor.dispatch(
                    Event::Keyboard(keyboard::Event::ModifiersChanged(
                        keyboard::Modifiers::SHIFT,
                    )),
                    mouse::Cursor::Unavailable,
                );
            }
            let messages = editor.wheel(delta);
            assert_eq!(editor.layout.scroll.horizontal_px, expected_px);
            assert_eq!(editor.layout.scroll.first_visible_row, 3);
            assert_eq!(editor.selections, selections);
            assert!(!messages.iter().any(|action| matches!(
                action,
                EditorAction::ScrollLines(_) | EditorAction::ScrollToRow(_)
            )));
        }
    }

    #[test]
    fn diagonal_wheel_scrolls_both_axes_with_or_without_animation() {
        for smooth in [false, true] {
            let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(20));
            editor.max_horizontal_px = 500.0;
            editor.smooth_scrolling = smooth;
            let selections = editor.selections.clone();
            let messages = editor.wheel(mouse::ScrollDelta::Lines { x: -2.0, y: -4.0 });
            assert_eq!(editor.layout.scroll.first_visible_row, 6);
            assert_eq!(editor.layout.scroll.horizontal_px, 54.0);
            assert_eq!(editor.selections, selections);
            assert!(messages.contains(&EditorAction::ScrollToHorizontal {
                offset_milli: 54_000
            }));
            if smooth {
                let scroll = editor.state.smooth_scroll.unwrap();
                let first = scroll.display_layout(editor.now);
                assert_eq!(first.scroll, ScrollOffset::ZERO);
                let next = scroll.display_layout(editor.now + Duration::from_millis(16));
                assert!(next.scroll.horizontal_px > 0.0 && next.scroll.horizontal_px < 54.0);
                assert!(next.scroll.first_visible_row < 6);
                assert!(next.vertical_offset_px > 0.0);
                assert_eq!(
                    scroll.display_layout(editor.now + super::super::scroll::SCROLL_DURATION),
                    editor.layout,
                );
            } else {
                assert!(
                    !editor
                        .state
                        .smooth_scroll
                        .is_some_and(|scroll| scroll.animating(editor.now))
                );
                assert!(messages.contains(&EditorAction::ScrollLines(6)));
            }
        }
    }

    #[test]
    fn click_during_horizontal_animation_uses_displayed_columns_and_stops_motion() {
        let _font_guard = crate::font_system_test_guard();
        let mut editor = TestEditor::new(&"abcdefgh".repeat(100));
        editor.max_horizontal_px = 1000.0;
        editor.smooth_scrolling = true;
        editor.wheel(mouse::ScrollDelta::Lines { x: -8.0, y: 0.0 });
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(
                editor.now + Duration::from_millis(35),
            )),
            mouse::Cursor::Unavailable,
        );
        let clicked_at = editor.now + Duration::from_millis(16);
        let displayed = editor
            .state
            .smooth_scroll
            .unwrap()
            .display_layout(clicked_at);
        let point = editor.text_point(0, 2);
        let expected_column =
            ((point.x - editor.bounds.x - editor.layout.metrics.text_origin_x(&editor.decorations)
                + displayed.scroll.horizontal_px)
                / editor.layout.metrics.character_width)
                .floor() as usize;
        let messages = editor.press(mouse::Button::Left, point);
        assert_eq!(
            editor.selections.main().cursor,
            EditorPosition::new(0, expected_column)
        );
        assert!(expected_column > 2 && expected_column < 29);
        let stopped = editor.state.smooth_scroll.unwrap();
        assert!(!stopped.animating(clicked_at));
        assert!(
            (editor.layout.scroll.horizontal_px - displayed.scroll.horizontal_px).abs() < 0.001
        );
        assert!(messages.contains(&EditorAction::ScrollToHorizontal {
            offset_milli: stopped.target_horizontal_milli(),
        }));
        assert_eq!(
            stopped.display_layout(clicked_at).scroll.horizontal_px,
            stopped
                .display_layout(clicked_at + super::super::scroll::SCROLL_DURATION)
                .scroll
                .horizontal_px,
        );
    }

    #[test]
    fn wrapped_views_ignore_horizontal_wheel_input_while_vertical_scrolling_continues() {
        for smooth in [false, true] {
            let mut editor = TestEditor::new(&"abcdefgh".repeat(100));
            editor.viewport =
                ViewportModel::new_wrapped(&editor.buffer, &FoldModel::default(), 4, 4);
            editor.max_horizontal_px = 500.0;
            editor.smooth_scrolling = smooth;
            let messages = editor.wheel(mouse::ScrollDelta::Lines { x: -2.0, y: -4.0 });
            assert_eq!(editor.layout.scroll.first_visible_row, 6);
            assert_eq!(editor.layout.scroll.horizontal_px, 0.0);
            assert!(
                !messages
                    .iter()
                    .any(|action| matches!(action, EditorAction::ScrollToHorizontal { .. }))
            );
        }
    }

    #[test]
    fn horizontal_scrollbar_drag_and_track_click_reach_limits_without_moving_carets() {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(20));
        editor.max_horizontal_px = 400.0;
        editor.layout.height -= super::super::scrollbar::HORIZONTAL_SCROLLBAR_HEIGHT;
        editor.layout.scroll.first_visible_row = 2;
        editor.selections = SelectionSet::from_ranges(
            vec![
                EditorSelection::new(EditorPosition::new(2, 1), EditorPosition::new(2, 3)),
                EditorSelection::new(EditorPosition::new(3, 2), EditorPosition::new(3, 4)),
            ],
            1,
        );
        let selections = editor.selections.clone();
        let text_origin_x = editor.layout.metrics.text_origin_x(&editor.decorations);
        let scrollbar =
            horizontal_scrollbar_geometry(editor.layout, editor.max_horizontal_px, text_origin_x)
                .unwrap();
        let point = Point::new(
            editor.bounds.x + scrollbar.thumb.x + scrollbar.thumb.width / 4.0,
            editor.bounds.y + scrollbar.thumb.y + scrollbar.thumb.height / 2.0,
        );
        editor.press(mouse::Button::Left, point);
        assert!(editor.state.scrollbar_grab_offset_x.is_some());
        assert_eq!(editor.selections, selections);
        editor.drag_to(Point::new(
            editor.bounds.x + editor.bounds.width + 100.0,
            point.y,
        ));
        assert_eq!(editor.layout.scroll.horizontal_px, 400.0);
        editor.drag_to(Point::new(editor.bounds.x - 100.0, point.y));
        assert_eq!(editor.layout.scroll.horizontal_px, 0.0);
        editor.dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Available(point),
        );
        assert!(editor.state.scrollbar_grab_offset_x.is_none());

        let track_end = Point::new(
            editor.bounds.x + scrollbar.track.x + scrollbar.track.width - 0.5,
            point.y,
        );
        editor.press(mouse::Button::Left, track_end);
        assert_eq!(editor.layout.scroll.horizontal_px, 400.0);
        assert_eq!(editor.layout.scroll.first_visible_row, 2);
        assert_eq!(editor.selections, selections);
        assert!(editor.state.drag_anchor.is_none());
        assert!(editor.state.text_drag.is_none());
    }

    #[test]
    fn horizontal_scrollbar_lane_padding_gutter_and_corner_do_not_place_carets() {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(20));
        editor.max_horizontal_px = 400.0;
        editor.layout.height -= super::super::scrollbar::HORIZONTAL_SCROLLBAR_HEIGHT;
        editor.selections = SelectionSet::new(EditorSelection::new(
            EditorPosition::new(0, 1),
            EditorPosition::new(0, 4),
        ));
        let selections = editor.selections.clone();
        let text_origin_x = editor.layout.metrics.text_origin_x(&editor.decorations);
        for local in [
            Point::new(text_origin_x + 20.0, editor.layout.height + 0.5),
            Point::new(10.0, editor.layout.height + 7.0),
            Point::new(editor.layout.width - 7.0, editor.layout.height + 7.0),
            Point::new(text_origin_x + 20.0, editor.bounds.height - 0.5),
        ] {
            let point = Point::new(editor.bounds.x + local.x, editor.bounds.y + local.y);
            let messages = editor.press(mouse::Button::Left, point);
            assert_eq!(editor.selections, selections);
            assert!(!messages.iter().any(|action| matches!(
                action,
                EditorAction::PlaceCaret(_) | EditorAction::PlaceCaretOnRow { .. }
            )));
            assert!(editor.state.drag_anchor.is_none());
            assert!(editor.state.text_drag.is_none());
            editor.dispatch(
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                mouse::Cursor::Available(point),
            );
        }
    }

    #[test]
    fn right_clicks_in_the_horizontal_scrollbar_lane_preserve_text_selection() {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(20));
        editor.max_horizontal_px = 400.0;
        editor.layout.height -= super::super::scrollbar::HORIZONTAL_SCROLLBAR_HEIGHT;
        editor.selections = SelectionSet::new(EditorSelection::new(
            EditorPosition::new(0, 1),
            EditorPosition::new(0, 4),
        ));
        let selections = editor.selections.clone();
        let scrollbar = horizontal_scrollbar_geometry(
            editor.layout,
            editor.max_horizontal_px,
            editor.layout.metrics.text_origin_x(&editor.decorations),
        )
        .unwrap();
        for local in [
            Point::new(
                scrollbar.track.x + scrollbar.track.width / 2.0,
                scrollbar.track.y + 5.0,
            ),
            Point::new(10.0, scrollbar.track.y + 5.0),
        ] {
            let point = Point::new(editor.bounds.x + local.x, editor.bounds.y + local.y);
            let messages = editor.press(mouse::Button::Right, point);
            assert_eq!(editor.selections, selections);
            assert!(!messages.iter().any(|action| matches!(
                action,
                EditorAction::PlaceCaret(_)
                    | EditorAction::PlaceCaretOnRow { .. }
                    | EditorAction::MoveSelection { .. }
            )));
        }
    }

    #[test]
    fn precise_input_on_one_axis_preserves_the_other_fractional_offset() {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(20));
        editor.max_horizontal_px = 1000.0;
        editor.smooth_scrolling = true;
        editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -4.0 });
        let vertical = editor.state.smooth_scroll.unwrap();
        let next_at = editor.now + Duration::from_millis(16);
        let before_horizontal = vertical.display_layout(next_at);
        editor.wheel(mouse::ScrollDelta::Pixels { x: -4.0, y: 0.0 });
        let horizontal = editor.state.smooth_scroll.unwrap();
        let after_horizontal = horizontal.display_layout(editor.now);
        assert_eq!(
            after_horizontal.scroll.first_visible_row,
            before_horizontal.scroll.first_visible_row
        );
        assert_eq!(
            after_horizontal.vertical_offset_px,
            before_horizontal.vertical_offset_px
        );
        let finished =
            horizontal.display_layout(editor.now + super::super::scroll::SCROLL_DURATION);
        assert_eq!(finished.scroll.first_visible_row, 0);
        assert!((finished.vertical_offset_px - 4.0).abs() < 0.001);
        assert_eq!(finished.scroll.horizontal_px, 4.0);
        assert!(!horizontal.animating(editor.now));

        let next_at = editor.now + Duration::from_millis(16);
        let before_vertical = horizontal.display_layout(next_at);
        editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -8.0 });
        let vertical = editor.state.smooth_scroll.unwrap();
        let after_vertical = vertical.display_layout(editor.now);
        assert_eq!(
            after_vertical.scroll.horizontal_px,
            before_vertical.scroll.horizontal_px
        );
        assert_eq!(vertical.target_horizontal_milli(), 4000);
        let finished = vertical.display_layout(editor.now + super::super::scroll::SCROLL_DURATION);
        assert_eq!(finished.scroll.horizontal_px, 4.0);
        assert!((finished.vertical_offset_px - 12.0).abs() < 0.001);
        assert!(!vertical.animating(editor.now));
    }

    #[test]
    fn smooth_scrolling_draws_intermediate_rows_and_schedules_until_finished() {
        let mut editor = TestEditor::new(&"abcdefgh\n".repeat(100));
        editor.smooth_scrolling = true;
        let messages = editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -4.0 });
        let started = editor.now;
        assert!(messages.contains(&EditorAction::ScrollToRow(6)));
        assert_eq!(editor.layout.scroll.first_visible_row, 6);
        assert_eq!(editor.last_redraw, window::RedrawRequest::NextFrame);
        let scroll = editor.state.smooth_scroll.expect("wheel starts motion");
        assert_eq!(scroll.display_layout(started).scroll.first_visible_row, 0);

        let frame = started + Duration::from_millis(16);
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(frame)),
            mouse::Cursor::Unavailable,
        );
        let displayed = editor.state.smooth_scroll.unwrap().display_layout(frame);
        assert!(displayed.scroll.first_visible_row < 6);
        assert!(displayed.vertical_offset_px > 0.0);
        assert_eq!(
            editor.last_redraw,
            window::RedrawRequest::At(frame + Duration::from_millis(16)),
        );

        let finished = started + super::super::scroll::SCROLL_DURATION;
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(finished)),
            mouse::Cursor::Unavailable,
        );
        let scroll = editor.state.smooth_scroll.unwrap();
        assert!(!scroll.animating(finished));
        assert_eq!(scroll.display_layout(finished), editor.layout);
        assert_eq!(editor.last_redraw, window::RedrawRequest::Wait);
    }

    #[test]
    fn smooth_scrolling_accumulates_wheel_input_and_reverses_without_jumping() {
        let mut editor = TestEditor::new(&"abcdefgh\n".repeat(100));
        editor.smooth_scrolling = true;
        editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -4.0 });
        let previous = editor.state.smooth_scroll.unwrap();
        let at = editor.now + Duration::from_millis(16);
        let messages = editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -4.0 });
        assert!(messages.contains(&EditorAction::ScrollToRow(12)));
        assert_eq!(
            editor.state.smooth_scroll.unwrap().display_layout(at),
            previous.display_layout(at),
        );

        let previous = editor.state.smooth_scroll.unwrap();
        let at = editor.now + Duration::from_millis(16);
        let before_reversal = previous.display_layout(at);
        let current_rows = before_reversal.scroll.first_visible_row as f64
            + f64::from(before_reversal.vertical_offset_px)
                / f64::from(before_reversal.metrics.line_height);
        let expected_rows = current_rows - 1.5;
        let messages = editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 });
        assert!(messages.contains(&EditorAction::ScrollToRow(expected_rows.floor() as usize)));
        let scroll = editor.state.smooth_scroll.unwrap();
        assert_eq!(scroll.display_layout(at), before_reversal);
        let next_frame = scroll.display_layout(at + Duration::from_millis(16));
        let next_rows = next_frame.scroll.first_visible_row as f64
            + f64::from(next_frame.vertical_offset_px) / f64::from(next_frame.metrics.line_height);
        assert!(
            next_rows < current_rows,
            "reversed input must move upward immediately"
        );
        let finished = at + super::super::scroll::SCROLL_DURATION;
        let displayed = scroll.display_layout(finished);
        let finished_rows = displayed.scroll.first_visible_row as f64
            + f64::from(displayed.vertical_offset_px) / f64::from(displayed.metrics.line_height);
        assert!((finished_rows - expected_rows).abs() < 0.000001);
    }

    #[test]
    fn smooth_scrolling_clamps_wheel_targets_to_document_ends() {
        let mut editor = TestEditor::new(&"abcdefgh\n".repeat(20));
        editor.smooth_scrolling = true;
        let max_row =
            editor.viewport.visible_row_count() - editor.layout.complete_visible_row_capacity();
        editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -1000.0 });
        assert_eq!(editor.layout.scroll.first_visible_row, max_row);
        let finished = editor.now + super::super::scroll::SCROLL_DURATION;
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(finished)),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(
            editor.state.smooth_scroll.unwrap().display_layout(finished),
            editor.layout,
        );
        editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: 1000.0 });
        assert_eq!(editor.layout.scroll.first_visible_row, 0);
        let finished = editor.now + super::super::scroll::SCROLL_DURATION;
        assert_eq!(
            editor
                .state
                .smooth_scroll
                .unwrap()
                .display_layout(finished)
                .scroll
                .first_visible_row,
            0,
        );
        assert_eq!(
            editor
                .state
                .smooth_scroll
                .unwrap()
                .display_layout(finished)
                .vertical_offset_px,
            0.0,
        );
    }

    #[test]
    fn disabling_smooth_scrolling_keeps_precise_pixel_positions() {
        let mut editor = TestEditor::new(&"abcdefgh\n".repeat(20));
        editor.smooth_scrolling = true;
        let messages = editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -4.0 });
        assert!(
            !messages
                .iter()
                .any(|action| matches!(action, EditorAction::ScrollToRow(_)))
        );
        let finished = editor.now + super::super::scroll::SCROLL_DURATION;
        let displayed = editor.state.smooth_scroll.unwrap().display_layout(finished);
        assert_eq!(displayed.scroll.first_visible_row, 0);
        assert!((displayed.vertical_offset_px - 4.0).abs() < 0.001);

        editor.smooth_scrolling = false;
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(finished)),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(editor.layout.scroll.first_visible_row, 0);
        assert_eq!(editor.displayed_layout(), displayed);
        let messages = editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -4.0 });
        assert!(!messages.contains(&EditorAction::ScrollLines(1)));
        assert!((editor.displayed_layout().vertical_offset_px - 8.0).abs() < 0.001);
        let messages = editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -8.0 });
        assert!(!messages.contains(&EditorAction::ScrollLines(1)));
        assert!((editor.displayed_layout().vertical_offset_px - 16.0).abs() < 0.001);
        let messages = editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -4.0 });
        assert!(messages.contains(&EditorAction::ScrollToRow(1)));
        assert_eq!(editor.layout.scroll.first_visible_row, 1);
        assert!((editor.displayed_layout().vertical_offset_px - 2.0).abs() < 0.001);
        assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));
    }

    #[test]
    fn smooth_scrolling_discards_motion_after_switching_documents_or_changing_geometry() {
        enum Change {
            Document,
            Width,
            ScrollPosition,
        }

        for change in [Change::Document, Change::Width, Change::ScrollPosition] {
            let mut editor = TestEditor::new(&"abcdefgh\n".repeat(20));
            editor.smooth_scrolling = true;
            editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -4.0 });
            let expected_row = match change {
                Change::Document => {
                    editor.viewport_key += 1;
                    6
                }
                Change::Width => {
                    editor.layout.width -= 20.0;
                    6
                }
                Change::ScrollPosition => {
                    editor.layout.scroll.first_visible_row = 10;
                    10
                }
            };
            editor.dispatch(
                Event::Window(window::Event::RedrawRequested(
                    editor.now + Duration::from_millis(16),
                )),
                mouse::Cursor::Unavailable,
            );
            assert!(editor.state.smooth_scroll.is_none());
            assert_eq!(editor.layout.scroll.first_visible_row, expected_row);
        }
    }

    #[test]
    fn clicking_during_smooth_scrolling_selects_the_visible_row_and_stops_motion() {
        let _font_guard = crate::font_system_test_guard();
        let mut editor = TestEditor::new(&"abcdefgh\n".repeat(100));
        editor.smooth_scrolling = true;
        editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -8.0 });
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(
                editor.now + Duration::from_millis(35),
            )),
            mouse::Cursor::Unavailable,
        );
        let clicked_at = editor.now + Duration::from_millis(16);
        let displayed = editor
            .state
            .smooth_scroll
            .unwrap()
            .display_layout(clicked_at);
        let point = editor.text_point(1, 2);
        let (_, expected_line) =
            hit_visible_row(point.y - editor.bounds.y, displayed, &editor.viewport)
                .expect("click is over text");
        let messages = editor.press(mouse::Button::Left, point);
        assert_eq!(editor.selections.main().cursor.line, expected_line);
        assert!(expected_line < 12);
        assert!(messages.contains(&EditorAction::ScrollToRow(
            displayed.scroll.first_visible_row
        )));
        let stopped = editor.state.smooth_scroll.unwrap();
        assert!(!stopped.animating(clicked_at));
        assert_eq!(stopped.display_layout(clicked_at), displayed);
        assert_eq!(
            stopped.display_layout(clicked_at + super::super::scroll::SCROLL_DURATION),
            displayed,
        );
    }

    #[test]
    fn drag_selection_reaches_the_partial_bottom_row_after_smooth_scrolling() {
        let _font_guard = crate::font_system_test_guard();
        let mut editor = TestEditor::new(&"abcdefgh\n".repeat(20));
        editor.smooth_scrolling = true;
        editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -4.0 });
        let finished = editor.now + super::super::scroll::SCROLL_DURATION;
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(finished)),
            mouse::Cursor::Unavailable,
        );
        let start = editor.text_point(1, 2);
        editor.press(mouse::Button::Left, start);
        let displayed = editor
            .state
            .smooth_scroll
            .unwrap()
            .display_layout(editor.now);
        assert!(displayed.vertical_offset_px > 0.0);
        let bottom = Point::new(start.x, editor.bounds.y + editor.bounds.height - 1.0);
        let (_, expected_line) =
            hit_visible_row(bottom.y - editor.bounds.y, displayed, &editor.viewport)
                .expect("bottom edge includes a partially visible text row");
        assert_eq!(expected_line, 5);

        let messages = editor.drag_to(bottom);
        assert_eq!(editor.selections.main().anchor.line, 1);
        assert_eq!(editor.selections.main().cursor.line, expected_line);
        assert!(messages.iter().any(|action| matches!(
            action,
            EditorAction::SelectRegion(selection) if selection.cursor.line == expected_line
        )));
        assert!(editor.state.drag_scroll_at.is_none());
    }

    #[test]
    fn focused_redraws_retain_caret_shaping_and_update_ime_coordinates() {
        let _font_guard = crate::font_system_test_guard();
        let mut editor = TestEditor::new("caf\u{e9}\t漢字\n".repeat(20).as_str());
        let cursor = EditorPosition::new(8, "caf\u{e9}\t漢".len());
        editor.selections = SelectionSet::new(EditorSelection::new(cursor, cursor));
        editor.state.is_focused = true;
        let now = Instant::now();
        let mut first = None;
        for (frame, first_visible_row) in [0, 4, 8, 4, 0].into_iter().enumerate() {
            editor.layout.scroll.first_visible_row = first_visible_row;
            editor.layout.scroll.horizontal_px = if first_visible_row == 8 { 12.0 } else { 0.0 };
            for tick in 0..3 {
                editor.dispatch(
                    Event::Window(window::Event::RedrawRequested(
                        now + Duration::from_millis((frame * 3 + tick) as u64 * 16),
                    )),
                    mouse::Cursor::Unavailable,
                );
                let expected = super::super::line_cache::measured_position_point_with_context(
                    &editor.buffer,
                    &editor.viewport,
                    &editor.decorations,
                    editor.layout,
                    cursor,
                    None,
                    &(),
                    None,
                );
                let InputMethod::Enabled {
                    cursor, purpose, ..
                } = editor.last_input_method
                else {
                    panic!("focused editor must keep the input method enabled");
                };
                assert_eq!(purpose, input_method::Purpose::Normal);
                assert_eq!(cursor.x, editor.bounds.x + expected.x);
                assert_eq!(cursor.y, editor.bounds.y + expected.y);
                if frame == 0 {
                    first = Some(cursor);
                }
                if frame == 4 {
                    assert_eq!(Some(cursor), first);
                }
            }
        }
        assert_eq!(
            editor.state.line_geometries.borrow().build_count(),
            1,
            "forced redraws and scroll-only changes must not reshape the caret paragraph"
        );
        editor.dispatch(
            Event::InputMethod(input_method::Event::Preedit(
                "候補".to_owned(),
                Some(0.."候補".len()),
            )),
            mouse::Cursor::Unavailable,
        );
        let InputMethod::Enabled {
            preedit: Some(ref preedit),
            ..
        } = editor.last_input_method
        else {
            panic!("preedit must still be published alongside retained caret geometry");
        };
        assert_eq!(preedit.content, "候補");
        assert_eq!(preedit.selection, Some(0.."候補".len()));
        assert_eq!(editor.state.line_geometries.borrow().build_count(), 1);
        editor.dispatch(
            Event::Window(window::Event::Unfocused),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(editor.last_redraw, window::RedrawRequest::NextFrame);
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

            assert!(
                messages.contains(&expected),
                "column {column}: {messages:?}"
            );
            assert!(editor.state.drag_anchor.is_none());
        }
    }

    #[test]
    fn clicks_outside_the_double_click_interval_only_place_the_caret() {
        for column in [0, 2] {
            let mut editor = TestEditor::new("alpha beta\nnext");
            let point = editor.text_point(0, column);
            editor.press(mouse::Button::Left, point);
            editor.dispatch(
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                mouse::Cursor::Available(point),
            );
            editor.now += Duration::from_millis(501);

            let messages = editor.press(mouse::Button::Left, point);

            assert!(messages.contains(&EditorAction::PlaceCaret(EditorPosition::new(0, column))));
            assert!(
                !messages.iter().any(|message| matches!(
                    message,
                    EditorAction::SelectRegion(_) | EditorAction::SelectWordAt(_)
                )),
                "column {column}: {messages:?}"
            );
            assert_eq!(
                editor.state.drag_anchor,
                Some(EditorPosition::new(0, column))
            );
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
