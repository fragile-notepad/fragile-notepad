use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Layout, image, layout, mouse, renderer};
use iced::{Border, Color, Length, Rectangle, Size, Theme};

use super::{Action, ControlStyle, HEIGHT, MAC_CONTROL_WIDTH};
use crate::ui::icons::hero::{self, HeroIcon};

pub(super) struct Symbol {
    pub action: Action,
    pub style: ControlStyle,
    pub focused: bool,
    pub maximized: bool,
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for Symbol
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(if self.style == ControlStyle::MacOS {
                MAC_CONTROL_WIDTH
            } else {
                46.0
            }),
            Length::Fixed(HEIGHT),
        )
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = <Self as Widget<Message, Theme, Renderer>>::size(self);
        layout::Node::new(limits.resolve(size.width, size.height, Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let center = bounds.center();
        let mac = self.style == ControlStyle::MacOS;
        let hover = if mac {
            // Native traffic lights reveal their symbols together when the
            // pointer enters the group, including the space between circles.
            let index = match self.action {
                Action::Close => 0.0,
                Action::Minimize => 1.0,
                _ => 2.0,
            };
            cursor.is_over(Rectangle {
                x: bounds.x - index * MAC_CONTROL_WIDTH,
                width: MAC_CONTROL_WIDTH * 3.0,
                ..bounds
            })
        } else {
            cursor.is_over(bounds)
        };
        let ink = if mac {
            Color::from_rgba(0.12, 0.10, 0.08, 0.8)
        } else {
            style.text_color
        };
        if mac {
            let color = if !self.focused && !hover {
                crate::ui::styles::shortcut_text_color(theme).scale_alpha(0.22)
            } else {
                match self.action {
                    Action::Close => Color::from_rgb8(255, 95, 87),
                    Action::Minimize => Color::from_rgb8(254, 188, 46),
                    _ => Color::from_rgb8(40, 200, 64),
                }
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        iced::Point::new(center.x - 6.0, center.y - 6.0),
                        Size::new(12.0, 12.0),
                    ),
                    border: Border {
                        radius: 6.0.into(),
                        width: 0.7,
                        color: Color::BLACK.scale_alpha(0.16),
                    },
                    ..Default::default()
                },
                color,
            );
            if !hover {
                return;
            }
        }
        let icon = match self.action {
            Action::Close => Some(HeroIcon::XMark),
            Action::Minimize => Some(HeroIcon::Minus),
            Action::ToggleMaximize if mac => Some(if self.maximized {
                HeroIcon::Minus
            } else {
                HeroIcon::Plus
            }),
            _ => None,
        };
        if let Some(icon) = icon {
            let size = if mac { 10.0 } else { 14.0 };
            let bounds = Rectangle::new(
                iced::Point::new(center.x - size / 2.0, center.y - size / 2.0),
                Size::new(size, size),
            );
            renderer.draw_image(
                image::Image::new(hero::handle_with_color(icon, ink))
                    .filter_method(image::FilterMethod::Linear),
                bounds,
                bounds,
            );
        } else {
            // Geometry keeps maximize/restore crisp without depending on OS fonts.
            let square = |renderer: &mut Renderer, x, y, size| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(iced::Point::new(x, y), Size::new(size, size)),
                        border: Border {
                            width: 1.0,
                            color: ink,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    Color::TRANSPARENT,
                )
            };
            if self.maximized {
                // Draw only the exposed top and right of the back window.
                for bounds in [
                    Rectangle::new(
                        iced::Point::new(center.x - 2.0, center.y - 5.0),
                        Size::new(7.0, 1.0),
                    ),
                    Rectangle::new(
                        iced::Point::new(center.x + 4.0, center.y - 5.0),
                        Size::new(1.0, 7.0),
                    ),
                ] {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds,
                            ..Default::default()
                        },
                        ink,
                    );
                }
                square(renderer, center.x - 5.0, center.y - 2.0, 8.0);
            } else {
                square(renderer, center.x - 5.0, center.y - 5.0, 10.0);
            }
        }
    }
}
