use super::*;
use crate::app::CloseGoal;
use crate::app::tests::test_support::{TestFile, run_task};
use crate::editor::EditorAction;
use crate::services::file_system;

fn open_file(app: &mut App, file: &TestFile) {
    let task = Task::perform(file_system::load_file(file.0.clone()), Message::FileOpened);
    run_task(app, task);
}

#[test]
fn deleted_clean_file_keeps_text_and_undo_history() {
    let file = TestFile::new(b"original text");
    let mut app = App::new().0;
    open_file(&mut app, &file);
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::EditorAction(id, EditorAction::SelectAll));
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("saved after editing".into()),
    ));
    let task = app.update(Message::SaveFile);
    run_task(&mut app, task);
    assert!(!app.workspace.document(id).unwrap().is_dirty);
    assert!(app.workspace.document(id).unwrap().can_undo());
    std::fs::remove_file(&file.0).unwrap();

    let task = app.update(Message::CheckDiskFiles);
    run_task(&mut app, task);

    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "saved after editing");
    assert_eq!(document.disk_state, DiskState::Missing);
    assert!(document.is_dirty);
    assert!(document.can_undo());
    let _ = app.update(Message::EditorAction(id, EditorAction::Undo));
    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "original text");
    assert!(document.is_dirty);
}

#[test]
fn changed_disk_file_preserves_unsaved_edits_and_external_text() {
    let file = TestFile::new(b"original text");
    let mut app = App::new().0;
    open_file(&mut app, &file);
    let id = app.workspace.active_document_id();
    let original_revision = app.workspace.document(id).unwrap().disk_revision;
    let _ = app.update(Message::EditorAction(id, EditorAction::SelectAll));
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("unsaved editor text".into()),
    ));
    std::fs::write(&file.0, b"external text").unwrap();

    let task = app.check_disk_files();
    run_task(&mut app, task);

    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "unsaved editor text");
    assert_eq!(document.disk_state, DiskState::Changed);
    assert_eq!(document.disk_revision, original_revision);
    assert!(document.is_dirty);
    assert!(document.can_undo());
    assert_eq!(std::fs::read(&file.0).unwrap(), b"external text");
}

#[test]
fn closing_before_the_next_poll_keeps_a_deleted_clean_file_open() {
    let file = TestFile::new(b"only editor copy remains");
    let mut app = App::new().0;
    open_file(&mut app, &file);
    let id = app.workspace.active_document_id();
    std::fs::remove_file(&file.0).unwrap();

    let task = app.update(Message::TabClosed(id));
    assert!(app.workspace.document(id).is_some());
    run_task(&mut app, task);

    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "only editor copy remains");
    assert_eq!(document.disk_state, DiskState::Missing);
    assert!(document.is_dirty);
    assert_eq!(app.close_prompt.document(), Some(id));
    assert!(!app.lifecycle.is_exiting());
}

#[test]
fn closing_during_reload_checks_and_preserves_the_previous_complete_text() {
    let file = TestFile::new(b"previous complete text");
    let mut app = App::new().0;
    open_file(&mut app, &file);
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::ReloadFromDisk);
    let generation = app
        .workspace
        .document(id)
        .unwrap()
        .load_generation()
        .unwrap();
    std::fs::remove_file(&file.0).unwrap();

    let task = app.update(Message::TabClosed(id));
    run_task(&mut app, task);
    let _ = app.update(Message::FileLoadChunk(
        crate::services::types::FileLoadChunk {
            document_id: id,
            generation,
            path: file.0.clone(),
            text: std::sync::Arc::new("late partial text".into()),
            reset: true,
            bytes_read: 17,
            total_bytes: None,
        },
    ));

    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "previous complete text");
    assert!(document.has_complete_text_index());
    assert_eq!(document.disk_state, DiskState::Missing);
    assert!(document.is_dirty);
    assert_eq!(app.close_prompt.document(), Some(id));
    assert!(!app.files.is_reloading(id));
}

#[test]
fn bulk_close_and_exit_without_sessions_check_clean_files_before_discarding_them() {
    for quitting in [false, true] {
        let missing = TestFile::new(b"keep this deleted file");
        let other = TestFile::new(b"another open file");
        let mut app = App::new().0;
        assert!(!app.session.is_enabled());
        open_file(&mut app, &missing);
        let missing_id = app.workspace.active_document_id();
        open_file(&mut app, &other);
        let other_id = app.workspace.active_document_id();
        std::fs::remove_file(&missing.0).unwrap();

        let task = app.update(if quitting {
            Message::Quit
        } else {
            Message::CloseAllFiles
        });
        run_task(&mut app, task);

        let document = app.workspace.document(missing_id).unwrap();
        assert_eq!(document.text(), "keep this deleted file");
        assert_eq!(document.disk_state, DiskState::Missing);
        assert!(document.is_dirty);
        assert_eq!(app.close_prompt.document(), Some(missing_id));
        assert!(app.workspace.document(other_id).is_some());
        assert!(!app.lifecycle.is_exiting());
        assert_eq!(
            app.files.close_goal,
            if quitting {
                CloseGoal::ExitApp
            } else {
                CloseGoal::KeepOpen
            }
        );
    }
}

#[test]
fn cached_polling_keeps_the_warning_until_original_disk_text_returns() {
    let file = TestFile::new(b"original text");
    let mut app = App::new().0;
    open_file(&mut app, &file);
    let id = app.workspace.active_document_id();
    let task = app.poll_disk_files();
    run_task(&mut app, task);
    std::fs::write(&file.0, b"external text is longer").unwrap();
    let task = app.poll_disk_files();
    run_task(&mut app, task);
    assert_eq!(
        app.workspace.document(id).unwrap().disk_state,
        DiskState::Changed
    );

    let task = app.poll_disk_files();
    run_task(&mut app, task);
    assert!(app.files.disk_stamps[&id].state.is_none());
    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.disk_state, DiskState::Changed);
    assert_eq!(document.text(), "original text");
    assert!(document.is_dirty);

    std::fs::write(&file.0, b"original text").unwrap();
    let task = app.poll_disk_files();
    run_task(&mut app, task);
    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.disk_state, DiskState::Current);
    assert_eq!(document.text(), "original text");
}

#[test]
fn an_old_disk_check_cannot_dirty_a_successful_save_as() {
    let source = TestFile::new(b"saved editor text");
    let destination = TestFile::new(b"");
    let mut app = App::new().0;
    open_file(&mut app, &source);
    let id = app.workspace.active_document_id();
    std::fs::remove_file(&source.0).unwrap();
    let old_check = app.check_disk_files();

    let _ = app.update(Message::SaveFileAs);
    let request = app.files.pending_save.clone().unwrap();
    let task = Task::perform(
        file_system::save_file(destination.0.clone(), request.snapshot.as_ref().clone()),
        move |result| Message::FileSaved(request.clone(), result),
    );
    run_task(&mut app, task);
    run_task(&mut app, old_check);

    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.path.as_ref(), Some(&destination.0));
    assert_eq!(document.text(), "saved editor text");
    assert_eq!(document.disk_state, DiskState::Current);
    assert!(!document.is_dirty);
    assert_eq!(std::fs::read(&destination.0).unwrap(), b"saved editor text");
}

#[test]
fn an_older_background_result_cannot_clear_the_warning_found_while_closing() {
    let file = TestFile::new(b"keep this text");
    let mut app = App::new().0;
    open_file(&mut app, &file);
    let id = app.workspace.active_document_id();
    let mut check = file_check(app.workspace.document(id).unwrap()).unwrap();
    app.assign_disk_check_id(&mut check);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let old_result = runtime.block_on(file_system::check_file(check));
    assert_eq!(old_result.state, Some(DiskState::Current));
    std::fs::remove_file(&file.0).unwrap();

    let close = app.update(Message::TabClosed(id));
    run_task(&mut app, close);
    let _ = app.update(Message::DiskFilesChecked(vec![old_result]));

    assert_eq!(
        app.workspace.document(id).unwrap().disk_state,
        DiskState::Missing
    );
    assert!(app.workspace.document(id).unwrap().is_dirty);
    assert_eq!(app.close_prompt.document(), Some(id));
}

fn finish_close_checks(app: &mut App, task: Task<Message>) {
    use futures::StreamExt;
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let mut tasks = vec![task];
            while let Some(task) = tasks.pop() {
                if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                    while let Some(action) = stream.next().await {
                        if let iced_runtime::Action::Output(message) = action {
                            tasks.push(app.update(message));
                        }
                    }
                }
            }
        });
}

#[test]
fn cancelling_exit_keeps_previously_checked_clean_tabs_open() {
    let clean = TestFile::new(b"still on disk");
    let missing = TestFile::new(b"only in editor");
    let mut app = App::new().0;
    open_file(&mut app, &clean);
    let clean_id = app.workspace.active_document_id();
    open_file(&mut app, &missing);
    let missing_id = app.workspace.active_document_id();
    std::fs::remove_file(&missing.0).unwrap();

    let quit = app.update(Message::Quit);
    finish_close_checks(&mut app, quit);
    assert_eq!(app.close_prompt.document(), Some(missing_id));
    assert!(app.workspace.document(clean_id).is_some());
    let _ = app.resolve_close(missing_id, crate::core::DirtyCloseDecision::Cancel);
    assert!(app.workspace.document(clean_id).is_some());
    assert!(app.workspace.document(missing_id).is_some());
    assert!(!app.lifecycle.is_exiting());
}

fn app_with_unreadable_session() -> App {
    let mut app = App::new_with_options(crate::startup::StartupOptions::default()).0;
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Err("damaged session".into())));
    let _ = app.update(Message::StartupReady);
    assert!(app.session.is_enabled());
    assert!(app.session.is_initialized());
    assert!(app.session.read_failed());
    app
}

#[test]
fn an_unreadable_session_does_not_allow_quit_to_drop_a_deleted_clean_file() {
    let file = TestFile::new(b"only editor copy remains");
    let mut app = app_with_unreadable_session();
    open_file(&mut app, &file);
    let id = app.workspace.active_document_id();
    assert!(!app.workspace.document(id).unwrap().is_dirty);
    std::fs::remove_file(&file.0).unwrap();

    let quit = app.update(Message::Quit);
    run_task(&mut app, quit);

    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "only editor copy remains");
    assert_eq!(document.disk_state, DiskState::Missing);
    assert!(document.is_dirty);
    assert!(!app.lifecycle.is_exiting());
    assert!(app.session.read_failed());
    assert!(app.close_prompt.document().is_none());
    assert!(!app.close_operation_pending());
    assert_eq!(app.files.close_goal, CloseGoal::KeepOpen);
    assert!(
        app.file_status
            .as_deref()
            .unwrap()
            .contains("unreadable previous session will be preserved")
    );
}

#[test]
fn an_unreadable_session_still_refuses_quit_with_existing_unsaved_edits() {
    let mut app = app_with_unreadable_session();
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("unsaved text".into()),
    ));

    let _ = app.update(Message::Quit);

    assert_eq!(app.workspace.document(id).unwrap().text(), "unsaved text");
    assert!(app.workspace.document(id).unwrap().is_dirty);
    assert!(!app.lifecycle.is_exiting());
    assert!(app.close_prompt.document().is_none());
    assert!(!app.close_operation_pending());
    assert!(
        app.file_status
            .as_deref()
            .unwrap()
            .contains("unreadable previous session will be preserved")
    );
}
