//! Wheel scrolling with pixel distances preserved and line distances scaled by speed.

use iced::Rectangle;
use iced::advanced::{Shell, mouse};
use iced::keyboard;
use iced::time::{Duration, Instant};

use crate::editor::action::EditorAction;
use crate::editor::layout::EditorLayout;

use super::super::scroll::ScrollAnimation;
use super::super::state::AdvancedEditorState;
use super::InteractionContext;

const FAST_SCROLL_SETTLE_MS: u64 = 120;

#[allow(clippy::too_many_arguments)]
pub(super) fn scroll_wheel<Message, Paragraph>(
    context: &InteractionContext<'_, Message>,
    state: &mut AdvancedEditorState<Paragraph>,
    delta: mouse::ScrollDelta,
    animate_wheel: bool,
    ended: bool,
    editor_layout: EditorLayout,
    bounds: Rectangle,
    shell: &mut Shell<'_, Message>,
) {
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
        } else if context.smooth_scrolling && !ended {
            scroll.scroll_precise(lines, horizontal_px, max_horizontal_px, context.now);
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

pub(in crate::editor::widget) fn scroll_delta_lines(
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
