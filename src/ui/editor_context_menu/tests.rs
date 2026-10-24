use super::*;
use crate::core::DocumentId;
use crate::editor::{EditorPosition, EditorSelection};
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::Headless;
use iced::keyboard::{self, Key, key::Named};

fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("CPU renderer")
}

fn key(named: Named) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(named),
        modified_key: Key::Named(named),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

#[test]
fn right_click_preserves_selection_and_escape_returns_keyboard_to_editor() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let mut document = Document::from_path(
        DocumentId::new(101),
        "context.txt",
        "alpha beta\nsecond line",
    );
    document.set_main_selection(EditorSelection::new(
        EditorPosition::new(0, 0),
        EditorPosition::new(0, 5),
    ));
    let mut editor = crate::ui::editor::view(&document, &settings);
    let mut tree = Tree::empty();
    tree.diff(editor.as_widget_mut());
    let viewport = Rectangle::with_size(Size::new(800.0, 600.0));
    let node = editor.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(viewport.size(), viewport.size()),
    );
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    editor.as_widget_mut().update(
        &mut tree,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        Layout::new(&node),
        mouse::Cursor::Available(Point::new(100.0, 14.0)),
        &renderer,
        &mut shell,
        &viewport,
    );
    assert!(!messages.iter().any(|message| matches!(
        message,
        Message::EditorAction(_, crate::editor::EditorAction::PlaceCaret(_))
    )));
    let mut frame_messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut frame_messages);
    editor.as_widget_mut().update(
        &mut tree,
        &Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        )),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut shell,
        &viewport,
    );
    assert!(matches!(
        shell.input_method(),
        iced::advanced::InputMethod::Disabled
    ));
    {
        let mut overlay = editor
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &viewport,
                Vector::ZERO,
            )
            .expect("context menu");
        let menu_node = overlay.as_overlay_mut().layout(&renderer, viewport.size());
        let now = iced::time::Instant::now();
        for instant in [now, now + std::time::Duration::from_millis(200)] {
            let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
            overlay.as_overlay_mut().update(
                &Event::Window(iced::window::Event::RedrawRequested(instant)),
                Layout::new(&menu_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
            );
        }
        let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
        overlay.as_overlay_mut().update(
            &key(Named::Escape),
            Layout::new(&menu_node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut shell,
        );
    }
    assert!(tree.state.downcast_ref::<State>().popup.is_open());
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut frame_messages);
    editor.as_widget_mut().update(
        &mut tree,
        &Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        )),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut shell,
        &viewport,
    );
    assert!(matches!(
        shell.input_method(),
        iced::advanced::InputMethod::Disabled
    ));
    {
        let mut overlay = editor
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &viewport,
                Vector::ZERO,
            )
            .expect("exiting context menu");
        let now = iced::time::Instant::now();
        for instant in [now, now + std::time::Duration::from_millis(200)] {
            let menu_node = overlay.as_overlay_mut().layout(&renderer, viewport.size());
            let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
            overlay.as_overlay_mut().update(
                &Event::Window(iced::window::Event::RedrawRequested(instant)),
                Layout::new(&menu_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
            );
            assert!(!shell.is_event_captured());
        }
    }
    assert!(!tree.state.downcast_ref::<State>().popup.is_open());
    assert!(
        tree.children[0]
            .state
            .downcast_ref::<AdvancedEditorState<<Renderer as text::Renderer>::Paragraph>>()
            .is_focused()
    );
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut frame_messages);
    editor.as_widget_mut().update(
        &mut tree,
        &Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        )),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut shell,
        &viewport,
    );
    assert!(matches!(
        shell.input_method(),
        iced::advanced::InputMethod::Enabled { .. }
    ));
}

#[test]
fn command_availability_and_custom_shortcut_survive_menu_construction() {
    let settings = EditorSettings::default();
    let document = Document::untitled(DocumentId::new(102));
    let items = entries(&document, &settings);
    assert!(
        matches!(&items[0], MenuNode::Disabled { label, shortcut: Some(_), message: Some(Message::Undo) } if label == "Undo")
    );
    assert!(matches!(&items[1], MenuNode::Disabled { label, .. } if label == "Redo"));
    assert!(items.iter().any(|entry| matches!(
        entry,
        MenuNode::Item {
            message: Message::Paste,
            ..
        }
    )));
    assert!(!items.iter().any(|entry| matches!(
        entry,
        MenuNode::Item {
            message: Message::Copy,
            ..
        }
    )));
}
