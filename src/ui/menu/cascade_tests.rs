use super::*;
use iced::advanced::Renderer as _;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::Headless;
use iced::{Color, Renderer, Theme, window};

const SIZE: Size = Size::new(440.0, 280.0);

fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn cascade(renderer: &Renderer) -> (MenuCascade<'static>, Tree, layout::Node) {
    let ancestors = (0..8)
        .map(|index| {
            let label = format!("Ancestor command {index}");
            item(label.clone(), Message::LanguageSelected(label))
        })
        .collect();
    let middle = (0..4)
        .map(|index| item(format!("Middle command {index}"), Message::None))
        .collect();
    let deepest = vec![
        disabled("Unavailable command"),
        separator(),
        item(
            "Use this command",
            Message::LanguageSelected("deepest".into()),
        ),
    ];
    let mut menu = MenuCascade::new(
        menu_panel(ancestors, 200.0, None, &[], 0),
        vec![
            PositionedLayer::new(menu_panel(middle, 200.0, None, &[], 1), 0.0),
            PositionedLayer::new(menu_panel(deepest, 200.0, None, &[], 2), ROW_HEIGHT),
        ],
    );
    let mut tree = Tree::empty();
    menu.diff(&mut tree);
    let node = menu.layout(&mut tree, renderer, &layout::Limits::new(Size::ZERO, SIZE));
    let layout = Layout::new(&node);
    assert_eq!(layout.child(1).bounds().x, 200.0);
    assert_eq!(layout.child(2).bounds().x, 0.0);
    assert!(
        layout
            .child(0)
            .bounds()
            .intersection(&layout.child(2).bounds())
            .is_some()
    );
    (menu, tree, node)
}

#[test]
fn flipped_submenu_covers_ancestor_text_in_both_themes() {
    let _guard = crate::font_system_test_guard();
    let mut renderer = renderer();
    let (menu, tree, node) = cascade(&renderer);
    let viewport = Rectangle::with_size(SIZE);
    let layout = Layout::new(&node);
    let deepest = layout.child(2).bounds();
    for theme in [Theme::Light, Theme::Dark] {
        renderer.reset(viewport);
        menu.draw(
            &tree,
            &mut renderer,
            &theme,
            &renderer::Style::default(),
            layout,
            mouse::Cursor::Unavailable,
            &viewport,
        );
        let combined = renderer.screenshot(Size::new(440, 280), 1.0, Color::TRANSPARENT);
        renderer.reset(viewport);
        menu.layers[1].element.as_widget().draw(
            &tree.children[2],
            &mut renderer,
            &theme,
            &renderer::Style::default(),
            layout.child(2),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        let alone = renderer.screenshot(Size::new(440, 280), 1.0, Color::TRANSPARENT);
        // Compare the opaque interior, excluding the rounded edge and shadow.
        for y in deepest.y as usize + 10..(deepest.y + deepest.height) as usize - 10 {
            let start = (y * 440 + deepest.x as usize + 10) * 4;
            let end = (y * 440 + (deepest.x + deepest.width) as usize - 10) * 4;
            assert_eq!(
                &combined[start..end],
                &alone[start..end],
                "ancestor content must stay behind the deepest panel in {theme:?}, row {y}"
            );
        }
    }
}

fn send(
    menu: &mut MenuCascade<'_>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    event: mouse::Event,
    point: Point,
) -> (Vec<Message>, bool) {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    menu.update(
        tree,
        &Event::Mouse(event),
        Layout::new(node),
        mouse::Cursor::Available(point),
        renderer,
        &mut shell,
        &Rectangle::with_size(SIZE),
    );
    let captured = shell.is_event_captured();
    (messages, captured)
}

#[test]
fn flipped_submenu_blocks_pointer_events_to_covered_ancestor_rows() {
    let _guard = crate::font_system_test_guard();
    let renderer = renderer();
    let (mut menu, mut tree, node) = cascade(&renderer);
    let deepest = Layout::new(&node).child(2).bounds();
    for y in [
        deepest.y + f32::from(PANEL_PADDING) + ROW_HEIGHT / 2.0,
        deepest.y + f32::from(PANEL_PADDING) + ROW_HEIGHT + SEPARATOR_HEIGHT / 2.0,
        deepest.y + 2.0,
    ] {
        let point = Point::new(deepest.x + 40.0, y);
        let mut messages = Vec::new();
        for event in [
            mouse::Event::CursorMoved { position: point },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            let pressed = matches!(&event, mouse::Event::ButtonPressed(_));
            let (sent, captured) = send(&mut menu, &mut tree, &node, &renderer, event, point);
            if pressed {
                assert!(captured, "presses inside the menu must not dismiss it");
            }
            messages.extend(sent);
        }
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::LanguageSelected(_)))
        );
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::MenuPathHovered(path) if path.depth < 2))
        );
        assert_eq!(
            menu.mouse_interaction(
                &tree,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &Rectangle::with_size(SIZE),
                &renderer,
            ),
            mouse::Interaction::Idle,
            "covered ancestor buttons must not change the pointer"
        );
    }

    let point = Point::new(
        deepest.x + 40.0,
        deepest.y + f32::from(PANEL_PADDING) + ROW_HEIGHT + SEPARATOR_HEIGHT + ROW_HEIGHT / 2.0,
    );
    let mut messages = Vec::new();
    for event in [
        mouse::Event::CursorMoved { position: point },
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        messages.extend(send(&mut menu, &mut tree, &node, &renderer, event, point).0);
    }
    let commands = messages
        .iter()
        .filter_map(|message| match message {
            Message::LanguageSelected(command) => Some(command.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(commands, ["deepest"]);
}
