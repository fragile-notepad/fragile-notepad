use super::*;
use crate::core::{DocumentId, Workspace};
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::Headless;
use iced::widget::{Space, mouse_area};
use iced::{Point, keyboard};

const WIDTH: f32 = 320.0;
const CONTENT_WIDTH: f32 = 900.0;
const VIEWPORT: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 1200.0,
    height: 100.0,
};

fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("CPU headless renderer")
}

fn strip() -> Element<'static, Message> {
    let id = DocumentId::new(1);
    view(
        mouse_area(
            Space::new()
                .width(CONTENT_WIDTH)
                .height(super::super::TAB_HEIGHT),
        )
        .on_press(Message::TabDragStarted(id))
        .on_release(Message::TabDragReleased(id)),
    )
}

fn relayout(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
    width: f32,
) -> layout::Node {
    element.as_widget_mut().layout(
        tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(width, VIEWPORT.height)),
    )
}

fn mount(element: &mut Element<'_, Message>, renderer: &Renderer) -> (Tree, layout::Node) {
    let mut tree = Tree::empty();
    tree.diff(element.as_widget_mut());
    let node = relayout(element, &mut tree, renderer, WIDTH);
    (tree, node)
}

fn dispatch(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    event: Event,
    cursor: mouse::Cursor,
) -> (bool, window::RedrawRequest, Vec<Message>) {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    element.as_widget_mut().update(
        tree,
        &event,
        Layout::new(node),
        cursor,
        renderer,
        &mut shell,
        &VIEWPORT,
    );
    (shell.is_event_captured(), shell.redraw_request(), messages)
}

fn pointer(point: Point) -> mouse::Cursor {
    mouse::Cursor::Available(point)
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn actual_tab_row_keeps_its_height_and_measures_overflow() {
    let renderer = renderer();
    let mut workspace = Workspace::new();
    for overflow in [false, true] {
        if overflow {
            for _ in 0..8 {
                workspace.create_untitled();
            }
        }
        let mut tabs = super::super::view(&workspace, None, None);
        let (tree, node) = mount(&mut tabs, &renderer);
        close(node.size().height, super::super::TAB_HEIGHT + 1.0);
        // Container forwards its child's widget state in this Iced version.
        let state = tree.state.downcast_ref::<State>();
        close(state.metrics.viewport_width, WIDTH);
        assert_eq!(state.metrics.maximum() > 0.0, overflow);
        assert_eq!(
            state
                .metrics
                .geometry(Layout::new(&node).child(0).bounds())
                .is_some(),
            overflow
        );
        close(state.reveal.value, 0.0);
        assert!(state.reveal.started.is_none());
    }
}

#[test]
fn hovering_a_tab_fades_in_and_reverses_without_jumping() {
    let renderer = renderer();
    let mut element = strip();
    let (mut tree, node) = mount(&mut element, &renderer);
    let body = pointer(Point::new(150.0, 15.0));
    let (_, redraw, _) = dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(150.0, 15.0),
        }),
        body,
    );
    assert_ne!(redraw, window::RedrawRequest::Wait);
    let start = tree.state.downcast_ref::<State>().reveal.started.unwrap();
    let frame = |elapsed| {
        Event::Window(window::Event::RedrawRequested(
            start + Duration::from_millis(elapsed),
        ))
    };
    dispatch(&mut element, &mut tree, &node, &renderer, frame(60), body);
    close(tree.state.downcast_ref::<State>().reveal.value, 0.5);
    // Leaving starts from the opacity already shown by this frame.
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        frame(90),
        mouse::Cursor::Unavailable,
    );
    let leaving = tree.state.downcast_ref::<State>();
    close(leaving.reveal.from, leaving.reveal.value);
    assert_eq!(leaving.reveal.target, 0.0);
    let before = leaving.reveal.value;
    dispatch(&mut element, &mut tree, &node, &renderer, frame(130), body);
    let returning = tree.state.downcast_ref::<State>();
    assert!(returning.reveal.value > 0.0 && returning.reveal.value < before);
    close(returning.reveal.from, returning.reveal.value);
    assert_eq!(returning.reveal.target, 1.0);
    dispatch(&mut element, &mut tree, &node, &renderer, frame(500), body);
    close(tree.state.downcast_ref::<State>().reveal.value, 1.0);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        frame(510),
        mouse::Cursor::Unavailable,
    );
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        frame(800),
        mouse::Cursor::Unavailable,
    );
    let hidden = tree.state.downcast_ref::<State>();
    close(hidden.reveal.value, 0.0);
    assert!(hidden.reveal.started.is_none());
}

#[test]
fn dragging_preserves_the_grab_point_clamps_and_suppresses_tab_release() {
    let renderer = renderer();
    let mut element = strip();
    let (mut tree, node) = mount(&mut element, &renderer);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: -1.0, y: 0.0 },
        }),
        pointer(Point::new(150.0, 15.0)),
    );
    let now = Instant::now();
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            now + Duration::from_millis(40),
        )),
        pointer(Point::new(150.0, 15.0)),
    );
    let original_offset = tree.state.downcast_ref::<State>().metrics.offset;
    assert!(original_offset > 0.0 && original_offset < 60.0);
    let bounds = Layout::new(&node).bounds();
    let geometry = tree
        .state
        .downcast_ref::<State>()
        .metrics
        .geometry(bounds)
        .unwrap();
    let point = Point::new(geometry.thumb_x + geometry.thumb_width * 0.2, 2.0);
    let (captured, _, messages) = dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        pointer(point),
    );
    assert!(captured && messages.is_empty());
    close(tree.state.downcast_ref::<State>().grabbed_at.unwrap(), 0.2);
    close(
        tree.state.downcast_ref::<State>().metrics.offset,
        original_offset,
    );
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            now + Duration::from_millis(400),
        )),
        pointer(point),
    );
    close(
        tree.state.downcast_ref::<State>().metrics.offset,
        original_offset,
    );
    assert_eq!(
        element.as_widget().mouse_interaction(
            &tree,
            Layout::new(&node),
            pointer(point),
            &VIEWPORT,
            &renderer,
        ),
        mouse::Interaction::Grabbing
    );
    for (x, expected) in [(2000.0, CONTENT_WIDTH - WIDTH), (-100.0, 0.0)] {
        let outside = Point::new(x, 50.0);
        let (captured, _, messages) = dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::CursorMoved { position: outside }),
            pointer(outside),
        );
        assert!(captured && messages.is_empty());
        close(tree.state.downcast_ref::<State>().metrics.offset, expected);
        assert_eq!(tree.state.downcast_ref::<State>().reveal.target, 1.0);
    }
    // MouseArea sends release messages without requiring a press. Releasing
    // over a tab body must still belong solely to the scrollbar gesture.
    let body = pointer(Point::new(150.0, 15.0));
    let (captured, _, messages) = dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        body,
    );
    assert!(captured && messages.is_empty());
    assert!(tree.state.downcast_ref::<State>().grabbed_at.is_none());
    let (_, _, pressed) = dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        body,
    );
    let (_, _, released) = dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        body,
    );
    assert!(matches!(pressed.as_slice(), [Message::TabDragStarted(_)]));
    assert!(matches!(released.as_slice(), [Message::TabDragReleased(_)]));
}

#[test]
fn wheel_over_the_top_lane_keeps_native_easing_and_precise_input() {
    let renderer = renderer();
    let mut element = strip();
    let (mut tree, node) = mount(&mut element, &renderer);
    let cursor = pointer(Point::new(100.0, 2.0));
    let (captured, redraw, _) = dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: -1.0, y: 0.0 },
        }),
        cursor,
    );
    assert!(captured);
    assert_ne!(redraw, window::RedrawRequest::Wait);
    close(tree.state.downcast_ref::<State>().metrics.offset, 0.0);
    let now = Instant::now();
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            now + Duration::from_millis(40),
        )),
        cursor,
    );
    let state = tree.state.downcast_ref::<State>();
    assert!(state.metrics.offset > 0.0 && state.metrics.offset < 60.0);
    let geometry = state.metrics.geometry(Layout::new(&node).bounds()).unwrap();
    close(
        geometry.thumb_x,
        geometry.track.x
            + (geometry.track.width - geometry.thumb_width) * state.metrics.offset
                / geometry.maximum,
    );
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            now + Duration::from_millis(400),
        )),
        cursor,
    );
    close(tree.state.downcast_ref::<State>().metrics.offset, 60.0);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: -12.5, y: 0.0 },
        }),
        cursor,
    );
    // The operation reports the native displayed translation, which snaps
    // fractional precise input to pixels (60 + 12.5 displays at 73).
    close(tree.state.downcast_ref::<State>().metrics.offset, 73.0);
    // Preserve the native horizontal row convention: Shift + vertical wheel.
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Keyboard(keyboard::Event::ModifiersChanged(
            keyboard::Modifiers::SHIFT,
        )),
        cursor,
    );
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: if cfg!(target_os = "macos") {
                mouse::ScrollDelta::Lines { x: -0.5, y: 0.0 }
            } else {
                mouse::ScrollDelta::Lines { x: 0.0, y: -0.5 }
            },
        }),
        cursor,
    );
    close(tree.state.downcast_ref::<State>().metrics.offset, 103.0);
}

#[test]
fn widening_until_tabs_fit_cancels_drag_and_hides_the_strip() {
    let renderer = renderer();
    let mut element = strip();
    let (mut tree, node) = mount(&mut element, &renderer);
    let cursor = pointer(Point::new(10.0, 2.0));
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        cursor,
    );
    assert!(tree.state.downcast_ref::<State>().grabbed_at.is_some());
    let expanded = relayout(&mut element, &mut tree, &renderer, 1000.0);
    close(expanded.size().height, node.size().height);
    let state = tree.state.downcast_ref::<State>();
    assert!(
        state
            .metrics
            .geometry(Layout::new(&expanded).bounds())
            .is_none()
    );
    assert!(state.grabbed_at.is_none());
    close(state.reveal.value, 0.0);
    close(state.emphasis.value, 0.0);
    dispatch(
        &mut element,
        &mut tree,
        &expanded,
        &renderer,
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(10.0, 2.0),
        }),
        cursor,
    );
    assert!(tree.state.downcast_ref::<State>().reveal.started.is_none());
    let narrow = relayout(&mut element, &mut tree, &renderer, WIDTH);
    assert!(
        tree.state
            .downcast_ref::<State>()
            .metrics
            .geometry(Layout::new(&narrow).bounds())
            .is_some()
    );
    assert!(tree.state.downcast_ref::<State>().grabbed_at.is_none());
}

#[test]
fn focus_loss_cancels_drag_and_stays_hidden_across_view_rebuilds() {
    let renderer = renderer();
    let mut element = strip();
    let (mut tree, node) = mount(&mut element, &renderer);
    let point = Point::new(10.0, 2.0);
    let cursor = pointer(point);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        cursor,
    );
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::Unfocused),
        cursor,
    );
    assert!(tree.state.downcast_ref::<State>().grabbed_at.is_none());
    assert!(!tree.state.downcast_ref::<State>().focused);
    element = strip();
    tree.diff(element.as_widget_mut());
    let node = relayout(&mut element, &mut tree, &renderer, WIDTH);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::CursorMoved { position: point }),
        cursor,
    );
    assert_eq!(tree.state.downcast_ref::<State>().reveal.target, 0.0);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::Focused),
        cursor,
    );
    assert_eq!(tree.state.downcast_ref::<State>().reveal.target, 1.0);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Mouse(mouse::Event::CursorLeft),
        cursor,
    );
    let now = Instant::now();
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(now + FADE_OUT)),
        cursor,
    );
    close(tree.state.downcast_ref::<State>().reveal.value, 0.0);
    assert_eq!(tree.state.downcast_ref::<State>().reveal.target, 0.0);
}
