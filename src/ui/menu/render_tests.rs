use super::*;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::Headless;
use iced::advanced::widget::{self, operation};
use iced::{Renderer, Vector, window};

const VIEWPORT: Size = Size::new(320.0, 150.0);

fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

#[derive(Default)]
struct MenuRows {
    scroll_bounds: Rectangle,
    content_bounds: Rectangle,
    scroll_y: f32,
    labels: Vec<(String, Rectangle)>,
}

impl Operation for MenuRows {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        _id: Option<&widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        self.scroll_bounds = bounds;
        self.content_bounds = content;
        self.scroll_y = translation.y;
    }

    fn text(&mut self, _id: Option<&widget::Id>, mut bounds: Rectangle, text: &str) {
        bounds.y -= self.scroll_y;
        self.labels.push((text.to_owned(), bounds));
    }
}

fn rows(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
) -> MenuRows {
    let mut rows = MenuRows::default();
    element
        .as_widget_mut()
        .operate(tree, Layout::new(node), renderer, &mut rows);
    rows
}

fn send(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    event: Event,
    cursor: mouse::Cursor,
) -> Vec<Message> {
    let mut messages = Vec::new();
    element.as_widget_mut().update(
        tree,
        &event,
        Layout::new(node),
        cursor,
        renderer,
        &mut Shell::new(&window::Headless, Waker::noop(), &mut messages),
        &Rectangle::with_size(VIEWPORT),
    );
    messages
}

fn pointer_event(pressed: bool) -> Event {
    Event::Mouse(if pressed {
        mouse::Event::ButtonPressed(mouse::Button::Left)
    } else {
        mouse::Event::ButtonReleased(mouse::Button::Left)
    })
}

fn mount(element: &mut Element<'_, Message>, renderer: &Renderer) -> (Tree, layout::Node) {
    let mut tree = Tree::empty();
    tree.diff(element.as_widget_mut());
    let limits = layout::Limits::new(Size::ZERO, VIEWPORT);
    let node = element.as_widget_mut().layout(&mut tree, renderer, &limits);
    send(
        element,
        &mut tree,
        &node,
        renderer,
        Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
        mouse::Cursor::Unavailable,
    );
    let node = element.as_widget_mut().layout(&mut tree, renderer, &limits);
    (tree, node)
}

#[test]
fn long_menu_scrolls_by_wheel_and_thumb_and_keeps_last_command_clickable() {
    let _guard = crate::font_system_test_guard();
    let renderer = renderer();
    let entries = (0..40)
        .map(|index| {
            let label = format!("Language {index}");
            item(label.clone(), Message::LanguageSelected(label))
        })
        .collect();
    let mut element = menu_panel(entries, 240.0, Some(360.0), &[], 0);
    let (mut tree, node) = mount(&mut element, &renderer);
    assert!(node.size().height <= VIEWPORT.height);
    let initial = rows(&mut element, &mut tree, &node, &renderer);
    assert!(initial.content_bounds.height > initial.scroll_bounds.height);
    send(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::WheelScrolled {
            phase: mouse::ScrollPhase::Moved,
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 },
        }),
        mouse::Cursor::Available(initial.scroll_bounds.center()),
    );
    send(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            std::time::Instant::now() + std::time::Duration::from_millis(250),
        )),
        mouse::Cursor::Unavailable,
    );
    let scrolled = rows(&mut element, &mut tree, &node, &renderer);
    let max_scroll = scrolled.content_bounds.height - scrolled.scroll_bounds.height;
    assert!(scrolled.scroll_y > 0.0 && scrolled.scroll_y < max_scroll);

    let ratio = scrolled.scroll_bounds.height / scrolled.content_bounds.height;
    let thumb_height = (scrolled.scroll_bounds.height * ratio).max(2.0);
    let thumb = Point::new(
        scrolled.scroll_bounds.x + scrolled.scroll_bounds.width - 5.0,
        scrolled.scroll_bounds.y + scrolled.scroll_y * ratio + thumb_height / 2.0,
    );
    let bottom = Point::new(
        thumb.x,
        scrolled.scroll_bounds.y + scrolled.scroll_bounds.height - 1.0,
    );
    for (event, point) in [
        (pointer_event(true), thumb),
        (
            Event::Mouse(mouse::Event::CursorMoved { position: bottom }),
            bottom,
        ),
        (pointer_event(false), bottom),
    ] {
        let messages = send(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            event,
            mouse::Cursor::Available(point),
        );
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::LanguageSelected(_)))
        );
    }
    let bottom_rows = rows(&mut element, &mut tree, &node, &renderer);
    assert!((bottom_rows.scroll_y - max_scroll).abs() < 1.0);
    let last = bottom_rows
        .labels
        .iter()
        .find(|(label, _)| label == "Language 39")
        .unwrap()
        .1;
    assert!(bottom_rows.scroll_bounds.contains(last.center()));
    let mut messages = Vec::new();
    for pressed in [true, false] {
        messages.extend(send(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            pointer_event(pressed),
            mouse::Cursor::Available(last.center()),
        ));
    }
    let selected_count = messages
        .iter()
        .filter(
            |message| matches!(message, Message::LanguageSelected(label) if label == "Language 39"),
        )
        .count();
    assert_eq!(selected_count, 1);

    let rail = Point::new(thumb.x, bottom_rows.scroll_bounds.y + 1.0);
    for pressed in [true, false] {
        let messages = send(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            pointer_event(pressed),
            mouse::Cursor::Available(rail),
        );
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::LanguageSelected(_)))
        );
    }
    assert!(rows(&mut element, &mut tree, &node, &renderer).scroll_y < max_scroll);
}

#[test]
fn short_menu_uses_only_the_space_needed_for_its_rows() {
    let _guard = crate::font_system_test_guard();
    let renderer = renderer();
    let mut element = menu_panel(
        vec![
            item("Open", Message::OpenFile),
            item("Save", Message::SaveFile),
        ],
        240.0,
        Some(360.0),
        &[],
        0,
    );
    let (mut tree, node) = mount(&mut element, &renderer);
    assert!(node.size().height <= ROW_HEIGHT * 2.0 + f32::from(PANEL_PADDING) * 2.0);
    let rows = rows(&mut element, &mut tree, &node, &renderer);
    assert_eq!(rows.scroll_y, 0.0);
    for (_, bounds) in rows.labels {
        assert!(node.bounds().contains(bounds.center()));
    }
}
