use super::test_support::*;
use iced::event::Status;
use iced::{Event, keyboard};
use std::time::{Duration, Instant};

fn key_event(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

#[test]
fn selectors_keep_the_input_barrier_until_dismissal_finishes() {
    for (tabs, opened, changed, closed, submitted) in [
        (
            false,
            Message::WindowListOpened,
            Message::WindowListQueryChanged as fn(String) -> Message,
            Message::WindowListClosed,
            Message::WindowListSubmitted,
        ),
        (
            true,
            Message::TabListOpened,
            Message::TabListQueryChanged as fn(String) -> Message,
            Message::TabListClosed,
            Message::TabListSubmitted,
        ),
    ] {
        let (mut app, _) = App::new();
        let native = app.main_window_id.unwrap();
        let document = app.workspace.active_document_id();
        let count = app.workspace.documents().len();
        let query = app.workspace.active_document().unwrap().title();
        let _ = app.update(opened.clone());
        let _ = app.update(changed(query.clone()));
        let start = Instant::now();
        let close_start = start + Duration::from_millis(140);
        let _ = app.update(Message::ChromeAnimationFrame(start));
        let _ = app.update(Message::ChromeAnimationFrame(close_start));
        assert_eq!(app.chrome_animation.selector.progress(), 1.0);

        assert_eq!(app.update(closed.clone()).units(), 0);
        assert_eq!(
            app.update(Message::ChromeAnimationFrame(close_start))
                .units(),
            0
        );
        assert_eq!(
            app.update(Message::ChromeAnimationFrame(
                close_start + Duration::from_millis(70),
            ))
            .units(),
            0,
            "focus must wait for the selector to leave the widget tree"
        );
        assert!(app.chrome_animation.selector.progress() > 0.0);
        assert!(app.chrome_animation.selector.progress() < 1.0);
        assert!(app.chrome_animation.selector.rendered_visible());
        assert!(!app.chrome_animation.selector.target_visible());

        for message in [
            changed("ignored".into()),
            submitted,
            closed.clone(),
            opened.clone(),
        ] {
            assert_eq!(app.update(message).units(), 0);
        }
        let primary = if cfg!(target_os = "macos") {
            keyboard::Modifiers::LOGO
        } else {
            keyboard::Modifiers::CTRL
        };
        for event in [
            key_event(keyboard::Key::Character("n".into()), primary),
            key_event(
                keyboard::Key::Named(keyboard::key::Named::Tab),
                keyboard::Modifiers::empty(),
            ),
        ] {
            assert_eq!(
                app.update(Message::RuntimeEvent(event, Status::Ignored, native))
                    .units(),
                0
            );
        }
        assert_eq!(app.is_tab_list_visible, tabs);
        assert_eq!(app.is_window_list_visible, !tabs);
        assert_eq!(
            if tabs {
                &app.tab_list_query
            } else {
                &app.window_list_query
            },
            &query
        );
        assert_eq!(app.workspace.active_document_id(), document);
        assert_eq!(app.workspace.documents().len(), count);
        assert!(app.selector_close_action.is_none());

        let focus = app.update(Message::ChromeAnimationFrame(
            close_start + Duration::from_millis(140),
        ));
        assert!(
            focus.units() > 0,
            "restore focus after the final fade frame"
        );
        assert!(!app.is_tab_list_visible);
        assert!(!app.is_window_list_visible);
        assert!(!app.needs_animation_frames());

        let (mut app, _) = App::new();
        let _ = app.update(opened);
        assert!(app.update(closed).units() > 0);
        assert!(!app.is_tab_list_visible);
        assert!(!app.is_window_list_visible);
        assert!(!app.needs_animation_frames());
    }
}

#[test]
fn tab_selector_applies_deferred_selection_only_to_a_tab_still_owned_by_its_window() {
    for transfer_during_fade in [false, true] {
        let (mut app, _) = App::new();
        let source = app.workspace.current_window_id();
        let native = app.main_window_id.unwrap();
        let original = app.workspace.active_document_id();
        let _ = app.update(Message::NewFile);
        let selected = app.workspace.active_document_id();
        let _ = app.update(Message::TabSelected(original));
        let (other, _) = app.create_editor(false, None);
        let _ = app.update(Message::TabListOpened);
        let start = Instant::now();
        let close_start = start + Duration::from_millis(140);
        let _ = app.update(Message::ChromeAnimationFrame(start));
        let _ = app.update(Message::ChromeAnimationFrame(close_start));

        assert_eq!(app.update(Message::TabListSelected(selected)).units(), 0);
        assert_eq!(app.workspace.active_document_id(), original);
        assert!(app.is_tab_list_visible);
        assert!(app.selector_close_action.is_some());
        if transfer_during_fade {
            assert!(app.workspace.transfer_tabs(source, &[selected], other, 0));
        }
        let _ = app.update(Message::ChromeAnimationFrame(close_start));
        let _ = app.update(Message::ChromeAnimationFrame(
            close_start + Duration::from_millis(70),
        ));
        assert_eq!(app.workspace.active_document_id(), original);
        assert!(app.is_tab_list_visible);

        let focus = app.update(Message::ChromeAnimationFrame(
            close_start + Duration::from_millis(140),
        ));
        assert!(focus.units() > 0);
        assert!(!app.is_tab_list_visible);
        assert!(app.selector_close_action.is_none());
        assert_eq!(
            app.workspace.active_document_id(),
            if transfer_during_fade {
                original
            } else {
                selected
            }
        );
        assert_eq!(app.workspace.current_window_id(), source);
        assert_eq!(app.main_window_id, Some(native));
        assert!(app.editor_windows[&source].ui.is_none());
        assert!(app.editor_windows[&other].ui.is_some());
        assert_eq!(
            app.workspace.owner_of(selected),
            Some(if transfer_during_fade { other } else { source })
        );
    }
}
