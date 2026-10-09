//! Keeps surface edges and joins antialiased without repainting covered interiors.

#[cfg(test)]
mod tests;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::message::Message;
use crate::ui::styles;

pub(super) fn shell<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(CoveredBackground {
        content: content.into(),
        background: Background::Shell,
    })
}

pub(super) fn editor<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(CoveredBackground {
        content: content.into(),
        background: Background::Editor,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Background {
    Shell,
    Editor,
}

struct CoveredBackground<'a> {
    content: Element<'a, Message>,
    background: Background,
}

impl Widget<Message, Theme, Renderer> for CoveredBackground<'_> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.content.as_widget_mut().diff(tree);
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
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        let bounds = layout.bounds();
        let appearance = match self.background {
            Background::Shell => {
                styles::app_shell(theme).background(styles::app_shell_background(theme))
            }
            Background::Editor => styles::editor_surface(theme),
        };
        let Some(scale) = renderer.scale_factor() else {
            // Software rendering supplies no physical scale hint. Its original
            // full fills preserve AA without introducing fractional clip edges.
            iced::widget::container::draw_background(renderer, &appearance, bounds);
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
            return;
        };
        let scale = scale.max(f32::EPSILON);
        if bounds.width * scale <= 4.0 || bounds.height * scale <= 4.0 {
            iced::widget::container::draw_background(renderer, &appearance, bounds);
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
            return;
        }
        let edge = 2.0 / scale;
        let floor = |value: f32| (value * scale).floor() / scale;
        let ceil = |value: f32| (value * scale).ceil() / scale;
        let left = ceil(bounds.x + edge);
        let top = ceil(bounds.y + edge);
        let right = floor(bounds.x + bounds.width - edge).max(left);
        let bottom = floor(bounds.y + bounds.height - edge).max(top);
        let interior = Rectangle {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        };
        let mut paint = |band: Rectangle| {
            if band.width > 0.0 && band.height > 0.0 {
                iced::widget::container::draw_background(renderer, &appearance, band);
            }
        };

        // Plain square fills can be split at physical pixel boundaries: internal
        // cuts have no AA at fragment centers, while outer edges stay unchanged.
        // Keep these quads in the parent layer to preserve foreground ordering.
        for band in [
            Rectangle {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: top - bounds.y,
            },
            Rectangle {
                x: bounds.x,
                y: bottom,
                width: bounds.width,
                height: bounds.y + bounds.height - bottom,
            },
            Rectangle {
                x: bounds.x,
                y: top,
                width: left - bounds.x,
                height: bottom - top,
            },
            Rectangle {
                x: right,
                y: top,
                width: bounds.x + bounds.width - right,
                height: bottom - top,
            },
        ] {
            paint(band);
        }

        if self.background == Background::Shell {
            for row in layout.children().skip(1) {
                let band = Rectangle {
                    x: left,
                    y: floor(row.bounds().y - edge),
                    width: right - left,
                    height: ceil(row.bounds().y + edge) - floor(row.bounds().y - edge),
                };
                if let Some(band) = band.intersection(&interior) {
                    paint(band);
                }
            }

            // The editor and animated function list also share a vertical join.
            for row in layout.children() {
                for child in row.children().skip(1) {
                    let band = Rectangle {
                        x: floor(child.bounds().x - edge),
                        y: floor(row.bounds().y + edge),
                        width: ceil(child.bounds().x + edge) - floor(child.bounds().x - edge),
                        height: (ceil(row.bounds().y + row.bounds().height - edge)
                            - floor(row.bounds().y + edge))
                        .max(0.0),
                    };
                    if let Some(band) = band.intersection(&interior) {
                        paint(band);
                    }
                }
            }
        }

        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
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
        self.content
            .as_widget_mut()
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
