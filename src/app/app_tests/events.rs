use super::test_support::*;
use crate::core::{EditorSettings, TextEncoding};

fn ready() -> App {
    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions {
        files: vec![],
        restore_session: true,
    });
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(None)));
    let _ = app.update(Message::StartupReady);
    // Complete any startup save caused by platform-specific new-file defaults.
    let _ = app.update(Message::SessionFlush);
    let _ = app.update(Message::SessionPersisted(Ok(())));
    app
}

#[test]
fn idle_ui_and_noop_edits_do_not_schedule_workers_or_session_saves() {
    let mut app = ready();
    let id = app.workspace.active_document_id();
    for message in [
        Message::None,
        Message::MenuClosed,
        Message::EditorAction(id, EditorAction::Backspace),
        Message::EncodingSelected(TextEncoding::Utf8),
    ] {
        assert_eq!(app.update(message).units(), 0);
        assert!(!app.session.is_dirty());
    }
    assert!(app.events.is_empty());
}

#[test]
fn text_mutation_notifies_search_outline_and_session_in_the_same_update() {
    let mut app = ready();
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::FindQueryChanged("needle".into()));
    assert!(!app.session.is_dirty());
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("needle".into()),
    ));
    assert_eq!(app.find.matches.len(), 1);
    assert!(app.active_outline_state().is_some());
    assert!(app.session.flush_scheduled());
    assert!(app.events.is_empty());
}

#[test]
fn closing_a_loading_document_aborts_its_worker_through_a_subscriber() {
    let mut app = ready();
    let _ = app.update(Message::FilePicked(Ok("closing.txt".into())));
    let id = app.workspace.active_document_id();
    assert!(app.files.load_handles().contains_key(&id));
    let _ = app.update(Message::TabClosed(id));
    assert!(app.workspace.document(id).is_none());
    assert!(!app.files.load_handles().contains_key(&id));
    assert!(!app.files.is_loading());
}

#[test]
fn subscriber_mutations_apply_settings_to_the_replacement_for_the_last_tab() {
    let mut app = ready();
    let _ = app.update(Message::ToggleVisibleSpaces);
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::TabClosed(id));
    let replacement = app.workspace.active_document().unwrap();
    assert_ne!(replacement.id, id);
    assert!(replacement.decorations.settings.show_spaces);
    assert_eq!(
        replacement.decorations.settings,
        app.settings.decoration_settings()
    );
    assert!(app.events.is_empty());
}

#[test]
fn shutdown_rejects_commands_before_mutations_and_event_delivery() {
    let mut app = ready();
    let settings = app.settings.clone();
    app.lifecycle.begin_shutdown();
    let _ = app.update(Message::ZoomIn);
    assert_eq!(app.settings, settings);
    assert!(!app.settings_persistence.has_early_edits());
    assert!(!app.session.is_dirty());
    assert!(app.events.is_empty());
}

#[test]
fn settings_changes_reach_documents_through_the_bus() {
    let mut app = ready();
    let mut settings = EditorSettings::default();
    settings.set_show_spaces(true);
    let _ = app.update(Message::SettingsLoaded(Ok(Some(settings))));
    assert!(
        app.workspace
            .active_document()
            .unwrap()
            .decorations
            .settings
            .show_spaces
    );
    assert!(app.events.is_empty());
}

#[test]
fn edits_before_session_initialization_schedule_a_save_when_startup_finishes() {
    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions {
        files: vec![],
        restore_session: true,
    });
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::EditorAction(
        id,
        EditorAction::InsertText("early edit".into()),
    ));
    assert!(app.session.is_dirty());
    assert!(!app.session.flush_scheduled());

    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(None)));
    let _ = app.update(Message::StartupReady);
    assert!(app.session.is_initialized());
    assert!(app.session.flush_scheduled());
    assert_eq!(app.workspace.document(id).unwrap().text(), "early edit");
}
