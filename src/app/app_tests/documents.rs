use super::test_support::*;

#[test]
fn encoding_selection_reopens_clean_files_then_conversion_preserves_decoded_text() {
    use crate::core::{FileRevision, TextEncoding, encode_text};
    let original = encode_text("あ", TextEncoding::ShiftJis).unwrap();
    let file = TestFile::new(&original);
    let mut app = App::new().0;
    let _ = app.update(Message::FileOpened(Ok(OpenedFile {
        path: file.0.clone(),
        contents: Arc::new(crate::core::decode_bytes(&original)),
        disk_revision: FileRevision::from_bytes(&original),
    })));
    let id = app.workspace.active_document_id();
    assert_ne!(app.workspace.active_document().unwrap().text(), "あ");
    let task = app.update(Message::EncodingSelected(TextEncoding::ShiftJis));
    assert!(app.files.pending_reloads().contains_key(&id));
    run_task(&mut app, task);
    let document = app.workspace.active_document().unwrap();
    assert_eq!(document.text(), "あ");
    assert_eq!(document.encoding, TextEncoding::ShiftJis);
    assert!(!document.is_dirty);
    assert_eq!(document.bytes_for_save().unwrap(), original);

    let task = app.update(Message::ReloadFromDisk);
    run_task(&mut app, task);
    assert_eq!(app.workspace.active_document().unwrap().text(), "あ");
    assert_eq!(
        app.workspace.active_document().unwrap().encoding,
        TextEncoding::ShiftJis
    );

    let _ = app.update(Message::EncodingConverted(TextEncoding::Utf8));
    let document = app.workspace.active_document().unwrap();
    assert!(document.is_dirty);
    assert_eq!(document.text(), "あ");
    assert_eq!(document.bytes_for_save().unwrap(), "あ".as_bytes());
}

#[test]
fn encoding_reopen_guards_unsaved_edits_and_conversion_during_loading() {
    use crate::core::TextEncoding;
    let mut app = App::new().0;
    let _ = app.update(Message::FileOpened(Ok(OpenedFile {
        path: "edited.txt".into(),
        contents: Arc::new(crate::core::decode_bytes(b"original")),
        disk_revision: crate::core::FileRevision::from_bytes(b"original"),
    })));
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("edit".into()),
    ));
    let before = app.workspace.active_document().unwrap().text();
    let _ = app.update(Message::EncodingSelected(TextEncoding::ShiftJis));
    let document = app.workspace.active_document().unwrap();
    assert_eq!(document.text(), before);
    assert!(document.is_dirty);
    assert_eq!(document.encoding, TextEncoding::Utf8);
    assert!(app.files.pending_reloads().is_empty());
    assert_eq!(
        app.file_status.as_deref(),
        Some("Save changes before reloading from disk.")
    );

    app.workspace.active_document_mut().unwrap().mark_clean();
    let _ = app.update(Message::EncodingSelected(TextEncoding::ShiftJis));
    let _ = app.update(Message::EncodingConverted(TextEncoding::Utf16LeBom));
    assert_eq!(
        app.workspace.active_document().unwrap().encoding,
        TextEncoding::Utf8
    );
    assert_eq!(
        app.file_status.as_deref(),
        Some("Finish loading before changing encoding.")
    );
}

#[test]
fn failed_encoding_reopen_keeps_previous_encoding_text_and_history() {
    use crate::core::TextEncoding;
    let mut app = App::new().0;
    let id = app.workspace.active_document_id();
    app.workspace
        .active_document_mut()
        .unwrap()
        .set_path("existing.txt");
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("keep me".into()),
    ));
    app.workspace.active_document_mut().unwrap().mark_clean();
    assert!(app.workspace.active_document().unwrap().can_undo());
    let _ = app.update(Message::EncodingSelected(TextEncoding::ShiftJis));
    let generation = app
        .workspace
        .active_document()
        .unwrap()
        .load_generation()
        .unwrap();
    let _ = app.update(Message::FileLoadFinished(Err(FileLoadFailure {
        document_id: id,
        generation,
        path: "existing.txt".into(),
        error: crate::services::types::FileError::Io(std::io::ErrorKind::PermissionDenied),
    })));
    let document = app.workspace.active_document().unwrap();
    assert_eq!(document.text(), "keep me");
    assert_eq!(document.encoding, TextEncoding::Utf8);
    assert!(document.can_undo());
    assert!(!document.is_loading_or_indexing());
}

#[test]
fn manual_and_auto_save_conflicts_keep_unsaved_changes() {
    use crate::core::FileRevision;
    use crate::services::types::FileError;

    for auto_save in [false, true] {
        let mut app = App::new().0;
        let id = app.workspace.active_document_id();
        let document = app.workspace.document_mut(id).unwrap();
        document.set_path("conflicted.txt");
        let original = FileRevision::from_bytes(b"original");
        document.disk_revision = Some(original);
        let _ = app.update(Message::EditorAction(
            id,
            EditorAction::InsertText("unsaved changes".into()),
        ));
        app.settings.auto_save = auto_save;
        let _ = app.update(if auto_save {
            Message::NewFile
        } else {
            Message::SaveFile
        });
        let request = app.files.pending_save().unwrap().clone();
        assert_eq!(request.document_id, id);
        let _ = app.update(Message::FileSaved(request, Err(FileError::FileChanged)));
        let document = app.workspace.document(id).unwrap();
        assert_eq!(document.text(), "unsaved changes");
        assert!(document.is_dirty);
        assert_eq!(document.disk_revision, Some(original));
        assert!(app.files.pending_save().is_none());
        assert!(app.file_status.as_deref().unwrap().contains("file changed"));
    }
}

#[test]
fn save_conflict_cancels_close_without_discarding_the_document() {
    use crate::services::types::FileError;

    let mut app = App::new().0;
    let id = app.workspace.active_document_id();
    app.workspace
        .document_mut(id)
        .unwrap()
        .set_path("conflicted.txt");
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("unsaved changes".into()),
    ));
    let _ = app.update(Message::DirtyCloseResolved(id, DirtyCloseDecision::Save));
    let request = app.files.pending_save().unwrap().clone();
    let _ = app.update(Message::FileSaved(request, Err(FileError::FileChanged)));
    assert!(app.workspace.document(id).unwrap().is_dirty);
    assert_eq!(
        app.workspace.document(id).unwrap().text(),
        "unsaved changes"
    );
    assert!(app.files.pending_close_after_save().is_none());
}

#[test]
fn saving_a_copy_over_the_source_refreshes_its_revision_without_marking_clean() {
    let mut app = App::new().0;
    let id = app.workspace.active_document_id();
    let path = PathBuf::from("copy-source.txt");
    let document = app.workspace.document_mut(id).unwrap();
    document.set_path(path.clone());
    document.disk_revision = Some(crate::core::FileRevision::from_bytes(b"original"));
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("unsaved changes".into()),
    ));
    let _ = app.update(Message::SaveCopyAs);
    let request = app.files.pending_save().unwrap().clone();
    let saved_revision = crate::core::FileRevision::from_bytes(&request.snapshot);
    let _ = app.update(Message::FileCopySaved(request, Ok(path.clone())));
    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.path, Some(path));
    assert_eq!(document.disk_revision, Some(saved_revision));
    assert!(document.is_dirty);
}

#[test]
fn loading_file_is_inserted_before_chunks_finish() {
    let mut app = App::new().0;
    let path = PathBuf::from("large.txt");

    let _ = app.update(Message::FilePicked(Ok(path.clone())));

    let document = app
        .workspace
        .active_document()
        .expect("loading file should be active immediately");

    assert_eq!(document.path, Some(std::path::absolute(path).unwrap()));
    assert!(document.is_loading_or_indexing());
    assert_eq!(document.buffer.text(), "");
    assert!(app.files.is_loading());
}

#[test]
fn stale_load_generation_is_ignored() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loading.txt");
    let stale_generation = crate::core::DocumentLoadGeneration::next();

    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation: stale_generation,
        path: PathBuf::from("loading.txt"),
        text: Arc::new("stale".to_owned()),
        reset: false,
        bytes_read: 5,
        total_bytes: Some(5),
    }));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.load_generation(), Some(generation));
    assert_eq!(document.buffer.text(), "");
}

#[test]
fn stale_load_progress_does_not_update_matching_generation_progress() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loading.txt");
    let stale_generation = crate::core::DocumentLoadGeneration::next();

    let _ = app.update(Message::FileLoadProgress(
        crate::services::types::FileLoadProgress {
            document_id,
            generation,
            path: PathBuf::from("loading.txt"),
            bytes_read: 7,
            total_bytes: Some(20),
        },
    ));
    let _ = app.update(Message::FileLoadProgress(
        crate::services::types::FileLoadProgress {
            document_id,
            generation: stale_generation,
            path: PathBuf::from("loading.txt"),
            bytes_read: 20,
            total_bytes: Some(20),
        },
    ));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(
        document.load_state,
        crate::core::DocumentLoadState::Loading {
            generation,
            bytes_read: 7,
            total_bytes: Some(20),
        }
    );
}

#[test]
fn loading_preview_accumulates_chunks_for_matching_generation() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loading.txt");

    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: PathBuf::from("loading.txt"),
        text: Arc::new("alpha".to_owned()),
        reset: false,
        bytes_read: 5,
        total_bytes: Some(11),
    }));
    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: PathBuf::from("loading.txt"),
        text: Arc::new(" beta".to_owned()),
        reset: false,
        bytes_read: 11,
        total_bytes: Some(11),
    }));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.buffer.text(), "alpha beta");
    assert!(document.is_loading_or_indexing());
}

#[test]
fn stale_load_completion_does_not_clear_active_loading_state() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loading.txt");
    let stale_generation = crate::core::DocumentLoadGeneration::next();
    app.files.set_loading(true);

    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        document_id,
        generation: stale_generation,
        path: PathBuf::from("loading.txt"),
        encoding: crate::core::TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: 5,
        total_bytes: Some(5),
    })));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.load_generation(), Some(generation));
    assert_eq!(document.buffer.text(), "");
    assert!(document.is_loading_or_indexing());
    assert!(app.files.is_loading());
}

#[test]
fn load_completion_for_closed_document_is_ignored_and_refreshes_loading_state() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loading.txt");
    app.files.set_loading(true);

    let _ = app.workspace.close(document_id);
    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        document_id,
        generation,
        path: PathBuf::from("loading.txt"),
        encoding: crate::core::TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: 6,
        total_bytes: Some(6),
    })));

    assert!(app.workspace.document(document_id).is_none());
    assert!(!app.files.is_loading());
}

#[test]
fn completion_from_closed_then_reopened_path_cannot_mutate_new_generation() {
    let mut app = App::new().0;
    let path = PathBuf::from("loading.txt");
    let (closed_id, closed_generation) = app.workspace.insert_loading_file(path.clone());
    app.files.set_loading(true);

    let _ = app.workspace.close(closed_id);
    let (new_id, new_generation) = app.workspace.insert_loading_file(path.clone());
    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id: new_id,
        generation: new_generation,
        path: path.clone(),
        text: Arc::new("new preview".to_owned()),
        reset: false,
        bytes_read: 11,
        total_bytes: Some(20),
    }));
    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        document_id: closed_id,
        generation: closed_generation,
        path,
        encoding: crate::core::TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: 12,
        total_bytes: Some(12),
    })));

    assert!(app.workspace.document(closed_id).is_none());
    let document = app.workspace.document(new_id).expect("new document");
    assert_eq!(document.text(), "new preview");
    assert_eq!(document.load_generation(), Some(new_generation));
    assert!(app.files.is_loading());
}

#[test]
fn load_completion_applies_matching_generation_and_clears_indexing_state() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loaded.txt");
    app.files.set_loading(true);

    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: PathBuf::from("loaded.txt"),
        text: Arc::new("loaded body".to_owned()),
        reset: false,
        bytes_read: 11,
        total_bytes: Some(11),
    }));
    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        document_id,
        generation,
        path: PathBuf::from("loaded.txt"),
        encoding: crate::core::TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: 11,
        total_bytes: Some(11),
    })));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.buffer.text(), "loaded body");
    assert!(!document.is_loading_or_indexing());
    assert!(!app.files.is_loading());
}

#[test]
fn reload_from_disk_requires_saved_clean_non_loading_document() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::ReloadFromDisk);
    assert_eq!(
        app.file_status.as_deref(),
        Some("Reload from disk requires a saved file.")
    );

    let document_id = app.workspace.active_document_id();
    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.set_path("note.txt");
        document.mark_dirty();
    }

    let _ = app.update(Message::ReloadFromDisk);
    assert_eq!(
        app.file_status.as_deref(),
        Some("Save changes before reloading from disk.")
    );

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.mark_clean();
        document.load_state = crate::core::DocumentLoadState::Loading {
            generation: crate::core::DocumentLoadGeneration::next(),
            bytes_read: 0,
            total_bytes: None,
        };
    }

    let _ = app.update(Message::ReloadFromDisk);
    assert_eq!(
        app.file_status.as_deref(),
        Some("Finish loading before reloading.")
    );
}

#[test]
fn reload_from_disk_reuses_active_document_and_chunked_completion() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let path = PathBuf::from("note.txt");

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.set_path(path.clone());
        document.buffer = crate::editor::EditorBuffer::from_text("old body");
        document.refresh_after_text_change();
        document.mark_clean();
    }

    let _ = app.update(Message::ReloadFromDisk);
    let generation = app
        .workspace
        .document(document_id)
        .expect("document")
        .load_generation()
        .expect("reload generation");

    assert_eq!(app.workspace.active_document_id(), document_id);
    assert!(app.files.is_loading());
    assert_eq!(
        app.workspace
            .document(document_id)
            .expect("document")
            .text(),
        "old body"
    );

    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: path.clone(),
        text: Arc::new("new body".to_owned()),
        reset: false,
        bytes_read: 8,
        total_bytes: Some(8),
    }));
    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        document_id,
        generation,
        path: path.clone(),
        encoding: crate::core::TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: 8,
        total_bytes: Some(8),
    })));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.path.as_deref(), Some(path.as_path()));
    assert_eq!(document.text(), "new body");
    assert!(!document.is_dirty);
    assert!(!app.files.is_loading());
}

#[test]
fn failed_load_sets_status_without_leaving_document_indexing() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("missing.txt");
    app.files.set_loading(true);

    let _ = app.update(Message::FileLoadFinished(Err(FileLoadFailure {
        document_id,
        generation,
        path: PathBuf::from("missing.txt"),
        error: crate::services::types::FileError::Io(std::io::ErrorKind::NotFound),
    })));

    let document = app.workspace.document(document_id).expect("document");
    assert!(!document.is_loading_or_indexing());
    assert_eq!(app.file_status.as_deref(), Some("Open failed: I/O error"));
    assert!(!app.files.is_loading());
}

#[test]
fn failed_reload_preserves_original_text_history_and_save_snapshot() {
    let mut app = App::new().0;
    let document_id = app.workspace.active_document_id();
    let path = PathBuf::from("reload.txt");
    let _ = app.update(Message::EditorAction(
        document_id,
        crate::editor::EditorAction::InsertText("original".to_owned()),
    ));
    let document = app.workspace.document_mut(document_id).expect("document");
    document.set_path(path.clone());
    document.mark_clean();
    let original_history = document.history.clone();
    let original_bytes = document.bytes_for_save().expect("snapshot");

    let _ = app.update(Message::ReloadFromDisk);
    let generation = app
        .workspace
        .document(document_id)
        .unwrap()
        .load_generation()
        .unwrap();
    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: path.clone(),
        text: Arc::new("partial".into()),
        reset: false,
        bytes_read: 7,
        total_bytes: Some(20),
    }));
    assert_eq!(
        app.workspace.document(document_id).unwrap().text(),
        "original"
    );
    let _ = app.update(Message::FileLoadFinished(Err(FileLoadFailure {
        document_id,
        generation,
        path,
        error: crate::services::types::FileError::Io(std::io::ErrorKind::UnexpectedEof),
    })));
    let document = app.workspace.document(document_id).unwrap();
    assert_eq!(document.text(), "original");
    assert_eq!(document.history, original_history);
    assert_eq!(document.bytes_for_save().unwrap(), original_bytes);
    assert!(!document.is_loading_or_indexing());
    assert!(!document.is_dirty);
    assert!(app.files.pending_reloads().is_empty());
}

#[test]
fn failed_initial_load_blocks_save_and_save_copy() {
    let mut app = App::new().0;
    let path = PathBuf::from("partial.txt");
    let (document_id, generation) = app.workspace.insert_loading_file(path.clone());
    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: path.clone(),
        text: Arc::new("partial".into()),
        reset: false,
        bytes_read: 7,
        total_bytes: Some(20),
    }));
    let _ = app.update(Message::FileLoadFinished(Err(FileLoadFailure {
        document_id,
        generation,
        path,
        error: crate::services::types::FileError::Io(std::io::ErrorKind::UnexpectedEof),
    })));
    for message in [Message::SaveFile, Message::SaveFileAs, Message::SaveCopyAs] {
        let _ = app.update(message);
        assert!(app.files.pending_save().is_none());
        assert_eq!(
            app.file_status.as_deref(),
            Some("Reload the file successfully before saving.")
        );
    }
}

#[test]
fn undo_during_save_cannot_keep_the_previous_clean_checkpoint() {
    let mut app = App::new().0;
    let document_id = app.workspace.active_document_id();
    let path = PathBuf::from("save-race.txt");
    let _ = app.update(Message::EditorAction(
        document_id,
        crate::editor::EditorAction::InsertText("a".to_owned()),
    ));
    let document = app.workspace.document_mut(document_id).unwrap();
    document.set_path(path.clone());
    document.mark_clean();
    let _ = app.update(Message::EditorAction(
        document_id,
        crate::editor::EditorAction::InsertText("b".to_owned()),
    ));
    let _ = app.update(Message::SaveFile);
    let request = app.files.pending_save().cloned().expect("pending save");
    let _ = app.update(Message::Undo);
    assert!(!app.workspace.document(document_id).unwrap().is_dirty);
    let _ = app.update(Message::CloseFile);
    assert!(app.workspace.document(document_id).is_some());
    let _ = app.update(Message::FileSaved(request, Ok(path)));
    assert_eq!(app.workspace.document(document_id).unwrap().text(), "a");
    assert!(app.workspace.document(document_id).unwrap().is_dirty);
    let _ = app.update(Message::Redo);
    let _ = app.update(Message::Undo);
    assert!(app.workspace.document(document_id).unwrap().is_dirty);
    let _ = app.update(Message::CloseFile);
    assert_eq!(app.close_prompt.document(), Some(document_id));
}

#[test]
fn dropped_file_completion_opens_document_through_existing_open_path() {
    let (mut app, _) = App::new();
    let main_window = app.main_window_id.expect("main window id");
    let path = PathBuf::from("dropped.txt");
    let contents = Arc::new(crate::core::decode_bytes(b"dropped body"));

    let _ = app.update(Message::FileDropped(main_window, path.clone()));
    assert!(app.files.is_loading());

    let _ = app.update(Message::FileOpened(Ok(OpenedFile {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        path: path.clone(),
        contents,
    })));

    let document = app
        .workspace
        .active_document()
        .expect("dropped file should open as active document");

    assert_eq!(document.path, Some(std::path::absolute(path).unwrap()));
    assert_eq!(document.buffer.text(), "dropped body");
    assert!(!app.files.is_loading());
}

#[test]
fn dropped_files_on_non_main_windows_are_ignored() {
    let (mut app, _) = App::new();
    let original_document_id = app.workspace.active_document_id();
    let secondary_window = iced::window::Id::unique();

    let _ = app.update(Message::FileDropped(
        secondary_window,
        PathBuf::from("ignored.txt"),
    ));

    assert_eq!(app.workspace.active_document_id(), original_document_id);
    assert_eq!(app.workspace.documents().len(), 1);
    assert!(!app.files.is_loading());
}

#[test]
fn dropped_files_still_schedule_while_a_previous_drop_is_loading() {
    let (mut app, _) = App::new();
    let main_window = app.main_window_id.expect("main window id");

    app.file_status = Some("stale status".to_owned());
    app.files.set_loading(true);

    let _ = app.update(Message::FileDropped(
        main_window,
        PathBuf::from("second-drop.txt"),
    ));

    assert!(app.files.is_loading());
    assert_eq!(app.file_status, None);
}

#[test]
fn manual_non_plain_language_selection_survives_save_as() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    let _ = app.update(Message::LanguageSelected("rs".to_owned()));
    let revision = app
        .workspace
        .document(document_id)
        .expect("document")
        .revision();
    let request = SaveRequest {
        document_id,
        revision,
        snapshot: Arc::new(Vec::new()),
    };
    let _ = app.update(Message::FileSaved(request, Ok(PathBuf::from("README"))));

    let document = app
        .workspace
        .active_document()
        .expect("workspace should have an active document");

    assert_eq!(document.syntax_token, "rs");
}

#[test]
fn plain_text_language_selection_returns_to_auto_detection_on_save_as() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    let _ = app.update(Message::LanguageSelected("txt".to_owned()));
    let revision = app
        .workspace
        .document(document_id)
        .expect("document")
        .revision();
    let request = SaveRequest {
        document_id,
        revision,
        snapshot: Arc::new(Vec::new()),
    };
    let _ = app.update(Message::FileSaved(request, Ok(PathBuf::from("main.c"))));

    let document = app
        .workspace
        .active_document()
        .expect("workspace should have an active document");

    assert_eq!(document.syntax_token, "c");
}

#[test]
fn save_file_is_blocked_while_document_is_loading() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loading.txt");
    app.workspace.select(document_id);

    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: PathBuf::from("loading.txt"),
        text: Arc::new("partial preview".to_owned()),
        reset: false,
        bytes_read: 15,
        total_bytes: Some(100),
    }));
    let _ = app.update(Message::SaveFile);

    assert!(app.files.pending_save().is_none());
    assert_eq!(
        app.file_status.as_deref(),
        Some("Finish loading before saving.")
    );
}

#[test]
fn editor_mutation_is_blocked_while_document_is_loading() {
    let mut app = App::new().0;
    let (document_id, generation) = app.workspace.insert_loading_file("loading.txt");
    app.workspace.select(document_id);
    app.files.set_loading(true);

    let _ = app.update(Message::EditorAction(
        document_id,
        crate::editor::EditorAction::InsertText("typed".to_owned()),
    ));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.text(), "");
    assert_eq!(
        app.file_status.as_deref(),
        Some("Finish loading before editing.")
    );

    let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
        document_id,
        generation,
        path: PathBuf::from("loading.txt"),
        text: Arc::new("loaded".to_owned()),
        reset: false,
        bytes_read: 6,
        total_bytes: Some(6),
    }));
    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        document_id,
        generation,
        path: PathBuf::from("loading.txt"),
        encoding: crate::core::TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: 6,
        total_bytes: Some(6),
    })));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.text(), "loaded");
    assert!(!document.is_dirty);
}

#[test]
fn save_completion_does_not_mark_clean_after_document_changes() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let path = PathBuf::from("note.txt");

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.set_path(path.clone());
        document.buffer = crate::editor::EditorBuffer::from_text("saved");
        document.refresh_after_text_change();
    }

    let request = app
        .workspace
        .document(document_id)
        .map(|document| SaveRequest {
            document_id,
            revision: document.revision(),
            snapshot: Arc::new(document.bytes_for_save().expect("snapshot")),
        })
        .expect("document");

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.buffer = crate::editor::EditorBuffer::from_text("changed");
        document.refresh_after_text_change();
        document.mark_dirty();
    }

    let saved_revision = crate::core::FileRevision::from_bytes(&request.snapshot);
    let _ = app.update(Message::FileSaved(request, Ok(path)));
    assert_eq!(
        app.workspace.document(document_id).unwrap().disk_revision,
        Some(saved_revision)
    );

    assert!(
        app.workspace
            .document(document_id)
            .expect("document")
            .is_dirty
    );
}

#[test]
fn save_completion_does_not_mark_clean_after_encoding_changes() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let path = PathBuf::from("note.txt");

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.set_path(path.clone());
        document.buffer = crate::editor::EditorBuffer::from_text("saved");
        document.refresh_after_text_change();
        document.mark_clean();
    }

    let request = app
        .workspace
        .document(document_id)
        .map(|document| SaveRequest {
            document_id,
            revision: document.revision(),
            snapshot: Arc::new(document.bytes_for_save().expect("snapshot")),
        })
        .expect("document");

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.set_encoding(crate::core::TextEncoding::Utf8Bom);
    }

    let _ = app.update(Message::FileSaved(request, Ok(path)));

    assert!(
        app.workspace
            .document(document_id)
            .expect("document")
            .is_dirty
    );
}

#[test]
fn save_copy_as_starts_pending_snapshot_without_changing_document() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let original_path = PathBuf::from("note.txt");

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.set_path(original_path.clone());
        document.buffer = crate::editor::EditorBuffer::from_text("copy body");
        document.refresh_after_text_change();
        document.mark_dirty();
    }

    let _ = app.update(Message::SaveCopyAs);

    let request = app
        .files
        .pending_save()
        .expect("save copy should create pending request");
    assert_eq!(request.document_id, document_id);
    assert_eq!(request.snapshot.as_ref().as_slice(), b"copy body\n");

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.path.as_deref(), Some(original_path.as_path()));
    assert!(document.is_dirty);
}

#[test]
fn save_copy_completion_preserves_document_path_and_dirty_state() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let original_path = PathBuf::from("note.txt");

    {
        let document = app.workspace.document_mut(document_id).expect("document");
        document.set_path(original_path.clone());
        document.buffer = crate::editor::EditorBuffer::from_text("copy body");
        document.refresh_after_text_change();
        document.mark_dirty();
    }

    let request = app
        .workspace
        .document(document_id)
        .map(|document| SaveRequest {
            document_id,
            revision: document.revision(),
            snapshot: Arc::new(document.bytes_for_save().expect("snapshot")),
        })
        .expect("document");
    app.files.set_pending_save(request.clone());

    let _ = app.update(Message::FileCopySaved(
        request,
        Ok(PathBuf::from("copy.txt")),
    ));

    let document = app.workspace.document(document_id).expect("document");
    assert_eq!(document.path.as_deref(), Some(original_path.as_path()));
    assert!(document.is_dirty);
    assert!(app.files.pending_save().is_none());
    assert_eq!(app.file_status.as_deref(), Some("Saved copy: copy.txt"));
}

#[test]
fn dirty_close_discard_closes_document() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    app.workspace
        .active_document_mut()
        .expect("active document")
        .mark_dirty();

    let _ = app.update(Message::DirtyCloseResolved(
        document_id,
        DirtyCloseDecision::Discard,
    ));

    assert_ne!(app.workspace.active_document_id(), document_id);
    assert!(app.workspace.document(document_id).is_none());
}

#[test]
fn dirty_close_cancel_keeps_document_open() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    app.workspace
        .active_document_mut()
        .expect("active document")
        .mark_dirty();

    let _ = app.update(Message::DirtyCloseResolved(
        document_id,
        DirtyCloseDecision::Cancel,
    ));

    assert!(app.workspace.document(document_id).is_some());
    assert_eq!(app.workspace.active_document_id(), document_id);
    assert_eq!(app.close_prompt.document(), None);
}

#[test]
fn closing_dirty_document_opens_in_app_prompt() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    app.workspace
        .active_document_mut()
        .expect("active document")
        .mark_dirty();

    let _ = app.update(Message::CloseFile);

    assert_eq!(app.close_prompt.document(), Some(document_id));
    assert!(app.workspace.document(document_id).is_some());
}

#[test]
fn dirty_close_save_closes_after_successful_save() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let path = PathBuf::from("note.txt");

    {
        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        document.set_path(path.clone());
        document.mark_dirty();
    }

    let _ = app.update(Message::DirtyCloseResolved(
        document_id,
        DirtyCloseDecision::Save,
    ));

    let request = app
        .files
        .pending_save()
        .cloned()
        .expect("dirty close save should start a save");
    let _ = app.update(Message::FileSaved(request, Ok(path)));

    assert!(app.workspace.document(document_id).is_none());
}

#[test]
fn dirty_close_save_encoding_failure_keeps_document_open_and_clears_pending_close() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    {
        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        document.buffer = crate::editor::EditorBuffer::from_text("\u{20ac}");
        document.set_encoding(crate::core::TextEncoding::Iso8859_1);
    }

    let _ = app.update(Message::DirtyCloseResolved(
        document_id,
        DirtyCloseDecision::Save,
    ));

    assert!(app.files.pending_save().is_none());
    assert_eq!(app.files.pending_close_after_save(), None);
    assert!(app.files.pending_close_documents().is_empty());
    assert!(app.workspace.document(document_id).is_some());
    assert_eq!(
        app.file_status.as_deref(),
        Some("Save failed: encoding error")
    );
}

#[test]
fn file_open_error_sets_visible_status() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FileOpened(Err(
        crate::services::types::FileError::Io(std::io::ErrorKind::PermissionDenied),
    )));

    assert_eq!(app.file_status.as_deref(), Some("Open failed: I/O error"));
    assert!(!app.files.is_loading());
}

#[test]
fn save_all_queues_dirty_documents_and_skips_clean_documents() {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();

    {
        let document = app.workspace.document_mut(first).expect("first document");
        document.set_path("first.txt");
        document.mark_dirty();
    }
    {
        let document = app.workspace.document_mut(second).expect("second document");
        document.set_path("second.txt");
    }
    {
        let document = app.workspace.document_mut(third).expect("third document");
        document.set_path("third.txt");
        document.mark_dirty();
    }

    let _ = app.update(Message::SaveAllFiles);

    assert_eq!(
        app.files.pending_save().map(|request| request.document_id),
        Some(first)
    );
    assert_eq!(pending_save_all_ids(&app), vec![first, third]);

    let first_request = app
        .files
        .pending_save()
        .cloned()
        .expect("first save request");
    let _ = app.update(Message::FileSaved(
        first_request,
        Ok(PathBuf::from("first.txt")),
    ));

    assert_eq!(
        app.files.pending_save().map(|request| request.document_id),
        Some(third)
    );
    assert_eq!(pending_save_all_ids(&app), vec![third]);

    let third_request = app
        .files
        .pending_save()
        .cloned()
        .expect("third save request");
    let _ = app.update(Message::FileSaved(
        third_request,
        Ok(PathBuf::from("third.txt")),
    ));

    assert!(app.files.pending_save().is_none());
    assert!(app.files.pending_save_all().is_empty());
    assert!(!app.workspace.document(first).expect("first").is_dirty);
    assert!(!app.workspace.document(second).expect("second").is_dirty);
    assert!(!app.workspace.document(third).expect("third").is_dirty);
}

#[test]
fn save_all_stops_after_failed_save() {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();

    {
        let document = app.workspace.document_mut(first).expect("first document");
        document.set_path("first.txt");
        document.mark_dirty();
    }
    {
        let document = app.workspace.document_mut(second).expect("second document");
        document.set_path("second.txt");
        document.mark_dirty();
    }

    let _ = app.update(Message::SaveAllFiles);

    let first_request = app
        .files
        .pending_save()
        .cloned()
        .expect("first save request");
    let _ = app.update(Message::FileSaved(
        first_request,
        Err(crate::services::types::FileError::Io(
            std::io::ErrorKind::Other,
        )),
    ));

    assert!(app.files.pending_save().is_none());
    assert!(app.files.pending_save_all().is_empty());
    assert!(app.workspace.document(first).expect("first").is_dirty);
    assert!(app.workspace.document(second).expect("second").is_dirty);
}

#[test]
fn auto_save_on_tab_switch_saves_named_dirty_document_without_save_as() {
    let (mut app, _) = App::new();
    app.settings.auto_save = true;
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    app.workspace.select(first);

    let path = PathBuf::from("auto-save.txt");
    let document = app.workspace.document_mut(first).expect("first document");
    document.set_path(path.clone());
    document.mark_dirty();

    let _ = app.update(Message::TabSelected(second));

    assert_eq!(app.workspace.active_document_id(), second);
    let request = app.files.pending_save().expect("auto-save request");
    assert_eq!(request.document_id, first);
    assert_eq!(request.snapshot.as_ref(), b"");
    assert!(app.files.pending_auto_saves().is_empty());
}

#[test]
fn auto_save_does_not_open_save_as_for_untitled_document() {
    let (mut app, _) = App::new();
    app.settings.auto_save = true;
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    app.workspace.select(first);
    app.workspace
        .document_mut(first)
        .expect("first document")
        .mark_dirty();

    let _ = app.update(Message::TabSelected(second));

    assert!(app.files.pending_save().is_none());
    assert!(app.files.pending_auto_saves().is_empty());
}

#[test]
fn auto_save_queues_tab_switches_while_a_write_is_in_flight() {
    let (mut app, _) = App::new();
    app.settings.auto_save = true;
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();
    app.workspace.select(first);

    let first_path = PathBuf::from("first-auto-save.txt");
    let second_path = PathBuf::from("second-auto-save.txt");
    {
        let document = app.workspace.document_mut(first).expect("first document");
        document.set_path(first_path.clone());
        document.mark_dirty();
    }
    {
        let document = app.workspace.document_mut(second).expect("second document");
        document.set_path(second_path.clone());
        document.mark_dirty();
    }

    let _ = app.update(Message::TabSelected(second));
    let first_request = app.files.pending_save().cloned().expect("first save");
    let _ = app.update(Message::TabSelected(third));

    assert_eq!(pending_auto_save_ids(&app), vec![second]);
    let _ = app.update(Message::FileSaved(first_request, Ok(first_path)));

    assert_eq!(
        app.files.pending_save().map(|request| request.document_id),
        Some(second)
    );
    assert!(app.files.pending_auto_saves().is_empty());
}

#[test]
fn auto_save_continues_after_save_copy_completes() {
    let (mut app, _) = App::new();
    app.settings.auto_save = true;
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    app.workspace.select(first);

    let path = PathBuf::from("copy-source.txt");
    let document = app.workspace.document_mut(first).expect("first document");
    document.set_path(path);
    document.mark_dirty();

    let _ = app.update(Message::SaveCopyAs);
    let copy_request = app.files.pending_save().cloned().expect("copy request");

    let _ = app.update(Message::TabSelected(second));
    assert_eq!(pending_auto_save_ids(&app), vec![first]);

    let _ = app.update(Message::FileCopySaved(
        copy_request,
        Ok(PathBuf::from("copy-target.txt")),
    ));

    assert_eq!(
        app.files.pending_save().map(|request| request.document_id),
        Some(first)
    );
    assert!(app.files.pending_auto_saves().is_empty());
}

#[test]
fn auto_save_continues_after_encoding_failure() {
    let (mut app, _) = App::new();
    app.settings.auto_save = true;
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();
    let fourth = app.workspace.create_untitled();

    {
        let document = app.workspace.document_mut(first).expect("first document");
        document.buffer = EditorBuffer::from_text("\u{20ac}");
        document.refresh_after_text_change();
        document.set_encoding(crate::core::TextEncoding::Iso8859_1);
        document.set_path("encoding-failure.txt");
        document.mark_dirty();
    }
    {
        let document = app.workspace.document_mut(second).expect("second document");
        document.set_path("queued-after-failure.txt");
        document.mark_dirty();
    }
    {
        let document = app.workspace.document_mut(third).expect("third document");
        document.set_path("copy-source-for-failure.txt");
        document.mark_dirty();
    }

    app.workspace.select(third);
    let _ = app.update(Message::SaveCopyAs);
    let copy_request = app.files.pending_save().cloned().expect("copy request");

    let _ = app.update(Message::TabSelected(first));
    let _ = app.update(Message::TabSelected(second));
    let _ = app.update(Message::TabSelected(fourth));
    assert_eq!(pending_auto_save_ids(&app), vec![third, first, second]);

    let _ = app.update(Message::FileCopySaved(
        copy_request,
        Ok(PathBuf::from("copy-target-for-failure.txt")),
    ));
    let third_request = app.files.pending_save().cloned().expect("third save");
    let _ = app.update(Message::FileSaved(
        third_request,
        Ok(PathBuf::from("copy-source-for-failure.txt")),
    ));

    assert_eq!(
        app.files.pending_save().map(|request| request.document_id),
        Some(second)
    );
    assert!(app.files.pending_auto_saves().is_empty());
}

#[test]
fn close_all_but_active_keeps_active_document_and_closes_clean_neighbors() {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();

    app.workspace.select(second);

    let _ = app.update(Message::CloseAllButActiveFile);

    assert!(app.workspace.document(first).is_none());
    assert!(app.workspace.document(third).is_none());
    assert!(app.workspace.document(second).is_some());
    assert_eq!(app.workspace.active_document_id(), second);
}

#[test]
fn close_all_but_pinned_keeps_pinned_documents_open() {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();

    assert!(app.workspace.toggle_pin(first));
    assert!(app.workspace.toggle_pin(third));

    let _ = app.update(Message::CloseAllButPinnedFiles);

    assert!(app.workspace.document(first).is_some());
    assert!(app.workspace.document(second).is_none());
    assert!(app.workspace.document(third).is_some());
}

#[test]
fn close_all_to_left_prompts_for_first_dirty_left_document() {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();

    app.workspace
        .document_mut(first)
        .expect("first")
        .mark_dirty();
    app.workspace.select(third);

    let _ = app.update(Message::CloseAllToLeft);

    assert_eq!(app.close_prompt.document(), Some(first));
    assert_eq!(
        app.files
            .pending_close_documents()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![second]
    );
    assert!(app.workspace.document(first).is_some());
    assert!(app.workspace.document(second).is_some());
    assert!(app.workspace.document(third).is_some());
}

#[test]
fn dirty_close_discard_continues_pending_close_queue() {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();

    app.workspace
        .document_mut(first)
        .expect("first")
        .mark_dirty();
    app.workspace.select(third);

    let _ = app.update(Message::CloseAllToLeft);
    let _ = app.update(Message::DirtyCloseResolved(
        first,
        DirtyCloseDecision::Discard,
    ));

    assert!(app.workspace.document(first).is_none());
    assert!(app.workspace.document(second).is_none());
    assert!(app.workspace.document(third).is_some());
    assert!(app.files.pending_close_documents().is_empty());
}

#[test]
fn close_all_unchanged_keeps_dirty_documents_open() {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();

    app.workspace
        .document_mut(second)
        .expect("second")
        .mark_dirty();

    let _ = app.update(Message::CloseAllUnchanged);

    assert!(app.workspace.document(first).is_none());
    assert!(app.workspace.document(third).is_none());
    assert!(app.workspace.document(second).is_some());
}
