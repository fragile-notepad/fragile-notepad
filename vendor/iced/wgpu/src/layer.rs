use crate::core::{self, Background, Color, Point, Rectangle, Svg, Transformation, renderer};
use crate::graphics;
use crate::graphics::Mesh;
use crate::graphics::color;
use crate::graphics::layer;
use crate::graphics::mesh;
use crate::graphics::text::{Editor, Paragraph};
use crate::image::{self, Image};
use crate::primitive::{self, Primitive};
use crate::quad::{self, Quad};
use crate::text::{self, Text};
use crate::triangle;

#[cfg(test)]
mod tests;

pub type Stack = layer::Stack<Layer>;

#[derive(Debug)]
pub struct Layer {
    pub bounds: Rectangle,
    pub quads: quad::Batch,
    pub triangles: triangle::Batch,
    pub primitives: primitive::Batch,
    pub images: image::Batch,
    pub text: text::Batch,
    pending_meshes: Vec<Mesh>,
    pending_text: Vec<Text>,
}

impl Layer {
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
            && self.triangles.is_empty()
            && self.primitives.is_empty()
            && self.images.is_empty()
            && self.text.is_empty()
            && self.pending_meshes.is_empty()
            && self.pending_text.is_empty()
    }

    pub fn draw_quad(
        &mut self,
        quad: renderer::Quad,
        background: Background,
        transformation: Transformation,
    ) {
        let bounds = quad.bounds * transformation;

        let quad = Quad {
            position: [bounds.x, bounds.y],
            size: [bounds.width, bounds.height],
            border_color: color::pack(quad.border.color),
            border_radius: (quad.border.radius * transformation.scale_factor()).into(),
            border_width: quad.border.width * transformation.scale_factor(),
            shadow_color: color::pack(quad.shadow.color),
            shadow_offset: (quad.shadow.offset * transformation.scale_factor()).into(),
            shadow_blur_radius: quad.shadow.blur_radius * transformation.scale_factor(),
            snap: quad.snap as u32,
        };

        self.quads.add(quad, &background);
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

        self.pending_text.push(paragraph);
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

        self.pending_text.push(editor);
    }

    pub fn draw_text(
        &mut self,
        text: crate::core::Text,
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

        self.pending_text.push(text);
    }

    pub fn draw_text_raw(&mut self, raw: graphics::text::Raw, transformation: Transformation) {
        let raw = Text::Raw {
            raw,
            transformation,
        };

        self.pending_text.push(raw);
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

    pub fn draw_mesh(&mut self, mut mesh: Mesh, transformation: Transformation) {
        match &mut mesh {
            Mesh::Solid {
                transformation: local_transformation,
                ..
            }
            | Mesh::Gradient {
                transformation: local_transformation,
                ..
            } => {
                *local_transformation = *local_transformation * transformation;
            }
        }

        self.pending_meshes.push(mesh);
    }

    pub fn draw_mesh_group(&mut self, meshes: Vec<Mesh>, transformation: Transformation) {
        self.flush_meshes();

        self.triangles.push(triangle::Item::Group {
            meshes,
            transformation,
        });
    }

    pub fn draw_mesh_cache(&mut self, cache: mesh::Cache, transformation: Transformation) {
        self.flush_meshes();

        self.triangles.push(triangle::Item::Cached {
            cache,
            transformation,
        });
    }

    pub fn draw_text_group(&mut self, text: Vec<Text>, transformation: Transformation) {
        self.flush_text();

        self.push_text_group(text, transformation);
    }

    pub fn draw_text_cache(&mut self, cache: text::Cache, transformation: Transformation) {
        self.flush_text();

        self.text.push(text::Item::Cached {
            cache,
            transformation,
        });
    }

    pub fn draw_primitive(
        &mut self,
        bounds: Rectangle,
        primitive: impl Primitive,
        transformation: Transformation,
    ) {
        let bounds = bounds * transformation;

        self.primitives
            .push(primitive::Instance::new(bounds, primitive));
    }

    fn flush_meshes(&mut self) {
        if !self.pending_meshes.is_empty() {
            self.triangles.push(triangle::Item::Group {
                transformation: Transformation::IDENTITY,
                meshes: self.pending_meshes.drain(..).collect(),
            });
        }
    }

    fn flush_text(&mut self) {
        if !self.pending_text.is_empty() {
            match self.text.last_mut() {
                Some(text::Item::Group {
                    transformation,
                    text,
                }) if *transformation == Transformation::IDENTITY => {
                    // Reuse the group and retain the pending allocation for
                    // later draws instead of allocating another group.
                    text.append(&mut self.pending_text);
                }
                _ => {
                    self.text.push(text::Item::Group {
                        transformation: Transformation::IDENTITY,
                        text: self.pending_text.drain(..).collect(),
                    });
                }
            }
        }
    }

    fn push_text_group(&mut self, mut text: Vec<Text>, transformation: Transformation) {
        match self.text.last_mut() {
            Some(text::Item::Group {
                transformation: previous_transformation,
                text: previous_text,
            }) if *previous_transformation == transformation => {
                previous_text.append(&mut text);
            }
            _ => {
                self.text.push(text::Item::Group {
                    text,
                    transformation,
                });
            }
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

    fn flush(&mut self) {
        self.flush_meshes();
        self.flush_text();
    }

    fn resize(&mut self, bounds: Rectangle) {
        self.bounds = bounds;
    }

    fn reset(&mut self) {
        self.bounds = Rectangle::INFINITE;

        self.quads.clear();
        self.triangles.clear();
        self.primitives.clear();
        self.text.clear();
        self.images.clear();
        self.pending_meshes.clear();
        self.pending_text.clear();
    }

    fn start(&self) -> usize {
        if !self.quads.is_empty() {
            return 1;
        }

        if !self.triangles.is_empty() {
            return 2;
        }

        if !self.primitives.is_empty() {
            return 3;
        }

        if !self.images.is_empty() {
            return 4;
        }

        if !self.text.is_empty() {
            return 5;
        }

        usize::MAX
    }

    fn end(&self) -> usize {
        if !self.text.is_empty() {
            return 5;
        }

        if !self.images.is_empty() {
            return 4;
        }

        if !self.primitives.is_empty() {
            return 3;
        }

        if !self.triangles.is_empty() {
            return 2;
        }

        if !self.quads.is_empty() {
            return 1;
        }

        0
    }

    fn merge(&mut self, layer: &mut Self) {
        self.quads.append(&mut layer.quads);
        self.triangles.append(&mut layer.triangles);
        self.primitives.append(&mut layer.primitives);
        self.images.append(&mut layer.images);

        // The stack only merges layers with matching clip bounds. Within
        // those bounds, preserve cached items and transformation changes as
        // barriers while joining adjacent compatible groups in draw order.
        for item in layer.text.drain(..) {
            match item {
                text::Item::Group {
                    text,
                    transformation,
                } => self.push_text_group(text, transformation),
                item => self.text.push(item),
            }
        }
    }
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            bounds: Rectangle::INFINITE,
            quads: quad::Batch::default(),
            triangles: triangle::Batch::default(),
            primitives: primitive::Batch::default(),
            text: text::Batch::default(),
            images: image::Batch::default(),
            pending_meshes: Vec::new(),
            pending_text: Vec::new(),
        }
    }
}
