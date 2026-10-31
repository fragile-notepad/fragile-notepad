//! Routes native input using the displayed viewport and emits editor actions.

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
use crate::editor::position::{EditorPosition, SelectionSet};
use crate::editor::render::text_size;
use crate::editor::viewport::ViewportModel;

use super::actions::key_action;
use super::line_cache::{
    measured_position_point_with_cache, measured_text_hit_target_with_context,
};
use super::scroll::horizontal_offset_milli;
use super::scrollbar::{
    horizontal_offset_for_position, horizontal_scrollbar_geometry, scrollbar_row_for_position,
    vertical_scrollbar_geometry,
};
use super::state::{AdvancedEditorState, CARET_BLINK_INTERVAL_MS, TextDrag};

mod pointer;
#[cfg(test)]
mod tests;
mod wheel;

use pointer::{
    last_line_end_position, place_caret_action, selection_contains_pointer, update_drag_selection,
    whole_line_selection,
};
#[cfg(test)]
pub(super) use wheel::scroll_delta_lines;

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

/// Updates geometry before dispatch so hit tests agree with the painted rows.
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
    if !context.smooth_scrolling
        && let Some(scroll) = state.smooth_scroll.as_mut()
        && scroll.finish_precise()
    {
        shell.request_redraw();
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
        shell.request_redraw();
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
            let ended = matches!(
                phase,
                mouse::ScrollPhase::ContactEnded
                    | mouse::ScrollPhase::Ended
                    | mouse::ScrollPhase::Cancelled
            );
            if ended
                && let Some(scroll) = state.smooth_scroll.as_mut()
                && scroll.finish_precise()
            {
                shell.request_redraw();
            }
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
            wheel::scroll_wheel(
                &context,
                state,
                delta,
                animate_wheel,
                ended,
                editor_layout,
                bounds,
                shell,
            );
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
