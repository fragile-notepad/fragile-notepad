//! Event-driven motion for transient surfaces and search activity.

use std::time::{Duration, Instant};

use iced::advanced::text::Renderer as _;
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, window};

use crate::message::Message;

const ENTRANCE_DURATION: Duration = Duration::from_millis(150);
const REVEAL_DURATION: Duration = Duration::from_millis(140);

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

/// The activity represented by the small indicator beside a search status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLightState {
    Idle,
    Searching,
    Success,
    Error,
}

/// One breathing dot that unfurls into a rounded loading arc and folds back.
///
/// Animation stays local to the widget, uses timed redraws, and pauses while
/// its window is unfocused. Rebuilding the surrounding status preserves its
/// phase and focus state. Changes morph from the current appearance, including
/// when another change interrupts an unfinished transition.
pub fn status_light(state: StatusLightState) -> Element<'static, Message> {
    Element::new(StatusLight(state))
}

const STATUS_LIGHT_SIZE: f32 = 12.0;
const STATUS_IDLE_FRAME: Duration = Duration::from_millis(67);
const STATUS_SEARCH_FRAME: Duration = Duration::from_millis(33);
const STATUS_TRANSITION: Duration = Duration::from_millis(200);

struct StatusLight(StatusLightState);

struct StatusLightAnimation {
    state: StatusLightState,
    focused: bool,
    elapsed: Duration,
    last_frame: Option<Instant>,
    next_frame: Option<Instant>,
    weights: [f32; 4],
    from_weights: [f32; 4],
    transition_elapsed: Duration,
    transitioning: bool,
}

impl StatusLightState {
    fn weights(self) -> [f32; 4] {
        let mut weights = [0.0; 4];
        weights[self as usize] = 1.0;
        weights
    }

    fn frame_interval(self) -> Option<Duration> {
        match self {
            Self::Idle => Some(STATUS_IDLE_FRAME),
            Self::Searching => Some(STATUS_SEARCH_FRAME),
            Self::Success | Self::Error => None,
        }
    }
}

impl StatusLightAnimation {
    fn new(state: StatusLightState) -> Self {
        Self {
            state,
            focused: true,
            elapsed: Duration::ZERO,
            last_frame: None,
            next_frame: None,
            weights: state.weights(),
            from_weights: state.weights(),
            transition_elapsed: Duration::ZERO,
            transitioning: false,
        }
    }

    fn retarget(&mut self, state: StatusLightState) {
        self.state = state;
        self.from_weights = self.weights;
        self.transition_elapsed = Duration::ZERO;
        self.transitioning = self.weights != state.weights();
        self.last_frame = None;
        self.next_frame = None;
    }

    fn advance(&mut self, elapsed: Duration) {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if !self.transitioning {
            return;
        }
        self.transition_elapsed = self.transition_elapsed.saturating_add(elapsed);
        let raw =
            (self.transition_elapsed.as_secs_f32() / STATUS_TRANSITION.as_secs_f32()).min(1.0);
        let eased = raw * raw * (3.0 - 2.0 * raw);
        let target = self.state.weights();
        for (index, weight) in self.weights.iter_mut().enumerate() {
            *weight = self.from_weights[index] + (target[index] - self.from_weights[index]) * eased;
        }
        if raw >= 1.0 {
            self.weights = target;
            self.from_weights = target;
            self.transitioning = false;
        }
    }

    fn frame_interval(&self) -> Option<Duration> {
        if self.transitioning {
            Some(STATUS_SEARCH_FRAME)
        } else {
            self.state.frame_interval()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct StatusLightGeometry {
    centerline_radius: f32,
    stroke_radius: f32,
    sweep: f32,
    head_angle: f32,
    taper: f32,
    sample_count: usize,
    glow_radius: f32,
    glow_alpha: f32,
}

impl StatusLightGeometry {
    fn from_animation(animation: &StatusLightAnimation) -> Self {
        let [idle, searching, success, error] = animation.weights;
        // Reduce the periodic phase before converting to f32 so a long-lived
        // window keeps a smooth spinner instead of losing fractional seconds.
        let elapsed = animation.elapsed.as_secs_f64();
        let breath_phase = (elapsed.rem_euclid(2.4) / 2.4) as f32;
        let spinner_phase = elapsed.rem_euclid(1.0) as f32;
        let breath = 0.5 - 0.5 * (std::f32::consts::TAU * breath_phase).cos();
        let centerline_radius = 4.0 * searching;
        let sweep = (std::f32::consts::TAU * 2.0 / 3.0) * searching;
        // Dense opaque round stamps form one connected stroke. At zero sweep
        // there is exactly one stamp; opacity never depends on overlap count.
        let sample_count = ((centerline_radius * sweep / 0.45).ceil() as usize + 1).clamp(1, 40);
        Self {
            centerline_radius,
            stroke_radius: (2.4 + 0.35 * breath) * idle + 0.9 * searching + 3.0 * (success + error),
            sweep,
            head_angle: std::f32::consts::TAU * spinner_phase - std::f32::consts::FRAC_PI_2,
            taper: 0.12 * searching,
            sample_count,
            glow_radius: (4.5 + 0.8 * breath) * idle + 5.0 * (searching + success + error),
            glow_alpha: (0.12 + 0.1 * breath) * idle + 0.1 * searching + 0.14 * (success + error),
        }
    }

    fn sample(self, center: iced::Point, index: usize) -> (iced::Point, f32) {
        let fraction = if self.sample_count > 1 {
            index as f32 / (self.sample_count - 1) as f32
        } else {
            0.5
        };
        let angle = self.head_angle - self.sweep * (1.0 - fraction);
        (
            iced::Point::new(
                center.x + self.centerline_radius * angle.cos(),
                center.y + self.centerline_radius * angle.sin(),
            ),
            self.stroke_radius + self.taper * (2.0 * fraction - 1.0),
        )
    }
}

impl Widget<Message, Theme, Renderer> for StatusLight {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<StatusLightAnimation>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(StatusLightAnimation::new(self.0))
    }

    fn diff(&mut self, tree: &mut Tree) {
        let animation = tree.state.downcast_mut::<StatusLightAnimation>();
        if animation.state != self.0 {
            animation.retarget(self.0);
        }
    }

    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(STATUS_LIGHT_SIZE),
            Length::Fixed(STATUS_LIGHT_SIZE),
        )
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(
            Length::Fixed(STATUS_LIGHT_SIZE),
            Length::Fixed(STATUS_LIGHT_SIZE),
            Size::new(STATUS_LIGHT_SIZE, STATUS_LIGHT_SIZE),
        ))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let animation = tree.state.downcast_mut::<StatusLightAnimation>();
        match event {
            Event::Window(window::Event::Unfocused) => {
                animation.focused = false;
                animation.last_frame = None;
                animation.next_frame = None;
                return;
            }
            Event::Window(window::Event::Focused) => {
                animation.focused = true;
                animation.last_frame = None;
                animation.next_frame = None;
            }
            _ => {}
        }
        if !animation.focused
            || layout
                .bounds()
                .intersection(viewport)
                .is_none_or(|bounds| bounds.width <= 0.0 || bounds.height <= 0.0)
        {
            animation.last_frame = None;
            animation.next_frame = None;
            return;
        }
        if animation.frame_interval().is_none() {
            return;
        }
        let now = if let Event::Window(window::Event::RedrawRequested(now)) = event {
            if let Some(previous) = animation.last_frame.replace(*now) {
                animation.advance(now.saturating_duration_since(previous));
            }
            *now
        } else {
            Instant::now()
        };
        let Some(interval) = animation.frame_interval() else {
            animation.next_frame = None;
            return;
        };
        // Other controls can redraw the window more often. Keep our deadline
        // instead of pulling it forward or requesting another immediate frame.
        let next = animation
            .next_frame
            .filter(|deadline| *deadline > now)
            .unwrap_or(now + interval);
        animation.next_frame = Some(next);
        shell.request_redraw_at(next);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(bounds) = layout.bounds().intersection(viewport) else {
            return;
        };
        let animation = tree.state.downcast_ref::<StatusLightAnimation>();
        let [idle, searching, success, error] = animation.weights;
        let geometry = StatusLightGeometry::from_animation(animation);
        let center = layout.bounds().center();
        let [accent, success_color, error_color] = super::styles::search_light_colors(theme);
        let activity_color = super::styles::search_activity_color(theme);
        // One opaque tint keeps the core bright and avoids alpha accumulation
        // where the rounded stroke samples overlap.
        let color = status_light_color([
            (accent, idle),
            (activity_color, searching),
            (success_color, success),
            (error_color, error),
        ]);
        renderer.with_layer(bounds, |renderer| {
            status_circle(
                renderer,
                center,
                geometry.glow_radius,
                color.scale_alpha(geometry.glow_alpha),
            );
            for index in 0..geometry.sample_count {
                let (point, radius) = geometry.sample(center, index);
                status_circle(renderer, point, radius, color);
            }
        });
    }
}

fn status_light_color(colors: [(Color, f32); 4]) -> Color {
    let mut weight_sum = 0.0;
    let mut red = 0.0;
    let mut green = 0.0;
    let mut blue = 0.0;
    for (color, weight) in colors {
        weight_sum += weight;
        red += color.r * weight;
        green += color.g * weight;
        blue += color.b * weight;
    }
    Color::from_rgb(red / weight_sum, green / weight_sum, blue / weight_sum)
}

fn status_circle(renderer: &mut Renderer, center: iced::Point, radius: f32, color: Color) {
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle::new(
                iced::Point::new(center.x - radius, center.y - radius),
                Size::new(radius * 2.0, radius * 2.0),
            ),
            border: iced::Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        color,
    );
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

        let interactive = self.interactive && self.reveal.is_none_or(|(expanded, _)| expanded);
        if self.reveal.is_some_and(|(expanded, _)| !expanded) {
            return;
        }
        if !interactive && !matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
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
mod tests {
    use super::*;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::Headless;
    use iced::advanced::widget::operation::{self, Operation, Outcome, focusable};
    use iced::widget::{Space, button, container, text_input};
    use iced::{Fill, Point};
    use std::cell::Cell;
    use std::rc::Rc;

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

    struct LayoutProbe {
        content: Element<'static, Message>,
        calls: Rc<Cell<usize>>,
        invalidate_next_frame: Rc<Cell<bool>>,
    }

    impl Widget<Message, Theme, Renderer> for LayoutProbe {
        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }

        fn diff(&mut self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_mut(&mut self.content));
        }

        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.calls.set(self.calls.get() + 1);
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
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
                layout,
                cursor,
                viewport,
            );
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
            if matches!(event, Event::Window(window::Event::RedrawRequested(_)))
                && self.invalidate_next_frame.replace(false)
            {
                shell.invalidate_layout();
            }
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                shell,
                viewport,
            );
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
                layout,
                renderer,
                operation,
            );
        }
    }

    fn measured_child(
        calls: &Rc<Cell<usize>>,
        invalidate_next_frame: &Rc<Cell<bool>>,
    ) -> Element<'static, Message> {
        Element::new(LayoutProbe {
            content: button(Space::new().width(Fill).height(40))
                .padding(0)
                .width(Fill)
                .on_press(Message::None)
                .into(),
            calls: calls.clone(),
            invalidate_next_frame: invalidate_next_frame.clone(),
        })
    }

    #[test]
    fn local_entrance_reuses_child_layout_while_visible_and_click_bounds_move() {
        let _font_guard = crate::font_system_test_guard();
        let renderer = renderer();
        let calls = Rc::new(Cell::new(0));
        let invalidate = Rc::new(Cell::new(false));
        let mut content = popup(measured_child(&calls, &invalidate));
        let (mut tree, mut node) = mount(&mut content, &renderer);
        let started = Instant::now();
        let mut previous_y = Layout::new(&node).child(0).bounds().y;
        assert_eq!(previous_y, 8.0);
        for index in 0..=10 {
            dispatch(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(
                    started + ENTRANCE_DURATION * index / 10,
                )),
                mouse::Cursor::Unavailable,
            );
            node = relayout(&mut content, &mut tree, &renderer);
            let y = Layout::new(&node).child(0).bounds().y;
            assert!(y <= previous_y);
            previous_y = y;
        }
        assert_eq!(previous_y, 0.0);
        assert_eq!(calls.get(), 1, "twelve child layouts become one");

        let point = Point::new(20.0, 20.0);
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            messages.extend(
                dispatch(
                    &mut content,
                    &mut tree,
                    &node,
                    &renderer,
                    Event::Mouse(event),
                    mouse::Cursor::Available(point),
                )
                .1,
            );
        }
        assert!(matches!(messages.as_slice(), [Message::None]));
    }

    #[test]
    fn local_disclosure_reuses_intrinsic_child_layout_while_height_changes() {
        let _font_guard = crate::font_system_test_guard();
        let renderer = renderer();
        let calls = Rc::new(Cell::new(0));
        let invalidate = Rc::new(Cell::new(false));
        let mut content = reveal(measured_child(&calls, &invalidate), true, |_| Color::WHITE);
        let (mut tree, mut node) = mount(&mut content, &renderer);
        let started = Instant::now();
        assert_eq!(node.size().height, 0.0);
        let mut previous_height = 0.0;
        for index in 0..=10 {
            dispatch(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(
                    started + REVEAL_DURATION * index / 10,
                )),
                mouse::Cursor::Unavailable,
            );
            node = relayout(&mut content, &mut tree, &renderer);
            assert!(node.size().height >= previous_height);
            assert_eq!(Layout::new(&node).child(0).bounds().height, 40.0);
            previous_height = node.size().height;
        }
        assert_eq!(previous_height, 40.0);
        assert_eq!(calls.get(), 1, "clip height does not remeasure the child");
    }

    #[test]
    fn motion_child_layout_refreshes_for_diff_child_requests_operations_and_limits() {
        let _font_guard = crate::font_system_test_guard();
        let renderer = renderer();
        let calls = Rc::new(Cell::new(0));
        let invalidate = Rc::new(Cell::new(false));
        let mut content = popup(measured_child(&calls, &invalidate));
        let (mut tree, mut node) = mount(&mut content, &renderer);
        let started = Instant::now();
        invalidate.set(true);
        dispatch(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(started)),
            mouse::Cursor::Unavailable,
        );
        let _ = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(
            calls.get(),
            2,
            "child invalidation reaches the cached layout"
        );
        node = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(calls.get(), 2);

        content.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut focusable::unfocus::<()>(),
        );
        let _ = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(calls.get(), 3);

        tree.diff(content.as_widget_mut());
        let _ = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(calls.get(), 4);

        let narrower = layout::Limits::new(Size::ZERO, Size::new(200.0, 180.0));
        node = content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &narrower);
        assert_eq!(calls.get(), 5);
        assert_eq!(node.size().width, 200.0);
    }

    #[test]
    fn motion_child_layout_refreshes_for_renderer_defaults_and_font_version() {
        let _font_guard = crate::font_system_test_guard();
        let renderer = renderer();
        let calls = Rc::new(Cell::new(0));
        let invalidate = Rc::new(Cell::new(false));
        let mut content = popup(measured_child(&calls, &invalidate));
        let (mut tree, _) = mount(&mut content, &renderer);

        // Simulate prior contexts without requiring a GPU or changing the
        // machine's fonts. The live renderer must replace each stale context.
        let state = tree.state.downcast_mut::<State>();
        state.child_layout.as_mut().unwrap().context.scale_factor = Some(2.0);
        let _ = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(calls.get(), 2);
        tree.state
            .downcast_mut::<State>()
            .child_layout
            .as_mut()
            .unwrap()
            .context
            .default_size = iced::Pixels(1.0);
        let _ = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(calls.get(), 3);
        tree.state
            .downcast_mut::<State>()
            .child_layout
            .as_mut()
            .unwrap()
            .context
            .default_font = iced::Font::MONOSPACE;
        let _ = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(calls.get(), 4);

        // Owned bytes always advance the global version, even when the font
        // was already registered by another renderer in this process.
        iced::advanced::graphics::text::font_system()
            .write()
            .expect("Write font system")
            .load_font(std::borrow::Cow::Owned(
                include_bytes!("../../vendor/iced/graphics/fonts/Iced-Icons.ttf").to_vec(),
            ));
        let _ = relayout(&mut content, &mut tree, &renderer);
        assert_eq!(calls.get(), 5);
    }

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
}
