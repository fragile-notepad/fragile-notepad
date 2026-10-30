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
fn retained_native_drag_autoscrolls_with_an_unavailable_widget_cursor_and_moves_thumb() {
    let renderer = renderer();
    let make = |drag_position: Option<super::super::DragPosition>| {
        view_tracked(
            Space::new()
                .width(CONTENT_WIDTH)
                .height(super::super::TAB_HEIGHT),
            drag_position.is_some(),
            drag_position,
        )
    };
    let mut element = make(Some(super::super::DragPosition {
        pointer_x: 900.0,
        grab_offset: 20.0,
        group_width: 120.0,
    }));
    let (mut tree, node) = mount(&mut element, &renderer);
    let start = Instant::now();
    let mut redraw = window::RedrawRequest::Wait;
    for frame in 0..100 {
        let (_, requested, _) = dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                start + Duration::from_millis(frame * 16),
            )),
            mouse::Cursor::Unavailable,
        );
        redraw = requested;
        if frame == 0 {
            assert!(tree.state.downcast_ref::<State>().metrics.offset > 0.0);
            assert_eq!(redraw, window::RedrawRequest::NextFrame);
        }
    }
    let state = tree.state.downcast_ref::<State>();
    close(state.metrics.offset, CONTENT_WIDTH - WIDTH);
    close(state.reveal.target, 1.0);
    assert_eq!(redraw, window::RedrawRequest::Wait);
    let right_thumb = state
        .metrics
        .geometry(Layout::new(&node).bounds())
        .unwrap()
        .thumb_x;
    element = make(Some(super::super::DragPosition {
        pointer_x: -100.0,
        grab_offset: 20.0,
        group_width: 120.0,
    }));
    tree.diff(element.as_widget_mut());
    let node = relayout(&mut element, &mut tree, &renderer, WIDTH);
    for frame in 100..200 {
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                start + Duration::from_millis(frame * 16),
            )),
            mouse::Cursor::Unavailable,
        );
    }
    let state = tree.state.downcast_ref::<State>();
    close(state.metrics.offset, 0.0);
    assert!(
        state
            .metrics
            .geometry(Layout::new(&node).bounds())
            .unwrap()
            .thumb_x
            < right_thumb
    );
    element = make(None);
    tree.diff(element.as_widget_mut());
    let node = relayout(&mut element, &mut tree, &renderer, WIDTH);
    dispatch(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            start + Duration::from_millis(3300),
        )),
        mouse::Cursor::Unavailable,
    );
    let state = tree.state.downcast_ref::<State>();
    assert!(state.drag_scroll.last_scroll.is_none());
    close(state.metrics.offset, 0.0);
    close(state.reveal.target, 0.0);
}

#[test]
fn edge_motion_preserves_pointer_displacement_and_speed_across_redraw_rates() {
    let renderer = renderer();
    let make = |pointer_x| {
        view_tracked(
            Space::new()
                .width(CONTENT_WIDTH)
                .height(super::super::TAB_HEIGHT),
            true,
            Some(super::super::DragPosition {
                pointer_x,
                grab_offset: 20.0,
                group_width: 120.0,
            }),
        )
    };
    for frame_ms in [8, 16, 32] {
        let mut element = make(120.0);
        let (mut tree, _) = mount(&mut element, &renderer);
        let start = Instant::now();
        for (frame, pointer_x) in [120.0, 200.0, 280.0, 440.0].into_iter().enumerate() {
            element = make(pointer_x);
            tree.diff(element.as_widget_mut());
            let node = relayout(&mut element, &mut tree, &renderer, WIDTH);
            dispatch(
                &mut element,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(
                    start + Duration::from_millis(frame as u64 * frame_ms),
                )),
                mouse::Cursor::Unavailable,
            );
            let drag = super::super::DragPosition {
                pointer_x,
                grab_offset: 20.0,
                group_width: 120.0,
            };
            let painted = drag.leading_x(drag.group_width, WIDTH);
            close(
                tree.state.downcast_ref::<State>().metrics.offset + painted,
                pointer_x - 20.0,
            );
        }
        let node = relayout(&mut element, &mut tree, &renderer, WIDTH);
        for _ in 0..8 {
            dispatch(
                &mut element,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(440.0, 15.0),
                }),
                mouse::Cursor::Unavailable,
            );
            close(tree.state.downcast_ref::<State>().metrics.offset, 220.0);
        }
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                start + Duration::from_millis(4 * frame_ms),
            )),
            mouse::Cursor::Unavailable,
        );
        // The preceding pointer movement was 160 pixels per frame. Holding at
        // the edge continues by that distance instead of a 480 px/s ceiling.
        close(tree.state.downcast_ref::<State>().metrics.offset, 380.0);
        element = make(420.0);
        tree.diff(element.as_widget_mut());
        let node = relayout(&mut element, &mut tree, &renderer, WIDTH);
        for frame in [5, 6] {
            dispatch(
                &mut element,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(
                    start + Duration::from_millis(frame * frame_ms),
                )),
                mouse::Cursor::Unavailable,
            );
            // Reversing 20 pixels consumes those 20 pixels immediately, then
            // stops continuation in the previous direction.
            close(tree.state.downcast_ref::<State>().metrics.offset, 360.0);
        }
    }
}

#[test]
fn release_or_native_cancellation_stops_edge_scroll_before_the_view_rebuilds() {
    use iced::window::tab_drag::{CancelReason, Event as DragEvent, SessionId};
    for terminal in [
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        Event::Window(window::Event::TabDrag(DragEvent::Released {
            session: SessionId(1),
        })),
        Event::Window(window::Event::TabDrag(DragEvent::Cancelled {
            session: SessionId(1),
            reason: CancelReason::Requested,
        })),
    ] {
        let renderer = renderer();
        let mut element = view_tracked(
            Space::new()
                .width(CONTENT_WIDTH)
                .height(super::super::TAB_HEIGHT),
            true,
            Some(super::super::DragPosition {
                pointer_x: 900.0,
                grab_offset: 20.0,
                group_width: 120.0,
            }),
        );
        let (mut tree, node) = mount(&mut element, &renderer);
        let start = Instant::now();
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(start)),
            mouse::Cursor::Unavailable,
        );
        let before = tree.state.downcast_ref::<State>().metrics.offset;
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            terminal,
            mouse::Cursor::Unavailable,
        );
        let state = tree.state.downcast_ref::<State>();
        close(state.metrics.offset, before);
        assert!(state.drag_scroll.last_scroll.is_none());
        assert!(state.drag_scroll.last_pointer.is_none());
        assert!(state.drag_scroll.velocity.is_none());
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
fn fade_and_emphasis_reverse_in_both_directions_without_jumping() {
    let start = Instant::now();
    for (forward, backward) in [(FADE_IN, FADE_OUT), (EMPHASIS_DURATION, EMPHASIS_DURATION)] {
        for initially_visible in [false, true] {
            let initial = if initially_visible { 1.0 } else { 0.0 };
            let (outbound, inbound) = if initially_visible {
                (backward, forward)
            } else {
                (forward, backward)
            };
            // Include immediate reversal, intermediate frames, and both sides
            // of the endpoint where advance clears the animation clock.
            for elapsed in [
                Duration::ZERO,
                Duration::from_millis(1),
                outbound / 2,
                outbound - Duration::from_millis(1),
                outbound,
                outbound + window::frame_interval(),
            ] {
                let mut transition = Transition {
                    value: initial,
                    target: initial,
                    ..Default::default()
                };
                transition.retarget(!initially_visible, start, outbound);
                let reversed_at = start + elapsed;
                transition.advance(reversed_at);
                let before = transition.value;
                transition.retarget(initially_visible, reversed_at, inbound);
                close(transition.value, before);
                close(transition.from, before);
                close(transition.target, initial);
                let mut previous = before;
                for step in 1..=8 {
                    transition.advance(reversed_at + inbound * step / 8);
                    assert!((0.0..=1.0).contains(&transition.value));
                    if initially_visible {
                        assert!(transition.value >= previous);
                    } else {
                        assert!(transition.value <= previous);
                    }
                    previous = transition.value;
                }
                close(transition.value, initial);
                assert!(transition.started.is_none());
            }
            // Repeated reversals must preserve the sampled value every time
            // and eventually settle at whichever endpoint was requested last.
            let mut transition = Transition {
                value: initial,
                target: initial,
                ..Default::default()
            };
            let mut now = start;
            let mut visible = initially_visible;
            for index in 0..64 {
                now += Duration::from_millis([0, 1, 7, 16, 40][index % 5]);
                transition.advance(now);
                let before = transition.value;
                visible = !visible;
                transition.retarget(visible, now, if visible { forward } else { backward });
                close(transition.value, before);
                close(transition.from, before);
                assert!((0.0..=1.0).contains(&transition.value));
            }
            transition.advance(now + forward + backward);
            close(transition.value, if visible { 1.0 } else { 0.0 });
            assert!(transition.started.is_none());
        }
    }
}

#[test]
fn rapid_input_during_fades_requests_a_frame_without_postponing_it() {
    let renderer = renderer();
    for visible in [true, false] {
        let mut element = strip();
        let (mut tree, node) = mount(&mut element, &renderer);
        let point = Point::new(50.0, if visible { 15.0 } else { 50.0 });
        let cursor = pointer(point);
        // Establish the native Scrollable's status so its own hover redraw
        // cannot mask a missing animation redraw in the strip.
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(Instant::now())),
            cursor,
        );
        let initial = if visible { 0.0 } else { 1.0 };
        tree.state.downcast_mut::<State>().reveal = Transition {
            value: initial,
            from: initial,
            target: 1.0 - initial,
            started: Some(Instant::now()),
            duration: if visible { FADE_IN } else { FADE_OUT },
        };
        let mut requested_frames = 0;
        for _ in 0..64 {
            let before = tree.state.downcast_ref::<State>().reveal.value;
            let (_, redraw, _) = dispatch(
                &mut element,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(mouse::Event::CursorMoved { position: point }),
                cursor,
            );
            let reveal = &tree.state.downcast_ref::<State>().reveal;
            // Scheduling delays may complete the fade between input packets.
            // Its final value still needs a frame; settled input can stay idle.
            if reveal.started.is_some() || reveal.value != before {
                assert_eq!(redraw, window::RedrawRequest::NextFrame);
                requested_frames += 1;
            } else {
                assert_eq!(redraw, window::RedrawRequest::Wait);
            }
        }
        assert!(requested_frames > 0);
    }
}

#[test]
fn fade_redraws_request_next_frames_and_stop_at_both_endpoints() {
    let renderer = renderer();
    let mut element = strip();
    let (mut tree, node) = mount(&mut element, &renderer);
    let now = Instant::now();
    // Only redraw events advance this clock; input events use wall time.
    for (cursor, start, duration, target) in [
        (pointer(Point::new(50.0, 15.0)), now, FADE_IN, 1.0),
        (
            mouse::Cursor::Unavailable,
            now + FADE_IN + window::frame_interval(),
            FADE_OUT,
            0.0,
        ),
    ] {
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(start)),
            cursor,
        );
        for (elapsed, expected_value, expected_redraw) in [
            (duration / 2, 0.5, window::RedrawRequest::NextFrame),
            (duration, target, window::RedrawRequest::Wait),
        ] {
            let (_, redraw, _) = dispatch(
                &mut element,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(start + elapsed)),
                cursor,
            );
            close(
                tree.state.downcast_ref::<State>().reveal.value,
                expected_value,
            );
            assert_eq!(redraw, expected_redraw);
        }
    }
}

#[test]
fn input_completing_a_fade_still_requests_its_final_frame() {
    let renderer = renderer();
    for (revealed, emphasized, fading_reveal) in [
        (false, false, true),
        (true, false, true),
        (true, true, false),
        (true, false, false),
    ] {
        let mut element = strip();
        let (mut tree, node) = mount(&mut element, &renderer);
        let cursor = if revealed {
            pointer(Point::new(50.0, if emphasized { 2.0 } else { 15.0 }))
        } else {
            mouse::Cursor::Unavailable
        };
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(Instant::now())),
            cursor,
        );
        let state = tree.state.downcast_mut::<State>();
        for (transition, visible, fading) in [
            (&mut state.reveal, revealed, fading_reveal),
            (&mut state.emphasis, emphasized, !fading_reveal),
        ] {
            *transition = Transition {
                value: if fading {
                    0.5
                } else if visible {
                    1.0
                } else {
                    0.0
                },
                from: if visible { 0.0 } else { 1.0 },
                target: if visible { 1.0 } else { 0.0 },
                started: fading.then(|| Instant::now() - FADE_OUT),
                duration: FADE_OUT,
            };
        }
        let (_, redraw, _) = dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::empty(),
            )),
            cursor,
        );
        let state = tree.state.downcast_ref::<State>();
        close(state.reveal.value, if revealed { 1.0 } else { 0.0 });
        close(state.emphasis.value, if emphasized { 1.0 } else { 0.0 });
        assert!(state.reveal.started.is_none() && state.emphasis.started.is_none());
        assert_eq!(redraw, window::RedrawRequest::NextFrame);
    }
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
            phase: mouse::ScrollPhase::Moved,
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
fn vertical_wheel_scrolls_tabs_both_ways_over_the_body_and_top_lane() {
    let renderer = renderer();
    for y in [2.0, 15.0] {
        let mut element = strip();
        let (mut tree, node) = mount(&mut element, &renderer);
        let cursor = pointer(Point::new(100.0, y));
        for (steps, expected) in [
            (-1.0, 60.0),
            (1.0, 0.0),
            (-100.0, CONTENT_WIDTH - WIDTH),
            (100.0, 0.0),
        ] {
            let before = tree.state.downcast_ref::<State>().metrics.offset;
            let (captured, redraw, _) = dispatch(
                &mut element,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(mouse::Event::WheelScrolled {
                    phase: mouse::ScrollPhase::Moved,
                    delta: mouse::ScrollDelta::Lines { x: 0.0, y: steps },
                }),
                cursor,
            );
            assert!(captured);
            assert_ne!(redraw, window::RedrawRequest::Wait);
            close(tree.state.downcast_ref::<State>().metrics.offset, before);
            let now = Instant::now();
            if expected == 60.0 {
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
                let offset = tree.state.downcast_ref::<State>().metrics.offset;
                assert!(offset > 0.0 && offset < expected);
            }
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
            close(tree.state.downcast_ref::<State>().metrics.offset, expected);
        }
    }
}

#[test]
fn vertical_precise_input_keeps_fractions_and_prefers_existing_horizontal_motion() {
    let renderer = renderer();
    let mut element = strip();
    let (mut tree, node) = mount(&mut element, &renderer);
    let cursor = pointer(Point::new(100.0, 15.0));
    for (delta, expected) in [
        (mouse::ScrollDelta::Pixels { x: 0.0, y: -12.5 }, 12.5),
        (
            mouse::ScrollDelta::Pixels {
                x: -3.25,
                y: -100.0,
            },
            15.75,
        ),
        (mouse::ScrollDelta::Lines { x: 0.0, y: -0.25 }, 30.75),
        (mouse::ScrollDelta::Lines { x: -0.5, y: -3.0 }, 60.75),
        (mouse::ScrollDelta::Pixels { x: 0.0, y: 5.5 }, 55.25),
    ] {
        let (captured, _, _) = dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta,
                phase: mouse::ScrollPhase::Moved,
            }),
            cursor,
        );
        assert!(captured);
        close(tree.state.downcast_ref::<State>().metrics.offset, expected);
    }
}

#[test]
fn vertical_wheel_does_not_scroll_or_capture_when_tabs_fit_or_pointer_is_outside() {
    let renderer = renderer();
    for (width, point) in [
        (1000.0, Point::new(100.0, 15.0)),
        (WIDTH, Point::new(WIDTH + 1.0, 15.0)),
    ] {
        let mut element = strip();
        let (mut tree, _) = mount(&mut element, &renderer);
        let node = relayout(&mut element, &mut tree, &renderer, width);
        let cursor = pointer(point);
        let (captured, _, _) = dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::WheelScrolled {
                phase: mouse::ScrollPhase::Moved,
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
            cursor,
        );
        assert!(!captured);
        close(tree.state.downcast_ref::<State>().metrics.offset, 0.0);
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_millis(400),
            )),
            cursor,
        );
        close(tree.state.downcast_ref::<State>().metrics.offset, 0.0);
    }
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
            phase: mouse::ScrollPhase::Moved,
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
            phase: mouse::ScrollPhase::Moved,
            delta: mouse::ScrollDelta::Pixels { x: -12.5, y: 0.0 },
        }),
        cursor,
    );
    // Native operations preserve logical offsets; only drawing snaps to the
    // physical pixel grid. The strip follows precise input without rounding.
    close(tree.state.downcast_ref::<State>().metrics.offset, 72.5);
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
            phase: mouse::ScrollPhase::Moved,
            delta: if cfg!(target_os = "macos") {
                mouse::ScrollDelta::Lines { x: -0.5, y: 0.0 }
            } else {
                mouse::ScrollDelta::Lines { x: 0.0, y: -0.5 }
            },
        }),
        cursor,
    );
    close(tree.state.downcast_ref::<State>().metrics.offset, 102.5);
}

#[test]
fn wheel_reverses_mid_scroll_in_both_directions_without_jumping() {
    let renderer = renderer();
    let cursor = pointer(Point::new(100.0, 2.0));
    for direction in [-1.0, 1.0] {
        let mut element = strip();
        let (mut tree, node) = mount(&mut element, &renderer);
        let initial = (CONTENT_WIDTH - WIDTH) / 2.0;
        element.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut ScrollOperation {
                offset: Some(initial),
                ..Default::default()
            },
        );
        let wheel = |x| {
            Event::Mouse(mouse::Event::WheelScrolled {
                phase: mouse::ScrollPhase::Moved,
                delta: mouse::ScrollDelta::Lines { x, y: 0.0 },
            })
        };
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            wheel(direction * 4.0),
            cursor,
        );
        close(tree.state.downcast_ref::<State>().metrics.offset, initial);
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_millis(40),
            )),
            cursor,
        );
        let before = tree.state.downcast_ref::<State>().metrics.offset;
        assert!((before - initial) * direction < 0.0);
        assert!((before - initial).abs() < 240.0);

        let (captured, redraw, _) = dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            wheel(-direction * 4.0),
            cursor,
        );
        assert!(captured);
        assert_ne!(redraw, window::RedrawRequest::Wait);
        close(tree.state.downcast_ref::<State>().metrics.offset, before);
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_millis(40),
            )),
            cursor,
        );
        let reversing = tree.state.downcast_ref::<State>().metrics.offset;
        assert!((reversing - before) * direction > 0.0);
        dispatch(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_millis(400),
            )),
            cursor,
        );
        let target = (before + direction * 240.0).clamp(0.0, CONTENT_WIDTH - WIDTH);
        close(tree.state.downcast_ref::<State>().metrics.offset, target);
        close(tree.state.downcast_ref::<State>().reveal.value, 1.0);
    }
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
