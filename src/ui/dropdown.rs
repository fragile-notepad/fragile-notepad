//! Settings dropdowns with a stationary trigger and an animated option surface.

use std::cell::Cell;
use std::rc::Rc;

use iced::advanced::widget::{self, Operation, Tree, operation, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Center, Element, Event, Fill, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::message::Message;
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::{motion, styles};

const ROW_HEIGHT: f32 = 30.0;
const MENU_PADDING: f32 = 4.0;
const MENU_MAX_HEIGHT: f32 = 280.0;
const EDGE_MARGIN: f32 = 8.0;

pub fn dropdown<'a, T: Clone + PartialEq + 'a>(
    selected: Option<T>,
    options: &'a [T],
    label: fn(&T) -> String,
    on_select: impl Fn(T) -> Message + 'a,
) -> Dropdown<'a, T> {
    Dropdown {
        selected,
        options,
        label,
        on_select: Box::new(on_select),
        width: Length::Fixed(220.0),
        placeholder: String::new(),
        trigger: Space::new().into(),
    }
}

pub struct Dropdown<'a, T> {
    selected: Option<T>,
    options: &'a [T],
    label: fn(&T) -> String,
    on_select: Box<dyn Fn(T) -> Message + 'a>,
    width: Length,
    placeholder: String,
    trigger: Element<'a, Message>,
}

impl<T> Dropdown<'_, T> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }
}

#[derive(Clone, Copy, Default)]
struct VisualState {
    open: bool,
    focused: bool,
}

struct State {
    visual: Rc<Cell<VisualState>>,
    highlighted: usize,
    menu: Tree,
    scroll_to_highlight: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            visual: Rc::new(Cell::new(VisualState::default())),
            highlighted: 0,
            menu: Tree::empty(),
            scroll_to_highlight: false,
        }
    }
}

impl State {
    fn open(&mut self, highlighted: usize) {
        self.visual.set(VisualState {
            open: true,
            focused: true,
        });
        self.highlighted = highlighted;
        self.menu = Tree::empty();
        self.scroll_to_highlight = true;
    }

    fn close(&mut self) {
        self.visual.set(VisualState {
            open: false,
            ..self.visual.get()
        });
    }
}

impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.visual.get().focused
    }

    fn focus(&mut self) {
        self.visual.set(VisualState {
            focused: true,
            ..self.visual.get()
        });
    }

    fn unfocus(&mut self) {
        self.visual.set(VisualState::default());
    }
}

impl<T: Clone + PartialEq> Widget<Message, Theme, Renderer> for Dropdown<'_, T> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if self.options.is_empty() {
            state.close();
        }
        state.highlighted = state.highlighted.min(self.options.len().saturating_sub(1));
        let visual = Rc::clone(&state.visual);
        let label = self
            .selected
            .as_ref()
            .map(self.label)
            .unwrap_or_else(|| self.placeholder.clone());
        self.trigger = button(
            row![
                text(label).size(14).width(Fill),
                hero::icon(HeroIcon::ChevronDown, 16, IconTone::Muted),
            ]
            .spacing(8)
            .height(Fill)
            .align_y(Center),
        )
        .height(32)
        .width(self.width)
        .padding([0, 10])
        .style(move |theme, status| {
            let visual = visual.get();
            styles::dropdown_trigger(theme, visual.open, visual.focused, status)
        })
        .on_press_maybe((!self.options.is_empty()).then_some(Message::None))
        .into();
        tree.diff_children(std::slice::from_mut(&mut self.trigger));
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(32.0))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        let state = tree.state.downcast_mut::<State>();
        if is_pointer_press(event) && !cursor.is_over(layout.bounds()) {
            state.visual.set(VisualState::default());
        }
        if shell.is_event_captured() {
            return;
        }
        if !self.options.is_empty()
            && state.visual.get().focused
            && let Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(key),
                ..
            }) = event
            && matches!(
                key,
                Named::Enter | Named::Space | Named::ArrowDown | Named::ArrowUp
            )
        {
            let selected = self
                .options
                .iter()
                .position(|option| Some(option) == self.selected.as_ref())
                .unwrap_or(0);
            state.open(selected);
            shell.capture_event();
            shell.invalidate_widgets();
            shell.request_redraw();
            return;
        }
        if is_pointer_press(event) && cursor.is_over(layout.bounds()) {
            operation::Focusable::focus(state);
        }
        let mut messages = Vec::new();
        let mut local = shell.local(&mut messages);
        self.trigger.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            &mut local,
            viewport,
        );
        let activated = !local.is_empty();
        shell.merge(local, |message| message);
        if activated {
            if state.visual.get().open {
                state.close();
            } else {
                let selected = self
                    .options
                    .iter()
                    .position(|option| Some(option) == self.selected.as_ref())
                    .unwrap_or(0);
                state.open(selected);
            }
            shell.invalidate_widgets();
            shell.request_redraw();
        }
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
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.focusable(None, layout.bounds(), tree.state.downcast_mut::<State>());
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.trigger.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.visual.get().open {
            return None;
        }
        let on_select = self.on_select.as_ref();
        let rows = self
            .options
            .iter()
            .enumerate()
            .fold(column![], |rows, (index, option)| {
                rows.push(
                    button(
                        container(text((self.label)(option)).size(14))
                            .center_y(Fill)
                            .width(Fill),
                    )
                    .height(ROW_HEIGHT)
                    .width(Fill)
                    .padding([0, 10])
                    .style(styles::dropdown_option(
                        Some(option) == self.selected.as_ref(),
                        index == state.highlighted,
                    ))
                    .on_press_with(move || on_select(option.clone())),
                )
            });
        let mut content = motion::dropdown(
            container(scrollable(rows).smooth_scroll(true).height(Length::Shrink))
                .padding(MENU_PADDING)
                .width(Fill)
                .style(styles::dropdown_menu),
        );
        state.menu.diff(content.as_widget_mut());
        Some(overlay::Element::new(Box::new(Menu {
            content,
            state,
            options: self.options,
            on_select,
            anchor: layout.bounds() + translation,
        })))
    }
}

impl<'a, T: Clone + PartialEq + 'a> From<Dropdown<'a, T>> for Element<'a, Message> {
    fn from(dropdown: Dropdown<'a, T>) -> Self {
        Element::new(dropdown)
    }
}

fn is_pointer_press(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(iced::touch::Event::FingerPressed { .. })
    )
}

struct Menu<'a, T> {
    content: Element<'a, Message>,
    state: &'a mut State,
    options: &'a [T],
    on_select: &'a dyn Fn(T) -> Message,
    anchor: Rectangle,
}

impl<T: Clone> overlay::Overlay<Message, Theme, Renderer> for Menu<'_, T> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let below = (bounds.height - self.anchor.y - self.anchor.height - EDGE_MARGIN).max(0.0);
        let above = (self.anchor.y - EDGE_MARGIN).max(0.0);
        let desired =
            (self.options.len() as f32 * ROW_HEIGHT + MENU_PADDING * 2.0).min(MENU_MAX_HEIGHT);
        let down = below >= desired || below >= above;
        let height = desired.min(if down { below } else { above });
        let width = self.anchor.width.min(bounds.width);
        let node = self.content.as_widget_mut().layout(
            &mut self.state.menu,
            renderer,
            &layout::Limits::new(Size::new(width, 0.0), Size::new(width, height)),
        );
        let x = self
            .anchor
            .x
            .clamp(0.0, (bounds.width - node.size().width).max(0.0));
        let y = if down {
            self.anchor.y + self.anchor.height + MENU_PADDING
        } else {
            self.anchor.y - node.size().height - MENU_PADDING
        };
        // Iced clips an overlay to its root bounds. Put the animation offset on
        // that root, keeping its child at the origin so clipping and hit tests
        // share the same moving rectangle. Menus above the trigger lift upward.
        let child = node.children()[0].clone();
        let offset = child.bounds().y * if down { 1.0 } else { -1.0 };
        let y = (y + offset).clamp(0.0, (bounds.height - node.size().height).max(0.0));
        let node = layout::Node::with_children(node.size(), vec![child.move_to((0.0, 0.0))])
            .move_to((x, y));
        if self.state.scroll_to_highlight {
            self.content.as_widget_mut().operate(
                &mut self.state.menu,
                Layout::new(&node),
                renderer,
                &mut RevealOption(self.state.highlighted),
            );
            self.state.scroll_to_highlight = false;
        }
        node
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        // The motion child owns the visible bounds while entering.
        let visible_bounds = layout.child(0).bounds();
        if is_pointer_press(event) && !cursor.is_over(visible_bounds) {
            self.state.close();
            shell.capture_event();
            shell.invalidate_widgets();
            shell.request_redraw();
            return;
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key),
            ..
        }) = event
        {
            match key {
                Named::Escape | Named::Tab => self.state.close(),
                Named::Enter | Named::Space => {
                    if let Some(option) = self.options.get(self.state.highlighted) {
                        shell.publish((self.on_select)(option.clone()));
                    }
                    self.state.close();
                }
                Named::ArrowDown => {
                    self.state.highlighted =
                        (self.state.highlighted + 1).min(self.options.len().saturating_sub(1))
                }
                Named::ArrowUp => self.state.highlighted = self.state.highlighted.saturating_sub(1),
                Named::Home => self.state.highlighted = 0,
                Named::End => self.state.highlighted = self.options.len().saturating_sub(1),
                _ => return,
            }
            self.state.scroll_to_highlight = true;
            if *key != Named::Tab {
                shell.capture_event();
            }
            shell.invalidate_widgets();
            shell.request_redraw();
            return;
        }
        let mut messages = Vec::new();
        let mut local = shell.local(&mut messages);
        self.content.as_widget_mut().update(
            &mut self.state.menu,
            event,
            layout,
            cursor,
            renderer,
            &mut local,
            &visible_bounds,
        );
        let selected = !local.is_empty();
        shell.merge(local, |message| message);
        if selected {
            self.state.close();
            shell.invalidate_widgets();
        }
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content.as_widget().draw(
            &self.state.menu,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.child(0).bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &self.state.menu,
            layout,
            cursor,
            &layout.child(0).bounds(),
            renderer,
        )
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content
            .as_widget_mut()
            .operate(&mut self.state.menu, layout, renderer, operation);
    }
}

/// Keep keyboard navigation visible while retaining ordinary wheel scrolling.
struct RevealOption(usize);

impl Operation for RevealOption {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        _id: Option<&widget::Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        let top = self.0 as f32 * ROW_HEIGHT;
        let bottom = top + ROW_HEIGHT;
        let offset = if top < translation.y {
            top
        } else if bottom > translation.y + bounds.height {
            bottom - bounds.height
        } else {
            return;
        };
        state.scroll_to(operation::scrollable::AbsoluteOffset {
            x: None,
            y: Some(offset),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::Headless;
    use iced::{Point, window};
    use std::time::{Duration, Instant};

    const VIEWPORT: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };

    fn renderer() -> Renderer {
        futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("CPU renderer")
    }

    fn control(options: &[u32], selected: u32) -> Element<'_, Message> {
        dropdown(Some(selected), options, u32::to_string, |option| {
            Message::FindQueryChanged(option.to_string())
        })
        .width(220)
        .into()
    }

    fn mount(
        content: &mut Element<'_, Message>,
        renderer: &Renderer,
        position: Point,
    ) -> (Tree, layout::Node) {
        let mut tree = Tree::empty();
        tree.diff(content.as_widget_mut());
        let node = content
            .as_widget_mut()
            .layout(
                &mut tree,
                renderer,
                &layout::Limits::new(Size::ZERO, VIEWPORT.size()),
            )
            .move_to(position);
        (tree, node)
    }

    fn pointer(pressed: bool) -> Event {
        Event::Mouse(if pressed {
            mouse::Event::ButtonPressed(mouse::Button::Left)
        } else {
            mouse::Event::ButtonReleased(mouse::Button::Left)
        })
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

    fn base_event(
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        event: Event,
        cursor: mouse::Cursor,
    ) {
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
    }

    fn open(
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
    ) {
        for pressed in [true, false] {
            base_event(
                content,
                tree,
                node,
                renderer,
                pointer(pressed),
                mouse::Cursor::Available(node.bounds().center()),
            );
        }
        assert!(tree.state.downcast_ref::<State>().visual.get().open);
    }

    fn menu_event(
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        event: Event,
        cursor: mouse::Cursor,
    ) -> (layout::Node, window::RedrawRequest, Vec<Message>) {
        let mut overlay = content
            .as_widget_mut()
            .overlay(tree, Layout::new(node), renderer, &VIEWPORT, Vector::ZERO)
            .expect("open menu");
        let menu = overlay.as_overlay_mut().layout(renderer, VIEWPORT.size());
        let mut messages = Vec::new();
        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
        overlay
            .as_overlay_mut()
            .update(&event, Layout::new(&menu), cursor, renderer, &mut shell);
        let redraw = shell.redraw_request();
        let menu = overlay.as_overlay_mut().layout(renderer, VIEWPORT.size());
        (menu, redraw, messages)
    }

    #[test]
    fn animated_menu_root_matches_hit_bounds_and_restarts_after_dismissal() {
        let renderer = renderer();
        let mut content = control(&[0, 1, 2], 0);
        let (mut tree, node) = mount(&mut content, &renderer, Point::new(20.0, 30.0));
        let original_trigger = node.bounds();
        for cycle in 0..2 {
            open(&mut content, &mut tree, &node, &renderer);
            let started = Instant::now() + Duration::from_secs(cycle);
            let mut positions = Vec::new();
            for (millis, expected) in [
                (0, window::RedrawRequest::NextFrame),
                (75, window::RedrawRequest::NextFrame),
                (160, window::RedrawRequest::Wait),
                (220, window::RedrawRequest::Wait),
            ] {
                let (menu, redraw, messages) = menu_event(
                    &mut content,
                    &mut tree,
                    &node,
                    &renderer,
                    Event::Window(window::Event::RedrawRequested(
                        started + Duration::from_millis(millis),
                    )),
                    mouse::Cursor::Unavailable,
                );
                assert_eq!(redraw, expected);
                assert!(messages.is_empty());
                assert_eq!(menu.bounds(), Layout::new(&menu).child(0).bounds());
                positions.push(menu.bounds().y);
            }
            assert_eq!(positions[0], positions[2] - 5.0);
            assert!(positions[1] > positions[0] && positions[1] < positions[2]);
            assert_eq!(positions[2], positions[3]);
            assert_eq!(node.bounds(), original_trigger);
            menu_event(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                pointer(true),
                mouse::Cursor::Available(Point::new(390.0, 290.0)),
            );
            assert!(!tree.state.downcast_ref::<State>().visual.get().open);
            assert!(
                content
                    .as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &VIEWPORT,
                        Vector::ZERO
                    )
                    .is_none()
            );
        }
    }

    #[test]
    fn click_selects_the_visible_moving_option_and_closes_menu() {
        let renderer = renderer();
        let mut content = control(&[0, 1, 2], 0);
        let (mut tree, node) = mount(&mut content, &renderer, Point::new(20.0, 30.0));
        open(&mut content, &mut tree, &node, &renderer);
        let (menu, _, _) = menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Waken,
            mouse::Cursor::Unavailable,
        );
        // At the entrance this is option 1, while the final layout would put
        // the same pointer in option 0. Selecting verifies animated hit tests.
        let cursor = mouse::Cursor::Available(Point::new(
            menu.bounds().x + 20.0,
            menu.bounds().y + MENU_PADDING + ROW_HEIGHT + 1.0,
        ));
        let (_, _, pressed) = menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            pointer(true),
            cursor,
        );
        assert!(pressed.is_empty());
        let (_, _, released) = menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            pointer(false),
            cursor,
        );
        assert!(matches!(&released[..], [Message::FindQueryChanged(value)] if value == "1"));
        assert!(!tree.state.downcast_ref::<State>().visual.get().open);
    }

    #[derive(Default)]
    struct ScrollPosition(f32);

    impl Operation for ScrollPosition {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            _id: Option<&widget::Id>,
            _bounds: Rectangle,
            _content: Rectangle,
            translation: Vector,
            _state: &mut dyn operation::Scrollable,
        ) {
            self.0 = translation.y;
        }
    }

    fn scroll_position(
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
    ) -> f32 {
        let mut overlay = content
            .as_widget_mut()
            .overlay(tree, Layout::new(node), renderer, &VIEWPORT, Vector::ZERO)
            .unwrap();
        let menu = overlay.as_overlay_mut().layout(renderer, VIEWPORT.size());
        let mut position = ScrollPosition::default();
        overlay
            .as_overlay_mut()
            .operate(Layout::new(&menu), renderer, &mut position);
        position.0
    }

    #[test]
    fn keyboard_navigation_scrolls_long_menu_and_upward_placement_stays_inside_window() {
        let renderer = renderer();
        let options: Vec<u32> = (0..30).collect();
        let mut content = control(&options, 20);
        let (mut tree, node) = mount(&mut content, &renderer, Point::new(330.0, 250.0));
        struct Focus;
        impl Operation for Focus {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn focusable(
                &mut self,
                _id: Option<&widget::Id>,
                _bounds: Rectangle,
                state: &mut dyn operation::Focusable,
            ) {
                state.focus();
            }
        }
        content
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut Focus);
        base_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            key(Named::Enter),
            mouse::Cursor::Unavailable,
        );
        let started = Instant::now();
        let (initial, _, _) = menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(started)),
            mouse::Cursor::Unavailable,
        );
        let (settled, _, _) = menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                started + Duration::from_millis(160),
            )),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(initial.bounds().y, settled.bounds().y + 5.0);
        assert!(settled.bounds().y >= 0.0);
        assert!(initial.bounds().y + initial.size().height <= node.bounds().y + 5.0);
        assert!(settled.bounds().x + settled.size().width <= VIEWPORT.width);
        assert!(scroll_position(&mut content, &mut tree, &node, &renderer) > 0.0);

        menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            key(Named::Home),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(
            scroll_position(&mut content, &mut tree, &node, &renderer),
            0.0
        );
        menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
            }),
            mouse::Cursor::Available(settled.bounds().center()),
        );
        assert_eq!(
            scroll_position(&mut content, &mut tree, &node, &renderer),
            0.0
        );
        // The overlay is rebuilt for every event. Its scroll animation must
        // remain in the retained tree and advance even with no pointer present.
        menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_millis(75),
            )),
            mouse::Cursor::Unavailable,
        );
        let midway = scroll_position(&mut content, &mut tree, &node, &renderer);
        assert!(midway > 0.0 && midway < 180.0);
        menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                Instant::now() + Duration::from_millis(200),
            )),
            mouse::Cursor::Unavailable,
        );
        assert!(scroll_position(&mut content, &mut tree, &node, &renderer) > 0.0);

        menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            key(Named::End),
            mouse::Cursor::Unavailable,
        );
        menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            key(Named::ArrowUp),
            mouse::Cursor::Unavailable,
        );
        let (_, _, selected) = menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            key(Named::Enter),
            mouse::Cursor::Unavailable,
        );
        assert!(matches!(&selected[..], [Message::FindQueryChanged(value)] if value == "28"));
        assert!(!tree.state.downcast_ref::<State>().visual.get().open);
        open(&mut content, &mut tree, &node, &renderer);
        let (_, _, cancelled) = menu_event(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            key(Named::Escape),
            mouse::Cursor::Unavailable,
        );
        assert!(cancelled.is_empty());
        assert!(!tree.state.downcast_ref::<State>().visual.get().open);
    }
}
