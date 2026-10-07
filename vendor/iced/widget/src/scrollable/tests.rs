use super::*;
use crate::core::shell::Waker;
use std::cell::Cell;

struct CursorContent<'a>(&'a Cell<Option<bool>>);

struct ViewportContent<'a>(&'a Cell<Option<Rectangle>>);

impl Widget<(), Theme, ()> for ViewportContent<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(1000.0), Length::Fixed(1000.0))
    }

    fn layout(&mut self, _: &mut Tree, _: &(), _: &layout::Limits) -> layout::Node {
        layout::Node::new(Size::new(1000.0, 1000.0))
    }

    fn draw(
        &self,
        _: &Tree,
        _: &mut (),
        _: &Theme,
        _: &renderer::Style,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
    }

    fn update(
        &mut self,
        _: &mut Tree,
        _: &Event,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &(),
        _: &mut Shell<'_, ()>,
        viewport: &Rectangle,
    ) {
        self.0.set(Some(*viewport));
    }
}

impl Widget<(), Theme, ()> for CursorContent<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(1000.0), Length::Fixed(1000.0))
    }

    fn layout(&mut self, _: &mut Tree, _: &(), _: &layout::Limits) -> layout::Node {
        layout::Node::new(Size::new(1000.0, 1000.0))
    }

    fn draw(
        &self,
        _: &Tree,
        _: &mut (),
        _: &Theme,
        _: &renderer::Style,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
    }

    fn update(
        &mut self,
        _: &mut Tree,
        _: &Event,
        _: Layout<'_>,
        cursor: mouse::Cursor,
        _: &(),
        _: &mut Shell<'_, ()>,
        _: &Rectangle,
    ) {
        self.0.set(Some(cursor.position().is_some()));
    }
}

const BOUNDS: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 100.0,
    height: 100.0,
};

fn update(
    scrollable: &mut Scrollable<'_, (), Theme, ()>,
    tree: &mut Tree,
    node: &layout::Node,
    event: Event,
    cursor: mouse::Cursor,
) -> window::RedrawRequest {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    scrollable.update(
        tree,
        &event,
        Layout::new(node),
        cursor,
        &(),
        &mut shell,
        &BOUNDS,
    );
    shell.redraw_request()
}

#[test]
fn changing_scrollable_identity_resets_position_and_pending_interactions() {
    let make = |id| smooth_widget().id(id);
    let mut scrollable = make("first");
    let mut tree = Tree::empty();
    tree.diff(&mut scrollable as &mut dyn Widget<(), Theme, ()>);
    let content = Rectangle::with_size(Size::new(1000.0, 1000.0));
    let state = tree.state.downcast_mut::<State>();
    state.scroll_to(AbsoluteOffset {
        x: None,
        y: Some(30.0),
    });
    assert!(state.queue_smooth_scroll(Vector::new(0.0, 60.0), BOUNDS, content, Instant::now()));
    state.interaction = Interaction::YScrollerGrabbed(0.5);

    let mut rebuilt = make("first");
    tree.diff(&mut rebuilt as &mut dyn Widget<(), Theme, ()>);
    let state = tree.state.downcast_ref::<State>();
    assert_eq!(state.offset_y, Offset::Absolute(30.0));
    assert!(state.smooth_motion.is_some());
    assert!(state.scrollers_grabbed());

    let mut replacement = make("second");
    tree.diff(&mut replacement as &mut dyn Widget<(), Theme, ()>);
    let state = tree.state.downcast_ref::<State>();
    assert_eq!(state.offset_y, Offset::Absolute(0.0));
    assert!(state.smooth_motion.is_none());
    assert!(!state.scrollers_grabbed());
    assert_eq!(state.last_id, Some(widget::Id::from("second")));
}

#[test]
fn child_updates_receive_the_visible_parent_viewport_in_content_coordinates() {
    let observed = Cell::new(None);
    let mut scrollable = Scrollable::<(), Theme, ()>::new(Element::new(ViewportContent(&observed)))
        .width(100)
        .height(100);
    let mut tree = Tree::empty();
    tree.diff(&mut scrollable as &mut dyn Widget<(), Theme, ()>);
    let node = scrollable.layout(
        &mut tree,
        &(),
        &layout::Limits::new(Size::ZERO, BOUNDS.size()),
    );
    tree.state
        .downcast_mut::<State>()
        .scroll_to(AbsoluteOffset {
            x: None,
            y: Some(30.0),
        });
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    let viewport = Rectangle {
        x: 20.0,
        y: 40.0,
        width: 40.0,
        height: 30.0,
    };

    scrollable.update(
        &mut tree,
        &Event::Window(window::Event::RedrawRequested(Instant::now())),
        Layout::new(&node),
        mouse::Cursor::Available(BOUNDS.center()),
        &(),
        &mut shell,
        &viewport,
    );

    assert_eq!(observed.get(), Some(viewport + Vector::new(0.0, 30.0)));
}

#[test]
fn fractional_scroll_offsets_are_preserved_for_physical_pixel_snapping() {
    let content = Rectangle::with_size(Size::new(1000.0, 1000.0));
    let mut state = State::default();
    state.scroll_to(AbsoluteOffset {
        x: Some(12.25),
        y: Some(25.5),
    });

    assert_eq!(
        state.translation(
            Direction::Both {
                vertical: Scrollbar::default(),
                horizontal: Scrollbar::default(),
            },
            BOUNDS,
            content,
        ),
        Vector::new(12.25, 25.5),
    );
}

#[test]
fn rebuilding_scrollable_preserves_hover_and_requests_redraw_on_exit() {
    let observed = Cell::new(None);
    let make = || {
        Scrollable::<(), Theme, ()>::new(crate::Space::new().width(100).height(1000))
            .width(100)
            .height(100)
            .style(|theme, status| {
                observed.set(Some(status));
                default(theme, status)
            })
    };
    let mut scrollable = make();
    let mut tree = Tree::empty();
    tree.diff(&mut scrollable as &mut dyn Widget<(), Theme, ()>);
    let node = scrollable.layout(
        &mut tree,
        &(),
        &layout::Limits::new(Size::ZERO, BOUNDS.size()),
    );
    let cursor = mouse::Cursor::Available(Point::new(95.0, 20.0));
    let _ = update(
        &mut scrollable,
        &mut tree,
        &node,
        Event::Window(window::Event::RedrawRequested(Instant::now())),
        cursor,
    );
    let mut rebuilt = make();
    tree.diff(&mut rebuilt as &mut dyn Widget<(), Theme, ()>);
    rebuilt.draw(
        &tree,
        &mut (),
        &Theme::Dark,
        &renderer::Style::default(),
        Layout::new(&node),
        cursor,
        &BOUNDS,
    );
    assert!(matches!(
        observed.get(),
        Some(Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        })
    ));
    assert_eq!(
        update(
            &mut rebuilt,
            &mut tree,
            &node,
            Event::Mouse(mouse::Event::CursorLeft),
            mouse::Cursor::Unavailable
        ),
        window::RedrawRequest::NextFrame,
    );
}

#[test]
fn dragging_scrollbars_suppresses_content_cursor_until_release() {
    for (direction, grab) in [
        (
            Direction::Vertical(Scrollbar::default()),
            Point::new(95.0, 10.0),
        ),
        (
            Direction::Horizontal(Scrollbar::default()),
            Point::new(10.0, 95.0),
        ),
    ] {
        let observed = Cell::new(None);
        let mut scrollable = Scrollable::new(Element::new(CursorContent(&observed)))
            .direction(direction)
            .width(100)
            .height(100);
        let mut tree = Tree::empty();
        tree.diff(&mut scrollable as &mut dyn Widget<(), Theme, ()>);
        let node = scrollable.layout(
            &mut tree,
            &(),
            &layout::Limits::new(Size::ZERO, BOUNDS.size()),
        );
        let center = mouse::Cursor::Available(BOUNDS.center());
        let redraw = || Event::Window(window::Event::RedrawRequested(Instant::now()));
        let _ = update(&mut scrollable, &mut tree, &node, redraw(), center);
        assert_eq!(observed.get(), Some(true));

        let _ = update(
            &mut scrollable,
            &mut tree,
            &node,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            mouse::Cursor::Available(grab),
        );
        assert!(tree.state.downcast_ref::<State>().scrollers_grabbed());
        observed.set(None);
        let _ = update(&mut scrollable, &mut tree, &node, redraw(), center);
        assert_eq!(
            observed.get(),
            Some(false),
            "content must not receive a pointer while dragging"
        );

        let _ = update(
            &mut scrollable,
            &mut tree,
            &node,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            center,
        );
        assert!(!tree.state.downcast_ref::<State>().scrollers_grabbed());
        let _ = update(&mut scrollable, &mut tree, &node, redraw(), center);
        assert_eq!(observed.get(), Some(true));
    }
}

fn smooth_widget() -> Scrollable<'static, (), Theme, ()> {
    Scrollable::new(crate::Space::new().width(1000).height(1000))
        .direction(Direction::Both {
            vertical: Scrollbar::default(),
            horizontal: Scrollbar::default(),
        })
        .smooth_scroll(true)
        .width(100)
        .height(100)
}

#[test]
fn wheel_motion_survives_rebuilds_accumulates_and_finishes_without_idle_frames() {
    let mut widget = smooth_widget();
    let mut tree = Tree::empty();
    tree.diff(&mut widget as &mut dyn Widget<(), Theme, ()>);
    let node = widget.layout(
        &mut tree,
        &(),
        &layout::Limits::new(Size::ZERO, BOUNDS.size()),
    );
    let cursor = mouse::Cursor::Available(BOUNDS.center());
    let wheel = || {
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: -1.0, y: -1.0 },
        })
    };
    assert_eq!(
        update(&mut widget, &mut tree, &node, wheel(), cursor),
        window::RedrawRequest::NextFrame
    );
    assert_eq!(
        tree.state.downcast_ref::<State>().offset_y,
        Offset::Absolute(0.0)
    );
    // Back-to-back wheel packets must accumulate without switching modes or
    // moving immediately, even when the event loop delivers a batch at once.
    let _ = update(&mut widget, &mut tree, &node, wheel(), cursor);
    assert_eq!(
        tree.state.downcast_ref::<State>().offset_y,
        Offset::Absolute(0.0)
    );
    let started = tree
        .state
        .downcast_ref::<State>()
        .smooth_motion
        .unwrap()
        .started;
    let mut rebuilt = smooth_widget();
    tree.diff(&mut rebuilt as &mut dyn Widget<(), Theme, ()>);
    let _ = update(
        &mut rebuilt,
        &mut tree,
        &node,
        Event::Window(window::Event::RedrawRequested(
            started + Duration::from_millis(50),
        )),
        cursor,
    );
    let position = tree
        .state
        .downcast_ref::<State>()
        .absolute_offset(BOUNDS, Rectangle::with_size(Size::new(1000.0, 1000.0)));
    assert!(position.y > 0.0 && position.y < 120.0);
    assert_eq!(position.x, position.y);
    let _ = update(
        &mut rebuilt,
        &mut tree,
        &node,
        Event::Window(window::Event::RedrawRequested(
            started + WHEEL_EASING_DURATION,
        )),
        cursor,
    );
    let state = tree.state.downcast_ref::<State>();
    assert_eq!(state.offset_y, Offset::Absolute(120.0));
    assert!(state.smooth_motion.is_none());
    assert_eq!(
        update(
            &mut rebuilt,
            &mut tree,
            &node,
            Event::Window(window::Event::RedrawRequested(
                started + Duration::from_millis(200)
            )),
            cursor
        ),
        window::RedrawRequest::Wait
    );
}

#[test]
fn reversal_bounds_resize_and_programmatic_scroll_do_not_leave_pending_travel() {
    let content = Rectangle::with_size(Size::new(1000.0, 1000.0));
    let now = Instant::now();
    let mut state = State::default();
    assert!(state.queue_smooth_scroll(Vector::new(0.0, 6000.0), BOUNDS, content, now));
    assert_eq!(state.smooth_motion.unwrap().target.y, 900.0);
    assert!(state.advance_smooth_scroll(now + Duration::from_millis(50), BOUNDS, content));
    let current = state.absolute_offset(BOUNDS, content).y;
    assert!(state.queue_smooth_scroll(Vector::new(0.0, -60.0), BOUNDS, content, now));
    assert_eq!(state.smooth_motion.unwrap().target.y, current - 60.0);
    state.scroll_to(AbsoluteOffset {
        x: None,
        y: Some(30.0),
    });
    assert!(!state.advance_smooth_scroll(now + WHEEL_EASING_DURATION, BOUNDS, content));
    assert_eq!(state.offset_y, Offset::Absolute(30.0));
    assert!(state.queue_smooth_scroll(Vector::new(0.0, 60.0), BOUNDS, content, now));
    let _ = state.advance_smooth_scroll(now + WHEEL_EASING_DURATION, BOUNDS, BOUNDS);
    assert_eq!(state.offset_y, Offset::Absolute(0.0));
    assert!(state.smooth_motion.is_none());
    assert!(!state.queue_smooth_scroll(Vector::new(0.0, -60.0), BOUNDS, content, now));
}

#[test]
fn pixel_input_drag_and_focus_loss_interrupt_wheel_easing() {
    for interruption in [
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -12.0 },
        }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Window(window::Event::Unfocused),
    ] {
        let mut widget = smooth_widget();
        let mut tree = Tree::empty();
        tree.diff(&mut widget as &mut dyn Widget<(), Theme, ()>);
        let node = widget.layout(
            &mut tree,
            &(),
            &layout::Limits::new(Size::ZERO, BOUNDS.size()),
        );
        let cursor = mouse::Cursor::Available(BOUNDS.center());
        let _ = update(
            &mut widget,
            &mut tree,
            &node,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
            cursor,
        );
        assert!(tree.state.downcast_ref::<State>().smooth_motion.is_some());
        let _ = update(&mut widget, &mut tree, &node, interruption.clone(), cursor);
        assert!(tree.state.downcast_ref::<State>().smooth_motion.is_none());
        if matches!(
            interruption,
            Event::Mouse(mouse::Event::WheelScrolled { .. })
        ) {
            assert_eq!(
                tree.state.downcast_ref::<State>().offset_y,
                Offset::Absolute(12.0)
            );
        }
    }
}

#[test]
fn wheel_at_boundary_bubbles_to_parent_without_scheduling_animation() {
    let mut widget = smooth_widget();
    let mut tree = Tree::empty();
    tree.diff(&mut widget as &mut dyn Widget<(), Theme, ()>);
    let node = widget.layout(
        &mut tree,
        &(),
        &layout::Limits::new(Size::ZERO, BOUNDS.size()),
    );
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    widget.update(
        &mut tree,
        &Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        }),
        Layout::new(&node),
        mouse::Cursor::Available(BOUNDS.center()),
        &(),
        &mut shell,
        &BOUNDS,
    );
    assert!(!shell.is_event_captured());
    assert!(tree.state.downcast_ref::<State>().smooth_motion.is_none());
}

#[test]
fn continuous_input_stays_direct_through_whole_line_packets_and_resets_after_pause() {
    let now = Instant::now();
    let line = |y| mouse::ScrollDelta::Lines { x: 0.0, y };
    let mut input = WheelScrollInput::default();
    assert!(!input.should_animate(line(-0.125), now));
    assert!(!input.should_animate(line(-1.0), now + Duration::from_millis(60)));
    assert!(!input.should_animate(line(1.0), now + Duration::from_millis(120)));
    assert!(input.should_animate(line(-1.0), now + Duration::from_millis(400)));
    assert!(input.should_animate(line(-1.0), now + Duration::from_millis(460)));
    assert!(input.should_animate(line(-1.0), now + Duration::from_millis(470)));
    assert!(input.should_animate(line(-1.0), now + Duration::from_millis(570)));
    assert!(!input.should_animate(
        mouse::ScrollDelta::Pixels { x: 0.0, y: -1.0 },
        now + Duration::from_millis(900)
    ));
    assert!(!input.should_animate(line(-1.0), now + Duration::from_millis(950)));
    assert!(!input.should_animate(
        mouse::ScrollDelta::Lines { x: 0.25, y: 0.0 },
        now + Duration::from_millis(1300)
    ));
}

#[test]
fn fractional_touchpad_streams_move_immediately_without_easing_or_a_tail() {
    for horizontal in [false, true] {
        let mut widget = smooth_widget();
        let mut tree = Tree::empty();
        tree.diff(&mut widget as &mut dyn Widget<(), Theme, ()>);
        let node = widget.layout(
            &mut tree,
            &(),
            &layout::Limits::new(Size::ZERO, BOUNDS.size()),
        );
        let cursor = mouse::Cursor::Available(BOUNDS.center());
        let mut expected = 0.0;
        for delta in [-0.125, -0.25, -1.0, -0.125, 0.25] {
            let (x, y) = if horizontal {
                (delta, 0.0)
            } else {
                (0.0, delta)
            };
            let _ = update(
                &mut widget,
                &mut tree,
                &node,
                Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines { x, y },
                }),
                cursor,
            );
            expected -= delta * 60.0;
            let state = tree.state.downcast_ref::<State>();
            assert_eq!(
                if horizontal {
                    state.offset_x
                } else {
                    state.offset_y
                },
                Offset::Absolute(expected)
            );
            assert!(state.smooth_motion.is_none());
        }
        let _ = update(
            &mut widget,
            &mut tree,
            &node,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_secs(1),
            )),
            cursor,
        );
        let state = tree.state.downcast_ref::<State>();
        assert_eq!(
            if horizontal {
                state.offset_x
            } else {
                state.offset_y
            },
            Offset::Absolute(expected)
        );
        assert!(state.smooth_motion.is_none());
    }
}

#[test]
fn precise_input_interrupts_at_displayed_position_without_flushing_pending_distance() {
    for next in [-0.25, 0.25] {
        let mut widget = smooth_widget();
        let mut tree = Tree::empty();
        tree.diff(&mut widget as &mut dyn Widget<(), Theme, ()>);
        let node = widget.layout(
            &mut tree,
            &(),
            &layout::Limits::new(Size::ZERO, BOUNDS.size()),
        );
        let cursor = mouse::Cursor::Available(BOUNDS.center());
        let _ = update(
            &mut widget,
            &mut tree,
            &node,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
            }),
            cursor,
        );
        let started = tree
            .state
            .downcast_ref::<State>()
            .smooth_motion
            .unwrap()
            .started;
        let _ = update(
            &mut widget,
            &mut tree,
            &node,
            Event::Window(window::Event::RedrawRequested(
                started + Duration::from_millis(40),
            )),
            cursor,
        );
        let before = tree
            .state
            .downcast_ref::<State>()
            .offset_y
            .absolute(100.0, 1000.0);
        assert!(before > 15.0 && before < 180.0);
        let _ = update(
            &mut widget,
            &mut tree,
            &node,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: next },
            }),
            cursor,
        );
        let expected = before - next * 60.0;
        assert_eq!(
            tree.state.downcast_ref::<State>().offset_y,
            Offset::Absolute(expected)
        );
        assert!(tree.state.downcast_ref::<State>().smooth_motion.is_none());
        let _ = update(
            &mut widget,
            &mut tree,
            &node,
            Event::Window(window::Event::RedrawRequested(
                started + Duration::from_secs(1),
            )),
            cursor,
        );
        assert_eq!(
            tree.state.downcast_ref::<State>().offset_y,
            Offset::Absolute(expected)
        );
    }
}

#[test]
fn mouse_wheel_speed_and_batched_delivery_never_change_easing_mode() {
    let now = Instant::now();
    let mut input = WheelScrollInput::default();
    for milliseconds in [0, 0, 8, 24, 63, 104, 165, 200, 500, 501] {
        assert!(input.should_animate(
            mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            now + Duration::from_millis(milliseconds)
        ));
    }
}
