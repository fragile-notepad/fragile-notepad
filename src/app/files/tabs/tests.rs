use super::*;
use crate::core::DirtyCloseDecision;
use crate::editor::EditorAction;
use iced::window;

fn send(app: &mut App, document: DocumentId, action: TabContextAction) {
    let _ = app.update(Message::TabContextAction(document, action));
}

fn three_tabs() -> (App, DocumentId, DocumentId, DocumentId) {
    let (mut app, _) = App::new();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let third = app.workspace.create_untitled();
    (app, first, second, third)
}

fn restored_tabs(
    remaining: crate::core::session::SessionDocument,
) -> (App, crate::core::EditorWindowId, DocumentId, DocumentId) {
    use crate::core::session::{Session, SessionDocument};

    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions::default());
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(Some(Session {
        documents: vec![
            remaining,
            SessionDocument {
                text: Some("moved tab".into()),
                ..Default::default()
            },
        ],
        active_index: 1,
        ..Default::default()
    }))));
    let _ = app.update(Message::StartupReady);
    let source = app.workspace.current_window_id();
    let ids = app.workspace.window_tab_ids(source);
    let (remaining, moved) = (ids[0], ids[1]);
    assert!(matches!(
        app.workspace.document(remaining).unwrap().load_state,
        crate::core::DocumentLoadState::Deferred { .. }
    ));
    (app, source, remaining, moved)
}

#[test]
fn close_others_keeps_the_clicked_inactive_tab_in_its_own_window() {
    let (mut app, first, clicked, third) = three_tabs();
    let source = app.workspace.current_window_id();
    app.workspace.select(third);
    let _ = app.update(Message::NewWindow);
    let other = app.workspace.active_document_id();

    send(&mut app, clicked, TabContextAction::CloseOthers);

    assert_eq!(app.workspace.window_tab_ids(source), &[clicked]);
    assert!(app.workspace.document(first).is_none());
    assert!(app.workspace.document(third).is_none());
    assert!(app.workspace.document(other).is_some());
}

#[test]
fn closing_an_inactive_tab_keeps_the_active_tab_and_other_selected_tabs() {
    let (mut app, clicked, second, active) = three_tabs();
    let owner = app.workspace.current_window_id();
    app.workspace.select_tabs(owner, &[second, active], active);
    send(&mut app, clicked, TabContextAction::Close);
    let window = app.workspace.window(owner).unwrap();
    assert_eq!(window.tabs, vec![second, active]);
    assert_eq!(window.active_document_id, Some(active));
    assert_eq!(window.selected_tabs, vec![second, active]);
}

#[test]
fn close_all_is_limited_to_the_clicked_tabs_owner() {
    let (mut app, _, clicked, _) = three_tabs();
    let source = app.workspace.current_window_id();
    let _ = app.update(Message::NewWindow);
    let target = app.workspace.current_window_id();
    let other = app.workspace.active_document_id();
    send(&mut app, clicked, TabContextAction::CloseAll);
    assert!(!app.editor_windows.contains_key(&source));
    assert_eq!(app.workspace.window_tab_ids(target), &[other]);
}

#[test]
fn close_sides_are_relative_to_the_clicked_tab_without_selecting_it() {
    for (action, closes_first) in [
        (TabContextAction::CloseToLeft, true),
        (TabContextAction::CloseToRight, false),
    ] {
        let (mut app, first, clicked, third) = three_tabs();
        let active = if closes_first { third } else { first };
        app.workspace.select(active);
        send(&mut app, clicked, action);
        assert_eq!(app.workspace.active_document_id(), active);
        assert!(app.workspace.document(clicked).is_some());
        assert_eq!(app.workspace.document(first).is_none(), closes_first);
        assert_eq!(app.workspace.document(third).is_none(), !closes_first);
    }
}

#[test]
fn pinning_an_inactive_tab_preserves_the_active_tab_and_selection() {
    let (mut app, clicked, second, active) = three_tabs();
    let owner = app.workspace.current_window_id();
    app.workspace.select_tabs(owner, &[second, active], active);

    send(&mut app, clicked, TabContextAction::TogglePin);

    let window = app.workspace.window(owner).unwrap();
    assert_eq!(window.active_document_id, Some(active));
    assert_eq!(window.selected_tabs, vec![second, active]);
    assert!(app.workspace.document(clicked).unwrap().is_pinned);
    assert_eq!(window.tabs.first(), Some(&clicked));
    send(&mut app, clicked, TabContextAction::TogglePin);
    assert!(!app.workspace.document(clicked).unwrap().is_pinned);
}

#[test]
fn close_unpinned_preserves_pinned_tabs_in_the_clicked_window() {
    let (mut app, pinned, clicked, other) = three_tabs();
    app.workspace.toggle_pin(pinned);
    send(&mut app, clicked, TabContextAction::CloseUnpinned);
    assert_eq!(app.workspace.document_ids(), vec![pinned]);
    assert!(app.workspace.document(clicked).is_none());
    assert!(app.workspace.document(other).is_none());
}

#[test]
fn close_unmodified_includes_clean_pinned_tabs_and_keeps_dirty_tabs() {
    let (mut app, pinned, clicked, dirty) = three_tabs();
    app.workspace.toggle_pin(pinned);
    app.workspace.document_mut(dirty).unwrap().mark_dirty();
    send(&mut app, clicked, TabContextAction::CloseUnmodified);
    assert_eq!(app.workspace.document_ids(), vec![dirty]);
    assert!(!app.close_operation_pending());
}

#[test]
fn bulk_close_captures_ids_and_rejects_competing_context_commands() {
    let (mut app, first, clicked, third) = three_tabs();
    let source = app.workspace.current_window_id();
    app.workspace.document_mut(first).unwrap().mark_dirty();
    send(&mut app, clicked, TabContextAction::CloseOthers);
    assert_eq!(app.close_prompt.document(), Some(first));
    assert_eq!(
        app.files
            .pending_close_documents()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![third]
    );
    let _ = app.update(Message::NewWindow);
    let other = app.workspace.active_document_id();
    send(&mut app, other, TabContextAction::CloseAll);
    assert_eq!(app.files.close_goal(), CloseGoal::KeepOpen);
    assert_eq!(
        app.files
            .pending_close_documents()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![third]
    );
    app.switch_editor(source);
    app.workspace.select(clicked);
    let _ = app.update(Message::DirtyCloseResolved(
        first,
        DirtyCloseDecision::Discard,
    ));
    assert!(app.workspace.document(first).is_none());
    assert!(app.workspace.document(third).is_none());
    assert!(app.workspace.document(clicked).is_some());
    assert!(app.workspace.document(other).is_some());
}

#[test]
fn closed_target_commands_do_not_affect_live_tabs_or_create_windows() {
    let (mut app, closed, second, third) = three_tabs();
    send(&mut app, closed, TabContextAction::Close);
    let windows = app.workspace.window_ids();
    for action in [
        TabContextAction::CloseOthers,
        TabContextAction::CloseAll,
        TabContextAction::CloseToLeft,
        TabContextAction::CloseToRight,
        TabContextAction::TogglePin,
        TabContextAction::CopyPath,
        TabContextAction::MoveToNewWindow,
    ] {
        send(&mut app, closed, action);
        assert_eq!(app.workspace.document_ids(), vec![second, third]);
        assert_eq!(app.workspace.window_ids(), windows);
    }
}

#[test]
fn copy_path_uses_the_clicked_file_without_selecting_it() {
    use futures::StreamExt;
    use iced::advanced::clipboard::Content;

    let (mut app, clicked, second, active) = three_tabs();
    let owner = app.workspace.current_window_id();
    let path = std::env::temp_dir().join("context-menu-copy-path.txt");
    app.workspace.document_mut(clicked).unwrap().set_path(&path);
    app.workspace.select_tabs(owner, &[second, active], active);
    let task = app.update_tab_context_action(clicked, TabContextAction::CopyPath);
    let mut actions = iced_runtime::task::into_stream(task).expect("clipboard task");
    let action = futures::executor::block_on(actions.next()).expect("clipboard write");
    assert!(matches!(
        action,
        iced_runtime::Action::Clipboard(iced_runtime::clipboard::Action::Write {
            content: Content::Text(text), ..
        }) if text == path.to_string_lossy()
    ));
    let window = app.workspace.window(owner).unwrap();
    assert_eq!(window.active_document_id, Some(active));
    assert_eq!(window.selected_tabs, vec![second, active]);
}

#[test]
fn moving_to_a_new_window_moves_only_the_clicked_tab_and_preserves_history() {
    let (mut app, clicked, second, active) = three_tabs();
    let source = app.workspace.current_window_id();
    let _ = app.update(Message::EditorAction(
        clicked,
        EditorAction::InsertText("edited".into()),
    ));
    app.workspace.select_tabs(source, &[second, active], active);
    let revision = app.workspace.document(clicked).unwrap().revision();

    send(&mut app, clicked, TabContextAction::MoveToNewWindow);

    let target = app.workspace.current_window_id();
    assert_ne!(source, target);
    assert_eq!(app.workspace.window_tab_ids(source), &[second, active]);
    assert_eq!(
        app.workspace.window(source).unwrap().selected_tabs,
        vec![second, active]
    );
    assert_eq!(app.workspace.window_tab_ids(target), &[clicked]);
    let document = app.workspace.document(clicked).unwrap();
    assert_eq!(document.text(), "edited");
    assert_eq!(document.revision(), revision);
    assert!(document.is_dirty && document.can_undo());
}

#[test]
fn moving_active_tab_to_new_window_restores_source_recovery_text() {
    let (mut app, source, remaining, moved) =
        restored_tabs(crate::core::session::SessionDocument {
            path: Some("source-recovery.txt".into()),
            text: Some("recovered source text".into()),
            is_dirty: true,
            ..Default::default()
        });

    send(&mut app, moved, TabContextAction::MoveToNewWindow);

    let target = app.workspace.current_window_id();
    assert_ne!(source, target);
    assert_eq!(app.workspace.window_tab_ids(source), &[remaining]);
    let document = app.workspace.active_document_in(source).unwrap();
    assert_eq!(document.id, remaining);
    assert_eq!(document.text(), "recovered source text");
    assert!(document.has_complete_text_index());
    assert!(document.is_dirty);
    assert_eq!(app.workspace.active_document_id(), moved);
    assert_eq!(app.workspace.owner_of(moved), Some(target));
    assert_eq!(app.workspace.document(moved).unwrap().text(), "moved tab");
}

#[test]
fn moving_active_tab_to_existing_window_loads_source_file() {
    use crate::app::tests::test_support::{TestFile, run_task};

    let file = TestFile::new(b"source from disk");
    let (mut app, source, remaining, moved) =
        restored_tabs(crate::core::session::SessionDocument {
            path: Some(file.0.clone()),
            ..Default::default()
        });
    let _ = app.update(Message::NewWindow);
    let target = app.workspace.current_window_id();
    let native = app.editor_windows[&target].native;

    let task = app.update(Message::TabContextAction(
        moved,
        TabContextAction::MoveToWindow(native),
    ));
    assert_eq!(app.workspace.current_window_id(), target);
    assert_eq!(app.workspace.active_document_id(), moved);
    run_task(&mut app, task);

    assert_eq!(app.workspace.window_tab_ids(source), &[remaining]);
    let document = app.workspace.active_document_in(source).unwrap();
    assert_eq!(document.id, remaining);
    assert_eq!(document.text(), "source from disk");
    assert!(document.has_complete_text_index());
    assert_eq!(app.workspace.active_document_in(target).unwrap().id, moved);
    assert_eq!(app.workspace.owner_of(moved), Some(target));
    assert_eq!(app.workspace.document(moved).unwrap().text(), "moved tab");
}

#[test]
fn moving_a_sole_tab_to_a_new_window_preserves_the_source_editor() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let clicked = app.workspace.active_document_id();
    let native = app.editor_windows[&source].native;

    send(&mut app, clicked, TabContextAction::MoveToNewWindow);

    assert_eq!(app.workspace.window_ids(), vec![source]);
    assert_eq!(app.workspace.window_tab_ids(source), &[clicked]);
    assert_eq!(app.workspace.owner_of(clicked), Some(source));
    assert_eq!(app.editor_windows[&source].native, native);
    assert_eq!(app.workspace.active_document_id(), clicked);
}

#[test]
fn moving_to_an_existing_window_closes_an_empty_source_and_rejects_stale_windows() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let clicked = app.workspace.active_document_id();
    let _ = app.update(Message::NewWindow);
    let target = app.workspace.current_window_id();
    let other = app.workspace.active_document_id();
    let native = app.editor_windows[&target].native;
    send(
        &mut app,
        clicked,
        TabContextAction::MoveToWindow(window::Id::unique()),
    );
    assert_eq!(app.workspace.owner_of(clicked), Some(source));

    send(&mut app, clicked, TabContextAction::MoveToWindow(native));

    assert_eq!(app.workspace.owner_of(clicked), Some(target));
    assert_eq!(app.workspace.window_tab_ids(target), &[other, clicked]);
    assert!(!app.editor_windows.contains_key(&source));
    assert_eq!(app.workspace.window_ids(), vec![target]);
}
