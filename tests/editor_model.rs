use fragile_notepad::core::DocumentId;
use fragile_notepad::editor::outline::{OutlineNodeKind, OutlineParseRequest, OutlineRegistry};
use fragile_notepad::editor::{
    CaretMotion, DecorationModel, DecorationSettings, EditorBuffer, EditorPosition, EditorRange,
    EditorSelection, FoldModel, FoldProvider, FoldRange, FunctionKind, HiddenLineSpan,
    IndentBraceFoldProvider, IndentGuide, SelectionRange, SelectionSet, SelectionShape,
    ViewportModel, containing_function, document_end, line_end, move_position, next_function_after,
    outline_for_syntax, parse_outline_snapshot, previous_function_before, word_range_at_position,
};
use std::sync::Arc;

#[test]
fn unmatched_closing_delimiters_do_not_block_fold_scanning() {
    let buffer = EditorBuffer::from_text("}] )\n{\nbody\n}\n]");
    assert!(
        IndentBraceFoldProvider::default()
            .compute_folds(&buffer)
            .contains(&FoldRange::new(1, 3))
    );
}

#[test]
fn incremental_line_index_matches_rebuild_across_edits_and_newline_pairing() {
    let sources = [
        "",
        "a\nb\nc",
        "a\r\nb\r\nc",
        "a\n\rb",
        "a\r\n\r\n\r\nb",
        "é\r好\nlast",
        "\r\n\r\n",
        "a\n",
    ];
    let replacements = ["", "x", "\n", "\r", "\r\n", "\n\r", "é\n好", "\n\r\n\r"];
    for source in sources {
        let original = EditorBuffer::from_text(source);
        for replacement in replacements {
            let mut appended = original.clone();
            appended.append_text(replacement);
            let rebuilt = EditorBuffer::from_text(format!("{source}{replacement}"));
            assert_eq!(appended, rebuilt, "append {replacement:?} to {source:?}");
        }
        let positions = (0..=source.len())
            .filter_map(|offset| original.position_for_byte_offset(offset))
            .collect::<Vec<_>>();
        for (index, start) in positions.iter().enumerate() {
            for end in &positions[index..] {
                for replacement in replacements {
                    let mut actual = original.clone();
                    actual.replace_range(EditorRange::new(*start, *end), replacement);
                    let rebuilt = EditorBuffer::from_text(actual.text());
                    assert_eq!(
                        actual.line_count(),
                        rebuilt.line_count(),
                        "{source:?}, {start:?}..{end:?}, {replacement:?}"
                    );
                    for line in 0..actual.line_count() {
                        assert_eq!(actual.line_text(line), rebuilt.line_text(line));
                    }
                    for offset in 0..=actual.len_bytes() {
                        assert_eq!(
                            actual.position_for_byte_offset(offset),
                            rebuilt.position_for_byte_offset(offset)
                        );
                    }
                }
            }
        }
    }
}

struct OutlineSnippetCase {
    syntax_token: &'static str,
    source: &'static str,
    expected: &'static [(&'static str, FunctionKind, usize)],
}

const RUST_SNIPPET_EXPECTED: &[(&str, FunctionKind, usize)] = &[
    ("helper", FunctionKind::Function, 1),
    ("required", FunctionKind::Declaration, 2),
    ("provided", FunctionKind::Method, 2),
    ("load", FunctionKind::Method, 2),
    ("local", FunctionKind::Method, 3),
    ("top", FunctionKind::Function, 0),
];
const PYTHON_SNIPPET_EXPECTED: &[(&str, FunctionKind, usize)] = &[
    ("top", FunctionKind::Function, 0),
    ("nested", FunctionKind::Function, 1),
    ("load", FunctionKind::Method, 1),
    ("local", FunctionKind::Method, 2),
    ("save", FunctionKind::Method, 1),
];
const JAVASCRIPT_SNIPPET_EXPECTED: &[(&str, FunctionKind, usize)] = &[
    ("top", FunctionKind::Function, 0),
    ("nested", FunctionKind::Function, 1),
    ("load", FunctionKind::Method, 1),
    ("local", FunctionKind::Method, 2),
];
const JAVA_SNIPPET_EXPECTED: &[(&str, FunctionKind, usize)] = &[
    ("Service", FunctionKind::Method, 1),
    ("load", FunctionKind::Method, 1),
    ("run", FunctionKind::Method, 3),
    ("format", FunctionKind::Method, 1),
    ("save", FunctionKind::Declaration, 2),
    ("name", FunctionKind::Method, 2),
];
const KOTLIN_SNIPPET_EXPECTED: &[(&str, FunctionKind, usize)] = &[
    ("load", FunctionKind::Method, 1),
    ("local", FunctionKind::Method, 2),
    ("save", FunctionKind::Method, 1),
    ("register", FunctionKind::Method, 2),
    ("required", FunctionKind::Declaration, 1),
    ("provided", FunctionKind::Method, 1),
];
const C_FAMILY_SNIPPET_EXPECTED: &[(&str, FunctionKind, usize)] = &[
    ("top", FunctionKind::Function, 0),
    ("label", FunctionKind::Function, 0),
    ("Service", FunctionKind::Declaration, 2),
    ("~Service", FunctionKind::Declaration, 2),
    ("load", FunctionKind::Method, 2),
    ("ready", FunctionKind::Declaration, 2),
    ("count", FunctionKind::Declaration, 2),
    ("operator==", FunctionKind::Declaration, 2),
    ("operator()", FunctionKind::Method, 2),
    ("run", FunctionKind::Method, 3),
    ("Service", FunctionKind::Function, 1),
    ("~Service", FunctionKind::Function, 1),
    ("ready", FunctionKind::Function, 1),
    ("count", FunctionKind::Function, 1),
    ("operator==", FunctionKind::Function, 1),
];
const RUBY_SNIPPET_EXPECTED: &[(&str, FunctionKind, usize)] = &[
    ("load", FunctionKind::Method, 2),
    ("helper", FunctionKind::Method, 3),
    ("top", FunctionKind::Function, 0),
];

#[test]
fn editor_model_buffer_preserves_text_and_exposes_lines_without_endings() {
    let buffer = EditorBuffer::from_text("one\r\ntwo\nthree");

    assert_eq!(buffer.text(), "one\r\ntwo\nthree");
    assert_eq!(buffer.line_count(), 3);
    assert_eq!(buffer.line(0).as_deref(), Some("one"));
    assert_eq!(buffer.line(1).as_deref(), Some("two"));
    assert_eq!(buffer.line(2).as_deref(), Some("three"));
}

#[test]
fn editor_model_buffer_rejects_offsets_inside_paired_line_endings() {
    let crlf = EditorBuffer::from_text("one\r\ntwo");
    assert_eq!(
        crlf.position_for_byte_offset(3),
        Some(EditorPosition::new(0, 3))
    );
    assert_eq!(crlf.position_for_byte_offset(4), None);
    assert_eq!(
        crlf.position_for_byte_offset(5),
        Some(EditorPosition::new(1, 0))
    );

    let lfcr = EditorBuffer::from_text("one\n\rtwo");
    assert_eq!(lfcr.position_for_byte_offset(4), None);
}

#[test]
fn editor_model_buffer_splits_cr_only_lines() {
    let buffer = EditorBuffer::from_text("one\rtwo\rthree");

    assert_eq!(buffer.line_count(), 3);
    assert_eq!(buffer.line(0).as_deref(), Some("one"));
    assert_eq!(buffer.line(1).as_deref(), Some("two"));
    assert_eq!(buffer.line(2).as_deref(), Some("three"));
    assert_eq!(
        buffer.position_for_byte_offset("one\r".len()),
        Some(EditorPosition::new(1, 0))
    );
    assert_eq!(
        buffer.byte_offset(EditorPosition::new(2, 5)),
        "one\rtwo\rthree".len()
    );
}

#[test]
fn editor_model_buffer_splits_lfcr_lines() {
    let buffer = EditorBuffer::from_text("one\n\rtwo");

    assert_eq!(buffer.line_count(), 2);
    assert_eq!(buffer.line(0).as_deref(), Some("one"));
    assert_eq!(buffer.line(1).as_deref(), Some("two"));
    assert_eq!(
        buffer.position_for_byte_offset("one\n\r".len()),
        Some(EditorPosition::new(1, 0))
    );
}

#[test]
fn editor_model_buffer_replace_range_handles_multiline_edits_and_reports_delta() {
    let mut buffer = EditorBuffer::from_text("one\ntwo\nthree");
    let delta = buffer.replace_range(
        EditorRange::new(EditorPosition::new(0, 1), EditorPosition::new(1, 2)),
        "XX\nYY",
    );

    assert_eq!(buffer.text(), "oXX\nYYo\nthree");
    assert_eq!(delta.before_range.start, EditorPosition::new(0, 1));
    assert_eq!(delta.before_range.end, EditorPosition::new(1, 2));
    assert_eq!(delta.after_range.start, EditorPosition::new(0, 1));
    assert_eq!(delta.after_range.end, EditorPosition::new(1, 2));
    assert_eq!(delta.before_text, "ne\ntw");
    assert_eq!(delta.after_text, "XX\nYY");
}

#[test]
fn editor_model_buffer_replace_range_clamps_columns_to_utf8_char_boundaries() {
    let mut buffer = EditorBuffer::from_text("\u{00e9}x");
    let delta = buffer.replace_range(
        EditorRange::new(EditorPosition::new(0, 1), EditorPosition::new(0, 2)),
        "z",
    );

    assert_eq!(buffer.text(), "zx");
    assert_eq!(delta.before_range.start, EditorPosition::new(0, 0));
    assert_eq!(delta.before_range.end, EditorPosition::new(0, 2));
    assert_eq!(delta.before_text, "\u{00e9}");
}

#[test]
fn editor_model_buffer_clamps_ranges_and_rejects_out_of_bounds_lookups() {
    for source in ["", "é好", "é\r\n好\n\r", "\r\n"] {
        let mut buffer = EditorBuffer::from_text(source);
        let beyond_end = EditorPosition::new(usize::MAX, usize::MAX);
        let full_range = EditorRange::new(beyond_end, EditorPosition::new(0, 0));

        assert!(buffer.line_count() >= 1);
        assert_eq!(buffer.line_text(buffer.line_count()), None);
        assert_eq!(buffer.line_text(usize::MAX), None);
        assert_eq!(buffer.position_for_byte_offset(source.len() + 1), None);
        assert_eq!(buffer.position_for_byte_offset(usize::MAX), None);
        assert_eq!(buffer.byte_offset(beyond_end), source.len());
        assert_eq!(buffer.slice_text(full_range), source);

        let delta = buffer.replace_range(full_range, "x");
        assert_eq!(delta.before_text, source);
        assert_eq!(buffer.text(), "x");
        assert_eq!(buffer.line_count(), 1);
    }

    let mut buffer = EditorBuffer::from_text("é好");
    let inside_characters = EditorRange::new(EditorPosition::new(0, 1), EditorPosition::new(0, 4));
    assert_eq!(buffer.slice_text(inside_characters), "é");
    assert_eq!(buffer.replace_range(inside_characters, "").before_text, "é");
    assert_eq!(buffer.text(), "好");
}

#[test]
fn editor_model_buffer_exposes_range_positions_and_chunks() {
    let buffer = EditorBuffer::from_text("one\n\u{00e9}two\nthree");
    let range = EditorRange::new(EditorPosition::new(1, 0), EditorPosition::new(1, 4));

    assert_eq!(buffer.slice_text(range), "\u{00e9}tw");
    assert_eq!(
        buffer.position_for_byte_offset("one\n\u{00e9}".len()),
        Some(EditorPosition::new(1, "\u{00e9}".len()))
    );
    assert_eq!(buffer.position_for_byte_offset("one\n".len() + 1), None);
    assert_eq!(buffer.chunks().collect::<String>(), buffer.text());
}

#[test]
fn editor_model_buffer_maps_large_unicode_byte_offsets_and_lines() {
    let mut text = (0..2048)
        .map(|line| format!("line-{line:04}-\u{00e9}\u{597d}"))
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text.push_str("tail");
    let buffer = EditorBuffer::from_text(text.clone());
    let target_prefix = (0..1024)
        .map(|line| format!("line-{line:04}-\u{00e9}\u{597d}"))
        .collect::<Vec<_>>()
        .join("\n");
    let offset = target_prefix.len() + "\nline-1024-\u{00e9}".len();

    assert_eq!(
        buffer.position_for_byte_offset(offset),
        Some(EditorPosition::new(1024, "line-1024-\u{00e9}".len()))
    );
    assert_eq!(buffer.position_for_byte_offset(offset - 1), None);
    assert_eq!(buffer.line(2048).as_deref(), Some("tail"));
    assert_eq!(buffer.byte_offset(EditorPosition::new(2048, 4)), text.len());
}

#[test]
fn editor_model_buffer_preserves_trailing_empty_final_line_after_append() {
    let mut buffer = EditorBuffer::from_text("alpha\r");

    buffer.append_text("\nbeta\n");

    assert_eq!(buffer.text(), "alpha\r\nbeta\n");
    assert_eq!(buffer.line_count(), 3);
    assert_eq!(buffer.line(0).as_deref(), Some("alpha"));
    assert_eq!(buffer.line(1).as_deref(), Some("beta"));
    assert_eq!(buffer.line(2).as_deref(), Some(""));
}

#[test]
fn editor_model_movement_handles_word_paragraph_and_document_boundaries() {
    let buffer = EditorBuffer::from_text("alpha, beta_2\n\nfinal \u{597d}");

    assert_eq!(
        move_position(
            &buffer,
            EditorPosition::new(0, "alpha, beta".len()),
            CaretMotion::WordLeft
        ),
        EditorPosition::new(0, "alpha, ".len())
    );
    assert_eq!(
        move_position(&buffer, EditorPosition::new(0, 2), CaretMotion::WordRight),
        EditorPosition::new(0, "alpha, ".len())
    );
    assert_eq!(
        move_position(
            &buffer,
            EditorPosition::new(0, 3),
            CaretMotion::ParagraphDown
        ),
        EditorPosition::new(2, 0)
    );
    assert_eq!(
        line_end(&buffer, 2),
        EditorPosition::new(2, "final \u{597d}".len())
    );
    assert_eq!(
        document_end(&buffer),
        EditorPosition::new(2, "final \u{597d}".len())
    );
}

#[test]
fn editor_model_word_range_selects_containing_word() {
    let buffer = EditorBuffer::from_text("alpha, beta_2");

    assert_eq!(
        word_range_at_position(&buffer, EditorPosition::new(0, "alpha, be".len()), "txt"),
        Some(EditorRange::new(
            EditorPosition::new(0, "alpha, ".len()),
            EditorPosition::new(0, "alpha, beta_2".len())
        ))
    );
}

#[test]
fn editor_model_selection_set_preserves_main_selection_and_ranges() {
    let first = EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 3));
    let second = EditorSelection::new(EditorPosition::new(2, 0), EditorPosition::new(2, 4));
    let selections = SelectionSet::from_ranges(vec![first, second], 1);

    assert_eq!(selections.main(), second);
    assert_eq!(selections.main_index(), 1);
    assert_eq!(
        selections.ranges(),
        &[SelectionRange::from(first), SelectionRange::from(second)]
    );
    assert_eq!(selections.len(), 2);
    assert!(!selections.is_single());
}

#[test]
fn editor_model_selection_set_defaults_to_single_caret_when_empty() {
    let selections = SelectionSet::from_ranges(Vec::new(), 10);

    assert_eq!(
        selections.main(),
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0))
    );
    assert_eq!(selections.main_index(), 0);
    assert!(selections.is_single());
}

#[test]
fn editor_model_selection_set_normalizes_ranges_and_preserves_main_selection() {
    let first = EditorSelection::new(EditorPosition::new(4, 0), EditorPosition::new(4, 2));
    let second = EditorSelection::new(EditorPosition::new(1, 3), EditorPosition::new(1, 1));
    let selections = SelectionSet::from_ranges(vec![first, second], 1);

    assert_eq!(selections.main(), second);
    assert_eq!(selections.main_index(), 0);
    assert_eq!(
        selections.ranges(),
        &[SelectionRange::from(second), SelectionRange::from(first)]
    );
}

#[test]
fn editor_model_selection_set_clamps_positions_without_losing_virtual_columns() {
    let buffer = EditorBuffer::from_text("abc\nx");
    let selection = SelectionRange::new(EditorPosition::new(9, 20), EditorPosition::new(0, 2))
        .with_virtual_columns(Some(12), Some(4));
    let selections = SelectionSet::from_selection_ranges(vec![selection], 0).clamped(&buffer);

    assert_eq!(
        selections.main_range().selection(),
        EditorSelection::new(EditorPosition::new(1, 1), EditorPosition::new(0, 2))
    );
    assert_eq!(selections.main_range().anchor_virtual_column, Some(12));
    assert_eq!(selections.main_range().cursor_virtual_column, Some(4));
}

#[test]
fn editor_model_rectangular_selection_projects_lines_and_virtual_columns() {
    let buffer = EditorBuffer::from_text("a\tb\nxy");
    let selections =
        SelectionSet::rectangular(EditorPosition::new(0, 1), EditorPosition::new(1, 2), 1, 6);
    let projected = selections.projected_lines(&buffer, 4);

    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].line, 0);
    assert_eq!(projected[0].start, EditorPosition::new(0, 1));
    assert_eq!(projected[0].end, EditorPosition::new(0, "a\tb".len()));
    assert_eq!(projected[0].start_visual_column, 1);
    assert_eq!(projected[0].end_visual_column, 6);
    assert_eq!(projected[0].end_virtual_column, Some(6));
    assert_eq!(projected[1].line, 1);
    assert_eq!(projected[1].start, EditorPosition::new(1, 1));
    assert_eq!(projected[1].end, EditorPosition::new(1, 2));
    assert_eq!(projected[1].end_virtual_column, Some(6));
    assert!(matches!(
        selections.main_range().shape,
        SelectionShape::Rectangular(_)
    ));
}

#[test]
fn editor_model_projection_preserves_utf8_byte_columns_and_visual_columns() {
    let buffer = EditorBuffer::from_text("\u{00e9}\u{597d}x");
    let selection = EditorSelection::new(
        EditorPosition::new(0, "\u{00e9}".len()),
        EditorPosition::new(0, "\u{00e9}\u{597d}".len()),
    );
    let selections = SelectionSet::single(selection);
    let projected = selections.projected_lines(&buffer, 4);

    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].start, EditorPosition::new(0, 2));
    assert_eq!(projected[0].end, EditorPosition::new(0, 5));
    assert_eq!(projected[0].start_visual_column, 1);
    assert_eq!(projected[0].end_visual_column, 3);
}

#[test]
fn editor_model_word_range_preserves_utf8_byte_columns() {
    let buffer = EditorBuffer::from_text("\u{597d} alpha");

    assert_eq!(
        word_range_at_position(&buffer, EditorPosition::new(0, "\u{597d} al".len()), "txt"),
        Some(EditorRange::new(
            EditorPosition::new(0, "\u{597d} ".len()),
            EditorPosition::new(0, "\u{597d} alpha".len())
        ))
    );
}

#[test]
fn editor_model_word_range_uses_syntax_registry_word_characters() {
    let buffer = EditorBuffer::from_text("$value = 1");

    assert_eq!(
        word_range_at_position(&buffer, EditorPosition::new(0, 0), "php"),
        Some(EditorRange::new(
            EditorPosition::new(0, 0),
            EditorPosition::new(0, "$value".len())
        ))
    );
    assert_eq!(
        word_range_at_position(&buffer, EditorPosition::new(0, 0), "txt"),
        None
    );
}

#[test]
fn editor_model_word_range_ignores_non_word_positions_except_line_end() {
    let buffer = EditorBuffer::from_text("alpha, beta");

    assert_eq!(
        word_range_at_position(&buffer, EditorPosition::new(0, "alpha".len()), "txt"),
        None
    );
    assert_eq!(
        word_range_at_position(&buffer, EditorPosition::new(0, "alpha,".len()), "txt"),
        None
    );
    assert_eq!(
        word_range_at_position(&buffer, EditorPosition::new(0, "alpha, beta".len()), "txt"),
        Some(EditorRange::new(
            EditorPosition::new(0, "alpha, ".len()),
            EditorPosition::new(0, "alpha, beta".len())
        ))
    );
}

#[test]
fn editor_model_fold_provider_detects_indentation_and_multiline_braces() {
    let buffer =
        EditorBuffer::from_text("fn main() {\n    let x = [\n        1,\n    ];\n}\nlet y = 2;");
    let folds = IndentBraceFoldProvider::default().compute_folds(&buffer);

    assert!(folds.contains(&FoldRange::new(0, 4)));
    assert!(folds.contains(&FoldRange::new(1, 3)));
    assert!(folds.contains(&FoldRange::new(1, 2)));
    assert!(!folds.contains(&FoldRange::new(0, 0)));
}

#[test]
fn editor_model_fold_provider_uses_configured_indent_width() {
    let buffer = EditorBuffer::from_text("root\n  child\nnext");
    let folds = IndentBraceFoldProvider::new(2).compute_folds(&buffer);

    assert!(folds.contains(&FoldRange::new(0, 1)));
}

#[test]
fn editor_model_brace_folds_ignore_braces_inside_strings_and_line_comments() {
    let buffer = EditorBuffer::from_text(
        "let text = \"{\\nnot a fold\\n}\";\n// {\ncomment body\n// }\nfn main() {\n    run();\n}",
    );
    let folds = IndentBraceFoldProvider::default().compute_folds(&buffer);

    assert!(folds.contains(&FoldRange::new(4, 6)));
    assert!(!folds.contains(&FoldRange::new(0, 2)));
    assert!(!folds.contains(&FoldRange::new(1, 3)));
}

#[test]
fn editor_model_brace_folds_ignore_braces_inside_block_comments() {
    let buffer = EditorBuffer::from_text("/* {\ncomment body\n} */\nfn main() {\n    run();\n}");
    let folds = IndentBraceFoldProvider::default().compute_folds(&buffer);

    assert!(folds.contains(&FoldRange::new(3, 5)));
    assert!(!folds.contains(&FoldRange::new(0, 2)));
}

#[test]
fn editor_model_brace_folds_ignore_braces_inside_raw_strings() {
    let buffer =
        EditorBuffer::from_text("let text = r#\"{\nnot a fold\n}\"#;\nfn main() {\n    run();\n}");
    let folds = IndentBraceFoldProvider::default().compute_folds(&buffer);

    assert!(folds.contains(&FoldRange::new(3, 5)));
    assert!(!folds.contains(&FoldRange::new(0, 2)));
}

#[test]
fn editor_model_unknown_syntax_uses_fallback_hints() {
    let buffer = EditorBuffer::from_text("// {\ncomment body\n// }\nfn main() {\n    run();\n}");
    let folds = IndentBraceFoldProvider::for_syntax(4, "unknown").compute_folds(&buffer);

    assert!(folds.contains(&FoldRange::new(3, 5)));
    assert!(!folds.contains(&FoldRange::new(0, 2)));
}

#[test]
fn editor_model_python_hints_use_hash_comments_not_slashes() {
    let buffer = EditorBuffer::from_text("# {\ncomment body\n# }\n// {\nbody\n// }");
    let folds = IndentBraceFoldProvider::for_syntax(4, "py").compute_folds(&buffer);

    assert!(!folds.contains(&FoldRange::new(0, 2)));
    assert!(folds.contains(&FoldRange::new(3, 5)));
}

#[test]
fn editor_model_markup_hints_ignore_braces_inside_xml_comments() {
    let buffer = EditorBuffer::from_text("<!-- {\ncomment body\n} -->\n<div>{\ntext\n}</div>");
    let folds = IndentBraceFoldProvider::for_syntax(4, "xml").compute_folds(&buffer);

    assert!(!folds.contains(&FoldRange::new(0, 2)));
    assert!(folds.contains(&FoldRange::new(3, 5)));
}

#[test]
fn editor_model_rust_outline_detects_simple_functions_with_body_ranges() {
    let buffer = EditorBuffer::from_text("fn main() {\n    run();\n}\n");
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].name, "main");
    assert_eq!(outline[0].kind, FunctionKind::Function);
    assert_eq!(outline[0].depth, 0);
    assert_eq!(
        outline[0].range,
        EditorRange::new(EditorPosition::new(0, 0), EditorPosition::new(2, 1))
    );
    assert_eq!(
        outline[0].body_range,
        Some(EditorRange::new(
            EditorPosition::new(0, "fn main() ".len()),
            EditorPosition::new(2, 1)
        ))
    );
}

#[test]
fn editor_model_rust_outline_detects_representative_modifiers() {
    let buffer = EditorBuffer::from_text(
        "pub fn public() {}\nasync fn load() {}\nconst fn size() -> usize { 1 }\nunsafe fn raw() {}\nextern \"C\" fn ffi() {}\n",
    );
    let outline = outline_for_syntax(&buffer, "rs");
    let names = outline
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["public", "load", "size", "raw", "ffi"]);
    assert!(
        outline
            .iter()
            .all(|entry| entry.kind == FunctionKind::Function)
    );
    assert!(outline.iter().all(|entry| entry.body_range.is_some()));
}

#[test]
fn editor_model_rust_outline_detects_methods_and_generic_parameters() {
    let buffer = EditorBuffer::from_text(
        "impl<T> Holder<T> {\n    pub fn new(value: T) -> Self { Self { value } }\n    async fn fetch<'a, U>(&'a self, input: U) where U: Clone {}\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "rs");
    let names = outline
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["new", "fetch"]);
    assert!(
        outline
            .iter()
            .all(|entry| entry.kind == FunctionKind::Method)
    );
    assert_eq!(outline[0].depth, 1);
    assert_eq!(outline[1].depth, 1);
}

#[test]
fn editor_model_rust_outline_ignores_non_code_fn_before_impl_for_method_kind() {
    let buffer = EditorBuffer::from_text(
        "// fn example\nimpl T {\n    fn commented() {}\n}\n#[doc = \"fn example\"]\nimpl U {\n    fn documented() {}\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect::<Vec<_>>(),
        vec![
            ("commented", FunctionKind::Method),
            ("documented", FunctionKind::Method)
        ]
    );
    assert!(outline.iter().all(|entry| entry.depth == 1));
}

#[test]
fn editor_model_rust_outline_detects_trait_declarations_without_bodies() {
    let buffer = EditorBuffer::from_text(
        "trait Service {\n    fn required(&self);\n    fn provided(&self) {}\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(outline.len(), 2);
    assert_eq!(outline[0].name, "required");
    assert_eq!(outline[0].kind, FunctionKind::Declaration);
    assert_eq!(outline[0].body_range, None);
    assert_eq!(outline[0].depth, 1);
    assert_eq!(outline[1].name, "provided");
    assert_eq!(outline[1].kind, FunctionKind::Method);
    assert!(outline[1].body_range.is_some());
}

#[test]
fn editor_model_rust_outline_tracks_nested_depth_and_navigation_helpers() {
    let buffer =
        EditorBuffer::from_text("fn outer() {\n    fn inner() {\n    }\n}\nfn later() {}\n");
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.depth))
            .collect::<Vec<_>>(),
        vec![("outer", 0), ("inner", 1), ("later", 0)]
    );
    assert_eq!(
        containing_function(&outline, EditorPosition::new(1, 8)).map(|entry| entry.name.as_str()),
        Some("inner")
    );
    assert_eq!(
        next_function_after(&outline, EditorPosition::new(3, 1)).map(|entry| entry.name.as_str()),
        Some("later")
    );
    assert_eq!(
        previous_function_before(&outline, EditorPosition::new(4, 0))
            .map(|entry| entry.name.as_str()),
        Some("inner")
    );
}

#[test]
fn editor_model_rust_outline_ignores_nested_block_comments() {
    let buffer = EditorBuffer::from_text("/* outer /* inner */ fn fake() {} */\nfn real() {}\n");
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(
        outline
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["real"]
    );
}

#[test]
fn editor_model_rust_outline_ignores_comments_strings_and_raw_strings() {
    let buffer = EditorBuffer::from_text(
        "// fn commented() {}\n/* fn blocked() {} */\nconst TEXT: &str = \"fn stringy() {}\";\nconst RAW: &str = r#\"fn raw() {}\"#;\nfn real() {}\n",
    );
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].name, "real");
}

#[test]
fn editor_model_outline_detects_c_family_function_keywords() {
    let buffer = EditorBuffer::from_text(
        "function top() {\n    return 1;\n}\nclass Service {\n    load() {\n    }\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "js");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("top", FunctionKind::Function, 0),
            ("load", FunctionKind::Method, 1)
        ]
    );
    assert!(outline.iter().all(|entry| entry.body_range.is_some()));
}

#[test]
fn editor_model_outline_detects_c_family_header_tokens() {
    let buffer = EditorBuffer::from_text("struct Service {\n    bool ready();\n};\n");
    let outline = outline_for_syntax(&buffer, "hpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![("ready", FunctionKind::Declaration, 1)]
    );
    assert_eq!(outline[0].body_range, None);
}

#[test]
fn editor_model_outline_detects_c_family_callable_shapes_without_call_expressions() {
    let buffer = EditorBuffer::from_text(
        "struct Service {\n    Service();\n    ~Service();\n    bool ready() const;\n    bool operator==(const Service& other) const;\n    void operator()(int value) {}\n    void load() { if (ready()) { run(); } }\n};\nService::Service() {}\nService::~Service() {}\nbool Service::ready() const { return true; }\nauto Service::count() -> size_t { return 1; }\nbool operator==(const Service& left, const Service& right) { return left.ready() == right.ready(); }\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("Service", FunctionKind::Declaration, 1),
            ("~Service", FunctionKind::Declaration, 1),
            ("ready", FunctionKind::Declaration, 1),
            ("operator==", FunctionKind::Declaration, 1),
            ("operator()", FunctionKind::Method, 1),
            ("load", FunctionKind::Method, 1),
            ("Service", FunctionKind::Function, 0),
            ("~Service", FunctionKind::Function, 0),
            ("ready", FunctionKind::Function, 0),
            ("count", FunctionKind::Function, 0),
            ("operator==", FunctionKind::Function, 0),
        ]
    );
}

#[test]
fn editor_model_outline_detects_c_family_custom_and_template_return_types() {
    let buffer = EditorBuffer::from_text(
        "custom_type build() { return {}; }\nstd::vector<int> items() { return {}; }\nif (left > call()) {}\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("build", FunctionKind::Function, 0),
            ("items", FunctionKind::Function, 0),
        ]
    );
}

#[test]
fn editor_model_outline_ignores_c_family_global_qualified_calls() {
    let buffer = EditorBuffer::from_text(
        "void run() {\n    ::HelloWorld();\n    Namespace::Helper();\n}\nNamespace::Widget::Widget() {}\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("run", FunctionKind::Function, 0),
            ("Widget", FunctionKind::Function, 0),
        ]
    );
}

#[test]
fn editor_model_outline_ignores_c_family_member_and_expression_calls() {
    let buffer = EditorBuffer::from_text(
        "void run() {\n    object.method();\n    pointer->method();\n    return Factory();\n    throw Error();\n    auto value = Builder();\n    if (condition()) { nested(); }\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![("run", FunctionKind::Function, 0)]
    );
}

#[test]
fn editor_model_outline_keeps_c_family_qualified_declarations_and_definitions() {
    let buffer = EditorBuffer::from_text(
        "namespace Namespace {\n    struct Widget {\n        void method();\n    };\n}\nvoid Namespace::Widget::method();\nvoid Namespace::Widget::method() {}\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("method", FunctionKind::Declaration, 2),
            ("method", FunctionKind::Declaration, 0),
            ("method", FunctionKind::Function, 0),
        ]
    );
}

#[test]
fn editor_model_outline_ignores_c_family_qualified_calls_in_nested_blocks() {
    let buffer = EditorBuffer::from_text(
        "void run() {\n    if (ready()) {\n        ::Reset();\n        Namespace::Reset();\n    }\n    while (next()) {\n        Worker::tick();\n    }\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![("run", FunctionKind::Function, 0)]
    );
}

#[test]
fn editor_model_outline_ignores_c_family_calls_in_multiline_for_headers() {
    let buffer = EditorBuffer::from_text(
        "void load(NppXml::Element node) {\n    for (NppXml::Element childNode = NppXml::firstChildElement(node, \"Item\");\n\n\n        childNode;\n\n\n        childNode = NppXml::nextSiblingElement(childNode, \"Item\")) {\n    }\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![("load", FunctionKind::Function, 0)]
    );
}

#[test]
fn editor_model_outline_ignores_c_family_calls_in_control_headers() {
    let buffer = EditorBuffer::from_text(
        "void run() {\n    for (auto item = Source::first(); item; item = Source::next()) {}\n    if (State::ready()) {}\n    else if (State::again()) {}\n    while (Queue::poll()) {}\n    switch (Router::select()) { case 1: break; }\n    try { work(); } catch (const Error& error) { Handler::recover(); }\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "cpp");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![("run", FunctionKind::Function, 0)]
    );
}

#[test]
fn editor_model_outline_ignores_java_record_headers_but_detects_record_methods() {
    let buffer = EditorBuffer::from_text(
        "public record Point(int x, int y) {\n    public int sum() { return x + y; }\n}\n",
    );
    let outline = outline_for_syntax(&buffer, "java");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![("sum", FunctionKind::Method, 1)]
    );
}

#[test]
fn editor_model_xml_registry_hash_changes_with_parser_content() {
    let base = r#"
        <outline-parsers schema-version="1">
          <family id="brace">
            <adapter name="generic-brace" />
            <body kind="brace" open="{" close="}" />
          </family>
          <language name="Mini">
            <token value="mini" />
            <use-family id="brace" adapter="generic-brace" />
            <lexical>
              <word-characters extra="_" unicode="true" />
            </lexical>
            <declaration kind="function" keyword="fn" name="after-keyword" body="brace" />
          </language>
        </outline-parsers>
    "#;
    let changed = base.replace("Mini", "MiniChanged");
    let first = OutlineRegistry::from_xml(base);
    let second = OutlineRegistry::from_xml(base);
    let changed = OutlineRegistry::from_xml(&changed);

    assert_eq!(first.registry_hash(), second.registry_hash());
    assert_ne!(first.registry_hash(), changed.registry_hash());
    assert_eq!(
        first
            .plan_for_syntax("MINI")
            .map(|plan| plan.adapter_name.as_str()),
        Some("generic-brace")
    );
    assert!(first.diagnostics().is_empty());
}

#[test]
fn editor_model_outline_parse_snapshot_preserves_request_metadata() {
    let request = OutlineParseRequest::new(
        DocumentId::new(42),
        Arc::new("fn target() {}\n".to_owned()),
        "rs",
        7,
        12345,
    );

    let result = parse_outline_snapshot(request);

    assert_eq!(result.document_id, DocumentId::new(42));
    assert_eq!(result.revision, 7);
    assert_eq!(result.syntax_token, "rs");
    assert_eq!(result.registry_hash, 12345);
    assert_eq!(
        result
            .functions
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["target"]
    );
}

#[test]
fn editor_model_outline_cascades_nested_rust_modules_impls_and_functions() {
    let request = OutlineParseRequest::new(
        DocumentId::new(1),
        Arc::new(
            "mod api {\n    fn helper() {}\n    impl Service {\n        fn new() {}\n        fn run() {\n            fn local() {}\n        }\n    }\n}\nfn top() {}\n"
                .to_owned(),
        ),
        "rs",
        0,
        0,
    );
    let result = parse_outline_snapshot(request);

    assert_eq!(
        result
            .functions
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("helper", FunctionKind::Function, 1),
            ("new", FunctionKind::Method, 2),
            ("run", FunctionKind::Method, 2),
            ("local", FunctionKind::Method, 3),
            ("top", FunctionKind::Function, 0),
        ]
    );
    assert_eq!(result.tree.roots[0].name, "api");
    assert_eq!(result.tree.roots[0].kind, OutlineNodeKind::Module);
    assert_eq!(
        result.tree.roots[0]
            .children
            .iter()
            .map(|node| (node.name.as_str(), node.kind))
            .collect::<Vec<_>>(),
        vec![
            ("helper", OutlineNodeKind::Function),
            ("Service", OutlineNodeKind::Impl)
        ]
    );
    assert_eq!(
        result.tree.roots[0].children[1]
            .children
            .iter()
            .map(|node| (node.name.as_str(), node.kind))
            .collect::<Vec<_>>(),
        vec![
            ("new", OutlineNodeKind::Method),
            ("run", OutlineNodeKind::Method)
        ]
    );
    let local = &result.tree.roots[0].children[1].children[1].children[0];
    assert_eq!(local.name, "local");
    assert_eq!(local.kind, OutlineNodeKind::Method);
    assert_eq!(local.depth, 3);
}

#[test]
fn editor_model_outline_matches_language_snippet_fixtures() {
    let cases = [
        OutlineSnippetCase {
            syntax_token: "rs",
            source: include_str!("fixtures/outline_snippets/rust_nested.rs"),
            expected: RUST_SNIPPET_EXPECTED,
        },
        OutlineSnippetCase {
            syntax_token: "py",
            source: include_str!("fixtures/outline_snippets/python_nested.py"),
            expected: PYTHON_SNIPPET_EXPECTED,
        },
        OutlineSnippetCase {
            syntax_token: "js",
            source: include_str!("fixtures/outline_snippets/javascript_nested.js"),
            expected: JAVASCRIPT_SNIPPET_EXPECTED,
        },
        OutlineSnippetCase {
            syntax_token: "java",
            source: include_str!("fixtures/outline_snippets/java_nested.java"),
            expected: JAVA_SNIPPET_EXPECTED,
        },
        OutlineSnippetCase {
            syntax_token: "kt",
            source: include_str!("fixtures/outline_snippets/kotlin_nested.kt"),
            expected: KOTLIN_SNIPPET_EXPECTED,
        },
        OutlineSnippetCase {
            syntax_token: "cpp",
            source: include_str!("fixtures/outline_snippets/c_family_nested.cpp"),
            expected: C_FAMILY_SNIPPET_EXPECTED,
        },
        OutlineSnippetCase {
            syntax_token: "rb",
            source: include_str!("fixtures/outline_snippets/ruby_nested.rb"),
            expected: RUBY_SNIPPET_EXPECTED,
        },
    ];

    for case in cases {
        let buffer = EditorBuffer::from_text(case.source);
        let outline = outline_for_syntax(&buffer, case.syntax_token);

        assert_eq!(
            outline
                .iter()
                .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
                .collect::<Vec<_>>(),
            case.expected,
            "outline mismatch for {} fixture",
            case.syntax_token
        );
    }
}

#[test]
fn editor_model_outline_deduplicates_overlapping_python_async_rules_in_tree() {
    let request = OutlineParseRequest::new(
        DocumentId::new(6),
        Arc::new("class Service:\n    async def save(self):\n        pass\n".to_owned()),
        "py",
        0,
        0,
    );
    let result = parse_outline_snapshot(request);

    assert_eq!(
        result
            .functions
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["save"]
    );
    assert_eq!(result.tree.roots.len(), 1);
    assert_eq!(
        result.tree.roots[0]
            .children
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        vec!["save"]
    );
}

#[test]
fn editor_model_outline_accepts_javascript_dollar_identifiers() {
    let buffer = EditorBuffer::from_text("function $render() {}\nclass View {\n  $mount() {}\n}\n");
    let outline = outline_for_syntax(&buffer, "js");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect::<Vec<_>>(),
        vec![
            ("$render", FunctionKind::Function),
            ("$mount", FunctionKind::Method),
        ]
    );
}

#[test]
fn editor_model_outline_detects_javascript_arrow_functions() {
    let buffer = EditorBuffer::from_text(
        "const top = () => {\n  const nested = () => {\n  };\n};\nconst withDefault = (callback = () => {}) => {\n};\nclass View {\n  load = () => {\n  };\n}\nconst expression = () => 1;\n",
    );
    let outline = outline_for_syntax(&buffer, "js");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("top", FunctionKind::Function, 0),
            ("nested", FunctionKind::Function, 1),
            ("withDefault", FunctionKind::Function, 0),
            ("load", FunctionKind::Method, 1),
        ]
    );
    assert!(outline.iter().all(|entry| entry.body_range.is_some()));
}

#[test]
fn editor_model_outline_detects_typescript_arrow_functions() {
    let buffer =
        EditorBuffer::from_text("const load = async (value: string): Promise<void> => {\n};\n");
    let outline = outline_for_syntax(&buffer, "ts");

    assert_eq!(
        outline
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![("load", FunctionKind::Function, 0)]
    );
    assert!(outline[0].body_range.is_some());
}

#[test]
fn editor_model_outline_masks_comments_strings_and_templates_for_javascript() {
    let buffer = EditorBuffer::from_text(
        "// function commented() {}\n/* function blocked() {} */\nconst text = \"function stringy() {}\";\nconst single = 'function quoted() {}';\nconst tmpl = `function templated() {}`;\nfunction real() {}\n",
    );
    let outline = outline_for_syntax(&buffer, "js");

    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].name, "real");
}

#[test]
fn editor_model_outline_masks_python_single_quoted_code_snippet() {
    let buffer = EditorBuffer::from_text(include_str!(
        "fixtures/outline_snippets/python_single_quote_masking.py"
    ));

    assert_eq!(
        outline_for_syntax(&buffer, "py")
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["real"]
    );
}

#[test]
fn editor_model_outline_masks_javascript_single_quoted_code_snippet() {
    let buffer = EditorBuffer::from_text(include_str!(
        "fixtures/outline_snippets/javascript_single_quote_masking.js"
    ));

    assert_eq!(
        outline_for_syntax(&buffer, "js")
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["real"]
    );
}

#[test]
fn editor_model_outline_masks_ruby_single_quoted_code_snippet() {
    let buffer = EditorBuffer::from_text(include_str!(
        "fixtures/outline_snippets/ruby_single_quote_masking.rb"
    ));

    assert_eq!(
        outline_for_syntax(&buffer, "rb")
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["real"]
    );
}

#[test]
fn editor_model_outline_detects_ruby_end_keyword_classes_modules_and_methods() {
    let request = OutlineParseRequest::new(
        DocumentId::new(2),
        Arc::new(
            "module Admin\n  class User\n    def load\n      def helper\n      end\n    end\n  end\nend\n\ndef top\nend\n"
                .to_owned(),
        ),
        "rb",
        0,
        0,
    );
    let result = parse_outline_snapshot(request);

    assert_eq!(
        result
            .functions
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind, entry.depth))
            .collect::<Vec<_>>(),
        vec![
            ("load", FunctionKind::Method, 2),
            ("helper", FunctionKind::Method, 3),
            ("top", FunctionKind::Function, 0),
        ]
    );
    assert_eq!(result.tree.roots[0].name, "Admin");
    assert_eq!(result.tree.roots[0].kind, OutlineNodeKind::Module);
    assert_eq!(result.tree.roots[0].children[0].name, "User");
    assert_eq!(
        result.tree.roots[0].children[0].kind,
        OutlineNodeKind::Class
    );
    assert_eq!(
        result.functions[0].body_range,
        Some(EditorRange::new(
            EditorPosition::new(2, "    def load".len()),
            EditorPosition::new(5, "    end".len())
        ))
    );
}

#[test]
fn editor_model_outline_accepts_ruby_bang_and_question_methods() {
    let buffer =
        EditorBuffer::from_text("class User\n  def save!\n  end\n  def ready?\n  end\nend\n");
    let outline = outline_for_syntax(&buffer, "rb");

    assert_eq!(
        outline
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["save!", "ready?"]
    );
}

#[test]
fn editor_model_outline_returns_no_entries_without_configured_rules() {
    let buffer = EditorBuffer::from_text("fn main() {}\nfunction run() {}\n");

    assert!(outline_for_syntax(&buffer, "unknown").is_empty());
}

#[test]
fn editor_model_rust_outline_captures_non_ascii_function_names() {
    let buffer = EditorBuffer::from_text("fn caf\u{00e9}() {}\nfn \u{597d}() {}\n");
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(outline.len(), 2);
    assert_eq!(outline[0].name, "caf\u{00e9}");
    assert_eq!(outline[1].name, "\u{597d}");
    assert_eq!(
        outline[0].range,
        EditorRange::new(
            EditorPosition::new(0, 0),
            EditorPosition::new(0, "fn caf\u{00e9}() {}".len())
        )
    );
    assert_eq!(
        outline[0].body_range,
        Some(EditorRange::new(
            EditorPosition::new(0, "fn caf\u{00e9}() ".len()),
            EditorPosition::new(0, "fn caf\u{00e9}() {}".len())
        ))
    );
    assert_eq!(
        outline[1].range,
        EditorRange::new(
            EditorPosition::new(1, 0),
            EditorPosition::new(1, "fn \u{597d}() {}".len())
        )
    );
    assert_eq!(
        outline[1].body_range,
        Some(EditorRange::new(
            EditorPosition::new(1, "fn \u{597d}() ".len()),
            EditorPosition::new(1, "fn \u{597d}() {}".len())
        ))
    );
}

#[test]
fn editor_model_rust_outline_uses_utf8_byte_columns() {
    let buffer = EditorBuffer::from_text("mod \u{597d} { fn named() {} }\n");
    let outline = outline_for_syntax(&buffer, "rs");

    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].name, "named");
    assert_eq!(outline[0].depth, 1);
    assert_eq!(
        outline[0].range.start,
        EditorPosition::new(0, "mod \u{597d} { ".len())
    );
    assert_eq!(
        outline[0].body_range.map(|range| range.start),
        Some(EditorPosition::new(0, "mod \u{597d} { fn named() ".len()))
    );
}

#[test]
fn editor_model_fold_model_keeps_collapsed_state_only_for_recomputed_matching_ranges() {
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 3), FoldRange::new(5, 8)]);

    assert!(folds.set_collapsed(FoldRange::new(0, 3), true));
    assert!(folds.is_line_hidden(2));

    folds.recompute(vec![FoldRange::new(0, 3), FoldRange::new(6, 9)]);

    assert!(folds.is_collapsed(FoldRange::new(0, 3)));
    assert!(!folds.is_collapsed(FoldRange::new(5, 8)));
}

#[test]
fn editor_model_fold_model_finds_current_fold_from_header_or_child() {
    let folds = FoldModel::new(vec![
        FoldRange::new(0, 6),
        FoldRange::new(1, 3),
        FoldRange::new(1, 2),
        FoldRange::new(4, 5),
    ]);

    assert_eq!(folds.range_at_or_parent(0), Some(FoldRange::new(0, 6)));
    assert_eq!(folds.range_at_or_parent(1), Some(FoldRange::new(1, 3)));
    assert_eq!(folds.range_at_or_parent(2), Some(FoldRange::new(1, 2)));
    assert_eq!(folds.range_at_or_parent(5), Some(FoldRange::new(4, 5)));
    assert_eq!(folds.range_at_or_parent(7), None);
}

#[test]
fn editor_model_fold_model_sets_all_collapsed_and_expanded() {
    let outer = FoldRange::new(0, 3);
    let inner = FoldRange::new(1, 2);
    let mut folds = FoldModel::new(vec![outer, inner]);

    assert!(folds.set_all_collapsed(true));
    assert!(folds.is_collapsed(outer));
    assert!(folds.is_collapsed(inner));
    assert!(folds.is_line_hidden(1));
    assert!(!folds.set_all_collapsed(true));

    assert!(folds.set_all_collapsed(false));
    assert!(!folds.is_collapsed(outer));
    assert!(!folds.is_collapsed(inner));
    assert!(!folds.is_line_hidden(1));
    assert!(!folds.set_all_collapsed(false));
}

#[test]
fn editor_model_viewport_maps_visible_rows_around_collapsed_fold_children() {
    let mut folds = FoldModel::new(vec![FoldRange::new(1, 3)]);
    folds.set_collapsed(FoldRange::new(1, 3), true);
    let viewport = ViewportModel::new(5, &folds);

    assert_eq!(viewport.visible_row_count(), 3);
    assert_eq!(viewport.document_line_to_visible_row(0), Some(0));
    assert_eq!(viewport.document_line_to_visible_row(1), Some(1));
    assert_eq!(viewport.document_line_to_visible_row(2), None);
    assert_eq!(viewport.document_line_to_visible_row(3), None);
    assert_eq!(viewport.document_line_to_visible_row(4), Some(2));
    assert_eq!(viewport.visible_row_to_document_line(2), Some(4));
}

#[test]
fn editor_model_decorations_capture_settings_hidden_spans_and_fold_controls() {
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 2)]);
    folds.set_collapsed(FoldRange::new(0, 2), true);
    let settings = DecorationSettings {
        show_spaces: true,
        show_tabs: true,
        show_end_of_line_markers: true,
        ..DecorationSettings::default()
    };
    let decorations =
        DecorationModel::from_folds(settings, 4, &folds, vec![IndentGuide { line: 1, depth: 1 }]);

    assert!(decorations.settings.show_spaces);
    assert!(decorations.settings.show_tabs);
    assert!(decorations.settings.show_end_of_line_markers);
    assert_eq!(
        decorations.hidden_line_spans,
        vec![HiddenLineSpan {
            header_line: 0,
            first_hidden_line: 1,
            last_hidden_line: 2,
        }]
    );
    assert_eq!(decorations.hidden_line_spans[0].hidden_line_count(), 2);
    assert_eq!(
        decorations.indent_guides,
        vec![IndentGuide { line: 1, depth: 1 }]
    );
    assert_eq!(decorations.line_decorations[0].line_number, Some(1));
    assert!(decorations.line_decorations[0].has_fold_control);
    assert!(decorations.line_decorations[0].is_fold_collapsed);
}

#[test]
fn editor_model_indentation_folds_skip_blank_lines_inside_blocks() {
    let buffer = EditorBuffer::from_text("root\n    first\n\n    second\nnext");
    let folds = IndentBraceFoldProvider::default().compute_folds(&buffer);

    assert!(folds.contains(&FoldRange::new(0, 3)));
    assert!(!folds.contains(&FoldRange::new(0, 2)));
}

#[test]
fn editor_model_viewport_prefers_outer_collapsed_fold_for_visible_mapping() {
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 5), FoldRange::new(1, 3)]);
    assert!(folds.set_collapsed(FoldRange::new(0, 5), true));
    assert!(folds.set_collapsed(FoldRange::new(1, 3), true));

    let viewport = ViewportModel::new(7, &folds);

    assert_eq!(viewport.line_count(), 7);
    assert_eq!(viewport.visible_row_count(), 2);
    assert_eq!(viewport.visible_row_to_document_line(0), Some(0));
    assert_eq!(viewport.visible_row_to_document_line(1), Some(6));
    for line in 1..=5 {
        assert_eq!(viewport.document_line_to_visible_row(line), None);
    }
}

#[test]
fn editor_model_decorations_can_disable_line_numbers_and_fold_controls() {
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 2)]);
    folds.set_collapsed(FoldRange::new(0, 2), true);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            show_line_numbers: false,
            show_folding_controls: false,
            ..DecorationSettings::default()
        },
        3,
        &folds,
        vec![],
    );

    assert_eq!(decorations.hidden_line_spans.len(), 1);
    assert!(
        decorations
            .line_decorations
            .iter()
            .all(|line| line.line_number.is_none() && !line.has_fold_control)
    );
    assert!(decorations.line_decorations[0].is_fold_collapsed);
}
