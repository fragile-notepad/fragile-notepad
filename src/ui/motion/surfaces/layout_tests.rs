//! Child layout reuse and invalidation during locally timed motion.

use super::*;
use crate::ui::motion::test_support::*;
use iced::advanced::widget::operation::focusable;
use iced::widget::{Space, button};
use iced::{Fill, Point};
use std::cell::Cell;
use std::rc::Rc;

struct LayoutCounter {
    content: Element<'static, Message>,
    calls: Rc<Cell<usize>>,
    invalidate_next_frame: Rc<Cell<bool>>,
}

impl Widget<Message, Theme, Renderer> for LayoutCounter {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.calls.set(self.calls.get() + 1);
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if matches!(event, Event::Window(window::Event::RedrawRequested(_)))
            && self.invalidate_next_frame.replace(false)
        {
            shell.invalidate_layout();
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

fn measured_child(
    calls: &Rc<Cell<usize>>,
    invalidate_next_frame: &Rc<Cell<bool>>,
) -> Element<'static, Message> {
    Element::new(LayoutCounter {
        content: button(Space::new().width(Fill).height(40))
            .padding(0)
            .width(Fill)
            .on_press(Message::None)
            .into(),
        calls: calls.clone(),
        invalidate_next_frame: invalidate_next_frame.clone(),
    })
}

#[test]
fn local_entrance_reuses_child_layout_while_visible_and_click_bounds_move() {
    let _font_guard = crate::font_system_test_guard();
    let renderer = renderer();
    let calls = Rc::new(Cell::new(0));
    let invalidate = Rc::new(Cell::new(false));
    let mut content = popup(measured_child(&calls, &invalidate));
    let (mut tree, mut node) = mount(&mut content, &renderer);
    let started = Instant::now();
    let mut previous_y = Layout::new(&node).child(0).bounds().y;
    assert_eq!(previous_y, 8.0);
    for index in 0..=10 {
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                started + ENTRANCE_DURATION * index / 10,
            )),
            mouse::Cursor::Unavailable,
        );
        node = relayout(&mut content, &mut tree, &renderer);
        let y = Layout::new(&node).child(0).bounds().y;
        assert!(y <= previous_y);
        previous_y = y;
    }
    assert_eq!(previous_y, 0.0);
    assert_eq!(calls.get(), 1, "twelve child layouts become one");

    let point = Point::new(20.0, 20.0);
    let mut messages = Vec::new();
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        messages.extend(
            dispatch(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(event),
                mouse::Cursor::Available(point),
            )
            .1,
        );
    }
    assert!(matches!(messages.as_slice(), [Message::None]));
}

#[test]
fn local_disclosure_reuses_intrinsic_child_layout_while_height_changes() {
    let _font_guard = crate::font_system_test_guard();
    let renderer = renderer();
    let calls = Rc::new(Cell::new(0));
    let invalidate = Rc::new(Cell::new(false));
    let mut content = reveal(measured_child(&calls, &invalidate), true, |_| Color::WHITE);
    let (mut tree, mut node) = mount(&mut content, &renderer);
    let started = Instant::now();
    assert_eq!(node.size().height, 0.0);
    let mut previous_height = 0.0;
    for index in 0..=10 {
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                started + REVEAL_DURATION * index / 10,
            )),
            mouse::Cursor::Unavailable,
        );
        node = relayout(&mut content, &mut tree, &renderer);
        assert!(node.size().height >= previous_height);
        assert_eq!(Layout::new(&node).child(0).bounds().height, 40.0);
        previous_height = node.size().height;
    }
    assert_eq!(previous_height, 40.0);
    assert_eq!(calls.get(), 1, "clip height does not remeasure the child");
}

#[test]
fn motion_child_layout_refreshes_for_diff_child_requests_operations_and_limits() {
    let _font_guard = crate::font_system_test_guard();
    let renderer = renderer();
    let calls = Rc::new(Cell::new(0));
    let invalidate = Rc::new(Cell::new(false));
    let mut content = popup(measured_child(&calls, &invalidate));
    let (mut tree, mut node) = mount(&mut content, &renderer);
    let started = Instant::now();
    invalidate.set(true);
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(started)),
        mouse::Cursor::Unavailable,
    );
    let _ = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(
        calls.get(),
        2,
        "child invalidation reaches the cached layout"
    );
    node = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(calls.get(), 2);

    content.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut focusable::unfocus::<()>(),
    );
    let _ = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(calls.get(), 3);

    tree.diff(content.as_widget_mut());
    let _ = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(calls.get(), 4);

    let narrower = layout::Limits::new(Size::ZERO, Size::new(200.0, 180.0));
    node = content
        .as_widget_mut()
        .layout(&mut tree, &renderer, &narrower);
    assert_eq!(calls.get(), 5);
    assert_eq!(node.size().width, 200.0);
}

#[test]
fn motion_child_layout_refreshes_for_renderer_defaults_and_font_version() {
    let _font_guard = crate::font_system_test_guard();
    let renderer = renderer();
    let calls = Rc::new(Cell::new(0));
    let invalidate = Rc::new(Cell::new(false));
    let mut content = popup(measured_child(&calls, &invalidate));
    let (mut tree, _) = mount(&mut content, &renderer);

    // Simulate prior contexts without requiring a GPU or changing the
    // machine's fonts. The live renderer must replace each stale context.
    let state = tree.state.downcast_mut::<State>();
    state.child_layout.as_mut().unwrap().context.scale_factor = Some(2.0);
    let _ = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(calls.get(), 2);
    tree.state
        .downcast_mut::<State>()
        .child_layout
        .as_mut()
        .unwrap()
        .context
        .default_size = iced::Pixels(1.0);
    let _ = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(calls.get(), 3);
    tree.state
        .downcast_mut::<State>()
        .child_layout
        .as_mut()
        .unwrap()
        .context
        .default_font = iced::Font::MONOSPACE;
    let _ = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(calls.get(), 4);

    // Owned bytes always advance the global version, even when the font
    // was already registered by another renderer in this process.
    iced::advanced::graphics::text::font_system()
        .write()
        .expect("Write font system")
        .load_font(std::borrow::Cow::Owned(
            include_bytes!("../../../../vendor/iced/graphics/fonts/Iced-Icons.ttf").to_vec(),
        ));
    let _ = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(calls.get(), 5);
}
