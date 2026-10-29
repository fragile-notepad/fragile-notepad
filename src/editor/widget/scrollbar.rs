use iced::Rectangle;

use crate::editor::layout::EditorLayout;

const VERTICAL_SCROLLBAR_WIDTH: f32 = 12.0;
const VERTICAL_SCROLLBAR_MARGIN: f32 = 2.0;
const MIN_SCROLLBAR_THUMB_HEIGHT: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerticalScrollbarGeometry {
    pub track: Rectangle,
    pub thumb: Rectangle,
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
