use fragile_notepad::core::{
    AppearanceMode, EditorSettings, HardwareAccelerationMode, IndentationMode, KeyBinding,
    SearchResultSettings, ShortcutCommand, ShortcutKey,
};
use std::path::PathBuf;

#[test]
fn search_result_settings_normalize_safe_bounds_and_preview_context() {
    let defaults = SearchResultSettings::default();
    assert_eq!(defaults.result_limit, 500);
    assert_eq!(defaults.preview_chars, 160);
    assert_eq!(defaults.context_before, 40);
    assert_eq!(defaults.normalized(), defaults);
    for (settings, expected) in [
        (
            SearchResultSettings {
                result_limit: 0,
                preview_chars: 0,
                context_before: usize::MAX,
            },
            SearchResultSettings {
                result_limit: 1,
                preview_chars: 1,
                context_before: 0,
            },
        ),
        (
            SearchResultSettings {
                result_limit: usize::MAX,
                preview_chars: usize::MAX,
                context_before: usize::MAX,
            },
            SearchResultSettings {
                result_limit: 10_000,
                preview_chars: 2_000,
                context_before: 1_999,
            },
        ),
    ] {
        assert_eq!(settings.normalized(), expected);
    }
}

#[test]
fn search_result_settings_round_trip_xml_and_normalize_before_persistence() {
    for search_results in [
        SearchResultSettings {
            result_limit: 1,
            preview_chars: 1,
            context_before: 0,
        },
        SearchResultSettings {
            result_limit: 250,
            preview_chars: 320,
            context_before: 80,
        },
        SearchResultSettings {
            result_limit: usize::MAX,
            preview_chars: 0,
            context_before: usize::MAX,
        },
    ] {
        let settings = EditorSettings {
            search_results,
            ..EditorSettings::default()
        };
        let xml = settings.to_xml_string();
        assert!(xml.contains("<search "));
        assert_eq!(
            EditorSettings::from_xml_str(&xml).search_results,
            search_results.normalized()
        );
    }
}

#[test]
fn search_result_settings_legacy_and_malformed_xml_keep_defaults() {
    for xml in [
        "<fragile-notepad-settings version=\"1\"><editor word-wrap=\"true\" /></fragile-notepad-settings>",
        "<fragile-notepad-settings><search /></fragile-notepad-settings>",
        "<fragile-notepad-settings><search result-limit=\"no\" preview-chars=\"-1\" context-before=\"999999999999999999999999999999999999999\" /></fragile-notepad-settings>",
    ] {
        assert_eq!(
            EditorSettings::from_xml_str(xml).search_results,
            SearchResultSettings::default()
        );
    }
}

#[test]
fn search_result_settings_xml_clamps_numeric_values_and_dependent_context() {
    for (attributes, expected) in [
        (
            "result-limit=\"0\" preview-chars=\"0\" context-before=\"40\"",
            SearchResultSettings {
                result_limit: 1,
                preview_chars: 1,
                context_before: 0,
            },
        ),
        (
            "result-limit=\"10001\" preview-chars=\"2001\" context-before=\"2000\"",
            SearchResultSettings {
                result_limit: 10_000,
                preview_chars: 2_000,
                context_before: 1_999,
            },
        ),
        (
            "preview-chars=\"10\"",
            SearchResultSettings {
                result_limit: 500,
                preview_chars: 10,
                context_before: 9,
            },
        ),
    ] {
        let xml =
            format!("<fragile-notepad-settings><search {attributes} /></fragile-notepad-settings>");
        assert_eq!(EditorSettings::from_xml_str(&xml).search_results, expected);
    }
}

#[test]
fn modern_syntax_presets_persist_and_highlight_distinct_token_roles() {
    use iced::advanced::text::highlighter::Highlighter as _;
    use iced::highlighter;
    for &theme in highlighter::Theme::VARIANTS {
        let mut settings = EditorSettings::default();
        settings.syntax_theme = theme;
        assert_eq!(
            EditorSettings::from_xml_str(&settings.to_xml_string()).syntax_theme,
            theme.family()
        );
        let mut parser = highlighter::Highlighter::new(&highlighter::Settings {
            theme,
            token: "rs".into(),
        });
        let mut token_color = |line: &str, token: &str| {
            let column = line.find(token).unwrap();
            parser
                .highlight_line(line)
                .collect::<Vec<_>>()
                .into_iter()
                .find(|(range, _)| range.contains(&column))
                .and_then(|(_, highlight)| highlight.color())
                .expect("colored token")
        };
        let keyword = token_color("let count = 42;", "let");
        let number = token_color("let count = 42;", "42");
        let string = token_color("let label = \"ready\";", "ready");
        let comment = token_color("// explanation", "explanation");
        assert_ne!(keyword, number);
        assert_ne!(number, string);
        assert_ne!(comment, keyword);
        assert_eq!(comment.a, 1.0);
        let type_color = token_color("let item: Option<u32> = None;", "Option");
        let macro_color = token_color("column![ text(\"Ready\") ];", "column");
        assert_ne!(type_color, keyword);
        assert_ne!(macro_color, type_color);
        assert_ne!(macro_color, string);
    }
}

#[test]
fn modern_syntax_presets_follow_explicit_appearance_changes() {
    use iced::highlighter::Theme;
    let mut settings = EditorSettings::default();
    settings.set_appearance(AppearanceMode::Light);
    assert_eq!(settings.resolved_syntax_theme(false), Theme::VSCodeLight);
    settings.set_appearance(AppearanceMode::Dark);
    assert_eq!(settings.syntax_theme, Theme::VSCodeDark);
    settings.syntax_theme = Theme::JetBrainsDark;
    settings.set_appearance(AppearanceMode::Light);
    assert_eq!(settings.resolved_syntax_theme(false), Theme::JetBrainsLight);
    settings.syntax_theme = Theme::SolarizedDark;
    settings.set_appearance(AppearanceMode::Light);
    assert_eq!(settings.resolved_syntax_theme(false), Theme::SolarizedLight);
}

#[test]
fn every_preset_follows_system_mode_and_keeps_one_selection_name() {
    use iced::highlighter::Theme;
    assert_eq!(Theme::ALL.len(), 7);
    for &family in Theme::ALL {
        let mut settings = EditorSettings::default();
        settings.set_syntax_theme(family);
        for appearance in [
            AppearanceMode::Light,
            AppearanceMode::Dark,
            AppearanceMode::System,
        ] {
            settings.set_appearance(appearance);
            for system_dark in [false, true] {
                let expected_dark = match appearance {
                    AppearanceMode::Light => false,
                    AppearanceMode::Dark => true,
                    AppearanceMode::System => system_dark,
                };
                let resolved = settings.resolved_syntax_theme(system_dark);
                assert_eq!(resolved.is_dark(), expected_dark);
                assert_eq!(resolved.family(), family);
                assert_eq!(settings.syntax_theme, family);
                assert_eq!(resolved.to_string(), family.to_string());
            }
        }
    }
}

#[test]
fn legacy_syntax_names_migrate_to_families() {
    use iced::highlighter::Theme;
    for (name, family) in [
        ("VS Code inspired · Light", Theme::VSCodeDark),
        ("VS Code inspired · Dark", Theme::VSCodeDark),
        ("JetBrains inspired · Light", Theme::JetBrainsDark),
        ("JetBrains inspired · Dark", Theme::JetBrainsDark),
        ("Solarized Dark", Theme::SolarizedDark),
        ("Inspired GitHub", Theme::InspiredGitHub),
    ] {
        let xml = format!(
            "<fragile-notepad-settings><general syntax-theme=\"{name}\" /></fragile-notepad-settings>"
        );
        assert_eq!(EditorSettings::from_xml_str(&xml).syntax_theme, family);
    }
}

#[test]
fn wrap_column_setting_round_trips_and_ignores_invalid_values() {
    for limit in [None, Some(80), Some(100), Some(120), Some(1000)] {
        let mut settings = EditorSettings::default();
        settings.wrap_column_limit = limit;
        assert_eq!(
            EditorSettings::from_xml_str(&settings.to_xml_string()).wrap_column_limit,
            limit
        );
    }
    for value in ["0", "-1", "1001", "abc"] {
        let xml = format!(
            "<fragile-notepad-settings><editor wrap-column=\"{value}\" /></fragile-notepad-settings>"
        );
        assert_eq!(EditorSettings::from_xml_str(&xml).wrap_column_limit, None);
    }
}

#[test]
fn wrap_visual_settings_are_independent_and_backward_compatible() {
    let legacy = EditorSettings::from_xml_str(
        "<fragile-notepad-settings version=\"1\"><decorations /></fragile-notepad-settings>",
    );
    assert!(legacy.decorations.show_wrap_indicator);
    assert!(legacy.decorations.show_wrap_guide);
    for (indicator, guide) in [(false, true), (true, false), (false, false)] {
        let mut settings = legacy.clone();
        settings.decorations.show_wrap_indicator = indicator;
        settings.decorations.show_wrap_guide = guide;
        let restored = EditorSettings::from_xml_str(&settings.to_xml_string());
        assert_eq!(restored.decorations.show_wrap_indicator, indicator);
        assert_eq!(restored.decorations.show_wrap_guide, guide);
        assert_eq!(
            restored.decoration_settings(),
            settings.decoration_settings()
        );
    }
}

#[test]
fn editor_settings_parse_xml_decoration_toggles_indentation_and_shortcuts() {
    let settings = EditorSettings::from_xml_str(
        "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<fragile-notepad-settings version=\"1\">
  <general appearance=\"dark\" hardware-acceleration=\"diagnostic\" auto-save=\"true\" syntax-theme=\"Solarized Dark\" />
  <editor word-wrap=\"false\" indentation=\"tabs\" scroll-speed=\"2.750\" />
  <appearance zoom=\"2.250\" />
  <decorations line-numbers=\"false\" spaces=\"true\" tabs=\"true\" eol-markers=\"true\" indentation-guides=\"false\" folding-controls=\"false\" />
  <shortcuts>
    <shortcut command=\"save_file\" binding=\"primary+shift+s\" />
    <shortcut command=\"fold_all\" />
  </shortcuts>
</fragile-notepad-settings>
",
    );

    assert!(!settings.word_wrap);
    assert!(settings.auto_save);
    assert_eq!(settings.zoom, 2.25);
    assert_eq!(settings.scroll_speed, 2.75);
    assert_eq!(settings.indentation, IndentationMode::Tabs);
    assert_eq!(settings.decoration_settings().indent_width, 4);
    assert_eq!(settings.appearance, AppearanceMode::Dark);
    assert_eq!(
        settings.hardware_acceleration,
        HardwareAccelerationMode::Diagnostic
    );
    assert!(!settings.decorations.show_line_numbers);
    assert!(settings.decorations.show_spaces);
    assert!(settings.decorations.show_tabs);
    assert!(settings.decorations.show_end_of_line_markers);
    assert!(!settings.decorations.show_indentation_guides);
    assert!(!settings.decorations.show_folding_controls);
    assert_eq!(
        settings
            .shortcuts
            .binding_display(ShortcutCommand::SaveFile),
        format!("{}+Shift+S", platform_primary_label())
    );
    assert_eq!(
        settings.shortcuts.binding_display(ShortcutCommand::FoldAll),
        "Unassigned"
    );
}

#[test]
fn editor_settings_parse_open_history_de_dupes_in_saved_order() {
    let settings = EditorSettings::from_xml_str(
        "\
<fragile-notepad-settings version=\"1\">
  <open-history>
    <file path=\"/tmp/alpha.txt\" />
    <file path=\"/tmp/beta.txt\" />
    <file path=\"/tmp/alpha.txt\" />
  </open-history>
</fragile-notepad-settings>
",
    );

    assert_eq!(
        settings.open_history,
        vec![
            PathBuf::from("/tmp/alpha.txt"),
            PathBuf::from("/tmp/beta.txt")
        ]
    );
}

#[test]
fn editor_settings_record_open_history_promotes_and_trims_recent_paths() {
    let mut settings = EditorSettings::default();
    settings.record_open_history_path("one.txt");
    settings.record_open_history_path("two.txt");
    settings.record_open_history_path("one.txt");

    assert_eq!(
        settings.open_history,
        vec![PathBuf::from("one.txt"), PathBuf::from("two.txt")]
    );

    for index in 0..(EditorSettings::MAX_OPEN_HISTORY + 2) {
        settings.record_open_history_path(format!("recent-{index}.txt"));
    }

    assert_eq!(
        settings.open_history.len(),
        EditorSettings::MAX_OPEN_HISTORY
    );
    assert_eq!(
        settings.open_history.first(),
        Some(&PathBuf::from(format!(
            "recent-{}.txt",
            EditorSettings::MAX_OPEN_HISTORY + 1
        )))
    );
    assert!(!settings.open_history.contains(&PathBuf::from("one.txt")));
}

#[test]
fn editor_settings_round_trip_preserves_xml_escaped_open_history() {
    let mut settings = EditorSettings::default();
    settings.record_open_history_path("notes/A & B.txt");

    let persisted = settings.to_xml_string();

    assert!(persisted.contains("<open-history>"));
    assert!(persisted.contains("path=\"notes/A &amp; B.txt\""));

    let parsed = EditorSettings::from_xml_str(&persisted);
    assert_eq!(parsed.open_history, vec![PathBuf::from("notes/A & B.txt")]);
}

#[test]
fn editor_settings_round_trip_preserves_general_and_decoration_settings() {
    let mut settings = EditorSettings::default();
    settings.set_word_wrap(false);
    settings.set_auto_save(true);
    settings.set_zoom(1.5);
    settings.set_scroll_speed(2.25);
    settings.set_indentation(IndentationMode::spaces(2));
    settings.set_appearance(AppearanceMode::Light);
    settings.set_show_line_numbers(false);
    settings.set_show_spaces(true);
    settings.set_show_tabs(true);
    settings.set_show_end_of_line_markers(true);
    settings.set_show_indentation_guides(false);
    settings.set_show_folding_controls(false);
    settings.set_hardware_acceleration(HardwareAccelerationMode::Diagnostic);

    assert_eq!(
        EditorSettings::from_xml_str(&settings.to_xml_string()),
        settings
    );
}

#[test]
fn editor_settings_round_trip_preserves_xml_escaped_shortcuts() {
    let mut settings = EditorSettings::default();
    settings
        .shortcuts
        .set_binding(
            ShortcutCommand::SaveFile,
            KeyBinding::primary(ShortcutKey::character('&')),
        )
        .expect("custom shortcut should not conflict");

    let persisted = settings.to_xml_string();
    assert!(persisted.contains("binding=\"primary+&amp;\""));

    let parsed = EditorSettings::from_xml_str(&persisted);
    assert_eq!(
        parsed.shortcuts.binding_display(ShortcutCommand::SaveFile),
        format!("{}+&", platform_primary_label())
    );
}

fn platform_primary_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    }
}

#[test]
fn editor_settings_invalid_values_keep_defaults_and_clamp_zoom() {
    let settings = EditorSettings::from_xml_str(
        "\
<fragile-notepad-settings version=\"1\">
  <general appearance=\"sepia\" />
  <editor word-wrap=\"maybe\" indentation=\"spaces:0\" scroll-speed=\"99\" />
  <appearance zoom=\"99\" />
  <decorations spaces=\"maybe\" tabs=\"true\" />
</fragile-notepad-settings>
",
    );

    assert!(settings.word_wrap);
    assert_eq!(settings.zoom, EditorSettings::MAX_ZOOM);
    assert_eq!(settings.scroll_speed, EditorSettings::MAX_SCROLL_SPEED);
    assert_eq!(
        settings.indentation,
        IndentationMode::Spaces(IndentationMode::DEFAULT_SPACE_WIDTH)
    );
    assert_eq!(settings.appearance, AppearanceMode::System);
    assert_eq!(
        settings.hardware_acceleration,
        HardwareAccelerationMode::Lazy
    );
    assert!(!settings.decorations.show_spaces);
    assert!(settings.decorations.show_tabs);
}

#[test]
fn editor_settings_reject_non_finite_numeric_values() {
    let settings = EditorSettings::from_xml_str(
        "\
<fragile-notepad-settings version=\"1\">
  <editor scroll-speed=\"NaN\" />
  <appearance zoom=\"NaN\" />
</fragile-notepad-settings>
",
    );

    assert_eq!(settings.zoom, EditorSettings::DEFAULT_ZOOM);
    assert_eq!(settings.scroll_speed, EditorSettings::DEFAULT_SCROLL_SPEED);
}
