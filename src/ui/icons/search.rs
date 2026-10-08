//! Small search controls drawn from geometry, independent of platform fonts.

use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Layout, layout, mouse, renderer};
use iced::{Border, Color, Element, Length, Point, Rectangle, Size, Theme};

use super::hero::IconTone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchIcon {
    MagnifyingGlass,
    Adjustments,
    ArrowUp,
    ArrowDown,
    Return,
}

pub fn icon<Message>(icon: SearchIcon, size: u32, tone: IconTone) -> Element<'static, Message>
where
    Message: 'static,
{
    Element::new(SearchSymbol { icon, size, tone })
}

struct SearchSymbol {
    icon: SearchIcon,
    size: u32,
    tone: IconTone,
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for SearchSymbol
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(self.size as f32),
            Length::Fixed(self.size as f32),
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
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let scale = bounds.width.min(bounds.height) / 24.0;
        let origin = Point::new(
            bounds.center_x() - 12.0 * scale,
            bounds.center_y() - 12.0 * scale,
        );
        let color = match self.tone {
            IconTone::Text => style.text_color,
            IconTone::Muted => style.text_color.scale_alpha(0.68),
        };
        let stroke = 1.8 * scale;
        let point = |x: f32, y: f32| Point::new(origin.x + x * scale, origin.y + y * scale);

        match self.icon {
            SearchIcon::MagnifyingGlass => {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            point(3.5, 3.5),
                            Size::new(13.0 * scale, 13.0 * scale),
                        ),
                        border: Border {
                            color,
                            width: stroke,
                            radius: (6.5 * scale).into(),
                        },
                        ..Default::default()
                    },
                    Color::TRANSPARENT,
                );
                line(
                    renderer,
                    point(15.1, 15.1),
                    point(20.5, 20.5),
                    stroke,
                    color,
                );
            }
            SearchIcon::ArrowUp | SearchIcon::ArrowDown => {
                let (tip, tail, wing) = if self.icon == SearchIcon::ArrowUp {
                    (4.5, 20.0, 10.5)
                } else {
                    (19.5, 4.0, 13.5)
                };
                line(renderer, point(12.0, tip), point(12.0, tail), stroke, color);
                line(renderer, point(6.0, wing), point(12.0, tip), stroke, color);
                line(renderer, point(18.0, wing), point(12.0, tip), stroke, color);
            }
            SearchIcon::Adjustments => {
                for (y, handle) in [(5.0, 8.0), (12.0, 16.0), (19.0, 10.0)] {
                    line(renderer, point(3.5, y), point(20.5, y), stroke, color);
                    line(
                        renderer,
                        point(handle, y - 2.5),
                        point(handle, y + 2.5),
                        stroke * 1.35,
                        color,
                    );
                }
            }
            SearchIcon::Return => {
                line(renderer, point(20.0, 5.0), point(20.0, 12.0), stroke, color);
                line(renderer, point(5.0, 12.0), point(20.0, 12.0), stroke, color);
                line(renderer, point(10.0, 7.0), point(5.0, 12.0), stroke, color);
                line(renderer, point(10.0, 17.0), point(5.0, 12.0), stroke, color);
            }
        }
    }
}

fn line(renderer: &mut impl renderer::Renderer, from: Point, to: Point, width: f32, color: Color) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if dx.abs() < f32::EPSILON || dy.abs() < f32::EPSILON {
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(
                        from.x.min(to.x) - width / 2.0,
                        from.y.min(to.y) - width / 2.0,
                    ),
                    Size::new(dx.abs() + width, dy.abs() + width),
                ),
                border: Border {
                    radius: (width / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            color,
        );
    } else {
        // Closely spaced round points form a smooth stroke without enabling
        // the much larger canvas renderer just for these compact controls.
        let steps = (dx.hypot(dy) / (width * 0.35)).ceil().max(1.0) as usize;
        for step in 0..=steps {
            let progress = step as f32 / steps as f32;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(
                            from.x + dx * progress - width / 2.0,
                            from.y + dy * progress - width / 2.0,
                        ),
                        Size::new(width, width),
                    ),
                    border: Border {
                        radius: (width / 2.0).into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                color,
            );
        }
    }
}
