use super::*;
use crate::editor::{EditorPosition, EditorSelection};
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::overlay::Overlay;
use iced::advanced::renderer::Headless;

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

fn dispatch(
    menu: &mut ContextMenu<'_>,
    renderer: &Renderer,
    viewport: Size,
    event: Event,
    cursor: mouse::Cursor,
) -> Vec<Message> {
    let node = menu.layout(renderer, viewport);
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    menu.update(&event, Layout::new(&node), cursor, renderer, &mut shell);
    assert!(shell.is_event_captured());
    menu.layout(renderer, viewport);
    messages
}

#[test]
fn keyboard_navigation_skips_unavailable_commands_and_dispatches_submenu_command() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let entries = vec![
        menu::disabled("Undo"),
        menu::separator(),
        menu::item("Copy", Message::Copy),
        menu::submenu(
            "case",
            "Convert",
            vec![
                menu::disabled("Unavailable"),
                menu::item("Uppercase", Message::Uppercase),
            ],
        ),
    ];
    let mut state = State::default();
    state.open(Point::ORIGIN, &entries, true);
    let mut menu = ContextMenu {
        state: &mut state,
        entries,
        settings: &settings,
        anchor: Point::new(50.0, 50.0),
    };
    let viewport = Size::new(800.0, 600.0);
    assert_eq!(menu.state.highlighted[0], Some(2));
    for named in [Named::ArrowDown, Named::ArrowRight] {
        assert!(
            dispatch(
                &mut menu,
                &renderer,
                viewport,
                key(named),
                mouse::Cursor::Unavailable
            )
            .is_empty()
        );
    }
    assert_eq!(menu.state.path, vec![3]);
    assert_eq!(menu.state.highlighted[1], Some(1));
    let messages = dispatch(
        &mut menu,
        &renderer,
        viewport,
        key(Named::Enter),
        mouse::Cursor::Unavailable,
    );
    assert!(matches!(messages.as_slice(), [Message::Uppercase]));
    assert!(menu.state.anchor.is_none());
}

#[test]
fn modified_navigation_shortcuts_dispatch_without_changing_plain_menu_controls() {
    let renderer = renderer();
    let mut settings = EditorSettings::default();
    settings
        .shortcuts
        .set_binding(
            crate::core::ShortcutCommand::NewFile,
            crate::core::KeyBinding::from_event(
                &Key::Named(Named::Home),
                keyboard::Modifiers::CTRL,
            )
            .unwrap(),
        )
        .unwrap();
    let entries = vec![
        menu::item("Copy", Message::Copy),
        menu::item("Paste", Message::Paste),
    ];
    let mut state = State::default();
    let viewport = Size::new(800.0, 600.0);
    let mut menu = ContextMenu {
        state: &mut state,
        entries,
        settings: &settings,
        anchor: Point::new(50.0, 50.0),
    };
    for (named, modifiers, command) in [
        (
            Named::ArrowDown,
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::ALT,
            crate::core::ShortcutCommand::AddCaretBelow,
        ),
        (
            Named::ArrowUp,
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::ALT,
            crate::core::ShortcutCommand::AddCaretAbove,
        ),
        (
            Named::Home,
            keyboard::Modifiers::CTRL,
            crate::core::ShortcutCommand::NewFile,
        ),
    ] {
        menu.state.open(Point::ORIGIN, &menu.entries, true);
        let mut event = key(named);
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            modifiers: actual, ..
        }) = &mut event
        {
            *actual = modifiers;
        }
        let messages = dispatch(
            &mut menu,
            &renderer,
            viewport,
            event,
            mouse::Cursor::Unavailable,
        );
        assert!(matches!(messages.as_slice(), [Message::Shortcut(actual)] if *actual == command));
        assert!(menu.state.anchor.is_none());
    }

    menu.state.open(Point::ORIGIN, &menu.entries, true);
    assert!(
        dispatch(
            &mut menu,
            &renderer,
            viewport,
            key(Named::ArrowDown),
            mouse::Cursor::Unavailable,
        )
        .is_empty()
    );
    assert_eq!(menu.state.highlighted[0], Some(1));
    let mut event = key(Named::Tab);
    if let Event::Keyboard(keyboard::Event::KeyPressed { modifiers, .. }) = &mut event {
        *modifiers = keyboard::Modifiers::SHIFT;
    }
    assert!(
        dispatch(
            &mut menu,
            &renderer,
            viewport,
            event,
            mouse::Cursor::Unavailable,
        )
        .is_empty()
    );
    assert!(menu.state.anchor.is_none());
}

#[test]
fn menus_flip_and_fit_near_window_edges_and_keyboard_scroll_reveals_items() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let entries = vec![menu::submenu(
        "lines",
        "Lines",
        (0..30)
            .map(|n| menu::item(format!("Line {n}"), Message::Copy))
            .collect(),
    )];
    let mut state = State::default();
    state.open(Point::ORIGIN, &entries, true);
    let viewport = Size::new(800.0, 240.0);
    let mut menu = ContextMenu {
        state: &mut state,
        entries,
        settings: &settings,
        anchor: Point::new(790.0, 230.0),
    };
    dispatch(
        &mut menu,
        &renderer,
        viewport,
        key(Named::ArrowRight),
        mouse::Cursor::Unavailable,
    );
    dispatch(
        &mut menu,
        &renderer,
        viewport,
        key(Named::End),
        mouse::Cursor::Unavailable,
    );
    let node = menu.layout(&renderer, viewport);
    let root = node.children()[0].bounds();
    let child = node.children()[1].bounds();
    assert!(child.x < root.x);
    for panel in node.children() {
        assert!(panel.bounds().x >= 0.0 && panel.bounds().y >= 0.0);
        assert!(panel.bounds().x + panel.bounds().width <= viewport.width);
        assert!(panel.bounds().y + panel.bounds().height <= viewport.height);
    }
    assert_eq!(menu.state.highlighted[1], Some(29));
    assert!(menu.state.offsets[1] > 0.0);
    let point = Point::new(child.x + 20.0, child.y + child.height - PADDING - 2.0);
    assert_eq!(menu.hit(Layout::new(&node), point), Some((1, 29)));
}

#[test]
fn disabled_click_and_dismissal_never_dispatch_an_editor_command() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let entries = vec![menu::disabled("Undo"), menu::item("Copy", Message::Copy)];
    let mut state = State::default();
    state.open(Point::ORIGIN, &entries, false);
    let viewport = Size::new(800.0, 600.0);
    let mut menu = ContextMenu {
        state: &mut state,
        entries,
        settings: &settings,
        anchor: Point::new(50.0, 50.0),
    };
    let click = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    assert!(
        dispatch(
            &mut menu,
            &renderer,
            viewport,
            click.clone(),
            mouse::Cursor::Available(Point::new(80.0, 70.0))
        )
        .is_empty()
    );
    assert!(menu.state.anchor.is_some());
    assert!(
        dispatch(
            &mut menu,
            &renderer,
            viewport,
            click,
            mouse::Cursor::Available(Point::new(700.0, 500.0))
        )
        .is_empty()
    );
    assert!(menu.state.anchor.is_none());
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
        let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
        overlay.as_overlay_mut().update(
            &key(Named::Escape),
            Layout::new(&menu_node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut shell,
        );
    }
    assert!(tree.state.downcast_ref::<State>().anchor.is_none());
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

#[test]
fn dismissing_or_switching_submenus_tolerates_cursor_queries_with_previous_layout() {
    let mut renderer = renderer();
    let settings = EditorSettings::default();
    let items = vec![
        menu::submenu("lines", "Lines", vec![menu::item("Copy", Message::Copy)]),
        menu::item("Paste", Message::Paste),
    ];
    let mut state = State::default();
    state.open(Point::ORIGIN, &items, true);
    let viewport = Size::new(800.0, 600.0);
    let mut menu = ContextMenu {
        state: &mut state,
        entries: items,
        settings: &settings,
        anchor: Point::new(100.0, 100.0),
    };
    dispatch(
        &mut menu,
        &renderer,
        viewport,
        key(Named::ArrowRight),
        mouse::Cursor::Unavailable,
    );
    let old_node = menu.layout(&renderer, viewport);
    let cursor = mouse::Cursor::Available(old_node.children()[1].bounds().center());
    menu.state.highlight(0, 1);
    menu.mouse_interaction(Layout::new(&old_node), cursor, &renderer);
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    menu.update(
        &Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        }),
        Layout::new(&old_node),
        cursor,
        &renderer,
        &mut shell,
    );
    menu.state.close();
    assert_eq!(
        menu.mouse_interaction(Layout::new(&old_node), cursor, &renderer),
        mouse::Interaction::Idle
    );
    menu.draw(
        &mut renderer,
        &Theme::Light,
        &renderer::Style::default(),
        Layout::new(&old_node),
        cursor,
    );
    assert_eq!(
        menu.hit(
            Layout::new(&old_node),
            old_node.children()[1].bounds().center()
        ),
        None
    );
}

#[test]
fn wheel_scroll_animates_and_keyboard_navigation_cancels_pending_motion() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let entries = (0..40)
        .map(|_| menu::item("Copy", Message::Copy))
        .collect::<Vec<_>>();
    let mut state = State::default();
    state.open(Point::new(10.0, 10.0), &entries, true);
    let mut menu = ContextMenu {
        state: &mut state,
        entries,
        settings: &settings,
        anchor: Point::new(10.0, 10.0),
    };
    let viewport = Size::new(400.0, 250.0);
    let cursor = mouse::Cursor::Available(Point::new(80.0, 80.0));
    let wheel = || {
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        })
    };
    for _ in 0..3 {
        let _ = dispatch(&mut menu, &renderer, viewport, wheel(), cursor);
        assert_eq!(menu.state.offsets[0], 0.0);
        assert!(menu.state.scroll_motion.is_some());
    }
    let started = menu.state.scroll_motion.as_ref().unwrap().started;
    let tick = |menu: &mut ContextMenu<'_>, elapsed| {
        let node = menu.layout(&renderer, viewport);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
        menu.update(
            &Event::Window(iced::window::Event::RedrawRequested(
                started + std::time::Duration::from_millis(elapsed),
            )),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut shell,
        );
        assert!(messages.is_empty());
    };
    tick(&mut menu, 50);
    assert!(menu.state.offsets[0] > 0.0 && menu.state.offsets[0] < ROW_HEIGHT * 9.0);
    tick(&mut menu, 150);
    assert_eq!(menu.state.offsets[0], ROW_HEIGHT * 9.0);
    assert!(menu.state.scroll_motion.is_none());
    // Begin another gesture after the synthetic animation's completion.
    menu.state.wheel_input = iced::widget::scrollable::WheelScrollInput::default();
    let _ = dispatch(&mut menu, &renderer, viewport, wheel(), cursor);
    assert!(menu.state.scroll_motion.is_some());
    let _ = dispatch(&mut menu, &renderer, viewport, key(Named::Home), cursor);
    assert_eq!(menu.state.offsets[0], 0.0);
    assert!(menu.state.scroll_motion.is_none());
    tick(&mut menu, 500);
    assert_eq!(menu.state.offsets[0], 0.0);
    let _ = dispatch(
        &mut menu,
        &renderer,
        viewport,
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -12.0 },
        }),
        cursor,
    );
    assert_eq!(menu.state.offsets[0], 12.0);
    assert!(menu.state.scroll_motion.is_none());
}

#[test]
fn context_menu_touchpad_line_stream_is_direct_and_preserves_whole_packets() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let entries = (0..40)
        .map(|_| menu::item("Copy", Message::Copy))
        .collect::<Vec<_>>();
    let mut state = State::default();
    state.open(Point::new(10.0, 10.0), &entries, true);
    let mut menu = ContextMenu {
        state: &mut state,
        entries,
        settings: &settings,
        anchor: Point::new(10.0, 10.0),
    };
    let viewport = Size::new(400.0, 250.0);
    let cursor = mouse::Cursor::Available(Point::new(80.0, 80.0));
    let mut expected = 0.0;
    for delta in [-0.125, -0.25, -1.0, -0.125, 0.25] {
        let _ = dispatch(
            &mut menu,
            &renderer,
            viewport,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: delta },
            }),
            cursor,
        );
        expected -= delta * ROW_HEIGHT * 3.0;
        assert_eq!(menu.state.offsets[0], expected);
        assert!(menu.state.scroll_motion.is_none());
    }
    // Precise input following an ambiguous whole-line event must take over
    // from the displayed position without jumping to the unfinished target.
    menu.state.open(Point::new(10.0, 10.0), &menu.entries, true);
    for delta in [-1.0, -0.25] {
        let _ = dispatch(
            &mut menu,
            &renderer,
            viewport,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: delta },
            }),
            cursor,
        );
    }
    assert_eq!(menu.state.offsets[0], 0.25 * ROW_HEIGHT * 3.0);
    assert!(menu.state.scroll_motion.is_none());
}
