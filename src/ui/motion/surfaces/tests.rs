//! Input, focus, paint, and timing behavior of surface animations.

use super::*;
use crate::ui::motion::test_support::*;
use iced::advanced::renderer::Headless;
use iced::advanced::widget::operation::{self, Operation, Outcome, focusable};
use iced::widget::{Space, button, container, text_input};
use iced::{Fill, Point};

#[test]
fn mounting_preserves_sizing_and_moves_clickable_child_bounds() {
    let renderer = renderer();
    let mut content = popup(
        button(Space::new().width(Fill).height(20))
            .width(Fill)
            .padding(0)
            .on_press(Message::None),
    );
    assert_eq!(content.as_widget().size().width, Fill);
    let (mut tree, node) = mount(&mut content, &renderer);
    assert_eq!(tree.children.len(), 1);
    assert_eq!(node.size(), Size::new(320.0, 20.0));
    assert_eq!(Layout::new(&node).child(0).bounds().y, 8.0);

    // y=25 is outside the final button bounds but inside its moving bounds.
    let cursor = mouse::Cursor::Available(Point::new(10.0, 25.0));
    for pressed in [true, false] {
        let event = if pressed {
            mouse::Event::ButtonPressed(mouse::Button::Left)
        } else {
            mouse::Event::ButtonReleased(mouse::Button::Left)
        };
        let (_, messages) = dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(event),
            cursor,
        );
        assert_eq!(messages.len(), usize::from(!pressed));
    }
}

#[test]
fn zero_opacity_keeps_input_focus_operations_reachable() {
    let renderer = renderer();
    let id = widget::Id::new("motion-find-input");
    let mut content = fade(
        container(text_input("Find", "").id(id.clone())),
        0.0,
        |_| Color::WHITE,
        false,
    );
    let (mut tree, node) = mount(&mut content, &renderer);
    content.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut focusable::focus::<()>(id),
    );
    let mut count = focusable::count();
    content.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut operation::black_box(&mut count),
    );
    match count.finish() {
        Outcome::Some(count) => {
            assert_eq!(count.total, 1);
            assert_eq!(count.focused, Some(0));
        }
        _ => panic!("focus traversal must produce a count"),
    }
}

#[test]
fn find_panel_keeps_initial_focus_while_expanding_from_zero_height() {
    use crate::core::FindState;
    use crate::ui::{find_panel, styles};
    use iced::keyboard::{self, Key, Location, Modifiers, key};

    let renderer = renderer();
    let find = FindState::default();
    let build = |progress| -> Element<'_, Message> {
        container(fade(
            find_panel::view(&find, false, false, 0.0),
            progress,
            styles::utility_bar_background,
            true,
        ))
        .height(Length::Fixed(46.0 * progress))
        .width(Fill)
        .clip(true)
        .into()
    };
    let mut content = build(0.0);
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());
    let limits = layout::Limits::new(Size::ZERO, Size::new(1200.0, 240.0));
    let node = content
        .as_widget_mut()
        .layout(&mut tree, &renderer, &limits);
    assert_eq!(node.size().height, 0.0);
    content.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut focusable::focus::<()>(widget::Id::new(find_panel::FIND_INPUT_ID)),
    );

    // Application animation frames rebuild the view and reconcile its tree.
    for progress in [0.2, 0.7, 1.0] {
        content = build(progress);
        tree.diff(content.as_widget_mut());
        content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
    }
    let node = content
        .as_widget_mut()
        .layout(&mut tree, &renderer, &limits);
    assert_eq!(node.size().height, 46.0);
    let (_, messages) = dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character("t".into()),
            modified_key: Key::Character("t".into()),
            physical_key: key::Physical::Code(key::Code::KeyT),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: Some("t".into()),
            repeat: false,
        }),
        mouse::Cursor::Unavailable,
    );
    assert!(
        messages
            .iter()
            .any(|message| { matches!(message, Message::FindQueryChanged(query) if query == "t") }),
        "find input must accept typing after its initial zero-height focus: {messages:?}"
    );
}

#[test]
fn closing_fade_suppresses_clicks_and_pointer_interaction() {
    let renderer = renderer();
    for interactive in [false, true] {
        let mut content = fade(
            button("Action").on_press(Message::None),
            0.5,
            |_| Color::WHITE,
            interactive,
        );
        let (mut tree, node) = mount(&mut content, &renderer);
        let cursor = mouse::Cursor::Available(Point::new(10.0, 10.0));
        let interaction = content.as_widget().mouse_interaction(
            &tree,
            Layout::new(&node),
            cursor,
            &VIEWPORT,
            &renderer,
        );
        assert_eq!(
            interaction,
            if interactive {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::None
            }
        );
        let mut emitted = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            let (_, messages) = dispatch(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(event),
                cursor,
            );
            emitted.extend(messages);
        }
        assert_eq!(emitted.len(), usize::from(interactive));
    }
}

#[test]
fn disclosure_reverses_from_current_height_and_stops_requesting_frames() {
    let renderer = renderer();
    let build = |expanded| {
        reveal(
            button(Space::new().width(Fill).height(40))
                .padding(0)
                .width(Fill)
                .on_press(Message::None),
            expanded,
            |_| Color::WHITE,
        )
    };
    let mut content = build(false);
    let (mut tree, mut node) = mount(&mut content, &renderer);
    assert_eq!(node.size().height, 0.0);
    let start = Instant::now();
    let frame = |at| Event::Window(window::Event::RedrawRequested(at));
    assert_eq!(
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            frame(start),
            mouse::Cursor::Unavailable
        )
        .0,
        window::RedrawRequest::Wait
    );

    content = build(true);
    tree.diff(content.as_widget_mut());
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        frame(start),
        mouse::Cursor::Unavailable,
    );
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        frame(start + REVEAL_DURATION / 2),
        mouse::Cursor::Unavailable,
    );
    node = relayout(&mut content, &mut tree, &renderer);
    let opening_height = node.size().height;
    assert!(opening_height > 0.0 && opening_height < 40.0);
    // The child keeps a 40px layout, but its unrevealed edge is not clickable.
    let outside = mouse::Cursor::Available(Point::new(10.0, (opening_height + 40.0) / 2.0));
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        assert!(
            dispatch(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(event),
                outside
            )
            .1
            .is_empty()
        );
    }

    content = build(false);
    tree.diff(content.as_widget_mut());
    node = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(
        node.size().height,
        opening_height,
        "reversing must not jump"
    );
    let close_start = start + REVEAL_DURATION / 2;
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        frame(close_start),
        mouse::Cursor::Unavailable,
    );
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        frame(close_start + REVEAL_DURATION / 2),
        mouse::Cursor::Unavailable,
    );
    node = relayout(&mut content, &mut tree, &renderer);
    let closing_height = node.size().height;
    assert!(closing_height > 0.0 && closing_height < opening_height);

    content = build(true);
    tree.diff(content.as_widget_mut());
    node = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(node.size().height, closing_height);
    let reopen = close_start + REVEAL_DURATION / 2;
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        frame(reopen),
        mouse::Cursor::Unavailable,
    );
    dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        frame(reopen + REVEAL_DURATION),
        mouse::Cursor::Unavailable,
    );
    node = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(node.size().height, 40.0);
    assert_eq!(
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            frame(reopen + REVEAL_DURATION * 2),
            mouse::Cursor::Unavailable
        )
        .0,
        window::RedrawRequest::Wait
    );
}

#[test]
fn collapsed_disclosures_exclude_inputs_from_focus_traversal() {
    let renderer = renderer();
    let id = widget::Id::new("disclosure-field");
    let build = |expanded| {
        reveal(text_input("Replace", "").id(id.clone()), expanded, |_| {
            Color::WHITE
        })
    };
    let mut content = build(true);
    let (mut tree, mut node) = mount(&mut content, &renderer);
    content.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut focusable::focus::<()>(id.clone()),
    );
    content = build(false);
    tree.diff(content.as_widget_mut());
    node = relayout(&mut content, &mut tree, &renderer);
    let mut count = focusable::count();
    content.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut operation::black_box(&mut count),
    );
    assert!(matches!(
        count.finish(),
        Outcome::Some(focusable::Count {
            total: 0,
            focused: None
        })
    ));
    content = build(true);
    tree.diff(content.as_widget_mut());
    node = relayout(&mut content, &mut tree, &renderer);
    let mut count = focusable::count();
    content.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut operation::black_box(&mut count),
    );
    assert!(matches!(
        count.finish(),
        Outcome::Some(focusable::Count {
            total: 1,
            focused: None
        })
    ));
}

#[test]
fn fade_covers_text_icons_quads_and_nested_layers() {
    use crate::ui::icons::hero::{self, HeroIcon, IconTone};
    use iced::widget::{row, stack, text};

    let mut renderer = renderer();
    let mut render = |progress| {
        let black = || {
            container(Space::new().width(64).height(32))
                .style(|_| container::Style::default().background(Color::BLACK))
        };
        let mut content = fade(
            row![
                text("Fade").size(20).width(64).color(Color::BLACK),
                container(hero::icon(HeroIcon::Plus, 24, IconTone::Text)).width(64),
                black(),
                stack![Space::new().width(64).height(32), black()],
            ]
            .height(32),
            progress,
            |_| Color::WHITE,
            false,
        );
        let (tree, node) = mount(&mut content, &renderer);
        renderer.reset(VIEWPORT);
        content.as_widget().draw(
            &tree,
            &mut renderer,
            &Theme::Light,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &VIEWPORT,
        );
        renderer.screenshot(Size::new(320, 240), 1.0, Color::WHITE)
    };

    let hidden = render(0.0);
    let halfway = render(0.5);
    let visible = render(1.0);
    let contrast = |pixels: &[u8], region: usize| -> u64 {
        (0..32)
            .flat_map(|y| (region * 64..(region + 1) * 64).map(move |x| (y * 320 + x) * 4))
            .map(|offset| {
                pixels[offset..offset + 3]
                    .iter()
                    .map(|v| u64::from(255 - v))
                    .sum::<u64>()
            })
            .sum()
    };
    for (region, name) in ["text", "icon", "quad", "nested layer"]
        .into_iter()
        .enumerate()
    {
        let full = contrast(&visible, region);
        let half = contrast(&halfway, region);
        assert!(full > 0, "{name} must be visible at full opacity");
        assert_eq!(
            contrast(&hidden, region),
            0,
            "{name} must disappear at zero opacity"
        );
        assert!(
            half > full / 5 && half < full * 4 / 5,
            "{name} must blend at intermediate opacity: {half}/{full}"
        );
    }
}

#[test]
fn entrance_settles_without_redraw_and_restarts_only_for_changed_key() {
    let renderer = renderer();
    let make_menu = |key| dropdown_with_key(key, Space::new().width(100).height(40));
    let mut content = make_menu("File");
    let (mut tree, mut node) = mount(&mut content, &renderer);
    let started = Instant::now();
    for (elapsed, expected) in [
        (Duration::ZERO, window::RedrawRequest::NextFrame),
        (ENTRANCE_DURATION / 2, window::RedrawRequest::NextFrame),
        (ENTRANCE_DURATION, window::RedrawRequest::Wait),
        (ENTRANCE_DURATION * 2, window::RedrawRequest::Wait),
    ] {
        let (redraw, _) = dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(started + elapsed)),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(redraw, expected);
        node = relayout(&mut content, &mut tree, &renderer);
    }
    assert_eq!(Layout::new(&node).child(0).bounds().y, 0.0);

    content = make_menu("File");
    tree.diff(content.as_widget_mut());
    node = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(Layout::new(&node).child(0).bounds().y, 0.0);

    content = make_menu("Edit");
    tree.diff(content.as_widget_mut());
    node = relayout(&mut content, &mut tree, &renderer);
    assert_eq!(Layout::new(&node).child(0).bounds().y, -5.0);
    let (redraw, _) = dispatch(
        &mut content,
        &mut tree,
        &node,
        &renderer,
        Event::Window(window::Event::RedrawRequested(
            started + ENTRANCE_DURATION * 3,
        )),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(redraw, window::RedrawRequest::NextFrame);
}
