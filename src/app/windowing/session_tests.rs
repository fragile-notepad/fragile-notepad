//! Recovery checks exercise window ownership without opening native surfaces.
use crate::app::App;
use crate::core::{DocumentLoadState, Session, SessionDocument, SessionWindow};
use crate::message::Message;
use crate::startup::StartupOptions;

fn recovery(text: &str) -> SessionDocument {
    SessionDocument {
        text: Some(text.into()),
        is_dirty: true,
        ..Default::default()
    }
}

fn restore(session: Session) -> App {
    let (mut app, _) = App::new_with_options(StartupOptions::default());
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(Some(session))));
    let _ = app.update(Message::StartupReady);
    app
}

#[test]
fn every_restored_window_loads_its_active_tab_and_retains_inactive_recovery() {
    let app = restore(Session {
        version: 2,
        documents: vec![
            recovery("first active"),
            recovery("first inactive"),
            recovery("second active"),
        ],
        windows: vec![
            SessionWindow {
                document_indices: vec![0, 1],
                bounds: Some([50.0, 80.0, 900.0, 700.0]),
                ..Default::default()
            },
            SessionWindow {
                document_indices: vec![2],
                maximized: true,
                ..Default::default()
            },
        ],
        focused_window: Some(1),
        ..Default::default()
    });
    let windows = app.workspace.window_ids();
    assert_eq!(windows.len(), 2);
    assert_eq!(app.workspace.current_window_id(), windows[1]);
    assert_eq!(
        app.workspace.active_document_in(windows[0]).unwrap().text(),
        "first active"
    );
    assert_eq!(
        app.workspace.active_document_in(windows[1]).unwrap().text(),
        "second active"
    );
    let inactive = app.workspace.window_tab_ids(windows[0])[1];
    assert!(matches!(
        app.workspace.document(inactive).unwrap().load_state,
        DocumentLoadState::Deferred { .. }
    ));
    let saved = app.snapshot_session();
    assert!(saved.validate().is_ok());
    assert_eq!(saved.version, 2);
    assert_eq!(saved.focused_window, Some(1));
    assert_eq!(saved.windows[0].bounds, Some([50.0, 80.0, 900.0, 700.0]));
    assert!(saved.windows[1].maximized);
    let index = saved.windows[0].document_indices[1];
    assert_eq!(
        saved.documents[index].text.as_deref(),
        Some("first inactive")
    );
}

#[test]
fn transferring_recovered_tabs_persists_their_new_window_without_duplicating_text() {
    let mut app = restore(Session {
        documents: vec![recovery("move me"), recovery("keep me")],
        ..Default::default()
    });
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let (target, _) = app.create_editor(true, None);
    assert!(app.workspace.transfer_tabs(source, &[moving], target, 0));
    let saved = app.snapshot_session();
    assert!(saved.validate().is_ok());
    assert_eq!(saved.documents.len(), 2);
    assert_eq!(saved.windows.len(), 2);
    assert_eq!(
        saved
            .documents
            .iter()
            .filter(|document| document.text.as_deref() == Some("move me"))
            .count(),
        1
    );
    let target_index = saved.windows[1].document_indices[0];
    assert_eq!(
        saved.documents[target_index].text.as_deref(),
        Some("move me")
    );
}
