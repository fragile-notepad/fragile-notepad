use iced::advanced::{image as advanced_image, renderer, text};
use iced::{Background, Color, Font, Pixels, Point, Rectangle, Size, alignment};

use crate::editor::cjk::CjkContext;
use crate::editor::decoration::DecorationModel;
use crate::editor::layout::{
    EditorLayout, EditorMetrics, scrolled_text_origin_x, text_area_bounds,
};
use crate::editor::render::{
    RenderPlan, line_number_left_x, text_baseline_offset, visible_marker_columns,
};
use crate::ui::icons::hero::{self, HeroIcon};

use super::cache::RichParagraphCache;
use super::font::{EDITOR_FONT, EDITOR_TEXT_SHAPING, editor_font_runs_for_fragment};
use super::line_cache::{
    LineGeometryCache, RowGeometries, measured_caret_x, measured_selection_x_and_width,
    measured_virtual_caret_x,
};
use super::markers::draw_row_markers;
use super::rich_text::{can_batch_fast_text, draw_row_text};
use super::scrollbar::vertical_scrollbar_geometry;
use super::style::EditorStyle;

const LONG_LINE_VISIBLE_MARGIN_COLUMNS: usize = 8;

#[cfg(test)]
mod indent_tests {
    use super::*;
    use crate::editor::{CaretRenderPlan, EditorPosition, IndentGuideRenderPlan, RowRenderPlan};

    #[test]
    fn active_layer_follows_caret_and_stops_at_separate_blocks() {
        let rows = [vec![1, 2], vec![1, 2], vec![1], vec![1, 2]]
            .into_iter()
            .enumerate()
            .map(|(line, depths)| RowRenderPlan {
                visible_row: line,
                line,
                start_column: 0,
                start_visual_column: 0,
                y: line as f32 * 20.0,
                text_x: 0.0,
                text: String::new(),
                line_number: None,
                is_active_line: line == 1,
                fold: None,
                hidden_lines: None,
                whitespace: vec![],
                eol: None,
                indent_guides: depths
                    .into_iter()
                    .map(|depth| IndentGuideRenderPlan {
                        line,
                        depth,
                        x: depth as f32 * 40.0,
                    })
                    .collect(),
                syntax_spans: vec![],
            })
            .collect();
        let mut plan = RenderPlan {
            rows,
            selections: vec![],
            carets: vec![],
            caret: Some(CaretRenderPlan {
                position: EditorPosition::new(1, 12),
                visual_column: None,
                x: 120.0,
                y: 20.0,
                height: 20.0,
            }),
        };
        assert_eq!(active_indent_guide(&plan), Some((2, 0..2)));
        plan.caret.as_mut().unwrap().x = 40.0;
        assert_eq!(active_indent_guide(&plan), Some((1, 0..4)));
        plan.caret.as_mut().unwrap().x = 0.0;
        assert_eq!(active_indent_guide(&plan), None);
        plan.caret = None;
        assert_eq!(active_indent_guide(&plan), None);
    }
}

// Highlight only the connected guide containing the primary caret, rather than
// unrelated blocks which happen to have the same indentation depth.
fn active_indent_guide(plan: &RenderPlan) -> Option<(usize, std::ops::Range<usize>)> {
    let caret = plan.caret?;
    let index = plan
        .rows
        .iter()
        .position(|row| row.line == caret.position.line && !row.indent_guides.is_empty())?;
    let depth = plan.rows[index]
        .indent_guides
        .iter()
        .filter(|guide| guide.x <= caret.x)
        .map(|guide| guide.depth)
        .max()?;
    let contains_depth = |index: usize| {
        plan.rows[index]
            .indent_guides
            .iter()
            .any(|guide| guide.depth == depth)
    };
    let mut start = index;
    let mut end = index + 1;
    while start > 0 && contains_depth(start - 1) {
        start -= 1;
    }
    while end < plan.rows.len() && contains_depth(end) {
        end += 1;
    }
    Some((depth, start..end))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_plan<Renderer>(
    renderer: &mut Renderer,
    bounds: Rectangle,
    layout: EditorLayout,
    decorations: &DecorationModel,
    plan: &RenderPlan,
    style: EditorStyle,
    total_visible_rows: usize,
    wrap_guide_column: Option<usize>,
    fast_text: bool,
    caret_visible: bool,
    fold_controls_hovered: bool,
    frame_id: u64,
    rich_paragraphs: &mut RichParagraphCache<Renderer::Paragraph>,
    line_geometries: &mut LineGeometryCache<Renderer::Paragraph>,
    cjk_context: Option<&CjkContext>,
) where
    Renderer: iced::advanced::Renderer
        + text::Renderer<Font = Font>
        + advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    let metrics = layout.metrics;
    let active_guide = active_indent_guide(plan);
    let gutter_width = metrics.gutter_width(decorations);
    let text_clip_bounds = text_area_bounds(bounds, layout, decorations);
    let scroll_text_clip_bounds =
        scroll_text_area_bounds(bounds, layout, decorations, total_visible_rows);
    let batch_text = fast_text
        && plan
            .rows
            .iter()
            .all(|row| row.syntax_spans.is_empty() && can_batch_fast_text(&row.text));
    let gutter_bounds = Rectangle {
        x: bounds.x,
        y: bounds.y,
        width: gutter_width + metrics.padding_left,
        height: bounds.height,
    };

    renderer.fill_quad(
        renderer::Quad {
            bounds: gutter_bounds,
            ..renderer::Quad::default()
        },
        Background::Color(style.gutter),
    );

    for row in &plan.rows {
        let row_y = bounds.y + row.y;

        if row.is_active_line {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + gutter_bounds.width,
                        y: row_y,
                        width: (bounds.width - gutter_bounds.width).max(0.0),
                        height: metrics.line_height,
                    },
                    ..renderer::Quad::default()
                },
                Background::Color(style.active_line),
            );
        }
    }

    // Draw over row backgrounds so the guide stays visible on the active line.
    // Its column remains independent of wrapping and scrolls with the text.
    if decorations.settings.show_wrap_guide
        && let Some(columns) = wrap_guide_column
    {
        let x = bounds.x
            + scrolled_text_origin_x(layout, decorations)
            + columns as f32 * metrics.character_width;
        renderer.with_layer(scroll_text_clip_bounds, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x,
                        y: bounds.y,
                        width: 1.0,
                        height: bounds.height,
                    },
                    ..renderer::Quad::default()
                },
                Background::Color(style.line_numbers.scale_alpha(0.20)),
            );
        });
    }

    let row_geometries = RowGeometries::new(
        &plan.rows,
        layout.metrics,
        line_geometries,
        renderer,
        cjk_context,
        decorations.settings.indent_width,
    );

    renderer.with_layer(text_clip_bounds, |renderer| {
        for selection in &plan.selections {
            let (x, width) = plan
                .rows
                .iter()
                .enumerate()
                .find(|(_, row)| row.line == selection.line && row.y == selection.y)
                .map(|(index, row)| {
                    let local = crate::editor::render::SelectionRenderPlan {
                        start_column: selection.start_column.saturating_sub(row.start_column),
                        end_column: selection.end_column.saturating_sub(row.start_column),
                        ..*selection
                    };
                    measured_selection_x_and_width(
                        &local,
                        row_geometries.get_by_row_index(index),
                        layout,
                        decorations,
                    )
                })
                .unwrap_or((selection.x, selection.width));

            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + x,
                        y: bounds.y + selection.y + 1.0,
                        width: width.max(1.0),
                        height: metrics.line_height - 2.0,
                    },
                    ..renderer::Quad::default()
                },
                Background::Color(style.selection),
            );
        }
    });

    renderer.with_layer(gutter_bounds, |renderer| {
        for row in &plan.rows {
            let row_y = bounds.y + row.y;

            if let Some(line_number) = row.line_number {
                draw_line_number(renderer, line_number, bounds, row_y, metrics, style);
            }
            if row.start_column > 0 && decorations.settings.show_wrap_indicator {
                let mut size = (metrics.character_width * 1.5)
                    .clamp(12.0, 18.0)
                    .min(metrics.line_height * 0.75);
                if !decorations.settings.show_line_numbers {
                    size = size.min(metrics.hidden_indicator_width);
                }
                let x = if decorations.settings.show_line_numbers {
                    bounds.x + line_number_left_x(metrics, size)
                } else {
                    bounds.x + gutter_bounds.width - metrics.hidden_indicator_width
                        + (metrics.hidden_indicator_width - size) / 2.0
                };
                draw_icon(
                    renderer,
                    HeroIcon::ArrowTurnDownRight,
                    Rectangle {
                        x,
                        y: row_y + (metrics.line_height - size) / 2.0,
                        width: size,
                        height: size,
                    },
                    gutter_bounds,
                    style.line_numbers,
                );
            }
        }
    });

    for row in &plan.rows {
        let row_y = bounds.y + row.y;

        if let Some(fold) = row.fold
            && (fold.collapsed || fold_controls_hovered)
        {
            draw_fold_control(
                renderer,
                bounds,
                row_y,
                fold.collapsed,
                metrics,
                decorations,
                style,
            );
        }

        if row.hidden_lines.is_some() && row.fold.is_none_or(|fold| !fold.collapsed) {
            draw_hidden_line_hint(renderer, bounds, row_y, metrics, decorations, style);
        }
    }

    renderer.with_layer(scroll_text_clip_bounds, |renderer| {
        if batch_text {
            draw_batched_row_text(
                renderer,
                bounds,
                layout,
                decorations,
                plan,
                style,
                scroll_text_clip_bounds,
            );
        } else {
            for row in &plan.rows {
                draw_row_text(
                    renderer,
                    row,
                    bounds.x + row.text_x,
                    bounds.y + row.y + text_baseline_offset(metrics),
                    metrics,
                    decorations,
                    style,
                    scroll_text_clip_bounds,
                    frame_id,
                    rich_paragraphs,
                    &editor_font_runs_for_fragment(
                        &row.text,
                        cjk_context,
                        row.line,
                        row.start_column,
                    ),
                    |renderer, content, position, bounds, color, align_x, metrics, clip_bounds| {
                        draw_text(
                            renderer,
                            content,
                            position,
                            bounds,
                            color,
                            align_x,
                            metrics,
                            clip_bounds,
                        );
                    },
                );
            }
        }
    });

    renderer.with_layer(text_clip_bounds, |renderer| {
        for (row_index, row) in plan.rows.iter().enumerate() {
            let row_y = bounds.y + row.y;

            for guide in &row.indent_guides {
                let x = bounds.x + guide.x;
                let mut color =
                    style.indent_guides[guide.depth.saturating_sub(1) % style.indent_guides.len()];
                if active_guide
                    .as_ref()
                    .is_some_and(|(depth, rows)| guide.depth == *depth && rows.contains(&row_index))
                {
                    color.a = 0.72;
                }
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x,
                            y: row_y,
                            width: 1.0,
                            height: metrics.line_height,
                        },
                        ..renderer::Quad::default()
                    },
                    Background::Color(color),
                );
            }

            let row_geometry = row_geometries.get_by_row_index(row_index);

            draw_row_markers(
                renderer,
                bounds,
                layout,
                decorations,
                row,
                row_geometry,
                style,
                text_clip_bounds,
            );
        }
    });

    renderer.with_layer(scroll_text_clip_bounds, |renderer| {
        for (row_index, row) in plan.rows.iter().enumerate() {
            if row.hidden_lines.is_none() {
                continue;
            }

            let text_end_x = measured_caret_x(
                row_geometries.get_by_row_index(row_index),
                row.text.len(),
                layout,
                decorations,
            );

            if let Some(indicator) = row.collapsed_indicator_bounds(metrics, text_end_x) {
                let indicator = Rectangle {
                    x: bounds.x + indicator.x,
                    y: bounds.y + indicator.y,
                    ..indicator
                };

                if indicator.intersects(&scroll_text_clip_bounds) {
                    draw_collapsed_fold_indicator(renderer, indicator, metrics, style);
                }
            }
        }
    });

    renderer.with_layer(text_clip_bounds, |renderer| {
        let fallback_caret = plan.caret.iter();
        let carets: Box<dyn Iterator<Item = _>> = if plan.carets.is_empty() {
            Box::new(fallback_caret)
        } else {
            Box::new(plan.carets.iter())
        };

        for caret in carets {
            let Some((row_index, row)) = plan
                .rows
                .iter()
                .enumerate()
                .find(|(_, row)| row.line == caret.position.line && row.y == caret.y)
            else {
                continue;
            };
            let row_geometry = row_geometries.get_by_row_index(row_index);
            let x = measured_virtual_caret_x(
                row_geometry,
                caret.position.column.saturating_sub(row.start_column),
                caret.visual_column,
                layout,
                decorations,
            );
            let caret_bounds = Rectangle {
                x: bounds.x + x,
                y: bounds.y + caret.y + 1.0,
                width: 1.5,
                height: caret.height - 2.0,
            };

            renderer.fill_quad(
                renderer::Quad {
                    bounds: caret_bounds,
                    ..renderer::Quad::default()
                },
                Background::Color(if caret_visible {
                    style.caret
                } else {
                    Color::TRANSPARENT
                }),
            );
        }
    });
}

pub(super) fn draw_vertical_scrollbar<Renderer>(
    renderer: &mut Renderer,
    layout: EditorLayout,
    total_visible_rows: usize,
    bounds: Rectangle,
    style: EditorStyle,
) where
    Renderer: iced::advanced::Renderer,
{
    let Some(scrollbar) = vertical_scrollbar_geometry(layout, total_visible_rows) else {
        return;
    };
    let track = Rectangle {
        x: bounds.x + scrollbar.track.x,
        y: bounds.y + scrollbar.track.y,
        width: scrollbar.track.width,
        height: scrollbar.track.height,
    };
    let thumb = Rectangle {
        x: bounds.x + scrollbar.thumb.x,
        y: bounds.y + scrollbar.thumb.y,
        width: scrollbar.thumb.width,
        height: scrollbar.thumb.height,
    };

    renderer.fill_quad(
        renderer::Quad {
            bounds: track,
            border: iced::Border {
                color: style.line_numbers.scale_alpha(0.14),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..renderer::Quad::default()
        },
        Background::Color(style.gutter.scale_alpha(0.78)),
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: thumb,
            border: iced::Border {
                color: style.line_numbers.scale_alpha(0.28),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..renderer::Quad::default()
        },
        Background::Color(style.line_numbers.scale_alpha(0.42)),
    );
}

fn scroll_text_area_bounds(
    bounds: Rectangle,
    layout: EditorLayout,
    decorations: &DecorationModel,
    total_visible_rows: usize,
) -> Rectangle {
    let mut text_bounds = text_area_bounds(bounds, layout, decorations);

    if let Some(scrollbar) = vertical_scrollbar_geometry(layout, total_visible_rows) {
        let scrollbar_left = bounds.x + scrollbar.track.x;
        text_bounds.width = (scrollbar_left - text_bounds.x).max(0.0);
    }

    text_bounds
}

fn draw_line_number<Renderer>(
    renderer: &mut Renderer,
    line_number: usize,
    bounds: Rectangle,
    row_y: f32,
    metrics: EditorMetrics,
    style: EditorStyle,
) where
    Renderer: text::Renderer<Font = Font>,
{
    let content = line_number.to_string();
    let text_width = (content.len() as f32 * metrics.character_width).max(metrics.character_width);
    let x = bounds.x + line_number_left_x(metrics, text_width);

    draw_text(
        renderer,
        content,
        Point::new(x, row_y + text_baseline_offset(metrics)),
        Size::new(text_width, metrics.line_height),
        style.line_numbers,
        text::Alignment::Left,
        metrics,
        Rectangle::INFINITE,
    );
}

fn draw_batched_row_text<Renderer>(
    renderer: &mut Renderer,
    bounds: Rectangle,
    layout: EditorLayout,
    decorations: &DecorationModel,
    plan: &RenderPlan,
    style: EditorStyle,
    clip_bounds: Rectangle,
) where
    Renderer: text::Renderer<Font = Font>,
{
    let Some(first_row) = plan.rows.first() else {
        return;
    };
    let metrics = layout.metrics;
    let text_origin_x = scrolled_text_origin_x(layout, decorations);
    let Some((first_visible_column, last_visible_column)) = visible_marker_columns(
        bounds.x + text_origin_x,
        metrics.character_width,
        clip_bounds,
    ) else {
        return;
    };
    let visible_start = first_visible_column.saturating_sub(LONG_LINE_VISIBLE_MARGIN_COLUMNS);
    let visible_end = last_visible_column.saturating_add(LONG_LINE_VISIBLE_MARGIN_COLUMNS);

    let mut content = String::new();
    let mut max_width = metrics.character_width;
    for (index, row) in plan.rows.iter().enumerate() {
        if index > 0 {
            content.push('\n');
        }

        let start = visible_start.min(row.text.len());
        let end = visible_end.min(row.text.len());
        content.push_str(&row.text[start..end]);

        let width = (end - start) as f32 * metrics.character_width;
        max_width = max_width.max(width);
    }

    draw_text(
        renderer,
        content,
        Point::new(
            bounds.x + text_origin_x + visible_start as f32 * metrics.character_width,
            bounds.y + first_row.y + text_baseline_offset(metrics),
        ),
        Size::new(max_width, plan.rows.len() as f32 * metrics.line_height),
        style.syntax_fallback_text,
        text::Alignment::Left,
        metrics,
        clip_bounds,
    );
}

fn draw_fold_control<Renderer>(
    renderer: &mut Renderer,
    bounds: Rectangle,
    row_y: f32,
    collapsed: bool,
    metrics: EditorMetrics,
    decorations: &DecorationModel,
    style: EditorStyle,
) where
    Renderer: iced::advanced::Renderer
        + text::Renderer<Font = Font>
        + advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    let icon_size = metrics.fold_lane_width.min(metrics.line_height * 0.8);
    let x = bounds.x + metrics.text_origin_x(decorations)
        - metrics.hidden_indicator_width
        - metrics.fold_lane_width
        + (metrics.fold_lane_width - icon_size) / 2.0;
    let y = row_y + (metrics.line_height - icon_size) / 2.0;

    draw_icon(
        renderer,
        if collapsed {
            HeroIcon::ChevronRight
        } else {
            HeroIcon::ChevronDown
        },
        Rectangle {
            x,
            y,
            width: icon_size,
            height: icon_size,
        },
        Rectangle::INFINITE,
        style.fold_controls,
    );
}

fn draw_collapsed_fold_indicator<Renderer>(
    renderer: &mut Renderer,
    bounds: Rectangle,
    metrics: EditorMetrics,
    style: EditorStyle,
) where
    Renderer: iced::advanced::Renderer,
{
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: iced::Border {
                radius: 1.0.into(),
                ..iced::Border::default()
            },
            ..renderer::Quad::default()
        },
        Background::Color(style.fold_control_background),
    );

    // Keep the three dots on the same inexpensive solid-quad path as visible
    // spaces, independent of the active font's ellipsis glyph or text batching.
    let dot_size = (metrics.character_width * 0.24).clamp(1.5, 3.0);
    let dot_step = dot_size * 2.0;
    let center_x = bounds.center_x();
    let center_y = bounds.center_y();

    for dot in -1..=1 {
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: center_x + dot as f32 * dot_step - dot_size / 2.0,
                    y: center_y - dot_size / 2.0,
                    width: dot_size,
                    height: dot_size,
                },
                border: iced::Border {
                    radius: (dot_size / 2.0).into(),
                    ..iced::Border::default()
                },
                ..renderer::Quad::default()
            },
            Background::Color(style.fold_controls),
        );
    }
}

fn draw_icon<Renderer>(
    renderer: &mut Renderer,
    icon: HeroIcon,
    bounds: Rectangle,
    clip_bounds: Rectangle,
    color: Color,
) where
    Renderer: advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    renderer.draw_image(
        advanced_image::Image::new(hero::handle_with_color(icon, color))
            .filter_method(advanced_image::FilterMethod::Linear),
        bounds,
        clip_bounds,
    );
}

fn draw_hidden_line_hint<Renderer>(
    renderer: &mut Renderer,
    bounds: Rectangle,
    row_y: f32,
    metrics: EditorMetrics,
    decorations: &DecorationModel,
    style: EditorStyle,
) where
    Renderer: iced::advanced::Renderer,
{
    let indicator_x =
        bounds.x + metrics.text_origin_x(decorations) - metrics.hidden_indicator_width;
    let indicator_y = row_y + (metrics.line_height - 8.0) / 2.0;

    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: indicator_x + 2.0,
                y: indicator_y,
                width: 6.0,
                height: 8.0,
            },
            border: iced::Border {
                color: style.hidden_line_indicators.scale_alpha(0.45),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..renderer::Quad::default()
        },
        Background::Color(style.hidden_line_indicators.scale_alpha(0.08)),
    );
}

fn draw_text<Renderer>(
    renderer: &mut Renderer,
    content: impl Into<String>,
    position: Point,
    bounds: Size,
    color: Color,
    align_x: text::Alignment,
    metrics: EditorMetrics,
    clip_bounds: Rectangle,
) where
    Renderer: text::Renderer<Font = Font>,
{
    renderer.fill_text(
        text::Text {
            content: content.into(),
            bounds,
            size: Pixels((metrics.line_height / 1.25).max(8.0)),
            line_height: text::LineHeight::Absolute(Pixels(metrics.line_height)),
            font: EDITOR_FONT,
            align_x,
            align_y: alignment::Vertical::Top,
            shaping: EDITOR_TEXT_SHAPING,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: renderer.scale_factor(),
        },
        position,
        color,
        clip_bounds,
    );
}
