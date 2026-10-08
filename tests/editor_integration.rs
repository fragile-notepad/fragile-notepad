use fragile_notepad::app::App;
use fragile_notepad::core::document::analyze_document;
use fragile_notepad::core::{
    DecodedText, Document, DocumentId, ShortcutCommand, ShortcutGroup, ShortcutMap, TextEncoding,
};
use fragile_notepad::editor::outline::{OutlineParseRequest, OutlineTree};
use fragile_notepad::editor::{
    CaretMotion, DecorationSettings, EditTransaction, EditorAction, EditorPosition, EditorRange,
    EditorSelection, FoldDelimiter, FoldRange, FunctionEntry, FunctionKind, OutlineParseResult,
    outline_registry_hash, parse_outline_snapshot, position_after_text,
};
use fragile_notepad::message::{Menu, Message};
use fragile_notepad::services::types::OpenedFile;
use iced::widget::text_editor::LineEnding;
use std::sync::Arc;

fn position(line: usize, column: usize) -> EditorPosition {
    EditorPosition::new(line, column)
}

fn selection(anchor: EditorPosition, cursor: EditorPosition) -> EditorSelection {
    EditorSelection::new(anchor, cursor)
}

fn caret(line: usize, column: usize) -> EditorSelection {
    selection(position(line, column), position(line, column))
}

fn opened_file(path: &str, text: &str) -> OpenedFile {
    OpenedFile {
        disk_revision: fragile_notepad::core::FileRevision::from_bytes(text.as_bytes()),
        path: path.into(),
        contents: Arc::new(DecodedText {
            text: text.to_owned(),
            encoding: TextEncoding::Utf8,
            had_errors: false,
        }),
    }
}

fn active_document_debug(app: &App) -> String {
    let debug = format!("{app:#?}");
    let active_id = debug
        .find("active_document_id:")
        .expect("debug state should include the active document id");

    debug[..active_id]
        .rsplit_once("Document {")
        .map(|(_, document)| format!("Document {{{document}"))
        .expect("debug state should include the active document")
}

fn assert_debug_contains(debug: &str, expected: &str) {
    let compact_debug = debug
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    let compact_expected = expected
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();

    assert!(
        compact_debug.contains(&compact_expected),
        "expected debug output to contain:\n{expected}\n\nactual debug output:\n{debug}"
    );
}

fn replace_selection(document: &mut Document, replacement: &str) {
    let before_selection = document.selection;
    let range = before_selection.range();

    let delta = document.buffer.replace_range(range, replacement);
    if let Some(line_ending) = fragile_notepad::core::document::detect_line_ending(replacement) {
        document.line_ending = Some(line_ending);
    }

    let cursor = document
        .buffer
        .clamp_position(position_after_text(range.start, replacement));
    document.selection = selection(cursor, cursor);

    if delta.before_text != delta.after_text {
        document.history.record(EditTransaction {
            delta,
            before_selection,
            after_selection: document.selection,
        });
        document.refresh_after_text_change();
    }
}

#[test]
fn document_editor_flow_handles_insert_newline_selection_replacement_and_history() {
    let mut document = Document::untitled(DocumentId::new(1));

    document.selection = caret(0, 0);
    replace_selection(&mut document, "hello");
    replace_selection(&mut document, "\n");
    replace_selection(&mut document, "world");

    assert_eq!(document.text(), "hello\nworld");
    assert_eq!(document.line_ending, Some(LineEnding::Lf));
    assert_eq!(document.selection, caret(1, 5));
    assert!(document.is_dirty);

    document.selection = selection(position(0, 1), position(0, 5));
    replace_selection(&mut document, "i");

    assert_eq!(document.text(), "hi\nworld");
    assert_eq!(document.selection, caret(0, 2));

    document.mark_clean();
    assert!(!document.is_dirty);

    document.selection = selection(position(0, 2), position(1, 0));
    replace_selection(&mut document, "");
    assert_eq!(document.text(), "hiworld");
    assert!(document.is_dirty);

    assert!(document.undo());
    assert_eq!(document.text(), "hi\nworld");
    assert_eq!(
        document.selection,
        selection(position(0, 2), position(1, 0))
    );
    assert!(!document.is_dirty);

    assert!(document.redo());
    assert_eq!(document.text(), "hiworld");
    assert_eq!(document.selection, caret(0, 2));
    assert!(document.is_dirty);

    document.selection = selection(position(0, 2), position(0, 3));
    replace_selection(&mut document, "");
    assert_eq!(document.text(), "hiorld");
}

#[test]
fn background_fold_analysis_refreshes_delimiters_without_expanding_matching_folds() {
    for wrapped in [false, true] {
        let mut document = Document::from_path(
            DocumentId::new(1),
            "main.rs",
            "fn main() {                          \t\n    run();\n}",
        );
        document.update_viewport_geometry(4, 66.0, 8.0);
        document.set_word_wrap(wrapped);
        let range = FoldRange::new(0, 2);
        assert_eq!(document.folds.delimiter(range).unwrap().opening_column, 10);
        document.restore_collapsed_folds(&[(0, 2)]);
        document.defer_analysis = true;
        document
            .buffer
            .replace_range(EditorRange::new(position(0, 0), position(0, 0)), "pub ");
        document.refresh_after_text_change();

        let (buffer, request) = document.analysis_request().unwrap();
        let result = analyze_document(buffer, request);
        let expected = FoldDelimiter {
            opening_column: 14,
            opening: '{',
            closing_column: 1,
        };
        assert_eq!(result.folds.delimiter(range), Some(expected));
        assert!(document.apply_analysis(result));
        assert_eq!(document.folds.delimiter(range), Some(expected));
        assert_eq!(
            document.decorations.line_decorations[0].fold_delimiter,
            Some(expected)
        );
        assert!(document.folds.is_collapsed(range));
        assert_eq!(document.viewport.document_line_to_visible_row(1), None);
        if wrapped {
            let opener_row = document
                .viewport
                .position_to_visible_row(position(0, expected.opening_column))
                .unwrap();
            let end_row = document
                .viewport
                .position_to_visible_row(position(0, document.buffer.line(0).unwrap().len()))
                .unwrap();
            assert_eq!(opener_row, end_row);
            assert!(
                document
                    .viewport
                    .row_segment(end_row, &document.buffer)
                    .unwrap()
                    .is_last
            );
        }
    }
}

#[test]
fn collapsed_fold_hides_children_without_changing_saved_text() {
    let original = "fn main() {\n    let value = 1;\n}\nnext();\n";
    let mut document = Document::from_path(DocumentId::new(2), "main.rs", original);
    let outer = FoldRange::new(0, 2);

    assert!(document.folds.ranges().contains(&outer));
    assert!(document.folds.set_collapsed(outer, true));
    document.refresh_view_models();

    assert_eq!(document.viewport.visible_row_to_document_line(0), Some(0));
    assert_eq!(document.viewport.visible_row_to_document_line(1), Some(3));
    assert_eq!(document.viewport.document_line_to_visible_row(1), None);
    assert_eq!(document.viewport.document_line_to_visible_row(2), None);
    assert_eq!(document.text(), original);
    assert_eq!(document.text_for_save(), original);
    assert_eq!(document.decorations.hidden_line_spans.len(), 1);
    assert_eq!(
        document.decorations.hidden_line_spans[0].hidden_line_count(),
        2
    );
}

#[test]
fn fold_current_command_collapses_fold_from_header() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn main() {\nlet value = 1;\n}\nnext();",
    ))));
    let _ = app.update(Message::MenuToggled(Menu::View));
    let _ = app.update(Message::FoldCurrent);

    assert_debug_contains(
        &format!("{app:#?}"),
        "menu: MenuState { active: None, path: [], }",
    );
    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "visible_lines: [\n                0,\n                3,\n            ]",
    );
    assert_debug_contains(
        &debug,
        "document_to_visible: [\n                Some(\n                    0,\n                ),\n                None,\n                None,\n                Some(\n                    1,\n                ),\n            ]",
    );
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {\n            anchor: EditorPosition {\n                line: 0,\n                column: 0,\n            },\n            cursor: EditorPosition {\n                line: 0,\n                column: 0,\n            },\n        }",
    );
}

#[test]
fn fold_current_command_collapses_parent_fold_from_child() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn main() {\nlet value = 1;\n}\nnext();",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(1, 4)),
    ));
    let _ = app.update(Message::FoldCurrent);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "visible_lines: [\n                0,\n                3,\n            ]",
    );
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {\n            anchor: EditorPosition {\n                line: 0,\n                column: 0,\n            },\n            cursor: EditorPosition {\n                line: 0,\n                column: 0,\n            },\n        }",
    );
}

#[test]
fn fold_all_and_unfold_all_commands_update_every_fold() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn main() {\n    if ready {\n        run();\n    }\n}\nnext();",
    ))));
    let _ = app.update(Message::FoldAll);

    let folded = active_document_debug(&app);
    assert_debug_contains(&folded, "start_line: 0,\n                    end_line: 4");
    assert_debug_contains(&folded, "start_line: 1,\n                    end_line: 3");
    assert_debug_contains(
        &folded,
        "visible_lines: [\n                0,\n                5,\n            ]",
    );

    let _ = app.update(Message::UnfoldAll);

    let unfolded = active_document_debug(&app);
    assert_debug_contains(&unfolded, "collapsed: {},");
    assert_debug_contains(
        &unfolded,
        "visible_lines: [\n                0,\n                1,\n                2,\n                3,\n                4,\n                5,\n            ]",
    );
}

#[test]
fn fold_all_command_clamps_hidden_selection_to_collapsed_header() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn main() {\n    if ready {\n        run();\n    }\n}\nnext();",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(2, 8), position(3, 5))),
    ));
    let _ = app.update(Message::FoldAll);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {\n            anchor: EditorPosition {\n                line: 0,\n                column: 0,\n            },\n            cursor: EditorPosition {\n                line: 0,\n                column: 0,\n            },\n        }",
    );
    assert_debug_contains(
        &debug,
        "document_to_visible: [\n                Some(\n                    0,\n                ),\n                None,\n                None,\n                None,\n                None,\n                Some(\n                    1,\n                ),\n            ]",
    );
}

#[test]
fn go_to_matching_delimiter_prefers_delimiter_before_caret() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn main() { call(value); }",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 10)),
    ));
    let _ = app.update(Message::GoToMatchingDelimiter);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {\n            anchor: EditorPosition {\n                line: 0,\n                column: 25,\n            },\n            cursor: EditorPosition {\n                line: 0,\n                column: 25,\n            },\n        }",
    );
}

#[test]
fn go_to_matching_delimiter_uses_delimiter_at_caret_when_previous_is_not_delimiter() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "let value = [one, [two]];",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 12)),
    ));
    let _ = app.update(Message::GoToMatchingDelimiter);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {\n            anchor: EditorPosition {\n                line: 0,\n                column: 23,\n            },\n            cursor: EditorPosition {\n                line: 0,\n                column: 23,\n            },\n        }",
    );
}

#[test]
fn go_to_matching_delimiter_reveals_target_line() {
    let (mut app, _) = App::new();
    let body = (0..40)
        .map(|line| format!("line{line};"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!("fn main() {{\n{body}\n}}\n");

    let _ = app.update(Message::FileOpened(Ok(opened_file("main.rs", &text))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, "fn main() {".len())),
    ));
    let _ = app.update(Message::GoToMatchingDelimiter);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 41,
                column: 0,
            },
            cursor: EditorPosition {
                line: 41,
                column: 0,
            },
        }",
    );
    assert_debug_contains(
        &debug,
        "scroll: ScrollOffset {\n            first_visible_row: 38,",
    );
}

#[test]
fn select_matching_delimiter_includes_both_delimiters_and_preserves_utf8_columns() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "let text = \u{597d}({x});",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 15)),
    ));
    let _ = app.update(Message::SelectMatchingDelimiter);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {\n            anchor: EditorPosition {\n                line: 0,\n                column: 14,\n            },\n            cursor: EditorPosition {\n                line: 0,\n                column: 19,\n            },\n        }",
    );
}

#[test]
fn select_matching_delimiter_in_place_preserves_scroll() {
    let (mut app, _) = App::new();
    let body = (0..40)
        .map(|line| format!("line{line};"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!("fn main() {{\n{body}\n}}\n");

    let _ = app.update(Message::FileOpened(Ok(opened_file("main.rs", &text))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, "fn main() {".len())),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectMatchingDelimiterInPlace,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 10,
            },
            cursor: EditorPosition {
                line: 41,
                column: 1,
            },
        }",
    );
    assert_debug_contains(
        &debug,
        "scroll: ScrollOffset {\n            first_visible_row: 0,",
    );
}

#[test]
fn unmatched_delimiter_commands_leave_selection_unchanged() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn main() {",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 3), position(0, 11))),
    ));
    let _ = app.update(Message::GoToMatchingDelimiter);
    let _ = app.update(Message::SelectMatchingDelimiter);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {\n            anchor: EditorPosition {\n                line: 0,\n                column: 3,\n            },\n            cursor: EditorPosition {\n                line: 0,\n                column: 11,\n            },\n        }",
    );
}

#[test]
fn function_navigation_commands_move_between_outline_entries() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn first() {\n}\n\nfn second() {\n    call();\n}\n\nfn third() {}\n",
    ))));
    let _ = app.update(Message::NextFunction);

    let after_next = active_document_debug(&app);
    assert_debug_contains(
        &after_next,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 3,
                column: 0,
            },
            cursor: EditorPosition {
                line: 3,
                column: 0,
            },
        }",
    );

    let _ = app.update(Message::PreviousFunction);

    let after_previous = active_document_debug(&app);
    assert_debug_contains(
        &after_previous,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 0,
            },
            cursor: EditorPosition {
                line: 0,
                column: 0,
            },
        }",
    );
    assert_debug_contains(&after_previous, "is_dirty: false");
}

#[test]
fn background_outline_results_are_cached_and_stale_results_are_rejected() {
    let (mut app, _) = App::new();
    let document_id = DocumentId::new(2);
    let registry_hash = outline_registry_hash();
    let text = "fn first() {}\nfn second() {}\n";

    let _ = app.update(Message::FileOpened(Ok(opened_file("main.rs", text))));

    let stale = OutlineParseResult::new(
        document_id,
        99,
        "rs",
        registry_hash,
        OutlineTree::default(),
        vec![FunctionEntry {
            name: "stale".to_owned(),
            kind: FunctionKind::Function,
            range: EditorRange::new(position(0, 0), position(0, 13)),
            body_range: None,
            depth: 0,
        }],
        Vec::new(),
    );
    let _ = app.update(Message::OutlineParseCompleted(stale));

    let after_stale = format!("{app:#?}");
    assert_debug_contains(&after_stale, "status: Pending");
    assert!(!after_stale.contains("name: \"stale\""));

    let current = parse_outline_snapshot(OutlineParseRequest::new(
        document_id,
        Arc::new(text.to_owned()),
        "rs",
        0,
        registry_hash,
    ));
    let _ = app.update(Message::OutlineParseCompleted(current));

    let after_current = format!("{app:#?}");
    assert_debug_contains(&after_current, "status: Ready");
    assert_debug_contains(&after_current, "name: \"first\"");
    assert_debug_contains(&after_current, "name: \"second\"");

    let _ = app.update(Message::NextFunction);

    assert_debug_contains(
        &active_document_debug(&app),
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 0,
            },
            cursor: EditorPosition {
                line: 1,
                column: 0,
            },
        }",
    );
}

#[test]
fn toggle_function_list_flips_visibility_and_closes_menu() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::MenuToggled(Menu::View));
    let before_toggle = format!("{app:#?}");
    assert_debug_contains(
        &before_toggle,
        "menu: MenuState { active: Some(View,), path: [], }",
    );
    assert_debug_contains(&before_toggle, "is_function_list_visible: false");

    let _ = app.update(Message::ToggleFunctionList);

    let after_open = format!("{app:#?}");
    assert_debug_contains(&after_open, "menu: MenuState { active: None, path: [], }");
    assert_debug_contains(&after_open, "is_function_list_visible: true");

    let _ = app.update(Message::MenuToggled(Menu::View));
    let _ = app.update(Message::ToggleFunctionList);

    let after_close = format!("{app:#?}");
    assert_debug_contains(&after_close, "menu: MenuState { active: None, path: [], }");
    assert_debug_contains(&after_close, "is_function_list_visible: false");
}

#[test]
fn function_list_entry_selection_moves_caret_reveals_line_and_preserves_clean_state() {
    let (mut app, _) = App::new();
    let text = (0..60)
        .map(|line| {
            if line == 45 {
                "    fn target() {}".to_owned()
            } else {
                format!("fn helper_{line}() {{}}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let _ = app.update(Message::FileOpened(Ok(opened_file("main.rs", &text))));
    let _ = app.update(Message::FunctionListEntrySelected(position(45, 4)));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 45,
                column: 4,
            },
            cursor: EditorPosition {
                line: 45,
                column: 4,
            },
        }",
    );
    assert_debug_contains(
        &debug,
        "scroll: ScrollOffset {\n            first_visible_row: 42,",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn function_navigation_shortcut_routes_to_active_editor() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn first() {}\nfn second() {}\n",
    ))));
    let _ = app.update(Message::Shortcut(ShortcutCommand::NextFunction));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 0,
            },
            cursor: EditorPosition {
                line: 1,
                column: 0,
            },
        }",
    );
}

#[test]
fn select_current_function_uses_deepest_outline_entry() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn outer() {\n    fn inner() {\n        call();\n    }\n}\n",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(2, 8)),
    ));
    let _ = app.update(Message::SelectCurrentFunction);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 4,
            },
            cursor: EditorPosition {
                line: 3,
                column: 5,
            },
        }",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn select_current_function_body_selects_body_range() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "fn outer() {\n    fn inner() {\n        call();\n    }\n}\n",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(2, 8)),
    ));
    let _ = app.update(Message::SelectCurrentFunctionBody);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 15,
            },
            cursor: EditorPosition {
                line: 3,
                column: 5,
            },
        }",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn select_current_python_function_body_selects_indent_body_range() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "script.py",
        "def outer():\n    def inner():\n        call()\n    done()\n",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(2, 8)),
    ));
    let _ = app.update(Message::SelectCurrentFunctionBody);

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 16,
            },
            cursor: EditorPosition {
                line: 2,
                column: 14,
            },
        }",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn function_commands_noop_for_unsupported_syntax() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "fn first() {}\nfn second() {}\n",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 3), position(0, 8))),
    ));
    let _ = app.update(Message::NextFunction);
    let _ = app.update(Message::PreviousFunction);
    let _ = app.update(Message::SelectCurrentFunction);
    let _ = app.update(Message::SelectCurrentFunctionBody);

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"fn first() {}\\nfn second() {}\\n\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 3,
            },
            cursor: EditorPosition {
                line: 0,
                column: 8,
            },
        }",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn decoration_settings_refresh_line_numbers_whitespace_guides_and_folding_controls() {
    let mut document = Document::from_path(
        DocumentId::new(3),
        "notes.txt",
        "root\n        child\n\t\tgrandchild\nnext",
    );
    let settings = DecorationSettings {
        show_line_numbers: false,
        show_spaces: true,
        show_tabs: true,
        show_end_of_line_markers: true,
        show_indentation_guides: false,
        show_folding_controls: false,
        ..DecorationSettings::default()
    };

    document.set_decoration_settings(settings);

    assert_eq!(document.decorations.settings, settings);
    assert!(
        document
            .decorations
            .line_decorations
            .iter()
            .all(|line| { line.line_number.is_none() && !line.has_fold_control })
    );
    assert!(!document.decorations.indent_guides.is_empty());
    assert_eq!(document.decorations.indent_guides[0].line, 1);
    assert_eq!(document.decorations.indent_guides[0].depth, 1);
}

#[test]
fn function_shortcut_catalog_entries_are_search_commands_without_defaults() {
    let shortcuts = ShortcutMap::default();

    for command in [
        ShortcutCommand::NextFunction,
        ShortcutCommand::PreviousFunction,
        ShortcutCommand::SelectCurrentFunction,
        ShortcutCommand::SelectCurrentFunctionBody,
    ] {
        assert_eq!(command.group(), ShortcutGroup::Search);
        assert_eq!(ShortcutCommand::from_key(command.key()), Some(command));
        assert_eq!(shortcuts.binding_display(command), "Unassigned");
    }
}

#[test]
fn word_motion_collapses_to_ascii_word_boundaries() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "alpha, beta_2 \u{597d} gamma",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, "alpha, beta_2 \u{597d} g".len())),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::WordLeft),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::WordLeft),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::WordRight),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 18,
            },
            cursor: EditorPosition {
                line: 0,
                column: 18,
            },
        }",
    );
}

#[test]
fn word_selection_preserves_anchor_and_uses_utf8_byte_columns() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "\u{597d} alpha, beta",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, "\u{597d} alpha".len())),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::Select(CaretMotion::WordRight),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::Select(CaretMotion::WordLeft),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 9,
            },
            cursor: EditorPosition {
                line: 0,
                column: 4,
            },
        }",
    );
}

#[test]
fn select_word_at_selects_whole_word_without_dirtying_document() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "alpha, beta_2 \u{597d}",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectWordAt(position(0, "alpha, be".len())),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 7,
            },
            cursor: EditorPosition {
                line: 0,
                column: 13,
            },
        }",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn select_word_at_uses_syntax_registry_word_characters() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "script.php",
        "$value = 1",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectWordAt(position(0, 0)),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 0,
            },
            cursor: EditorPosition {
                line: 0,
                column: 6,
            },
        }",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn select_word_at_falls_back_to_matching_delimiter_selection() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "main.rs",
        "call(value);",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectWordAt(position(0, "call".len())),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 4,
            },
            cursor: EditorPosition {
                line: 0,
                column: 11,
            },
        }",
    );
    assert_debug_contains(&debug, "is_dirty: false");
}

#[test]
fn paragraph_motion_moves_between_nonblank_paragraph_boundaries() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "first\nstill first\n   \nsecond\nstill second\n\nthird",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(4, 6)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::ParagraphUp),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::ParagraphDown),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 6,
                column: 0,
            },
            cursor: EditorPosition {
                line: 6,
                column: 0,
            },
        }",
    );
}

#[test]
fn paragraph_down_from_final_paragraph_moves_to_document_end() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "first\n\nfinal \u{597d}",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(2, 2)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::ParagraphDown),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 2,
                column: 9,
            },
            cursor: EditorPosition {
                line: 2,
                column: 9,
            },
        }",
    );
}

#[test]
fn vertical_motion_remembers_column_across_shorter_lines() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "0123456789\nabc\n0123456789",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 8)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::Down),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::Down),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 2,
                column: 8,
            },
            cursor: EditorPosition {
                line: 2,
                column: 8,
            },
        }",
    );
}

#[test]
fn vertical_selection_remembers_column_across_shorter_lines() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "0123456789\nabc\n0123456789",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 8)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::Select(CaretMotion::Down),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::Select(CaretMotion::Down),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 8,
            },
            cursor: EditorPosition {
                line: 2,
                column: 8,
            },
        }",
    );
}

#[test]
fn non_vertical_motion_resets_remembered_vertical_column() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "0123456789\nabc\n0123456789",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 8)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::Down),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::Left),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::MoveCaret(CaretMotion::Down),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 2,
                column: 2,
            },
            cursor: EditorPosition {
                line: 2,
                column: 2,
            },
        }",
    );
}

#[test]
fn paragraph_selection_preserves_anchor() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "first\nstill first\n\nsecond\nstill second\n\nthird",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(3, 2)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::Select(CaretMotion::ParagraphDown),
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 3,
                column: 2,
            },
            cursor: EditorPosition {
                line: 6,
                column: 0,
            },
        }",
    );
}

#[test]
fn multi_selection_shortcuts_add_adjacent_carets() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(1, 1)),
    ));
    let _ = app.update(Message::Shortcut(ShortcutCommand::AddCaretAbove));
    let _ = app.update(Message::Shortcut(ShortcutCommand::AddCaretBelow));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "selection_set: SelectionSet");
    assert_debug_contains(
        &debug,
        "line: 0,\n                                column: 1",
    );
    assert_debug_contains(
        &debug,
        "line: 1,\n                                column: 1",
    );
    assert_debug_contains(
        &debug,
        "line: 2,\n                                column: 1",
    );
    assert_debug_contains(&debug, "main: 1");
}

#[test]
fn split_selection_into_lines_creates_linewise_selection_set() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 1), position(2, 2))),
    ));
    let _ = app.update(Message::Shortcut(ShortcutCommand::SplitSelectionIntoLines));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "selection_set: SelectionSet");
    assert_debug_contains(
        &debug,
        "line: 0,\n                                column: 1",
    );
    assert_debug_contains(
        &debug,
        "line: 0,\n                                column: 3",
    );
    assert_debug_contains(
        &debug,
        "line: 1,\n                                column: 0",
    );
    assert_debug_contains(
        &debug,
        "line: 1,\n                                column: 3",
    );
    assert_debug_contains(
        &debug,
        "line: 2,\n                                column: 0",
    );
    assert_debug_contains(
        &debug,
        "line: 2,\n                                column: 2",
    );
    assert_debug_contains(&debug, "main: 0");
}

#[test]
fn convert_selection_to_rectangle_preserves_visual_column_shape() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "a\tz\nabcdef",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 1), position(1, 4))),
    ));
    let _ = app.update(Message::Shortcut(
        ShortcutCommand::ConvertSelectionToRectangle,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "shape: Rectangular");
    assert_debug_contains(&debug, "anchor_visual_column: 1");
    assert_debug_contains(&debug, "cursor_visual_column: 4");
}

#[test]
fn duplicate_line_at_caret_inserts_copy_below_current_line() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(1, 1)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::DuplicateLine,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one\\ntwo\\ntwo\\nthree\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 2,
                column: 0,
            },
            cursor: EditorPosition {
                line: 3,
                column: 0,
            },
        }",
    );
}

#[test]
fn duplicate_final_line_without_line_ending_keeps_lines_separate() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(1, 1)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::DuplicateLine,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one\\ntwo\\ntwo\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 2,
                column: 0,
            },
            cursor: EditorPosition {
                line: 2,
                column: 3,
            },
        }",
    );
}

#[test]
fn duplicate_selected_touched_lines_is_one_undoable_edit() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree\nfour",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(1, 2), position(2, 1))),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::DuplicateLine,
    ));

    let duplicated = active_document_debug(&app);
    assert_debug_contains(
        &duplicated,
        "text: \"one\\ntwo\\nthree\\ntwo\\nthree\\nfour\"",
    );

    let _ = app.update(Message::Undo);

    let undone = active_document_debug(&app);
    assert_debug_contains(&undone, "text: \"one\\ntwo\\nthree\\nfour\"");
    assert_debug_contains(
        &undone,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 2,
            },
            cursor: EditorPosition {
                line: 2,
                column: 1,
            },
        }",
    );
}

#[test]
fn duplicate_line_selection_ending_at_next_line_start_duplicates_only_first_line() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 0), position(1, 0))),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::DuplicateLine,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one\\none\\ntwo\\nthree\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 0,
            },
            cursor: EditorPosition {
                line: 2,
                column: 0,
            },
        }",
    );
}

#[test]
fn delete_line_removes_all_selected_touched_lines() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree\nfour",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(1, 2), position(2, 1))),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::DeleteLine,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one\\nfour\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 0,
            },
            cursor: EditorPosition {
                line: 1,
                column: 0,
            },
        }",
    );
}

#[test]
fn delete_line_selection_ending_at_next_line_start_deletes_only_first_line() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 0), position(1, 0))),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::DeleteLine,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"two\\nthree\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 0,
            },
            cursor: EditorPosition {
                line: 0,
                column: 0,
            },
        }",
    );
}

#[test]
fn cut_line_with_selection_deletes_touched_lines_and_undo_restores_selection() {
    let (mut app, _) = App::new();
    let original_selection = selection(position(0, 1), position(1, 2));

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(original_selection),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::CutLine,
    ));

    assert_debug_contains(&active_document_debug(&app), "text: \"three\"");

    let _ = app.update(Message::Undo);

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one\\ntwo\\nthree\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 1,
            },
            cursor: EditorPosition {
                line: 1,
                column: 2,
            },
        }",
    );
}

#[test]
fn uppercase_selection_transforms_selected_text_and_undo_restores() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one Two\nmix",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 4), position(1, 3))),
    ));
    let _ = app.update(Message::Uppercase);

    assert_debug_contains(&active_document_debug(&app), "text: \"one TWO\\nMIX\"");

    let _ = app.update(Message::Undo);

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one Two\\nmix\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 4,
            },
            cursor: EditorPosition {
                line: 1,
                column: 3,
            },
        }",
    );
}

#[test]
fn lowercase_without_selection_is_no_op() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "LOUD\nText",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 2)),
    ));
    let _ = app.update(Message::Lowercase);

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"LOUD\\nText\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 2,
            },
            cursor: EditorPosition {
                line: 0,
                column: 2,
            },
        }",
    );
}

#[test]
fn trim_trailing_spaces_updates_selected_lines_and_undo_restores() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one   \ntwo\t \nthree  ",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::SelectRegion(selection(position(0, 1), position(1, 2))),
    ));
    let _ = app.update(Message::TrimTrailingSpaces);

    assert_debug_contains(
        &active_document_debug(&app),
        "text: \"one\\ntwo\\nthree  \"",
    );

    let _ = app.update(Message::Undo);

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one   \\ntwo\\t \\nthree  \"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 1,
            },
            cursor: EditorPosition {
                line: 1,
                column: 2,
            },
        }",
    );
}

#[test]
fn join_lines_with_caret_joins_current_and_next_line_with_undo() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one   \n \ttwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(0, 1)),
    ));
    let _ = app.update(Message::JoinLines);

    assert_debug_contains(&active_document_debug(&app), "text: \"one two\\nthree\"");

    let _ = app.update(Message::Undo);

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one   \\n \\ttwo\\nthree\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 0,
                column: 1,
            },
            cursor: EditorPosition {
                line: 0,
                column: 1,
            },
        }",
    );
}

#[test]
fn cut_action_with_caret_cuts_current_full_line() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(1, 1)),
    ));
    let _ = app.update(Message::EditorAction(DocumentId::new(2), EditorAction::Cut));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one\\nthree\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 0,
            },
            cursor: EditorPosition {
                line: 1,
                column: 0,
            },
        }",
    );
}

#[test]
fn copy_line_and_caret_copy_do_not_mutate_document() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Ok(opened_file(
        "notes.txt",
        "one\ntwo\nthree",
    ))));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::PlaceCaret(position(1, 1)),
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::Copy,
    ));
    let _ = app.update(Message::EditorAction(
        DocumentId::new(2),
        EditorAction::CopyLine,
    ));

    let debug = active_document_debug(&app);
    assert_debug_contains(&debug, "text: \"one\\ntwo\\nthree\"");
    assert_debug_contains(
        &debug,
        "selection: EditorSelection {
            anchor: EditorPosition {
                line: 1,
                column: 1,
            },
            cursor: EditorPosition {
                line: 1,
                column: 1,
            },
        }",
    );
}
