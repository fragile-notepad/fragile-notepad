use fragile_notepad::editor::layout::visual_column_for;
use fragile_notepad::editor::viewport::RowSegment;
use fragile_notepad::editor::{
    EditorBuffer, EditorPosition, EditorRange, FoldModel, FoldRange, IndentBraceFoldProvider,
    ViewportModel,
};
use unicode_segmentation::UnicodeSegmentation;

fn rows(buffer: &EditorBuffer, viewport: &ViewportModel) -> Vec<(usize, String, RowSegment)> {
    viewport
        .visible_rows()
        .map(|row| {
            let text = buffer.line(row.document_line).unwrap();
            let segment = viewport.row_segment(row.row, buffer).unwrap();
            (
                row.document_line,
                text[segment.start_column..segment.end_column].to_owned(),
                segment,
            )
        })
        .collect()
}

fn row_texts(buffer: &EditorBuffer, viewport: &ViewportModel) -> Vec<String> {
    rows(buffer, viewport)
        .into_iter()
        .map(|(_, text, _)| text)
        .collect()
}

#[test]
fn wraps_at_word_and_punctuation_boundaries_without_discarding_spaces() {
    let buffer = EditorBuffer::from_text("alpha beta gamma\nalpha.beta/gamma");
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 8, 4);

    assert_eq!(
        row_texts(&buffer, &viewport),
        ["alpha ", "beta ", "gamma", "alpha.", "beta/", "gamma"]
    );
    assert_eq!(viewport.line_count(), 2);
    assert_eq!(viewport.visible_row_count(), 6);
    assert_eq!(viewport.document_line_to_visible_row(1), Some(3));
    assert_eq!(viewport.wrap_columns(), Some(8));
}

#[test]
fn preserves_indentation_repeated_spaces_and_trailing_whitespace() {
    let buffer = EditorBuffer::from_text("   one  two   ");
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 5, 4);
    let displayed = row_texts(&buffer, &viewport);

    assert_eq!(displayed.concat(), "   one  two   ");
    assert!(
        displayed
            .iter()
            .all(|row| !row.is_empty() && row.len() <= 5)
    );
}

#[test]
fn collapsed_terminal_opener_keeps_trailing_whitespace_and_carets_on_its_final_row() {
    let header = String::from("fn 字() {") + &"\t ".repeat(16);
    let source = format!("{header}\n    run();\n}}\nafter");
    let range = FoldRange::new(0, 2);
    for columns in [8, 16] {
        let mut buffer = EditorBuffer::from_text(&source);
        let mut folds = IndentBraceFoldProvider::for_syntax(4, "rs").compute_fold_model(&buffer);
        assert!(folds.set_collapsed(range, true));
        let opener = folds.delimiter(range).unwrap().opening_column;
        let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, columns, 4);
        for extra in ["", "\t  "] {
            if !extra.is_empty() {
                let end = EditorPosition::new(0, buffer.line(0).unwrap().len());
                buffer.replace_range(EditorRange::new(end, end), extra);
                assert!(viewport.reflow_wrapped_lines(&buffer, &folds, 0, 0));
            }
            let header = buffer.line(0).unwrap();
            let header_rows = rows(&buffer, &viewport)
                .into_iter()
                .filter(|(line, _, _)| *line == 0)
                .collect::<Vec<_>>();
            let (_, _, last) = header_rows.last().unwrap();
            assert!(last.is_last);
            assert!(last.start_column <= opener && opener < last.end_column);
            assert_eq!(last.end_column, header.len());
            assert_eq!(
                last.end_visual_column,
                visual_column_for(&header, header.len(), 4)
            );
            assert_eq!(
                header_rows
                    .iter()
                    .map(|(_, text, _)| text.as_str())
                    .collect::<String>(),
                header
            );
            for column in [opener, opener + 1, header.len()] {
                assert_eq!(
                    viewport.position_to_visible_row(EditorPosition::new(0, column)),
                    Some(header_rows.len() - 1)
                );
            }
            assert_eq!(
                viewport,
                ViewportModel::new_wrapped(&buffer, &folds, columns, 4)
            );
        }
    }
}

#[test]
fn hard_wraps_long_tokens_and_avoids_an_extra_row_at_exact_width() {
    let buffer = EditorBuffer::from_text("abcdefghij\nabcd");
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 4, 4);

    assert_eq!(
        row_texts(&buffer, &viewport),
        ["abcd", "efgh", "ij", "abcd"]
    );
    assert_eq!(viewport.document_line_to_visible_row(1), Some(3));
    assert!(!viewport.row_segment(0, &buffer).unwrap().is_last);
    assert!(viewport.row_segment(2, &buffer).unwrap().is_last);
    assert!(viewport.row_segment(3, &buffer).unwrap().is_last);
}

#[test]
fn does_not_split_combining_characters_flags_or_joined_emoji() {
    let text = "a\u{301}b🇸🇬👩‍💻z";
    let buffer = EditorBuffer::from_text(text);
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 2, 4);
    let displayed = row_texts(&buffer, &viewport);
    let boundaries = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();

    assert_eq!(displayed, ["a\u{301}b", "🇸🇬", "👩‍💻", "z"]);
    assert_eq!(displayed.concat(), text);
    for (_, _, segment) in rows(&buffer, &viewport) {
        assert!(boundaries.contains(&segment.start_column));
        assert!(boundaries.contains(&segment.end_column));
    }
}

#[test]
fn keeps_tabs_on_original_logical_stops_across_wrapped_rows() {
    let text = "ab\tcdef\tgh";
    let buffer = EditorBuffer::from_text(text);
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 5, 4);

    assert_eq!(row_texts(&buffer, &viewport), ["ab\t", "cdef", "\t", "gh"]);
    for (_, _, segment) in rows(&buffer, &viewport) {
        assert_eq!(
            segment.start_visual_column,
            visual_column_for(text, segment.start_column, 4)
        );
        assert_eq!(
            segment.end_visual_column,
            visual_column_for(text, segment.end_column, 4)
        );
    }
    let tab = viewport.row_segment(2, &buffer).unwrap();
    assert_eq!((tab.start_visual_column, tab.end_visual_column), (8, 12));
}

#[test]
fn empty_lines_and_line_endings_remain_logical_lines() {
    let buffer = EditorBuffer::from_text("abcde\r\n\nlast\n");
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 3, 4);

    assert_eq!(
        row_texts(&buffer, &viewport),
        ["abc", "de", "", "las", "t", ""]
    );
    assert_eq!(viewport.line_count(), 4);
    assert_eq!(viewport.document_line_to_visible_row(1), Some(2));
    assert_eq!(viewport.document_line_to_visible_row(3), Some(5));

    let empty = EditorBuffer::from_text("");
    let empty_viewport = ViewportModel::new_wrapped(&empty, &FoldModel::default(), 3, 4);
    assert_eq!(row_texts(&empty, &empty_viewport), [""]);
    assert_eq!(
        empty_viewport.position_to_visible_row(EditorPosition::new(0, 0)),
        Some(0)
    );
}

#[test]
fn boundary_positions_belong_to_following_row_and_eol_to_final_row() {
    let buffer = EditorBuffer::from_text("abcdefghij\nnext");
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 4, 4);

    for (column, expected_row) in [(0, 0), (3, 0), (4, 1), (7, 1), (8, 2), (10, 2), (99, 2)] {
        assert_eq!(
            viewport.position_to_visible_row(EditorPosition::new(0, column)),
            Some(expected_row)
        );
    }
    assert_eq!(
        viewport.position_to_visible_row(EditorPosition::new(1, 0)),
        Some(3)
    );
    assert_eq!(
        viewport.position_to_visible_row(EditorPosition::new(2, 0)),
        None
    );
    assert_eq!(viewport.row_segment(4, &buffer), None);
}

#[test]
fn hides_fold_bodies_and_reserves_space_for_the_collapsed_header_badge() {
    let buffer = EditorBuffer::from_text("abcdefghij\nhidden\nalso hidden\ntail");
    let range = FoldRange::new(0, 2);
    let mut folds = FoldModel::new(vec![range]);
    folds.set_collapsed(range, true);
    let viewport = ViewportModel::new_wrapped(&buffer, &folds, 10, 4);

    assert_eq!(row_texts(&buffer, &viewport), ["abcde", "fghij", "tail"]);
    assert_eq!(viewport.document_line_to_visible_row(0), Some(0));
    assert_eq!(viewport.document_line_to_visible_row(1), None);
    assert_eq!(viewport.document_line_to_visible_row(2), None);
    assert_eq!(viewport.document_line_to_visible_row(3), Some(2));
    assert_eq!(viewport.visible_row_to_document_line(1), Some(0));
    assert_eq!(
        viewport.position_to_visible_row(EditorPosition::new(1, 0)),
        None
    );
    assert!(viewport.row_segment(1, &buffer).unwrap().is_continuation());

    folds.set_collapsed(range, false);
    let expanded = ViewportModel::new_wrapped(&buffer, &folds, 10, 4);
    assert_eq!(expanded.row_segment(0, &buffer).unwrap().end_column, 10);
}

#[test]
fn nested_collapsed_folds_keep_the_outermost_visible_headers() {
    let buffer = EditorBuffer::from_text("a\nb\nc\nd\ne\nf\ng");
    let mut folds = FoldModel::new(vec![
        FoldRange::new(0, 2),
        FoldRange::new(0, 3),
        FoldRange::new(1, 2),
        FoldRange::new(4, 5),
    ]);
    folds.set_all_collapsed(true);
    let viewport = ViewportModel::new_wrapped(&buffer, &folds, 8, 4);

    assert_eq!(row_texts(&buffer, &viewport), ["a", "e", "g"]);
    assert_eq!(
        viewport
            .visible_rows()
            .map(|row| row.document_line)
            .collect::<Vec<_>>(),
        [0, 4, 6]
    );
}

#[test]
fn zero_width_viewport_still_progresses_by_complete_graphemes() {
    let buffer = EditorBuffer::from_text("字\ta\u{301}");
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 0, 0);

    assert_eq!(viewport.wrap_columns(), Some(1));
    assert_eq!(row_texts(&buffer, &viewport), ["字", "\t", "a\u{301}"]);
}

#[test]
fn unwrapped_segments_use_the_configured_tab_width() {
    let buffer = EditorBuffer::from_text("\tab\nsecond");
    let viewport = ViewportModel::new_with_tab_width(2, &FoldModel::default(), 8);
    let segment = viewport.row_segment(0, &buffer).unwrap();

    assert_eq!(viewport.wrap_columns(), None);
    assert_eq!((segment.start_column, segment.end_column), (0, 3));
    assert_eq!(
        (segment.start_visual_column, segment.end_visual_column),
        (0, 10)
    );
    assert!(segment.is_last);
    assert!(!segment.is_continuation());
    assert_eq!(
        viewport.position_to_visible_row(EditorPosition::new(0, 3)),
        Some(0)
    );
}

#[test]
fn appended_loading_reflows_the_last_line_and_maps_new_lines() {
    let mut buffer = EditorBuffer::from_text("alpha beta\nlast");
    let mut viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 7, 4);

    for chunk in [" part", "\nmore lines\n", "final", "\r", "\nend"] {
        buffer.append_text(chunk);
        viewport.sync_unfolded_wrapped_buffer(&buffer);
        assert_eq!(
            viewport,
            ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 7, 4)
        );
    }
}

#[test]
fn append_sync_rebuilds_safely_if_given_folded_or_shorter_input() {
    let buffer = EditorBuffer::from_text("header body\nhidden\ntail");
    let range = FoldRange::new(0, 1);
    let mut folds = FoldModel::new(vec![range]);
    folds.set_collapsed(range, true);
    let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, 6, 4);

    viewport.sync_unfolded_wrapped_buffer(&buffer);
    assert_eq!(
        viewport,
        ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 6, 4)
    );
    let shorter = EditorBuffer::from_text("short");
    viewport.sync_unfolded_wrapped_buffer(&shorter);
    assert_eq!(
        viewport,
        ViewportModel::new_wrapped(&shorter, &FoldModel::default(), 6, 4)
    );
}

#[test]
fn line_count_only_sync_explicitly_returns_to_unwrapped_identity() {
    for columns in [3, 100] {
        let buffer = EditorBuffer::from_text("alpha beta\nlast");
        let mut viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), columns, 4);

        viewport.sync_unfolded_line_count(3);
        assert_eq!(viewport.wrap_columns(), None);
        assert_eq!(viewport.visible_row_count(), 3);
        assert_eq!(viewport.document_line_to_visible_row(2), Some(2));
        assert_eq!(viewport, ViewportModel::new(3, &FoldModel::default()));
    }
}

#[test]
fn maps_long_minified_lines_without_losing_or_repeating_bytes() {
    let text = "x".repeat(120_000);
    let buffer = EditorBuffer::from_text(&text);
    let viewport = ViewportModel::new_wrapped(&buffer, &FoldModel::default(), 80, 4);

    assert_eq!(viewport.visible_row_count(), 1_500);
    assert_eq!(
        viewport.position_to_visible_row(EditorPosition::new(0, 119_920)),
        Some(1_499)
    );
    let mut expected_start = 0;
    for row in viewport.visible_rows() {
        let segment = viewport.row_segment(row.row, &buffer).unwrap();
        assert_eq!(segment.start_column, expected_start);
        assert_eq!(segment.end_column - segment.start_column, 80);
        expected_start = segment.end_column;
    }
    assert_eq!(expected_start, text.len());
}

#[test]
fn localized_reflow_updates_growing_shrinking_and_empty_lines() {
    let mut buffer = EditorBuffer::from_text("header\nmiddle\n\tlast line\n");
    let folds = FoldModel::default();
    let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, 7, 4);

    for (line, replacement) in [
        (1, "a much longer middle line"),
        (0, ""),
        (2, "字\t👩‍💻 e\u{301}"),
        (3, "now the formerly empty final line wraps"),
        (1, "x"),
        (3, ""),
    ] {
        buffer.replace_range(
            EditorRange::new(
                EditorPosition::new(line, 0),
                EditorPosition::new(line, usize::MAX),
            ),
            replacement,
        );

        assert!(viewport.reflow_wrapped_lines(&buffer, &folds, line, line));
        assert_eq!(viewport, ViewportModel::new_wrapped(&buffer, &folds, 7, 4));
    }
}

#[test]
fn localized_reflow_preserves_folded_bodies_and_header_badge_space() {
    let mut buffer =
        EditorBuffer::from_text("long header\nhidden one\nhidden two\nvisible suffix\nlast");
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 2)]);
    folds.set_all_collapsed(true);
    let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, 8, 4);
    buffer.replace_range(
        EditorRange::new(
            EditorPosition::new(0, 0),
            EditorPosition::new(3, usize::MAX),
        ),
        "longer changed header\nhidden changed\nmore hidden\nvisible changed suffix",
    );

    assert!(viewport.reflow_wrapped_lines(&buffer, &folds, 0, 3));
    assert_eq!(viewport, ViewportModel::new_wrapped(&buffer, &folds, 8, 4));
    assert_eq!(viewport.document_line_to_visible_row(1), None);
    assert_eq!(viewport.document_line_to_visible_row(2), None);
    for row in viewport.visible_rows().filter(|row| row.document_line == 0) {
        let segment = viewport.row_segment(row.row, &buffer).unwrap();
        assert!(segment.end_visual_column - segment.start_visual_column <= 4);
    }
}

#[test]
fn reflow_of_hidden_lines_does_not_change_the_visible_mapping() {
    let mut buffer = EditorBuffer::from_text("header\nhidden\nother hidden\ntail");
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 2)]);
    folds.set_all_collapsed(true);
    let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, 8, 4);
    let before = viewport.clone();
    buffer.replace_range(
        EditorRange::new(
            EditorPosition::new(1, 0),
            EditorPosition::new(2, usize::MAX),
        ),
        "a very long changed hidden line\nsecond hidden replacement",
    );

    assert!(viewport.reflow_wrapped_lines(&buffer, &folds, 1, 2));
    assert_eq!(viewport, before);
    assert_eq!(viewport, ViewportModel::new_wrapped(&buffer, &folds, 8, 4));
}

#[test]
fn localized_reflow_rejects_changed_line_count_folds_and_unwrapped_state() {
    let buffer = EditorBuffer::from_text("header\nbody\ntail");
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 1)]);
    let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, 4, 4);
    let before = viewport.clone();
    let mut appended = buffer.clone();
    appended.append_text("\nnew line");

    assert!(!viewport.reflow_wrapped_lines(&appended, &folds, 2, 3));
    assert_eq!(viewport, before);
    folds.set_all_collapsed(true);
    assert!(!viewport.reflow_wrapped_lines(&buffer, &folds, 0, 1));
    assert_eq!(viewport, before);

    let mut unwrapped = ViewportModel::new(buffer.line_count(), &folds);
    let unwrapped_before = unwrapped.clone();
    assert!(!unwrapped.reflow_wrapped_lines(&buffer, &folds, 0, 1));
    assert_eq!(unwrapped, unwrapped_before);
}

#[test]
fn localized_reflow_keeps_suffix_positions_correct_after_repeated_edits() {
    let mut buffer = EditorBuffer::from_text("one\ntwo\nthree\nfour\nfive\n");
    let folds = FoldModel::default();
    let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, 6, 8);
    let replacements = [
        "",
        "x",
        "one two three four",
        "a\tb",
        "字字字字",
        "e\u{301}🇸🇬👩‍💻",
    ];

    for step in 0..36 {
        let line = step % buffer.line_count();
        let replacement = replacements[(step / buffer.line_count() + line) % replacements.len()];
        buffer.replace_range(
            EditorRange::new(
                EditorPosition::new(line, 0),
                EditorPosition::new(line, usize::MAX),
            ),
            replacement,
        );

        assert!(viewport.reflow_wrapped_lines(&buffer, &folds, line, line));
        assert_eq!(viewport, ViewportModel::new_wrapped(&buffer, &folds, 6, 8));
        let last = buffer.line_count() - 1;
        let end = buffer.line(last).unwrap().len();
        assert_eq!(
            viewport.position_to_visible_row(EditorPosition::new(last, end)),
            Some(viewport.visible_row_count() - 1)
        );
    }
}

#[test]
fn localized_reflow_and_append_sync_preserve_custom_badge_reservation() {
    let mut buffer = EditorBuffer::from_text("abcdef\nhidden\ntail");
    let fold = FoldRange::new(0, 1);
    let mut folds = FoldModel::new(vec![fold]);
    folds.set_collapsed(fold, true);
    let mut viewport =
        ViewportModel::new_wrapped_with_fold_indicator_columns(&buffer, &folds, 10, 4, 6);
    assert_eq!(row_texts(&buffer, &viewport), ["abcd", "ef", "tail"]);
    buffer.replace_range(
        EditorRange::new(
            EditorPosition::new(0, 0),
            EditorPosition::new(0, usize::MAX),
        ),
        "abcdefghi",
    );

    assert!(viewport.reflow_wrapped_lines(&buffer, &folds, 0, 0));
    assert_eq!(
        viewport,
        ViewportModel::new_wrapped_with_fold_indicator_columns(&buffer, &folds, 10, 4, 6,)
    );
    viewport.sync_unfolded_wrapped_buffer(&buffer);
    assert_eq!(
        viewport,
        ViewportModel::new_wrapped_with_fold_indicator_columns(
            &buffer,
            &FoldModel::default(),
            10,
            4,
            6,
        )
    );
}
