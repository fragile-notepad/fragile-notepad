use crate::app::editor_ops::navigation::move_document_position;
use crate::app::editor_ops::{
    add_adjacent_caret, backspace, delete, delete_line, indent, line_span_text, paste_selection,
    replace_ranges_for_search, replace_selection, selected_text,
};
use crate::core::DocumentId;
use crate::core::document::Document;
use crate::editor::{CaretMotion, EditorPosition, EditorSelection, SelectionSet};
use crate::message::ClipboardMode;

fn position(line: usize, column: usize) -> EditorPosition {
    EditorPosition::new(line, column)
}

#[test]
fn drag_move_is_one_undo_step_and_restores_selections_in_both_directions() {
    for (text, selected, target, expected, moved) in [
        (
            "one two three",
            selection(0, 0, 0, 3),
            position(0, 13),
            " two threeone",
            "one",
        ),
        (
            "one two three",
            selection(0, 13, 0, 8),
            position(0, 0),
            "threeone two ",
            "three",
        ),
        (
            "αβ\r\n猫🐈\r\nend",
            selection(0, 0, 1, 3),
            position(2, 3),
            "🐈\r\nendαβ\r\n猫",
            "αβ\r\n猫",
        ),
        (
            "αβ\r\n猫🐈\r\nend",
            selection(1, 3, 2, 3),
            position(0, 0),
            "🐈\r\nendαβ\r\n猫",
            "🐈\r\nend",
        ),
    ] {
        let mut document = document(text, selected);
        let source = document.selection_set().clone();
        assert!(crate::app::editor_ops::move_selection(
            &mut document,
            &source,
            target,
            4
        ));
        assert_eq!(document.buffer.text(), expected);
        assert_eq!(
            document
                .buffer
                .slice_text(document.main_selection().range()),
            moved
        );
        let destination_selection = document.selection_set().clone();
        assert!(document.is_dirty);
        assert!(document.undo());
        assert_eq!(document.buffer.text(), text);
        assert_eq!(document.selection_set(), &source);
        assert!(!document.is_dirty);
        assert!(!document.undo());
        assert!(document.redo());
        assert_eq!(document.buffer.text(), expected);
        assert_eq!(document.selection_set(), &destination_selection);
    }
}

#[test]
fn drag_move_rejects_source_drops_stale_selections_and_incomplete_loads() {
    for column in 2..=5 {
        let mut document = document("abcdefgh", selection(0, 2, 0, 5));
        let source = document.selection_set().clone();
        assert!(!crate::app::editor_ops::move_selection(
            &mut document,
            &source,
            position(0, column),
            4
        ));
        assert_eq!(document.buffer.text(), "abcdefgh");
        assert_eq!(document.selection_set(), &source);
        assert!(!document.undo());
    }
    let mut document = document("abcdefgh", selection(0, 2, 0, 5));
    let source = document.selection_set().clone();
    document.set_main_selection(caret(0, 0));
    assert!(!crate::app::editor_ops::move_selection(
        &mut document,
        &source,
        position(0, 8),
        4
    ));
    document.set_selection_set(source.clone());
    document.load_state = crate::core::DocumentLoadState::Failed {
        generation: crate::core::DocumentLoadGeneration::next(),
    };
    assert!(!crate::app::editor_ops::move_selection(
        &mut document,
        &source,
        position(0, 8),
        4
    ));
    assert_eq!(document.buffer.text(), "abcdefgh");
}

#[test]
fn drag_move_handles_multiple_and_rectangular_selections() {
    for source in [
        SelectionSet::from_ranges(vec![selection(0, 1, 0, 3), selection(1, 1, 1, 3)], 1),
        SelectionSet::rectangular(position(0, 1), position(1, 3), 1, 3),
    ] {
        let mut document = document("abcd\nefgh\nend", caret(0, 0));
        document.set_selection_set(source.clone());
        assert!(crate::app::editor_ops::move_selection(
            &mut document,
            &source,
            position(2, 3),
            4
        ));
        assert_eq!(document.buffer.text(), "ad\neh\nendbc\nfg");
        assert_eq!(
            document
                .buffer
                .slice_text(document.main_selection().range()),
            "bc\nfg"
        );
        assert!(document.undo());
        assert_eq!(document.buffer.text(), "abcd\nefgh\nend");
        assert_eq!(document.selection_set(), &source);
    }
}

fn caret(line: usize, column: usize) -> EditorSelection {
    EditorSelection::new(position(line, column), position(line, column))
}

fn selection(
    anchor_line: usize,
    anchor_column: usize,
    cursor_line: usize,
    cursor_column: usize,
) -> EditorSelection {
    EditorSelection::new(
        position(anchor_line, anchor_column),
        position(cursor_line, cursor_column),
    )
}

fn document(text: &str, selection: EditorSelection) -> Document {
    let mut document = Document::untitled(DocumentId::new(1));
    document.buffer = crate::editor::EditorBuffer::from_text(text);
    document.set_main_selection(selection);
    document.refresh_after_text_change();
    document.mark_clean();
    document
}

fn wrapped_document(text: &str, selection: EditorSelection, columns: usize) -> Document {
    let mut document = document(text, selection);
    document.update_viewport_geometry(2, columns as f32 * 8.0 + 2.0, 8.0);
    document.set_word_wrap(true);
    document
}

fn navigate(document: &mut Document, motion: CaretMotion) -> EditorPosition {
    let current = document.main_selection().cursor;
    let target = move_document_position(document, current, motion);
    document.set_main_selection(EditorSelection::new(target, target));
    target
}

#[test]
fn wrapped_navigation_moves_by_visual_rows_and_preserves_local_column() {
    let mut document = wrapped_document("abcdefghijkl\nxy\nmnopqrst", caret(0, 2), 4);
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(0, 6));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(0, 10));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(1, 2));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(2, 2));
    assert_eq!(
        navigate(&mut document, CaretMotion::PageUp),
        position(0, 10)
    );
    assert_eq!(
        navigate(&mut document, CaretMotion::PageDown),
        position(2, 2)
    );
    assert_eq!(navigate(&mut document, CaretMotion::Up), position(1, 2));
}

#[test]
fn wrapped_home_end_and_vertical_clamping_keep_the_intended_row() {
    let mut document = wrapped_document("abcdefghijkl", caret(0, 2), 4);
    assert_eq!(
        navigate(&mut document, CaretMotion::LineEnd),
        position(0, 4)
    );
    assert_eq!(document.caret_visible_row(), Some(0));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(0, 8));
    assert_eq!(document.caret_visible_row(), Some(1));
    assert_eq!(navigate(&mut document, CaretMotion::Up), position(0, 4));
    assert_eq!(document.caret_visible_row(), Some(0));
    assert_eq!(
        navigate(&mut document, CaretMotion::LineStart),
        position(0, 0)
    );
    assert_eq!(
        navigate(&mut document, CaretMotion::DocumentEnd),
        position(0, 12)
    );
    assert_eq!(document.caret_visible_row(), Some(2));
    assert_eq!(
        navigate(&mut document, CaretMotion::LineStart),
        position(0, 8)
    );
    assert_eq!(
        navigate(&mut document, CaretMotion::DocumentStart),
        position(0, 0)
    );
}

#[test]
fn wrapped_vertical_navigation_handles_short_word_rows_unicode_and_tabs() {
    let mut document = wrapped_document("ab cdefghij\n\t界éz", caret(0, 7), 5);
    assert_eq!(document.caret_visible_row(), Some(1));
    assert_eq!(navigate(&mut document, CaretMotion::Up), position(0, 3));
    assert_eq!(document.caret_visible_row(), Some(0));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(0, 7));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(0, 11));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(1, 1));
    assert_eq!(document.caret_visible_row(), Some(3));
    assert_eq!(
        navigate(&mut document, CaretMotion::Down),
        position(1, "\t界éz".len())
    );
}

#[test]
fn wrapped_adjacent_carets_and_copy_keep_original_logical_text() {
    let original = "abcdefghijkl\r\n界界界界";
    let mut document = wrapped_document(original, caret(0, 2), 4);
    add_adjacent_caret(&mut document, CaretMotion::Down);
    assert_eq!(document.selection_set().len(), 2);
    assert!(
        document
            .selection_set()
            .ranges()
            .iter()
            .any(|range| range.cursor == position(0, 6))
    );
    assert_eq!(document.main_selection(), caret(0, 2));
    document.set_main_selection(selection(0, 0, 1, "界界界界".len()));
    assert_eq!(selected_text(&document, 4).as_deref(), Some(original));
    assert!(replace_selection(&mut document, "replacement", false, 4));
    assert!(document.undo());
    assert_eq!(document.text(), original);
    assert_eq!(selected_text(&document, 4).as_deref(), Some(original));
}

#[test]
fn wrapped_adjacent_carets_keep_both_sides_of_soft_breaks_when_rendered() {
    use crate::editor::{
        EditorLayout, EditorMetrics, ScrollOffset, SyntaxLineCache,
        build_render_plan_for_selection_set_with_cache_and_caret_rows,
    };

    for motion in [CaretMotion::Up, CaretMotion::Down] {
        let mut document = wrapped_document("abcdefghijkl", caret(0, 6), 4);
        navigate(&mut document, CaretMotion::LineEnd);
        assert_eq!(document.main_selection(), caret(0, 8));
        assert_eq!(document.caret_visible_row(), Some(1));

        add_adjacent_caret(&mut document, motion);

        let (target, row) = if motion == CaretMotion::Up {
            (position(0, 4), 0)
        } else {
            (position(0, 12), 2)
        };
        assert_eq!(document.selection_set().len(), 2);
        assert_eq!(document.caret_visible_row(), Some(1));
        assert_eq!(document.position_visible_row(target), Some(row));
        let metrics = EditorMetrics::default();
        let plan = build_render_plan_for_selection_set_with_cache_and_caret_rows(
            &document.buffer,
            &document.viewport,
            &document.decorations,
            document.selection_set().clone(),
            EditorLayout::new(metrics, ScrollOffset::ZERO, 200.0, 200.0),
            &SyntaxLineCache::default(),
            document.caret_row_affinities(),
        );
        assert_eq!(plan.carets.len(), 2);
        let main = plan.caret.unwrap();
        let added = plan
            .carets
            .iter()
            .find(|caret| caret.position == target)
            .unwrap();
        assert_eq!(main.y, plan.rows[1].y);
        assert_eq!(added.y, plan.rows[row].y);
        assert_eq!(main.x, plan.rows[1].text_x + 4.0 * metrics.character_width);
        assert_eq!(added.x, main.x);

        document.update_viewport_geometry(2, 26.0, 8.0);
        assert!(document.caret_row_affinities().is_empty());
    }
}

#[test]
fn wrapped_vertical_navigation_does_not_split_emoji_graphemes() {
    let mut document = wrapped_document("xxabcd\n👩‍💻abcd", caret(0, 2), 4);
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(0, 6));
    assert_eq!(navigate(&mut document, CaretMotion::Down), position(1, 0));
    assert_eq!(document.caret_visible_row(), Some(2));
    assert_eq!(
        navigate(&mut document, CaretMotion::Down),
        position(1, "👩‍💻ab".len())
    );
}

#[test]
fn wrapped_local_edits_reflow_the_whole_multi_caret_span() {
    let original = (0..40).map(|_| "abcdefgh").collect::<Vec<_>>().join("\n");
    let mut document = wrapped_document(&original, caret(1, 2), 4);
    document.defer_analysis = true;
    document.set_selection_set(SelectionSet::from_ranges(
        vec![caret(1, 2), caret(30, 6)],
        0,
    ));
    assert!(replace_selection(&mut document, "XYZ", false, 4));
    let expected =
        crate::editor::ViewportModel::new_wrapped(&document.buffer, &document.folds, 4, 4);
    assert_eq!(document.viewport, expected);
    assert_eq!(document.viewport.visible_row_count(), 82);
    assert!(document.undo());
    assert_eq!(document.text(), original);
    assert_eq!(document.viewport.visible_row_count(), 80);
}

#[test]
fn wrapped_settings_and_pointer_affinity_reach_the_active_document() {
    let (mut app, _) = super::App::new();
    let id = app.workspace.active_document_id();
    let active = app.workspace.active_document_mut().unwrap();
    active.buffer = crate::editor::EditorBuffer::from_text("abcdefghijkl");
    active.refresh_after_text_change();
    let _ = app.update(crate::message::Message::EditorAction(
        id,
        crate::editor::EditorAction::ViewportChanged {
            visible_rows: 2,
            text_width: 34,
            character_width_milli: 8000,
            font_size_milli: 14_545,
            hint_factor_milli: None,
        },
    ));
    assert_eq!(
        app.workspace
            .active_document()
            .unwrap()
            .viewport
            .visible_row_count(),
        3
    );
    let _ = app.update(crate::message::Message::EditorAction(
        id,
        crate::editor::EditorAction::PlaceCaretOnRow {
            position: position(0, 4),
            row: 0,
        },
    ));
    assert_eq!(
        app.workspace.active_document().unwrap().caret_visible_row(),
        Some(0)
    );
    let _ = app.update(crate::message::Message::ToggleWordWrap);
    let document = app.workspace.active_document().unwrap();
    assert!(!document.word_wrap());
    assert_eq!(document.viewport.visible_row_count(), 1);
    assert_eq!(document.main_selection(), caret(0, 4));
}

#[test]
fn multiline_linear_replacement_and_deletion_include_line_endings() {
    let mut document = document("one\ntwo", selection(0, 0, 1, 3));
    assert!(replace_selection(&mut document, "X", false, 4));
    assert_eq!(document.text(), "X");
    assert_eq!(document.selection_set().len(), 1);
    assert!(document.undo());
    assert!(delete(&mut document, 4));
    assert_eq!(document.text(), "");
    assert!(document.undo());
    document.set_main_selection(selection(0, 3, 1, 0));
    assert!(backspace(&mut document, 4));
    assert_eq!(document.text(), "onetwo");
}

#[test]
fn clipboard_preserves_blank_lines_and_original_line_endings() {
    let document = document("one\r\n\r\ntwo\n", selection(0, 0, 3, 0));
    assert_eq!(
        selected_text(&document, 4).as_deref(),
        Some("one\r\n\r\ntwo\n")
    );
    assert_eq!(
        line_span_text(&document, 4).as_deref(),
        Some("one\r\n\r\ntwo\n")
    );
}

#[test]
fn delete_lines_preserves_disjoint_carets_and_undoes_in_one_step() {
    let original = "keep\nalpha\nbetween\nomega\nlast";
    let mut document = document(original, caret(1, 2));
    document.set_selection_set(SelectionSet::from_ranges(vec![caret(1, 2), caret(3, 1)], 1));
    let before = document.selection_set().clone();
    let revision = document.revision();
    assert_eq!(
        line_span_text(&document, 4).as_deref(),
        Some("alpha\nomega\n")
    );

    assert!(delete_line(&mut document));

    assert_eq!(document.text(), "keep\nbetween\nlast");
    let after = SelectionSet::from_ranges(vec![caret(1, 0), caret(2, 0)], 1);
    assert_eq!(document.selection_set(), &after);
    assert_eq!(document.revision(), revision + 1);
    assert!(document.undo());
    assert_eq!(document.text(), original);
    assert_eq!(document.selection_set(), &before);
    assert!(!document.can_undo());
    assert!(document.redo());
    assert_eq!(document.text(), "keep\nbetween\nlast");
    assert_eq!(document.selection_set(), &after);
}

#[test]
fn delete_lines_merges_duplicate_overlapping_and_adjacent_line_spans() {
    let original = "zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive";
    let mut document = document(original, caret(1, 0));
    document.set_selection_set(SelectionSet::from_ranges(
        vec![
            selection(1, 1, 3, 0),
            selection(4, 0, 2, 1),
            caret(2, 0),
            caret(2, 0),
            caret(4, 1),
        ],
        1,
    ));
    let before = document.selection_set().clone();
    assert_eq!(
        line_span_text(&document, 4).as_deref(),
        Some("one\r\ntwo\r\nthree\r\nfour\r\n")
    );

    assert!(delete_line(&mut document));

    assert_eq!(document.text(), "zero\r\nfive");
    assert_eq!(document.selection_set(), &SelectionSet::single(caret(1, 0)));
    assert!(document.undo());
    assert_eq!(document.text(), original);
    assert_eq!(document.selection_set(), &before);
    assert!(!document.can_undo());
}

#[test]
fn delete_lines_includes_rectangular_endpoint_at_column_zero() {
    let mut document = document("a\nb\nc\nd", caret(0, 0));
    document.set_selection_set(SelectionSet::rectangular(
        position(2, 0),
        position(0, 0),
        0,
        0,
    ));
    let before = document.selection_set().clone();
    assert_eq!(line_span_text(&document, 8).as_deref(), Some("a\nb\nc\n"));

    assert!(delete_line(&mut document));

    assert_eq!(document.text(), "d");
    assert_eq!(document.selection_set(), &SelectionSet::single(caret(0, 0)));
    assert!(document.undo());
    assert_eq!(document.text(), "a\nb\nc\nd");
    assert_eq!(document.selection_set(), &before);
}

#[test]
fn delete_final_line_preserves_existing_line_ending_semantics() {
    for (original, at, expected, changed) in [
        ("first\nlast", caret(1, 2), "first\n", true),
        ("first\r\nlast", caret(1, 2), "first\r\n", true),
        ("only", caret(0, 2), "", true),
        ("first\n", caret(1, 0), "first\n", false),
        ("", caret(0, 0), "", false),
    ] {
        let mut document = document(original, at);

        assert_eq!(delete_line(&mut document), changed);
        assert_eq!(document.text(), expected);
        assert_eq!(document.can_undo(), changed);
        if changed {
            assert!(document.undo());
            assert_eq!(document.text(), original);
            assert_eq!(document.main_selection(), at);
        }
    }
}

#[test]
fn cut_and_delete_line_actions_remove_all_caret_lines_through_unterminated_eof() {
    use crate::editor::EditorAction;

    for action in [
        EditorAction::Cut,
        EditorAction::CutLine,
        EditorAction::DeleteLine,
    ] {
        let (mut app, _) = super::App::new();
        let document_id = app.workspace.active_document_id();
        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        document.buffer = crate::editor::EditorBuffer::from_text("keep\nalpha\nmiddle\nlast");
        document.set_selection_set(SelectionSet::from_ranges(vec![caret(1, 1), caret(3, 1)], 1));
        document.refresh_after_text_change();
        document.mark_clean();
        let before = document.selection_set().clone();
        assert_eq!(line_span_text(document, 4).as_deref(), Some("alpha\nlast"));

        let _ = app.update_editor(document_id, action);

        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        assert_eq!(document.text(), "keep\nmiddle\n");
        assert_eq!(document.main_selection(), caret(2, 0));
        assert_eq!(document.selection_set().len(), 2);
        assert!(document.undo());
        assert_eq!(document.text(), "keep\nalpha\nmiddle\nlast");
        assert_eq!(document.selection_set(), &before);
        assert!(!document.can_undo());
    }
}

#[test]
fn indent_preserves_selected_contents_direction_and_undo() {
    let original = selection(2, 0, 0, 1);
    let mut document = document("one\ntwo\nthree", original);
    assert!(indent(&mut document, 2, "  "));
    assert_eq!(document.text(), "  one\n  two\nthree");
    assert_eq!(document.main_selection(), selection(2, 0, 0, 3));
    assert!(document.undo());
    assert_eq!(document.text(), "one\ntwo\nthree");
    assert_eq!(document.main_selection(), original);
}

#[test]
fn search_batch_is_one_undoable_transaction() {
    let mut document = document("one one one", caret(0, 0));
    let revision = document.revision();
    let ranges = [0, 4, 8]
        .into_iter()
        .map(|column| {
            (
                crate::editor::EditorRange::new(position(0, column), position(0, column + 3)),
                "X".to_owned(),
            )
        })
        .collect();
    assert!(replace_ranges_for_search(&mut document, ranges));
    assert_eq!(document.text(), "X X X");
    assert_eq!(document.selection_set().len(), 1);
    assert_eq!(document.main_selection(), caret(0, 1));
    assert_eq!(document.revision(), revision + 1);
    assert!(document.undo());
    assert_eq!(document.text(), "one one one");
    assert!(!document.can_undo());
    assert!(document.redo());
    assert_eq!(document.selection_set().len(), 1);
    assert_eq!(document.main_selection(), caret(0, 1));
}

#[test]
fn replace_selection_records_single_edit_and_updates_selection() {
    let mut document = document("hello world", selection(0, 6, 0, 11));

    assert!(replace_selection(&mut document, "notepad", false, 4));

    assert_eq!(document.text(), "hello notepad");
    assert_eq!(document.main_selection(), caret(0, 13));
    assert!(document.is_dirty);
    assert!(document.can_undo());
}

#[test]
fn replace_selection_maps_many_replacement_carets_without_full_document_copy() {
    let line_count = 1024;
    let text = (0..line_count).map(|_| "x").collect::<Vec<_>>().join("\n");
    let mut document = document(&text, caret(0, 1));
    document.set_selection_set(SelectionSet::from_ranges(
        (0..line_count).map(|line| caret(line, 1)).collect(),
        line_count - 1,
    ));

    assert!(replace_selection(&mut document, "!", false, 4));

    let expected = (0..line_count).map(|_| "x!").collect::<Vec<_>>().join("\n");
    assert_eq!(document.text(), expected);
    assert_eq!(document.main_selection(), caret(line_count - 1, 2));
}

#[test]
fn backspace_removes_previous_grapheme_for_each_caret() {
    let mut document = document("alpha\nbeta", caret(1, 2));

    assert!(backspace(&mut document, 4));

    assert_eq!(document.text(), "alpha\nbta");
    assert_eq!(document.main_selection(), caret(1, 1));
}

#[test]
fn delete_removes_next_grapheme_for_each_caret() {
    let mut document = document("alpha", caret(0, 1));

    assert!(delete(&mut document, 4));

    assert_eq!(document.text(), "apha");
    assert_eq!(document.main_selection(), caret(0, 1));
}

#[test]
fn rectangular_paste_replaces_projected_lines() {
    let mut document = document("one\ntwo\nthree", caret(0, 1));
    document.set_selection_set(SelectionSet::rectangular(
        position(0, 1),
        position(2, 2),
        1,
        2,
    ));

    assert!(paste_selection(
        &mut document,
        ClipboardMode::Rectangular { line_count: 3 },
        "A\nB\nC",
        4,
    ));

    assert_eq!(document.text(), "oAe\ntBo\ntCree");
}

#[test]
fn rectangular_paste_splits_crlf_clipboard_lines() {
    let mut document = document("one\ntwo\nthree", caret(0, 1));
    document.set_selection_set(SelectionSet::rectangular(
        position(0, 1),
        position(2, 2),
        1,
        2,
    ));

    assert!(paste_selection(
        &mut document,
        ClipboardMode::Rectangular { line_count: 3 },
        "A\r\nB\r\nC",
        4,
    ));

    assert_eq!(document.text(), "oAe\ntBo\ntCree");
}

#[test]
fn rectangular_paste_preserves_trailing_crlf_as_empty_line() {
    let mut document = document("one\ntwo\nthree", caret(0, 1));
    document.set_selection_set(SelectionSet::rectangular(
        position(0, 1),
        position(2, 2),
        1,
        2,
    ));

    assert!(paste_selection(
        &mut document,
        ClipboardMode::Rectangular { line_count: 3 },
        "A\r\nB\r\n",
        4,
    ));

    assert_eq!(document.text(), "oAe\ntBo\ntree");
}

#[test]
fn rectangular_paste_falls_back_to_linear_when_line_counts_do_not_match() {
    let mut document = document("one\ntwo", caret(0, 1));
    document.set_selection_set(SelectionSet::rectangular(
        position(0, 1),
        position(1, 2),
        1,
        2,
    ));

    assert!(paste_selection(
        &mut document,
        ClipboardMode::Rectangular { line_count: 3 },
        "A\nB\nC",
        4,
    ));

    assert_eq!(document.text(), "oA\nB\nCe\ntA\nB\nCo");
}
