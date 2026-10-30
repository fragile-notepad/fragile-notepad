use super::test_support::*;
use crate::core::{
    Document, DocumentLoadGeneration, DocumentLoadState, EditorSettings, SearchResultSettings,
    TextEncoding,
};
use iced::widget::text_editor::LineEnding;

#[test]
fn frame_settings_apply_save_and_cancel_with_the_settings_dialog() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::ToggleSettingsPanel);
    let _ = app.update(Message::DraftMaxFpsChanged("144".into()));
    let _ = app.update(Message::DraftVsyncToggled(false));
    assert_eq!(app.settings.max_fps, 120);
    assert!(app.settings.vsync);

    let _ = app.update(Message::ApplySettings);
    assert_eq!(app.settings.max_fps, 144);
    assert!(!app.settings.vsync);

    let _ = app.update(Message::DraftMaxFpsChanged("240".into()));
    let _ = app.update(Message::DraftVsyncToggled(true));
    let _ = app.update(Message::CancelSettings);
    assert_eq!(app.settings.max_fps, 144);
    assert!(!app.settings.vsync);
    assert_eq!(app.settings_dialog.max_fps_input, "144");
    assert!(!app.settings_dialog.draft.vsync);

    let _ = app.update(Message::DraftMaxFpsChanged("240".into()));
    let _ = app.update(Message::DraftVsyncToggled(true));
    let _ = app.update(Message::SaveSettings);
    assert_eq!(app.settings.max_fps, 240);
    assert!(app.settings.vsync);
    assert!(app.settings_persistence.flush_scheduled());
}

#[test]
fn invalid_frame_rate_blocks_apply_and_save_until_corrected() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::DraftMaxFpsChanged("144".into()));
    let _ = app.update(Message::DraftVsyncToggled(false));
    for invalid in ["", "0", "1001", "-1", "NaN"] {
        let _ = app.update(Message::DraftMaxFpsChanged(invalid.into()));
        assert_eq!(app.settings_dialog.draft.max_fps, 144);
        assert!(app.settings_dialog.validation_error().is_some());
        let _ = app.update(Message::ApplySettings);
        let _ = app.update(Message::SaveSettings);
        assert_eq!(app.settings.max_fps, 120);
        assert!(app.settings.vsync);
        assert!(!app.settings_persistence.flush_scheduled());
    }

    let _ = app.update(Message::DraftMaxFpsChanged("144".into()));
    assert!(app.settings_dialog.validation_error().is_none());
    let _ = app.update(Message::ApplySettings);
    assert_eq!(app.settings.max_fps, 144);
    assert!(!app.settings.vsync);
}

#[test]
fn applying_frame_settings_updates_the_runtime_only_when_the_values_change() {
    use crate::message::SettingsMessage;
    use futures::StreamExt;

    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    for message in [
        SettingsMessage::DraftMaxFpsChanged("144".into()),
        SettingsMessage::DraftVsyncToggled(false),
    ] {
        assert!(iced_runtime::task::into_stream(app.update_settings(message)).is_none());
    }

    let task = app.update_settings(SettingsMessage::ApplySettings);
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    assert!(matches!(
        futures::executor::block_on(stream.next()),
        Some(iced_runtime::Action::Window(
            iced_runtime::window::Action::SetFrameSettings {
                max_fps: 144,
                vsync: false,
            }
        ))
    ));
    assert!(futures::executor::block_on(stream.next()).is_none());
    if let Some(mut stream) =
        iced_runtime::task::into_stream(app.update_settings(SettingsMessage::ApplySettings))
    {
        while let Some(action) = futures::executor::block_on(stream.next()) {
            assert!(!matches!(
                action,
                iced_runtime::Action::Window(iced_runtime::window::Action::SetFrameSettings { .. })
            ));
        }
    }
}

#[test]
fn loading_frame_settings_respects_an_early_apply_and_updates_the_runtime() {
    use crate::message::SettingsMessage;
    use futures::StreamExt;

    for applied_before_load in [false, true] {
        let (mut app, _) = App::new();
        if applied_before_load {
            let _ = app.update(Message::ApplySettings);
        }
        let saved = EditorSettings {
            max_fps: 60,
            vsync: false,
            ..EditorSettings::default()
        };
        let task = app.update_settings(SettingsMessage::SettingsLoaded(Ok(Some(saved))));
        let mut updates = Vec::new();
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = futures::executor::block_on(stream.next()) {
                if let iced_runtime::Action::Window(
                    iced_runtime::window::Action::SetFrameSettings { max_fps, vsync },
                ) = action
                {
                    updates.push((max_fps, vsync));
                }
            }
        }
        let expected = if applied_before_load {
            (120, true)
        } else {
            (60, false)
        };
        assert_eq!((app.settings.max_fps, app.settings.vsync), expected);
        assert_eq!(app.settings_dialog.max_fps_input, expected.0.to_string());
        assert_eq!(app.settings_dialog.draft.vsync, expected.1);
        assert_eq!(
            updates,
            if applied_before_load {
                vec![]
            } else {
                vec![expected]
            }
        );
    }
}

#[test]
fn smooth_scrolling_changes_apply_save_and_cancel_with_the_settings_dialog() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::ToggleSettingsPanel);
    let _ = app.update(Message::DraftSmoothScrollingToggled(false));
    assert!(app.settings.smooth_scrolling);
    assert!(!app.settings_dialog.draft.smooth_scrolling);

    let _ = app.update(Message::ApplySettings);
    assert!(!app.settings.smooth_scrolling);
    let _ = app.update(Message::DraftSmoothScrollingToggled(true));
    let _ = app.update(Message::CancelSettings);
    assert!(!app.settings.smooth_scrolling);
    assert!(!app.settings_dialog.draft.smooth_scrolling);

    let _ = app.update(Message::DraftSmoothScrollingToggled(true));
    let _ = app.update(Message::SaveSettings);
    assert!(app.settings.smooth_scrolling);
    assert!(app.settings_persistence.flush_scheduled());
}

#[test]
fn search_result_preferences_keep_settings_inputs_in_sync() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::ToggleSettingsPanel);
    let _ = app.update(Message::AdvancedPreviewContextChanged("2".into()));
    let _ = app.update(Message::AdvancedPreviewCharsChanged("12".into()));
    let _ = app.update(Message::AdvancedResultLimitChanged("9".into()));
    assert_eq!(app.settings_dialog.result_limit_input, "9");
    assert_eq!(app.settings_dialog.preview_chars_input, "12");
    assert_eq!(app.settings_dialog.context_before_input, "2");
    assert!(app.settings_dialog.validation_error().is_none());
    let _ = app.update(Message::ApplySettings);
    assert_eq!(app.settings.search_results.result_limit, 9);
    assert_eq!(app.settings.search_results.preview_chars, 12);
}

#[test]
fn settings_search_preferences_apply_together_and_cancel_discards_later_edits() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::ToggleSettingsPanel);
    let original = app.settings.search_results;

    let _ = app.update(Message::DraftSearchResultLimitChanged("7".into()));
    let _ = app.update(Message::DraftSearchContextBeforeChanged("4".into()));
    let _ = app.update(Message::DraftSearchPreviewCharsChanged("24".into()));
    assert_eq!(app.settings.search_results, original);
    assert_eq!(app.search_dialog.result_settings, original);

    let _ = app.update(Message::ApplySettings);
    let applied = SearchResultSettings {
        result_limit: 7,
        preview_chars: 24,
        context_before: 4,
    };
    assert_eq!(app.settings.search_results, applied);
    assert_eq!(app.search_dialog.result_settings, applied);
    assert_eq!(app.find.match_limit, Some(7));

    let _ = app.update(Message::DraftSearchResultsReset);
    assert_eq!(app.settings_dialog.draft.search_results, original);
    assert_eq!(app.settings.search_results, applied);
    let _ = app.update(Message::CancelSettings);
    assert_eq!(app.settings.search_results, applied);
    assert_eq!(app.settings_dialog.result_limit_input, "7");
    assert_eq!(app.settings_dialog.preview_chars_input, "24");
    assert_eq!(app.settings_dialog.context_before_input, "4");
}

#[test]
fn invalid_numeric_settings_block_apply_and_save_without_losing_last_valid_values() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::ToggleSettingsPanel);
    let original = app.settings.clone();
    let _ = app.update(Message::DraftNewFileEncodingSelected(TextEncoding::Utf8Bom));
    let _ = app.update(Message::DraftSearchResultLimitChanged("12".into()));
    let _ = app.update(Message::DraftSearchResultLimitChanged("0".into()));
    assert_eq!(app.settings_dialog.draft.search_results.result_limit, 12);
    assert_eq!(app.settings_dialog.result_limit_input, "0");
    assert!(app.settings_dialog.validation_error().is_some());

    let _ = app.update(Message::ApplySettings);
    let save_task = app.update_settings(crate::message::SettingsMessage::SaveSettings);
    assert_eq!(app.settings, original);
    assert!(!app.settings_persistence.flush_scheduled());
    assert!(iced_runtime::task::into_stream(save_task).is_none());

    let _ = app.update(Message::DraftSearchResultLimitChanged("12".into()));
    let _ = app.update(Message::DraftSearchPreviewCharsChanged("40".into()));
    assert!(app.settings_dialog.validation_error().is_some());
    let _ = app.update(Message::DraftSearchContextBeforeChanged("39".into()));
    assert!(app.settings_dialog.validation_error().is_none());
    let _ = app.update(Message::ApplySettings);
    assert_eq!(app.settings.new_file_encoding, TextEncoding::Utf8Bom);
    assert_eq!(app.settings.search_results.preview_chars, 40);
    assert_eq!(app.settings.search_results.context_before, 39);
}

#[test]
fn recent_file_limit_applies_to_live_history_and_zero_disables_history() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    app.settings.record_open_history_path("first.txt");
    let _ = app.update(Message::ToggleSettingsPanel);
    app.settings.record_open_history_path("second.txt");
    let _ = app.update(Message::DraftRecentFileLimitChanged("1".into()));
    let _ = app.update(Message::ApplySettings);
    assert_eq!(app.settings.recent_file_limit, 1);
    assert_eq!(app.settings.open_history, vec![PathBuf::from("second.txt")]);

    let _ = app.update(Message::DraftRecentFileLimitChanged("101".into()));
    assert_eq!(app.settings_dialog.draft.recent_file_limit, 1);
    assert!(app.settings_dialog.validation_error().is_some());
    let _ = app.update(Message::ApplySettings);
    assert_eq!(app.settings.open_history.len(), 1);

    let _ = app.update(Message::DraftRecentFileLimitChanged("0".into()));
    let _ = app.update(Message::ApplySettings);
    assert!(app.settings.open_history.is_empty());
    assert!(!app.settings.record_open_history_path("third.txt"));
    assert!(app.settings.open_history.is_empty());
}

#[test]
fn fixed_wrap_validation_requires_valid_column_until_window_width_selected() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::DraftFixedWrapSelected(true));
    let _ = app.update(Message::DraftWrapColumnChanged(String::new()));
    assert!(app.settings_dialog.validation_error().is_some());
    let _ = app.update(Message::ApplySettings);
    assert_eq!(app.settings.wrap_column_limit, None);

    let _ = app.update(Message::DraftWordWrapToggled(false));
    assert!(app.settings_dialog.validation_error().is_some());
    let _ = app.update(Message::ApplySettings);
    assert!(app.settings.word_wrap);
    assert_eq!(app.settings.wrap_column_limit, None);

    let _ = app.update(Message::DraftFixedWrapSelected(false));
    assert!(app.settings_dialog.validation_error().is_none());
    let _ = app.update(Message::ApplySettings);
    assert!(!app.settings.word_wrap);
    assert_eq!(app.settings.wrap_column_limit, None);
}

#[test]
fn new_file_defaults_remain_draft_until_saved_and_cancel_resets_them() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::DraftNewFileEncodingSelected(
        TextEncoding::Utf16LeBom,
    ));
    let _ = app.update(Message::DraftNewFileLineEndingSelected(LineEnding::CrLf));
    assert_eq!(app.settings.new_file_encoding, TextEncoding::Utf8);
    assert_eq!(
        app.settings.new_file_line_ending,
        EditorSettings::DEFAULT_NEW_FILE_LINE_ENDING
    );
    let _ = app.update(Message::SaveSettings);
    assert_eq!(app.settings.new_file_encoding, TextEncoding::Utf16LeBom);
    assert_eq!(app.settings.new_file_line_ending, LineEnding::CrLf);
    assert!(app.settings_persistence.flush_scheduled());

    let _ = app.update(Message::DraftNewFileEncodingSelected(TextEncoding::Utf8));
    let _ = app.update(Message::DraftNewFileLineEndingSelected(LineEnding::Lf));
    let _ = app.update(Message::CancelSettings);
    assert_eq!(
        app.settings_dialog.draft.new_file_encoding,
        TextEncoding::Utf16LeBom
    );
    assert_eq!(
        app.settings_dialog.draft.new_file_line_ending,
        LineEnding::CrLf
    );
}

#[test]
fn loaded_new_file_defaults_control_startup_and_new_file_newlines_and_saved_bytes() {
    let (mut app, _) = App::new();
    let settings = EditorSettings {
        new_file_encoding: TextEncoding::Utf16LeBom,
        new_file_line_ending: LineEnding::CrLf,
        ..EditorSettings::default()
    };
    let _ = app.update(Message::SettingsLoaded(Ok(Some(settings))));
    let startup_id = app.workspace.active_document_id();

    for new_file in [false, true] {
        if new_file {
            let _ = app.update(Message::NewFile);
            assert_ne!(app.workspace.active_document_id(), startup_id);
        }
        let id = app.workspace.active_document_id();
        let document = app.workspace.document(id).unwrap();
        assert_eq!(document.encoding, TextEncoding::Utf16LeBom);
        assert_eq!(document.line_ending, Some(LineEnding::CrLf));
        assert!(!document.is_dirty);

        let _ = app.update(Message::EditorAction(
            id,
            EditorAction::InsertText("A".into()),
        ));
        let _ = app.update(Message::EditorAction(id, EditorAction::InsertNewline));
        let document = app.workspace.document(id).unwrap();
        assert_eq!(document.text(), "A\r\n");
        assert_eq!(
            document.bytes_for_save().unwrap(),
            vec![0xff, 0xfe, 0x41, 0x00, 0x0d, 0x00, 0x0a, 0x00]
        );
    }
}

#[test]
fn loading_and_applying_new_file_defaults_preserves_existing_and_recovered_metadata() {
    let (mut app, _) = App::new();
    let edited_id = app.workspace.active_document_id();
    {
        let document = app.workspace.document_mut(edited_id).unwrap();
        document.encoding = TextEncoding::Utf8Bom;
        document.line_ending = Some(LineEnding::Cr);
    }
    let _ = app.update(Message::EditorAction(
        edited_id,
        EditorAction::InsertText("draft".into()),
    ));
    let settings = EditorSettings {
        new_file_encoding: TextEncoding::Utf16LeBom,
        new_file_line_ending: LineEnding::CrLf,
        ..EditorSettings::default()
    };
    let _ = app.update(Message::SettingsLoaded(Ok(Some(settings))));
    let edited = app.workspace.document(edited_id).unwrap();
    assert_eq!(edited.encoding, TextEncoding::Utf8Bom);
    assert_eq!(edited.line_ending, Some(LineEnding::Cr));
    assert_eq!(edited.text(), "draft");

    let named_id = app.workspace.generate_document_id();
    let mut named = Document::from_path(named_id, "named.txt", "named\n");
    named.encoding = TextEncoding::Windows1252;
    app.workspace.push_document(named);

    let recovered_id = app.workspace.generate_document_id();
    let generation = DocumentLoadGeneration::next();
    let mut recovered = Document::loading(recovered_id, PathBuf::new(), generation);
    recovered.path = None;
    recovered.load_state = DocumentLoadState::Deferred { generation };
    recovered.encoding = TextEncoding::ShiftJis;
    recovered.line_ending = Some(LineEnding::Cr);
    app.workspace.push_document(recovered);

    let _ = app.update(Message::NewFile);
    let clean_id = app.workspace.active_document_id();
    let clean = app.workspace.document(clean_id).unwrap();
    assert_eq!(clean.encoding, TextEncoding::Utf16LeBom);
    assert_eq!(clean.line_ending, Some(LineEnding::CrLf));
    assert!(!clean.is_dirty);

    let _ = app.update(Message::DraftNewFileEncodingSelected(TextEncoding::Utf8));
    let _ = app.update(Message::DraftNewFileLineEndingSelected(LineEnding::Lf));
    let _ = app.update(Message::ApplySettings);
    for (id, encoding, ending) in [
        (edited_id, TextEncoding::Utf8Bom, LineEnding::Cr),
        (named_id, TextEncoding::Windows1252, LineEnding::Lf),
        (recovered_id, TextEncoding::ShiftJis, LineEnding::Cr),
        (clean_id, TextEncoding::Utf16LeBom, LineEnding::CrLf),
    ] {
        let document = app.workspace.document(id).unwrap();
        assert_eq!(document.encoding, encoding);
        assert_eq!(document.line_ending, Some(ending));
    }
    assert!(matches!(
        app.workspace.document(recovered_id).unwrap().load_state,
        DocumentLoadState::Deferred { .. }
    ));
}

#[test]
fn reopening_a_window_after_last_tab_close_uses_current_new_file_defaults() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::DraftNewFileEncodingSelected(TextEncoding::Utf8Bom));
    let _ = app.update(Message::DraftNewFileLineEndingSelected(LineEnding::CrLf));
    let _ = app.update(Message::ApplySettings);
    app.settings.exit_on_last_window_close = false;
    let old_id = app.workspace.active_document_id();
    let _ = app.update(Message::CloseFile);
    assert!(app.editor_windows.is_empty());
    assert!(app.workspace.document(old_id).is_none());
    let _ = app.update(Message::NewWindow);
    let document = app.workspace.active_document().unwrap();
    assert_ne!(document.id, old_id);
    assert_eq!(document.encoding, TextEncoding::Utf8Bom);
    assert_eq!(document.line_ending, Some(LineEnding::CrLf));
    assert!(!document.is_dirty);
}

#[test]
fn platform_line_ending_defaults_work_before_load_and_with_missing_or_legacy_settings() {
    let expected_ending = if cfg!(target_os = "windows") {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    };
    let expected_text = if cfg!(target_os = "windows") {
        "A\r\n"
    } else {
        "A\n"
    };
    assert_eq!(
        EditorSettings::DEFAULT_NEW_FILE_LINE_ENDING,
        expected_ending
    );

    for saved_settings in [
        None,
        Some(None),
        Some(Some(EditorSettings::from_xml_str(
            "<fragile-notepad-settings version=\"1\"><general auto-save=\"false\" /></fragile-notepad-settings>",
        ))),
    ] {
        let (mut app, _) = App::new();
        let scratch = app.workspace.active_document().unwrap();
        assert_eq!(scratch.line_ending, Some(expected_ending));
        assert!(!scratch.is_dirty);
        if let Some(settings) = saved_settings {
            let _ = app.update(Message::SettingsLoaded(Ok(settings)));
        }
        assert_eq!(app.settings.new_file_line_ending, expected_ending);

        for new_file in [false, true] {
            if new_file {
                let _ = app.update(Message::NewFile);
            }
            let id = app.workspace.active_document_id();
            let document = app.workspace.document(id).unwrap();
            assert_eq!(document.line_ending, Some(expected_ending));
            assert!(!document.is_dirty);
            let _ = app.update(Message::EditorAction(
                id,
                EditorAction::InsertText("A".into()),
            ));
            let _ = app.update(Message::EditorAction(id, EditorAction::InsertNewline));
            let document = app.workspace.document(id).unwrap();
            assert_eq!(document.text(), expected_text);
            assert_eq!(document.bytes_for_save().unwrap(), expected_text.as_bytes());
        }
    }
}

#[test]
fn explicit_saved_line_ending_overrides_platform_for_startup_and_future_documents() {
    let (stored_ending, expected_ending, expected_text) = if cfg!(target_os = "windows") {
        ("lf", LineEnding::Lf, "A\n")
    } else {
        ("crlf", LineEnding::CrLf, "A\r\n")
    };
    let saved = EditorSettings::from_xml_str(&format!(
        "<fragile-notepad-settings><files new-file-line-ending=\"{stored_ending}\" /></fragile-notepad-settings>"
    ));
    assert_eq!(saved.new_file_line_ending, expected_ending);
    let (mut app, _) = App::new();
    assert_ne!(
        app.workspace.active_document().unwrap().line_ending,
        Some(expected_ending)
    );
    let _ = app.update(Message::SettingsLoaded(Ok(Some(saved))));
    assert_eq!(app.settings.new_file_line_ending, expected_ending);

    for new_file in [false, true] {
        if new_file {
            let _ = app.update(Message::NewFile);
        }
        let id = app.workspace.active_document_id();
        let document = app.workspace.document(id).unwrap();
        assert_eq!(document.line_ending, Some(expected_ending));
        assert!(!document.is_dirty);
        let _ = app.update(Message::EditorAction(
            id,
            EditorAction::InsertText("A".into()),
        ));
        let _ = app.update(Message::EditorAction(id, EditorAction::InsertNewline));
        let document = app.workspace.document(id).unwrap();
        assert_eq!(document.text(), expected_text);
        assert_eq!(document.bytes_for_save().unwrap(), expected_text.as_bytes());
    }
}
