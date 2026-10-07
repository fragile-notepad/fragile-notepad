use fragile_notepad::editor::layout::{caret_x, scrolled_text_origin_x};
use fragile_notepad::editor::render::{
    collapsed_delimiter_indicator_bounds, collapsed_fold_indicator_bounds,
    collapsed_fold_indicator_reservation,
};
use fragile_notepad::editor::{
    DecorationModel, DecorationSettings, EditorBuffer, EditorLayout, EditorMetrics, EditorPosition,
    EditorSelection, FoldRange, IndentBraceFoldProvider, RenderPlan, ScrollOffset, SyntaxLineCache,
    ViewportModel, build_render_plan_with_cache, planned_text_draws,
};
use iced::Rectangle;

fn plan_for(
    text: &str,
    collapsed: bool,
    settings: DecorationSettings,
    layout: EditorLayout,
) -> (RenderPlan, DecorationModel) {
    plan_for_range(
        text,
        FoldRange::new(0, EditorBuffer::from_text(text).line_count() - 1),
        collapsed,
        settings,
        layout,
    )
}

fn plan_for_range(
    text: &str,
    range: FoldRange,
    collapsed: bool,
    settings: DecorationSettings,
    layout: EditorLayout,
) -> (RenderPlan, DecorationModel) {
    let buffer = EditorBuffer::from_text(text);
    let mut folds = IndentBraceFoldProvider::for_syntax(4, "rs").compute_fold_model(&buffer);
    assert!(folds.ranges().contains(&range), "Fixture fold {range:?}");
    folds.set_collapsed(range, collapsed);
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(settings, buffer.line_count(), &folds, vec![]);
    let selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0));
    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        selection,
        layout,
        &SyntaxLineCache::default(),
    );

    (plan, decorations)
}

fn default_layout() -> EditorLayout {
    EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 640.0, 320.0)
}

#[test]
fn collapsed_block_has_one_inline_indicator_and_expanded_block_has_none() {
    for collapsed in [false, true] {
        let layout = default_layout();
        let (plan, _) = plan_for(
            "fn main() {\n    run();\n}",
            collapsed,
            DecorationSettings::default(),
            layout,
        );
        let indicators = plan
            .rows
            .iter()
            .filter(|row| {
                row.collapsed_indicator_bounds(layout.metrics, 180.0)
                    .is_some()
            })
            .count();

        assert_eq!(indicators, usize::from(collapsed));
        assert_eq!(planned_text_draws(&plan, true), 1);
    }
}

#[test]
fn collapsed_indicator_survives_hidden_gutter_controls() {
    let layout = default_layout();
    let (plan, _) = plan_for(
        "{\n}",
        true,
        DecorationSettings {
            show_folding_controls: false,
            ..DecorationSettings::default()
        },
        layout,
    );

    assert!(plan.rows[0].fold.is_none());
    assert!(
        plan.rows[0]
            .collapsed_indicator_bounds(layout.metrics, 100.0)
            .is_some()
    );
}

#[test]
fn delimiter_indicator_replaces_the_opener_at_its_measured_position() {
    let layout = default_layout();
    let (plan, _) = plan_for("\t字 {\n}", true, DecorationSettings::default(), layout);
    let measured_opener_x = 173.25;
    let indicator = plan.rows[0]
        .collapsed_indicator_bounds(layout.metrics, measured_opener_x)
        .expect("collapsed indicator");
    let expected =
        collapsed_delimiter_indicator_bounds(layout.metrics, plan.rows[0].y, measured_opener_x);

    assert_eq!(plan.rows[0].text, "\t字 {");
    assert_eq!(plan.rows[0].collapsed_indicator_column(), "\t字 ".len());
    assert_eq!(indicator, expected);
    assert_eq!(indicator.x, measured_opener_x);
    let (with_eol, _) = plan_for(
        "\t字 {\n}",
        true,
        DecorationSettings {
            show_end_of_line_markers: true,
            ..DecorationSettings::default()
        },
        layout,
    );
    assert_eq!(
        with_eol.rows[0].collapsed_indicator_bounds(layout.metrics, measured_opener_x),
        Some(indicator)
    );
}

#[test]
fn collapsed_indicator_moves_with_horizontal_scroll_and_stays_after_long_header() {
    let unscrolled = default_layout();
    let scrolled = EditorLayout {
        scroll: ScrollOffset {
            first_visible_row: 0,
            horizontal_px: 160.0,
        },
        ..unscrolled
    };
    let header = format!("\t{} {{", "x".repeat(100));
    let (plan, decorations) = plan_for(
        &format!("{header}\n}}"),
        true,
        DecorationSettings::default(),
        unscrolled,
    );
    let row = &plan.rows[0];
    let indicator_at = |layout: EditorLayout| {
        row.collapsed_indicator_bounds(
            layout.metrics,
            caret_x(
                &header,
                row.collapsed_indicator_column(),
                layout,
                &decorations,
            ),
        )
        .expect("collapsed indicator")
    };
    let before = indicator_at(unscrolled);
    let after = indicator_at(scrolled);

    assert_eq!(before.x - after.x, scrolled.scroll.horizontal_px);
    assert!(
        before.x > unscrolled.width,
        "long headers retain their true endpoint"
    );
    assert!(before.x > scrolled_text_origin_x(unscrolled, &decorations));
}

#[test]
fn only_parser_matched_terminal_delimiters_replace_source_syntax() {
    for (source, opening) in [
        ("fn main() {\n    run();\n}", '{'),
        ("fn main() { \t\n    run();\n}", '{'),
        ("let entries = [\n    1,\n];", '['),
        ("call(\n    value\n);", '('),
    ] {
        let (plan, _) = plan_for(
            source,
            true,
            DecorationSettings::default(),
            default_layout(),
        );
        let row = &plan.rows[0];
        let delimiter = row
            .hidden_lines
            .unwrap()
            .delimiter
            .expect("Matched delimiter");
        assert_eq!(delimiter.opening, opening);
        assert_eq!(delimiter.opening_column, row.text.find(opening).unwrap());
        assert_eq!(row.collapsed_indicator_column(), delimiter.opening_column);
        assert_eq!(row.text, source.lines().next().unwrap());
    }

    for source in [
        "// {\n    }",
        "let text = \"{\n    }\";",
        "let text = r#\"{\n    }\"#;",
        "if ready {\n    run();",
        "if ready:\n    run();",
    ] {
        let (plan, _) = plan_for(
            source,
            true,
            DecorationSettings::default(),
            default_layout(),
        );
        let row = &plan.rows[0];
        assert!(
            row.hidden_lines.unwrap().delimiter.is_none(),
            "Source {source:?}"
        );
        assert_eq!(row.collapsed_indicator_column(), row.text.len());
        assert_eq!(
            row.collapsed_indicator_bounds(default_layout().metrics, 180.0),
            Some(collapsed_fold_indicator_bounds(
                default_layout().metrics,
                row.y,
                180.0,
                false
            ))
        );
    }

    for (source, range) in [
        ("fn main() {\n    run();\n}", FoldRange::new(0, 1)),
        ("fn main() { // header\n    run();\n}", FoldRange::new(0, 2)),
    ] {
        let (plan, _) = plan_for_range(
            source,
            range,
            true,
            DecorationSettings::default(),
            default_layout(),
        );
        assert!(plan.rows[0].hidden_lines.unwrap().delimiter.is_none());
        assert_eq!(
            plan.rows[0].collapsed_indicator_column(),
            plan.rows[0].text.len()
        );
    }
}

#[test]
fn wrapped_delimiter_uses_the_opener_in_the_final_fragment_and_fits_the_reserved_width() {
    let source = "fn wrapped_thing() {\n    run();\n}";
    let buffer = EditorBuffer::from_text(source);
    let mut folds = IndentBraceFoldProvider::for_syntax(4, "rs").compute_fold_model(&buffer);
    folds.set_collapsed(FoldRange::new(0, 2), true);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    for zoom in [0.5, 1.0, 3.0] {
        let metrics = EditorMetrics::new(20.0 * zoom, 8.8 * zoom);
        let reservation = (collapsed_fold_indicator_reservation(metrics.character_width)
            / metrics.character_width)
            .ceil() as usize;
        let viewport = ViewportModel::new_wrapped_with_fold_indicator_columns(
            &buffer,
            &folds,
            8,
            4,
            reservation,
        );
        let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 500.0, 600.0);
        let plan = build_render_plan_with_cache(
            &buffer,
            &viewport,
            &decorations,
            EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
            layout,
            &SyntaxLineCache::default(),
        );
        let row = plan
            .rows
            .iter()
            .find(|row| row.hidden_lines.is_some())
            .unwrap();
        let column = row.collapsed_indicator_column();
        assert_eq!(row.collapsed_delimiter().unwrap().opening, '{');
        assert_eq!(row.start_column + column, source.find('{').unwrap());
        assert_eq!(&row.text[column..], "{");
        let anchor = caret_x(&row.text, column, layout, &decorations);
        let bounds = row.collapsed_indicator_bounds(metrics, anchor).unwrap();
        assert_eq!(bounds.x, anchor);
        assert!(
            bounds.x + bounds.width
                <= metrics.text_origin_x(&decorations) + 8.0 * metrics.character_width
        );
    }
}

#[test]
fn collapsed_indicator_fits_inside_its_row_and_scales_with_zoom() {
    for line_height in [10.0, 18.0, 36.0, 72.0] {
        let metrics = EditorMetrics::new(line_height, line_height * 0.45);
        let row_y = 40.0;
        for indicator in [
            collapsed_fold_indicator_bounds(metrics, row_y, 180.0, false),
            collapsed_delimiter_indicator_bounds(metrics, row_y, 180.0),
        ] {
            assert!(indicator.y >= row_y);
            assert!(indicator.y + indicator.height <= row_y + line_height);
            assert_eq!(indicator.center_y(), row_y + line_height / 2.0);
            assert!(indicator.width >= metrics.character_width * 2.0);
            assert!(indicator.height > 0.0);
        }
    }
}

#[test]
fn indicator_near_viewport_edge_is_clipped_without_shifting_onto_text() {
    let metrics = EditorMetrics::default();
    let clip = Rectangle {
        x: 80.0,
        y: 0.0,
        width: 220.0,
        height: 60.0,
    };
    let partially_visible = collapsed_fold_indicator_bounds(metrics, 4.0, 280.0, false);
    let hidden = collapsed_fold_indicator_bounds(metrics, 4.0, 320.0, false);

    assert!(partially_visible.intersects(&clip));
    assert!(partially_visible.x + partially_visible.width > clip.x + clip.width);
    assert!(!hidden.intersects(&clip));
    assert!(hidden.x > 320.0);
}
