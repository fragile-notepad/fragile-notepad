use iced::advanced::{image as advanced_image, renderer, text};
use iced::{Background, Font, Rectangle};

use crate::editor::decoration::DecorationModel;
use crate::editor::layout::{EditorLayout, scrolled_text_origin_x, visual_column_for_with_offset};
use crate::editor::render::{
    RowRenderPlan, WhitespaceKind, space_marker_bounds, space_marker_size, visible_marker_columns,
};
use crate::ui::icons::hero::{self, HeroIcon};

use super::line_cache::{LineGeometry, measured_caret_x};
use super::style::EditorStyle;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_row_markers<Renderer>(
    renderer: &mut Renderer,
    bounds: Rectangle,
    layout: EditorLayout,
    decorations: &DecorationModel,
    row: &RowRenderPlan,
    line_geometry: &LineGeometry<Renderer::Paragraph>,
    style: EditorStyle,
    clip_bounds: Rectangle,
) where
    Renderer: iced::advanced::Renderer
        + text::Renderer<Font = Font>
        + advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    let collapsed_eol_x = row.eol.and_then(|_| {
        row.collapsed_delimiter().and_then(|delimiter| {
            let anchor_x =
                measured_caret_x(line_geometry, delimiter.opening_column, layout, decorations);
            row.collapsed_indicator_bounds(layout.metrics, anchor_x)
                .map(|indicator| {
                    indicator.x + indicator.width + layout.metrics.character_width * 0.25
                })
        })
    });
    let context = MarkerRenderContext {
        bounds,
        layout,
        decorations,
        row,
        style,
        clip_bounds,
        collapsed_eol_x,
    };

    match line_geometry {
        LineGeometry::Fast { .. } => draw_fast_row_markers(renderer, context),
        _ => draw_measured_row_markers(renderer, context, line_geometry),
    }
}

#[derive(Debug, Clone, Copy)]
struct MarkerRenderContext<'a> {
    bounds: Rectangle,
    layout: EditorLayout,
    decorations: &'a DecorationModel,
    row: &'a RowRenderPlan,
    style: EditorStyle,
    clip_bounds: Rectangle,
    collapsed_eol_x: Option<f32>,
}

fn draw_fast_row_markers<Renderer>(renderer: &mut Renderer, context: MarkerRenderContext<'_>)
where
    Renderer: iced::advanced::Renderer + advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    let MarkerRenderContext {
        bounds,
        layout,
        decorations,
        row,
        clip_bounds,
        ..
    } = context;

    if row.whitespace.is_empty() && row.eol.is_none() {
        return;
    }

    let metrics = layout.metrics;
    let text_origin_x = bounds.x + scrolled_text_origin_x(layout, decorations);
    let Some((first_visible_column, last_visible_column)) =
        visible_marker_columns(text_origin_x, metrics.character_width, clip_bounds)
    else {
        return;
    };

    for whitespace in &row.whitespace {
        if row
            .collapsed_delimiter()
            .is_some_and(|delimiter| whitespace.column >= delimiter.opening_column)
        {
            continue;
        }
        let visual_column = visual_column_for_with_offset(
            &row.text,
            whitespace.column,
            decorations.settings.indent_width,
            row.start_visual_column,
        )
        .saturating_sub(row.start_visual_column);

        if visual_column < first_visible_column || visual_column > last_visible_column {
            continue;
        }

        if whitespace.kind == WhitespaceKind::Space {
            draw_space_marker(renderer, context, visual_column);
            continue;
        }

        draw_marker_icon(renderer, context, visual_column, HeroIcon::ChevronRight);
    }

    if row.eol.is_some() {
        if let Some(x) = context.collapsed_eol_x {
            draw_marker_icon_at_x(renderer, context, x, HeroIcon::ArrowTurnDownLeft);
            return;
        }
        let visual_column = visual_column_for_with_offset(
            &row.text,
            row.text.len(),
            decorations.settings.indent_width,
            row.start_visual_column,
        )
        .saturating_sub(row.start_visual_column);

        if visual_column >= first_visible_column && visual_column <= last_visible_column {
            draw_marker_icon(
                renderer,
                context,
                visual_column,
                HeroIcon::ArrowTurnDownLeft,
            );
        }
    }
}

fn draw_space_marker<Renderer>(
    renderer: &mut Renderer,
    context: MarkerRenderContext<'_>,
    visual_column: usize,
) where
    Renderer: iced::advanced::Renderer,
{
    renderer.fill_quad(
        renderer::Quad {
            bounds: space_marker_bounds(
                context.bounds,
                context.layout,
                context.decorations,
                context.row,
                visual_column,
            ),
            ..renderer::Quad::default()
        },
        Background::Color(context.style.whitespace_markers),
    );
}

fn draw_measured_row_markers<Renderer>(
    renderer: &mut Renderer,
    context: MarkerRenderContext<'_>,
    line_geometry: &LineGeometry<Renderer::Paragraph>,
) where
    Renderer:
        advanced_image::Renderer<Handle = advanced_image::Handle> + text::Renderer<Font = Font>,
{
    for whitespace in &context.row.whitespace {
        if context
            .row
            .collapsed_delimiter()
            .is_some_and(|delimiter| whitespace.column >= delimiter.opening_column)
        {
            continue;
        }
        let x = measured_caret_x(
            line_geometry,
            whitespace.column,
            context.layout,
            context.decorations,
        );
        match whitespace.kind {
            WhitespaceKind::Space => draw_space_marker_at_x(renderer, context, x),
            WhitespaceKind::Tab => {
                draw_marker_icon_at_x(renderer, context, x, HeroIcon::ChevronRight)
            }
        }
    }

    if context.row.eol.is_some() {
        let x = context.collapsed_eol_x.unwrap_or_else(|| {
            measured_caret_x(
                line_geometry,
                context.row.text.len(),
                context.layout,
                context.decorations,
            )
        });

        draw_marker_icon_at_x(renderer, context, x, HeroIcon::ArrowTurnDownLeft);
    }
}

fn draw_space_marker_at_x<Renderer>(
    renderer: &mut Renderer,
    context: MarkerRenderContext<'_>,
    x: f32,
) where
    Renderer: iced::advanced::Renderer,
{
    let metrics = context.layout.metrics;
    let dot_size = space_marker_size(metrics);
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: context.bounds.x + x + (metrics.character_width - dot_size) / 2.0,
                y: context.bounds.y + context.row.y + (metrics.line_height - dot_size) / 2.0,
                width: dot_size,
                height: dot_size,
            },
            ..renderer::Quad::default()
        },
        Background::Color(context.style.whitespace_markers),
    );
}

fn draw_marker_icon<Renderer>(
    renderer: &mut Renderer,
    context: MarkerRenderContext<'_>,
    visual_column: usize,
    icon: HeroIcon,
) where
    Renderer: advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    let x = scrolled_text_origin_x(context.layout, context.decorations)
        + visual_column as f32 * context.layout.metrics.character_width;

    draw_marker_icon_at_x(renderer, context, x, icon);
}

fn draw_marker_icon_at_x<Renderer>(
    renderer: &mut Renderer,
    context: MarkerRenderContext<'_>,
    x: f32,
    icon: HeroIcon,
) where
    Renderer: advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    let metrics = context.layout.metrics;
    let size = metrics
        .character_width
        .min(metrics.line_height * 0.72)
        .max(7.0);
    let bounds = Rectangle {
        x: context.bounds.x + x + (metrics.character_width - size) / 2.0,
        y: context.bounds.y + context.row.y + (metrics.line_height - size) / 2.0,
        width: size,
        height: size,
    };

    renderer.draw_image(
        advanced_image::Image::new(hero::handle_with_color(
            icon,
            context.style.whitespace_markers,
        ))
        .filter_method(advanced_image::FilterMethod::Linear),
        bounds,
        context.clip_bounds,
    );
}
