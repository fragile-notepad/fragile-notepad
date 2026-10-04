//! Short, event-driven transitions for transient UI surfaces.

use std::time::{Duration, Instant};

use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, window};

use crate::message::Message;

const ENTRANCE_DURATION: Duration = Duration::from_millis(150);

type FadeBackground = fn(&Theme) -> Color;

// Fade paint colors instead of covering the editor behind a translucent modal.
pub(super) fn fade_container(
    mut style: iced::widget::container::Style,
    opacity: f32,
) -> iced::widget::container::Style {
    style.background = style.background.map(|color| color.scale_alpha(opacity));
    style.text_color = style.text_color.map(|color| color.scale_alpha(opacity));
    style.border.color = style.border.color.scale_alpha(opacity);
    style.shadow.color = style.shadow.color.scale_alpha(opacity);
    style
}

pub(super) fn fade_button(
    mut style: iced::widget::button::Style,
    opacity: f32,
) -> iced::widget::button::Style {
    style.background = style.background.map(|color| color.scale_alpha(opacity));
    style.text_color = style.text_color.scale_alpha(opacity);
    style.border.color = style.border.color.scale_alpha(opacity);
    style.shadow.color = style.shadow.color.scale_alpha(opacity);
    style
}

/// Lift a newly mounted dialog into place without moving surrounding widgets.
pub fn popup<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Motion::entrance(content.into(), 8.0, String::new()))
}

/// Follow an already-eased reveal progress: rise on entry and descend on exit.
pub fn popup_with_progress<'a>(
    content: impl Into<Element<'a, Message>>,
    progress: f32,
) -> Element<'a, Message> {
    let mut motion = Motion::entrance(content.into(), 8.0, String::new());
    motion.external_progress = Some(progress.clamp(0.0, 1.0));
    Element::new(motion)
}

/// Drop a newly mounted menu into place.
pub fn dropdown<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    dropdown_with_key(String::new(), content)
}

/// Replay the entrance when an existing menu position changes its contents.
pub fn dropdown_with_key<'a>(
    key: impl Into<String>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(Motion::entrance(content.into(), -5.0, key.into()))
}

/// Fade opaque content against its surrounding surface.
///
/// Iced does not expose group opacity. A clipped, theme-aware veil blends this
/// content into a matching solid background instead. The caller owns progress
/// and retains the widget until an exit reaches zero, preserving input state.
pub fn fade<'a>(
    content: impl Into<Element<'a, Message>>,
    progress: f32,
    background: FadeBackground,
    interactive: bool,
) -> Element<'a, Message> {
    Element::new(Motion {
        content: content.into(),
        distance: 0.0,
        key: String::new(),
        fade: Some((progress.clamp(0.0, 1.0), background)),
        external_progress: None,
        interactive,
    })
}

struct Motion<'a> {
    content: Element<'a, Message>,
    distance: f32,
    key: String,
    fade: Option<(f32, FadeBackground)>,
    external_progress: Option<f32>,
    interactive: bool,
}

impl<'a> Motion<'a> {
    fn entrance(content: Element<'a, Message>, distance: f32, key: String) -> Self {
        Self {
            content,
            distance,
            key,
            fade: None,
            external_progress: None,
            interactive: true,
        }
    }
}

struct State {
    key: String,
    started: Option<Instant>,
    progress: f32,
}

impl Widget<Message, Theme, Renderer> for Motion<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            key: self.key.clone(),
            started: None,
            progress: if self.fade.is_some() { 1.0 } else { 0.0 },
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.key != self.key {
            state.key.clone_from(&self.key);
            state.started = None;
            state.progress = 0.0;
        }
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let remaining = self.external_progress.map_or_else(
            || (1.0 - tree.state.downcast_ref::<State>().progress).powi(3),
            |progress| 1.0 - progress,
        );
        let offset = self.distance * remaining;
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        // Move the actual child layout so touch, mouse, keyboard operations and
        // nested overlays all agree with the visible bounds throughout motion.
        layout::Node::with_children(
            content.size(),
            vec![content.translate(Vector::new(0.0, offset))],
        )
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
        if self.fade.is_none() && self.external_progress.is_none() {
            let state = tree.state.downcast_mut::<State>();
            if state.progress < 1.0 {
                if let Event::Window(window::Event::RedrawRequested(now)) = event {
                    let started = *state.started.get_or_insert(*now);
                    let progress = (now.saturating_duration_since(started).as_secs_f32()
                        / ENTRANCE_DURATION.as_secs_f32())
                    .min(1.0);
                    if progress != state.progress {
                        state.progress = progress;
                        shell.invalidate_layout();
                    }
                }
                if state.progress < 1.0 {
                    shell.request_redraw();
                }
            }
        }

        if !self.interactive && !matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
            return;
        }

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.child(0),
            cursor,
            renderer,
            shell,
            viewport,
        );
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
            layout.child(0),
            cursor,
            viewport,
        );

        if let Some((progress, background)) = self.fade
            && progress < 1.0
            && let Some(bounds) = layout.bounds().intersection(viewport)
        {
            let mut color = background(theme);
            color.a *= 1.0 - progress;
            // Renderers batch quads before text and images within each
            // layer. A final layer puts the veil above every child,
            // including content that creates its own clipping layers.
            renderer.with_layer(bounds, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        ..renderer::Quad::default()
                    },
                    color,
                );
            });
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            operation,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if !self.interactive {
            return mouse::Interaction::None;
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout.child(0),
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        if !self.interactive {
            return None;
        }
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::Headless;
    use iced::advanced::widget::operation::{self, Operation, Outcome, focusable};
    use iced::widget::{Space, button, container, text_input};
    use iced::{Fill, Point};

    const VIEWPORT: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 240.0,
    };

    fn renderer() -> Renderer {
        futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("CPU headless renderer must be available")
    }

    fn mount(content: &mut Element<'_, Message>, renderer: &Renderer) -> (Tree, layout::Node) {
        // The vendored runtime diffs even a newly created tree before layout;
        // Widget has no children() hook in this version of Iced.
        let mut tree = Tree::empty();
        tree.diff(content.as_widget_mut());
        let node = relayout(content, &mut tree, renderer);
        (tree, node)
    }

    fn relayout(
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

    fn dispatch(
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
            messages.iter().any(|message| {
                matches!(message, Message::FindQueryChanged(query) if query == "t")
            }),
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
}
