use iced::advanced::Renderer as _;
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector, window};

use super::Action;
use crate::message::Message;

pub(super) fn frame(
    content: Element<Message>,
    id: window::Id,
    border: f32,
    focused: bool,
) -> Element<Message> {
    Element::new(ChromeRegion {
        content,
        id,
        border: Some(border),
        focused,
    })
}

pub(super) fn drag_region(content: Element<Message>, id: window::Id) -> Element<Message> {
    Element::new(ChromeRegion {
        content,
        id,
        border: None,
        focused: false,
    })
}

struct ChromeRegion<'a> {
    content: Element<'a, Message>,
    id: window::Id,
    border: Option<f32>,
    focused: bool,
}

#[derive(Default)]
struct State {
    click: Option<mouse::Click>,
    drag_origin: Option<Point>,
}

impl Widget<Message, Theme, Renderer> for ChromeRegion<'_> {
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
        layout::Node::with_children(node.size(), vec![node])
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
        if self.border.is_some() && renderer.scale_factor().is_none() {
            // The software renderer has no physical scale hint. Preserve its
            // original full fill instead of splitting antialiased clip masks.
            iced::widget::container::draw_background(
                renderer,
                &crate::ui::styles::window_frame(theme, self.focused),
                layout.bounds(),
            );
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout.child(0),
                cursor,
                viewport,
            );
            return;
        }
        if let Some(border) = self.border {
            let bounds = layout.bounds();
            // Children cover the interior. Keep the original quad geometry for
            // its border and AA, but rasterize only the visible frame bands.
            let scale = renderer.scale_factor().unwrap_or(1.0).max(f32::EPSILON);
            let edge = (border.max(1.0) + 1.0 / scale)
                .min(bounds.width / 2.0)
                .min(bounds.height / 2.0);
            // Back the children's antialiased inset edges. Pixel-aligned clip
            // joins also avoid partial coverage with the software renderer.
            let left = ((bounds.x + edge) * scale).ceil() / scale;
            let right = (((bounds.x + bounds.width - edge) * scale).floor() / scale).max(left);
            let top = ((bounds.y + edge) * scale).ceil() / scale;
            let bottom = (((bounds.y + bounds.height - edge) * scale).floor() / scale).max(top);
            // The caption and client background share an antialiased edge.
            // Back the pixels around it too: at fractional scales their partial
            // coverage can otherwise reveal the cleared target between them.
            let caption_bottom = (bounds.y + super::HEIGHT) * scale;
            let seam_top = ((caption_bottom - 1.0).floor() / scale).clamp(top, bottom);
            let seam_bottom = ((caption_bottom + 1.0).ceil() / scale).clamp(seam_top, bottom);
            let appearance = crate::ui::styles::window_frame(theme, self.focused);
            for band in [
                Rectangle {
                    height: top - bounds.y,
                    ..bounds
                },
                Rectangle {
                    y: bottom,
                    height: bounds.y + bounds.height - bottom,
                    ..bounds
                },
                Rectangle {
                    y: top,
                    width: left - bounds.x,
                    height: bottom - top,
                    ..bounds
                },
                Rectangle {
                    x: right,
                    y: top,
                    width: bounds.x + bounds.width - right,
                    height: bottom - top,
                    ..bounds
                },
                Rectangle {
                    x: left,
                    y: seam_top,
                    width: right - left,
                    height: seam_bottom - seam_top,
                },
            ] {
                if let Some(clip) = band.intersection(viewport) {
                    renderer.with_layer(clip, |renderer| {
                        iced::widget::container::draw_background(renderer, &appearance, bounds);
                    });
                }
            }
        }
        if self.border.is_some() {
            // Keep children after the frame's clipped layers. Returning to the
            // parent layer would put their unlayered quads before those bands.
            if let Some(clip) = layout.bounds().intersection(viewport) {
                renderer.with_layer(clip, |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        layout.child(0),
                        cursor,
                        viewport,
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
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
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
        if let Some(border) = self.border
            && let Some(point) = cursor.position()
            && let Some(direction) = resize_direction(layout.bounds(), point, border)
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
        {
            shell.publish(Message::WindowChrome(self.id, Action::Resize(direction)));
            shell.capture_event();
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
        if self.border.is_some() {
            return;
        }
        if shell.is_event_captured() {
            if matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::ButtonReleased(_))
            ) {
                let state = tree.state.downcast_mut::<State>();
                state.click = None;
                state.drag_origin = None;
            }
            return;
        }
        if matches!(event, Event::Window(window::Event::Unfocused)) {
            *tree.state.downcast_mut::<State>() = State::default();
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) {
            tree.state.downcast_mut::<State>().drag_origin = None;
        }
        // Wait for real motion before entering the OS move loop. Starting that
        // loop on every press can consume the second click of a double-click.
        if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
            let state = tree.state.downcast_mut::<State>();
            if state
                .drag_origin
                .is_some_and(|origin| origin.distance(*position) >= 4.0)
            {
                state.drag_origin = None;
                state.click = None;
                shell.publish(Message::WindowChrome(self.id, Action::Drag));
                shell.capture_event();
                return;
            }
        }
        let Some(point) = cursor.position_over(layout.bounds()) else {
            return;
        };
        let action = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let state = tree.state.downcast_mut::<State>();
                let click = mouse::Click::new(point, mouse::Button::Left, state.click);
                let action = if click.kind() == mouse::click::Kind::Double {
                    state.drag_origin = None;
                    Some(Action::ToggleMaximize)
                } else {
                    state.drag_origin = Some(point);
                    None
                };
                state.click = Some(click);
                shell.capture_event();
                action
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) if cfg!(windows) => {
                Some(Action::SystemMenu)
            }
            _ => None,
        };
        if let Some(action) = action {
            shell.publish(Message::WindowChrome(self.id, action));
            shell.capture_event();
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if let Some(border) = self.border
            && let Some(point) = cursor.position()
            && let Some(direction) = resize_direction(layout.bounds(), point, border)
        {
            return match direction {
                window::Direction::North | window::Direction::South => {
                    mouse::Interaction::ResizingVertically
                }
                window::Direction::East | window::Direction::West => {
                    mouse::Interaction::ResizingHorizontally
                }
                window::Direction::NorthWest | window::Direction::SouthEast => {
                    mouse::Interaction::ResizingDiagonallyDown
                }
                _ => mouse::Interaction::ResizingDiagonallyUp,
            };
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
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            viewport,
            translation,
        )
    }
}

fn resize_direction(bounds: Rectangle, point: Point, border: f32) -> Option<window::Direction> {
    if border <= 0.0 || !bounds.contains(point) {
        return None;
    }
    let x = point.x - bounds.x;
    let y = point.y - bounds.y;
    let left = x < border;
    let right = x >= bounds.width - border;
    let top = y < border;
    let bottom = y >= bounds.height - border;
    if !left && !right && !top && !bottom {
        return None;
    }
    // Extend corner grips along the thin edge without stealing client-area clicks.
    let corner = 16.0;
    match (
        x < corner,
        x >= bounds.width - corner,
        y < corner,
        y >= bounds.height - corner,
    ) {
        (true, _, true, _) => Some(window::Direction::NorthWest),
        (_, true, true, _) => Some(window::Direction::NorthEast),
        (true, _, _, true) => Some(window::Direction::SouthWest),
        (_, true, _, true) => Some(window::Direction::SouthEast),
        _ if left => Some(window::Direction::West),
        _ if right => Some(window::Direction::East),
        _ if top => Some(window::Direction::North),
        _ => Some(window::Direction::South),
    }
}
