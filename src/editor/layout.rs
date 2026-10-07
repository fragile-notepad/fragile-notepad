use super::buffer::EditorBuffer;
use super::decoration::DecorationModel;
use super::fold::FoldRange;
use super::position::EditorPosition;
use super::viewport::ViewportModel;
use iced::Rectangle;
use unicode_width::UnicodeWidthChar;

pub(super) const GUTTER_RIGHT_MARGIN: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorMetrics {
    pub line_height: f32,
    pub character_width: f32,
    pub padding_left: f32,
    pub padding_top: f32,
    pub line_number_width: f32,
    pub fold_lane_width: f32,
    pub hidden_indicator_width: f32,
}

impl EditorMetrics {
    pub const fn new(line_height: f32, character_width: f32) -> Self {
        Self {
            line_height,
            character_width,
            padding_left: 6.0,
            padding_top: 4.0,
            line_number_width: 48.0,
            fold_lane_width: 16.0,
            hidden_indicator_width: 12.0,
        }
    }

    /// Fits the largest logical line number while preserving the minimum width.
    pub fn with_line_count(mut self, line_count: usize) -> Self {
        let digits = line_count.max(1).ilog10() + 1;
        self.line_number_width = self
            .line_number_width
            .max(digits as f32 * self.character_width + GUTTER_RIGHT_MARGIN);
        self
    }

    pub fn gutter_width(self, decorations: &DecorationModel) -> f32 {
        let line_number_width = if decorations.settings.show_line_numbers {
            self.line_number_width
        } else {
            0.0
        };

        let fold_lane_width = if decorations.settings.show_folding_controls {
            self.fold_lane_width
        } else {
            0.0
        };

        line_number_width + fold_lane_width + self.hidden_indicator_width
    }

    pub fn text_origin_x(self, decorations: &DecorationModel) -> f32 {
        self.padding_left + self.gutter_width(decorations)
    }
}

impl Default for EditorMetrics {
    fn default() -> Self {
        Self::new(18.0, 8.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollOffset {
    pub first_visible_row: usize,
    pub horizontal_px: f32,
}

impl ScrollOffset {
    pub const ZERO: Self = Self {
        first_visible_row: 0,
        horizontal_px: 0.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorLayout {
    pub metrics: EditorMetrics,
    pub scroll: ScrollOffset,
    pub width: f32,
    pub height: f32,
}

impl EditorLayout {
    pub const fn new(
        metrics: EditorMetrics,
        scroll: ScrollOffset,
        width: f32,
        height: f32,
    ) -> Self {
        Self {
            metrics,
            scroll,
            width,
            height,
        }
    }

    pub fn visible_row_capacity(self) -> usize {
        if self.metrics.line_height <= 0.0 || self.height <= self.metrics.padding_top {
            return 0;
        }

        ((self.height - self.metrics.padding_top) / self.metrics.line_height).ceil() as usize
    }

    /// Rows that fit completely, used by navigation and scroll limits so the
    /// final row can always be brought fully into view.
    pub fn complete_visible_row_capacity(self) -> usize {
        if self.metrics.line_height <= 0.0 || self.height <= self.metrics.padding_top {
            return 0;
        }

        ((self.height - self.metrics.padding_top) / self.metrics.line_height).floor() as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitTarget {
    Text(EditorPosition),
    GutterLine { line: usize },
    FoldControl { line: usize, range: FoldRange },
    HiddenLineIndicator { line: usize },
    Outside,
}

pub fn hit_test(
    x: f32,
    y: f32,
    layout: EditorLayout,
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
) -> HitTarget {
    if x < 0.0 || y < 0.0 || x > layout.width || y > layout.height {
        return HitTarget::Outside;
    }

    let Some((visible_row, line)) = hit_visible_row(y, layout, viewport) else {
        return HitTarget::Outside;
    };

    let metrics = layout.metrics;
    let text_origin_x = metrics.text_origin_x(decorations);
    let fold_start_x = metrics.padding_left
        + if decorations.settings.show_line_numbers {
            metrics.line_number_width
        } else {
            0.0
        };
    let fold_end_x = fold_start_x
        + if decorations.settings.show_folding_controls {
            metrics.fold_lane_width
        } else {
            0.0
        };
    let hidden_indicator_end_x = text_origin_x;

    let segment = viewport.row_segment(visible_row, buffer);
    let is_first_row = segment.is_none_or(|segment| segment.start_column == 0);

    if is_first_row
        && decorations.settings.show_folding_controls
        && x >= fold_start_x
        && x < fold_end_x
        && let Some(decoration) = decorations
            .line_decorations
            .iter()
            .find(|decoration| decoration.line == line && decoration.has_fold_control)
        && let Some(range) = decoration.fold_range
    {
        return HitTarget::FoldControl { line, range };
    }

    if is_first_row
        && x >= fold_end_x
        && x < hidden_indicator_end_x
        && decorations
            .hidden_line_spans
            .iter()
            .any(|span| span.header_line == line)
    {
        return HitTarget::HiddenLineIndicator { line };
    }

    if x < text_origin_x {
        return HitTarget::GutterLine { line };
    }

    let horizontal_px = if viewport.wrap_columns().is_some() {
        0.0
    } else {
        layout.scroll.horizontal_px
    };
    let start_visual_column = segment.map_or(0, |segment| segment.start_visual_column);
    let visual_column = ((x - text_origin_x + horizontal_px) / metrics.character_width)
        .floor()
        .max(0.0) as usize
        + start_visual_column;
    let text = viewport.display_text(line, buffer);
    let column = byte_column_for(&text, visual_column, decorations.settings.indent_width)
        .min(segment.map_or(usize::MAX, |segment| segment.end_column));
    let position =
        buffer.clamp_position(viewport.source_position(EditorPosition::new(line, column)));

    HitTarget::Text(position)
}

pub fn hit_visible_row(
    y: f32,
    layout: EditorLayout,
    viewport: &ViewportModel,
) -> Option<(usize, usize)> {
    if y < layout.metrics.padding_top || layout.metrics.line_height <= 0.0 {
        return None;
    }

    let visible_row = layout.scroll.first_visible_row
        + ((y - layout.metrics.padding_top) / layout.metrics.line_height) as usize;
    let line = viewport.visible_row_to_document_line(visible_row)?;

    Some((visible_row, line))
}

pub fn row_y(visible_row: usize, layout: EditorLayout) -> f32 {
    layout.metrics.padding_top
        + visible_row.saturating_sub(layout.scroll.first_visible_row) as f32
            * layout.metrics.line_height
}

pub fn text_area_bounds(
    bounds: Rectangle,
    layout: EditorLayout,
    decorations: &DecorationModel,
) -> Rectangle {
    let text_origin_x = layout.metrics.text_origin_x(decorations);

    Rectangle {
        x: bounds.x + text_origin_x,
        y: bounds.y,
        width: (bounds.width - text_origin_x).max(0.0),
        height: bounds.height,
    }
}

pub fn scrolled_text_origin_x(layout: EditorLayout, decorations: &DecorationModel) -> f32 {
    layout.metrics.text_origin_x(decorations) - layout.scroll.horizontal_px
}

pub fn caret_x(
    line_text: &str,
    column: usize,
    layout: EditorLayout,
    decorations: &DecorationModel,
) -> f32 {
    scrolled_text_origin_x(layout, decorations)
        + visual_column_for(line_text, column, decorations.settings.indent_width) as f32
            * layout.metrics.character_width
}

pub fn x_for_visual_column(
    visual_column: usize,
    layout: EditorLayout,
    decorations: &DecorationModel,
) -> f32 {
    scrolled_text_origin_x(layout, decorations)
        + visual_column as f32 * layout.metrics.character_width
}

pub fn visual_column_for_byte_column(text: &str, byte_column: usize) -> usize {
    visual_column_for(text, byte_column, 4)
}

pub fn visual_column_for(text: &str, byte_column: usize, tab_width: usize) -> usize {
    visual_column_for_with_offset(text, byte_column, tab_width, 0)
}

/// Returns a logical visual column for a fragment beginning at an existing tab stop offset.
pub fn visual_column_for_with_offset(
    text: &str,
    byte_column: usize,
    tab_width: usize,
    start_visual_column: usize,
) -> usize {
    let mut visual_column = start_visual_column;

    for (offset, ch) in text.char_indices() {
        if offset >= byte_column {
            break;
        }

        visual_column += visual_width_with_tab_width(ch, visual_column, tab_width);
    }

    visual_column
}

pub fn byte_column_for_visual_column(text: &str, target_visual_column: usize) -> usize {
    byte_column_for(text, target_visual_column, 4)
}

pub fn byte_column_for(text: &str, target_visual_column: usize, tab_width: usize) -> usize {
    byte_column_for_with_offset(text, target_visual_column, tab_width, 0)
}

/// Returns a fragment byte column from an absolute logical visual column.
pub fn byte_column_for_with_offset(
    text: &str,
    target_visual_column: usize,
    tab_width: usize,
    start_visual_column: usize,
) -> usize {
    let mut visual_column = start_visual_column;

    for (offset, ch) in text.char_indices() {
        let next_visual_column =
            visual_column + visual_width_with_tab_width(ch, visual_column, tab_width);

        if target_visual_column < next_visual_column {
            return offset;
        }

        visual_column = next_visual_column;
    }

    text.len()
}

pub fn visual_width(ch: char, visual_column: usize) -> usize {
    visual_width_with_tab_width(ch, visual_column, 4)
}

pub fn visual_width_with_tab_width(ch: char, visual_column: usize, tab_width: usize) -> usize {
    let tab_width = tab_width.max(1);

    match ch {
        '\t' => tab_width - visual_column % tab_width,
        _ => UnicodeWidthChar::width(ch).unwrap_or(0),
    }
}
