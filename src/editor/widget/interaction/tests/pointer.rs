//! Caret placement, selection dragging, and wrapped-row hit testing.

use super::*;

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
                        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
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
