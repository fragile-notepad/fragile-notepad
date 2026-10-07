use fragile_notepad::editor::render::collapsed_fold_indicator_reservation;
use fragile_notepad::editor::{
    DecorationModel, DecorationSettings, EditorBuffer, EditorLayout, EditorMetrics, EditorPosition,
    EditorSelection, FoldModel, FoldRange, IndentBraceFoldProvider, ProjectionFragment,
    ScrollOffset, SyntaxLineCache, ViewportModel, build_render_plan_with_cache,
};

const BRANCHES: &str = "let mode = if cfg!(feature = \"abort\") {\n    abort();\n} else {\n    unwind();\n};\nafter();";

fn collapsed_model(source: &str, ranges: &[(usize, usize)]) -> (EditorBuffer, FoldModel) {
    let buffer = EditorBuffer::from_text(source);
    let mut folds = IndentBraceFoldProvider::for_syntax(4, "rs").compute_fold_model(&buffer);
    for &(start, end) in ranges {
        assert!(folds.set_collapsed(FoldRange::new(start, end), true));
    }
    (buffer, folds)
}

fn visible_text(buffer: &EditorBuffer, viewport: &ViewportModel) -> Vec<String> {
    viewport
        .visible_rows()
        .map(|row| {
            let text = viewport.display_text(row.document_line, buffer);
            let segment = viewport.row_segment(row.row, buffer).unwrap();
            text[segment.start_column..segment.end_column].to_owned()
        })
        .collect()
}

#[test]
fn collapsing_either_or_both_branches_preserves_the_other_branch_and_semicolon() {
    for (ranges, expected) in [
        (
            &[(0, 2)][..],
            vec![
                "let mode = if cfg!(feature = \"abort\") {...} else {",
                "    unwind();",
                "};",
                "after();",
            ],
        ),
        (
            &[(2, 4)][..],
            vec![
                "let mode = if cfg!(feature = \"abort\") {",
                "    abort();",
                "} else {...};",
                "after();",
            ],
        ),
        (
            &[(0, 2), (2, 4)][..],
            vec![
                "let mode = if cfg!(feature = \"abort\") {...} else {...};",
                "after();",
            ],
        ),
    ] {
        let (buffer, mut folds) = collapsed_model(BRANCHES, ranges);
        let viewport = ViewportModel::new_with_buffer(&buffer, &folds, 4);
        assert_eq!(visible_text(&buffer, &viewport), expected);
        assert_eq!(buffer.text(), BRANCHES);
        folds.set_all_collapsed(false);
        let expanded = ViewportModel::new_with_buffer(&buffer, &folds, 4);
        assert_eq!(
            visible_text(&buffer, &expanded),
            BRANCHES.lines().collect::<Vec<_>>()
        );
        assert_eq!(buffer.text(), BRANCHES);
    }
}

#[test]
fn chained_else_if_comments_and_commas_remain_visible() {
    for (source, ranges, expected) in [
        (
            "if first {\n    one();\n} else if second {\n    two();\n} else {\n    three();\n}\nafter();",
            &[(0, 2), (2, 4), (4, 6)][..],
            "if first {...} else if second {...} else {...}",
        ),
        (
            "let value = {\n    build()\n}, // 保留注释 🦀\nafter();",
            &[(0, 2)][..],
            "let value = {...}, // 保留注释 🦀",
        ),
        (
            "let value = [\n    item()\n]; // keep\nafter();",
            &[(0, 2)][..],
            "let value = [...]; // keep",
        ),
        (
            "call(\n    value\n); // keep\nafter();",
            &[(0, 2)][..],
            "call(...); // keep",
        ),
    ] {
        let (buffer, folds) = collapsed_model(source, ranges);
        let viewport = ViewportModel::new_with_buffer(&buffer, &folds, 4);
        assert_eq!(visible_text(&buffer, &viewport), [expected, "after();"]);
        assert_eq!(buffer.text(), source);
    }
}

#[test]
fn projected_suffix_positions_round_trip_to_the_original_source_bytes() {
    let source = "let value = {\n    build()\n}, // 保留注释 🦀\nafter();";
    let (buffer, folds) = collapsed_model(source, &[(0, 2)]);
    let viewport = ViewportModel::new_with_buffer(&buffer, &folds, 4);
    let prefix = "let value = ".len();
    let suffix_display = prefix + "{...}".len();
    let suffix = ", // 保留注释 🦀";
    for offset in suffix
        .char_indices()
        .map(|(offset, _)| offset)
        .chain([suffix.len()])
    {
        let source = EditorPosition::new(2, 1 + offset);
        let display = EditorPosition::new(0, suffix_display + offset);
        assert_eq!(viewport.display_position(source), Some(display));
        assert_eq!(viewport.source_position(display), source);
    }
    let opener = EditorPosition::new(0, prefix);
    for column in prefix..suffix_display {
        assert_eq!(
            viewport.source_position(EditorPosition::new(0, column)),
            opener
        );
    }
    assert_eq!(
        viewport.source_position(EditorPosition::new(0, suffix_display)),
        EditorPosition::new(2, 1)
    );
    assert_eq!(viewport.display_position(EditorPosition::new(1, 4)), None);
}

#[test]
fn wrapped_unicode_suffixes_preserve_text_source_mapping_and_whole_placeholders_at_all_zooms() {
    let source = "let value = {\n    build()\n}, // Unicode 保留注释 🦀 and a long trailing comment\nafter();";
    let expected = "let value = {...}, // Unicode 保留注释 🦀 and a long trailing comment";
    let (buffer, folds) = collapsed_model(source, &[(0, 2)]);
    for zoom in [0.5, 1.0, 3.0] {
        let metrics = EditorMetrics::new(20.0 * zoom, 8.8 * zoom);
        let reservation = (collapsed_fold_indicator_reservation(metrics.character_width)
            / metrics.character_width)
            .ceil() as usize;
        let viewport = ViewportModel::new_wrapped_with_fold_indicator_columns(
            &buffer,
            &folds,
            12,
            4,
            reservation,
        );
        let rows = viewport
            .visible_rows()
            .filter(|row| row.document_line == 0)
            .map(|row| {
                let text = viewport.display_text(0, &buffer);
                let segment = viewport.row_segment(row.row, &buffer).unwrap();
                text[segment.start_column..segment.end_column].to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(rows.concat(), expected);
        assert_eq!(rows.iter().filter(|text| text.contains("{...}")).count(), 1);
        let closing = buffer.line(2).unwrap();
        for column in closing
            .char_indices()
            .map(|(offset, _)| offset)
            .filter(|&column| column >= 1)
            .chain([closing.len()])
        {
            let original = EditorPosition::new(2, column);
            let display = viewport.display_position(original).expect("Visible suffix");
            assert_eq!(viewport.source_position(display), original);
            let expected_row = viewport
                .visible_rows()
                .filter(|row| row.document_line == display.line)
                .filter(|row| {
                    viewport.row_segment(row.row, &buffer).unwrap().start_column <= display.column
                })
                .map(|row| row.row)
                .last()
                .unwrap();
            assert_eq!(
                viewport.position_to_visible_row(original),
                Some(expected_row)
            );
        }
        assert_eq!(buffer.text(), source);
    }
}

#[test]
fn an_outer_collapse_hides_nested_placeholders_and_controls_until_it_expands() {
    let source = "let outer = {\n    if first {\n        one();\n    } else {\n        two();\n    }\n};\nafter();";
    let (buffer, mut folds) = collapsed_model(source, &[(0, 6), (1, 3), (3, 5)]);
    let viewport = ViewportModel::new_with_buffer(&buffer, &folds, 4);
    assert_eq!(
        visible_text(&buffer, &viewport),
        ["let outer = {...};", "after();"]
    );
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
        EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 640.0, 320.0),
        &SyntaxLineCache::default(),
    );
    let placeholders = plan
        .rows
        .iter()
        .flat_map(|row| &row.projection)
        .filter_map(|fragment| match fragment {
            ProjectionFragment::Placeholder { range, .. } => Some(*range),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(placeholders, [FoldRange::new(0, 6)]);
    assert_eq!(plan.rows.iter().filter(|row| row.fold.is_some()).count(), 1);
    folds.set_collapsed(FoldRange::new(0, 6), false);
    let inner = ViewportModel::new_with_buffer(&buffer, &folds, 4);
    assert_eq!(
        visible_text(&buffer, &inner),
        [
            "let outer = {",
            "    if first {...} else {...}",
            "};",
            "after();"
        ]
    );
    assert_eq!(buffer.text(), source);
}
