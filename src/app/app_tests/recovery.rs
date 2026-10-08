use super::test_support::*;
use crate::core::{EditorSettings, TextEncoding};
use iced::{event, keyboard};

fn close(app: &mut App) {
    let _ = app.update(Message::WindowCloseRequested(app.main_window_id.unwrap()));
    assert!(app.lifecycle.is_exiting());
}

fn fail_exit(app: &mut App) {
    let _ = app.update(Message::ShutdownPersisted(Err("disk unavailable".into())));
    assert!(!app.lifecycle.is_exiting());
}

#[test]
fn consumed_settings_timer_resumes_after_failed_exit_and_allows_later_saves() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::ZoomIn);
    assert!(app.settings_persistence.flush_scheduled());
    let wrap = app.settings.word_wrap;
    close(&mut app);
    let _ = app.update(Message::SettingsFlush);
    let _ = app.update(Message::ToggleWordWrap);
    fail_exit(&mut app);
    assert!(!app.settings_persistence.flush_scheduled());
    assert_eq!(
        app.settings.word_wrap, wrap,
        "shutdown rejects new user edits"
    );
    let _ = app.update(Message::ToggleWordWrap);
    assert!(app.settings_persistence.flush_scheduled());
}

#[test]
fn consumed_session_timer_resumes_after_failed_exit_and_allows_later_saves() {
    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions {
        files: vec![],
        restore_session: true,
    });
    let _ = app.update(Message::SessionLoaded(Ok(None)));
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::StartupReady);
    let _ = app.update(Message::NewFile);
    assert!(app.session.flush_scheduled());
    close(&mut app);
    let _ = app.update(Message::SessionFlush);
    fail_exit(&mut app);
    assert!(!app.session.flush_scheduled());
    assert!(
        app.session.is_saving(),
        "the consumed timer must start its save on resume"
    );
    let _ = app.update(Message::SessionPersisted(Ok(())));
    let _ = app.update(Message::NewFile);
    assert!(app.session.flush_scheduled());
}

#[test]
fn streamed_load_replays_chunks_before_completion_after_failed_exit() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::FilePicked(Ok("shutdown-stream.txt".into())));
    let document = app.workspace.active_document().unwrap();
    let id = document.id;
    let generation = document.load_generation().unwrap();
    let path = document.path.clone().unwrap();
    close(&mut app);
    for (text, bytes_read) in [("one\n", 4), ("two", 7)] {
        let _ = app.update(Message::FileLoadChunk(FileLoadChunk {
            document_id: id,
            generation,
            path: path.clone(),
            text: Arc::new(text.into()),
            reset: false,
            bytes_read,
            total_bytes: Some(7),
        }));
    }
    let _ = app.update(Message::FileLoadFinished(Ok(FileLoadFinished {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        document_id: id,
        generation,
        path,
        encoding: TextEncoding::Utf8,
        had_errors: false,
        fallback_contents: None,
        bytes_read: 7,
        total_bytes: Some(7),
    })));
    assert_eq!(app.workspace.document(id).unwrap().text(), "");
    fail_exit(&mut app);
    let document = app.workspace.document(id).unwrap();
    assert_eq!(document.text(), "one\ntwo");
    assert!(document.has_complete_text_index());
    assert!(app.files.load_handles().is_empty());
}

#[test]
fn failed_load_completion_is_replayed_after_failed_exit() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::FilePicked(Ok("missing-shutdown.txt".into())));
    let document = app.workspace.active_document().unwrap();
    let id = document.id;
    let generation = document.load_generation().unwrap();
    let path = document.path.clone().unwrap();
    close(&mut app);
    let _ = app.update(Message::FileLoadFinished(Err(FileLoadFailure {
        document_id: id,
        generation,
        path,
        error: crate::services::types::FileError::Io(std::io::ErrorKind::NotFound),
    })));
    fail_exit(&mut app);
    assert!(matches!(
        app.workspace.document(id).unwrap().load_state,
        crate::core::DocumentLoadState::Failed { .. }
    ));
    assert!(app.files.load_handles().is_empty());
}

fn reset_key(app: &App, status: event::Status) -> Message {
    Message::RuntimeEvent(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("0".into()),
            modified_key: keyboard::Key::Character("0".into()),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: if cfg!(target_os = "macos") {
                keyboard::Modifiers::LOGO
            } else {
                keyboard::Modifiers::CTRL
            },
            text: None,
            repeat: false,
        }),
        status,
        app.main_window_id.unwrap(),
    )
}

#[test]
fn early_zoom_reset_survives_settings_load_from_every_command_entry() {
    for input in 0..3 {
        let (mut app, _) = App::new();
        let message = match input {
            0 => Message::ZoomReset,
            1 => Message::Shortcut(crate::core::ShortcutCommand::ZoomReset),
            _ => reset_key(&app, event::Status::Ignored),
        };
        let _ = app.update(message);
        let loaded = EditorSettings {
            zoom: 2.0,
            ..EditorSettings::default()
        };
        let _ = app.update(Message::SettingsLoaded(Ok(Some(loaded))));
        assert_eq!(app.settings.zoom, 1.0, "entry {input}");
    }
}

#[test]
fn captured_shortcut_does_not_override_loaded_preferences() {
    let (mut app, _) = App::new();
    let _ = app.update(reset_key(&app, event::Status::Captured));
    let loaded = EditorSettings {
        zoom: 2.0,
        ..EditorSettings::default()
    };
    let _ = app.update(Message::SettingsLoaded(Ok(Some(loaded))));
    assert_eq!(app.settings.zoom, 2.0);
}
