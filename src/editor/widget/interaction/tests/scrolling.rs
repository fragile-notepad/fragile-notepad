//! Wheel, scrollbar, and animated viewport behavior.

use super::*;
use crate::editor::widget::{scroll, scrollbar};

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

        let displayed = editor
            .state
            .smooth_scroll
            .unwrap()
            .display_layout(editor.now + Duration::from_millis(64));
        assert_eq!(displayed.scroll.horizontal_px, 24.0);
        assert_eq!(displayed.scroll.first_visible_row, 1);
        assert!((displayed.vertical_offset_px - 6.0).abs() < 0.001);
        editor.apply_messages(&messages);
        editor.dispatch(
            Event::Window(window::Event::RedrawRequested(
                editor.now + Duration::from_millis(64),
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
                editor.now + scroll::SCROLL_DURATION,
            )),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(editor.displayed_layout().scroll, editor.layout.scroll);
    }
}

#[test]
fn precise_pixel_scroll_keeps_exact_distances_at_every_zoom() {
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
                    assert_eq!(
                        editor.state.smooth_scroll.unwrap().animating(editor.now),
                        smooth
                    );
                    if smooth {
                        editor.settle_precise();
                    }
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

    let initial = editor.displayed_layout();
    assert!((initial.scroll.horizontal_px - before.scroll.horizontal_px).abs() < 0.001);
    let initial_y = initial.scroll.first_visible_row as f32 * initial.metrics.line_height
        + initial.vertical_offset_px;
    assert!((initial_y - before_y).abs() < 0.001);
    assert!(editor.state.smooth_scroll.unwrap().animating(editor.now));
    editor.settle_precise();
    let after = editor.displayed_layout();
    let after_y = after.scroll.first_visible_row as f32 * after.metrics.line_height
        + after.vertical_offset_px;
    assert!((after.scroll.horizontal_px - before.scroll.horizontal_px - 8.0).abs() < 0.001);
    assert!((after_y - before_y - 8.0).abs() < 0.001);
    assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));
}

#[test]
fn fractional_line_input_preserves_both_axes_and_interpolates_later_packets() {
    let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(50)).repeat(100));
    editor.max_horizontal_px = 10_000.0;
    editor.smooth_scrolling = true;

    editor.wheel(mouse::ScrollDelta::Lines { x: -0.25, y: -1.0 });
    assert!(editor.state.smooth_scroll.unwrap().animating(editor.now));
    editor.settle_precise();

    let displayed = editor.displayed_layout();
    assert_eq!(displayed.scroll.horizontal_px, 6.75);
    assert_eq!(displayed.scroll.first_visible_row, 1);
    assert_eq!(displayed.vertical_offset_px, 9.0);
    assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));

    editor.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 });
    assert!(editor.state.smooth_scroll.unwrap().animating(editor.now));
    editor.settle_precise();

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
fn precise_gesture_end_finishes_the_received_distance_without_a_coast() {
    for phase in [
        mouse::ScrollPhase::ContactEnded,
        mouse::ScrollPhase::Ended,
        mouse::ScrollPhase::Cancelled,
    ] {
        let mut editor = TestEditor::new(&format!("{}\n", "abcdefgh".repeat(30)).repeat(100));
        editor.max_horizontal_px = 1000.0;
        editor.smooth_scrolling = true;
        editor.wheel(mouse::ScrollDelta::Pixels { x: -8.0, y: -24.0 });
        assert!(editor.state.smooth_scroll.unwrap().animating(editor.now));
        editor.dispatch(
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 },
                phase,
            }),
            mouse::Cursor::Unavailable,
        );
        let ended = editor.displayed_layout();
        assert_eq!(ended.scroll.horizontal_px, 8.0);
        assert_eq!(ended.scroll.first_visible_row, 1);
        assert!((ended.vertical_offset_px - 6.0).abs() < 0.001);
        assert!(!editor.state.smooth_scroll.unwrap().animating(editor.now));
        editor.settle_precise();
        assert_eq!(editor.displayed_layout(), ended);
        assert_ne!(editor.last_redraw, window::RedrawRequest::NextFrame);
    }
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
                scroll.display_layout(editor.now + scroll::SCROLL_DURATION),
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
    assert!((editor.layout.scroll.horizontal_px - displayed.scroll.horizontal_px).abs() < 0.001);
    assert!(messages.contains(&EditorAction::ScrollToHorizontal {
        offset_milli: stopped.target_horizontal_milli(),
    }));
    assert_eq!(
        stopped.display_layout(clicked_at).scroll.horizontal_px,
        stopped
            .display_layout(clicked_at + scroll::SCROLL_DURATION)
            .scroll
            .horizontal_px,
    );
}

#[test]
fn wrapped_views_ignore_horizontal_wheel_input_while_vertical_scrolling_continues() {
    for smooth in [false, true] {
        let mut editor = TestEditor::new(&"abcdefgh".repeat(100));
        editor.viewport = ViewportModel::new_wrapped(&editor.buffer, &FoldModel::default(), 4, 4);
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
    editor.layout.height -= scrollbar::HORIZONTAL_SCROLLBAR_HEIGHT;
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
    editor.layout.height -= scrollbar::HORIZONTAL_SCROLLBAR_HEIGHT;
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
    editor.layout.height -= scrollbar::HORIZONTAL_SCROLLBAR_HEIGHT;
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
    let finished = horizontal.display_layout(editor.now + scroll::SCROLL_DURATION);
    assert_eq!(finished.scroll.first_visible_row, 0);
    assert!((finished.vertical_offset_px - 4.0).abs() < 0.001);
    assert_eq!(finished.scroll.horizontal_px, 4.0);
    assert!(horizontal.animating(editor.now));

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
    let finished = vertical.display_layout(editor.now + scroll::SCROLL_DURATION);
    assert_eq!(finished.scroll.horizontal_px, 4.0);
    assert!((finished.vertical_offset_px - 12.0).abs() < 0.001);
    assert!(vertical.animating(editor.now));
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

    let frame = started + window::frame_interval();
    editor.dispatch(
        Event::Window(window::Event::RedrawRequested(frame)),
        mouse::Cursor::Unavailable,
    );
    let displayed = editor.state.smooth_scroll.unwrap().display_layout(frame);
    assert!(displayed.scroll.first_visible_row < 6);
    assert!(displayed.vertical_offset_px > 0.0);
    assert_eq!(editor.last_redraw, window::RedrawRequest::NextFrame);

    let finished = started + scroll::SCROLL_DURATION;
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
    let finished = at + scroll::SCROLL_DURATION;
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
    let finished = editor.now + scroll::SCROLL_DURATION;
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
    let finished = editor.now + scroll::SCROLL_DURATION;
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
    let finished = editor.now + scroll::SCROLL_DURATION;
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
        stopped.display_layout(clicked_at + scroll::SCROLL_DURATION),
        displayed,
    );
}

#[test]
fn drag_selection_reaches_the_partial_bottom_row_after_smooth_scrolling() {
    let _font_guard = crate::font_system_test_guard();
    let mut editor = TestEditor::new(&"abcdefgh\n".repeat(20));
    editor.smooth_scrolling = true;
    editor.wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -4.0 });
    let finished = editor.now + scroll::SCROLL_DURATION;
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
