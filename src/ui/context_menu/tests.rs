use super::*;
use crate::core::DocumentId;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::overlay::Overlay;
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

fn redraw(
    menu: &mut ContextMenu<'_>,
    renderer: &Renderer,
    viewport: Size,
    now: std::time::Instant,
) -> iced::window::RedrawRequest {
    let node = menu.layout(renderer, viewport);
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    menu.update(
        &Event::Window(iced::window::Event::RedrawRequested(now)),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        renderer,
        &mut shell,
    );
    assert!(
        !shell.is_event_captured(),
        "redraw capture hides Iced overlays"
    );
    assert!(shell.is_empty());
    shell.redraw_request()
}

fn settle(menu: &mut ContextMenu<'_>, renderer: &Renderer, viewport: Size) {
    let now = std::time::Instant::now();
    redraw(menu, renderer, viewport, now);
    redraw(
        menu,
        renderer,
        viewport,
        now + std::time::Duration::from_millis(250),
    );
}

fn finish_closing(menu: &mut ContextMenu<'_>, renderer: &Renderer, viewport: Size) {
    assert!(menu.state.is_open());
    assert!(menu.state.is_closing());
    settle(menu, renderer, viewport);
    assert!(!menu.state.is_open());
}

fn pixel(
    menu: &mut ContextMenu<'_>,
    renderer: &mut Renderer,
    viewport: Size,
    point: Point,
) -> [u8; 4] {
    let node = menu.layout(renderer, viewport);
    renderer.reset(Rectangle::with_size(viewport));
    menu.draw(
        renderer,
        &Theme::Dark,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
    );
    let size = Size::new(viewport.width as u32, viewport.height as u32);
    let pixels = renderer.screenshot(size, 1.0, Color::TRANSPARENT);
    let offset = (point.y as usize * size.width as usize + point.x as usize) * 4;
    pixels[offset..offset + 4].try_into().unwrap()
}

#[test]
fn context_menu_entry_animates_pixels_and_bounds_and_submenus_enter_independently() {
    let _guard = crate::font_system_test_guard();
    let mut renderer = renderer();
    let settings = EditorSettings::default();
    let entries = vec![menu::submenu(
        "convert",
        "Convert",
        vec![menu::item("Copy", Message::Copy)],
    )];
    let mut state = State::default();
    state.open(Point::new(30.0, 40.0), &entries, true);
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(30.0, 40.0));
    let viewport = Size::new(700.0, 220.0);
    let node = menu.layout(&renderer, viewport);
    let final_bounds = node.children()[0].bounds();
    let initial_bounds = menu.panel_bounds(0, Layout::new(&node).child(0));
    let sample_point = Point::new(final_bounds.x + 9.0, final_bounds.y + 15.0);
    let entrance_hit = Point::new(initial_bounds.x + 20.0, initial_bounds.y + PADDING + 1.0);
    assert_eq!(menu.state.panel_progress(0), 0.0);
    assert_ne!(initial_bounds.y, final_bounds.y);
    assert_eq!(menu.hit(Layout::new(&node), entrance_hit), Some((0, 0)));
    assert_eq!(
        pixel(&mut menu, &mut renderer, viewport, sample_point)[3],
        0
    );
    let now = std::time::Instant::now();
    assert_ne!(
        redraw(&mut menu, &renderer, viewport, now),
        iced::window::RedrawRequest::Wait
    );
    redraw(
        &mut menu,
        &renderer,
        viewport,
        now + std::time::Duration::from_millis(75),
    );
    let mid_bounds = menu.panel_bounds(0, Layout::new(&node).child(0));
    let alpha = pixel(&mut menu, &mut renderer, viewport, sample_point)[3];
    assert!((0.0..1.0).contains(&menu.state.panel_progress(0)));
    assert!(alpha > 0 && alpha < 255);
    assert!((mid_bounds.y - final_bounds.y).abs() < (initial_bounds.y - final_bounds.y).abs());
    assert_eq!(
        redraw(
            &mut menu,
            &renderer,
            viewport,
            now + std::time::Duration::from_millis(200)
        ),
        iced::window::RedrawRequest::Wait
    );
    assert_eq!(menu.state.panel_progress(0), 1.0);
    assert_eq!(
        menu.panel_bounds(0, Layout::new(&node).child(0)).y,
        final_bounds.y
    );
    assert_eq!(
        pixel(&mut menu, &mut renderer, viewport, sample_point)[3],
        255
    );
    assert_eq!(menu.hit(Layout::new(&node), entrance_hit), None);

    dispatch(
        &mut menu,
        &renderer,
        viewport,
        key(Named::ArrowRight),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(menu.state.panel_progress(0), 1.0);
    assert_eq!(menu.state.panel_progress(1), 0.0);
    let child_start = now + std::time::Duration::from_millis(300);
    redraw(&mut menu, &renderer, viewport, child_start);
    redraw(
        &mut menu,
        &renderer,
        viewport,
        child_start + std::time::Duration::from_millis(75),
    );
    assert_eq!(menu.state.panel_progress(0), 1.0);
    assert!((0.0..1.0).contains(&menu.state.panel_progress(1)));
    assert_eq!(
        redraw(
            &mut menu,
            &renderer,
            viewport,
            child_start + std::time::Duration::from_millis(200)
        ),
        iced::window::RedrawRequest::Wait
    );
    assert_eq!(menu.state.panel_progress(1), 1.0);
    assert_eq!(
        redraw(
            &mut menu,
            &renderer,
            viewport,
            child_start + std::time::Duration::from_millis(300)
        ),
        iced::window::RedrawRequest::Wait
    );
}

#[test]
fn context_menu_exit_retains_panels_blocks_duplicate_actions_and_hard_close_cancels_motion() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let entries = vec![menu::submenu(
        "convert",
        "Convert",
        vec![menu::item("Copy", Message::Copy)],
    )];
    let mut state = State::default();
    state.open(Point::ORIGIN, &entries, true);
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(30.0, 40.0));
    let viewport = Size::new(700.0, 220.0);
    settle(&mut menu, &renderer, viewport);
    dispatch(
        &mut menu,
        &renderer,
        viewport,
        key(Named::ArrowRight),
        mouse::Cursor::Unavailable,
    );
    let entered = std::time::Instant::now();
    redraw(&mut menu, &renderer, viewport, entered);
    redraw(
        &mut menu,
        &renderer,
        viewport,
        entered + std::time::Duration::from_millis(75),
    );
    let child_progress = menu.state.panel_progress(1);
    assert!((0.0..1.0).contains(&child_progress));
    assert!(matches!(
        dispatch(
            &mut menu,
            &renderer,
            viewport,
            key(Named::Enter),
            mouse::Cursor::Unavailable
        )
        .as_slice(),
        [Message::Copy]
    ));
    assert!(menu.state.is_open() && menu.state.is_closing());
    assert_eq!(menu.state.path, vec![0]);
    assert!(
        dispatch(
            &mut menu,
            &renderer,
            viewport,
            key(Named::Enter),
            mouse::Cursor::Unavailable
        )
        .is_empty()
    );
    let now = std::time::Instant::now();
    redraw(&mut menu, &renderer, viewport, now);
    assert_ne!(
        redraw(
            &mut menu,
            &renderer,
            viewport,
            now + std::time::Duration::from_millis(50)
        ),
        iced::window::RedrawRequest::Wait
    );
    assert!(menu.state.is_open());
    assert!((0.0..1.0).contains(&menu.state.panel_progress(0)));
    assert!(
        menu.state.panel_progress(1) < child_progress,
        "entering flyouts also fade out after dismissal"
    );
    assert_eq!(
        redraw(
            &mut menu,
            &renderer,
            viewport,
            now + std::time::Duration::from_millis(200)
        ),
        iced::window::RedrawRequest::Wait
    );
    assert!(!menu.state.is_open());
    assert!(menu.state.path.is_empty());
    assert_eq!(
        redraw(
            &mut menu,
            &renderer,
            viewport,
            now + std::time::Duration::from_millis(300)
        ),
        iced::window::RedrawRequest::Wait
    );

    menu.state.open(Point::ORIGIN, &menu.entries, true);
    redraw(&mut menu, &renderer, viewport, now);
    menu.state.close();
    assert!(!menu.state.is_open());
    assert_eq!(
        redraw(
            &mut menu,
            &renderer,
            viewport,
            now + std::time::Duration::from_millis(500)
        ),
        iced::window::RedrawRequest::Wait
    );
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
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(50.0, 50.0));
    let viewport = Size::new(800.0, 600.0);
    settle(&mut menu, &renderer, viewport);
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
    finish_closing(&mut menu, &renderer, viewport);
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
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(50.0, 50.0));
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
        settle(&mut menu, &renderer, viewport);
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
        finish_closing(&mut menu, &renderer, viewport);
    }

    menu.state.open(Point::ORIGIN, &menu.entries, true);
    settle(&mut menu, &renderer, viewport);
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
    finish_closing(&mut menu, &renderer, viewport);
}

#[test]
fn shortcut_overrides_keep_commands_addressed_to_the_context_target() {
    let renderer = renderer();
    let target = DocumentId::new(103);
    let viewport = Size::new(800.0, 600.0);
    for modifiers in [keyboard::Modifiers::CTRL, keyboard::Modifiers::empty()] {
        let mut settings = EditorSettings::default();
        settings
            .shortcuts
            .set_binding(
                crate::core::ShortcutCommand::NewFile,
                crate::core::KeyBinding::from_event(&Key::Named(Named::F6), modifiers).unwrap(),
            )
            .unwrap();
        let entries = vec![menu::item("Copy", Message::Copy)];
        let mut state = State::default();
        state.open(Point::ORIGIN, &entries, true);
        let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(50.0, 50.0))
            .with_shortcut_override(
                crate::core::ShortcutCommand::NewFile,
                Message::TabContextAction(target, crate::message::TabContextAction::Close),
            );
        settle(&mut menu, &renderer, viewport);
        let mut event = key(Named::F6);
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
        assert!(matches!(
            messages.as_slice(),
            [Message::TabContextAction(actual, crate::message::TabContextAction::Close)]
                if *actual == target
        ));
        finish_closing(&mut menu, &renderer, viewport);
    }
}

#[test]
fn modifier_changes_propagate_without_dismissing_the_menu() {
    let renderer = renderer();
    let settings = EditorSettings::default();
    let entries = vec![menu::item("Copy", Message::Copy)];
    let mut state = State::default();
    state.open(Point::ORIGIN, &entries, true);
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(50.0, 50.0));
    let node = menu.layout(&renderer, Size::new(800.0, 600.0));
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    menu.update(
        &Event::Keyboard(keyboard::Event::ModifiersChanged(
            keyboard::Modifiers::empty(),
        )),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut shell,
    );
    assert!(!shell.is_event_captured());
    assert!(shell.is_empty());
    assert!(menu.state.anchor.is_some());
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
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(790.0, 230.0));
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
    settle(&mut menu, &renderer, viewport);
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
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(50.0, 50.0));
    settle(&mut menu, &renderer, viewport);
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
    finish_closing(&mut menu, &renderer, viewport);
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
    let mut menu = ContextMenu::new(&mut state, items, &settings, Point::new(100.0, 100.0));
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
            phase: mouse::ScrollPhase::Moved,
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
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(10.0, 10.0));
    let viewport = Size::new(400.0, 250.0);
    let cursor = mouse::Cursor::Available(Point::new(80.0, 80.0));
    let wheel = || {
        Event::Mouse(mouse::Event::WheelScrolled {
            phase: mouse::ScrollPhase::Moved,
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
    menu.state.wheel_input = mouse::WheelScrollInput::default();
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
            phase: mouse::ScrollPhase::Moved,
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
    let mut menu = ContextMenu::new(&mut state, entries, &settings, Point::new(10.0, 10.0));
    let viewport = Size::new(400.0, 250.0);
    let cursor = mouse::Cursor::Available(Point::new(80.0, 80.0));
    let mut expected = 0.0;
    for delta in [-0.125, -0.25, -1.0, -0.125, 0.25] {
        let _ = dispatch(
            &mut menu,
            &renderer,
            viewport,
            Event::Mouse(mouse::Event::WheelScrolled {
                phase: mouse::ScrollPhase::Moved,
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
                phase: mouse::ScrollPhase::Moved,
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: delta },
            }),
            cursor,
        );
    }
    assert_eq!(menu.state.offsets[0], 0.25 * ROW_HEIGHT * 3.0);
    assert!(menu.state.scroll_motion.is_none());
}
