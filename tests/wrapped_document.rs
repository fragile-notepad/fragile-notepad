use fragile_notepad::core::document::analyze_document;
use fragile_notepad::core::{Document, DocumentId, DocumentLoadGeneration, TextEncoding};
use fragile_notepad::editor::{
    EditTransaction, EditorPosition, EditorRange, EditorSelection, FoldModel, FoldRange,
};

fn caret(line: usize, column: usize) -> EditorSelection {
    let position = EditorPosition::new(line, column);
    EditorSelection::new(position, position)
}

fn wrapped_document(text: &str, columns: usize) -> Document {
    let mut document = Document::from_path(DocumentId::new(1), "wrapped.txt", text);
    document.update_viewport_geometry(3, columns as f32 * 8.0 + 2.0, 8.0);
    document.set_word_wrap(true);
    document
}

#[test]
fn wrapped_toggle_and_resize_preserve_text_history_and_logical_top() {
    let text = "header\r\nabcdefghijklmnopqrst\r\nlast";
    let mut document = wrapped_document(text, 5);
    let save_bytes = document.bytes_for_save().unwrap();
    document.scroll.first_visible_row = document
        .viewport
        .position_to_visible_row(EditorPosition::new(1, 10))
        .unwrap();
    document.scroll.horizontal_px = 48.0;

    document.update_viewport_geometry(3, 82.0, 8.0);

    assert_eq!(document.viewport.wrap_columns(), Some(10));
    assert_eq!(document.scroll.first_visible_row, 2);
    assert_eq!(document.scroll.horizontal_px, 0.0);
    document.set_word_wrap(false);
    assert_eq!(document.viewport.visible_row_count(), 3);
    assert_eq!(document.scroll.first_visible_row, 1);
    document.set_word_wrap(true);
    assert_eq!(document.scroll.first_visible_row, 1);
    assert_eq!(document.text(), text);
    assert_eq!(document.bytes_for_save().unwrap(), save_bytes);
    assert!(!document.is_dirty);
    assert!(!document.can_undo());
}

#[test]
fn wrapped_toggle_keeps_a_visible_caret_in_view_in_both_directions() {
    let mut document = Document::from_path(DocumentId::new(1), "wrap.txt", &"a".repeat(800));
    document.update_viewport_geometry(10, 322.0, 8.0);
    document.set_main_selection(caret(0, 600));
    document.ensure_caret_visible();
    assert!(document.scroll.horizontal_px > 0.0);

    document.set_word_wrap(true);

    assert_eq!(document.caret_visible_row(), Some(15));
    assert_eq!(document.scroll.first_visible_row, 6);
    assert_eq!(document.scroll.horizontal_px, 0.0);

    document.set_word_wrap(false);

    assert_eq!(document.scroll.first_visible_row, 0);
    let caret_x = 600.0 * 8.0;
    assert!(caret_x >= document.scroll.horizontal_px);
    assert!(caret_x < document.scroll.horizontal_px + document.viewport_text_width);
    assert!(!document.is_dirty);
    assert!(!document.can_undo());
}

#[test]
fn wrapped_toggle_preserves_a_view_scrolled_away_from_the_caret() {
    let mut document = Document::from_path(DocumentId::new(1), "wrap.txt", &"a".repeat(800));
    document.update_viewport_geometry(10, 322.0, 8.0);
    document.set_main_selection(caret(0, 600));
    document.set_word_wrap(true);
    assert_eq!(document.scroll.first_visible_row, 0);
    assert_eq!(document.caret_visible_row(), Some(15));

    let mut document = wrapped_document("first\nabcdefghijklm\nlast", 4);
    document.scroll.first_visible_row = 3;
    document.set_word_wrap(false);
    assert_eq!(document.scroll.first_visible_row, 1);
    document.set_word_wrap(true);
    assert_eq!(document.scroll.first_visible_row, 2);
}

#[test]
fn wrapped_edit_undo_and_redo_reflow_even_while_analysis_is_deferred() {
    let mut document = wrapped_document("éééé", 3);
    document.defer_analysis = true;
    let before = caret(0, "éééé".len());
    document.set_main_selection(before);
    let delta = document.buffer.replace_range(before.range(), "ééé");
    let after = caret(0, "ééééééé".len());
    document.set_main_selection(after);
    document.history.record(EditTransaction {
        delta,
        before_selection: before,
        after_selection: after,
    });
    document.refresh_text_from(0);

    assert_eq!(document.viewport.visible_row_count(), 3);
    assert_eq!(document.text(), "ééééééé");
    assert!(document.undo());
    assert_eq!(document.viewport.visible_row_count(), 2);
    assert_eq!(document.text(), "éééé");
    assert!(document.redo());
    assert_eq!(document.viewport.visible_row_count(), 3);
    assert_eq!(document.main_selection(), after);
}

#[test]
fn wrapped_folds_reflow_and_revealing_hidden_caret_expands_them() {
    let mut document = wrapped_document("headerabcdefgh\n  abcdefghi\nend\nlast", 10);
    let range = FoldRange::new(0, 2);
    document.folds = FoldModel::new(vec![range]);
    document.refresh_view_models();
    let expanded_rows = document.viewport.visible_row_count();
    document.scroll.first_visible_row = document.viewport.document_line_to_visible_row(1).unwrap();
    document.folds.set_collapsed(range, true);
    document.refresh_view_models();

    assert!(document.viewport.visible_row_count() < expanded_rows);
    assert_eq!(document.viewport.document_line_to_visible_row(1), None);
    assert_eq!(document.scroll.first_visible_row, 0);
    document.set_main_selection(caret(1, 9));
    document.ensure_caret_visible();
    assert!(!document.folds.is_collapsed(range));
    assert_eq!(document.viewport.visible_row_count(), expanded_rows);
    let row = document.caret_visible_row().unwrap();
    assert!(row >= document.scroll.first_visible_row);
    assert!(row < document.scroll.first_visible_row + document.viewport_visible_rows);
}

#[test]
fn wrapped_tab_width_changes_reflow_before_background_analysis() {
    let mut document = wrapped_document("\ta", 4);
    document.defer_analysis = true;
    assert_eq!(document.viewport.visible_row_count(), 2);
    let mut settings = document.decorations.settings;
    settings.indent_width = 2;

    document.set_decoration_settings(settings);

    assert_eq!(document.viewport.visible_row_count(), 1);
    assert_eq!(document.text(), "\ta");
    assert!(document.analysis_pending);
}

#[test]
fn wrapped_streamed_loading_and_analysis_keep_continuation_rows() {
    let generation = DocumentLoadGeneration::next();
    let mut document = Document::loading(DocumentId::new(1), "wrapped.txt", generation);
    document.defer_analysis = true;
    document.update_viewport_geometry(3, 34.0, 8.0);
    document.set_word_wrap(true);
    assert!(document.replace_loading_preview(generation, "abc", true, 3, Some(12)));
    assert!(document.replace_loading_preview(generation, "defgh\nxyz", false, 12, Some(12)));
    assert_eq!(document.viewport.visible_row_count(), 3);
    assert!(document.complete_streaming_load(generation, TextEncoding::Utf8));
    let (buffer, request) = document.analysis_request().unwrap();
    assert!(document.apply_analysis(analyze_document(buffer, request)));
    assert_eq!(document.viewport.visible_row_count(), 3);
    assert_eq!(document.text(), "abcdefgh\nxyz");
}

#[test]
fn wrapped_caret_affinity_is_preserved_at_row_end_and_cleared_by_reflow() {
    let mut document = wrapped_document("abcdefghijkl", 4);
    document.viewport_visible_rows = 1;
    let position = EditorPosition::new(0, 4);
    document.set_main_selection(caret(0, 4));
    document.set_caret_row_affinity(position, 0);
    document.ensure_caret_visible();
    assert_eq!(document.caret_visible_row(), Some(0));
    assert_eq!(document.scroll.first_visible_row, 0);

    // A pixel change that leaves the number of columns unchanged retains the
    // selected side of the wrap boundary; an actual reflow invalidates it.
    document.update_viewport_geometry(1, 35.0, 8.0);
    assert_eq!(document.caret_visible_row(), Some(0));
    document.update_viewport_geometry(1, 26.0, 8.0);
    assert_eq!(document.caret_visible_row(), Some(1));
    document.ensure_caret_visible();
    assert_eq!(document.scroll.first_visible_row, 1);
    document.buffer.replace_range(
        EditorRange::new(EditorPosition::new(0, 0), EditorPosition::new(0, 12)),
        "x",
    );
    document.refresh_text_from(0);
    assert_eq!(document.scroll.first_visible_row, 0);
}

#[test]
fn wrapped_reveal_position_finds_deep_continuations_and_unfolds_targets() {
    let mut document = wrapped_document("header\nabcdefghijklmnopqrstuvwx\nend", 4);
    let range = FoldRange::new(0, 2);
    document.folds = FoldModel::new(vec![range]);
    document.folds.set_collapsed(range, true);
    document.refresh_view_models();
    document.viewport_visible_rows = 2;
    let target = EditorPosition::new(1, 21);
    assert_eq!(document.main_selection(), caret(0, 0));

    document.reveal_position(target);

    assert!(!document.folds.is_collapsed(range));
    let row = document.viewport.position_to_visible_row(target).unwrap();
    assert_eq!(document.scroll.first_visible_row, row - 1);
    assert_eq!(document.main_selection(), caret(0, 0));
}

#[test]
fn wrapped_analysis_preserves_retained_folds_and_reveals_removed_folds() {
    let mut document = wrapped_document("header\n  abcdefghi\nend\nlast", 5);
    let range = FoldRange::new(0, 2);
    document.folds = FoldModel::new(vec![range]);
    document.refresh_view_models();
    let expanded_rows = document.viewport.visible_row_count();
    document.folds.set_collapsed(range, true);
    document.refresh_view_models();
    let collapsed_rows = document.viewport.visible_row_count();
    assert_eq!(document.viewport.document_line_to_visible_row(1), None);

    document.analysis_pending = true;
    let (_, mut retained) = document.analysis_request().unwrap();
    retained.folds = FoldModel::new(vec![range]);
    assert!(document.apply_analysis(retained));
    assert!(document.folds.is_collapsed(range));
    assert_eq!(document.viewport.visible_row_count(), collapsed_rows);
    assert_eq!(document.viewport.document_line_to_visible_row(1), None);

    document.analysis_pending = true;
    let (_, removed) = document.analysis_request().unwrap();
    assert!(document.apply_analysis(removed));
    assert!(!document.folds.is_collapsed(range));
    assert_eq!(document.viewport.visible_row_count(), expanded_rows);
    assert!(document.viewport.document_line_to_visible_row(1).is_some());
}

#[test]
fn wrapped_background_analysis_preserves_boundary_affinity_when_visibility_is_unchanged() {
    let mut document = wrapped_document("abcdefghijkl", 4);
    document.defer_analysis = true;
    document.refresh_text_lines(0, 0);
    let (buffer, request) = document.analysis_request().unwrap();
    document.set_main_selection(caret(0, 4));
    document.set_caret_row_affinity(EditorPosition::new(0, 4), 0);

    assert!(document.apply_analysis(analyze_document(buffer, request)));

    assert_eq!(document.caret_visible_row(), Some(0));
    assert_eq!(document.viewport.visible_row_count(), 3);
}

#[test]
fn wrapped_resize_keeps_visible_caret_visible_without_jumping_from_scrolled_away_text() {
    let mut document = wrapped_document("abcdefghijklmnopqrstuvwx", 12);
    document.viewport_visible_rows = 2;
    document.set_main_selection(caret(0, 20));

    document.update_viewport_geometry(2, 34.0, 8.0);

    assert_eq!(document.caret_visible_row(), Some(5));
    assert_eq!(document.scroll.first_visible_row, 4);
    document.set_main_selection(caret(0, 0));
    document.scroll.first_visible_row = 2;

    document.update_viewport_geometry(2, 18.0, 8.0);

    assert_eq!(document.scroll.first_visible_row, 4);
    assert_eq!(document.caret_visible_row(), Some(0));
}

#[test]
fn wrapped_eol_markers_reserve_space_and_reflow_when_toggled() {
    let mut document = wrapped_document("abcd", 4);
    assert_eq!(document.viewport.visible_row_count(), 1);
    let mut settings = document.decorations.settings;
    settings.show_end_of_line_markers = true;

    document.set_decoration_settings(settings);

    assert_eq!(document.viewport.wrap_columns(), Some(3));
    assert_eq!(document.viewport.visible_row_count(), 2);
    settings.show_end_of_line_markers = false;
    document.set_decoration_settings(settings);
    assert_eq!(document.viewport.visible_row_count(), 1);
    settings.show_end_of_line_markers = true;
    document.set_decoration_settings(settings);
    document.update_viewport_geometry(2, 1.0, 8.0);
    assert_eq!(document.viewport.wrap_columns(), Some(1));
    assert_eq!(document.viewport.visible_row_count(), 4);
    assert_eq!(document.text(), "abcd");
}
