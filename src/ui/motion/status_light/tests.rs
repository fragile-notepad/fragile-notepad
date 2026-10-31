//! Status light scheduling, interrupted transitions, and stroke geometry.

use super::*;
use crate::ui::motion::test_support::*;
use iced::Point;
use iced::advanced::graphics::core::shell::Waker;

#[test]
fn status_light_schedules_bounded_frames_and_leaves_input_uncaptured() {
    let renderer = renderer();
    let start = Instant::now();
    for (state, interval) in [
        (StatusLightState::Idle, Some(STATUS_IDLE_FRAME)),
        (StatusLightState::Searching, Some(STATUS_SEARCH_FRAME)),
        (StatusLightState::Success, None),
        (StatusLightState::Error, None),
    ] {
        let mut content = status_light(state);
        let (mut tree, node) = mount(&mut content, &renderer);
        assert_eq!(node.size(), Size::new(12.0, 12.0));
        let (request, messages) = dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(start)),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(
            request,
            interval.map_or(window::RedrawRequest::Wait, |frame| {
                window::RedrawRequest::At(start + frame)
            })
        );
        assert!(messages.is_empty());
        if let Some(frame) = interval {
            let (request, _) = dispatch(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(start + frame / 2)),
                mouse::Cursor::Unavailable,
            );
            assert_eq!(request, window::RedrawRequest::At(start + frame));
        }
        for event in [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
                iced::keyboard::Modifiers::SHIFT,
            )),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            content.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(Point::new(6.0, 6.0)),
                &renderer,
                &mut shell,
                &VIEWPORT,
            );
            assert!(!shell.is_event_captured());
            assert!(messages.is_empty());
        }
    }
}

#[test]
fn status_light_pauses_unfocused_and_stays_paused_across_state_changes() {
    let renderer = renderer();
    let mut content = status_light(StatusLightState::Idle);
    let (mut tree, node) = mount(&mut content, &renderer);
    let start = Instant::now();
    for at in [start, start + Duration::from_millis(1200)] {
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(at)),
            mouse::Cursor::Unavailable,
        );
    }
    let paused = tree.state.downcast_ref::<StatusLightAnimation>().elapsed;
    assert_eq!(paused, Duration::from_millis(1200));
    let (request, _) = dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::Unfocused),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(request, window::RedrawRequest::Wait);
    let (request, _) = dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            start + Duration::from_secs(5),
        )),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(request, window::RedrawRequest::Wait);
    assert_eq!(
        tree.state.downcast_ref::<StatusLightAnimation>().elapsed,
        paused
    );
    content = status_light(StatusLightState::Searching);
    tree.diff(content.as_widget_mut());
    assert!(!tree.state.downcast_ref::<StatusLightAnimation>().focused);
    let resume = start + Duration::from_secs(6);
    assert_eq!(
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(resume)),
            mouse::Cursor::Unavailable,
        )
        .0,
        window::RedrawRequest::Wait
    );
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::Focused),
        mouse::Cursor::Unavailable,
    );
    let (request, _) = dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(resume)),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(
        request,
        window::RedrawRequest::At(resume + STATUS_SEARCH_FRAME)
    );
    assert_eq!(
        tree.state.downcast_ref::<StatusLightAnimation>().elapsed,
        paused
    );
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            resume + STATUS_TRANSITION / 2,
        )),
        mouse::Cursor::Unavailable,
    );
    let blend = tree.state.downcast_ref::<StatusLightAnimation>().weights;
    let phase = tree.state.downcast_ref::<StatusLightAnimation>().elapsed;
    let geometry = StatusLightGeometry::from_animation(tree.state.downcast_ref());
    assert_eq!(blend, [0.5, 0.5, 0.0, 0.0]);
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::Unfocused),
        mouse::Cursor::Unavailable,
    );
    let later = resume + Duration::from_secs(20);
    assert_eq!(
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(later)),
            mouse::Cursor::Unavailable,
        )
        .0,
        window::RedrawRequest::Wait
    );
    assert_eq!(
        tree.state.downcast_ref::<StatusLightAnimation>().weights,
        blend
    );
    assert_eq!(
        tree.state.downcast_ref::<StatusLightAnimation>().elapsed,
        phase
    );
    assert_eq!(
        StatusLightGeometry::from_animation(tree.state.downcast_ref()),
        geometry
    );
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::Focused),
        mouse::Cursor::Unavailable,
    );
    for at in [later, later + STATUS_TRANSITION / 2] {
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(at)),
            mouse::Cursor::Unavailable,
        );
    }
    assert_eq!(
        tree.state.downcast_ref::<StatusLightAnimation>().weights,
        StatusLightState::Searching.weights()
    );
    content = status_light(StatusLightState::Success);
    tree.diff(content.as_widget_mut());
    let finish_start = later + STATUS_TRANSITION / 2;
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(finish_start)),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                finish_start + STATUS_TRANSITION
            )),
            mouse::Cursor::Unavailable,
        )
        .0,
        window::RedrawRequest::Wait
    );
    assert_eq!(
        tree.state.downcast_ref::<StatusLightAnimation>().weights,
        StatusLightState::Success.weights()
    );
    let mut shell_messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut shell_messages);
    content = status_light(StatusLightState::Idle);
    tree.diff(content.as_widget_mut());
    content.as_widget_mut().update(
        &mut tree,
        &Event::Window(window::Event::RedrawRequested(resume)),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut shell,
        &Rectangle::new(Point::new(40.0, 40.0), Size::new(20.0, 20.0)),
    );
    assert_eq!(
        shell.redraw_request(),
        window::RedrawRequest::Wait,
        "offscreen indicators must not schedule redraws"
    );
}

#[test]
fn status_light_retargets_from_the_visible_blend_and_finishes_at_rest() {
    let mut animation = StatusLightAnimation::new(StatusLightState::Idle);
    animation.advance(Duration::from_millis(600));
    let phase = animation.elapsed;
    animation.retarget(StatusLightState::Searching);
    assert_eq!(
        animation.elapsed, phase,
        "state changes must preserve the motion phase"
    );
    animation.advance(STATUS_TRANSITION / 2);
    assert_eq!(animation.weights, [0.5, 0.5, 0.0, 0.0]);
    let visible = animation.weights;
    let geometry = StatusLightGeometry::from_animation(&animation);
    animation.retarget(StatusLightState::Idle);
    assert_eq!(
        animation.weights, visible,
        "reversing must preserve the current appearance"
    );
    assert_eq!(
        StatusLightGeometry::from_animation(&animation),
        geometry,
        "reversing must preserve the current shape and angle"
    );
    animation.advance(STATUS_TRANSITION / 2);
    assert_eq!(animation.weights, [0.75, 0.25, 0.0, 0.0]);
    let visible = animation.weights;
    animation.retarget(StatusLightState::Error);
    assert_eq!(animation.weights, visible);
    animation.advance(STATUS_TRANSITION / 2);
    assert_eq!(animation.weights, [0.375, 0.125, 0.0, 0.5]);
    let visible = animation.weights;
    animation.retarget(StatusLightState::Success);
    assert_eq!(animation.weights, visible);
    assert_eq!(animation.frame_interval(), Some(STATUS_SEARCH_FRAME));
    animation.advance(STATUS_TRANSITION);
    assert_eq!(animation.weights, StatusLightState::Success.weights());
    assert!(!animation.transitioning);
    assert_eq!(animation.frame_interval(), None);
    animation.retarget(StatusLightState::Error);
    animation.advance(STATUS_TRANSITION);
    assert_eq!(animation.weights, StatusLightState::Error.weights());
    assert_eq!(animation.frame_interval(), None);
}

#[test]
fn status_light_geometry_unfurls_one_connected_stroke_inside_its_bounds() {
    let center = iced::Point::new(6.0, 6.0);
    let mut animation = StatusLightAnimation::new(StatusLightState::Idle);
    let dot = StatusLightGeometry::from_animation(&animation);
    assert_eq!(dot.centerline_radius, 0.0);
    assert_eq!(dot.sweep, 0.0);
    assert_eq!(dot.sample_count, 1);
    assert_eq!(dot.sample(center, 0).0, center);
    let mut previous_radius = 0.0;
    let mut previous_stroke = dot.stroke_radius;
    for progress in [0.0, 0.1, 0.25, 0.5, 0.75, 1.0] {
        animation.weights = [1.0 - progress, progress, 0.0, 0.0];
        let geometry = StatusLightGeometry::from_animation(&animation);
        assert!(geometry.centerline_radius >= previous_radius);
        assert!(geometry.stroke_radius <= previous_stroke);
        assert!((1..=40).contains(&geometry.sample_count));
        assert!(geometry.glow_radius < 6.0);
        previous_radius = geometry.centerline_radius;
        previous_stroke = geometry.stroke_radius;
        let mut previous = None;
        for index in 0..geometry.sample_count {
            let (point, radius) = geometry.sample(center, index);
            assert!(point.x - radius >= 0.0 && point.x + radius <= 12.0);
            assert!(point.y - radius >= 0.0 && point.y + radius <= 12.0);
            if let Some((prior, prior_radius)) = previous {
                let delta: iced::Vector = point - prior;
                assert!(
                    (delta.x * delta.x + delta.y * delta.y).sqrt() < radius + prior_radius,
                    "neighboring stamps must join into one continuous stroke"
                );
            }
            previous = Some((point, radius));
        }
    }
    let spinner = StatusLightGeometry::from_animation(&animation);
    assert_eq!(spinner.centerline_radius, 4.0);
    assert_eq!(spinner.stroke_radius, 0.9);
    assert_eq!(spinner.sweep, std::f32::consts::TAU * 2.0 / 3.0);
    assert!(spinner.sample(center, 0).1 < spinner.sample(center, spinner.sample_count - 1).1);
    animation.weights = StatusLightState::Success.weights();
    let complete = StatusLightGeometry::from_animation(&animation);
    assert_eq!(complete.sample_count, 1);
    assert_eq!(complete.sample(center, 0), (center, 3.0));
    animation.weights = StatusLightState::Searching.weights();
    animation.elapsed = Duration::from_secs(1_000_000);
    let angle = StatusLightGeometry::from_animation(&animation).head_angle;
    animation.elapsed += STATUS_SEARCH_FRAME;
    let next_angle = StatusLightGeometry::from_animation(&animation).head_angle;
    assert!(
        (next_angle - angle - std::f32::consts::TAU * 0.033).abs() < 0.0001,
        "long-lived windows must preserve fractional rotation phase"
    );
}
