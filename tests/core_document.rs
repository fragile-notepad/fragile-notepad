use fragile_notepad::core::{
    Document, DocumentId, MAX_FULL_DOCUMENT_ANALYSIS_BYTES, TextEncoding, decode_bytes, encode_text,
};
use fragile_notepad::editor::{
    EditTransaction, EditorPosition, EditorRange, EditorSelection, SelectionSet,
};
use iced::widget::text_editor::LineEnding;
use std::path::{Path, PathBuf};

#[test]
fn fixed_wrap_columns_follow_window_size_and_zoom_without_changing_text() {
    let text = "x".repeat(240);
    let mut document = Document::from_path(DocumentId::new(900), "wrap.txt", &text);
    document.update_viewport_geometry(20, 962.0, 8.0);
    document.set_wrap_column_limit(Some(80));
    document.set_word_wrap(true);
    assert_eq!(document.viewport.wrap_columns(), Some(80));
    document.update_viewport_geometry(20, 322.0, 8.0);
    assert_eq!(document.viewport.wrap_columns(), Some(40));
    document.update_viewport_geometry(20, 962.0, 8.0);
    assert_eq!(document.viewport.wrap_columns(), Some(80));
    document.update_viewport_geometry(20, 962.0, 16.0);
    assert_eq!(document.viewport.wrap_columns(), Some(60));
    document.set_wrap_column_limit(None);
    document.update_viewport_geometry(20, 962.0, 8.0);
    assert_eq!(document.viewport.wrap_columns(), Some(120));
    document.set_word_wrap(false);
    assert_eq!(document.viewport.wrap_columns(), None);
    assert_eq!(document.text(), text);
}

fn fixture_path(file_name: &str) -> PathBuf {
    Path::new("work").join(file_name)
}

fn caret(line: usize, column: usize) -> EditorSelection {
    let position = EditorPosition::new(line, column);

    EditorSelection::new(position, position)
}

fn document_end(document: &Document) -> EditorPosition {
    let line = document.buffer.line_count().saturating_sub(1);
    let column = document.buffer.line(line).unwrap_or_default().len();

    EditorPosition::new(line, column)
}

fn insert_at_end(document: &mut Document, text: &str) {
    let before_selection = caret(document_end(document).line, document_end(document).column);
    document.selection = before_selection;
    let delta = document.buffer.replace_range(
        EditorRange::new(document_end(document), document_end(document)),
        text,
    );

    let after = document_end(document);
    document.selection = EditorSelection::new(after, after);
    document.history.record(EditTransaction {
        delta,
        before_selection,
        after_selection: document.selection,
    });
    document.refresh_after_text_change();
}

#[test]
fn untitled_document_has_default_title_and_clean_text_state() {
    let document = Document::untitled(DocumentId::new(7));

    assert_eq!(document.title(), "Untitled 7");
    assert!(document.path.is_none());
    assert_eq!(document.syntax_token, "txt");
    assert_eq!(document.line_ending, Some(LineEnding::Lf));
    assert!(!document.is_dirty);
    assert_eq!(document.text(), "");
    assert_eq!(document.text_for_save(), "");
}

#[test]
fn loaded_document_uses_file_name_and_lowercase_extension_for_syntax() {
    let document = Document::from_path(
        DocumentId::new(2),
        fixture_path("Notes.Example.RS"),
        "fn main() {}\n",
    );

    assert_eq!(document.title(), "Notes.Example.RS");
    assert_eq!(document.syntax_token, "rs");
    assert!(!document.is_dirty);
}

#[test]
fn loaded_document_without_extension_uses_plain_text_syntax() {
    let document = Document::from_path(DocumentId::new(3), fixture_path("README"), "hello");

    assert_eq!(document.title(), "README");
    assert_eq!(document.syntax_token, "txt");
}

#[test]
fn only_non_plain_text_documents_use_syntax_highlighting() {
    let untitled = Document::untitled(DocumentId::new(1));
    let text = Document::from_path(DocumentId::new(2), fixture_path("notes.txt"), "hello");
    let rust = Document::from_path(
        DocumentId::new(3),
        fixture_path("main.rs"),
        "fn main() {}\n",
    );

    assert!(!untitled.uses_syntax_highlighting());
    assert!(!text.uses_syntax_highlighting());
    assert!(rust.uses_syntax_highlighting());
}

#[test]
fn document_selection_set_helpers_preserve_single_selection_compatibility() {
    let mut document = Document::from_path(DocumentId::new(20), fixture_path("note.txt"), "hello");
    let selection = EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 4));

    document.set_main_selection(selection);

    assert_eq!(document.selection, selection);
    assert_eq!(document.main_selection(), selection);
    assert_eq!(document.selection_set().main(), selection);
    assert!(document.selection_set().is_single());
}

#[test]
fn document_selection_set_helpers_store_and_clamp_multi_selection() {
    let mut document =
        Document::from_path(DocumentId::new(21), fixture_path("note.txt"), "abc\nxy");
    let first = EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 9));
    let second = EditorSelection::new(EditorPosition::new(4, 0), EditorPosition::new(1, 1));
    let selection_set = SelectionSet::from_ranges(vec![first, second], 1);

    document.set_selection_set(selection_set);

    assert_eq!(
        document.main_selection(),
        EditorSelection::new(EditorPosition::new(1, 0), EditorPosition::new(1, 1))
    );
    assert_eq!(document.selection, document.main_selection());
    assert_eq!(document.selection_set().len(), 2);
    assert_eq!(
        document.selection_set().ranges()[0].selection(),
        EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 3))
    );
}

#[test]
fn syntax_token_can_be_selected_without_dirtying_document() {
    let mut document = Document::untitled(DocumentId::new(1));

    document.set_syntax_token("rs");

    assert_eq!(document.syntax_token, "rs");
    assert!(!document.is_dirty);
    assert!(document.uses_syntax_highlighting());
}

#[test]
fn document_revision_increments_for_text_changes_undo_and_redo() {
    let mut document = Document::from_path(DocumentId::new(13), fixture_path("note.txt"), "a");

    assert_eq!(document.revision(), 0);

    insert_at_end(&mut document, "b");
    assert_eq!(document.text(), "ab");
    assert_eq!(document.revision(), 1);

    assert!(document.undo());
    assert_eq!(document.text(), "a");
    assert_eq!(document.revision(), 2);

    assert!(document.redo());
    assert_eq!(document.text(), "ab");
    assert_eq!(document.revision(), 3);
}

#[test]
fn document_revision_increments_for_syntax_changes_without_dirtying_document() {
    let mut document = Document::from_path(DocumentId::new(14), fixture_path("note.txt"), "hello");

    assert_eq!(document.revision(), 0);
    assert!(!document.is_dirty);

    document.set_syntax_token("rs");

    assert_eq!(document.syntax_token, "rs");
    assert_eq!(document.revision(), 1);
    assert!(!document.is_dirty);

    document.set_syntax_token("rs");

    assert_eq!(document.revision(), 1);
    assert!(!document.is_dirty);
}

#[test]
fn syntax_token_selection_recomputes_syntax_aware_folds() {
    let mut document = Document::from_path(
        DocumentId::new(11),
        fixture_path("example.unknown"),
        "# {\ncomment body\n# }\n// {\nbody\n// }",
    );
    let hash_comment_fold = fragile_notepad::editor::FoldRange::new(0, 2);
    let slash_comment_fold = fragile_notepad::editor::FoldRange::new(3, 5);

    assert!(document.folds.ranges().contains(&hash_comment_fold));
    assert!(!document.folds.ranges().contains(&slash_comment_fold));

    document.set_syntax_token("py");

    assert!(!document.folds.ranges().contains(&hash_comment_fold));
    assert!(document.folds.ranges().contains(&slash_comment_fold));
    assert!(!document.is_dirty);
}

#[test]
fn manual_non_plain_syntax_survives_path_changes() {
    let mut document = Document::untitled(DocumentId::new(1));

    document.set_syntax_token("rs");
    document.set_path(fixture_path("README"));

    assert_eq!(document.syntax_token, "rs");
}

#[test]
fn plain_text_syntax_selection_allows_later_path_detection() {
    let mut document = Document::untitled(DocumentId::new(1));

    document.set_syntax_token("txt");
    document.set_path(fixture_path("main.c"));

    assert_eq!(document.syntax_token, "c");
}

#[test]
fn apply_action_marks_dirty_only_for_edits() {
    let mut document = Document::untitled(DocumentId::new(1));

    document.selection = caret(0, 0);

    assert!(!document.is_dirty);
    assert_eq!(document.text(), "");

    insert_at_end(&mut document, "x");

    assert!(document.is_dirty);
    assert_eq!(document.text(), "x");

    document.mark_clean();

    assert!(!document.is_dirty);
}

#[test]
fn metadata_dirty_state_survives_text_refresh() {
    let mut document = Document::from_path(DocumentId::new(18), fixture_path("note.txt"), "saved");
    document.mark_clean();

    document.set_encoding(TextEncoding::Utf8Bom);
    assert!(document.is_dirty);

    document.refresh_after_text_change();

    assert!(document.is_dirty);
}

#[test]
fn undo_restores_previous_text_and_clean_state() {
    let mut document = Document::from_path(DocumentId::new(1), fixture_path("note.txt"), "a");

    insert_at_end(&mut document, "b");

    assert_eq!(document.text(), "ab");
    assert!(document.is_dirty);
    assert!(document.can_undo());

    assert!(document.undo());

    assert_eq!(document.text(), "a");
    assert!(!document.is_dirty);
    assert!(!document.can_undo());
    assert!(document.can_redo());
}

#[test]
fn redo_reapplies_undone_edit_and_dirty_state() {
    let mut document = Document::from_path(DocumentId::new(1), fixture_path("note.txt"), "a");

    insert_at_end(&mut document, "b");
    assert!(document.undo());
    assert!(document.redo());

    assert_eq!(document.text(), "ab");
    assert!(document.is_dirty);
    assert!(document.can_undo());
    assert!(!document.can_redo());
}

#[test]
fn new_edit_after_undo_clears_redo_history() {
    let mut document = Document::from_path(DocumentId::new(1), fixture_path("note.txt"), "a");

    insert_at_end(&mut document, "b");
    assert!(document.undo());
    insert_at_end(&mut document, "c");

    assert_eq!(document.text(), "ac");
    assert!(!document.can_redo());
}

#[test]
fn undo_and_redo_report_false_when_history_is_empty() {
    let mut document = Document::untitled(DocumentId::new(1));

    assert!(!document.can_undo());
    assert!(!document.can_redo());
    assert!(!document.undo());
    assert!(!document.redo());
}

#[test]
fn text_for_save_preserves_detected_crlf_and_adds_final_line_ending() {
    let document = Document::from_path(DocumentId::new(4), fixture_path("notes.txt"), "one\r\ntwo");

    assert_eq!(document.line_ending, Some(LineEnding::CrLf));
    assert_eq!(document.text(), "one\r\ntwo");
    assert_eq!(document.text_for_save(), "one\r\ntwo\r\n");
    assert_eq!(
        document.bytes_for_save().expect("encoded bytes"),
        b"one\r\ntwo\r\n"
    );
}

#[test]
fn text_for_save_does_not_duplicate_existing_final_line_ending() {
    let document = Document::from_path(DocumentId::new(5), fixture_path("notes.txt"), "one\ntwo\n");

    assert_eq!(document.line_ending, Some(LineEnding::Lf));
    assert_eq!(document.text_for_save(), "one\ntwo\n");
}

#[test]
fn text_for_save_uses_detected_line_ending_after_replacement() {
    let mut document = Document::untitled(DocumentId::new(6));
    let before_selection = caret(0, 0);

    document.selection = before_selection;
    let delta = document.buffer.replace_range(
        EditorRange::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
        "one\r\ntwo",
    );
    document.line_ending = fragile_notepad::core::document::detect_line_ending("one\r\ntwo");
    document.selection = caret(1, 3);
    document.history.record(EditTransaction {
        delta,
        before_selection,
        after_selection: document.selection,
    });
    document.refresh_after_text_change();

    assert_eq!(document.line_ending, Some(LineEnding::CrLf));
    assert_eq!(document.text_for_save(), "one\r\ntwo\r\n");
}

#[test]
fn loaded_utf8_bom_document_strips_marker_from_editor_text_and_saves_it_back() {
    let document = Document::from_decoded(
        DocumentId::new(8),
        fixture_path("note.txt"),
        decode_bytes(b"\xef\xbb\xbfhello"),
    );

    assert_eq!(document.encoding, TextEncoding::Utf8Bom);
    assert_eq!(document.text(), "hello");
    assert_eq!(
        document.bytes_for_save().expect("encoded bytes"),
        b"\xef\xbb\xbfhello"
    );
}

#[test]
fn changing_encoding_to_utf8_bom_marks_document_dirty_and_adds_marker_on_save() {
    let mut document = Document::from_path(DocumentId::new(9), fixture_path("note.txt"), "hello");

    assert_eq!(document.encoding, TextEncoding::Utf8);
    assert!(!document.is_dirty);

    document.set_encoding(TextEncoding::Utf8Bom);

    assert!(document.is_dirty);
    assert_eq!(
        document.bytes_for_save().expect("encoded bytes"),
        b"\xef\xbb\xbfhello"
    );
}

#[test]
fn bom_marked_documents_preserve_leading_feff_content_on_open_reload_and_save() {
    for encoding in [
        TextEncoding::Utf8Bom,
        TextEncoding::Utf16BeBom,
        TextEncoding::Utf16LeBom,
    ] {
        for text in ["\u{feff}", "\u{feff}hello", "\u{feff}\u{feff}hello"] {
            let bytes = encode_text(text, encoding).unwrap();
            let decoded = decode_bytes(&bytes);
            assert_eq!(decoded.text, text);
            assert!(!decoded.had_errors);
            let document = Document::from_decoded(DocumentId::new(10), "bom.txt", decoded);
            assert_eq!(document.text(), text);
            assert_eq!(document.bytes_for_save().unwrap(), bytes);

            let generation = fragile_notepad::core::DocumentLoadGeneration::next();
            let mut reloaded = Document::loading(DocumentId::new(11), "bom.txt", generation);
            assert!(reloaded.complete_loading(generation, decode_bytes(&bytes)));
            assert_eq!(reloaded.text(), text);
            assert_eq!(reloaded.bytes_for_save().unwrap(), bytes);
        }
    }
}

#[test]
fn editor_text_keeps_leading_feff_in_utf8_saves_and_rejects_unmappable_legacy_saves() {
    let text = "\u{feff}hello";
    let mut document = Document::from_path(DocumentId::new(12), "pasted.txt", text);
    assert_eq!(document.text(), text);
    assert_eq!(document.bytes_for_save().unwrap(), text.as_bytes());
    assert_eq!(
        encode_text(text, TextEncoding::Utf8).unwrap(),
        text.as_bytes()
    );

    for encoding in [
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Oem437,
    ] {
        document.set_encoding(encoding);
        assert_eq!(
            document.bytes_for_save(),
            Err(fragile_notepad::core::EncodingError::UnmappableCharacters)
        );
    }
}

#[test]
fn decoded_utf16le_bom_document_saves_back_as_utf16le() {
    let decoded = decode_bytes(&[0xff, 0xfe, b'h', 0, b'i', 0]);
    let document = Document::from_decoded(DocumentId::new(10), fixture_path("note.txt"), decoded);

    assert_eq!(document.encoding, TextEncoding::Utf16LeBom);
    assert_eq!(document.text(), "hi");
    assert_eq!(
        document.bytes_for_save().expect("encoded bytes"),
        vec![0xff, 0xfe, b'h', 0, b'i', 0]
    );
}

#[test]
fn loading_document_text_index_gates_full_document_analysis_until_completion() {
    let generation = fragile_notepad::core::DocumentLoadGeneration::next();
    let mut document = Document::loading(DocumentId::new(15), fixture_path("large.rs"), generation);

    assert!(!document.has_complete_text_index());
    assert!(!document.can_run_full_document_analysis());
    assert!(document.replace_loading_preview(generation, "fn partial() {\n", false, 15, None));
    assert!(!document.has_complete_text_index());
    assert!(document.folds.ranges().is_empty());

    assert!(document.complete_loading(
        generation,
        decode_bytes(b"fn complete() {\n    run();\n}\n")
    ));

    assert!(document.has_complete_text_index());
    assert!(document.can_run_full_document_analysis());
    assert!(
        document
            .folds
            .ranges()
            .contains(&fragile_notepad::editor::FoldRange::new(0, 2))
    );
}

#[test]
fn large_document_keeps_text_index_but_skips_unbounded_semantic_and_rich_text_work() {
    let text = format!(
        "fn oversized() {{\n{}\n}}\n",
        "x".repeat(MAX_FULL_DOCUMENT_ANALYSIS_BYTES)
    );
    let document = Document::from_path(DocumentId::new(18), fixture_path("oversized.rs"), &text);

    assert!(document.has_complete_text_index());
    assert!(!document.can_run_full_document_analysis());
    assert_eq!(document.syntax_token, "rs");
    assert_eq!(document.render_syntax_token(), "txt");
    assert!(document.folds.ranges().is_empty());
    assert!(document.decorations.indent_guides.is_empty());
}

#[test]
fn utf8_chunked_save_preserves_bom_crlf_and_trailing_empty_line() {
    let mut document = Document::from_decoded(
        DocumentId::new(16),
        fixture_path("large.txt"),
        decode_bytes(b"\xef\xbb\xbfalpha\r\nbeta\r\n"),
    );
    document.buffer.append_text("caf\u{00e9}");

    assert_eq!(document.encoding, TextEncoding::Utf8Bom);
    assert_eq!(document.line_ending, Some(LineEnding::CrLf));
    assert_eq!(document.buffer.line(2).as_deref(), Some("caf\u{00e9}"));
    assert_eq!(
        document.bytes_for_save().expect("encoded bytes"),
        b"\xef\xbb\xbfalpha\r\nbeta\r\ncaf\xc3\xa9\r\n"
    );
}

#[test]
fn stale_generation_cannot_finish_or_fail_loading_document() {
    let generation = fragile_notepad::core::DocumentLoadGeneration::next();
    let stale_generation = fragile_notepad::core::DocumentLoadGeneration::next();
    let mut document =
        Document::loading(DocumentId::new(17), fixture_path("large.txt"), generation);

    assert!(!document.complete_loading(stale_generation, decode_bytes(b"stale")));
    assert_eq!(document.text(), "");
    assert!(document.is_loading_or_indexing());
    assert!(!document.fail_loading(stale_generation));
    assert!(document.is_loading_or_indexing());
    assert_eq!(document.load_generation(), Some(generation));
}

#[test]
fn windows_1252_round_trip_preserves_representable_text() {
    let bytes = encode_text("cafe\u{00e9}", TextEncoding::Windows1252).expect("encode");
    let decoded = decode_bytes(&bytes);

    assert_eq!(bytes, b"cafe\xe9");
    assert_eq!(decoded.encoding, TextEncoding::Windows1252);
    assert_eq!(decoded.text, "cafe\u{00e9}");
}

#[test]
fn iso_8859_1_saves_latin1_bytes_and_rejects_unmappable_text() {
    assert_eq!(
        encode_text("cafe\u{00e9}", TextEncoding::Iso8859_1).expect("encode"),
        b"cafe\xe9"
    );

    assert!(encode_text("\u{20ac}", TextEncoding::Iso8859_1).is_err());
}

#[test]
fn iso_8859_9_is_available_for_turkish_menu_entry() {
    assert_eq!(TextEncoding::Iso8859_9.label(), "ISO 8859-9");
    assert!(encode_text("Istanbul", TextEncoding::Iso8859_9).is_ok());
}

#[test]
fn oem_encoding_routes_through_vendored_encoding_rs_extension() {
    assert_eq!(TextEncoding::Oem437.label(), "OEM-US");
    assert_eq!(
        encode_text("cafe\u{00e9}", TextEncoding::Oem437).expect("encode"),
        b"cafe\x82"
    );
    assert_eq!(
        encode_text("\u{20ac}", TextEncoding::Oem858).expect("encode"),
        b"\xd5"
    );
    assert!(encode_text("\u{20ac}", TextEncoding::Oem437).is_err());
}

#[test]
fn set_decoration_settings_refreshes_document_view_models() {
    let mut document = Document::from_path(
        DocumentId::new(7),
        fixture_path("note.txt"),
        "root\n    child\nnext",
    );
    let range = fragile_notepad::editor::FoldRange::new(0, 1);

    assert!(document.folds.set_collapsed(range, true));
    document.refresh_view_models();
    assert_eq!(document.viewport.visible_row_count(), 2);

    document.set_decoration_settings(fragile_notepad::editor::DecorationSettings {
        show_line_numbers: false,
        show_folding_controls: false,
        ..fragile_notepad::editor::DecorationSettings::default()
    });

    assert_eq!(document.viewport.visible_row_count(), 2);
    assert!(
        document
            .decorations
            .line_decorations
            .iter()
            .all(|line| line.line_number.is_none() && !line.has_fold_control)
    );
}

#[test]
fn reveal_line_scrolls_target_near_viewport_with_context() {
    let text = (0..120)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut document = Document::from_path(DocumentId::new(12), fixture_path("long.txt"), &text);

    document.reveal_line(90);

    assert_eq!(document.scroll.first_visible_row, 87);

    document.reveal_line_with_context(3, 10);

    assert_eq!(document.scroll.first_visible_row, 0);
}
