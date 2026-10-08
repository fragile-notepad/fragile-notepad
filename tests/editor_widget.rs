use fragile_notepad::core::{
    KeyBinding, ShortcutCommand, ShortcutDisplayPart, ShortcutKey, ShortcutMap,
    ShortcutModifierIcon, ShortcutModifiers,
};
use fragile_notepad::editor::layout::{
    byte_column_for, byte_column_for_visual_column, text_area_bounds, visual_column_for,
    visual_column_for_byte_column,
};
use fragile_notepad::editor::widget::{
    EDITOR_TEXT_SHAPING, key_action, scrollbar_row_for_position, vertical_scrollbar_geometry,
};
use fragile_notepad::editor::{
    CaretMotion, DecorationModel, DecorationSettings, EditorAction, EditorBuffer, EditorLayout,
    EditorMetrics, EditorPosition, EditorSelection, FoldModel, FoldRange, HitTarget, IndentGuide,
    ScrollOffset, SelectionSet, SyntaxLineCache, ViewportModel, build_render_plan,
    build_render_plan_for_selection_set_with_cache, build_render_plan_with_cache, hit_test,
    line_number_left_x, line_number_text_x, planned_text_draws, planned_text_draws_with_markers,
};
use iced::Rectangle;
use iced::advanced::text;
use iced::highlighter;
use iced::keyboard::{self, key};

fn caret(line: usize, column: usize) -> EditorSelection {
    let position = EditorPosition::new(line, column);

    EditorSelection::new(position, position)
}

#[test]
fn document_boundary_keys_preserve_shift_selection() {
    let shortcuts = ShortcutMap::default();
    for (named, motion) in [
        (key::Named::Home, CaretMotion::DocumentStart),
        (key::Named::End, CaretMotion::DocumentEnd),
    ] {
        let key = keyboard::Key::Named(named);
        assert_eq!(
            key_action(&key, &key, keyboard::Modifiers::COMMAND, None, &shortcuts),
            Some(EditorAction::MoveCaret(motion))
        );
        assert_eq!(
            key_action(
                &key,
                &key,
                keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
                None,
                &shortcuts
            ),
            Some(EditorAction::Select(motion))
        );
    }
}

#[test]
fn every_default_shortcut_resolves_to_its_own_command() {
    let shortcuts = ShortcutMap::default();
    for command in ShortcutCommand::ALL {
        let Some(binding) = shortcuts.binding(command) else {
            continue;
        };
        let mut modifiers = keyboard::Modifiers::empty();
        if binding.modifiers.primary {
            modifiers |= keyboard::Modifiers::COMMAND;
        }
        if binding.modifiers.ctrl {
            modifiers |= keyboard::Modifiers::CTRL;
        }
        if binding.modifiers.shift {
            modifiers |= keyboard::Modifiers::SHIFT;
        }
        if binding.modifiers.alt {
            modifiers |= keyboard::Modifiers::ALT;
        }
        if binding.modifiers.logo {
            modifiers |= keyboard::Modifiers::LOGO;
        }
        let key = match binding.key {
            ShortcutKey::Character(ch) => keyboard::Key::Character(ch.to_string().into()),
            _ => continue,
        };
        assert_eq!(
            shortcuts.resolve(&key, &key, modifiers),
            Some(command),
            "{command:?}"
        );
    }
}

fn syntax_settings(token: &str) -> highlighter::Settings {
    highlighter::Settings {
        token: token.to_owned(),
        theme: highlighter::Theme::InspiredGitHub,
    }
}

#[test]
fn editor_widget_text_area_bounds_exclude_gutter_for_clipped_rendering() {
    let folds = FoldModel::default();
    let decorations = DecorationModel::from_folds(DecorationSettings::default(), 1, &folds, vec![]);
    let metrics = EditorMetrics {
        padding_left: 8.0,
        line_number_width: 52.0,
        fold_lane_width: 18.0,
        hidden_indicator_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 320.0, 120.0);
    let bounds = Rectangle {
        x: 12.0,
        y: 6.0,
        width: 320.0,
        height: 120.0,
    };
    let clip = text_area_bounds(bounds, layout, &decorations);
    let text_origin_x = metrics.text_origin_x(&decorations);

    assert_eq!(clip.x, bounds.x + text_origin_x);
    assert_eq!(clip.y, bounds.y);
    assert_eq!(clip.width, bounds.width - text_origin_x);
    assert_eq!(clip.height, bounds.height);
}

#[test]
fn editor_widget_vertical_scrollbar_reflects_visible_row_range() {
    let metrics = EditorMetrics {
        line_height: 20.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(
        metrics,
        ScrollOffset {
            first_visible_row: 25,
            horizontal_px: 0.0,
        },
        300.0,
        204.0,
    );

    let scrollbar = vertical_scrollbar_geometry(layout, 100)
        .expect("content taller than viewport should expose a scrollbar");

    assert_eq!(scrollbar.track.x, 288.0);
    assert_eq!(scrollbar.track.height, 200.0);
    assert!(scrollbar.thumb.height >= 24.0);
    assert!(scrollbar.thumb.y > scrollbar.track.y);
    assert!(
        scrollbar.thumb.y + scrollbar.thumb.height
            <= scrollbar.track.y + scrollbar.track.height + f32::EPSILON
    );
}

#[test]
fn editor_widget_scrollbar_position_maps_to_first_visible_row() {
    let layout = EditorLayout::new(
        EditorMetrics {
            line_height: 20.0,
            ..EditorMetrics::default()
        },
        ScrollOffset::ZERO,
        300.0,
        204.0,
    );
    let scrollbar = vertical_scrollbar_geometry(layout, 100).expect("scrollbar");

    assert_eq!(
        scrollbar_row_for_position(scrollbar.track.y, 0.0, layout, 100),
        0
    );
    assert_eq!(
        scrollbar_row_for_position(scrollbar.track.y + scrollbar.track.height, 0.0, layout, 100),
        90
    );
}

#[test]
fn editor_widget_hit_test_maps_text_points_to_document_byte_columns() {
    let buffer = EditorBuffer::from_text("alpha\nbeta");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics {
        line_height: 20.0,
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 400.0, 200.0);

    assert_eq!(
        hit_test(
            metrics.text_origin_x(&decorations) + 20.0,
            metrics.padding_top + 25.0,
            layout,
            &buffer,
            &viewport,
            &decorations,
        ),
        HitTarget::Text(EditorPosition::new(1, 2))
    );
}

#[test]
fn editor_widget_visual_columns_map_utf8_and_tabs_to_monospace_cells() {
    let text = "\u{00e9}\t\u{597d}";

    assert_eq!(visual_column_for_byte_column(text, "\u{00e9}".len()), 1);
    assert_eq!(visual_column_for_byte_column(text, "\u{00e9}\t".len()), 4);
    assert_eq!(
        visual_column_for_byte_column(text, "\u{00e9}\t\u{597d}".len()),
        6
    );
    assert_eq!(byte_column_for_visual_column(text, 0), 0);
    assert_eq!(byte_column_for_visual_column(text, 1), "\u{00e9}".len());
    assert_eq!(byte_column_for_visual_column(text, 3), "\u{00e9}".len());
    assert_eq!(byte_column_for_visual_column(text, 4), "\u{00e9}\t".len());
}

#[test]
fn editor_widget_visual_columns_use_configured_tab_width() {
    let text = "a\tb";

    assert_eq!(visual_column_for(text, "a\t".len(), 2), 2);
    assert_eq!(visual_column_for(text, "a\t".len(), 8), 8);
    assert_eq!(byte_column_for(text, 1, 2), 1);
    assert_eq!(byte_column_for(text, 2, 2), 2);
    assert_eq!(byte_column_for(text, 7, 8), 1);
    assert_eq!(byte_column_for(text, 8, 8), 2);
}

#[test]
fn editor_widget_hit_test_maps_visual_columns_to_utf8_byte_columns() {
    let buffer = EditorBuffer::from_text("\u{00e9}\u{6f02}\u{7d30}");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 400.0, 200.0);

    assert_eq!(
        hit_test(
            metrics.text_origin_x(&decorations) + 30.0,
            metrics.padding_top + 1.0,
            layout,
            &buffer,
            &viewport,
            &decorations,
        ),
        HitTarget::Text(EditorPosition::new(0, "\u{00e9}\u{6f02}".len()))
    );
}

#[test]
fn editor_widget_hit_test_uses_configured_tab_width() {
    let buffer = EditorBuffer::from_text("a\tb");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            indent_width: 8,
            ..DecorationSettings::default()
        },
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 400.0, 200.0);

    assert_eq!(
        hit_test(
            metrics.text_origin_x(&decorations) + 70.0,
            metrics.padding_top + 1.0,
            layout,
            &buffer,
            &viewport,
            &decorations,
        ),
        HitTarget::Text(EditorPosition::new(0, 1))
    );
    assert_eq!(
        hit_test(
            metrics.text_origin_x(&decorations) + 80.0,
            metrics.padding_top + 1.0,
            layout,
            &buffer,
            &viewport,
            &decorations,
        ),
        HitTarget::Text(EditorPosition::new(0, 2))
    );
}

#[test]
fn editor_widget_hit_test_identifies_fold_controls_in_gutter() {
    let buffer = EditorBuffer::from_text("root\n    child\nnext");
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 1)]);
    folds.set_collapsed(FoldRange::new(0, 1), true);
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics::default();
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 400.0, 200.0);
    let fold_x = metrics.padding_left + metrics.line_number_width + 1.0;

    assert_eq!(
        hit_test(
            fold_x,
            metrics.padding_top + 1.0,
            layout,
            &buffer,
            &viewport,
            &decorations,
        ),
        HitTarget::FoldControl {
            line: 0,
            range: FoldRange::new(0, 1),
        }
    );
}

#[test]
fn editor_widget_hit_test_identifies_expanded_fold_controls_in_gutter() {
    let buffer = EditorBuffer::from_text("root\n    child\nnext");
    let folds = FoldModel::new(vec![FoldRange::new(0, 1)]);
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics::default();
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 400.0, 200.0);
    let fold_x = metrics.padding_left + metrics.line_number_width + 1.0;

    assert_eq!(
        hit_test(
            fold_x,
            metrics.padding_top + 1.0,
            layout,
            &buffer,
            &viewport,
            &decorations,
        ),
        HitTarget::FoldControl {
            line: 0,
            range: FoldRange::new(0, 1),
        }
    );
}

#[test]
fn editor_widget_render_plan_contains_notepad_style_decoration_layers() {
    let buffer = EditorBuffer::from_text("fn main() {\n\tvalue \n}");
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 2)]);
    folds.set_collapsed(FoldRange::new(0, 2), true);
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let settings = DecorationSettings {
        show_spaces: true,
        show_tabs: true,
        show_end_of_line_markers: true,
        ..DecorationSettings::default()
    };
    let decorations = DecorationModel::from_folds(
        settings,
        buffer.line_count(),
        &folds,
        vec![IndentGuide { line: 0, depth: 1 }],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 3),
        layout,
        &syntax_settings("rs"),
    );
    let first_row = &plan.rows[0];

    assert_eq!(first_row.line_number, Some(1));
    assert!(first_row.is_active_line);
    assert_eq!(first_row.fold.map(|fold| fold.collapsed), Some(true));
    assert_eq!(
        first_row
            .hidden_lines
            .map(|hidden_lines| hidden_lines.hidden_line_count),
        Some(2)
    );
    assert!(first_row.eol.is_some());
    assert_eq!(first_row.indent_guides.len(), 1);
    assert!(plan.caret.is_some());
}

#[test]
fn editor_widget_indent_guides_use_configured_indent_width() {
    let buffer = EditorBuffer::from_text("root\n  child");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let settings = DecorationSettings {
        indent_width: 2,
        ..DecorationSettings::default()
    };
    let decorations = DecorationModel::from_folds(
        settings,
        buffer.line_count(),
        &folds,
        vec![IndentGuide { line: 1, depth: 1 }],
    );
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 500.0, 200.0);
    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(
        plan.rows[1].indent_guides[0].x,
        metrics.text_origin_x(&decorations) + 20.0
    );
}

#[test]
fn editor_widget_line_number_origin_uses_right_edge_anchor() {
    let metrics = EditorMetrics {
        padding_left: 8.0,
        character_width: 10.0,
        line_number_width: 52.0,
        ..EditorMetrics::default()
    };

    assert_eq!(line_number_text_x(metrics), 54.0);
    assert_eq!(line_number_left_x(metrics, metrics.character_width), 44.0);
    assert_eq!(
        line_number_left_x(metrics, metrics.character_width * 2.0),
        34.0
    );
}

#[test]
fn editor_widget_render_plan_updates_line_numbers_after_scroll() {
    let text = (1..=30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let buffer = EditorBuffer::from_text(text);
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(
        EditorMetrics::default(),
        ScrollOffset {
            first_visible_row: 10,
            horizontal_px: 0.0,
        },
        500.0,
        200.0,
    );

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(10, 0),
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(plan.rows.first().map(|row| row.visible_row), Some(10));
    assert_eq!(plan.rows.first().map(|row| row.line), Some(10));
    assert_eq!(plan.rows.first().and_then(|row| row.line_number), Some(11));
    assert!(plan.rows.iter().any(|row| row.line_number == Some(20)));
}

#[test]
fn editor_widget_uses_auto_shaping_for_unicode_font_fallback() {
    assert_eq!(EDITOR_TEXT_SHAPING, text::Shaping::Auto);
}

#[test]
fn editor_widget_key_action_maps_keyboard_to_app_local_actions() {
    let shortcuts = ShortcutMap::default();

    assert_eq!(
        key_action(
            &keyboard::Key::Character("a".into()),
            &keyboard::Key::Character("a".into()),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::SelectAll)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::ArrowRight),
            &keyboard::Key::Named(key::Named::ArrowRight),
            keyboard::Modifiers::SHIFT,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Select(CaretMotion::Right))
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::ArrowLeft),
            &keyboard::Key::Named(key::Named::ArrowLeft),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::MoveCaret(CaretMotion::WordLeft))
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::ArrowRight),
            &keyboard::Key::Named(key::Named::ArrowRight),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::MoveCaret(CaretMotion::WordRight))
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::ArrowLeft),
            &keyboard::Key::Named(key::Named::ArrowLeft),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Select(CaretMotion::WordLeft))
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::ArrowRight),
            &keyboard::Key::Named(key::Named::ArrowRight),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Select(CaretMotion::WordRight))
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Character("q".into()),
            &keyboard::Key::Character("q".into()),
            keyboard::Modifiers::NONE,
            Some("q"),
            &shortcuts,
        ),
        Some(EditorAction::InsertText("q".to_owned()))
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::Space),
            &keyboard::Key::Named(key::Named::Space),
            keyboard::Modifiers::NONE,
            None,
            &shortcuts,
        ),
        Some(EditorAction::InsertText(" ".to_owned()))
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::Tab),
            &keyboard::Key::Named(key::Named::Tab),
            keyboard::Modifiers::NONE,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Indent)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::Tab),
            &keyboard::Key::Named(key::Named::Tab),
            keyboard::Modifiers::SHIFT,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Unindent)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Character("v".into()),
            &keyboard::Key::Character("v".into()),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Paste)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Character("d".into()),
            &keyboard::Key::Character("d".into()),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::DuplicateLine)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::ArrowDown),
            &keyboard::Key::Named(key::Named::ArrowDown),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::ALT,
            None,
            &shortcuts,
        ),
        Some(EditorAction::AddCaretBelow)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Character("L".into()),
            &keyboard::Key::Character("L".into()),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
            None,
            &shortcuts,
        ),
        Some(EditorAction::SplitSelectionIntoLines)
    );
}

#[test]
fn editor_widget_hit_test_identifies_hidden_line_indicator() {
    let buffer = EditorBuffer::from_text("root\n    child\nnext");
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 1)]);
    folds.set_collapsed(FoldRange::new(0, 1), true);
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics::default();
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 400.0, 200.0);
    let x = metrics.padding_left + metrics.line_number_width + metrics.fold_lane_width + 1.0;

    assert_eq!(
        hit_test(
            x,
            metrics.padding_top + 1.0,
            layout,
            &buffer,
            &viewport,
            &decorations,
        ),
        HitTarget::HiddenLineIndicator { line: 0 }
    );
}

#[test]
fn editor_widget_render_plan_uses_consistent_horizontal_scroll_positions() {
    let buffer = EditorBuffer::from_text("  abc");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let settings = DecorationSettings {
        show_spaces: true,
        show_end_of_line_markers: true,
        ..DecorationSettings::default()
    };
    let decorations = DecorationModel::from_folds(
        settings,
        buffer.line_count(),
        &folds,
        vec![IndentGuide { line: 0, depth: 1 }],
    );
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let horizontal_px = 24.0;
    let layout = EditorLayout::new(
        metrics,
        ScrollOffset {
            first_visible_row: 0,
            horizontal_px,
        },
        500.0,
        200.0,
    );
    let selection = EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 4));
    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        selection,
        layout,
        &syntax_settings("txt"),
    );
    let row = &plan.rows[0];
    let expected_text_x = metrics.text_origin_x(&decorations) - horizontal_px;

    assert_eq!(row.text_x, expected_text_x);
    assert_eq!(row.whitespace[0].x, expected_text_x);
    assert_eq!(
        row.whitespace[1].x,
        expected_text_x + metrics.character_width
    );
    assert_eq!(
        row.indent_guides[0].x,
        expected_text_x + metrics.character_width * 4.0
    );
    assert_eq!(row.eol.map(|eol| eol.x), Some(expected_text_x + 50.0));
    assert_eq!(plan.selections[0].x, expected_text_x + 10.0);
    assert_eq!(plan.selections[0].width, 30.0);

    let caret_plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 4),
        layout,
        &syntax_settings("txt"),
    );
    assert_eq!(
        caret_plan.caret.map(|caret| caret.x),
        Some(expected_text_x + 40.0)
    );
}

#[test]
fn editor_widget_render_plan_uses_visual_columns_for_unicode_and_tabs() {
    let buffer = EditorBuffer::from_text("\u{00e9}\t\u{597d}");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let settings = DecorationSettings {
        show_tabs: true,
        show_end_of_line_markers: true,
        ..DecorationSettings::default()
    };
    let decorations = DecorationModel::from_folds(settings, buffer.line_count(), &folds, vec![]);
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 500.0, 200.0);
    let text_x = metrics.text_origin_x(&decorations);
    let tab_column = "\u{00e9}".len();

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, "\u{00e9}\t\u{597d}".len()),
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(plan.rows[0].whitespace[0].x, text_x + 10.0);
    assert_eq!(plan.rows[0].whitespace[0].column, tab_column);
    assert_eq!(plan.rows[0].eol.map(|eol| eol.x), Some(text_x + 60.0));
    assert_eq!(plan.caret.map(|caret| caret.x), Some(text_x + 60.0));
}

#[test]
fn editor_widget_render_plan_uses_configured_tab_width_for_caret_selection_and_markers() {
    let buffer = EditorBuffer::from_text("a\tb");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let settings = DecorationSettings {
        indent_width: 8,
        show_tabs: true,
        show_end_of_line_markers: true,
        ..DecorationSettings::default()
    };
    let decorations = DecorationModel::from_folds(settings, buffer.line_count(), &folds, vec![]);
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 500.0, 200.0);
    let text_x = metrics.text_origin_x(&decorations);
    let selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 2));

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        selection,
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(plan.rows[0].whitespace[0].x, text_x + 10.0);
    assert_eq!(plan.rows[0].eol.map(|eol| eol.x), Some(text_x + 90.0));
    assert_eq!(plan.selections[0].x, text_x);
    assert_eq!(plan.selections[0].width, 80.0);

    let caret_plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 2),
        layout,
        &syntax_settings("txt"),
    );
    assert_eq!(caret_plan.caret.map(|caret| caret.x), Some(text_x + 80.0));
}

#[test]
fn editor_widget_render_plan_exposes_visible_whitespace_eol_selection_and_caret() {
    let buffer = EditorBuffer::from_text("a b\n\tcd\nend");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let settings = DecorationSettings {
        show_spaces: true,
        show_tabs: true,
        show_end_of_line_markers: true,
        ..DecorationSettings::default()
    };
    let decorations = DecorationModel::from_folds(
        settings,
        buffer.line_count(),
        &folds,
        vec![IndentGuide { line: 1, depth: 1 }],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let selection = EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(1, 2));

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        selection,
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(plan.rows.len(), 3);
    assert_eq!(
        plan.rows[0].whitespace[0].kind,
        fragile_notepad::editor::WhitespaceKind::Space
    );
    assert!(plan.rows[0].eol.is_some());
    assert_eq!(
        plan.rows[1].whitespace[0].kind,
        fragile_notepad::editor::WhitespaceKind::Tab
    );
    assert_eq!(plan.rows[1].indent_guides.len(), 1);
    assert_eq!(plan.selections.len(), 2);
    assert_eq!(plan.selections[0].line, 0);
    assert_eq!(plan.selections[1].line, 1);
    assert!(plan.caret.is_none());

    let caret_plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(2, 3),
        layout,
        &syntax_settings("txt"),
    );
    assert_eq!(
        caret_plan.caret.map(|caret| caret.position),
        Some(EditorPosition::new(2, 3))
    );
}

#[test]
fn editor_widget_render_plan_marks_selected_empty_lines_without_carets() {
    let buffer = EditorBuffer::from_text("one\n\nthree");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 500.0, 200.0);
    let selection = EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(2, 2));

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        selection,
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(
        plan.selections
            .iter()
            .map(|selection| (selection.line, selection.width))
            .collect::<Vec<_>>(),
        vec![(0, 20.0), (1, 10.0), (2, 20.0)]
    );
    assert!(plan.caret.is_none());
    assert!(plan.carets.is_empty());
}

#[test]
fn editor_widget_render_plan_does_not_mark_terminal_empty_end_line() {
    let buffer = EditorBuffer::from_text("one\n\nthree");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(1, 0));

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        selection,
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(
        plan.selections
            .iter()
            .map(|selection| selection.line)
            .collect::<Vec<_>>(),
        vec![0]
    );
    assert!(plan.carets.is_empty());
}

#[test]
fn editor_widget_render_plan_supports_multiple_selection_ranges_with_main_caret() {
    let buffer = EditorBuffer::from_text("alpha\nbeta\ngamma");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();
    let selections = SelectionSet::from_ranges(
        vec![
            EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 3)),
            caret(2, 2),
            EditorSelection::new(EditorPosition::new(1, 0), EditorPosition::new(1, 4)),
        ],
        1,
    );

    let plan = build_render_plan_for_selection_set_with_cache(
        &buffer,
        &viewport,
        &decorations,
        selections,
        layout,
        &cache,
    );

    assert_eq!(plan.selections.len(), 2);
    assert_eq!(plan.selections[0].line, 0);
    assert_eq!(plan.selections[1].line, 1);
    assert_eq!(
        plan.caret.map(|caret| caret.position),
        Some(EditorPosition::new(2, 2))
    );
}

#[test]
fn editor_widget_render_plan_projects_rectangular_selection_visual_columns() {
    let buffer = EditorBuffer::from_text("a\tz\nshort\nabcdef");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            indent_width: 4,
            ..DecorationSettings::default()
        },
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();
    let selections =
        SelectionSet::rectangular(EditorPosition::new(0, 1), EditorPosition::new(2, 4), 2, 5);

    let plan = build_render_plan_for_selection_set_with_cache(
        &buffer,
        &viewport,
        &decorations,
        selections,
        layout,
        &cache,
    );

    assert_eq!(plan.selections.len(), 3);
    assert!(plan.selections.iter().all(|selection| {
        selection.start_visual_column == 2 && selection.end_visual_column == 5
    }));
    assert_eq!(
        plan.selections
            .iter()
            .map(|selection| selection.width)
            .collect::<Vec<_>>(),
        vec![30.0, 30.0, 30.0]
    );
}

#[test]
fn editor_widget_render_plan_preserves_rectangular_virtual_columns_beyond_line_end() {
    let buffer = EditorBuffer::from_text("a\nabcd");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics {
        character_width: 8.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();
    let selections =
        SelectionSet::rectangular(EditorPosition::new(0, 1), EditorPosition::new(1, 4), 3, 6);

    let plan = build_render_plan_for_selection_set_with_cache(
        &buffer,
        &viewport,
        &decorations,
        selections,
        layout,
        &cache,
    );

    assert_eq!(plan.selections.len(), 2);
    assert_eq!(plan.selections[0].start_column, 1);
    assert_eq!(plan.selections[0].end_column, 1);
    assert_eq!(plan.selections[0].start_virtual_column, Some(3));
    assert_eq!(plan.selections[0].end_virtual_column, Some(6));
    assert_eq!(plan.selections[0].width, 24.0);
}

#[test]
fn editor_widget_render_plan_omits_rectangular_rows_hidden_by_folds() {
    let buffer = EditorBuffer::from_text("root\n    child\nnext");
    let mut folds = FoldModel::new(vec![FoldRange::new(0, 1)]);
    folds.set_collapsed(FoldRange::new(0, 1), true);
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();
    let selections =
        SelectionSet::rectangular(EditorPosition::new(0, 1), EditorPosition::new(2, 3), 1, 3);

    let plan = build_render_plan_for_selection_set_with_cache(
        &buffer,
        &viewport,
        &decorations,
        selections,
        layout,
        &cache,
    );

    assert_eq!(
        plan.selections
            .iter()
            .map(|selection| selection.line)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
}

#[test]
fn editor_widget_render_plan_keeps_many_selection_output_visible_bounded() {
    let text = (0..200)
        .map(|line| format!("line {line:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let buffer = EditorBuffer::from_text(text);
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics {
        line_height: 20.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(
        metrics,
        ScrollOffset {
            first_visible_row: 40,
            horizontal_px: 0.0,
        },
        500.0,
        100.0,
    );
    let cache = SyntaxLineCache::default();
    let ranges = (0..200)
        .map(|line| {
            EditorSelection::new(EditorPosition::new(line, 0), EditorPosition::new(line, 4))
        })
        .collect();
    let selections = SelectionSet::from_ranges(ranges, 40);

    let plan = build_render_plan_for_selection_set_with_cache(
        &buffer,
        &viewport,
        &decorations,
        selections,
        layout,
        &cache,
    );

    assert_eq!(plan.rows.len(), 6);
    assert_eq!(plan.selections.len(), 6);
    assert!(
        plan.selections
            .iter()
            .all(|selection| (40..=45).contains(&selection.line))
    );
}

#[test]
fn editor_widget_render_plan_hides_caret_when_line_is_offscreen() {
    let text = (0..30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let buffer = EditorBuffer::from_text(text);
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(
        EditorMetrics::default(),
        ScrollOffset {
            first_visible_row: 10,
            horizontal_px: 0.0,
        },
        500.0,
        200.0,
    );

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &syntax_settings("txt"),
    );

    assert!(plan.caret.is_none());
}

#[test]
fn editor_widget_render_plan_hides_selection_when_lines_are_offscreen() {
    let text = (0..30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let buffer = EditorBuffer::from_text(text);
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(
        EditorMetrics::default(),
        ScrollOffset {
            first_visible_row: 10,
            horizontal_px: 0.0,
        },
        500.0,
        200.0,
    );
    let selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(2, 4));

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        selection,
        layout,
        &syntax_settings("txt"),
    );

    assert!(plan.selections.is_empty());
}

#[test]
fn editor_widget_render_plan_exposes_syntax_spans_for_visible_rows() {
    let buffer = EditorBuffer::from_text("fn main() {\n    let value = 1;\n}");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);

    let settings = syntax_settings("rs");
    let cache = SyntaxLineCache::rebuild(&buffer, &settings);
    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &cache,
    );

    assert_eq!(plan.rows.len(), 3);
    assert!(plan.rows.iter().all(|row| !row.syntax_spans.is_empty()));
    assert!(
        plan.rows
            .iter()
            .flat_map(|row| &row.syntax_spans)
            .any(|span| span.color.is_some())
    );
    assert!(
        plan.rows
            .iter()
            .flat_map(|row| &row.syntax_spans)
            .all(|span| span.range.start < span.range.end)
    );
    assert!(
        plan.rows
            .iter()
            .flat_map(|row| row.syntax_spans.iter().map(|span| (row.text.len(), span)))
            .all(|(line_length, span)| span.range.end <= line_length)
    );
}

#[test]
fn editor_widget_render_plan_wrapper_honors_syntax_settings() {
    let buffer = EditorBuffer::from_text("fn main() {\n    let value = 1;\n}");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);

    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &syntax_settings("rs"),
    );

    assert!(plan.rows.iter().all(|row| !row.syntax_spans.is_empty()));
}

#[test]
fn editor_widget_render_plan_uses_prebuilt_syntax_cache() {
    let buffer = EditorBuffer::from_text("fn main() {\n    let value = 1;\n}");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let settings = syntax_settings("rs");
    let cache = SyntaxLineCache::rebuild(&buffer, &settings);

    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &cache,
    );

    assert!(cache.is_current(&settings, buffer.line_count()));
    assert!(plan.rows.iter().all(|row| !row.syntax_spans.is_empty()));
    assert_eq!(planned_text_draws(&plan, false), plan.rows.len());
    assert_eq!(planned_text_draws(&plan, true), plan.rows.len());
}

#[test]
fn editor_widget_fast_scroll_keeps_highlighted_rows_separate() {
    let buffer = EditorBuffer::from_text("fn main() {\n    let value = 1;\n}");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let settings = syntax_settings("rs");
    let cache = SyntaxLineCache::rebuild(&buffer, &settings);

    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &cache,
    );

    assert!(plan.rows.iter().all(|row| !row.syntax_spans.is_empty()));
    assert_eq!(planned_text_draws(&plan, true), plan.rows.len());
}

#[test]
fn editor_widget_fast_scroll_batches_ascii_rows() {
    let buffer = EditorBuffer::from_text("plain\ntext");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();

    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &cache,
    );

    assert_eq!(planned_text_draws(&plan, false), plan.rows.len());
    assert_eq!(planned_text_draws(&plan, true), 1);
}

#[test]
fn editor_widget_fast_scroll_keeps_non_ascii_rows_separate() {
    let buffer = EditorBuffer::from_text("plain\n\u{597d}\u{6f02}\u{7d30}");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();

    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &cache,
    );

    assert_eq!(planned_text_draws(&plan, true), plan.rows.len());
}

#[test]
fn editor_widget_fast_scroll_keeps_tabbed_rows_separate() {
    let buffer = EditorBuffer::from_text("plain\n\tindented");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();

    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &cache,
    );

    assert_eq!(planned_text_draws(&plan, true), plan.rows.len());
}

#[test]
fn editor_widget_renders_whitespace_and_eol_markers_without_extra_text_draws() {
    let buffer = EditorBuffer::from_text("a b c\n\tcd ");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            show_spaces: true,
            show_tabs: true,
            show_end_of_line_markers: true,
            ..DecorationSettings::default()
        },
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let cache = SyntaxLineCache::default();

    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &cache,
    );
    assert_eq!(plan.rows.len(), 2);
    assert_eq!(planned_text_draws_with_markers(&plan, false), 2);
}

#[test]
fn editor_widget_marker_draw_count_remains_bounded_for_distant_markers() {
    let buffer = EditorBuffer::from_text("x ".repeat(400));
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            show_spaces: true,
            ..DecorationSettings::default()
        },
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let plan = build_render_plan(
        &buffer,
        &viewport,
        &decorations,
        caret(0, 0),
        layout,
        &syntax_settings("txt"),
    );

    assert_eq!(planned_text_draws_with_markers(&plan, false), 1);
}

#[test]
fn editor_widget_plain_text_syntax_cache_stays_empty() {
    let buffer = EditorBuffer::from_text("plain\ntext");
    let settings = syntax_settings("txt");
    let cache = SyntaxLineCache::rebuild(&buffer, &settings);

    assert!(cache.is_current(&settings, buffer.line_count()));
    assert_eq!(cache, SyntaxLineCache::default());
}

#[test]
fn editor_widget_syntax_cache_can_prepare_only_visible_lines() {
    let buffer = EditorBuffer::from_text(
        (0..1_000)
            .map(|line| format!("let value_{line} = {line};"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let settings = syntax_settings("rs");
    let mut cache = SyntaxLineCache::new(&settings);

    cache.ensure_visible(&buffer, &settings, 0, 35);

    assert!(cache.is_compatible(&settings));
    assert!(!cache.is_current(&settings, buffer.line_count()));
    assert_eq!(cache.cached_line_count(), 36);
}

#[test]
fn editor_widget_syntax_cache_scrolls_forward_by_appending_one_line() {
    let buffer = EditorBuffer::from_text(
        (0..1_000)
            .map(|line| format!("let value_{line} = {line};"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let settings = syntax_settings("rs");
    let mut cache = SyntaxLineCache::new(&settings);

    cache.ensure_visible(&buffer, &settings, 0, 36);
    assert_eq!(cache.cached_line_count(), 37);

    cache.ensure_visible(&buffer, &settings, 1, 37);
    assert_eq!(
        cache.cached_line_count(),
        38,
        "scrolling down one row should append only the newly visible line"
    );

    cache.ensure_visible(&buffer, &settings, 2, 37);
    assert_eq!(
        cache.cached_line_count(),
        38,
        "a fully cached visible range should not rewind or rebuild spans"
    );
}

#[test]
fn editor_widget_syntax_cache_invalidation_aligns_to_highlighter_snapshot() {
    let buffer = EditorBuffer::from_text(
        (0..1_000)
            .map(|line| format!("let value_{line} = {line};"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let settings = syntax_settings("rs");
    let mut cache = SyntaxLineCache::new(&settings);

    cache.ensure_visible(&buffer, &settings, 0, 120);
    cache.invalidate_from(75);

    assert_eq!(
        cache.cached_line_count(),
        50,
        "invalidating from a line inside a syntect snapshot rewinds to the snapshot start"
    );

    cache.ensure_visible(&buffer, &settings, 75, 111);
    assert!(cache.cached_line_count() >= 112);
}

#[test]
fn editor_widget_syntax_cache_invalidates_from_changed_line() {
    let buffer = EditorBuffer::from_text("fn main() {\n    let value = 1;\n}\n");
    let settings = syntax_settings("rs");
    let mut cache = SyntaxLineCache::new(&settings);

    cache.ensure_visible(&buffer, &settings, 0, 2);
    assert_eq!(cache.cached_line_count(), 3);

    cache.invalidate_from(1);

    assert_eq!(cache.cached_line_count(), 0);
}

#[test]
fn editor_widget_key_action_maps_undo_redo_select_all_and_text_inputs() {
    let shortcuts = ShortcutMap::default();

    assert_eq!(
        key_action(
            &keyboard::Key::Character("z".into()),
            &keyboard::Key::Character("z".into()),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Undo)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Character("y".into()),
            &keyboard::Key::Character("y".into()),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::Redo)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Named(key::Named::Enter),
            &keyboard::Key::Named(key::Named::Enter),
            keyboard::Modifiers::NONE,
            None,
            &shortcuts,
        ),
        Some(EditorAction::InsertNewline)
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Character("ignored".into()),
            &keyboard::Key::Character("ignored".into()),
            keyboard::Modifiers::NONE,
            Some("\u{0007}"),
            &shortcuts,
        ),
        None
    );
}

#[test]
fn editor_widget_key_action_preserves_windows_dead_key_text() {
    let shortcuts = ShortcutMap::default();
    // An incompatible key after a Windows dead key produces both characters in
    // the text payload while the logical key remains the second pressed key.
    let key = keyboard::Key::Character("q".into());
    assert_eq!(
        key_action(
            &key,
            &key,
            keyboard::Modifiers::NONE,
            Some("'q"),
            &shortcuts
        ),
        Some(EditorAction::InsertText("'q".into()))
    );

    // Space commits the dead character, rather than inserting a literal space.
    let key = keyboard::Key::Named(key::Named::Space);
    assert_eq!(
        key_action(&key, &key, keyboard::Modifiers::NONE, Some("'"), &shortcuts),
        Some(EditorAction::InsertText("'".into()))
    );
    assert_eq!(
        key_action(&key, &key, keyboard::Modifiers::NONE, None, &shortcuts),
        Some(EditorAction::InsertText(" ".into()))
    );
}

#[test]
fn editor_widget_key_action_preserves_multi_scalar_text() {
    let shortcuts = ShortcutMap::default();
    for input in ["e\u{0301}", "\u{0915}\u{094d}\u{0937}"] {
        let key = keyboard::Key::Character(input.into());
        assert_eq!(
            key_action(
                &key,
                &key,
                keyboard::Modifiers::NONE,
                Some(input),
                &shortcuts
            ),
            Some(EditorAction::InsertText(input.into()))
        );
    }
}

#[test]
fn editor_widget_key_action_filters_controls_without_truncating_text() {
    let shortcuts = ShortcutMap::default();
    let key = keyboard::Key::Character("a".into());
    for (input, expected) in [
        ("\u{0007}ab\u{0000}cd\n", Some("abcd")),
        ("\u{0007}\u{0000}\n", None),
        ("", None),
    ] {
        assert_eq!(
            key_action(
                &key,
                &key,
                keyboard::Modifiers::NONE,
                Some(input),
                &shortcuts
            ),
            expected.map(|text| EditorAction::InsertText(text.into()))
        );
    }
}

#[test]
fn editor_widget_key_action_uses_custom_shortcut_map() {
    let mut shortcuts = ShortcutMap::default();
    shortcuts.clear(ShortcutCommand::DuplicateLine);
    shortcuts
        .set_binding(
            ShortcutCommand::DuplicateLine,
            KeyBinding::primary(ShortcutKey::character('l')),
        )
        .expect("custom shortcut should not conflict");

    assert_eq!(
        key_action(
            &keyboard::Key::Character("d".into()),
            &keyboard::Key::Character("d".into()),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        None
    );
    assert_eq!(
        key_action(
            &keyboard::Key::Character("l".into()),
            &keyboard::Key::Character("l".into()),
            keyboard::Modifiers::COMMAND,
            None,
            &shortcuts,
        ),
        Some(EditorAction::DuplicateLine)
    );
}

#[test]
fn editor_widget_key_action_resolves_shifted_number_row_from_modified_key() {
    let shortcuts = ShortcutMap::default();

    assert_eq!(
        key_action(
            &keyboard::Key::Character(")".into()),
            &keyboard::Key::Character("0".into()),
            keyboard::Modifiers::SHIFT | keyboard::Modifiers::ALT,
            None,
            &shortcuts,
        ),
        Some(EditorAction::UnfoldAll)
    );
}

#[test]
fn shortcut_map_resolves_binding_after_first_key_misses() {
    let mut shortcuts = ShortcutMap::default();
    shortcuts.clear(ShortcutCommand::SaveFile);
    shortcuts.clear(ShortcutCommand::UnfoldAll);
    shortcuts
        .set_binding(
            ShortcutCommand::SaveFile,
            KeyBinding::new(ShortcutModifiers::alt_shift(), ShortcutKey::character('0')),
        )
        .expect("custom shortcut should not conflict");

    assert_eq!(
        shortcuts.resolve(
            &keyboard::Key::Character(")".into()),
            &keyboard::Key::Character("0".into()),
            keyboard::Modifiers::SHIFT | keyboard::Modifiers::ALT,
        ),
        Some(ShortcutCommand::SaveFile)
    );
}

#[test]
fn shortcut_default_zoom_in_resolves_shifted_plus_key() {
    let shortcuts = ShortcutMap::default();
    let modifiers = keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT;

    assert_eq!(
        shortcuts.resolve(
            &keyboard::Key::Character("+".into()),
            &keyboard::Key::Character("=".into()),
            modifiers,
        ),
        Some(ShortcutCommand::ZoomIn)
    );
}

#[test]
fn shortcut_display_is_ascii_and_exposes_icon_parts_for_ui() {
    let logo = KeyBinding::new(
        ShortcutModifiers {
            logo: true,
            ..ShortcutModifiers::none()
        },
        ShortcutKey::character('k'),
    );

    if cfg!(target_os = "macos") {
        assert_eq!(
            KeyBinding::primary(ShortcutKey::character('s')).display(),
            "Cmd+S"
        );
        assert_eq!(
            KeyBinding::primary(ShortcutKey::character('s'))
                .display_parts()
                .modifiers,
            vec![ShortcutDisplayPart::Icon(ShortcutModifierIcon::Command)]
        );
        assert_eq!(logo.display(), "Cmd+K");
    } else {
        assert_eq!(
            KeyBinding::primary_shift(ShortcutKey::character('s')).display(),
            "Ctrl+Shift+S"
        );
        assert_eq!(
            KeyBinding::primary_shift(ShortcutKey::character('s'))
                .display_parts()
                .modifiers,
            vec![
                ShortcutDisplayPart::Text("Ctrl"),
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Shift)
            ]
        );
        assert_eq!(logo.display(), "Win+K");
        assert_eq!(
            logo.display_parts().modifiers,
            vec![ShortcutDisplayPart::Icon(ShortcutModifierIcon::Windows)]
        );
    }
}

#[test]
fn editor_widget_state_tracks_ime_preedit_without_document_commit() {
    let mut state: fragile_notepad::editor::AdvancedEditorState<()> =
        fragile_notepad::editor::AdvancedEditorState::default();
    let buffer = EditorBuffer::from_text("base");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings::default(),
        buffer.line_count(),
        &folds,
        vec![],
    );
    let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 500.0, 200.0);
    let settings = syntax_settings("rs");
    let mut cache = SyntaxLineCache::new(&settings);

    state.preedit = Some(iced::advanced::input_method::Preedit {
        content: "kana".to_owned(),
        selection: Some(1..3),
        text_size: None,
    });

    let plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, buffer.text().len()),
        layout,
        &cache,
    );

    assert_eq!(buffer.text(), "base");
    assert_eq!(plan.rows[0].text, "base");
    assert_eq!(
        state
            .preedit
            .as_ref()
            .map(|preedit| preedit.content.as_str()),
        Some("kana")
    );
    assert_eq!(
        state
            .preedit
            .as_ref()
            .and_then(|preedit| preedit.selection.clone()),
        Some(1..3)
    );

    cache.ensure_visible(&buffer, &settings, 0, 0);
    cache.invalidate_from(0);
    cache.ensure_visible(&buffer, &settings, 0, 0);
    let rebuilt_plan = build_render_plan_with_cache(
        &buffer,
        &viewport,
        &decorations,
        caret(0, buffer.text().len()),
        layout,
        &cache,
    );

    assert_eq!(buffer.text(), "base");
    assert_eq!(rebuilt_plan.rows[0].text, "base");
    assert_eq!(
        state
            .preedit
            .as_ref()
            .map(|preedit| preedit.content.as_str()),
        Some("kana")
    );
}
