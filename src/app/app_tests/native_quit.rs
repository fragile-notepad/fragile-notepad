use super::test_support::*;
use futures::{FutureExt, StreamExt};
use iced::advanced::subscription;

fn cancels_native_quit(task: iced::Task<Message>) -> bool {
    let Some(mut actions) = iced_runtime::task::into_stream(task) else {
        return false;
    };
    while let Some(Some(action)) = actions.next().now_or_never() {
        if matches!(
            action,
            iced_runtime::Action::System(iced_runtime::system::Action::CancelQuit)
        ) {
            return true;
        }
    }
    false
}

#[test]
fn native_quit_refused_during_startup_replies_to_appkit() {
    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions::default());
    assert!(cancels_native_quit(
        app.update(Message::NativeQuitRequested)
    ));
    assert!(!app.lifecycle.is_exiting());
    assert!(
        app.file_status
            .as_deref()
            .unwrap()
            .contains("Finish starting")
    );
}

#[test]
fn native_quit_immediate_exit_is_accepted() {
    let (mut app, _) = App::new();
    assert!(!cancels_native_quit(
        app.update(Message::NativeQuitRequested)
    ));
    assert!(app.lifecycle.is_exiting());
}

#[test]
fn native_quit_during_existing_shutdown_waits_for_its_completion() {
    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions::default());
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(None)));
    let _ = app.update(Message::StartupReady);
    let _ = app.update(Message::Quit);
    assert!(app.lifecycle.is_exiting());
    assert!(!cancels_native_quit(
        app.update(Message::NativeQuitRequested)
    ));
    assert!(app.lifecycle.is_exiting());
    assert!(cancels_native_quit(app.update(Message::ShutdownPersisted(
        Err("disk unavailable".into())
    ))));
}

#[test]
fn native_quit_cancelled_in_dirty_prompt_replies_once() {
    let (mut app, _) = App::new();
    let id = app.workspace.active_document_id();
    app.workspace.active_document_mut().unwrap().mark_dirty();
    assert!(!cancels_native_quit(
        app.update(Message::NativeQuitRequested)
    ));
    assert_eq!(app.close_prompt.document(), Some(id));
    assert!(cancels_native_quit(app.update(
        Message::DirtyCloseResolved(id, crate::core::DirtyCloseDecision::Cancel,)
    )));
    assert!(!cancels_native_quit(app.update(Message::None)));
    assert!(app.workspace.document(id).is_some());
}

#[test]
fn native_quit_save_dialog_cancellation_replies_to_appkit() {
    let (mut app, _) = App::new();
    let id = app.workspace.active_document_id();
    app.workspace.active_document_mut().unwrap().mark_dirty();
    let _ = app.update(Message::NativeQuitRequested);
    let _ = app.update(Message::DirtyCloseResolved(
        id,
        crate::core::DirtyCloseDecision::Save,
    ));
    let request = app.files.pending_save().cloned().unwrap();
    assert!(cancels_native_quit(app.update(Message::FileSaved(
        request,
        Err(crate::services::types::FileError::DialogClosed),
    ))));
    assert!(app.workspace.document(id).is_some());
}

#[test]
fn native_quit_aborted_by_an_existing_save_replies_to_appkit() {
    let (mut app, _) = App::new();
    let id = app.workspace.active_document_id();
    app.workspace.active_document_mut().unwrap().mark_dirty();
    let _ = app.update(Message::NativeQuitRequested);
    let _ = app.update(Message::SaveFile);
    assert!(app.files.pending_save().is_some());
    assert!(cancels_native_quit(app.update(
        Message::DirtyCloseResolved(id, crate::core::DirtyCloseDecision::Save,)
    )));
    assert_eq!(app.files.close_goal(), crate::app::CloseGoal::KeepOpen);
}

#[test]
fn native_quit_save_failure_replies_to_appkit_and_allows_retry() {
    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions::default());
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(None)));
    let _ = app.update(Message::StartupReady);
    let _ = app.update(Message::NativeQuitRequested);
    assert!(app.lifecycle.is_exiting());
    assert!(cancels_native_quit(app.update(Message::ShutdownPersisted(
        Err("disk unavailable".into(),)
    ))));
    assert!(!app.lifecycle.is_exiting());
    let _ = app.update(Message::NativeQuitRequested);
    assert!(app.lifecycle.is_exiting());
}

#[test]
fn native_quit_enters_shutdown_with_edits_still_waiting_for_session_timer() {
    let (mut app, _) = App::new_with_options(crate::startup::StartupOptions::default());
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    let _ = app.update(Message::SessionLoaded(Ok(None)));
    let _ = app.update(Message::StartupReady);
    let id = app.workspace.active_document_id();
    let _ = app.update(Message::EditorAction(
        id,
        crate::editor::EditorAction::InsertText("just typed!".into()),
    ));
    assert!(app.session.flush_scheduled());

    let native_quit = subscription::Event::PlatformSpecific(subscription::PlatformSpecific::MacOS(
        subscription::MacOS::QuitRequested,
    ));
    let mut quit_messages = Vec::new();
    for recipe in subscription::into_recipes(app.subscriptions()) {
        let mut output = recipe.stream(Box::pin(futures::stream::iter([native_quit.clone()])));
        while let Some(Some(message)) = output.next().now_or_never() {
            if matches!(message, Message::NativeQuitRequested) {
                quit_messages.push(message);
            }
        }
    }
    assert_eq!(quit_messages.len(), 1);

    let _ = app.update(quit_messages.pop().unwrap());
    assert!(app.lifecycle.is_exiting());
    let snapshot = app.snapshot_session();
    assert_eq!(
        snapshot.documents[snapshot.active_index].text.as_deref(),
        Some("just typed!")
    );
}
