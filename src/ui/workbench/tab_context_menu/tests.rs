use super::*;
use iced::Point;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::Headless;
use iced::advanced::widget::{self, operation};
use iced::keyboard::{self, Key, key::Named};

const VIEWPORT: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 420.0,
    height: 500.0,
};

fn tab_row<'a>(workspace: &'a Workspace, settings: &'a EditorSettings) -> Element<'a, Message> {
    let editor = workspace.current_window_id();
    wrap(
        tabs::view_for_window(workspace, editor, None, None),
        workspace,
        editor,
        settings,
        Vec::new(),
        false,
    )
}

fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("CPU renderer")
}

fn mount(element: &mut Element<'_, Message>, renderer: &Renderer) -> (Tree, layout::Node) {
    let mut tree = Tree::empty();
    tree.diff(element.as_widget_mut());
    let node = element
        .as_widget_mut()
        .layout(
            &mut tree,
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, VIEWPORT.height)),
        )
        .move_to(Point::new(30.0, 50.0));
    (tree, node)
}

fn right_click(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    point: Point,
) -> (bool, Vec<Message>) {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    element.as_widget_mut().update(
        tree,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        Layout::new(node),
        mouse::Cursor::Available(point),
        renderer,
        &mut shell,
        &VIEWPORT,
    );
    (shell.is_event_captured(), messages)
}

fn geometry(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
) -> tabs::StripGeometry {
    tabs::geometry(element, tree, Layout::new(node), renderer, &VIEWPORT)
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

#[test]
fn tab_context_menu_targets_inactive_tab_controls_and_dispatches_with_keyboard() {
    let _guard = crate::font_system_test_guard();
    let renderer = renderer();
    let mut workspace = Workspace::new();
    let target = workspace.active_document_id();
    let active = workspace.create_untitled();
    let editor = workspace.current_window_id();
    let selection = workspace.window(editor).unwrap().selected_tabs.clone();
    let settings = EditorSettings::default();
    let mut element = tab_row(&workspace, &settings);
    let (mut tree, node) = mount(&mut element, &renderer);
    let geometry = geometry(&mut element, &mut tree, &node, &renderer);
    let bounds = geometry
        .tabs
        .iter()
        .find(|(id, _)| *id == target)
        .unwrap()
        .1;
    // The close button is part of the context target without becoming a close click.
    let point = Point::new(bounds.x + bounds.width - 8.0, bounds.center_y());
    let (captured, messages) = right_click(&mut element, &mut tree, &node, &renderer, point);
    assert!(captured);
    assert!(matches!(messages.as_slice(), [Message::MenuClosed]));
    let state = tree.state.downcast_ref::<State>();
    assert_eq!(state.target, Some(target));
    assert_eq!(state.popup.anchor(), Some(point - Vector::new(30.0, 50.0)));
    assert_eq!(workspace.active_document_id(), active);
    assert_eq!(workspace.window(editor).unwrap().selected_tabs, selection);

    let mut messages = Vec::new();
    {
        let mut popup = element
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &VIEWPORT,
                Vector::ZERO,
            )
            .expect("tab popup");
        let mut menu_node = popup.as_overlay_mut().layout(&renderer, VIEWPORT.size());
        let bounds = Layout::new(&menu_node).child(0).bounds();
        assert_eq!(bounds.y, point.y);
        assert_eq!(
            bounds.x,
            point.x.clamp(8.0, VIEWPORT.width - bounds.width - 8.0)
        );
        assert!(bounds.height > node.size().height);
        assert!(bounds.x + bounds.width <= VIEWPORT.width);
        assert!(bounds.y + bounds.height <= VIEWPORT.height);
        let now = iced::time::Instant::now();
        for instant in [now, now + std::time::Duration::from_millis(200)] {
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            popup.as_overlay_mut().update(
                &Event::Window(window::Event::RedrawRequested(instant)),
                Layout::new(&menu_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
            );
            assert!(!shell.is_event_captured());
        }
        for named in [Named::ArrowDown, Named::Enter] {
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            popup.as_overlay_mut().update(
                &key(named),
                Layout::new(&menu_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
            );
            assert!(shell.is_event_captured());
            menu_node = popup.as_overlay_mut().layout(&renderer, VIEWPORT.size());
        }
        let now = iced::time::Instant::now();
        for instant in [now, now + std::time::Duration::from_millis(200)] {
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            popup.as_overlay_mut().update(
                &Event::Window(window::Event::RedrawRequested(instant)),
                Layout::new(&menu_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
            );
            assert!(!shell.is_event_captured());
        }
    }
    assert!(
        matches!(messages.as_slice(), [Message::TabContextAction(id, TabContextAction::Close)] if *id == target)
    );
    assert!(!tree.state.downcast_ref::<State>().popup.is_open());
}

struct ScrollTo(f32);

impl Operation for ScrollTo {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        _id: Option<&widget::Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        _translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        state.scroll_to(iced::widget::scrollable::AbsoluteOffset {
            x: Some(self.0),
            y: None,
        });
    }
}

#[test]
fn tab_context_menu_hit_testing_uses_scrolled_visible_bounds_and_closes_for_removed_target() {
    let _guard = crate::font_system_test_guard();
    let renderer = renderer();
    let mut workspace = Workspace::new();
    for _ in 0..7 {
        workspace.create_untitled();
    }
    let settings = EditorSettings::default();
    let mut element = tab_row(&workspace, &settings);
    let (mut tree, node) = mount(&mut element, &renderer);
    element.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut ScrollTo(180.0),
    );
    let geometry = geometry(&mut element, &mut tree, &node, &renderer);
    let (target, bounds) = geometry
        .tabs
        .iter()
        .find(|(_, bounds)| {
            bounds.x < geometry.bounds.x && bounds.x + bounds.width > geometry.bounds.x
        })
        .copied()
        .expect("partly clipped tab");
    let (captured, _) = right_click(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Point::new(geometry.bounds.x - 1.0, bounds.center_y()),
    );
    assert!(!captured);
    assert!(!tree.state.downcast_ref::<State>().popup.is_open());
    right_click(
        &mut element,
        &mut tree,
        &node,
        &renderer,
        Point::new(geometry.bounds.x + 1.0, bounds.center_y()),
    );
    assert_eq!(tree.state.downcast_ref::<State>().target, Some(target));
    drop(element);

    workspace.close(target).unwrap();
    let mut element = tab_row(&workspace, &settings);
    tree.diff(element.as_widget_mut());
    let state = tree.state.downcast_ref::<State>();
    assert!(state.target.is_none());
    assert!(!state.popup.is_open());
}

#[test]
fn tab_context_menu_disables_unavailable_groups_and_uses_target_pin_state() {
    let mut workspace = Workspace::new();
    let id = workspace.active_document_id();
    workspace.toggle_pin(id);
    let settings = EditorSettings::default();
    let items = entries(
        &workspace,
        workspace.current_window_id(),
        id,
        &settings,
        &[],
    );
    for label in [
        "Close Other Tabs",
        "Close Tabs to the Left",
        "Close Tabs to the Right",
        "Close All but Pinned Tabs",
        "Copy Full Path",
        "Move Tab to New Window",
    ] {
        assert!(
            items.iter().any(
                |entry| matches!(entry, MenuNode::Disabled { label: actual, .. } if actual == label)
            ),
            "{label}"
        );
    }
    assert!(items.iter().any(|entry| matches!(entry, MenuNode::Item { label, message: Message::TabContextAction(target, TabContextAction::TogglePin), .. } if label == "Unpin Tab" && *target == id)));
}

#[test]
fn tab_context_menu_keeps_editor_ime_disabled_and_releases_selection_modifiers() {
    use iced_runtime::user_interface::{self, UserInterface};

    let _guard = crate::font_system_test_guard();
    let mut renderer = renderer();
    let mut workspace = Workspace::new();
    workspace.create_untitled();
    let editor = workspace.current_window_id();
    let settings = EditorSettings::default();
    let content = iced::widget::column![
        tabs::view_for_window(&workspace, editor, None, None),
        crate::ui::editor::view(workspace.active_document().unwrap(), &settings),
    ]
    .into();
    let mut ui = UserInterface::build(
        wrap(content, &workspace, editor, &settings, Vec::new(), false),
        VIEWPORT.size(),
        user_interface::Cache::new(),
        &mut renderer,
    );
    let mut dispatch = |event, cursor| {
        let mut messages = Vec::new();
        let (state, statuses) = ui.update(
            &window::Headless,
            &Waker::noop(),
            &[event],
            cursor,
            &mut renderer,
            &mut messages,
        );
        let ime_enabled = match state {
            user_interface::State::Updated { input_method, .. } => Some(input_method.is_enabled()),
            user_interface::State::Outdated => None,
        };
        (ime_enabled, statuses, messages)
    };
    let press = |button| Event::Mouse(mouse::Event::ButtonPressed(button));
    let frame = || Event::Window(window::Event::RedrawRequested(iced::time::Instant::now()));
    let modifiers = |value| Event::Keyboard(keyboard::Event::ModifiersChanged(value));
    let no_cursor = mouse::Cursor::Unavailable;
    dispatch(
        press(mouse::Button::Left),
        mouse::Cursor::Available(Point::new(150.0, 100.0)),
    );
    assert_eq!(dispatch(frame(), no_cursor).0, Some(true));
    dispatch(modifiers(keyboard::Modifiers::CTRL), no_cursor);
    let tab_cursor = mouse::Cursor::Available(Point::new(20.0, 13.0));
    assert!(
        dispatch(press(mouse::Button::Right), tab_cursor)
            .2
            .iter()
            .any(|message| matches!(message, Message::MenuClosed))
    );
    let (ime_enabled, statuses, _) = dispatch(frame(), no_cursor);
    assert_eq!(ime_enabled, Some(false));
    assert_eq!(
        statuses,
        vec![iced::event::Status::Ignored],
        "capturing a base redraw hides Iced's rendered overlay"
    );
    dispatch(
        Event::Window(window::Event::RedrawRequested(
            iced::time::Instant::now() + std::time::Duration::from_millis(200),
        )),
        no_cursor,
    );
    assert_eq!(
        dispatch(modifiers(keyboard::Modifiers::empty()), no_cursor).0,
        Some(false)
    );
    dispatch(key(Named::Escape), no_cursor);
    let now = iced::time::Instant::now();
    assert_eq!(
        dispatch(
            Event::Window(window::Event::RedrawRequested(now)),
            no_cursor
        )
        .0,
        Some(false)
    );
    dispatch(
        Event::Window(window::Event::RedrawRequested(
            now + std::time::Duration::from_millis(200),
        )),
        no_cursor,
    );
    assert_eq!(dispatch(frame(), no_cursor).0, Some(true));
    assert!(dispatch(press(mouse::Button::Left), tab_cursor).2.iter().any(
        |message| matches!(message, Message::TabPressed(_, _, modifiers) if modifiers.is_empty())
    ));
}
