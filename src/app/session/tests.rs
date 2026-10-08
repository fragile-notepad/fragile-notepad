use super::*;
use crate::core::DirtyCloseDecision;
use crate::core::{DecodedText, EditorSettings, TextEncoding};
use crate::editor::EditorAction;
use crate::services::types::{FileLoadChunk, FileLoadFinished};
use std::sync::Arc;

fn ready(saved: Session) -> App {
    let (mut app, _) = App::new_with_options(StartupOptions::default());
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(Some(saved))));
    let _ = app.update(Message::StartupReady);
    app
}

#[test]
fn wrapped_session_restores_logical_top_after_provisional_geometry_and_analysis() {
    let mut original = ready(Session::default());
    let original_id = original.workspace.active_document_id();
    let document = original.workspace.document_mut(original_id).unwrap();
    document.buffer = EditorBuffer::from_text("x".repeat(12_000));
    document.refresh_after_text_change();
    let _ = original.update(Message::EditorAction(
        original_id,
        EditorAction::ViewportChanged {
            visible_rows: 6,
            text_width: 962,
            character_width_milli: 8000,
            font_size_milli: 14_545,
            hint_factor_milli: None,
        },
    ));
    let _ = original.update(Message::EditorAction(
        original_id,
        EditorAction::ScrollToRow(20),
    ));
    let saved = original.snapshot_session();
    assert_eq!(saved.documents[0].first_visible_position, Some((0, 2400)));

    let mut restored = ready(saved);
    let id = restored.workspace.active_document_id();
    assert_eq!(
        restored.snapshot_session().documents[0].first_visible_position,
        Some((0, 2400)),
        "an early snapshot must retain the exact recovery anchor"
    );
    let document = restored.workspace.document_mut(id).unwrap();
    let (buffer, request) = document.analysis_request().unwrap();
    let _ = restored.update(Message::DocumentAnalyzed(
        crate::core::document::analyze_document(buffer, request),
    ));
    let _ = restored.update(Message::EditorAction(
        id,
        EditorAction::ViewportChanged {
            visible_rows: 6,
            text_width: 242,
            character_width_milli: 8000,
            font_size_milli: 14_545,
            hint_factor_milli: None,
        },
    ));

    let document = restored.workspace.document(id).unwrap();
    assert_eq!(document.viewport.wrap_columns(), Some(30));
    assert_eq!(document.scroll.first_visible_row, 80);
    assert_eq!(
        document.session_top_position(),
        Some(EditorPosition::new(0, 2400))
    );

    let _ = restored.update(Message::EditorAction(id, EditorAction::ScrollToRow(90)));
    let _ = restored.update(Message::EditorAction(
        id,
        EditorAction::ViewportChanged {
            visible_rows: 6,
            text_width: 482,
            character_width_milli: 8000,
            font_size_milli: 14_545,
            hint_factor_milli: None,
        },
    ));
    assert_eq!(
        restored
            .workspace
            .document(id)
            .unwrap()
            .scroll
            .first_visible_row,
        45,
        "later resizing must preserve user scrolling, not reapply recovery"
    );
}

#[test]
fn legacy_wrapped_sessions_clamp_against_screen_rows_and_clear_horizontal_scroll() {
    for saved_row in [50, usize::MAX] {
        let restored = ready(Session {
            documents: vec![SessionDocument {
                text: Some("x".repeat(16_000)),
                first_visible_row: saved_row,
                horizontal_offset: 240.0,
                ..Default::default()
            }],
            ..Default::default()
        });
        let document = restored.workspace.active_document().unwrap();

        assert!(document.word_wrap());
        assert_eq!(document.buffer.line_count(), 1);
        assert_eq!(
            document.scroll.first_visible_row,
            saved_row.min(document.viewport.visible_row_count() - 1)
        );
        assert_eq!(document.scroll.horizontal_px, 0.0);
    }
}

#[test]
fn streamed_session_restores_immediately_when_geometry_was_already_measured() {
    let mut restored = ready(Session {
        documents: vec![SessionDocument {
            path: Some(std::path::absolute("wrapped-session.txt").unwrap()),
            first_visible_row: 20,
            first_visible_position: Some((0, 2400)),
            ..Default::default()
        }],
        ..Default::default()
    });
    let id = restored.workspace.active_document_id();
    let _ = restored.update(Message::EditorAction(
        id,
        EditorAction::ViewportChanged {
            visible_rows: 6,
            text_width: 802,
            character_width_milli: 8000,
            font_size_milli: 14_545,
            hint_factor_milli: None,
        },
    ));
    let document = restored.workspace.document_mut(id).unwrap();
    let generation = document.load_generation().unwrap();
    assert!(document.replace_loading_preview(
        generation,
        &"x".repeat(6000),
        true,
        6000,
        Some(6000),
    ));
    assert!(document.complete_streaming_load(generation, TextEncoding::Utf8));
    let _ = restored.update(Message::None);
    assert_eq!(
        restored
            .workspace
            .document(id)
            .unwrap()
            .scroll
            .first_visible_row,
        24
    );

    let _ = restored.update(Message::EditorAction(id, EditorAction::ScrollToRow(30)));
    let _ = restored.update(Message::EditorAction(
        id,
        EditorAction::ViewportChanged {
            visible_rows: 6,
            text_width: 402,
            character_width_milli: 8000,
            font_size_milli: 14_545,
            hint_factor_milli: None,
        },
    ));
    assert_eq!(
        restored
            .workspace
            .document(id)
            .unwrap()
            .scroll
            .first_visible_row,
        60
    );
}

#[test]
fn wrapped_session_keeps_exact_header_position_until_saved_folds_are_restored() {
    let mut restored = ready(Session {
        documents: vec![SessionDocument {
            text: Some(format!("{} {{\n  body\n}}\ntail", "x".repeat(96))),
            first_visible_position: Some((0, 30)),
            collapsed_folds: vec![(0, 2)],
            ..Default::default()
        }],
        ..Default::default()
    });
    let id = restored.workspace.active_document_id();
    let _ = restored.update(Message::EditorAction(
        id,
        EditorAction::ViewportChanged {
            visible_rows: 4,
            text_width: 90,
            character_width_milli: 8000,
            font_size_milli: 14_545,
            hint_factor_milli: None,
        },
    ));
    assert_eq!(
        restored.snapshot_session().documents[0].first_visible_position,
        Some((0, 30))
    );
    let document = restored.workspace.document_mut(id).unwrap();
    let (buffer, request) = document.analysis_request().unwrap();
    let _ = restored.update(Message::DocumentAnalyzed(
        crate::core::document::analyze_document(buffer, request),
    ));

    let document = restored.workspace.document(id).unwrap();
    assert!(
        document
            .folds
            .is_collapsed(crate::editor::FoldRange::new(0, 2))
    );
    assert_eq!(
        document.scroll.first_visible_row,
        document
            .viewport
            .position_to_visible_row(EditorPosition::new(0, 30))
            .unwrap()
    );
    assert_eq!(
        document.session_top_position(),
        Some(EditorPosition::new(0, 30))
    );
}

#[test]
fn restores_one_active_file_and_keeps_99_tabs_deferred() {
    let saved = Session {
        documents: (0..100)
            .map(|i| SessionDocument {
                path: Some(std::path::absolute(format!("restore-{i}.txt")).unwrap()),
                is_pinned: i < 2,
                ..Default::default()
            })
            .collect(),
        active_index: 42,
        ..Default::default()
    };
    let mut app = ready(saved.clone());
    assert_eq!(app.workspace.documents().len(), 100);
    assert_eq!(
        app.workspace
            .documents()
            .iter()
            .filter(|doc| doc.is_loading())
            .count(),
        1
    );
    assert_eq!(
        app.workspace
            .documents()
            .iter()
            .filter(|doc| matches!(doc.load_state, DocumentLoadState::Deferred { .. }))
            .count(),
        99
    );
    assert_eq!(app.snapshot_session(), saved);
    let id = app.workspace.documents()[4].id;
    let _ = app.update(Message::TabSelected(id));
    assert!(app.workspace.document(id).unwrap().is_loading());
    assert_eq!(app.files.load_handles().len(), 2);
    let _ = app.update(Message::TabClosed(id));
    assert!(!app.files.load_handles().contains_key(&id));
    assert_eq!(app.files.load_handles().len(), 2); // the newly active deferred neighbor starts loading
    assert!(!app.session.pending.contains_key(&id));
}

#[test]
fn preserves_inactive_unsaved_tabs_and_recovers_exact_text_on_selection() {
    let unsaved = SessionDocument {
        text: Some("one\r\n二\r\n".into()),
        is_dirty: true,
        is_pinned: true,
        encoding: TextEncoding::Utf16LeBom,
        line_ending: Some("\r\n".into()),
        anchor_line: 1,
        cursor_line: 1,
        anchor_column: 3,
        cursor_column: 3,
        ..Default::default()
    };
    let mut app = ready(Session {
        documents: vec![
            SessionDocument {
                text: Some("active".into()),
                ..Default::default()
            },
            unsaved.clone(),
        ],
        ..Default::default()
    });
    assert_eq!(app.snapshot_session().documents[1], unsaved);
    let id = app.workspace.documents()[1].id;
    let _ = app.update(Message::TabSelected(id));
    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "one\r\n二\r\n");
    assert!(document.is_dirty);
    assert_eq!(document.encoding, TextEncoding::Utf16LeBom);
    assert_eq!(document.main_selection().cursor, EditorPosition::new(1, 3));
    assert_eq!(app.snapshot_session().documents[1].text, unsaved.text);
    let _ = app.update(Message::TabClosed(id));
    assert_eq!(app.close_prompt.document(), Some(id));
    let _ = app.update(Message::DirtyCloseResolved(id, DirtyCloseDecision::Cancel));
    assert!(app.workspace.document(id).is_some());
}

#[test]
fn missing_file_keeps_session_metadata_and_can_retry_after_failure() {
    let path = std::path::absolute("missing-session.txt").unwrap();
    let entry = SessionDocument {
        path: Some(path.clone()),
        cursor_line: 80,
        ..Default::default()
    };
    let mut app = ready(Session {
        documents: vec![entry.clone()],
        ..Default::default()
    });
    let doc = app.workspace.active_document().unwrap();
    let id = doc.id;
    let generation = doc.load_generation().unwrap();
    let _ = app.update(Message::FileLoadFinished(Err(
        crate::services::types::FileLoadFailure {
            document_id: id,
            generation,
            path,
            error: crate::services::types::FileError::Io(std::io::ErrorKind::NotFound),
        },
    )));
    assert_eq!(app.snapshot_session().documents[0], entry);
    assert!(matches!(
        app.workspace.document(id).unwrap().load_state,
        DocumentLoadState::Failed { .. }
    ));
}

#[test]
fn settings_initialization_merges_early_history_and_preferences() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::ZoomIn);
    app.settings.record_open_history_path("new.txt");
    let mut loaded = EditorSettings {
        word_wrap: true,
        ..EditorSettings::default()
    };
    loaded.record_open_history_path("old.txt");
    let _ = app.update(Message::SettingsLoaded(Ok(Some(loaded))));
    assert_eq!(app.settings.zoom, 1.1);
    assert!(app.settings.word_wrap);
    assert_eq!(
        app.settings.open_history,
        vec![PathBuf::from("new.txt"), PathBuf::from("old.txt")]
    );
    assert!(app.settings_persistence.flush_scheduled());
}

#[test]
fn early_external_paths_are_opened_after_initialization_without_session() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::OpenPaths(vec![
        "forwarded-before-settings.txt".into(),
    ]));
    assert_eq!(app.workspace.documents().len(), 1);
    assert!(app.workspace.active_document().unwrap().path.is_none());
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    assert!(
        app.workspace
            .active_document()
            .unwrap()
            .path
            .as_ref()
            .unwrap()
            .ends_with("forwarded-before-settings.txt")
    );
}

#[test]
fn explicit_default_reset_before_settings_load_wins_without_losing_other_preferences() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::ZoomReset);
    let _ = app.update(Message::ToggleLineNumbers);
    let _ = app.update(Message::ToggleLineNumbers);
    let mut loaded = EditorSettings {
        zoom: 2.0,
        ..EditorSettings::default()
    };
    loaded.decorations.show_line_numbers = false;
    loaded.decorations.show_spaces = true;
    let _ = app.update(Message::SettingsLoaded(Ok(Some(loaded))));
    assert_eq!(app.settings.zoom, 1.0);
    assert!(app.settings.decorations.show_line_numbers);
    assert!(app.settings.decorations.show_spaces);
}

#[test]
fn streaming_completion_defers_analysis_and_rejects_stale_result() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::FilePicked(Ok("background.rs".into())));
    let document = app.workspace.active_document().unwrap();
    let id = document.id;
    let generation = document.load_generation().unwrap();
    let path = document.path.clone().unwrap();
    let source = "fn sample() {\n    let x = 1;\n}\n";
    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id: id,
        generation,
        path: path.clone(),
        text: Arc::new(source.into()),
        reset: false,
        bytes_read: source.len() as u64,
        total_bytes: Some(source.len() as u64),
    }));
    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        document_id: id,
        generation,
        path,
        encoding: TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: source.len() as u64,
        total_bytes: Some(source.len() as u64),
    })));
    let document = app.workspace.document(id).unwrap();
    assert!(document.analysis_pending);
    assert!(document.folds.ranges().is_empty());
    let (buffer, request) = document.analysis_request().unwrap();
    let stale = crate::core::document::analyze_document(buffer, request);
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("x".into()),
    ));
    assert!(
        !app.workspace
            .document_mut(id)
            .unwrap()
            .apply_analysis(stale)
    );
    let document = app.workspace.document(id).unwrap();
    let (buffer, request) = document.analysis_request().unwrap();
    assert!(
        app.workspace
            .document_mut(id)
            .unwrap()
            .apply_analysis(crate::core::document::analyze_document(buffer, request))
    );
    assert!(
        !app.workspace
            .document(id)
            .unwrap()
            .folds
            .ranges()
            .is_empty()
    );
}

#[test]
fn duplicate_paths_select_existing_document() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::OpenPaths(vec![
        "same.txt".into(),
        "same.txt".into(),
    ]));
    assert_eq!(app.workspace.documents().len(), 1);
}

#[test]
fn recovery_snapshot_contains_current_unsaved_edits_and_clean_file_paths_only() {
    let mut app = ready(Session {
        documents: vec![SessionDocument {
            text: Some("before".into()),
            ..Default::default()
        }],
        ..Default::default()
    });
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::EditorAction(id, EditorAction::SelectAll));
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("after🙂".into()),
    ));
    let session = app.snapshot_session();
    session.validate().unwrap();
    assert_eq!(session.documents[0].text.as_deref(), Some("after🙂"));
    assert!(session.documents[0].is_dirty);
    assert!(app.session.dirty);
    let disk = app.workspace.insert_decoded_file(
        "disk.txt",
        DecodedText {
            text: "on disk".into(),
            encoding: TextEncoding::Utf8,
            had_errors: false,
        },
    );
    assert!(
        app.snapshot_session()
            .documents
            .iter()
            .any(
                |d| d.path.as_deref() == Some(std::path::Path::new("disk.txt")) && d.text.is_none()
            )
    );
    let document = app.workspace.document_mut(disk).unwrap();
    let before = document.folds.ranges().to_vec();
    let mut settings = document.decorations.settings;
    settings.show_line_numbers = false;
    document.set_decoration_settings(settings);
    assert_eq!(document.folds.ranges(), before);
    assert!(
        document
            .decorations
            .line_decorations
            .iter()
            .all(|line| line.line_number.is_none())
    );
}

#[test]
fn actual_tab_click_activates_deferred_recovery_and_close_activates_neighbor() {
    let mut app = ready(Session {
        documents: (0..3)
            .map(|i| SessionDocument {
                text: Some(format!("tab {i}")),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    });
    let first = app.workspace.documents()[0].id;
    let last = app.workspace.documents()[2].id;
    let _ = app.update(Message::TabDragStarted(last));
    assert_eq!(app.workspace.active_document().unwrap().text(), "tab 2");
    let _ = app.update(Message::TabClosed(first));
    let _ = app.update(Message::TabClosed(last));
    assert_eq!(app.workspace.active_document().unwrap().text(), "tab 1");
    assert!(
        app.workspace
            .active_document()
            .unwrap()
            .has_complete_text_index()
    );
}

#[test]
fn failed_shutdown_keeps_current_unsaved_document_open() {
    let mut app = ready(Session {
        documents: vec![SessionDocument {
            text: Some("recover me".into()),
            is_dirty: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    app.lifecycle.begin_shutdown();
    let _ = app.update(Message::ShutdownPersisted(Err("disk full".into())));
    assert!(!app.lifecycle.is_exiting());
    assert!(app.workspace.active_document().unwrap().is_dirty);
    assert_eq!(
        app.workspace.active_document().unwrap().text(),
        "recover me"
    );
    assert!(app.file_status.as_ref().unwrap().contains("disk full"));
}

#[test]
fn failed_restoration_is_read_only_and_legacy_edits_are_snapshotted() {
    let path = std::path::absolute("missing-recovery.txt").unwrap();
    let entry = SessionDocument {
        path: Some(path.clone()),
        ..Default::default()
    };
    let mut app = ready(Session {
        documents: vec![entry.clone()],
        ..Default::default()
    });
    let document = app.workspace.active_document().unwrap();
    let id = document.id;
    let generation = document.load_generation().unwrap();
    let _ = app.update(Message::FileLoadFinished(Err(
        crate::services::types::FileLoadFailure {
            document_id: id,
            generation,
            path,
            error: crate::services::types::FileError::Io(std::io::ErrorKind::NotFound),
        },
    )));
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("cannot lose this".into()),
    ));
    let _ = app.update(Message::EncodingSelected(TextEncoding::Utf16LeBom));
    let document = app.workspace.document(id).unwrap();
    assert!(document.text().is_empty());
    assert!(!document.is_dirty);
    assert_eq!(document.encoding, TextEncoding::Utf8);
    assert_eq!(app.snapshot_session().documents[0], entry);

    // Also preserve dirty buffers produced before the failed-load guard was
    // introduced, rather than returning stale deferred metadata.
    let document = app.workspace.document_mut(id).unwrap();
    document.buffer = EditorBuffer::from_text("existing unsaved work");
    document.mark_dirty();
    let snapshot = app.snapshot_session();
    snapshot.validate().unwrap();
    assert_eq!(
        snapshot.documents[0].text.as_deref(),
        Some("existing unsaved work")
    );
    assert!(snapshot.documents[0].is_dirty);
    let recovered = ready(snapshot);
    assert_eq!(
        recovered.workspace.active_document().unwrap().text(),
        "existing unsaved work"
    );
    assert!(recovered.workspace.active_document().unwrap().is_dirty);
}

#[test]
fn deferred_pin_changes_survive_activation_and_another_session() {
    for originally_pinned in [false, true] {
        let mut app = ready(Session {
            documents: vec![
                SessionDocument {
                    text: Some("active".into()),
                    ..Default::default()
                },
                SessionDocument {
                    text: Some("deferred".into()),
                    is_pinned: originally_pinned,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let id = app.workspace.documents()[1].id;
        let _ = app.update(Message::TabPinToggled(id));
        assert_eq!(
            app.workspace.document(id).unwrap().is_pinned,
            !originally_pinned
        );
        let _ = app.update(Message::TabSelected(id));
        assert_eq!(
            app.workspace.document(id).unwrap().is_pinned,
            !originally_pinned
        );
        let restored = ready(app.snapshot_session());
        assert_eq!(
            restored.workspace.active_document().unwrap().is_pinned,
            !originally_pinned
        );
    }
}

#[test]
fn restoration_preserves_automatic_and_manual_language_selection() {
    for manual in [false, true] {
        let (mut original, _) = App::new();
        let id = original
            .workspace
            .insert_loaded_file("automatic.rs", "fn main() {}");
        if manual {
            original
                .workspace
                .document_mut(id)
                .unwrap()
                .set_syntax_token("rs");
        }
        original.workspace.document_mut(id).unwrap().mark_dirty();
        let snapshot = original.snapshot_session();
        assert_eq!(
            snapshot.documents.last().unwrap().syntax_automatic,
            Some(!manual)
        );
        let mut restored = ready(snapshot);
        let document = restored.workspace.active_document_mut().unwrap();
        assert_eq!(document.syntax_is_automatic(), !manual);
        document.set_path("renamed.py");
        assert_eq!(document.syntax_token, if manual { "rs" } else { "py" });
    }
}

#[test]
fn legacy_sessions_infer_automatic_syntax_when_extension_matches() {
    let mut restored = ready(Session {
        documents: vec![SessionDocument {
            path: Some("legacy.rs".into()),
            text: Some("fn main() {}".into()),
            syntax_token: Some("rs".into()),
            ..Default::default()
        }],
        ..Default::default()
    });
    let document = restored.workspace.active_document_mut().unwrap();
    assert!(document.syntax_is_automatic());
    document.set_path("renamed.py");
    assert_eq!(document.syntax_token, "py");
}

#[test]
fn forwarded_files_are_rejected_while_shutdown_is_persisting() {
    let mut app = ready(Session::default());
    app.lifecycle.begin_shutdown();
    let receipt = crate::ipc::AdmissionReceipt::new();
    let _ = app.update(Message::ForwardedFiles(
        vec!["shutdown-forward.txt".into()],
        crate::ipc::ActivationRequest::default(),
        receipt.clone(),
    ));
    assert!(!receipt.wait_for_acceptance());
    assert_eq!(app.workspace.documents().len(), 1);
    assert!(app.workspace.active_document().unwrap().path.is_none());
}

#[test]
fn rejected_forwarding_cannot_open_late_and_accepted_paths_enter_session() {
    let mut app = ready(Session::default());
    let receipt = crate::ipc::AdmissionReceipt::new();
    receipt.resolve(false);
    let _ = app.update(Message::ForwardedFiles(
        vec!["late.txt".into()],
        crate::ipc::ActivationRequest::default(),
        receipt,
    ));
    assert!(app.workspace.active_document().unwrap().path.is_none());
    let receipt = crate::ipc::AdmissionReceipt::new();
    let path = std::path::absolute("accepted.txt").unwrap();
    let _ = app.update(Message::ForwardedFiles(
        vec![path.clone()],
        crate::ipc::ActivationRequest::default(),
        receipt.clone(),
    ));
    assert!(receipt.wait_for_acceptance());
    assert_eq!(
        app.snapshot_session().documents[0].path.as_ref(),
        Some(&path)
    );
    assert!(!app.lifecycle.is_exiting());
}

#[test]
fn replace_all_and_background_analysis_keep_scrolling_within_document() {
    for advanced in [false, true] {
        let mut app = ready(Session {
            documents: vec![SessionDocument {
                text: Some("line\n".repeat(100)),
                is_dirty: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let id = app.workspace.active_document_id();
        let _ = app.update(Message::EditorAction(id, EditorAction::ScrollToRow(80)));
        if advanced {
            app.search_dialog.query = "(?s).+".into();
            app.search_dialog.mode = crate::core::SearchMode::Regex;
            app.search_dialog.replacement = "X".into();
            let _ = app.update(Message::AdvancedReplaceAllCurrentRun);
        } else {
            let _ = app.update(Message::FindQueryChanged("line\n".repeat(100)));
            let _ = app.update(Message::FindReplacementChanged("X".into()));
            let _ = app.update(Message::ReplaceAll);
        }
        let document = app.workspace.document_mut(id).unwrap();
        assert_eq!(document.text(), "X");
        assert_eq!(document.scroll.first_visible_row, 0);
        let (buffer, request) = document.analysis_request().unwrap();
        assert!(document.apply_analysis(crate::core::document::analyze_document(buffer, request)));
        assert_eq!(document.scroll.first_visible_row, 0);
        assert_eq!(document.viewport.visible_row_count(), 1);
    }
}

#[test]
fn restoring_collapsed_folds_clamps_scroll_after_viewport_shrinks() {
    let mut document = Document::from_path(
        DocumentId::new(1),
        "scroll.rs",
        "fn main() {\n    a();\n    b();\n}\n",
    );
    document.scroll.first_visible_row = 4;
    document.restore_collapsed_folds(&[(0, 3)]);
    assert_eq!(document.viewport.visible_row_count(), 2);
    assert_eq!(document.scroll.first_visible_row, 1);
}
