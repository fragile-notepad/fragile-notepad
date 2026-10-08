use crate::Primitive;
use crate::core::renderer::Quad;
use crate::core::{Background, Color, Gradient, Rectangle, Size, Transformation, Vector};
use crate::graphics::{Image, Text};
use crate::text::{self, PipelineStats};
use crate::trace;

#[derive(Debug)]
pub struct Engine {
    text_pipeline: text::Pipeline,

    #[cfg(feature = "image")]
    pub(crate) raster_pipeline: crate::raster::Pipeline,
    #[cfg(feature = "svg")]
    pub(crate) vector_pipeline: crate::vector::Pipeline,
}

pub(crate) struct ClipMask<'a> {
    mask: &'a mut tiny_skia::Mask,
    bounds: Option<Rectangle>,
    rebuilds: usize,
    reuses: usize,
}

impl<'a> ClipMask<'a> {
    pub(crate) fn new(mask: &'a mut tiny_skia::Mask) -> Self {
        Self {
            mask,
            bounds: None,
            rebuilds: 0,
            reuses: 0,
        }
    }

    pub(crate) fn set(&mut self, bounds: Rectangle) -> &tiny_skia::Mask {
        if self.bounds != Some(bounds) {
            if trace::enabled() {
                trace::event(
                    "tiny_skia_clip_mask_rebuild",
                    0,
                    format_args!("bounds={}", format_rect(bounds)),
                );
            }
            if let Some(previous) = self.bounds {
                clear_mask_region(self.mask, previous);
                fill_clip_mask(self.mask, bounds);
            } else {
                adjust_clip_mask(self.mask, bounds);
            }
            self.bounds = Some(bounds);
            self.rebuilds += 1;
        } else {
            self.reuses += 1;
        }

        self.mask
    }

    pub(crate) fn stats(&self) -> ClipMaskStats {
        ClipMaskStats {
            rebuilds: self.rebuilds,
            reuses: self.reuses,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ClipMaskStats {
    pub rebuilds: usize,
    pub reuses: usize,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            text_pipeline: text::Pipeline::new(),
            #[cfg(feature = "image")]
            raster_pipeline: crate::raster::Pipeline::new(),
            #[cfg(feature = "svg")]
            vector_pipeline: crate::vector::Pipeline::new(),
        }
    }

    pub(crate) fn text_stats(&self) -> PipelineStats {
        self.text_pipeline.stats()
    }

    pub fn draw_quad(
        &mut self,
        quad: &Quad,
        background: &Background,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut ClipMask<'_>,
        clip_bounds: Rectangle,
    ) {
        let physical_bounds = quad.bounds * transformation;
        // Rounded paths must honor the same pixel-grid snapping as solid
        // rectangles. Keep curves anti-aliased after aligning their bounds.
        let physical_bounds = if quad.snap {
            (physical_bounds + Vector::new(0.001, 0.001)).round()
        } else {
            physical_bounds
        };
        let quad = Quad {
            bounds: if quad.snap {
                physical_bounds * transformation.inverse()
            } else {
                quad.bounds
            },
            ..*quad
        };

        if physical_bounds.width <= 0.0
            || physical_bounds.height <= 0.0
            || !clip_bounds.intersects(&physical_bounds)
        {
            if trace::enabled() {
                trace::event(
                    "tiny_skia_engine_quad_skip",
                    0,
                    format_args!(
                        "reason=no_intersection physical={} clip={}",
                        format_rect(physical_bounds),
                        format_rect(clip_bounds)
                    ),
                );
            }
            return;
        }

        if let (Background::Color(color), [0.0, 0.0, 0.0, 0.0], 0.0, 0.0) = (
            background,
            <[f32; 4]>::from(quad.border.radius),
            quad.border.width,
            quad.shadow.color.a,
        ) {
            if let Some(visible_bounds) = physical_bounds.intersection(&clip_bounds)
                && let Some(rect) = tiny_skia::Rect::from_xywh(
                    visible_bounds.x,
                    visible_bounds.y,
                    visible_bounds.width,
                    visible_bounds.height,
                )
            {
                // tiny-skia 0.11's anti-aliased rectangle rasterizer can
                // round a fractional one-pixel edge into a zero-width inner
                // span and trip a debug assertion. Keep the fast path for
                // snapped quads and visible rectangles at least two pixels
                // wide and high; let the path rasterizer handle thinner
                // anti-aliased geometry.
                if quad.snap || (visible_bounds.width >= 2.0 && visible_bounds.height >= 2.0) {
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_engine_quad_fast",
                            0,
                            format_args!(
                                "physical={} visible={} clip={} mask=false",
                                format_rect(physical_bounds),
                                format_rect(visible_bounds),
                                format_rect(clip_bounds)
                            ),
                        );
                    }

                    pixels.fill_rect(
                        rect,
                        &tiny_skia::Paint {
                            shader: tiny_skia::Shader::SolidColor(into_color(*color)),
                            anti_alias: !quad.snap,
                            ..tiny_skia::Paint::default()
                        },
                        tiny_skia::Transform::identity(),
                        None,
                    );

                    return;
                }
            }
        }

        let transform = into_transform(transformation);

        // Make sure the border radius is not larger than the bounds
        let border_width = quad
            .border
            .width
            .min(quad.bounds.width / 2.0)
            .min(quad.bounds.height / 2.0);

        let mut fill_border_radius = <[f32; 4]>::from(quad.border.radius);

        for radius in &mut fill_border_radius {
            *radius = (*radius)
                .min(quad.bounds.width / 2.0)
                .min(quad.bounds.height / 2.0);
        }

        let path = rounded_rectangle(quad.bounds, fill_border_radius);

        let shadow = quad.shadow;

        if shadow.color.a > 0.0 {
            let shadow_bounds = Rectangle {
                x: quad.bounds.x + shadow.offset.x - shadow.blur_radius,
                y: quad.bounds.y + shadow.offset.y - shadow.blur_radius,
                width: quad.bounds.width + shadow.blur_radius * 2.0,
                height: quad.bounds.height + shadow.blur_radius * 2.0,
            } * transformation;

            let radii = fill_border_radius
                .into_iter()
                .map(|radius| radius * transformation.scale_factor())
                .collect::<Vec<_>>();
            let (x, y, width, height) = (
                shadow_bounds.x as u32,
                shadow_bounds.y as u32,
                shadow_bounds.width as u32,
                shadow_bounds.height as u32,
            );
            let half_width = physical_bounds.width / 2.0;
            let half_height = physical_bounds.height / 2.0;

            let colors = (y..y + height)
                .flat_map(|y| (x..x + width).map(move |x| (x as f32, y as f32)))
                .filter_map(|(x, y)| {
                    tiny_skia::Size::from_wh(half_width, half_height).map(|size| {
                        let shadow_distance = rounded_box_sdf(
                            Vector::new(
                                x - physical_bounds.position().x
                                    - (shadow.offset.x * transformation.scale_factor())
                                    - half_width,
                                y - physical_bounds.position().y
                                    - (shadow.offset.y * transformation.scale_factor())
                                    - half_height,
                            ),
                            size,
                            &radii,
                        )
                        .max(0.0);
                        let shadow_alpha = 1.0
                            - smoothstep(
                                -shadow.blur_radius * transformation.scale_factor(),
                                shadow.blur_radius * transformation.scale_factor(),
                                shadow_distance,
                            );

                        let mut color = into_color(shadow.color);
                        color.apply_opacity(shadow_alpha);

                        color.to_color_u8().premultiply()
                    })
                })
                .collect();

            if let Some(pixmap) = tiny_skia::IntSize::from_wh(width, height)
                .and_then(|size| tiny_skia::Pixmap::from_vec(bytemuck::cast_vec(colors), size))
            {
                pixels.draw_pixmap(
                    x as i32,
                    y as i32,
                    pixmap.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::default(),
                    Some(clip_mask.set(clip_bounds)),
                );
            }
        }

        let clip_mask = if physical_bounds.is_within(&clip_bounds) {
            None
        } else {
            Some(clip_mask.set(clip_bounds))
        };

        if trace::enabled() {
            trace::event(
                "tiny_skia_engine_quad_path",
                0,
                format_args!(
                    "physical={} clip={} mask={}",
                    format_rect(physical_bounds),
                    format_rect(clip_bounds),
                    clip_mask.is_some()
                ),
            );
        }

        pixels.fill_path(
            &path,
            &tiny_skia::Paint {
                shader: match background {
                    Background::Color(color) => tiny_skia::Shader::SolidColor(into_color(*color)),
                    Background::Gradient(Gradient::Linear(linear)) => {
                        let (start, end) = quad.bounds.chord(linear.angle);

                        let stops: Vec<tiny_skia::GradientStop> = linear
                            .stops
                            .into_iter()
                            .flatten()
                            .map(|stop| {
                                tiny_skia::GradientStop::new(
                                    stop.offset,
                                    tiny_skia::Color::from_rgba(
                                        stop.color.b,
                                        stop.color.g,
                                        stop.color.r,
                                        stop.color.a,
                                    )
                                    .expect("Create color"),
                                )
                            })
                            .collect();

                        tiny_skia::LinearGradient::new(
                            tiny_skia::Point {
                                x: start.x,
                                y: start.y,
                            },
                            tiny_skia::Point { x: end.x, y: end.y },
                            if stops.is_empty() {
                                vec![tiny_skia::GradientStop::new(0.0, tiny_skia::Color::BLACK)]
                            } else {
                                stops
                            },
                            tiny_skia::SpreadMode::Pad,
                            tiny_skia::Transform::identity(),
                        )
                        .expect("Create linear gradient")
                    }
                },
                anti_alias: true,
                ..tiny_skia::Paint::default()
            },
            tiny_skia::FillRule::EvenOdd,
            transform,
            clip_mask,
        );

        if border_width > 0.0 {
            // Border path is offset by half the border width
            let border_bounds = Rectangle {
                x: quad.bounds.x + border_width / 2.0,
                y: quad.bounds.y + border_width / 2.0,
                width: quad.bounds.width - border_width,
                height: quad.bounds.height - border_width,
            };

            // Make sure the border radius is correct
            let mut border_radius = <[f32; 4]>::from(quad.border.radius);
            let mut is_simple_border = true;

            for radius in &mut border_radius {
                *radius = if *radius == 0.0 {
                    // Path should handle this fine
                    0.0
                } else if *radius > border_width / 2.0 {
                    *radius - border_width / 2.0
                } else {
                    is_simple_border = false;
                    0.0
                }
                .min(border_bounds.width / 2.0)
                .min(border_bounds.height / 2.0);
            }

            // Stroking a path works well in this case
            if is_simple_border {
                let border_path = rounded_rectangle(border_bounds, border_radius);

                pixels.stroke_path(
                    &border_path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(into_color(quad.border.color)),
                        anti_alias: true,
                        ..tiny_skia::Paint::default()
                    },
                    &tiny_skia::Stroke {
                        width: border_width,
                        ..tiny_skia::Stroke::default()
                    },
                    transform,
                    clip_mask,
                );
            } else {
                // Draw corners that have too small border radii as having no border radius,
                // but mask them with the rounded rectangle with the correct border radius.
                let mut temp_pixmap =
                    tiny_skia::Pixmap::new(quad.bounds.width as u32, quad.bounds.height as u32)
                        .unwrap();

                let mut quad_mask =
                    tiny_skia::Mask::new(quad.bounds.width as u32, quad.bounds.height as u32)
                        .unwrap();

                let zero_bounds = Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: quad.bounds.width,
                    height: quad.bounds.height,
                };
                let path = rounded_rectangle(zero_bounds, fill_border_radius);

                quad_mask.fill_path(&path, tiny_skia::FillRule::EvenOdd, true, transform);
                let path_bounds = Rectangle {
                    x: border_width / 2.0,
                    y: border_width / 2.0,
                    width: quad.bounds.width - border_width,
                    height: quad.bounds.height - border_width,
                };

                let border_radius_path = rounded_rectangle(path_bounds, border_radius);

                temp_pixmap.stroke_path(
                    &border_radius_path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(into_color(quad.border.color)),
                        anti_alias: true,
                        ..tiny_skia::Paint::default()
                    },
                    &tiny_skia::Stroke {
                        width: border_width,
                        ..tiny_skia::Stroke::default()
                    },
                    transform,
                    Some(&quad_mask),
                );

                pixels.draw_pixmap(
                    quad.bounds.x as i32,
                    quad.bounds.y as i32,
                    temp_pixmap.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    transform,
                    clip_mask,
                );
            }
        }
    }

    pub fn draw_text(
        &mut self,
        text: &Text,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        _clip_mask: &mut ClipMask<'_>,
        clip_bounds: Rectangle,
    ) {
        match text {
            Text::Paragraph {
                paragraph,
                position,
                color,
                clip_bounds: local_clip_bounds,
                transformation: local_transformation,
            } => {
                let transformation = transformation * *local_transformation;
                let Some(clip_bounds) =
                    clip_bounds.intersection(&(*local_clip_bounds * transformation))
                else {
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_engine_text_skip",
                            0,
                            format_args!(
                                "kind=paragraph reason=no_clip_intersection outer_clip={} local_clip={}",
                                format_rect(clip_bounds),
                                format_rect(*local_clip_bounds * transformation)
                            ),
                        );
                    }
                    return;
                };

                let physical_bounds =
                    Rectangle::new(*position, paragraph.min_bounds) * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_engine_text_skip",
                            0,
                            format_args!(
                                "kind=paragraph reason=no_bounds_intersection physical={} clip={}",
                                format_rect(physical_bounds),
                                format_rect(clip_bounds)
                            ),
                        );
                    }
                    return;
                }

                let Some(visible_bounds) = physical_bounds.intersection(&clip_bounds) else {
                    return;
                };

                let run_clip_bounds = (visible_bounds != physical_bounds).then_some(clip_bounds);

                if trace::enabled() {
                    trace::event(
                        "tiny_skia_engine_text_draw",
                        0,
                        format_args!(
                            "kind=paragraph physical={} visible={} clip={} mask={}",
                            format_rect(physical_bounds),
                            format_rect(visible_bounds),
                            format_rect(clip_bounds),
                            run_clip_bounds.is_some()
                        ),
                    );
                }

                self.text_pipeline.draw_paragraph(
                    paragraph,
                    *position,
                    *color,
                    pixels,
                    None,
                    transformation,
                    run_clip_bounds,
                );
            }
            Text::Editor {
                editor,
                position,
                color,
                clip_bounds: local_clip_bounds,
                transformation: local_transformation,
            } => {
                let transformation = transformation * *local_transformation;

                let Some(clip_bounds) =
                    clip_bounds.intersection(&(*local_clip_bounds * transformation))
                else {
                    return;
                };

                self.text_pipeline.draw_editor(
                    editor,
                    *position,
                    *color,
                    pixels,
                    None,
                    transformation,
                    Some(clip_bounds),
                );
            }
            Text::Cached {
                content,
                bounds,
                color,
                size,
                line_height,
                font,
                align_x,
                align_y,
                shaping,
                wrapping,
                ellipsis,
                clip_bounds: local_clip_bounds,
            } => {
                let physical_bounds = *bounds * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_engine_text_skip",
                            0,
                            format_args!(
                                "kind=cached reason=no_outer_intersection physical={} outer_clip={}",
                                format_rect(physical_bounds),
                                format_rect(clip_bounds)
                            ),
                        );
                    }
                    return;
                }

                let Some(clip_bounds) =
                    clip_bounds.intersection(&(*local_clip_bounds * transformation))
                else {
                    if trace::enabled() {
                        trace::event(
                            "tiny_skia_engine_text_skip",
                            0,
                            format_args!(
                                "kind=cached reason=no_local_intersection outer_clip={} local_clip={}",
                                format_rect(clip_bounds),
                                format_rect(*local_clip_bounds * transformation)
                            ),
                        );
                    }
                    return;
                };

                let Some(visible_bounds) = physical_bounds.intersection(&clip_bounds) else {
                    return;
                };

                let run_clip_bounds = (visible_bounds != physical_bounds).then_some(clip_bounds);

                if trace::enabled() {
                    trace::event(
                        "tiny_skia_engine_text_draw",
                        0,
                        format_args!(
                            "kind=cached physical={} visible={} clip={} mask={} run_clip={}",
                            format_rect(physical_bounds),
                            format_rect(visible_bounds),
                            format_rect(clip_bounds),
                            run_clip_bounds.is_some(),
                            run_clip_bounds.is_some()
                        ),
                    );
                }

                self.text_pipeline.draw_cached(
                    content,
                    *bounds,
                    *color,
                    *size,
                    *line_height,
                    *font,
                    *align_x,
                    *align_y,
                    *shaping,
                    *wrapping,
                    *ellipsis,
                    pixels,
                    None,
                    transformation,
                    run_clip_bounds,
                );
            }
            Text::Raw {
                raw,
                transformation: local_transformation,
            } => {
                let Some(buffer) = raw.buffer.upgrade() else {
                    return;
                };

                let transformation = transformation * *local_transformation;
                let (width, height) = buffer.size();

                let physical_bounds = Rectangle::new(
                    raw.position,
                    Size::new(
                        width.unwrap_or(clip_bounds.width),
                        height.unwrap_or(clip_bounds.height),
                    ),
                ) * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let run_clip_bounds = clip_bounds
                    .intersection(&physical_bounds)
                    .filter(|visible_bounds| *visible_bounds != physical_bounds)
                    .map(|_| clip_bounds);

                self.text_pipeline.draw_raw(
                    &buffer,
                    raw.position,
                    raw.color,
                    pixels,
                    None,
                    transformation,
                    run_clip_bounds,
                );
            }
        }
    }

    pub fn draw_primitive(
        &mut self,
        primitive: &Primitive,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut ClipMask<'_>,
        clip_bounds: Rectangle,
    ) {
        match primitive {
            Primitive::Fill { path, paint, rule } => {
                let physical_bounds = {
                    let bounds = path.bounds();

                    Rectangle {
                        x: bounds.x(),
                        y: bounds.y(),
                        width: bounds.width(),
                        height: bounds.height(),
                    } * transformation
                };

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = if physical_bounds.is_within(&clip_bounds) {
                    None
                } else {
                    Some(clip_mask.set(clip_bounds))
                };

                pixels.fill_path(
                    path,
                    paint,
                    *rule,
                    into_transform(transformation),
                    clip_mask,
                );
            }
            Primitive::Stroke {
                path,
                paint,
                stroke,
            } => {
                let physical_bounds = {
                    let bounds = path.bounds();

                    Rectangle {
                        x: bounds.x() - stroke.width / 2.0,
                        y: bounds.y() - stroke.width / 2.0,
                        width: bounds.width() + stroke.width,
                        height: bounds.height() + stroke.width,
                    } * transformation
                };

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = if physical_bounds.is_within(&clip_bounds) {
                    None
                } else {
                    Some(clip_mask.set(clip_bounds))
                };

                pixels.stroke_path(
                    path,
                    paint,
                    stroke,
                    into_transform(transformation),
                    clip_mask,
                );
            }
        }
    }

    pub fn draw_image(
        &mut self,
        image: &Image,
        _transformation: Transformation,
        _pixels: &mut tiny_skia::PixmapMut<'_>,
        _clip_mask: &mut ClipMask<'_>,
        _clip_bounds: Rectangle,
    ) {
        match image {
            #[cfg(feature = "image")]
            Image::Raster {
                image,
                bounds,
                clip_bounds: local_clip_bounds,
            } => {
                let physical_bounds = *local_clip_bounds * _transformation;

                let Some(clip_bounds) = physical_bounds.intersection(&_clip_bounds) else {
                    return;
                };

                let center = bounds.center();
                let radians = f32::from(image.rotation);

                let transform = into_transform(_transformation).pre_rotate_at(
                    radians.to_degrees(),
                    center.x,
                    center.y,
                );

                self.raster_pipeline.draw(
                    &image.handle,
                    image.filter_method,
                    *bounds,
                    image.opacity,
                    image.snap,
                    _pixels,
                    transform,
                    Some(_clip_mask.set(clip_bounds)),
                );
            }
            #[cfg(feature = "svg")]
            Image::Vector { svg, bounds, .. } => {
                let physical_bounds = *bounds * _transformation;

                if !_clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = if physical_bounds.is_within(&_clip_bounds) {
                    None
                } else {
                    Some(_clip_mask.set(_clip_bounds))
                };

                let center = physical_bounds.center();
                let radians = f32::from(svg.rotation);

                let transform = into_transform(_transformation).post_rotate_at(
                    radians.to_degrees(),
                    center.x,
                    center.y,
                );

                self.vector_pipeline.draw(
                    &svg.handle,
                    svg.color,
                    *bounds,
                    svg.opacity,
                    _pixels,
                    transform,
                    clip_mask,
                );
            }
            #[cfg(not(feature = "image"))]
            Image::Raster { .. } => {
                log::warn!("Unsupported primitive in `iced_tiny_skia`: {image:?}",);
            }
            #[cfg(not(feature = "svg"))]
            Image::Vector { .. } => {
                log::warn!("Unsupported primitive in `iced_tiny_skia`: {image:?}",);
            }
        }
    }

    pub fn trim(&mut self) {
        self.text_pipeline.trim_cache();

        #[cfg(feature = "image")]
        self.raster_pipeline.trim_cache();

        #[cfg(feature = "svg")]
        self.vector_pipeline.trim_cache();
    }
}

pub fn into_color(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(color.b, color.g, color.r, color.a)
        .expect("Convert color from iced to tiny_skia")
}

fn into_transform(transformation: Transformation) -> tiny_skia::Transform {
    let translation = transformation.translation();

    tiny_skia::Transform {
        sx: transformation.scale_factor(),
        kx: 0.0,
        ky: 0.0,
        sy: transformation.scale_factor(),
        tx: translation.x,
        ty: translation.y,
    }
}

fn rounded_rectangle(bounds: Rectangle, border_radius: [f32; 4]) -> tiny_skia::Path {
    let [top_left, top_right, bottom_right, bottom_left] = border_radius;

    if top_left == 0.0 && top_right == 0.0 && bottom_right == 0.0 && bottom_left == 0.0 {
        return tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height)
                .expect("Build quad rectangle"),
        );
    }

    if top_left == top_right
        && top_left == bottom_right
        && top_left == bottom_left
        && top_left == bounds.width / 2.0
        && top_left == bounds.height / 2.0
    {
        return tiny_skia::PathBuilder::from_circle(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
            top_left,
        )
        .expect("Build circle path");
    }

    let mut builder = tiny_skia::PathBuilder::new();

    builder.move_to(bounds.x + top_left, bounds.y);
    builder.line_to(bounds.x + bounds.width - top_right, bounds.y);

    if top_right > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bounds.width - top_right,
            bounds.y,
            bounds.x + bounds.width,
            bounds.y + top_right,
            top_right,
        );
    }

    maybe_line_to(
        &mut builder,
        bounds.x + bounds.width,
        bounds.y + bounds.height - bottom_right,
    );

    if bottom_right > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bounds.width,
            bounds.y + bounds.height - bottom_right,
            bounds.x + bounds.width - bottom_right,
            bounds.y + bounds.height,
            bottom_right,
        );
    }

    maybe_line_to(
        &mut builder,
        bounds.x + bottom_left,
        bounds.y + bounds.height,
    );

    if bottom_left > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bottom_left,
            bounds.y + bounds.height,
            bounds.x,
            bounds.y + bounds.height - bottom_left,
            bottom_left,
        );
    }

    maybe_line_to(&mut builder, bounds.x, bounds.y + top_left);

    if top_left > 0.0 {
        arc_to(
            &mut builder,
            bounds.x,
            bounds.y + top_left,
            bounds.x + top_left,
            bounds.y,
            top_left,
        );
    }

    builder.finish().expect("Build rounded rectangle path")
}

fn maybe_line_to(path: &mut tiny_skia::PathBuilder, x: f32, y: f32) {
    if path.last_point() != Some(tiny_skia::Point { x, y }) {
        path.line_to(x, y);
    }
}

fn arc_to(
    path: &mut tiny_skia::PathBuilder,
    x_from: f32,
    y_from: f32,
    x_to: f32,
    y_to: f32,
    radius: f32,
) {
    let svg_arc = kurbo::SvgArc {
        from: kurbo::Point::new(f64::from(x_from), f64::from(y_from)),
        to: kurbo::Point::new(f64::from(x_to), f64::from(y_to)),
        radii: kurbo::Vec2::new(f64::from(radius), f64::from(radius)),
        x_rotation: 0.0,
        large_arc: false,
        sweep: true,
    };

    match kurbo::Arc::from_svg_arc(&svg_arc) {
        Some(arc) => {
            arc.to_cubic_beziers(0.1, |p1, p2, p| {
                path.cubic_to(
                    p1.x as f32,
                    p1.y as f32,
                    p2.x as f32,
                    p2.y as f32,
                    p.x as f32,
                    p.y as f32,
                );
            });
        }
        None => {
            path.line_to(x_to, y_to);
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let x = ((x - a) / (b - a)).clamp(0.0, 1.0);

    x * x * (3.0 - 2.0 * x)
}

fn rounded_box_sdf(to_center: Vector, size: tiny_skia::Size, radii: &[f32]) -> f32 {
    let radius = match (to_center.x > 0.0, to_center.y > 0.0) {
        (true, true) => radii[2],
        (true, false) => radii[1],
        (false, true) => radii[3],
        (false, false) => radii[0],
    };

    let x = (to_center.x.abs() - size.width() + radius).max(0.0);
    let y = (to_center.y.abs() - size.height() + radius).max(0.0);

    (x.powf(2.0) + y.powf(2.0)).sqrt() - radius
}

pub fn adjust_clip_mask(clip_mask: &mut tiny_skia::Mask, bounds: Rectangle) {
    clip_mask.clear();
    fill_clip_mask(clip_mask, bounds);
}

fn clear_mask_region(mask: &mut tiny_skia::Mask, bounds: Rectangle) {
    // Include a conservative pixel fringe while retaining the same tiny-skia
    // path rasterizer for fractional rectangle coverage.
    let width = mask.width() as usize;
    let left = (bounds.x.floor() - 1.0).clamp(0.0, mask.width() as f32) as usize;
    let right = ((bounds.x + bounds.width).ceil() + 1.0).clamp(0.0, mask.width() as f32) as usize;
    let top = (bounds.y.floor() - 1.0).clamp(0.0, mask.height() as f32) as usize;
    let bottom =
        ((bounds.y + bounds.height).ceil() + 1.0).clamp(0.0, mask.height() as f32) as usize;
    for row in top..bottom {
        mask.data_mut()[row * width + left..row * width + right].fill(0);
    }
}

fn fill_clip_mask(clip_mask: &mut tiny_skia::Mask, bounds: Rectangle) {
    let path = tiny_skia::PathBuilder::from_rect(
        tiny_skia::Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height)
            .expect("Create clip rectangle"),
    );

    clip_mask.fill_path(
        &path,
        tiny_skia::FillRule::EvenOdd,
        false,
        tiny_skia::Transform::default(),
    );
}

fn format_rect(rect: Rectangle) -> String {
    format!(
        "[{:.1},{:.1},{:.1},{:.1}]",
        rect.x, rect.y, rect.width, rect.height
    )
}

#[cfg(test)]
mod clip_mask_tests {
    use super::*;

    #[test]
    fn rounded_quads_snap_after_scaling_and_translation() {
        let mut engine = Engine::new();
        let mut render = |quad: Quad, transformation| {
            let mut pixmap = tiny_skia::Pixmap::new(64, 64).unwrap();
            let mut mask = tiny_skia::Mask::new(64, 64).unwrap();
            engine.draw_quad(
                &quad,
                &Background::Color(Color::BLACK),
                transformation,
                &mut pixmap.as_mut(),
                &mut ClipMask::new(&mut mask),
                Rectangle::with_size(Size::new(64.0, 64.0)),
            );
            pixmap
        };
        for (scale, left, top, right, bottom) in [
            (1.0, 6.0, 5.0, 22.0, 18.0),
            (1.5, 8.0, 8.0, 33.0, 26.0),
            (2.0, 11.0, 10.0, 43.0, 34.0),
        ] {
            let transformation =
                Transformation::translate(0.25, 0.75) * Transformation::scale(scale);
            let quad = Quad {
                bounds: Rectangle {
                    x: 5.25,
                    y: 4.5,
                    width: 16.25,
                    height: 12.25,
                },
                border: crate::core::Border {
                    radius: 2.0.into(),
                    ..crate::core::Border::default()
                },
                snap: true,
                ..Quad::default()
            };
            let actual = render(quad, transformation);
            let fractional = render(
                Quad {
                    snap: false,
                    ..quad
                },
                transformation,
            );
            let expected = render(
                Quad {
                    bounds: Rectangle {
                        x: left,
                        y: top,
                        width: right - left,
                        height: bottom - top,
                    },
                    border: crate::core::Border {
                        radius: (2.0 * scale).into(),
                        ..quad.border
                    },
                    snap: false,
                    ..quad
                },
                Transformation::IDENTITY,
            );
            assert_eq!(actual.data(), expected.data(), "scale={scale}");
            assert_ne!(actual.data(), fractional.data(), "scale={scale}");
        }
    }

    #[test]
    fn fractional_hairline_quad_avoids_tiny_skia_aa_assertion() {
        let mut pixmap = tiny_skia::Pixmap::new(16, 16).unwrap();
        let mut pixels = pixmap.as_mut();
        let mut mask = tiny_skia::Mask::new(16, 16).unwrap();
        let mut clip_mask = ClipMask::new(&mut mask);
        let mut engine = Engine::new();
        let quad = Quad {
            bounds: Rectangle {
                x: 0.1,
                y: 0.0,
                width: 1.0,
                height: 8.0,
            },
            snap: false,
            ..Quad::default()
        };

        engine.draw_quad(
            &quad,
            &Background::Color(Color::BLACK),
            Transformation::IDENTITY,
            &mut pixels,
            &mut clip_mask,
            Rectangle::with_size(Size::new(16.0, 16.0)),
        );

        assert!(pixmap.data().chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn reused_mask_matches_full_clear_for_fractional_and_clipped_rectangles() {
        let mut actual = tiny_skia::Mask::new(64, 48).unwrap();
        actual.data_mut().fill(113);
        let mut expected = actual.clone();
        let mut cached = ClipMask::new(&mut actual);
        for i in 0..200 {
            let bounds = Rectangle {
                x: ((i * 17) % 81) as f32 - 10.25,
                y: ((i * 11) % 61) as f32 - 8.75,
                width: ((i * 3) % 40) as f32 + 0.25,
                height: ((i * 7) % 35) as f32 + 0.75,
            };
            adjust_clip_mask(&mut expected, bounds);
            assert_eq!(cached.set(bounds).data(), expected.data(), "{bounds:?}");
            assert_eq!(cached.set(bounds).data(), expected.data());
        }
        assert_eq!(cached.stats().reuses, 200);
    }
}
