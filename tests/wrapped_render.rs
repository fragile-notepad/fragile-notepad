use fragile_notepad::editor::{
    DecorationModel, DecorationSettings, EditorBuffer, EditorLayout, EditorMetrics, EditorPosition,
    EditorSelection, FoldModel, FoldRange, RenderPlan, ScrollOffset, SelectionSet, SyntaxLineCache,
    ViewportModel, build_render_plan_for_selection_set_with_cache_and_caret_row,
};

fn render(
    text: &str,
    columns: usize,
    selection: EditorSelection,
    first_row: usize,
    caret_row: Option<usize>,
) -> RenderPlan {
    let buffer = EditorBuffer::from_text(text);
    let folds = FoldModel::default();
    let viewport = ViewportModel::new_wrapped(&buffer, &folds, columns, 4);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    build_render_plan_for_selection_set_with_cache_and_caret_row(
        &buffer,
        &viewport,
        &decorations,
        SelectionSet::single(selection),
        EditorLayout::new(
            EditorMetrics::default(),
            ScrollOffset {
                first_visible_row: first_row,
                horizontal_px: 900.0,
            },
            640.0,
            200.0,
        ),
        &SyntaxLineCache::default(),
        caret_row,
    )
}

fn caret(line: usize, column: usize) -> EditorSelection {
    let position = EditorPosition::new(line, column);
    EditorSelection::new(position, position)
}

#[test]
fn wrapped_render_preserves_text_and_numbers_only_the_first_row() {
    let plan = render("abcdefghij\n\nlast", 4, caret(0, 0), 0, None);
    assert_eq!(
        plan.rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>(),
        ["abcd", "efgh", "ij", "", "last"]
    );
    assert_eq!(
        plan.rows
            .iter()
            .map(|row| row.line_number)
            .collect::<Vec<_>>(),
        [Some(1), None, None, Some(2), Some(3)]
    );
    assert_eq!(plan.rows[1].start_column, 4);
    assert_eq!(plan.rows[1].start_visual_column, 4);
    assert!(
        plan.rows
            .iter()
            .all(|row| row.text_x == plan.rows[0].text_x)
    );
    assert!(
        plan.rows[0].text_x > 0.0,
        "wrapped text ignores stale horizontal scroll"
    );
}

#[test]
fn wrapped_selection_intersects_each_screen_row_once() {
    let plan = render(
        "abcdefghij",
        4,
        EditorSelection::new(EditorPosition::new(0, 2), EditorPosition::new(0, 9)),
        0,
        None,
    );
    assert_eq!(plan.selections.len(), 3);
    assert_eq!(
        plan.selections
            .iter()
            .map(|part| (part.start_column, part.end_column))
            .collect::<Vec<_>>(),
        [(2, 4), (4, 8), (8, 9)]
    );
    assert_eq!(
        plan.selections
            .iter()
            .map(|part| part.width)
            .collect::<Vec<_>>(),
        [16.0, 32.0, 8.0]
    );
    assert_eq!(plan.selections[1].x, plan.rows[1].text_x);
    assert_eq!(plan.selections[2].y, plan.rows[2].y);
}

#[test]
fn wrapped_caret_uses_continuation_row_and_boundary_affinity() {
    let downstream = render("abcdefghij", 4, caret(0, 4), 0, None);
    assert_eq!(downstream.carets.len(), 1);
    let cursor = downstream.caret.unwrap();
    assert_eq!(cursor.y, downstream.rows[1].y);
    assert_eq!(cursor.x, downstream.rows[1].text_x);
    let upstream = render("abcdefghij", 4, caret(0, 4), 0, Some(0));
    let cursor = upstream.caret.unwrap();
    assert_eq!(cursor.y, upstream.rows[0].y);
    assert_eq!(cursor.x, upstream.rows[0].text_x + 32.0);
    assert_eq!(upstream.carets, vec![cursor]);
}

#[test]
fn wrapped_caret_and_selection_remain_visible_when_header_row_is_above_viewport() {
    let plan = render("abcdefghijklmnop", 4, caret(0, 10), 2, None);
    assert_eq!(plan.rows[0].text, "ijkl");
    assert_eq!(plan.caret.unwrap().y, plan.rows[0].y);
    assert_eq!(plan.caret.unwrap().x, plan.rows[0].text_x + 16.0);
    let selection = render(
        "abcdefghijklmnop",
        4,
        EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 15)),
        2,
        None,
    );
    assert_eq!(selection.selections.len(), 2);
    assert_eq!(selection.selections[0].start_column, 8);
}

#[test]
fn wrapped_syntax_spans_and_end_markers_follow_fragments() {
    let buffer = EditorBuffer::from_text("// colorful comment spread across rows\n");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new_wrapped(&buffer, &folds, 10, 4);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            show_end_of_line_markers: true,
            ..DecorationSettings::default()
        },
        buffer.line_count(),
        &folds,
        vec![],
    );
    let cache = SyntaxLineCache::rebuild(
        &buffer,
        &iced::highlighter::Settings {
            token: "rs".into(),
            theme: iced::highlighter::Theme::InspiredGitHub,
        },
    );
    let plan = build_render_plan_for_selection_set_with_cache_and_caret_row(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0).into(),
        EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 640.0, 300.0),
        &cache,
        None,
    );
    let comment_rows = plan
        .rows
        .iter()
        .filter(|row| row.line == 0)
        .collect::<Vec<_>>();
    assert!(comment_rows.len() > 1);
    for row in &comment_rows {
        assert!(!row.syntax_spans.is_empty());
        assert!(
            row.syntax_spans
                .iter()
                .all(|span| span.range.end <= row.text.len()
                    && row.text.is_char_boundary(span.range.start)
                    && row.text.is_char_boundary(span.range.end))
        );
    }
    assert_eq!(
        comment_rows.iter().filter(|row| row.eol.is_some()).count(),
        1
    );
    assert!(comment_rows.last().unwrap().eol.is_some());
}

#[test]
fn wrapped_fold_control_and_ellipsis_occupy_first_and_last_header_rows() {
    let buffer = EditorBuffer::from_text("a_very_long_header {\n    hidden();\n}\nafter");
    let fold = FoldRange::new(0, 2);
    let mut folds = FoldModel::new(vec![fold]);
    folds.set_collapsed(fold, true);
    let viewport = ViewportModel::new_wrapped(&buffer, &folds, 12, 4);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let plan = build_render_plan_for_selection_set_with_cache_and_caret_row(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0).into(),
        EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 640.0, 300.0),
        &SyntaxLineCache::default(),
        None,
    );
    let header = plan
        .rows
        .iter()
        .filter(|row| row.line == 0)
        .collect::<Vec<_>>();
    assert!(header.len() > 1);
    assert!(header[0].fold.is_some());
    assert!(header[1..].iter().all(|row| row.fold.is_none()));
    assert_eq!(
        header
            .iter()
            .filter(|row| row.hidden_lines.is_some())
            .count(),
        1
    );
    assert!(header.last().unwrap().hidden_lines.is_some());
    assert!(plan.rows.iter().all(|row| row.line != 1 && row.line != 2));
}

#[test]
fn wrapped_tab_markers_keep_original_tab_stops() {
    let plan = render("abcde\txy", 5, caret(0, 6), 0, None);
    let tab_row = plan
        .rows
        .iter()
        .find(|row| row.text.contains('\t'))
        .unwrap();
    assert_eq!(tab_row.start_visual_column, 5);
    assert_eq!(
        plan.caret.unwrap().x,
        tab_row.text_x + 3.0 * EditorMetrics::default().character_width
    );
}

#[test]
fn wrapped_collapsed_badges_and_eol_markers_fit_at_minimum_zoom() {
    use fragile_notepad::core::{Document, DocumentId};
    use fragile_notepad::editor::layout::caret_x;

    for show_eol in [false, true] {
        for header_len in [35, 36, 70, 72] {
            let mut document = Document::from_path(
                DocumentId::new(1),
                "wrapped.txt",
                &format!("{}\nhidden\n}}", "x".repeat(header_len)),
            );
            let fold = FoldRange::new(0, 2);
            document.folds = FoldModel::new(vec![fold]);
            document.folds.set_collapsed(fold, true);
            let mut settings = document.decorations.settings;
            settings.show_end_of_line_markers = show_eol;
            document.set_decoration_settings(settings);
            let metrics = EditorMetrics::new(10.0, 4.4);
            let text_width = 178.0;
            document.update_viewport_geometry(10, text_width, metrics.character_width);
            document.set_word_wrap(true);
            document.refresh_view_models();
            let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 300.0, 200.0);
            let plan = build_render_plan_for_selection_set_with_cache_and_caret_row(
                &document.buffer,
                &document.viewport,
                &document.decorations,
                caret(0, 0).into(),
                layout,
                &SyntaxLineCache::default(),
                None,
            );
            let header = plan
                .rows
                .iter()
                .find(|row| row.hidden_lines.is_some())
                .unwrap();
            let end_x = caret_x(
                &header.text,
                header.text.len(),
                layout,
                &document.decorations,
            );
            let badge = header.collapsed_indicator_bounds(metrics, end_x).unwrap();
            assert!(
                badge.x + badge.width <= metrics.text_origin_x(&document.decorations) + text_width,
                "badge is clipped at 50% zoom: header_len={header_len}, show_eol={show_eol}, badge={badge:?}"
            );
        }
    }
}

#[test]
fn wrapped_zoom_reflows_badge_reservation_when_text_column_budget_is_unchanged() {
    use fragile_notepad::core::{Document, DocumentId};

    let mut document = Document::from_path(
        DocumentId::new(1),
        "wrapped.txt",
        &format!("{}\nhidden", "x".repeat(34)),
    );
    let fold = FoldRange::new(0, 1);
    document.folds = FoldModel::new(vec![fold]);
    document.folds.set_collapsed(fold, true);
    document.update_viewport_geometry(10, 40.0 * 8.0 + 2.0, 8.0);
    document.set_word_wrap(true);
    assert_eq!(document.viewport.visible_row_count(), 1);
    assert_eq!(document.viewport.fold_indicator_columns(), 6);

    document.update_viewport_geometry(10, 40.0 * 4.4 + 2.0, 4.4);

    assert_eq!(document.viewport.wrap_columns(), Some(40));
    assert_eq!(document.viewport.fold_indicator_columns(), 7);
    assert_eq!(document.viewport.visible_row_count(), 2);
}
