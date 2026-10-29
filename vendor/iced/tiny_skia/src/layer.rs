use crate::Primitive;
use crate::core::renderer::Quad;
use crate::core::{self, Background, Color, Point, Rectangle, Svg, Transformation, Vector};
use crate::graphics::damage;
use crate::graphics::layer;
use crate::graphics::text::{Editor, Paragraph, Text};
use crate::graphics::{self, Image};

use std::sync::Arc;

pub type Stack = layer::Stack<Layer>;
pub(crate) type DamageSummary = damage::Summary;
pub(crate) type Scroll = damage::Scroll;
const MAX_SCROLL_MISMATCHED_TEXT_ITEMS: usize = 10;

#[derive(Debug, Clone)]
pub struct Layer {
    pub bounds: Rectangle,
    pub quads: Vec<(Quad, Background)>,
    pub primitives: Vec<Item<Primitive>>,
    pub images: Vec<Image>,
    pub text: Vec<Item<Text>>,
    pub(crate) opacity: Option<OpacityGroup>,
}

#[derive(Debug, Clone)]
pub(crate) struct OpacityGroup {
    pub layers: Arc<[Layer]>,
    pub bounds: Rectangle,
    pub opacity: f32,
}

impl Layer {
    pub fn draw_quad(
        &mut self,
        mut quad: Quad,
        background: Background,
        transformation: Transformation,
    ) {
        quad.bounds = quad.bounds * transformation;
        self.quads.push((quad, background));
    }

    pub fn draw_paragraph(
        &mut self,
        paragraph: &Paragraph,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let paragraph = Text::Paragraph {
            paragraph: paragraph.downgrade(),
            position,
            color,
            clip_bounds,
            transformation,
        };

        self.text.push(Item::Live(paragraph));
    }

    pub fn draw_editor(
        &mut self,
        editor: &Editor,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let editor = Text::Editor {
            editor: editor.downgrade(),
            position,
            color,
            clip_bounds,
            transformation,
        };

        self.text.push(Item::Live(editor));
    }

    pub fn draw_text(
        &mut self,
        text: core::Text,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let text = Text::Cached {
            content: text.content,
            bounds: Rectangle::new(position, text.bounds) * transformation,
            color,
            size: text.size * transformation.scale_factor(),
            line_height: text.line_height.to_absolute(text.size) * transformation.scale_factor(),
            font: text.font,
            align_x: text.align_x,
            align_y: text.align_y,
            shaping: text.shaping,
            wrapping: text.wrapping,
            ellipsis: text.ellipsis,
            clip_bounds: clip_bounds * transformation,
        };

        self.text.push(Item::Live(text));
    }

    pub fn draw_text_raw(&mut self, raw: graphics::text::Raw, transformation: Transformation) {
        let raw = Text::Raw {
            raw,
            transformation,
        };

        self.text.push(Item::Live(raw));
    }

    pub fn draw_text_group(
        &mut self,
        text: Vec<Text>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.text
            .push(Item::Group(text, clip_bounds, transformation));
    }

    pub fn draw_text_cache(
        &mut self,
        text: Arc<[Text]>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.text
            .push(Item::Cached(text, clip_bounds, transformation));
    }

    pub fn draw_image(&mut self, image: Image, transformation: Transformation) {
        match image {
            Image::Raster {
                image,
                bounds,
                clip_bounds,
            } => {
                self.draw_raster(image, bounds, clip_bounds, transformation);
            }
            Image::Vector {
                svg,
                bounds,
                clip_bounds,
            } => {
                self.draw_svg(svg, bounds, clip_bounds, transformation);
            }
        }
    }

    pub fn draw_raster(
        &mut self,
        image: core::Image,
        bounds: Rectangle,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let image = Image::Raster {
            image: core::Image {
                border_radius: image.border_radius * transformation.scale_factor(),
                ..image
            },
            bounds: bounds * transformation,
            clip_bounds: clip_bounds * transformation,
        };

        self.images.push(image);
    }

    pub fn draw_svg(
        &mut self,
        svg: Svg,
        bounds: Rectangle,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let svg = Image::Vector {
            svg,
            bounds: bounds * transformation,
            clip_bounds: clip_bounds * transformation,
        };

        self.images.push(svg);
    }

    pub fn draw_primitive_group(
        &mut self,
        primitives: Vec<Primitive>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.primitives.push(Item::Group(
            primitives,
            clip_bounds * transformation,
            transformation,
        ));
    }

    pub fn draw_primitive_cache(
        &mut self,
        primitives: Arc<[Primitive]>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.primitives.push(Item::Cached(
            primitives,
            clip_bounds * transformation,
            transformation,
        ));
    }

    pub fn damage(previous: &Self, current: &Self) -> Vec<Rectangle> {
        Self::damage_with_scroll(previous, current, None)
    }

    pub(crate) fn damage_with_scroll(
        previous: &Self,
        current: &Self,
        scroll: Option<Scroll>,
    ) -> Vec<Rectangle> {
        Self::damage_with_scroll_summary(previous, current, scroll).0
    }

    pub(crate) fn damage_with_scroll_summary(
        previous: &Self,
        current: &Self,
        scroll: Option<Scroll>,
    ) -> (Vec<Rectangle>, DamageSummary) {
        if previous.opacity.is_some() || current.opacity.is_some() {
            return (
                vec![previous.bounds, current.bounds],
                DamageSummary::default(),
            );
        }
        if previous.bounds != current.bounds {
            return (
                vec![previous.bounds, current.bounds],
                DamageSummary {
                    quads: previous.quads.len() + current.quads.len(),
                    text: previous.text.len() + current.text.len(),
                    primitives: previous.primitives.len() + current.primitives.len(),
                    images: previous.images.len() + current.images.len(),
                    scroll: false,
                },
            );
        }

        let layer_bounds = current.bounds.expand(1.0);

        let quads = damage::list(
            &previous.quads,
            &current.quads,
            |(quad, _)| {
                let Some(bounds) = quad_damage_bounds(quad).intersection(&layer_bounds) else {
                    return vec![];
                };

                vec![bounds]
            },
            |(quad_a, background_a), (quad_b, background_b)| {
                quad_a == quad_b && background_a == background_b
            },
        );

        let text = if let Some(scroll) = scroll {
            scroll.damage()
        } else {
            damage::diff(
                &previous.text,
                &current.text,
                |item| {
                    item.as_slice()
                        .iter()
                        .filter_map(Text::visible_bounds)
                        .map(|bounds| bounds * item.transformation())
                        .collect()
                },
                |text_a, text_b| {
                    damage::list(
                        text_a.as_slice(),
                        text_b.as_slice(),
                        |text| {
                            text.visible_bounds()
                                .into_iter()
                                .map(|bounds| bounds * text_a.transformation())
                                .collect()
                        },
                        |text_a, text_b| text_a == text_b,
                    )
                },
            )
        };

        let primitives = damage::list(
            &previous.primitives,
            &current.primitives,
            |item| match item {
                Item::Live(primitive) => vec![primitive.visible_bounds()],
                Item::Group(primitives, group_bounds, transformation) => primitives
                    .as_slice()
                    .iter()
                    .map(Primitive::visible_bounds)
                    .map(|bounds| bounds * *transformation)
                    .filter_map(|bounds| bounds.intersection(group_bounds))
                    .collect(),
                Item::Cached(_primitives, bounds, _transformation) => {
                    vec![*bounds]
                }
            },
            |primitive_a, primitive_b| match (primitive_a, primitive_b) {
                (
                    Item::Cached(cache_a, bounds_a, transformation_a),
                    Item::Cached(cache_b, bounds_b, transformation_b),
                ) => {
                    Arc::ptr_eq(cache_a, cache_b)
                        && bounds_a == bounds_b
                        && transformation_a == transformation_b
                }
                _ => false,
            },
        );

        let images = damage::list(
            &previous.images,
            &current.images,
            |image| vec![image.bounds().expand(1.0)],
            Image::eq,
        );

        let summary = DamageSummary {
            quads: quads.len(),
            text: text.len(),
            primitives: primitives.len(),
            images: images.len(),
            scroll: scroll.is_some(),
        };
        let mut damage = quads;
        damage.extend(text);
        damage.extend(primitives);
        damage.extend(images);

        (damage, summary)
    }

    pub(crate) fn scroll(previous: &Self, current: &Self) -> Option<Scroll> {
        if previous.opacity.is_some() || current.opacity.is_some() {
            return None;
        }
        if previous.bounds != current.bounds {
            return None;
        }

        if !unchanged_non_text(previous, current) {
            return None;
        }

        let delta = text_scroll_delta(&previous.text, &current.text)?;

        let bounds = if has_non_text(current) {
            let bounds = text_scroll_bounds(current)?.intersection(&current.bounds)?;

            if non_text_bounds(current)
                .into_iter()
                .any(|non_text_bounds| non_text_bounds.intersection(&bounds).is_some())
            {
                return None;
            }

            bounds
        } else {
            current.bounds
        };

        if delta.x.abs() > 0.5 || delta.y.abs() < 0.5 || delta.y.abs() >= bounds.height {
            return None;
        }

        Some(Scroll { bounds, delta })
    }
}

fn unchanged_non_text(previous: &Layer, current: &Layer) -> bool {
    previous.quads == current.quads
        && previous.images.len() == current.images.len()
        && previous
            .images
            .iter()
            .zip(&current.images)
            .all(|(previous, current)| previous.eq(current))
        && previous.primitives.len() == current.primitives.len()
        && previous
            .primitives
            .iter()
            .zip(&current.primitives)
            .all(|(previous, current)| unchanged_primitive_item(previous, current))
}

fn unchanged_primitive_item(previous: &Item<Primitive>, current: &Item<Primitive>) -> bool {
    match (previous, current) {
        (
            Item::Cached(previous_primitives, previous_bounds, previous_transformation),
            Item::Cached(current_primitives, current_bounds, current_transformation),
        ) => {
            Arc::ptr_eq(previous_primitives, current_primitives)
                && previous_bounds == current_bounds
                && previous_transformation == current_transformation
        }
        _ => false,
    }
}

fn has_non_text(layer: &Layer) -> bool {
    !layer.quads.is_empty() || !layer.primitives.is_empty() || !layer.images.is_empty()
}

fn text_scroll_bounds(layer: &Layer) -> Option<Rectangle> {
    layer
        .text
        .iter()
        .flat_map(|item| {
            item.as_slice()
                .iter()
                .filter_map(Text::visible_bounds)
                .map(|bounds| bounds * item.transformation())
        })
        .reduce(|a, b| a.union(&b))
}

fn non_text_bounds(layer: &Layer) -> Vec<Rectangle> {
    let mut bounds = layer
        .quads
        .iter()
        .map(|(quad, _)| quad_damage_bounds(quad))
        .collect::<Vec<_>>();

    bounds.extend(layer.primitives.iter().flat_map(|item| {
        match item {
            Item::Live(primitive) => vec![primitive.visible_bounds()],
            Item::Group(primitives, group_bounds, transformation) => primitives
                .as_slice()
                .iter()
                .map(Primitive::visible_bounds)
                .map(|bounds| bounds * *transformation)
                .filter_map(|bounds| bounds.intersection(group_bounds))
                .collect(),
            Item::Cached(_primitives, bounds, _transformation) => vec![*bounds],
        }
    }));

    bounds.extend(layer.images.iter().map(|image| image.bounds().expand(1.0)));

    bounds
}

fn quad_damage_bounds(quad: &Quad) -> Rectangle {
    let bounds = quad.bounds.expand(1.0);

    if quad.shadow.color.a > 0.0 {
        bounds.expand(
            quad.shadow.offset.x.abs().max(quad.shadow.offset.y.abs()) + quad.shadow.blur_radius,
        )
    } else {
        bounds
    }
}

fn text_scroll_delta(previous: &[Item<Text>], current: &[Item<Text>]) -> Option<Vector> {
    let previous = previous.iter().flat_map(Item::as_slice).collect::<Vec<_>>();
    let current = current.iter().flat_map(Item::as_slice).collect::<Vec<_>>();

    crate::graphics::text::scroll_delta(&previous, &current, MAX_SCROLL_MISMATCHED_TEXT_ITEMS)
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            bounds: Rectangle::INFINITE,
            quads: Vec::new(),
            primitives: Vec::new(),
            text: Vec::new(),
            images: Vec::new(),
            opacity: None,
        }
    }
}

impl graphics::Layer for Layer {
    fn with_bounds(bounds: Rectangle) -> Self {
        Self {
            bounds,
            ..Self::default()
        }
    }

    fn bounds(&self) -> Rectangle {
        self.bounds
    }

    fn flush(&mut self) {}

    fn resize(&mut self, bounds: Rectangle) {
        self.bounds = bounds;
    }

    fn reset(&mut self) {
        self.bounds = Rectangle::INFINITE;

        self.quads.clear();
        self.primitives.clear();
        self.text.clear();
        self.images.clear();
        self.opacity = None;
    }

    fn start(&self) -> usize {
        if self.opacity.is_some() {
            return 0;
        }
        if !self.quads.is_empty() {
            return 1;
        }

        if !self.primitives.is_empty() {
            return 2;
        }

        if !self.images.is_empty() {
            return 3;
        }

        if !self.text.is_empty() {
            return 4;
        }

        usize::MAX
    }

    fn end(&self) -> usize {
        if self.opacity.is_some() {
            return usize::MAX;
        }
        if !self.text.is_empty() {
            return 4;
        }

        if !self.images.is_empty() {
            return 3;
        }

        if !self.primitives.is_empty() {
            return 2;
        }

        if !self.quads.is_empty() {
            return 1;
        }

        0
    }

    fn merge(&mut self, layer: &mut Self) {
        self.quads.append(&mut layer.quads);
        self.primitives.append(&mut layer.primitives);
        self.text.append(&mut layer.text);
        self.images.append(&mut layer.images);
    }
}

#[derive(Debug, Clone)]
pub enum Item<T> {
    Live(T),
    Group(Vec<T>, Rectangle, Transformation),
    Cached(Arc<[T]>, Rectangle, Transformation),
}

impl<T> Item<T> {
    pub fn transformation(&self) -> Transformation {
        match self {
            Item::Live(_) => Transformation::IDENTITY,
            Item::Group(_, _, transformation) | Item::Cached(_, _, transformation) => {
                *transformation
            }
        }
    }

    pub fn clip_bounds(&self) -> Rectangle {
        match self {
            Item::Live(_) => Rectangle::INFINITE,
            Item::Group(_, clip_bounds, _) | Item::Cached(_, clip_bounds, _) => *clip_bounds,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match self {
            Item::Live(item) => std::slice::from_ref(item),
            Item::Group(group, _, _) => group.as_slice(),
            Item::Cached(cache, _, _) => cache,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::font::Font;
    use crate::core::renderer;
    use crate::core::text::{Alignment, Ellipsis, Shaping, Wrapping};
    use crate::core::{Pixels, Size};
    use crate::graphics::Layer as _;

    fn cached_text(content: &str, y: f32) -> Text {
        Text::Cached {
            content: content.to_owned(),
            bounds: Rectangle::new(Point::new(10.0, y), Size::new(80.0, 18.0)),
            color: Color::BLACK,
            size: Pixels(14.0),
            line_height: Pixels(18.0),
            font: Font::MONOSPACE,
            align_x: Alignment::Left,
            align_y: crate::core::alignment::Vertical::Top,
            shaping: Shaping::Basic,
            wrapping: Wrapping::None,
            ellipsis: Ellipsis::None,
            clip_bounds: Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)),
        }
    }

    #[test]
    fn detects_text_only_vertical_scroll() {
        let mut previous =
            Layer::with_bounds(Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)));
        let mut current = Layer::with_bounds(previous.bounds);

        previous.text.push(Item::Live(cached_text("a", 0.0)));
        previous.text.push(Item::Live(cached_text("b", 18.0)));
        previous.text.push(Item::Live(cached_text("c", 36.0)));
        current.text.push(Item::Live(cached_text("b", 0.0)));
        current.text.push(Item::Live(cached_text("c", 18.0)));
        current.text.push(Item::Live(cached_text("d", 36.0)));

        let scroll = Layer::scroll(&previous, &current).expect("detect scroll");

        assert_eq!(scroll.delta.y, -18.0);
        assert_eq!(
            Layer::damage_with_scroll(&previous, &current, Some(scroll)),
            vec![Rectangle {
                x: 0.0,
                y: 182.0,
                width: 200.0,
                height: 18.0,
            }]
        );
    }

    #[test]
    fn detects_text_scroll_with_unchanged_non_overlapping_quads() {
        let mut previous =
            Layer::with_bounds(Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)));
        let mut current = Layer::with_bounds(previous.bounds);
        let gutter = (
            renderer::Quad {
                bounds: Rectangle::new(Point::ORIGIN, Size::new(5.0, 200.0)),
                ..renderer::Quad::default()
            },
            Background::Color(Color::WHITE),
        );

        previous.quads.push(gutter);
        current.quads.push(gutter);
        previous.text.push(Item::Live(cached_text("a", 0.0)));
        previous.text.push(Item::Live(cached_text("b", 18.0)));
        previous.text.push(Item::Live(cached_text("c", 36.0)));
        current.text.push(Item::Live(cached_text("b", 0.0)));
        current.text.push(Item::Live(cached_text("c", 18.0)));
        current.text.push(Item::Live(cached_text("d", 36.0)));

        let scroll = Layer::scroll(&previous, &current).expect("detect mixed-layer scroll");

        assert_eq!(scroll.delta.y, -18.0);
        assert_eq!(
            scroll.bounds,
            Rectangle {
                x: 10.0,
                y: 0.0,
                width: 80.0,
                height: 54.0,
            }
        );
    }

    #[test]
    fn does_not_scroll_copy_overlapping_quads() {
        let mut previous =
            Layer::with_bounds(Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)));
        let mut current = Layer::with_bounds(previous.bounds);
        let background = (
            renderer::Quad {
                bounds: Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)),
                ..renderer::Quad::default()
            },
            Background::Color(Color::WHITE),
        );

        previous.quads.push(background);
        current.quads.push(background);
        previous.text.push(Item::Live(cached_text("a", 0.0)));
        previous.text.push(Item::Live(cached_text("b", 18.0)));
        previous.text.push(Item::Live(cached_text("c", 36.0)));
        current.text.push(Item::Live(cached_text("b", 0.0)));
        current.text.push(Item::Live(cached_text("c", 18.0)));
        current.text.push(Item::Live(cached_text("d", 36.0)));

        assert_eq!(Layer::scroll(&previous, &current), None);
    }

    #[test]
    fn does_not_scroll_copy_changed_quads() {
        let mut previous =
            Layer::with_bounds(Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)));
        let mut current = Layer::with_bounds(previous.bounds);

        previous.quads.push((
            renderer::Quad {
                bounds: Rectangle::new(Point::ORIGIN, Size::new(5.0, 200.0)),
                ..renderer::Quad::default()
            },
            Background::Color(Color::WHITE),
        ));
        current.quads.push((
            renderer::Quad {
                bounds: Rectangle::new(Point::ORIGIN, Size::new(6.0, 200.0)),
                ..renderer::Quad::default()
            },
            Background::Color(Color::WHITE),
        ));
        previous.text.push(Item::Live(cached_text("a", 0.0)));
        previous.text.push(Item::Live(cached_text("b", 18.0)));
        previous.text.push(Item::Live(cached_text("c", 36.0)));
        current.text.push(Item::Live(cached_text("b", 0.0)));
        current.text.push(Item::Live(cached_text("c", 18.0)));
        current.text.push(Item::Live(cached_text("d", 36.0)));

        assert_eq!(Layer::scroll(&previous, &current), None);
    }

    #[test]
    fn detects_small_multi_line_text_scroll() {
        let mut previous =
            Layer::with_bounds(Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)));
        let mut current = Layer::with_bounds(previous.bounds);

        for line in 0..35 {
            previous.text.push(Item::Live(cached_text(
                &format!("line {line}"),
                line as f32 * 18.0,
            )));
        }

        for line in 2..37 {
            current.text.push(Item::Live(cached_text(
                &format!("line {line}"),
                (line - 2) as f32 * 18.0,
            )));
        }

        let scroll = Layer::scroll(&previous, &current).expect("detect two-line scroll");

        assert_eq!(scroll.delta.y, -36.0);
        assert_eq!(
            Layer::damage_with_scroll(&previous, &current, Some(scroll)),
            vec![Rectangle {
                x: 0.0,
                y: 164.0,
                width: 200.0,
                height: 36.0,
            }]
        );
    }

    #[test]
    fn rejects_large_text_jump_scroll() {
        let mut previous =
            Layer::with_bounds(Rectangle::new(Point::ORIGIN, Size::new(200.0, 200.0)));
        let mut current = Layer::with_bounds(previous.bounds);

        for line in 0..35 {
            previous.text.push(Item::Live(cached_text(
                &format!("line {line}"),
                line as f32 * 18.0,
            )));
        }

        for line in 20..55 {
            current.text.push(Item::Live(cached_text(
                &format!("line {line}"),
                (line - 20) as f32 * 18.0,
            )));
        }

        assert_eq!(Layer::scroll(&previous, &current), None);
    }
}
