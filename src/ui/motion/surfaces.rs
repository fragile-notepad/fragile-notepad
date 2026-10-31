//! Animates child bounds so pointer input follows the painted position.

use std::time::{Duration, Instant};

use iced::advanced::text::Renderer as _;
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, window};

use crate::message::Message;

const ENTRANCE_DURATION: Duration = Duration::from_millis(150);
const REVEAL_DURATION: Duration = Duration::from_millis(140);

type FadeBackground = fn(&Theme) -> Color;

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
        reveal: None,
    })
}

/// Keep a disclosure mounted while its height and paint settle into place.
///
/// Always call this at the same tree position, including while collapsed. Put
/// spacing inside `content` so it also folds away. Width stays at its natural
/// size; height eases from zero to the child's intrinsic height. The animation
/// requests redraws only during a transition and reverses from its current
/// position when toggled quickly.
pub fn reveal<'a>(
    content: impl Into<Element<'a, Message>>,
    expanded: bool,
    background: FadeBackground,
) -> Element<'a, Message> {
    let mut motion = Motion::entrance(content.into(), 0.0, String::new());
    motion.reveal = Some((expanded, background));
    Element::new(motion)
}

struct Motion<'a> {
    content: Element<'a, Message>,
    distance: f32,
    key: String,
    fade: Option<(f32, FadeBackground)>,
    external_progress: Option<f32>,
    interactive: bool,
    reveal: Option<(bool, FadeBackground)>,
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
            reveal: None,
        }
    }

    fn layout_child(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Local entrances and disclosures relayout the surrounding tree every
        // frame. Their child's constraints stay fixed while only its offset or
        // visible height changes. Reuse that untransformed layout; external
        // fades still reconcile their child normally on every app update.
        let local_animation = self.reveal.is_some()
            || (self.fade.is_none() && self.external_progress.is_none() && self.distance != 0.0);
        if !local_animation {
            return self
                .content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits);
        }

        let context = ChildLayoutContext::new(renderer, *limits);
        if let Some(cached) = &tree.state.downcast_ref::<State>().child_layout
            && cached.context == context
        {
            return cached.node.clone();
        }

        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        tree.state.downcast_mut::<State>().child_layout = Some(ChildLayout {
            context,
            node: node.clone(),
        });
        node
    }
}

#[derive(Clone, Copy, PartialEq)]
struct ChildLayoutContext {
    limits: layout::Limits,
    scale_factor: Option<f32>,
    font_version: iced::advanced::graphics::text::Version,
    default_font: iced::Font,
    default_size: iced::Pixels,
}

impl ChildLayoutContext {
    fn new(renderer: &Renderer, limits: layout::Limits) -> Self {
        Self {
            limits,
            scale_factor: renderer.scale_factor(),
            font_version: iced::advanced::graphics::text::font_system()
                .read()
                .expect("Read font system")
                .version(),
            default_font: renderer.default_font(),
            default_size: renderer.default_size(),
        }
    }
}

struct ChildLayout {
    context: ChildLayoutContext,
    node: layout::Node,
}

/// Keeps progress across view rebuilds; reversing a disclosure starts from its current height.
struct State {
    key: String,
    started: Option<Instant>,
    progress: f32,
    reveal_target: bool,
    reveal_from: f32,
    child_layout: Option<ChildLayout>,
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
            reveal_target: self.reveal.is_some_and(|(expanded, _)| expanded),
            reveal_from: 0.0,
            child_layout: None,
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        state.child_layout = None;
        if state.key != self.key {
            state.key.clone_from(&self.key);
            state.started = None;
            state.progress = 0.0;
        }
        if let Some((expanded, _)) = self.reveal
            && state.reveal_target != expanded
        {
            state.reveal_target = expanded;
            state.reveal_from = state.progress;
            state.started = None;
        }
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        let mut size = self.content.as_widget().size();
        if self.reveal.is_some() {
            size.height = Length::Shrink;
        }
        size
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if self.reveal.is_some() {
            // Keep the complete child layout on every frame. Clipping its paint,
            // rather than compressing its own layout, keeps caret and hit-test
            // geometry stable throughout the disclosure.
            let child_limits = layout::Limits::with_compression(
                Size::new(limits.min().width, 0.0),
                limits.max(),
                limits.compression(),
            );
            let content = self.layout_child(tree, renderer, &child_limits);
            let size = Size::new(
                content.size().width,
                content.size().height * tree.state.downcast_ref::<State>().progress,
            );
            return layout::Node::with_children(size, vec![content]);
        }
        let remaining = self.external_progress.map_or_else(
            || (1.0 - tree.state.downcast_ref::<State>().progress).powi(3),
            |progress| 1.0 - progress,
        );
        let offset = self.distance * remaining;
        let content = self.layout_child(tree, renderer, limits);
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
        if self.reveal.is_some() {
            let state = tree.state.downcast_mut::<State>();
            let target = if state.reveal_target { 1.0 } else { 0.0 };
            if state.started.is_some() || (state.progress - target).abs() > f32::EPSILON {
                if let Event::Window(window::Event::RedrawRequested(now)) = event {
                    let started = *state.started.get_or_insert(*now);
                    let raw = (now.saturating_duration_since(started).as_secs_f32()
                        / REVEAL_DURATION.as_secs_f32())
                    .min(1.0);
                    let eased = 1.0 - (1.0 - raw).powi(3);
                    state.progress = state.reveal_from + (target - state.reveal_from) * eased;
                    if raw >= 1.0 {
                        state.progress = target;
                        state.reveal_from = target;
                        state.started = None;
                    }
                    shell.invalidate_layout();
                }
                if state.started.is_some() || (state.progress - target).abs() > f32::EPSILON {
                    shell.request_redraw();
                }
            }
        } else if self.fade.is_none() && self.external_progress.is_none() {
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

        // Hidden disclosures do not forward events. Closing fades still pass
        // redraws to the child so its own paint can settle while input is off.
        if self.reveal.is_some_and(|(expanded, _)| !expanded) {
            return;
        }
        if !self.interactive && !matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
            return;
        }

        let clipped_viewport = self
            .reveal
            .map(|_| layout.bounds().intersection(viewport).unwrap_or_default());
        let cursor = if clipped_viewport.is_some_and(|bounds| !cursor.is_over(bounds)) {
            mouse::Cursor::Unavailable
        } else {
            cursor
        };

        let mut messages = Vec::new();
        let mut child_shell = shell.local(&mut messages);
        if shell.is_event_captured() {
            child_shell.capture_event();
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.child(0),
            cursor,
            renderer,
            &mut child_shell,
            clipped_viewport.as_ref().unwrap_or(viewport),
        );
        if !matches!(event, Event::Window(window::Event::RedrawRequested(_)))
            || child_shell.is_layout_invalid().is_some()
            || child_shell.are_widgets_invalid()
        {
            tree.state.downcast_mut::<State>().child_layout = None;
        }
        shell.merge(child_shell, std::convert::identity);
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
        if self.reveal.is_some() {
            if let Some(bounds) = layout.bounds().intersection(viewport) {
                renderer.with_layer(bounds, |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        layout.child(0),
                        cursor,
                        &bounds,
                    );
                });
            }
        } else {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout.child(0),
                cursor,
                viewport,
            );
        }

        let fade = self.fade.or_else(|| {
            self.reveal
                .map(|(_, background)| (tree.state.downcast_ref::<State>().progress, background))
        });
        if let Some((progress, background)) = fade
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
        tree.state.downcast_mut::<State>().child_layout = None;
        if self.reveal.is_some_and(|(expanded, _)| !expanded) {
            // Collapsed fields retain their tree and editing state, but stay
            // out of focus traversal and cannot retain an invisible caret.
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout.child(0),
                renderer,
                &mut widget::operation::focusable::unfocus::<()>(),
            );
            return;
        }
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
        if !self.interactive
            || self
                .reveal
                .is_some_and(|(expanded, _)| !expanded || !cursor.is_over(layout.bounds()))
        {
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
        if !self.interactive || self.reveal.is_some_and(|(expanded, _)| !expanded) {
            return None;
        }
        let overlay = self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            viewport,
            translation,
        );
        // An overlay can mutate child state through its separate update path.
        // Keep ordinary layout behavior while such an overlay is mounted.
        if overlay.is_some() {
            tree.state.downcast_mut::<State>().child_layout = None;
        }
        overlay
    }
}

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;
