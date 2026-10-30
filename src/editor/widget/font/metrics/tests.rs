use super::super::{EDITOR_FONT, EDITOR_TEXT_SHAPING};
use iced::advanced::{graphics, text};

#[test]
fn unix_ascii_geometry_matches_rendered_font_across_zoom_and_tabs() {
    use crate::editor::layout::{EditorLayout, ScrollOffset, scrolled_text_origin_x};
    use crate::editor::render::SelectionRenderPlan;
    use crate::editor::widget::line_cache::{
        LineGeometry, measured_caret_x, measured_selection_x_and_width,
    };
    use crate::editor::widget::rich_text::expand_tabs_for_rendering_with_offset;
    use crate::editor::{DecorationModel, DecorationSettings};
    use iced::advanced::text::Paragraph as _;
    use iced::{Pixels, Size, alignment};

    let _font_guard = crate::font_system_test_guard();
    let decorations = DecorationModel::new(DecorationSettings::default());
    for zoom in [0.75, 1.0, 1.5] {
        let metrics = crate::ui::editor::metrics(zoom);
        let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, 1000.0, 200.0);
        let origin = scrolled_text_origin_x(layout, &decorations);
        for content in [
            "The quick brown fox jumps over the lazy dog.",
            "let\tvalue = 0123456789;",
        ] {
            let expanded = expand_tabs_for_rendering_with_offset(content, 4, 0);
            let display = expanded
                .as_ref()
                .map_or(content, |value| value.text.as_str());
            let paragraph = graphics::text::Paragraph::with_text(text::Text {
                content: display,
                bounds: Size::new(1000.0, metrics.line_height),
                size: Pixels(16.0 * zoom),
                line_height: text::LineHeight::Absolute(Pixels(metrics.line_height)),
                font: EDITOR_FONT,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: EDITOR_TEXT_SHAPING,
                wrapping: text::Wrapping::None,
                ellipsis: text::Ellipsis::None,
                hint_factor: None,
            });
            let geometry = LineGeometry::<()>::new_with_visual_offset(content, metrics, &(), 0);
            assert!(matches!(geometry, LineGeometry::Fast { .. }));
            let selection = SelectionRenderPlan {
                line: 0,
                start_column: 0,
                end_column: content.len(),
                start_visual_column: 0,
                end_visual_column: display.len(),
                start_virtual_column: None,
                end_virtual_column: None,
                y: 0.0,
                x: 0.0,
                width: 0.0,
            };
            let (start, width) =
                measured_selection_x_and_width(&selection, &geometry, layout, &decorations);
            assert!((start - origin).abs() < 0.01);
            assert!(
                (width - paragraph.min_width()).abs() < 0.01,
                "zoom={zoom}: {content}"
            );
            for byte in 0..=content.len() {
                let displayed_byte = expanded
                    .as_ref()
                    .map_or(byte, |value| value.byte_offsets[byte]);
                let expected = paragraph.grapheme_position(0, displayed_byte).unwrap().x;
                let actual = measured_caret_x(&geometry, byte, layout, &decorations) - origin;
                assert!(
                    (actual - expected).abs() < 0.01,
                    "zoom={zoom}, byte={byte}: {content}"
                );
                assert_eq!(
                    geometry.byte_column_for_x(expected + metrics.character_width * 0.25, 4),
                    byte,
                    "zoom={zoom}, byte={byte}: {content}"
                );
            }
        }
    }
}

#[test]
fn unix_multiline_selection_pixels_follow_rendered_text_width() {
    use crate::editor::widget::cache::RichParagraphCache;
    use crate::editor::widget::draw::draw_plan;
    use crate::editor::widget::line_cache::LineGeometryCache;
    use crate::editor::widget::rich_text::expand_tabs_for_rendering_with_offset;
    use crate::editor::widget::style::EditorStyle;
    use crate::editor::{
        DecorationModel, DecorationSettings, EditorBuffer, EditorLayout, EditorPosition,
        EditorSelection, FoldModel, ScrollOffset, SyntaxLineCache, ViewportModel,
        build_render_plan_for_selection_set_with_cache_and_caret_row,
    };
    use iced::advanced::renderer::{self, Headless, Renderer as _};
    use iced::advanced::text::Paragraph as _;
    use iced::{Color, Pixels, Rectangle, Size, Theme, alignment};

    let _font_guard = crate::font_system_test_guard();
    let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let source = "    if let Some(token) = source.next_token(cursor) {\n\treturn Some(RuleTerminator::Body { open: token.end });\n    } // end of expression";
    let buffer = EditorBuffer::from_text(source);
    let folds = FoldModel::default();
    let viewport = ViewportModel::new(buffer.line_count(), &folds);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            show_line_numbers: false,
            show_folding_controls: false,
            show_indentation_guides: false,
            ..DecorationSettings::default()
        },
        buffer.line_count(),
        &folds,
        vec![],
    );
    let size = Size::new(1200, 130);
    let bounds = Rectangle::with_size(Size::new(size.width as f32, size.height as f32));
    for zoom in [0.75, 1.0, 1.5] {
        let metrics = crate::ui::editor::metrics(zoom);
        for horizontal_px in [0.0, 41.25] {
            let layout = EditorLayout::new(
                metrics,
                ScrollOffset {
                    first_visible_row: 0,
                    horizontal_px,
                },
                bounds.width,
                bounds.height,
            );
            let selected = build_render_plan_for_selection_set_with_cache_and_caret_row(
                &buffer,
                &viewport,
                &decorations,
                EditorSelection::new(
                    EditorPosition::new(0, 0),
                    EditorPosition::new(2, buffer.line(2).unwrap().len()),
                )
                .into(),
                layout,
                &SyntaxLineCache::default(),
                None,
            );
            let mut unselected = selected.clone();
            unselected.selections.clear();
            let mut draw = |plan: &crate::editor::RenderPlan| {
                renderer.reset(bounds);
                draw_plan(
                    &mut renderer,
                    bounds,
                    layout,
                    &decorations,
                    plan,
                    EditorStyle::from_theme(&Theme::Light),
                    viewport.visible_row_count(),
                    None,
                    false,
                    false,
                    0.0,
                    1,
                    &mut RichParagraphCache::default(),
                    &mut LineGeometryCache::default(),
                    None,
                );
                renderer.screenshot(size, 1.0, Color::WHITE)
            };
            let before = draw(&unselected);
            let after = draw(&selected);
            for row in &selected.rows {
                let expanded = expand_tabs_for_rendering_with_offset(&row.text, 4, 0);
                let displayed = expanded
                    .as_ref()
                    .map_or(row.text.as_str(), |value| value.text.as_str());
                let paragraph = graphics::text::Paragraph::with_text(text::Text {
                    content: displayed,
                    bounds: Size::new(1200.0, metrics.line_height),
                    size: Pixels(16.0 * zoom),
                    line_height: text::LineHeight::Absolute(Pixels(metrics.line_height)),
                    font: EDITOR_FONT,
                    align_x: text::Alignment::Left,
                    align_y: alignment::Vertical::Top,
                    shaping: EDITOR_TEXT_SHAPING,
                    wrapping: text::Wrapping::None,
                    ellipsis: text::Ellipsis::None,
                    hint_factor: None,
                });
                let right = (row.y.ceil() as usize..(row.y + metrics.line_height).floor() as usize)
                    .flat_map(|y| (0..size.width as usize).map(move |x| (x, y)))
                    .filter_map(|(x, y)| {
                        let offset = (y * size.width as usize + x) * 4;
                        (before[offset..offset + 4] != after[offset..offset + 4]).then_some(x)
                    })
                    .max()
                    .expect("selection must paint pixels") as f32;
                let expected_right = row.text_x + paragraph.min_width();
                assert!(
                    (right + 1.0 - expected_right).abs() <= 1.0,
                    "Selection must end with the rendered row: zoom={zoom}, scroll={horizontal_px}, text={:?}, right={right}, expected={expected_right}",
                    row.text,
                );
            }
        }
    }
}
