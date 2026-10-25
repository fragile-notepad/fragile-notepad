//! State integration checks do not execute native window or file-system tasks.
use crate::app::App;
use crate::core::{DirtyCloseDecision, DocumentId, EditorWindowId};
use crate::editor::{EditorAction, EditorPosition, EditorSelection};
use crate::message::{BulkScope, Message, SearchScope};
use crate::ui::tabs::StripGeometry;
use iced::window::tab_drag::{CancelReason, Capability, Event as DragEvent, SessionId};
use iced::{Point, Rectangle, keyboard, window};
use std::time::{Duration, Instant};

mod tab_drag;

fn native(app: &App, editor: EditorWindowId) -> window::Id {
    app.editor_windows[&editor].native
}

fn send(app: &mut App, editor: EditorWindowId, message: Message) {
    let _ = app.update(Message::ForWindow(native(app, editor), Box::new(message)));
}

fn another_window(app: &mut App) -> EditorWindowId {
    let _ = app.update(Message::NewWindow);
    app.workspace.current_window_id()
}

fn add_tab(app: &mut App, editor: EditorWindowId) -> DocumentId {
    send(app, editor, Message::NewFile);
    app.workspace.active_document_in(editor).unwrap().id
}

fn edit(app: &mut App, document: DocumentId, text: &str) {
    let _ = app.update(Message::EditorAction(
        document,
        EditorAction::InsertText(text.into()),
    ));
}

fn shortcut_key(window: window::Id, modifiers: keyboard::Modifiers) -> Message {
    Message::RuntimeEvent(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("n".into()),
            modified_key: keyboard::Key::Character("n".into()),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }),
        iced::event::Status::Ignored,
        window,
    )
}

fn resolve(
    app: &mut App,
    editor: EditorWindowId,
    document: DocumentId,
    decision: DirtyCloseDecision,
) {
    let start = Instant::now();
    for offset in [0, 200] {
        send(
            app,
            editor,
            Message::ChromeAnimationFrame(start + Duration::from_millis(offset)),
        );
    }
    send(app, editor, Message::DirtyCloseResolved(document, decision));
    for offset in [200, 400] {
        send(
            app,
            editor,
            Message::ChromeAnimationFrame(start + Duration::from_millis(offset)),
        );
    }
    send(app, editor, Message::DirtyCloseFadeFinished(document));
}

fn geometry(app: &App, editor: EditorWindowId) -> StripGeometry {
    StripGeometry {
        bounds: Rectangle {
            x: 0.0,
            y: 50.0,
            width: 700.0,
            height: 27.0,
        },
        tabs: app
            .workspace
            .window_tab_ids(editor)
            .iter()
            .enumerate()
            .map(|(index, id)| {
                (
                    *id,
                    Rectangle {
                        x: index as f32 * 120.0,
                        y: 50.0,
                        width: 120.0,
                        height: 27.0,
                    },
                )
            })
            .collect(),
    }
}

fn start_drag(app: &mut App, source: EditorWindowId, document: DocumentId) -> SessionId {
    start_drag_with_capability(app, source, document, Capability::Desktop)
}

fn start_drag_with_capability(
    app: &mut App,
    source: EditorWindowId,
    document: DocumentId,
    capability: Capability,
) -> SessionId {
    let session = SessionId(app.next_drag_session);
    let shape = geometry(app, source);
    let bounds = shape.tabs.iter().find(|(id, _)| *id == document).unwrap().1;
    send(app, source, Message::TabStripGeometry(shape));
    send(
        app,
        source,
        Message::TabPressed(
            document,
            Point::new(bounds.x + 20.0, 60.0),
            keyboard::Modifiers::empty(),
        ),
    );
    let _ = app.update(Message::TabDragEvent(DragEvent::Started {
        session,
        window: native(app, source),
        capability,
    }));
    assert!(app.tab_drag.is_some());
    session
}

fn drag_out(app: &mut App, source: EditorWindowId, session: SessionId) {
    let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
        session,
        window: native(app, source),
        position: Point::new(300.0, 180.0),
        desktop: Some(Point::new(900.0, 500.0)),
        hovered: None,
    }));
}

fn drag_into(app: &mut App, source: EditorWindowId, target: EditorWindowId, session: SessionId) {
    let shape = geometry(app, target);
    send(app, target, Message::TabStripGeometry(shape));
    let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
        session,
        window: native(app, source),
        position: Point::new(450.0, 60.0),
        desktop: Some(Point::new(1250.0, 160.0)),
        hovered: Some((native(app, target), Point::new(450.0, 60.0))),
    }));
}

#[test]
fn window_origins_keep_tabs_find_and_panels_independent() {
    let (mut app, _) = App::new();
    let first = app.workspace.current_window_id();
    let first_document = app.workspace.active_document_id();
    let second = another_window(&mut app);
    let second_document = app.workspace.active_document_id();
    assert_ne!(first_document, second_document);
    send(
        &mut app,
        first,
        Message::FindQueryChanged("first query".into()),
    );
    send(&mut app, first, Message::ToggleFind);
    send(
        &mut app,
        second,
        Message::FindQueryChanged("second query".into()),
    );
    assert_eq!(app.window_ui(first).unwrap().find.query, "first query");
    assert_eq!(app.window_ui(second).unwrap().find.query, "second query");
    assert!(*app.window_ui(first).unwrap().is_find_visible);
    assert!(!*app.window_ui(second).unwrap().is_find_visible);
    let added = add_tab(&mut app, first);
    assert_eq!(app.workspace.owner_of(added), Some(first));
    assert_eq!(app.workspace.window_tab_ids(second), &[second_document]);
    assert_eq!(app.main_window_id, Some(native(&app, first)));
}

#[test]
fn moving_selected_tabs_preserves_live_edit_state_and_undo_history() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let remaining = add_tab(&mut app, source);
    let target = another_window(&mut app);
    edit(&mut app, moving, "unsaved text");
    let selection = EditorSelection::new(EditorPosition::new(0, 1), EditorPosition::new(0, 5));
    app.workspace.document_mut(moving).unwrap().selection = selection;
    let revision = app.workspace.document(moving).unwrap().revision();
    app.workspace.select_tabs(source, &[moving], moving);
    let target_native = native(&app, target);
    send(&mut app, source, Message::MoveTabsToWindow(target_native));
    let document = app.workspace.document(moving).unwrap();
    assert_eq!(app.workspace.owner_of(moving), Some(target));
    assert_eq!(app.workspace.window_tab_ids(source), &[remaining]);
    assert_eq!(document.text(), "unsaved text");
    assert_eq!(document.selection, selection);
    assert_eq!(document.revision(), revision);
    assert!(document.is_dirty && document.can_undo());
    let _ = app.update(Message::EditorAction(moving, EditorAction::Undo));
    assert_eq!(app.workspace.document(moving).unwrap().text(), "");
    assert!(app.workspace.document(moving).unwrap().can_redo());
}

#[test]
fn bulk_scope_targets_and_save_queue_are_captured_before_focus_changes() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let excluded = add_tab(&mut app, source);
    let target = another_window(&mut app);
    let other = app.workspace.active_document_id();
    for id in [first, second, excluded, other] {
        edit(&mut app, id, "dirty");
    }
    app.switch_editor(source);
    app.workspace.select_tabs(source, &[first, second], first);
    assert_eq!(
        app.scoped_document_ids(BulkScope::SelectedTabs),
        vec![first, second]
    );
    assert_eq!(
        app.scoped_document_ids(BulkScope::ThisWindow),
        vec![first, second, excluded]
    );
    assert_eq!(app.scoped_document_ids(BulkScope::AllWindows).len(), 4);
    send(
        &mut app,
        source,
        Message::SaveScoped(BulkScope::SelectedTabs),
    );
    assert_eq!(app.files.pending_save().unwrap().document_id, first);
    app.workspace.select_in_window(source, excluded);
    send(
        &mut app,
        target,
        Message::FindQueryChanged("focus elsewhere".into()),
    );
    assert_eq!(
        app.files
            .pending_save_all()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(app.files.pending_save().unwrap().document_id, first);
}

#[test]
fn selected_close_queue_is_not_redirected_by_another_window() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let excluded = add_tab(&mut app, source);
    let target = another_window(&mut app);
    edit(&mut app, first, "first");
    edit(&mut app, second, "second");
    app.workspace.select_tabs(source, &[first, second], first);
    send(
        &mut app,
        source,
        Message::CloseScoped(BulkScope::SelectedTabs),
    );
    send(&mut app, target, Message::FindQueryChanged("other".into()));
    assert_eq!(
        app.window_ui(source).unwrap().close_prompt.document(),
        Some(first)
    );
    assert_eq!(
        app.files
            .pending_close_documents()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![second]
    );
    resolve(&mut app, source, first, DirtyCloseDecision::Cancel);
    assert!(app.files.pending_close_documents().is_empty());
    assert_eq!(
        app.workspace.window_tab_ids(source),
        &[first, second, excluded]
    );
    assert!(app.editor_windows.contains_key(&target));
}

#[test]
fn advanced_search_uses_each_explicit_scope() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let excluded = add_tab(&mut app, source);
    let target = another_window(&mut app);
    let other = app.workspace.active_document_id();
    for id in [first, second, excluded, other] {
        edit(&mut app, id, "needle");
    }
    app.workspace.select_tabs(source, &[first, second], first);
    app.search_dialog.set_query("needle");
    for (scope, expected) in [
        (SearchScope::CurrentDocument, vec![first]),
        (SearchScope::SelectedTabs, vec![first, second]),
        (SearchScope::ThisWindow, vec![first, second, excluded]),
        (
            SearchScope::AllWindows,
            vec![first, second, excluded, other],
        ),
    ] {
        app.search_dialog.set_scope(scope);
        send(&mut app, source, Message::AdvancedCountRun);
        let summary = app
            .search_dialog
            .count_summary
            .as_ref()
            .expect("search completed synchronously");
        assert_eq!(summary.total_matches, expected.len());
        assert_eq!(
            summary
                .per_document
                .iter()
                .map(|document| document.document_id)
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert!(app.editor_windows.contains_key(&target));
}

#[test]
fn pending_search_retains_target_ids_after_transfer_and_selection_changes() {
    use crate::core::{DocumentIndexState, DocumentLoadGeneration};
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let excluded = add_tab(&mut app, source);
    let target = another_window(&mut app);
    for id in [first, second, excluded] {
        edit(&mut app, id, "needle");
    }
    app.workspace.select_tabs(source, &[first, second], first);
    app.workspace.document_mut(second).unwrap().index_state = DocumentIndexState::Pending {
        generation: DocumentLoadGeneration::next(),
    };
    app.search_dialog.set_query("needle");
    app.search_dialog.set_scope(SearchScope::SelectedTabs);
    send(&mut app, source, Message::AdvancedCountRun);
    assert!(app.pending_search.is_some());
    assert!(app.workspace.transfer_tabs(source, &[second], target, 0));
    app.workspace.select_in_window(source, excluded);
    send(&mut app, target, Message::None);
    assert!(app.pending_search.is_some());
    app.workspace.document_mut(second).unwrap().index_state = DocumentIndexState::Complete;
    app.events
        .publish(crate::app::events::Event::SearchRequested);
    send(&mut app, target, Message::None);
    assert!(app.pending_search.is_none());
    let summary = app
        .search_dialog
        .count_summary
        .as_ref()
        .expect("captured search completed");
    assert_eq!(summary.total_matches, 2);
    assert_eq!(
        summary
            .per_document
            .iter()
            .map(|document| document.document_id)
            .collect::<Vec<_>>(),
        vec![first, second]
    );
}

#[test]
fn nonfinal_window_close_can_cancel_then_save_before_removing_its_editor() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let document = app.workspace.active_document_id();
    let target = another_window(&mut app);
    edit(&mut app, document, "keep these changes");
    send(&mut app, source, Message::CloseWindow);
    assert_eq!(app.close_prompt.document(), Some(document));
    resolve(&mut app, source, document, DirtyCloseDecision::Cancel);
    assert!(app.editor_windows.contains_key(&source));
    send(&mut app, source, Message::CloseWindow);
    resolve(&mut app, source, document, DirtyCloseDecision::Save);
    let request = app
        .files
        .pending_save()
        .cloned()
        .expect("save snapshot captured");
    assert_eq!(request.document_id, document);
    assert!(app.editor_windows.contains_key(&source));
    let _ = app.update(Message::FileSaved(
        request,
        Ok("saved-window-test.txt".into()),
    ));
    assert!(!app.editor_windows.contains_key(&source));
    assert!(app.editor_windows.contains_key(&target));
    assert!(app.workspace.document(document).is_none());
    assert!(!app.lifecycle.is_exiting());
}

#[test]
fn keeping_daemon_alive_allows_fresh_ui_after_the_last_window_closes() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    app.settings.exit_on_last_window_close = false;
    send(
        &mut app,
        source,
        Message::FindQueryChanged("old query".into()),
    );
    send(&mut app, source, Message::ToggleFind);
    send(&mut app, source, Message::CloseWindow);
    assert!(app.editor_windows.is_empty());
    assert!(app.workspace.documents().is_empty());
    assert!(app.main_window_id.is_none());
    assert!(!app.lifecycle.is_exiting());
    let fresh = another_window(&mut app);
    assert_ne!(fresh, source);
    assert_eq!(app.editor_windows.len(), 1);
    assert!(app.workspace.active_document_in(fresh).is_some());
    assert!(app.find.query.is_empty());
    assert!(!app.is_find_visible);
}

#[test]
fn new_file_shortcut_opens_an_editor_when_only_settings_remains() {
    use super::managed::ManagedWindow;

    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    app.settings.exit_on_last_window_close = false;
    let _ = app.toggle_settings_window();
    let settings_native = app.settings_window.unwrap().id();
    send(&mut app, source, Message::CloseWindow);
    assert!(app.editor_windows.is_empty());

    let _ = app.update(shortcut_key(settings_native, keyboard::Modifiers::COMMAND));

    assert_eq!(app.editor_windows.len(), 1);
    let editor = app.workspace.current_window_id();
    assert_ne!(editor, source);
    assert_eq!(app.workspace.window_tab_ids(editor).len(), 1);
    assert!(app.workspace.active_document().is_some());
    assert_eq!(app.last_active_editor, Some(editor));
    assert_eq!(app.settings_window.unwrap().id(), settings_native);
    assert!(app.snapshot_session().validate().is_ok());
}

#[test]
fn tab_click_auto_saves_the_previous_named_document() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let previous = app.workspace.active_document_id();
    let clicked = add_tab(&mut app, source);
    app.workspace.select(previous);
    app.settings.auto_save = true;
    app.workspace
        .document_mut(previous)
        .unwrap()
        .set_path("tab-click-auto-save.txt");
    edit(&mut app, previous, "save before switching");

    let session = start_drag(&mut app, source, clicked);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));

    assert_eq!(app.workspace.active_document_id(), clicked);
    let request = app.files.pending_save().expect("auto-save request");
    assert_eq!(request.document_id, previous);
}

#[test]
fn deselecting_the_active_tab_auto_saves_it() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let remaining = app.workspace.active_document_id();
    let previous = add_tab(&mut app, source);
    app.settings.auto_save = true;
    app.workspace
        .document_mut(previous)
        .unwrap()
        .set_path("deselected-auto-save.txt");
    edit(&mut app, previous, "save the deselected tab");
    app.workspace
        .select_tabs(source, &[remaining, previous], previous);

    send(
        &mut app,
        source,
        Message::TabPressed(
            previous,
            Point::new(140.0, 60.0),
            keyboard::Modifiers::COMMAND,
        ),
    );

    assert_eq!(app.workspace.active_document_id(), remaining);
    assert!(app.tab_drag.is_none());
    assert_eq!(app.files.pending_save().unwrap().document_id, previous);
}

#[test]
fn clicking_the_active_tab_does_not_auto_save_it() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let document = app.workspace.active_document_id();
    app.settings.auto_save = true;
    app.workspace
        .document_mut(document)
        .unwrap()
        .set_path("same-tab-auto-save.txt");
    edit(&mut app, document, "keep editing");

    let session = start_drag(&mut app, source, document);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));

    assert!(app.files.pending_save().is_none());
    assert!(app.files.pending_auto_saves().is_empty());
    assert!(app.workspace.document(document).unwrap().is_dirty);
}

#[test]
fn native_drag_moves_a_group_and_release_commits_without_closing_documents() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let remaining = add_tab(&mut app, source);
    let target = another_window(&mut app);
    app.workspace.select_tabs(source, &[first, second], first);
    let session = start_drag(&mut app, source, first);
    drag_into(&mut app, source, target, session);
    assert_eq!(app.workspace.owner_of(first), Some(target));
    assert_eq!(app.workspace.owner_of(second), Some(target));
    assert_eq!(app.workspace.window_tab_ids(source), &[remaining]);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert!(app.tab_drag.is_none());
    assert_eq!(
        app.workspace.window(target).unwrap().selected_tabs,
        vec![first, second]
    );
    assert_eq!(app.workspace.documents().len(), 4);
    assert_eq!(app.workspace.current_window_id(), target);
}

#[test]
fn escape_rolls_back_exact_layout_and_selection_while_preserving_edits() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let _remaining = add_tab(&mut app, source);
    let target = another_window(&mut app);
    app.workspace.select_tabs(source, &[first, second], second);
    let before = app
        .workspace
        .window_ids()
        .into_iter()
        .map(|id| (id, app.workspace.window(id).unwrap().clone()))
        .collect::<Vec<_>>();
    let session = start_drag(&mut app, source, first);
    drag_into(&mut app, source, target, session);
    edit(&mut app, first, "edited while attached");
    let _ = app.update(Message::TabDragEvent(DragEvent::Cancelled {
        session,
        reason: CancelReason::Requested,
    }));
    assert!(app.tab_drag.is_none());
    for (id, expected) in before {
        assert_eq!(app.workspace.window(id), Some(&expected));
    }
    let document = app.workspace.document(first).unwrap();
    assert_eq!(document.text(), "edited while attached");
    assert!(document.is_dirty && document.can_undo());
    assert_eq!(app.workspace.current_window_id(), source);
}

#[test]
fn release_before_detached_window_opens_ignores_its_stale_follow_callback() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let _remaining = add_tab(&mut app, source);
    let session = start_drag(&mut app, source, moving);
    let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
        session,
        window: native(&app, source),
        position: Point::new(300.0, 160.0),
        desktop: Some(Point::new(900.0, 500.0)),
        hovered: None,
    }));
    let detached = app.workspace.owner_of(moving).unwrap();
    assert_ne!(detached, source);
    let detached_native = native(&app, detached);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    let stale = app.update(Message::TabDetachedReady(session, detached_native));
    assert!(app.tab_drag.is_none());
    assert_eq!(app.workspace.owner_of(moving), Some(detached));
    assert_eq!(stale.units(), 0);
}

#[test]
fn detach_attach_detach_remains_one_cancellable_gesture() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let remaining = add_tab(&mut app, source);
    let target = another_window(&mut app);
    let target_tabs = app.workspace.window_tab_ids(target).to_vec();
    let session = start_drag(&mut app, source, moving);
    for pass in 0..2 {
        let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
            session,
            window: native(&app, app.workspace.owner_of(moving).unwrap()),
            position: Point::new(300.0, 180.0),
            desktop: Some(Point::new(900.0 + pass as f32 * 50.0, 500.0)),
            hovered: None,
        }));
        let detached = app.workspace.owner_of(moving).unwrap();
        assert_ne!(detached, source);
        assert_ne!(detached, target);
        assert!(app.tab_drag.is_some());
        if pass == 0 {
            drag_into(&mut app, source, target, session);
            assert_eq!(app.workspace.owner_of(moving), Some(target));
            assert!(app.workspace.window_tab_ids(detached).is_empty());
        }
    }
    let _ = app.update(Message::TabDragEvent(DragEvent::Cancelled {
        session,
        reason: CancelReason::CaptureLost,
    }));
    assert!(app.tab_drag.is_none());
    assert_eq!(app.editor_windows.len(), 2);
    assert_eq!(app.workspace.window_ids().len(), 2);
    assert_eq!(app.workspace.window_tab_ids(source), &[moving, remaining]);
    assert_eq!(app.workspace.window_tab_ids(target), target_tabs);
}

#[test]
fn stale_native_events_cannot_end_a_newer_gesture() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let document = app.workspace.active_document_id();
    let first = start_drag(&mut app, source, document);
    let _ = app.update(Message::TabDragEvent(DragEvent::Cancelled {
        session: first,
        reason: CancelReason::CaptureLost,
    }));
    let newer = start_drag(&mut app, source, document);
    assert_ne!(first, newer);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released {
        session: first,
    }));
    let _ = app.update(Message::TabDragEvent(DragEvent::Cancelled {
        session: first,
        reason: CancelReason::NativeFailure,
    }));
    assert!(app.tab_drag.is_some());
    let _ = app.update(Message::TabDragEvent(DragEvent::Released {
        session: newer,
    }));
    assert!(app.tab_drag.is_none());
}

#[test]
fn detached_window_open_failure_rolls_back_and_preserves_source_buffers() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let remaining = add_tab(&mut app, source);
    edit(&mut app, moving, "keep on native failure");
    let before = app.workspace.window(source).unwrap().clone();
    let session = start_drag(&mut app, source, moving);
    drag_out(&mut app, source, session);
    let detached = app.workspace.owner_of(moving).unwrap();
    let failed_native = native(&app, detached);
    assert_ne!(source, detached);
    let _ = app.update(Message::WindowOpenFailed(
        failed_native,
        "simulated creation failure".into(),
    ));
    assert!(app.tab_drag.is_none());
    assert_eq!(app.editor_windows.len(), 1);
    assert_eq!(app.workspace.window(source), Some(&before));
    assert_eq!(app.workspace.window_tab_ids(source), &[moving, remaining]);
    let document = app.workspace.document(moving).unwrap();
    assert_eq!(document.text(), "keep on native failure");
    assert!(document.is_dirty && document.can_undo());
    assert!(
        app.file_status
            .as_deref()
            .is_some_and(|status| status.contains("simulated creation failure"))
    );
    assert_eq!(app.workspace.current_window_id(), source);
}

#[test]
fn committed_detach_creation_failure_returns_documents_to_a_surviving_window_once() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let remaining = add_tab(&mut app, source);
    edit(&mut app, moving, "committed but unopened");
    let session = start_drag(&mut app, source, moving);
    drag_out(&mut app, source, session);
    let detached = app.workspace.owner_of(moving).unwrap();
    let failed_native = native(&app, detached);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert!(app.tab_drag.is_none());
    for _ in 0..2 {
        let _ = app.update(Message::WindowOpenFailed(
            failed_native,
            "simulated delayed failure".into(),
        ));
    }
    assert_eq!(app.editor_windows.len(), 1);
    assert_eq!(app.workspace.window_ids(), vec![source]);
    assert_eq!(app.workspace.window_tab_ids(source), &[remaining, moving]);
    assert_eq!(app.workspace.all_document_ids().len(), 2);
    assert_eq!(
        app.workspace.document(moving).unwrap().text(),
        "committed but unopened"
    );
    assert!(app.workspace.document(moving).unwrap().can_undo());
}

#[test]
fn structural_command_cancels_drag_then_applies_to_its_originating_window() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let remaining = add_tab(&mut app, source);
    let target = another_window(&mut app);
    let target_original = app.workspace.window_tab_ids(target).to_vec();
    let session = start_drag(&mut app, source, moving);
    drag_into(&mut app, source, target, session);
    assert_eq!(app.workspace.owner_of(moving), Some(target));
    let added = add_tab(&mut app, target);
    assert!(app.tab_drag.is_none());
    assert_eq!(app.workspace.window_tab_ids(source), &[moving, remaining]);
    assert_eq!(
        app.workspace.window_tab_ids(target),
        [target_original.as_slice(), &[added]].concat()
    );
    assert_eq!(app.workspace.owner_of(added), Some(target));
    assert_eq!(app.workspace.current_window_id(), target);
}

#[test]
fn structural_shortcuts_cancel_drag_then_apply_to_the_originating_window() {
    use crate::core::ShortcutCommand;

    for shortcut in [ShortcutCommand::NewFile, ShortcutCommand::NewWindow] {
        for runtime_event in [false, true] {
            let (mut app, _) = App::new();
            let source = app.workspace.current_window_id();
            let moving = app.workspace.active_document_id();
            let remaining = add_tab(&mut app, source);
            let target = another_window(&mut app);
            let target_original = app.workspace.window_tab_ids(target).to_vec();
            let session = start_drag(&mut app, source, moving);
            drag_into(&mut app, source, target, session);
            let message = if runtime_event {
                let modifiers = keyboard::Modifiers::COMMAND
                    | if shortcut == ShortcutCommand::NewWindow {
                        keyboard::Modifiers::SHIFT
                    } else {
                        keyboard::Modifiers::empty()
                    };
                shortcut_key(native(&app, target), modifiers)
            } else {
                Message::Shortcut(shortcut)
            };

            send(&mut app, target, message);

            assert!(app.tab_drag.is_none());
            assert_eq!(app.workspace.window_tab_ids(source), &[moving, remaining]);
            if shortcut == ShortcutCommand::NewFile {
                assert_eq!(app.workspace.current_window_id(), target);
                let added = app.workspace.active_document_id();
                assert_eq!(
                    app.workspace.window_tab_ids(target),
                    [target_original.as_slice(), &[added]].concat()
                );
                assert_eq!(app.editor_windows.len(), 2);
            } else {
                let added_window = app.workspace.current_window_id();
                assert_ne!(added_window, source);
                assert_ne!(added_window, target);
                assert_eq!(app.workspace.window_tab_ids(added_window).len(), 1);
                assert_eq!(app.workspace.window_tab_ids(target), target_original);
                assert_eq!(app.editor_windows.len(), 3);
            }
            assert!(app.snapshot_session().validate().is_ok());
        }
    }
}

#[test]
fn new_file_shortcut_from_a_detached_window_cancels_drag_before_creating_a_tab() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let remaining = add_tab(&mut app, source);
    let session = start_drag(&mut app, source, moving);
    drag_out(&mut app, source, session);
    let detached = app.workspace.owner_of(moving).unwrap();
    assert_ne!(detached, source);
    let message = shortcut_key(native(&app, detached), keyboard::Modifiers::COMMAND);

    send(&mut app, detached, message);

    assert!(app.tab_drag.is_none());
    assert_eq!(app.editor_windows.len(), 1);
    assert_eq!(app.workspace.current_window_id(), source);
    let added = app.workspace.active_document_id();
    assert_eq!(
        app.workspace.window_tab_ids(source),
        &[moving, remaining, added]
    );
    assert!(app.snapshot_session().validate().is_ok());
}

#[test]
fn geometry_changes_recalculate_insertion_without_pointer_motion() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let third = add_tab(&mut app, source);
    let fourth = add_tab(&mut app, source);
    let session = start_drag(&mut app, source, moving);
    let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
        session,
        window: native(&app, source),
        position: Point::new(190.0, 60.0),
        desktop: Some(Point::new(190.0, 60.0)),
        hovered: Some((native(&app, source), Point::new(190.0, 60.0))),
    }));
    assert_eq!(
        app.workspace.window_tab_ids(source),
        &[second, moving, third, fourth]
    );
    let mut scrolled = geometry(&app, source);
    scrolled.bounds.width = 300.0;
    for (_, bounds) in &mut scrolled.tabs {
        bounds.x -= 200.0;
    }
    send(&mut app, source, Message::TabStripGeometry(scrolled));
    assert!(app.tab_drag.is_some());
    assert_eq!(
        app.workspace.window_tab_ids(source),
        &[second, third, fourth, moving]
    );
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert_eq!(
        app.workspace.window_tab_ids(source),
        &[second, third, fourth, moving]
    );
}

#[test]
fn pressing_a_second_selected_tab_preserves_the_group_and_activates_that_tab() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let _remaining = add_tab(&mut app, source);
    let target = another_window(&mut app);
    app.workspace.select_tabs(source, &[first, second], first);
    let session = start_drag(&mut app, source, second);
    let window = app.workspace.window(source).unwrap();
    assert_eq!(window.selected_tabs, vec![first, second]);
    assert_eq!(window.active_document_id, Some(second));
    drag_into(&mut app, source, target, session);
    let window = app.workspace.window(target).unwrap();
    assert_eq!(window.selected_tabs, vec![first, second]);
    assert_eq!(window.active_document_id, Some(second));
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert_eq!(app.workspace.active_document_in(target).unwrap().id, second);
}

#[test]
fn surface_only_drag_cannot_tear_off_but_can_transfer_to_an_existing_window() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let remaining = add_tab(&mut app, source);
    let target = another_window(&mut app);
    let session = start_drag_with_capability(&mut app, source, moving, Capability::SurfaceOnly);
    drag_out(&mut app, source, session);
    assert_eq!(app.editor_windows.len(), 2);
    assert_eq!(app.workspace.owner_of(moving), Some(source));
    assert!(app.tab_drag.is_some());
    drag_into(&mut app, source, target, session);
    assert_eq!(app.workspace.owner_of(moving), Some(target));
    assert_eq!(app.workspace.window_tab_ids(source), &[remaining]);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert_eq!(app.editor_windows.len(), 2);
    assert!(app.tab_drag.is_none());
}

#[test]
fn dragging_every_tab_reuses_the_source_native_window() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let source_native = native(&app, source);
    app.workspace.select_tabs(source, &[first, second], second);
    let session = start_drag(&mut app, source, second);
    drag_out(&mut app, source, session);
    assert_eq!(app.editor_windows.len(), 1);
    assert_eq!(app.workspace.window_ids(), vec![source]);
    assert_eq!(native(&app, source), source_native);
    assert_eq!(app.workspace.window_tab_ids(source), &[first, second]);
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert!(app.tab_drag.is_none());
    assert_eq!(app.editor_windows.len(), 1);
    assert_eq!(native(&app, source), source_native);
    assert_eq!(
        app.workspace.window(source).unwrap().selected_tabs,
        vec![first, second]
    );
}

#[test]
fn tabs_do_not_attach_to_a_strip_covered_by_a_modal_overlay() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    add_tab(&mut app, source);
    let target = another_window(&mut app);
    let target_document = app.workspace.active_document_id();
    send(&mut app, target, Message::AboutOpened);
    let session = start_drag(&mut app, source, moving);
    drag_out(&mut app, source, session);
    drag_into(&mut app, source, target, session);
    assert_ne!(app.workspace.owner_of(moving), Some(target));
    assert_eq!(app.workspace.window_tab_ids(target), &[target_document]);
    let _ = app.update(Message::TabDragEvent(DragEvent::Cancelled {
        session,
        reason: CancelReason::Requested,
    }));
    assert_eq!(app.workspace.owner_of(moving), Some(source));
}

#[test]
fn renderer_handoff_waits_for_the_drag_transaction_to_finish() {
    use crate::app::rendering::RenderingState;
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let session = start_drag(&mut app, source, moving);
    let _ = app.update(Message::BackendBoostRequested);
    assert!(app.pending_drag_gpu_boost);
    assert_eq!(app.rendering, RenderingState::Software);
    let task = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert!(!app.pending_drag_gpu_boost);
    use futures::StreamExt;
    let actions = futures::executor::block_on(async {
        iced_runtime::task::into_stream(task)
            .unwrap()
            .collect::<Vec<_>>()
            .await
    });
    assert!(actions.into_iter().any(|action| {
        let iced_runtime::Action::Output(mut message) = action else {
            return false;
        };
        while let Message::ForWindow(_, inner) = message {
            message = *inner;
        }
        matches!(message, Message::BackendBoostRequested)
    }));
}
