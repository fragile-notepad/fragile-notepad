//! Search indicator timing and dot-to-arc drawing.

use std::time::{Duration, Instant};

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, renderer};
use iced::{Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, window};

use crate::message::Message;
use crate::ui::styles;

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
    // Idle, searching, success, error weights sum to one across interrupted transitions.
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
        // Smoothstep has zero slope at both ends, avoiding an abrupt start or
        // stop as the dot changes into an arc (and back).
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
        let [accent, success_color, error_color] = styles::search_light_colors(theme);
        let activity_color = styles::search_activity_color(theme);
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

// Keep stroke stamps opaque so their overlaps do not darken the stroke.
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

#[cfg(test)]
mod tests;
