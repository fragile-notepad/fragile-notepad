//! Headless mounting and event dispatch shared by motion widget tests.

use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::Headless;
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, layout, mouse, renderer};
use iced::{Element, Event, Rectangle, Renderer, Size, window};

use crate::message::Message;

pub(super) const VIEWPORT: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 320.0,
    height: 240.0,
};

pub(super) fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("CPU headless renderer must be available")
}

pub(super) fn mount(
    content: &mut Element<'_, Message>,
    renderer: &Renderer,
) -> (Tree, layout::Node) {
    // The vendored runtime diffs even a newly created tree before layout;
    // Widget has no children() hook in this version of Iced.
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());
    let node = relayout(content, &mut tree, renderer);
    (tree, node)
}

pub(super) fn relayout(
    content: &mut Element<'_, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
) -> layout::Node {
    content.as_widget_mut().layout(
        tree,
        renderer,
        &layout::Limits::new(Size::ZERO, VIEWPORT.size()),
    )
}

pub(super) fn dispatch(
    content: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    event: Event,
    cursor: mouse::Cursor,
) -> (window::RedrawRequest, Vec<Message>) {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    content.as_widget_mut().update(
        tree,
        &event,
        Layout::new(node),
        cursor,
        renderer,
        &mut shell,
        &VIEWPORT,
    );
    let redraw = shell.redraw_request();
    (redraw, messages)
}
