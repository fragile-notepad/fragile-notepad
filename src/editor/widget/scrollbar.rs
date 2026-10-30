use iced::Rectangle;

use crate::editor::layout::EditorLayout;

const VERTICAL_SCROLLBAR_WIDTH: f32 = 12.0;
const VERTICAL_SCROLLBAR_MARGIN: f32 = 2.0;
const MIN_SCROLLBAR_THUMB_HEIGHT: f32 = 32.0;
pub(super) const HORIZONTAL_SCROLLBAR_HEIGHT: f32 = 14.0;
const HORIZONTAL_SCROLLBAR_MARGIN: f32 = 2.0;
const MIN_HORIZONTAL_SCROLLBAR_THUMB_WIDTH: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollbarStatus {
    Idle,
    Hovered,
    Dragged,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerticalScrollbarGeometry {
    pub track: Rectangle,
    pub thumb: Rectangle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HorizontalScrollbarGeometry {
    pub(super) track: Rectangle,
    pub(super) thumb: Rectangle,
}

/// Places the horizontal scrollbar below the editor's text area.
pub(super) fn horizontal_scrollbar_geometry(
    layout: EditorLayout,
    max_offset: f32,
    text_origin_x: f32,
) -> Option<HorizontalScrollbarGeometry> {
    let width = (layout.width - text_origin_x - HORIZONTAL_SCROLLBAR_HEIGHT).max(0.0);
    if max_offset <= 0.0 || width <= 0.0 {
        return None;
    }

    let track = Rectangle {
        x: text_origin_x,
        y: layout.height + HORIZONTAL_SCROLLBAR_MARGIN,
        width,
        height: HORIZONTAL_SCROLLBAR_HEIGHT - HORIZONTAL_SCROLLBAR_MARGIN * 2.0,
    };
    let thumb_width = (track.width * (track.width / (track.width + max_offset))).clamp(
        MIN_HORIZONTAL_SCROLLBAR_THUMB_WIDTH.min(track.width),
        track.width,
    );
    let travel = track.width - thumb_width;
    let progress = (layout.scroll.horizontal_px / max_offset).clamp(0.0, 1.0);
    let thumb = Rectangle {
        x: track.x + travel * progress,
        y: track.y + HORIZONTAL_SCROLLBAR_MARGIN,
        width: thumb_width,
        height: track.height - HORIZONTAL_SCROLLBAR_MARGIN * 2.0,
    };

    Some(HorizontalScrollbarGeometry { track, thumb })
}

pub(super) fn horizontal_offset_for_position(
    x: f32,
    grab_offset_x: f32,
    layout: EditorLayout,
    max_offset: f32,
    text_origin_x: f32,
) -> f32 {
    let Some(scrollbar) = horizontal_scrollbar_geometry(layout, max_offset, text_origin_x) else {
        return 0.0;
    };
    let travel = scrollbar.track.width - scrollbar.thumb.width;
    if travel <= 0.0 {
        return 0.0;
    }
    let left = (x - grab_offset_x - scrollbar.track.x).clamp(0.0, travel);

    left / travel * max_offset
}

pub fn vertical_scrollbar_geometry(
    layout: EditorLayout,
    total_visible_rows: usize,
) -> Option<VerticalScrollbarGeometry> {
    let visible_rows = layout.complete_visible_row_capacity().max(1);

    if total_visible_rows <= visible_rows || layout.height <= 0.0 {
        return None;
    }

    let track = Rectangle {
        x: (layout.width - VERTICAL_SCROLLBAR_WIDTH).max(0.0),
        y: VERTICAL_SCROLLBAR_MARGIN,
        width: VERTICAL_SCROLLBAR_WIDTH,
        height: (layout.height - VERTICAL_SCROLLBAR_MARGIN * 2.0).max(0.0),
    };

    if track.height <= 0.0 {
        return None;
    }

    let thumb_height = (track.height * visible_rows as f32 / total_visible_rows as f32)
        .clamp(MIN_SCROLLBAR_THUMB_HEIGHT.min(track.height), track.height);
    let max_first_row = total_visible_rows.saturating_sub(visible_rows);
    let travel = (track.height - thumb_height).max(0.0);
    let progress = if max_first_row == 0 {
        0.0
    } else {
        let first_row = layout.scroll.first_visible_row as f64
            + f64::from(layout.vertical_offset_px / layout.metrics.line_height);
        (first_row / max_first_row as f64).clamp(0.0, 1.0) as f32
    };
    let thumb = Rectangle {
        x: track.x + VERTICAL_SCROLLBAR_MARGIN,
        y: track.y + travel * progress,
        width: (track.width - VERTICAL_SCROLLBAR_MARGIN * 2.0).max(1.0),
        height: thumb_height,
    };

    Some(VerticalScrollbarGeometry { track, thumb })
}

pub fn scrollbar_row_for_position(
    y: f32,
    grab_offset_y: f32,
    layout: EditorLayout,
    total_visible_rows: usize,
) -> usize {
    let visible_rows = layout.complete_visible_row_capacity().max(1);
    let max_first_row = total_visible_rows.saturating_sub(visible_rows);

    if max_first_row == 0 {
        return 0;
    }

    let Some(scrollbar) = vertical_scrollbar_geometry(layout, total_visible_rows) else {
        return 0;
    };
    let travel = (scrollbar.track.height - scrollbar.thumb.height).max(1.0);
    let top = (y - grab_offset_y).clamp(scrollbar.track.y, scrollbar.track.y + travel)
        - scrollbar.track.y;

    ((top / travel) * max_first_row as f32).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::layout::{EditorMetrics, ScrollOffset, row_y};

    #[test]
    fn horizontal_scrollbar_follows_fractional_offsets_and_preserves_grab_position() {
        let mut layout =
            EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 300.0, 94.0);
        let text_origin_x = 86.0;
        let max_offset = 200.0;
        let start = horizontal_scrollbar_geometry(layout, max_offset, text_origin_x).unwrap();
        assert_eq!(start.track.x, text_origin_x);
        assert_eq!(start.track.width, 200.0);
        assert_eq!(start.track.y, layout.height + 2.0);
        assert_eq!(start.track.height, 10.0);
        assert_eq!(start.thumb.width, 100.0);

        layout.scroll.horizontal_px = 50.125;
        let scrolled = horizontal_scrollbar_geometry(layout, max_offset, text_origin_x).unwrap();
        assert_eq!(scrolled.thumb.x, start.thumb.x + 25.0625);
        let grab_offset_x = scrolled.thumb.width / 3.0;
        let offset = horizontal_offset_for_position(
            scrolled.thumb.x + grab_offset_x,
            grab_offset_x,
            layout,
            max_offset,
            text_origin_x,
        );
        assert!((offset - layout.scroll.horizontal_px).abs() < 0.0001);
    }

    #[test]
    fn horizontal_scrollbar_reaches_both_limits_and_clamps_pointer_positions() {
        let mut layout =
            EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 300.0, 94.0);
        let max_offset = 123.5;
        let text_origin_x = 86.0;
        for (offset, expected) in [(-10.0, 0.0), (max_offset + 10.0, max_offset)] {
            layout.scroll.horizontal_px = offset;
            let scrollbar =
                horizontal_scrollbar_geometry(layout, max_offset, text_origin_x).unwrap();
            let expected_x = if expected == 0.0 {
                scrollbar.track.x
            } else {
                scrollbar.track.x + scrollbar.track.width - scrollbar.thumb.width
            };
            assert!((scrollbar.thumb.x - expected_x).abs() < 0.0001);
        }
        assert_eq!(
            horizontal_offset_for_position(-100.0, 10.0, layout, max_offset, text_origin_x),
            0.0,
        );
        assert_eq!(
            horizontal_offset_for_position(1000.0, 10.0, layout, max_offset, text_origin_x),
            max_offset,
        );
    }

    #[test]
    fn horizontal_scrollbar_stays_inside_its_lane_in_narrow_editors() {
        let mut layout =
            EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 300.0, 94.0);
        let text_origin_x = 86.0;
        assert!(horizontal_scrollbar_geometry(layout, 0.0, text_origin_x).is_none());
        assert!(horizontal_scrollbar_geometry(layout, -1.0, text_origin_x).is_none());
        for width in [14.0, 100.0, 100.5, 110.0, 300.0] {
            layout.width = width;
            layout.scroll.horizontal_px = 1000.0;
            let Some(scrollbar) = horizontal_scrollbar_geometry(layout, 200.0, text_origin_x)
            else {
                assert!(width <= text_origin_x + HORIZONTAL_SCROLLBAR_HEIGHT);
                continue;
            };
            assert!(scrollbar.track.x >= text_origin_x);
            assert!(scrollbar.track.x + scrollbar.track.width <= width);
            assert!(scrollbar.thumb.x >= scrollbar.track.x);
            assert!(
                scrollbar.thumb.x + scrollbar.thumb.width
                    <= scrollbar.track.x + scrollbar.track.width
            );
            assert!(scrollbar.thumb.y >= scrollbar.track.y);
            assert!(
                scrollbar.thumb.y + scrollbar.thumb.height
                    <= scrollbar.track.y + scrollbar.track.height
            );
            assert!(
                scrollbar.track.y + scrollbar.track.height
                    <= layout.height + HORIZONTAL_SCROLLBAR_HEIGHT
            );
            assert!(
                horizontal_offset_for_position(500.0, 0.0, layout, 200.0, text_origin_x)
                    .is_finite()
            );
        }
    }

    #[test]
    fn scrollbar_thumb_follows_fractional_vertical_offset() {
        let mut layout =
            EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 300.0, 94.0);
        let start = vertical_scrollbar_geometry(layout, 100).unwrap();
        layout.scroll.first_visible_row = 1;
        let next_row = vertical_scrollbar_geometry(layout, 100).unwrap();
        layout.scroll.first_visible_row = 0;
        layout.vertical_offset_px = layout.metrics.line_height / 2.0;
        let halfway = vertical_scrollbar_geometry(layout, 100).unwrap();

        assert_eq!(halfway.thumb.height, start.thumb.height);
        assert!((halfway.thumb.y - (start.thumb.y + next_row.thumb.y) / 2.0).abs() < 0.001);
    }

    #[test]
    fn scrollbar_exposes_a_final_row_that_only_partially_fits() {
        let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 300.0, 49.0);
        assert_eq!(layout.visible_row_capacity(), 3);
        assert_eq!(layout.complete_visible_row_capacity(), 2);
        assert!(vertical_scrollbar_geometry(layout, 3).is_some());
    }

    #[test]
    fn scrollbar_bottom_fully_exposes_final_visual_row_with_partial_viewport_row() {
        let mut layout =
            EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 300.0, 49.0);
        for total_rows in [3, 10] {
            let scrollbar = vertical_scrollbar_geometry(layout, total_rows).expect("scrollbar");
            let bottom = scrollbar_row_for_position(
                scrollbar.track.y + scrollbar.track.height,
                0.0,
                layout,
                total_rows,
            );
            assert_eq!(bottom, total_rows - 2);
            layout.scroll.first_visible_row = bottom;
            assert!(row_y(total_rows - 1, layout) + layout.metrics.line_height <= layout.height);
            let scrollbar = vertical_scrollbar_geometry(layout, total_rows).expect("scrollbar");
            assert!(
                (scrollbar.thumb.y + scrollbar.thumb.height
                    - scrollbar.track.y
                    - scrollbar.track.height)
                    .abs()
                    < 0.001
            );
        }
    }
}
