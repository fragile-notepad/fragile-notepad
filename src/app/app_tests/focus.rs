//! Focus transitions through the complete application view and scoped tasks.
use super::test_support::*;
use futures::StreamExt;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::{self, operation};
use iced::event::Status;
use iced::{Event, Size, keyboard, mouse, window};
use iced_runtime::Action;
use iced_runtime::user_interface::{Cache, UserInterface};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

struct FocusedInput(Option<widget::Id>);

impl operation::Operation for FocusedInput {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn operation::Operation)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        id: Option<&widget::Id>,
        _bounds: iced::Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        if state.is_focused() {
            assert!(
                self.0.is_none(),
                "only one widget should own keyboard focus"
            );
            self.0 = id.cloned();
        }
    }
}

struct ViewHarness {
    renderer: iced::Renderer,
    cache: Option<Cache>,
    window: window::Id,
}

impl ViewHarness {
    fn new(window: window::Id) -> Self {
        Self {
            renderer: futures::executor::block_on(<iced::Renderer as Headless>::new(
                renderer::Settings::default(),
                Some("tiny-skia"),
            ))
            .expect("software renderer"),
            cache: Some(Cache::new()),
            window,
        }
    }

    fn update(
        &mut self,
        app: &App,
        actions: Vec<Action<Message>>,
        events: &[Event],
    ) -> (Vec<Message>, Vec<Status>, Option<widget::Id>) {
        // Match the runtime: rebuild from the previous cache before running
        // operations returned by the application update.
        let mut ui = UserInterface::build(
            app.view(self.window),
            Size::new(1024.0, 768.0),
            self.cache.take().unwrap(),
            &mut self.renderer,
        );
        for action in actions {
            if let Action::WindowWidget(target, mut operation) = action {
                assert_eq!(target, self.window);
                loop {
                    ui.operate(&self.renderer, operation.as_mut());
                    match operation.finish() {
                        operation::Outcome::Chain(next) => operation = next,
                        _ => break,
                    }
                }
            } else if let Action::Widget(_) = action {
                panic!("application focus operations must target their editor window");
            }
        }
        let mut focused = FocusedInput(None);
        ui.operate(&self.renderer, &mut focused);
        let mut messages = Vec::new();
        let (_, statuses) = ui.update(
            &window::Headless,
            &Waker::noop(),
            events,
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut messages,
        );
        self.cache = Some(ui.into_cache());
        (messages, statuses, focused.0)
    }
}

fn immediate_actions(app: &mut App, task: iced::Task<Message>) -> Vec<Action<Message>> {
    let mut tasks = vec![task];
    let mut actions = Vec::new();
    while let Some(task) = tasks.pop() {
        let Some(mut stream) = iced_runtime::task::into_stream(task) else {
            continue;
        };
        let mut context = Context::from_waker(futures::task::noop_waker_ref());
        while let Poll::Ready(Some(action)) = stream.poll_next_unpin(&mut context) {
            if let Action::Output(message) = action {
                tasks.push(app.update(message));
            } else {
                actions.push(action);
            }
        }
        // Background disk and syntax work is unrelated to this focus cycle.
    }
    actions
}

fn character(value: &'static str, code: keyboard::key::Code, command: bool) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Character(value.into()),
        modified_key: keyboard::Key::Character(value.into()),
        physical_key: keyboard::key::Physical::Code(code),
        location: keyboard::Location::Standard,
        modifiers: if command {
            keyboard::Modifiers::COMMAND
        } else {
            keyboard::Modifiers::empty()
        },
        // AppKit supplies printable text even for command chords.
        text: Some(value.into()),
        repeat: false,
    })
}

fn apply_widget_messages(app: &mut App, messages: Vec<Message>) -> Vec<Action<Message>> {
    messages
        .into_iter()
        .flat_map(|message| {
            let task = app.update(message);
            immediate_actions(app, task)
        })
        .collect()
}

#[test]
fn find_shortcut_focuses_full_app_view_before_and_after_animation() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let (mut app, _) = App::new();
    set_active_document_text(
        &mut app,
        "needle in the document",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    let native = app.main_window_id.unwrap();
    let mut view = ViewHarness::new(native);
    let focus_editor =
        iced::widget::operation::focus(crate::ui::editor::EDITOR_ID).scope_window(native);
    let actions = immediate_actions(&mut app, focus_editor);
    let (messages, _, focused) = view.update(
        &app,
        actions,
        &[Event::Window(
            window::Event::RedrawRequested(Instant::now()),
        )],
    );
    assert_eq!(focused, Some(widget::Id::new(crate::ui::editor::EDITOR_ID)));
    let actions = apply_widget_messages(&mut app, messages);
    view.update(&app, actions, &[]);

    let shortcut = character("f", keyboard::key::Code::KeyF, true);
    let (messages, statuses, _) = view.update(&app, vec![], std::slice::from_ref(&shortcut));
    assert!(messages.is_empty(), "Cmd+F must not edit the document");
    assert_eq!(statuses, [Status::Ignored]);
    let message = crate::app::shortcuts::event_to_message(shortcut, statuses[0], native).unwrap();
    let task = app.update(message);
    let actions = immediate_actions(&mut app, task);
    assert!(app.is_find_visible);
    assert_eq!(app.chrome_animation_info().find_progress, 0.0);
    let (_, _, focused) = view.update(&app, actions, &[]);
    let query_id = widget::Id::new(crate::ui::find_panel::FIND_INPUT_ID);
    assert_eq!(focused, Some(query_id.clone()), "initial Find focus");

    let start = Instant::now();
    for elapsed in [0, 70, 200] {
        let task = app.update(Message::ChromeAnimationFrame(
            start + Duration::from_millis(elapsed),
        ));
        let actions = immediate_actions(&mut app, task);
        let (messages, _, focused) = view.update(
            &app,
            actions,
            &[Event::Window(window::Event::RedrawRequested(
                start + Duration::from_millis(elapsed),
            ))],
        );
        assert_eq!(
            focused,
            Some(query_id.clone()),
            "Find focus at {elapsed} ms"
        );
        let actions = apply_widget_messages(&mut app, messages);
        view.update(&app, actions, &[]);
    }

    let (messages, statuses, _) = view.update(
        &app,
        vec![],
        &[
            Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::empty(),
            )),
            character("n", keyboard::key::Code::KeyN, false),
        ],
    );
    assert_eq!(statuses.last(), Some(&Status::Captured));
    assert_eq!(messages.len(), 1, "typing must reach exactly one field");
    let mut message = messages.into_iter().next().unwrap();
    while let Message::ForWindow(_, inner) = message {
        message = *inner;
    }
    assert!(matches!(&message, Message::FindQueryChanged(query) if query == "n"));
    let _ = app.update(Message::ForWindow(native, Box::new(message)));
    assert_eq!(app.find.query, "n");
    assert_eq!(
        app.workspace.active_document().unwrap().buffer.text(),
        "needle in the document",
    );
}

#[test]
fn startup_new_tab_and_new_window_accept_typing_without_clicking() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    for scenario in ["startup", "new tab", "new window"] {
        let (mut app, _) = App::new();
        let original = app.main_window_id.unwrap();
        let mut view = ViewHarness::new(original);
        // A dismissed modal can leave the remounted editor unfocused. Creating
        // a document must establish focus independently of the prior view.
        view.update(&app, vec![], &[]);
        let task = app.update(match scenario {
            "startup" => Message::WindowOpened(original),
            "new tab" => Message::NewFile,
            "new window" => Message::NewWindow,
            _ => unreachable!(),
        });
        let mut actions = immediate_actions(&mut app, task);
        if scenario == "new window" {
            let native = app.main_window_id.unwrap();
            assert_ne!(native, original);
            view = ViewHarness::new(native);
            let task = app
                .update(Message::WindowOpened(native))
                .scope_window(original);
            actions = immediate_actions(&mut app, task);
        }
        let (messages, _, focused) = view.update(
            &app,
            actions,
            &[Event::Window(
                window::Event::RedrawRequested(Instant::now()),
            )],
        );
        assert_eq!(
            focused,
            Some(widget::Id::new(crate::ui::editor::EDITOR_ID)),
            "{scenario}",
        );
        let actions = apply_widget_messages(&mut app, messages);
        let (messages, statuses, _) = view.update(
            &app,
            actions,
            &[character("n", keyboard::key::Code::KeyN, false)],
        );
        assert_eq!(statuses, [Status::Captured]);
        apply_widget_messages(&mut app, messages);
        assert_eq!(app.workspace.active_document().unwrap().buffer.text(), "n");
    }
}

fn frame(app: &mut App, view: &mut ViewHarness, at: Instant) -> Option<widget::Id> {
    let task = app.update(Message::ChromeAnimationFrame(at));
    let actions = immediate_actions(app, task);
    let (messages, _, _) = view.update(
        app,
        actions,
        &[Event::Window(window::Event::RedrawRequested(at))],
    );
    let actions = apply_widget_messages(app, messages);
    view.update(app, actions, &[]).2
}

#[test]
fn find_paste_captures_shortcut_while_clipboard_read_is_pending() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let (mut app, _) = App::new();
    set_active_document_text(
        &mut app,
        "keep the document unchanged",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    let native = app.main_window_id.unwrap();
    let mut view = ViewHarness::new(native);
    let task = app.update(Message::ToggleFind);
    let actions = immediate_actions(&mut app, task);
    view.update(&app, actions, &[]);
    let start = Instant::now();
    frame(&mut app, &mut view, start);
    frame(&mut app, &mut view, start + Duration::from_millis(200));

    let paste = character("v", keyboard::key::Code::KeyV, true);
    let (messages, statuses, focused) = view.update(
        &app,
        vec![],
        &[
            Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::COMMAND,
            )),
            paste.clone(),
        ],
    );
    assert_eq!(
        focused,
        Some(widget::Id::new(crate::ui::find_panel::FIND_INPUT_ID))
    );
    assert!(messages.is_empty(), "the clipboard has not returned yet");
    assert_eq!(statuses.last(), Some(&Status::Captured));
    assert!(
        crate::app::shortcuts::event_to_message(paste.clone(), *statuses.last().unwrap(), native)
            .is_none()
    );

    let (messages, statuses, _) = view.update(&app, vec![], std::slice::from_ref(&paste));
    assert!(messages.is_empty());
    assert_eq!(
        statuses,
        [Status::Captured],
        "a pending read still owns Paste"
    );

    let release = Event::Keyboard(keyboard::Event::KeyReleased {
        key: keyboard::Key::Character("v".into()),
        modified_key: keyboard::Key::Character("v".into()),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyV),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::COMMAND,
    });
    view.update(&app, vec![], std::slice::from_ref(&release));

    let clipboard = Event::Clipboard(iced::advanced::clipboard::Event::Read(Ok(
        std::sync::Arc::new(iced::clipboard::Content::Text("needle".into())),
    )));
    let (messages, statuses, _) = view.update(&app, vec![], &[clipboard]);
    assert_eq!(statuses, [Status::Captured]);
    let actions = apply_widget_messages(&mut app, messages);
    let (_, _, focused) = view.update(&app, actions, &[]);
    assert_eq!(app.find.query, "needle");
    assert_eq!(
        app.workspace.active_document().unwrap().buffer.text(),
        "keep the document unchanged"
    );
    assert_eq!(
        focused,
        Some(widget::Id::new(crate::ui::find_panel::FIND_INPUT_ID))
    );

    // A released paste must fetch the current clipboard on the next press,
    // and a failed read must not leave the field waiting forever.
    let (messages, statuses, _) = view.update(&app, vec![], std::slice::from_ref(&paste));
    assert!(
        messages.is_empty(),
        "a new Paste needs a fresh clipboard read"
    );
    assert_eq!(statuses, [Status::Captured]);
    view.update(&app, vec![], std::slice::from_ref(&release));
    let error = Event::Clipboard(iced::advanced::clipboard::Event::Read(Err(
        iced::clipboard::Error::ClipboardUnavailable,
    )));
    view.update(&app, vec![], &[error]);
    let (messages, statuses, _) = view.update(&app, vec![], &[paste]);
    assert!(messages.is_empty());
    assert_eq!(statuses, [Status::Captured]);
    view.update(&app, vec![], &[release]);
    let clipboard = Event::Clipboard(iced::advanced::clipboard::Event::Read(Ok(
        std::sync::Arc::new(iced::clipboard::Content::Text(" again".into())),
    )));
    let (messages, statuses, _) = view.update(&app, vec![], &[clipboard]);
    assert_eq!(statuses, [Status::Captured]);
    apply_widget_messages(&mut app, messages);
    assert_eq!(app.find.query, "needle again");
    assert_eq!(
        app.workspace.active_document().unwrap().buffer.text(),
        "keep the document unchanged"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn close_shortcuts_reach_tab_and_window_handlers_from_editor_and_find() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    for close_window in [false, true] {
        for from_find in [false, true] {
            let (mut app, _) = App::new();
            if close_window {
                let _ = app.update(Message::NewWindow);
            }
            set_active_document_text(
                &mut app,
                "draft",
                EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
            );
            app.workspace.active_document_mut().unwrap().mark_dirty();
            let document = app.workspace.active_document_id();
            let native = app.main_window_id.unwrap();
            let mut view = ViewHarness::new(native);
            if from_find {
                let task = app.update(Message::ToggleFind);
                let actions = immediate_actions(&mut app, task);
                view.update(&app, actions, &[]);
                let start = Instant::now();
                frame(&mut app, &mut view, start);
                frame(&mut app, &mut view, start + Duration::from_millis(200));
            } else {
                let focus = iced::widget::operation::focus(crate::ui::editor::EDITOR_ID)
                    .scope_window(native);
                let actions = immediate_actions(&mut app, focus);
                view.update(&app, actions, &[]);
            }
            let modifiers = keyboard::Modifiers::COMMAND
                | if close_window {
                    keyboard::Modifiers::SHIFT
                } else {
                    keyboard::Modifiers::empty()
                };
            let mut shortcut = character("w", keyboard::key::Code::KeyW, true);
            if let Event::Keyboard(keyboard::Event::KeyPressed {
                modifiers: event_modifiers,
                ..
            }) = &mut shortcut
            {
                *event_modifiers = modifiers;
            }
            let (messages, statuses, _) = view.update(
                &app,
                vec![],
                &[
                    Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
                    shortcut.clone(),
                ],
            );
            assert_eq!(statuses.last(), Some(&Status::Ignored));
            apply_widget_messages(&mut app, messages);
            let message = crate::app::shortcuts::event_to_message(
                shortcut,
                *statuses.last().unwrap(),
                native,
            )
            .unwrap();
            let _ = app.update(message);
            assert_eq!(app.close_prompt.document(), Some(document));
            assert_eq!(
                app.workspace.active_document().unwrap().buffer.text(),
                "draft"
            );
            assert!(app.find.query.is_empty());
            if close_window {
                assert!(matches!(app.files.close_goal(), CloseGoal::CloseWindow(_)));
            } else {
                assert!(matches!(app.files.close_goal(), CloseGoal::KeepOpen));
            }
        }
    }
}

#[test]
fn hiding_find_preserves_editor_focus_after_its_final_animation_frame() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let (mut app, _) = App::new();
    let native = app.main_window_id.unwrap();
    let mut view = ViewHarness::new(native);
    view.update(&app, vec![], &[]);
    let task = app.update(Message::ToggleFind);
    let actions = immediate_actions(&mut app, task);
    view.update(&app, actions, &[]);
    let start = Instant::now();
    frame(&mut app, &mut view, start);
    frame(&mut app, &mut view, start + Duration::from_millis(200));

    let task = app.update(Message::HideFind);
    let actions = immediate_actions(&mut app, task);
    let (_, _, focused) = view.update(&app, actions, &[]);
    let editor_id = widget::Id::new(crate::ui::editor::EDITOR_ID);
    assert_eq!(focused, Some(editor_id.clone()));
    frame(&mut app, &mut view, start + Duration::from_millis(300));
    assert_eq!(
        frame(&mut app, &mut view, start + Duration::from_millis(500)),
        Some(editor_id),
        "removing the closed Find panel must retain editor state",
    );
    assert!(!app.chrome_animation_info().find_rendered_visible);
    let (messages, statuses, _) = view.update(
        &app,
        vec![],
        &[character("n", keyboard::key::Code::KeyN, false)],
    );
    assert_eq!(statuses, [Status::Captured]);
    apply_widget_messages(&mut app, messages);
    assert_eq!(app.workspace.active_document().unwrap().buffer.text(), "n");
}

#[test]
fn dirty_close_cancel_and_discard_restore_editor_focus_after_fading() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    for (decision, inline_find) in [
        (DirtyCloseDecision::Cancel, false),
        (DirtyCloseDecision::Discard, false),
        (DirtyCloseDecision::Cancel, true),
        (DirtyCloseDecision::Discard, true),
    ] {
        let (mut app, _) = App::new();
        if decision == DirtyCloseDecision::Discard {
            let _ = app.update(Message::NewFile);
        }
        set_active_document_text(
            &mut app,
            "draft",
            EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
        );
        app.workspace.active_document_mut().unwrap().mark_dirty();
        let document = app.workspace.active_document_id();
        let native = app.main_window_id.unwrap();
        let mut view = ViewHarness::new(native);
        if inline_find {
            let task = app.update(Message::ShowInlineReplace);
            let actions = immediate_actions(&mut app, task);
            view.update(&app, actions, &[]);
            let start = Instant::now();
            frame(&mut app, &mut view, start);
            frame(&mut app, &mut view, start + Duration::from_millis(200));
        }
        let focus =
            iced::widget::operation::focus(crate::ui::editor::EDITOR_ID).scope_window(native);
        let actions = immediate_actions(&mut app, focus);
        view.update(&app, actions, &[]);

        let task = app.update(Message::CloseFile);
        let actions = immediate_actions(&mut app, task);
        view.update(&app, actions, &[]);
        let start = Instant::now();
        frame(&mut app, &mut view, start);
        frame(&mut app, &mut view, start + Duration::from_millis(200));
        assert_eq!(app.close_prompt.document(), Some(document));
        let task = app.update(Message::DirtyCloseResolved(document, decision));
        let actions = immediate_actions(&mut app, task);
        let (_, _, focused) = view.update(&app, actions, &[]);
        assert_eq!(
            focused, None,
            "the closing modal must retain its input barrier"
        );
        assert_eq!(
            frame(&mut app, &mut view, start + Duration::from_millis(300)),
            None,
        );
        assert_eq!(
            frame(&mut app, &mut view, start + Duration::from_millis(500)),
            Some(widget::Id::new(crate::ui::editor::EDITOR_ID)),
            "{decision:?}, inline_find={inline_find}",
        );
        assert_eq!(app.close_prompt.document(), None);
        assert_eq!(app.is_find_visible, inline_find);
        let before = app.workspace.active_document().unwrap().buffer.text();
        let (messages, statuses, _) = view.update(
            &app,
            vec![],
            &[character("n", keyboard::key::Code::KeyN, false)],
        );
        assert_eq!(statuses, [Status::Captured]);
        apply_widget_messages(&mut app, messages);
        assert_eq!(
            app.workspace.active_document().unwrap().buffer.text(),
            format!("n{before}"),
        );
    }
}

#[test]
fn dirty_close_save_restores_editor_focus_after_native_dialog_completion() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    for (save_succeeds, newer_find) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let (mut app, _) = App::new();
        if save_succeeds {
            let _ = app.update(Message::NewFile);
        }
        set_active_document_text(
            &mut app,
            "draft",
            EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
        );
        app.workspace.active_document_mut().unwrap().mark_dirty();
        let document = app.workspace.active_document_id();
        let native = app.main_window_id.unwrap();
        let mut view = ViewHarness::new(native);
        let task = app.update(Message::ShowInlineReplace);
        let actions = immediate_actions(&mut app, task);
        view.update(&app, actions, &[]);
        let start = Instant::now();
        frame(&mut app, &mut view, start);
        frame(&mut app, &mut view, start + Duration::from_millis(200));
        let focus =
            iced::widget::operation::focus(crate::ui::editor::EDITOR_ID).scope_window(native);
        let actions = immediate_actions(&mut app, focus);
        view.update(&app, actions, &[]);

        let task = app.update(Message::CloseFile);
        let actions = immediate_actions(&mut app, task);
        view.update(&app, actions, &[]);
        let start = Instant::now();
        frame(&mut app, &mut view, start);
        frame(&mut app, &mut view, start + Duration::from_millis(200));
        let task = app.update(Message::DirtyCloseResolved(
            document,
            DirtyCloseDecision::Save,
        ));
        let actions = immediate_actions(&mut app, task);
        view.update(&app, actions, &[]);
        frame(&mut app, &mut view, start + Duration::from_millis(300));
        assert_eq!(
            frame(&mut app, &mut view, start + Duration::from_millis(500)),
            None,
            "native save dialog still owns input",
        );
        assert_eq!(app.close_prompt.document(), None);
        assert_eq!(app.files.pending_close_after_save(), Some(document));
        let request = app
            .files
            .pending_save()
            .cloned()
            .expect("pending native save");
        if newer_find {
            // The sheet has closed, but the async save result has not arrived.
            // A user can focus Find before that result returns to the editor.
            let focus = iced::widget::operation::focus(crate::ui::find_panel::FIND_INPUT_ID)
                .scope_window(native);
            let actions = immediate_actions(&mut app, focus);
            view.update(&app, actions, &[]);
        }
        let result = if save_succeeds {
            Ok(std::env::temp_dir().join("fragile-focus-save.txt"))
        } else {
            Err(crate::services::types::FileError::DialogClosed)
        };
        // Simulate only the native result; this test never opens a dialog or
        // writes a file. The same completion message drives production saves.
        let task = app.update(Message::FileSaved(request, result));
        let actions = immediate_actions(&mut app, task);
        let (_, _, focused) = view.update(&app, actions, &[]);
        assert_eq!(
            focused,
            Some(widget::Id::new(if newer_find {
                crate::ui::find_panel::FIND_INPUT_ID
            } else {
                crate::ui::editor::EDITOR_ID
            })),
        );
        assert_eq!(app.files.pending_close_after_save(), None);
        assert_eq!(app.workspace.document(document).is_none(), save_succeeds);
        assert!(app.is_find_visible);
        let before = app.workspace.active_document().unwrap().buffer.text();
        let (messages, statuses, _) = view.update(
            &app,
            vec![],
            &[character("n", keyboard::key::Code::KeyN, false)],
        );
        assert_eq!(statuses, [Status::Captured]);
        apply_widget_messages(&mut app, messages);
        if newer_find {
            assert_eq!(app.find.query, "n");
            assert_eq!(
                app.workspace.active_document().unwrap().buffer.text(),
                before
            );
        } else {
            assert_eq!(
                app.workspace.active_document().unwrap().buffer.text(),
                format!("n{before}"),
            );
        }
    }
}

#[test]
fn dirty_close_save_encoding_failure_restores_editor_focus_without_native_dialog() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let (mut app, _) = App::new();
    set_active_document_text(
        &mut app,
        "draft🙂",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    app.workspace
        .active_document_mut()
        .unwrap()
        .set_encoding(crate::core::TextEncoding::Windows1252);
    let document = app.workspace.active_document_id();
    let native = app.main_window_id.unwrap();
    let mut view = ViewHarness::new(native);
    let task = app.update(Message::ShowInlineReplace);
    let actions = immediate_actions(&mut app, task);
    view.update(&app, actions, &[]);
    let start = Instant::now();
    frame(&mut app, &mut view, start);
    frame(&mut app, &mut view, start + Duration::from_millis(200));
    let focus = iced::widget::operation::focus(crate::ui::editor::EDITOR_ID).scope_window(native);
    let actions = immediate_actions(&mut app, focus);
    view.update(&app, actions, &[]);

    let task = app.update(Message::CloseFile);
    let actions = immediate_actions(&mut app, task);
    view.update(&app, actions, &[]);
    let start = Instant::now();
    frame(&mut app, &mut view, start);
    frame(&mut app, &mut view, start + Duration::from_millis(200));
    let task = app.update(Message::DirtyCloseResolved(
        document,
        DirtyCloseDecision::Save,
    ));
    let actions = immediate_actions(&mut app, task);
    view.update(&app, actions, &[]);
    assert_eq!(
        frame(&mut app, &mut view, start + Duration::from_millis(300)),
        None
    );
    assert_eq!(
        frame(&mut app, &mut view, start + Duration::from_millis(500)),
        Some(widget::Id::new(crate::ui::editor::EDITOR_ID)),
    );
    assert_eq!(app.close_prompt.document(), None);
    assert_eq!(app.files.pending_close_after_save(), None);
    assert!(
        app.files.pending_save().is_none(),
        "encoding failed before native Save"
    );
    assert!(
        app.file_status
            .as_deref()
            .is_some_and(|status| status.starts_with("Save failed:"))
    );
    assert!(app.workspace.document(document).unwrap().is_dirty);
    assert!(app.is_find_visible);

    let (messages, statuses, _) = view.update(
        &app,
        vec![],
        &[character("n", keyboard::key::Code::KeyN, false)],
    );
    assert_eq!(statuses, [Status::Captured]);
    apply_widget_messages(&mut app, messages);
    assert_eq!(
        app.workspace.active_document().unwrap().buffer.text(),
        "ndraft🙂"
    );
}

#[test]
fn find_paste_recovers_after_focus_loss_and_ignores_late_clipboard_results() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let release = Event::Keyboard(keyboard::Event::KeyReleased {
        key: keyboard::Key::Character("v".into()),
        modified_key: keyboard::Key::Character("v".into()),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyV),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::COMMAND,
    });
    let clipboard = |text: &str| {
        Event::Clipboard(iced::advanced::clipboard::Event::Read(Ok(
            std::sync::Arc::new(iced::clipboard::Content::Text(text.into())),
        )))
    };

    for blur in ["operation", "mouse", "window"] {
        for late_response in [false, true] {
            let (mut app, _) = App::new();
            set_active_document_text(
                &mut app,
                "keep the document unchanged",
                EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
            );
            let native = app.main_window_id.unwrap();
            let mut view = ViewHarness::new(native);
            let task = app.update(Message::ToggleFind);
            let actions = immediate_actions(&mut app, task);
            view.update(&app, actions, &[]);
            let start = Instant::now();
            frame(&mut app, &mut view, start);
            frame(&mut app, &mut view, start + Duration::from_millis(200));
            let paste = character("v", keyboard::key::Code::KeyV, true);
            view.update(
                &app,
                vec![],
                &[
                    Event::Keyboard(keyboard::Event::ModifiersChanged(
                        keyboard::Modifiers::COMMAND,
                    )),
                    paste.clone(),
                    release.clone(),
                ],
            );

            match blur {
                "operation" => {
                    let focus = iced::widget::operation::focus(crate::ui::editor::EDITOR_ID)
                        .scope_window(native);
                    let actions = immediate_actions(&mut app, focus);
                    view.update(&app, actions, &[]);
                }
                "mouse" => {
                    // The harness cursor is outside every input.
                    view.update(
                        &app,
                        vec![],
                        &[Event::Mouse(mouse::Event::ButtonPressed(
                            mouse::Button::Left,
                        ))],
                    );
                }
                "window" => {
                    view.update(&app, vec![], &[Event::Window(window::Event::Unfocused)]);
                }
                _ => unreachable!(),
            }

            if !late_response {
                let (messages, _, _) = view.update(&app, vec![], &[clipboard("stale")]);
                assert!(
                    messages.is_empty(),
                    "blurred input must discard its paste: {blur}"
                );
            }

            let focus = iced::widget::operation::focus(crate::ui::find_panel::FIND_INPUT_ID)
                .scope_window(native);
            let actions = immediate_actions(&mut app, focus);
            view.update(&app, actions, &[]);
            let (messages, statuses, _) =
                view.update(&app, vec![], &[paste.clone(), release.clone()]);
            assert!(messages.is_empty());
            assert_eq!(statuses, [Status::Captured, Status::Captured]);

            if late_response {
                let (messages, _, _) = view.update(&app, vec![], &[clipboard("stale")]);
                assert!(
                    messages.is_empty(),
                    "a cancelled reply must not satisfy a newer paste: {blur}",
                );
            }

            let (messages, statuses, _) = view.update(&app, vec![], &[clipboard("fresh")]);
            assert_eq!(statuses, [Status::Captured]);
            let actions = apply_widget_messages(&mut app, messages);
            view.update(&app, actions, &[]);
            assert_eq!(app.find.query, "fresh", "paste recovers after {blur}");

            // An unsupported clipboard payload must complete the read too.
            view.update(&app, vec![], &[paste.clone(), release.clone()]);
            let non_text = Event::Clipboard(iced::advanced::clipboard::Event::Read(Ok(
                std::sync::Arc::new(iced::clipboard::Content::Html("ignored".into())),
            )));
            let (messages, _, _) = view.update(&app, vec![], &[non_text]);
            assert!(messages.is_empty());
            view.update(&app, vec![], &[paste, release.clone()]);
            let (messages, _, _) = view.update(&app, vec![], &[clipboard(" again")]);
            apply_widget_messages(&mut app, messages);
            assert_eq!(app.find.query, "fresh again");
            assert_eq!(
                app.workspace.active_document().unwrap().buffer.text(),
                "keep the document unchanged",
            );
        }
    }
}
