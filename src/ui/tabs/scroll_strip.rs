//! A hover-revealed top scrollbar over the native, smoothly scrolling tab row.

use std::time::{Duration, Instant};

use iced::advanced::widget::{self, Operation, Tree, operation, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::scrollable;
use iced::{
    Border, Element, Event, Fill, Length, Rectangle, Renderer, Size, Theme, Vector, keyboard,
    window,
};

use crate::message::Message;
use crate::ui::styles;

const EDGE_INSET: f32 = 2.0;
const TOP_INSET: f32 = 1.0;
const HIT_HEIGHT: f32 = 7.0;
const THUMB_HEIGHT: f32 = 3.0;
const ACTIVE_THUMB_HEIGHT: f32 = 5.0;
const MIN_THUMB_WIDTH: f32 = 32.0;
const FADE_IN: Duration = Duration::from_millis(120);
const FADE_OUT: Duration = Duration::from_millis(200);
const EMPHASIS_DURATION: Duration = Duration::from_millis(100);
const FRAME: Duration = Duration::from_millis(16);

#[cfg(test)]
pub(super) fn view<'a>(tabs: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    strip(tabs, false, false)
}

pub(super) fn view_tracked<'a>(
    tabs: impl Into<Element<'a, Message>>,
    dragging_tabs: bool,
) -> Element<'a, Message> {
    strip(tabs, true, dragging_tabs)
}

fn strip<'a>(
    tabs: impl Into<Element<'a, Message>>,
    track_tabs: bool,
    dragging_tabs: bool,
) -> Element<'a, Message> {
    Element::new(ScrollStrip {
        content: scrollable(tabs)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::hidden(),
            ))
            .smooth_scroll(true)
            .style(styles::scrollable)
            .width(Fill)
            .height(super::TAB_HEIGHT)
            .into(),
        track_tabs,
        dragging_tabs,
    })
}

struct ScrollStrip<'a> {
    content: Element<'a, Message>,
    track_tabs: bool,
    dragging_tabs: bool,
}

struct State {
    metrics: Metrics,
    reveal: Transition,
    emphasis: Transition,
    grabbed_at: Option<f32>,
    focused: bool,
    cursor_inside_window: bool,
    tab_geometry: Option<super::StripGeometry>,
    pressed_tab: Option<crate::core::DocumentId>,
    modifiers: keyboard::Modifiers,
    last_drag_scroll: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            metrics: Metrics::default(),
            reveal: Transition::default(),
            emphasis: Transition::default(),
            grabbed_at: None,
            focused: true,
            cursor_inside_window: true,
            tab_geometry: None,
            pressed_tab: None,
            modifiers: keyboard::Modifiers::empty(),
            last_drag_scroll: None,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Metrics {
    viewport_width: f32,
    content_width: f32,
    offset: f32,
}

impl Metrics {
    fn maximum(self) -> f32 {
        (self.content_width - self.viewport_width).max(0.0)
    }

    fn geometry(self, bounds: Rectangle) -> Option<Geometry> {
        let track_width = (bounds.width - EDGE_INSET * 2.0).max(0.0);
        if self.maximum() <= 0.0 || track_width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let thumb_width = (track_width * self.viewport_width / self.content_width)
            .max(MIN_THUMB_WIDTH.min(track_width / 2.0))
            .min(track_width);
        let travel = track_width - thumb_width;
        Some(Geometry {
            track: Rectangle {
                x: bounds.x + EDGE_INSET,
                y: bounds.y,
                width: track_width,
                height: HIT_HEIGHT.min(bounds.height),
            },
            thumb_x: bounds.x
                + EDGE_INSET
                + travel * (self.offset / self.maximum()).clamp(0.0, 1.0),
            thumb_width,
            maximum: self.maximum(),
        })
    }
}

#[derive(Clone, Copy)]
struct Geometry {
    track: Rectangle,
    thumb_x: f32,
    thumb_width: f32,
    maximum: f32,
}

impl Geometry {
    fn thumb_hit_bounds(self) -> Rectangle {
        Rectangle {
            x: self.thumb_x,
            width: self.thumb_width,
            ..self.track
        }
    }

    fn offset_at(self, pointer_x: f32, grabbed_at: f32) -> f32 {
        let travel = self.track.width - self.thumb_width;
        if travel <= 0.0 {
            return 0.0;
        }
        ((pointer_x - self.track.x - self.thumb_width * grabbed_at) / travel).clamp(0.0, 1.0)
            * self.maximum
    }
}

#[derive(Default)]
struct Transition {
    value: f32,
    from: f32,
    target: f32,
    started: Option<Instant>,
    duration: Duration,
}

impl Transition {
    fn advance(&mut self, now: Instant) {
        if let Some(started) = self.started {
            let t = (now.saturating_duration_since(started).as_secs_f32()
                / self.duration.as_secs_f32())
            .min(1.0);
            let eased = t * t * (3.0 - 2.0 * t);
            self.value = self.from + (self.target - self.from) * eased;
            if t >= 1.0 {
                self.value = self.target;
                self.started = None;
            }
        }
    }

    fn retarget(&mut self, visible: bool, now: Instant, duration: Duration) {
        self.advance(now);
        let target = if visible { 1.0 } else { 0.0 };
        if target != self.target {
            self.from = self.value;
            self.target = target;
            self.duration = duration;
            self.started = Some(now);
        }
    }
}

/// Query or move only the root native scrollable, through its public operation API.
#[derive(Default)]
struct ScrollOperation {
    metrics: Metrics,
    offset: Option<f32>,
}

impl Operation for ScrollOperation {
    fn traverse(&mut self, _operate: &mut dyn FnMut(&mut dyn Operation)) {}

    fn scrollable(
        &mut self,
        _id: Option<&widget::Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        self.metrics = Metrics {
            viewport_width: bounds.width,
            content_width: content_bounds.width,
            offset: translation.x,
        };
        if let Some(offset) = self.offset {
            let offset = offset.clamp(0.0, self.metrics.maximum());
            state.scroll_to(scrollable::AbsoluteOffset {
                x: Some(offset),
                y: None,
            });
            self.metrics.offset = offset;
        }
    }
}

#[derive(Default)]
struct TabGeometryOperation {
    translation: Vector,
    tabs: Vec<(crate::core::DocumentId, Rectangle)>,
}

impl Operation for TabGeometryOperation {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        _id: Option<&widget::Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        self.translation = translation;
    }

    fn custom(
        &mut self,
        _id: Option<&widget::Id>,
        mut bounds: Rectangle,
        state: &mut dyn std::any::Any,
    ) {
        if let Some(tab) = state.downcast_ref::<super::TabMarker>() {
            bounds.x -= self.translation.x;
            bounds.y -= self.translation.y;
            self.tabs.push((tab.0, bounds));
        }
    }
}

impl ScrollStrip<'_> {
    fn update_tabs(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
        now: Instant,
    ) {
        if !self.track_tabs {
            return;
        }
        let bounds = layout
            .bounds()
            .intersection(viewport)
            .unwrap_or(Rectangle::default());
        let mut operation = TabGeometryOperation::default();
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            &mut operation,
        );
        let geometry = super::StripGeometry {
            bounds,
            tabs: operation.tabs,
        };
        let state = tree.state.downcast_mut::<State>();
        if state.tab_geometry.as_ref() != Some(&geometry) {
            state.tab_geometry = Some(geometry.clone());
            shell.publish(Message::TabStripGeometry(geometry.clone()));
        }
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                state.pressed_tab = None;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if !shell.is_event_captured() =>
            {
                if let Some(point) = cursor.position()
                    && let Some(document) = geometry.body_at(point)
                {
                    state.pressed_tab = Some(document);
                    shell.publish(Message::TabPressed(document, point, state.modifiers));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if state.pressed_tab.is_some() || self.dragging_tabs {
                    shell.publish(Message::TabPointerMoved(*position));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.pressed_tab.take().is_some() || self.dragging_tabs {
                    shell.publish(Message::TabPointerReleased);
                }
            }
            _ => {}
        }
        // Keep scrolling while a held drag remains at an edge, even if the
        // physical pointer is still. Use the native Scrollable's own state.
        let auto_scroll = self.dragging_tabs
            && cursor
                .position()
                .is_some_and(|point| geometry.contains(point))
            && state.metrics.maximum() > 0.0;
        let edge_direction = if auto_scroll && let Some(point) = cursor.position() {
            let lane = 32.0_f32.min(bounds.width / 2.0);
            if point.x < bounds.x + lane {
                -((bounds.x + lane - point.x) / lane).clamp(0.0, 1.0)
            } else if point.x > bounds.x + bounds.width - lane {
                ((point.x - (bounds.x + bounds.width - lane)) / lane).clamp(0.0, 1.0)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let frame_elapsed = state.last_drag_scroll.map_or(0.016, |last| {
            now.saturating_duration_since(last).as_secs_f32().min(0.05)
        });
        state.last_drag_scroll = (edge_direction != 0.0).then_some(now);
        if edge_direction != 0.0 {
            let offset = (state.metrics.offset + edge_direction * 480.0 * frame_elapsed)
                .clamp(0.0, state.metrics.maximum());
            if offset != state.metrics.offset {
                self.sync_scroll(tree, layout.child(0), renderer, Some(offset));
                shell.request_redraw_at(now + FRAME);
            }
        }
    }

    fn sync_scroll(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        offset: Option<f32>,
    ) {
        let mut operation = ScrollOperation {
            offset,
            ..Default::default()
        };
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            renderer,
            &mut operation,
        );
        let state = tree.state.downcast_mut::<State>();
        state.metrics = operation.metrics;
        if state.metrics.geometry(layout.bounds()).is_none() {
            state.grabbed_at = None;
            state.reveal = Transition::default();
            state.emphasis = Transition::default();
        }
    }
}

impl Widget<Message, Theme, Renderer> for ScrollStrip<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
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
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        self.sync_scroll(tree, Layout::new(&node), renderer, None);
        layout::Node::with_children(node.size(), vec![node])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            operation,
        );
        self.sync_scroll(tree, layout.child(0), renderer, None);
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
        self.sync_scroll(tree, layout.child(0), renderer, None);
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        let state = tree.state.downcast_mut::<State>();
        let was_dragging = state.grabbed_at.is_some();
        match event {
            Event::Window(window::Event::Unfocused) => {
                state.focused = false;
                state.grabbed_at = None;
            }
            Event::Window(window::Event::Focused) => state.focused = true,
            Event::Mouse(mouse::Event::CursorLeft) => state.cursor_inside_window = false,
            Event::Mouse(mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. }) => {
                state.cursor_inside_window = true;
            }
            _ => {}
        }
        let over_strip = state.focused
            && state.cursor_inside_window
            && layout
                .bounds()
                .intersection(viewport)
                .is_some_and(|bounds| cursor.is_over(bounds));
        let geometry = state.metrics.geometry(layout.bounds());
        let over_track = over_strip && geometry.is_some_and(|g| cursor.is_over(g.track));
        let mut offset = None;
        if let Some(geometry) = geometry {
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if over_track => {
                    let position = cursor.position().unwrap();
                    let grabbed_at = if cursor.is_over(geometry.thumb_hit_bounds()) {
                        (position.x - geometry.thumb_x) / geometry.thumb_width
                    } else {
                        0.5
                    };
                    state.grabbed_at = Some(grabbed_at);
                    // This also cancels any pending native wheel animation.
                    offset = Some(geometry.offset_at(position.x, grabbed_at));
                    shell.capture_event();
                }
                Event::Mouse(mouse::Event::CursorMoved { position })
                    if state.grabbed_at.is_some() =>
                {
                    offset = Some(geometry.offset_at(position.x, state.grabbed_at.unwrap()));
                    shell.capture_event();
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if was_dragging => {
                    state.grabbed_at = None;
                    shell.capture_event();
                }
                _ => {}
            }
        }
        let dragging = state.grabbed_at.is_some();
        let reveal = geometry.is_some() && (over_strip || dragging);
        let emphasis = dragging
            || (over_track && geometry.is_some_and(|g| cursor.is_over(g.thumb_hit_bounds())));
        let previous_values = (state.reveal.value, state.emphasis.value);
        state
            .reveal
            .retarget(reveal, now, if reveal { FADE_IN } else { FADE_OUT });
        state.emphasis.retarget(emphasis, now, EMPHASIS_DURATION);

        let animating = state.reveal.started.is_some() || state.emphasis.started.is_some();
        let values_changed = previous_values != (state.reveal.value, state.emphasis.value);
        // Repeated input must not keep pushing the next frame into the future.
        // It can also advance a transition to its endpoint, which still needs
        // painting even though neither transition is animating anymore.
        if !matches!(event, Event::Window(window::Event::RedrawRequested(_)))
            && (animating || values_changed)
        {
            shell.request_redraw();
        } else if animating {
            shell.request_redraw_at(now + FRAME);
        }
        if offset.is_some() || was_dragging != dragging {
            shell.request_redraw();
        }
        // MouseArea emits tab releases even without a matching press. Suppress
        // its cursor for the entire scrollbar gesture, including the release.
        // Wheel events over the top lane still reach the native Scrollable.
        let child_cursor = if dragging
            || was_dragging
            || (over_track
                && !matches!(
                    event,
                    Event::Touch(_) | Event::Mouse(mouse::Event::WheelScrolled { .. })
                )) {
            cursor.levitate()
        } else {
            cursor
        };
        if offset.is_some() {
            self.sync_scroll(tree, layout.child(0), renderer, offset);
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.child(0),
            child_cursor,
            renderer,
            shell,
            viewport,
        );
        // Wheel easing advances in native redraw events; sample it afterwards.
        self.sync_scroll(tree, layout.child(0), renderer, None);
        self.update_tabs(tree, event, layout, cursor, renderer, shell, viewport, now);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.pressed_tab.is_some() || self.dragging_tabs && cursor.is_over(layout.bounds()) {
            return mouse::Interaction::Grabbing;
        }
        if state.grabbed_at.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if state.focused
            && layout
                .bounds()
                .intersection(viewport)
                .is_some_and(|b| cursor.is_over(b))
            && let Some(geometry) = state.metrics.geometry(layout.bounds())
            && cursor.is_over(geometry.track)
        {
            return if cursor.is_over(geometry.thumb_hit_bounds()) {
                mouse::Interaction::Grab
            } else {
                mouse::Interaction::Pointer
            };
        }
        if state.tab_geometry.as_ref().is_some_and(|geometry| {
            cursor
                .position()
                .is_some_and(|point| geometry.body_at(point).is_some())
        }) {
            return mouse::Interaction::Pointer;
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout.child(0),
            cursor,
            viewport,
            renderer,
        )
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
        let state = tree.state.downcast_ref::<State>();
        let geometry = state.metrics.geometry(layout.bounds());
        let child_cursor =
            if state.grabbed_at.is_some() || geometry.is_some_and(|g| cursor.is_over(g.track)) {
                cursor.levitate()
            } else {
                cursor
            };
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.child(0),
            child_cursor,
            viewport,
        );
        if self.dragging_tabs
            && let Some(geometry) = state.tab_geometry.as_ref()
            && let Some(point) = cursor.position()
            && geometry.contains(point)
            && let Some(clip) = geometry.bounds.intersection(viewport)
        {
            let index = geometry.insertion(point);
            let x = geometry
                .tabs
                .get(index)
                .map(|(_, bounds)| bounds.x)
                .or_else(|| {
                    geometry
                        .tabs
                        .last()
                        .map(|(_, bounds)| bounds.x + bounds.width)
                })
                .unwrap_or(geometry.bounds.x);
            renderer.with_layer(clip, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: x - 1.0,
                            y: geometry.bounds.y + 2.0,
                            width: 2.0,
                            height: (geometry.bounds.height - 4.0).max(0.0),
                        },
                        ..Default::default()
                    },
                    styles::accent_color(theme),
                );
            });
        }
        if state.reveal.value <= 0.0 {
            return;
        }
        let Some(geometry) = geometry else {
            return;
        };
        let Some(clip) = layout.bounds().intersection(viewport) else {
            return;
        };
        let height = THUMB_HEIGHT + (ACTIVE_THUMB_HEIGHT - THUMB_HEIGHT) * state.emphasis.value;
        let thumb_style = styles::scrollable(
            theme,
            scrollable::Status::Hovered {
                is_horizontal_scrollbar_hovered: true,
                is_vertical_scrollbar_hovered: false,
                is_horizontal_scrollbar_disabled: false,
                is_vertical_scrollbar_disabled: false,
            },
        )
        .horizontal_rail
        .scroller;
        let backdrop = styles::tab_strip(theme).background;
        // Clear the accent beneath the thumb, including its top inset and
        // rounded ends, so it cannot protrude above or tint the scrollbar.
        // The backing follows the same fade and leaves the rest of the active
        // tab's indicator visible. A final layer keeps both above tab paint.
        renderer.with_layer(clip, |renderer| {
            if let Some(backdrop) = backdrop {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: geometry.thumb_x,
                            y: geometry.track.y,
                            width: geometry.thumb_width,
                            height: TOP_INSET + height,
                        },
                        ..Default::default()
                    },
                    backdrop.scale_alpha(state.reveal.value),
                );
            }
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: geometry.thumb_x,
                        y: geometry.track.y + TOP_INSET,
                        width: geometry.thumb_width,
                        height,
                    },
                    border: Border {
                        radius: (height / 2.0).into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                thumb_style
                    .background
                    .scale_alpha(state.reveal.value * (0.65 + 0.35 * state.emphasis.value)),
            );
        });
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
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
mod tests;
