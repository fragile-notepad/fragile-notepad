//! Closing queues are process-wide even when their prompt belongs to an inactive editor.
use crate::app::{App, CloseGoal};
use crate::core::{DirtyCloseDecision, DocumentId, EditorWindowId};
use crate::message::{BulkScope, Message};
use std::time::{Duration, Instant};

fn send(app: &mut App, window: EditorWindowId, message: Message) {
    let native = app.editor_windows[&window].native;
    let _ = app.update(Message::ForWindow(native, Box::new(message)));
}

fn setup() -> (
    App,
    EditorWindowId,
    EditorWindowId,
    DocumentId,
    DocumentId,
    DocumentId,
) {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let first = app.workspace.active_document_id();
    let second = app.workspace.create_untitled();
    let _ = app.update(Message::NewWindow);
    let target = app.workspace.current_window_id();
    let other = app.workspace.active_document_id();
    for id in [first, second, other] {
        app.workspace.document_mut(id).unwrap().mark_dirty();
    }
    (app, source, target, first, second, other)
}

fn resolve(
    app: &mut App,
    window: EditorWindowId,
    document: DocumentId,
    decision: DirtyCloseDecision,
) {
    let start = Instant::now();
    for elapsed in [0, 200] {
        send(
            app,
            window,
            Message::ChromeAnimationFrame(start + Duration::from_millis(elapsed)),
        );
    }
    send(app, window, Message::DirtyCloseResolved(document, decision));
    for elapsed in [200, 400] {
        send(
            app,
            window,
            Message::ChromeAnimationFrame(start + Duration::from_millis(elapsed)),
        );
    }
    send(app, window, Message::DirtyCloseFadeFinished(document));
}

#[test]
fn another_windows_close_commands_preserve_the_original_prompt_queue_and_goal() {
    let (mut app, source, target, first, second, other) = setup();
    send(&mut app, source, Message::CloseWindow);
    assert_eq!(app.files.close_goal(), CloseGoal::CloseWindow(source));
    for command in [
        Message::CloseWindow,
        Message::CloseFile,
        Message::CloseAllFiles,
        Message::CloseScoped(BulkScope::AllWindows),
        Message::TabClosed(other),
        Message::Quit,
    ] {
        send(&mut app, target, command);
        assert_eq!(app.files.close_goal(), CloseGoal::CloseWindow(source));
        assert_eq!(
            app.files
                .pending_close_documents()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![second]
        );
        assert_eq!(
            app.window_ui(source).unwrap().close_prompt.document(),
            Some(first)
        );
        assert!(
            app.window_ui(target)
                .unwrap()
                .close_prompt
                .document()
                .is_none()
        );
        assert!(app.workspace.document(other).is_some());
    }
    resolve(&mut app, source, first, DirtyCloseDecision::Cancel);
    assert!(!app.close_operation_pending());
    send(&mut app, target, Message::CloseWindow);
    assert_eq!(app.files.close_goal(), CloseGoal::CloseWindow(target));
    assert_eq!(
        app.window_ui(target).unwrap().close_prompt.document(),
        Some(other)
    );
}

#[test]
fn original_bulk_close_continues_after_a_rejected_competing_window_close() {
    let (mut app, source, target, first, second, other) = setup();
    send(&mut app, source, Message::CloseWindow);
    send(&mut app, target, Message::CloseWindow);
    resolve(&mut app, source, first, DirtyCloseDecision::Discard);
    assert_eq!(
        app.window_ui(source).unwrap().close_prompt.document(),
        Some(second)
    );
    assert_eq!(app.files.close_goal(), CloseGoal::CloseWindow(source));
    resolve(&mut app, source, second, DirtyCloseDecision::Discard);
    assert!(!app.editor_windows.contains_key(&source));
    assert!(app.editor_windows.contains_key(&target));
    assert!(app.workspace.document(other).is_some());
    assert!(!app.close_operation_pending());
}

#[test]
fn a_close_waiting_for_save_keeps_its_goal_when_another_window_closes() {
    let (mut app, source, target, first, second, other) = setup();
    send(&mut app, source, Message::CloseWindow);
    resolve(&mut app, source, first, DirtyCloseDecision::Save);
    assert_eq!(app.files.pending_close_after_save(), Some(first));
    let request = app.files.pending_save().unwrap().clone();
    send(&mut app, target, Message::CloseWindow);
    assert_eq!(app.files.close_goal(), CloseGoal::CloseWindow(source));
    assert_eq!(app.files.pending_close_after_save(), Some(first));
    let _ = app.update(Message::FileSaved(
        request,
        Ok("saved-close-admission.txt".into()),
    ));
    assert!(app.workspace.document(first).is_none());
    assert_eq!(
        app.window_ui(source).unwrap().close_prompt.document(),
        Some(second)
    );
    assert!(app.workspace.document(other).is_some());
}
