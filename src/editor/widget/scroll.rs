use iced::time::{Duration, Instant};

use crate::editor::layout::{EditorLayout, ScrollOffset};

pub(super) const SCROLL_DURATION: Duration = Duration::from_millis(140);

/// Keeps wheel motion in the widget; the document stores the destination offsets.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScrollAnimation {
    layout: EditorLayout,
    document_scroll: ScrollOffset,
    viewport_key: u64,
    row_count: usize,
    wrap_columns: Option<usize>,
    viewport_height: f32,
    from_rows: f64,
    target_rows: f64,
    from_horizontal_px: f64,
    started: Instant,
}

impl ScrollAnimation {
    pub(super) fn new(
        layout: EditorLayout,
        viewport_key: u64,
        row_count: usize,
        wrap_columns: Option<usize>,
        viewport_height: f32,
        now: Instant,
    ) -> Self {
        Self {
            layout,
            document_scroll: layout.scroll,
            viewport_key,
            row_count,
            wrap_columns,
            viewport_height,
            from_rows: layout.scroll.first_visible_row as f64,
            target_rows: layout.scroll.first_visible_row as f64,
            from_horizontal_px: f64::from(layout.scroll.horizontal_px),
            started: now,
        }
    }

    pub(super) fn viewport_height(self) -> f32 {
        self.viewport_height
    }

    pub(super) fn text_height(self) -> f32 {
        self.layout.height
    }

    pub(super) fn set_text_height(&mut self, height: f32) {
        self.layout.height = height;
    }

    pub(super) fn matches(
        self,
        layout: EditorLayout,
        viewport_key: u64,
        row_count: usize,
        wrap_columns: Option<usize>,
    ) -> bool {
        let mut expected = self.layout;
        expected.scroll = layout.scroll;
        expected == layout
            && self.accepts_document_scroll(layout.scroll)
            && self.viewport_key == viewport_key
            && self.row_count == row_count
            && self.wrap_columns == wrap_columns
    }

    /// Iced can deliver several events before the application applies their
    /// scroll messages. Keep the last document offset until the target arrives.
    pub(super) fn observe_document_scroll(&mut self, scroll: ScrollOffset) -> bool {
        if !self.accepts_document_scroll(scroll) {
            return false;
        }
        self.document_scroll = scroll;
        true
    }

    fn accepts_document_scroll(self, scroll: ScrollOffset) -> bool {
        (scroll.first_visible_row == self.document_scroll.first_visible_row
            || scroll.first_visible_row == self.layout.scroll.first_visible_row)
            && (scroll.horizontal_px == self.document_scroll.horizontal_px
                || scroll.horizontal_px == self.layout.scroll.horizontal_px)
    }

    pub(super) fn scroll_by(
        &mut self,
        lines: f32,
        horizontal_px: f32,
        max_horizontal_px: f32,
        now: Instant,
    ) {
        let max_row = self
            .row_count
            .saturating_sub(self.layout.complete_visible_row_capacity().max(1));
        let (current, current_horizontal) = self.position(now);
        let reversing =
            self.animating(now) && (self.target_rows - current) * f64::from(lines) < 0.0;
        let previous_target = if reversing { current } else { self.target_rows };
        let target = if lines == 0.0 {
            self.target_rows
        } else {
            (previous_target + f64::from(lines)).clamp(0.0, max_row as f64)
        };
        let horizontal_target = f64::from(self.layout.scroll.horizontal_px);
        let reversing_horizontal = self.animating(now)
            && (horizontal_target - current_horizontal) * f64::from(horizontal_px) < 0.0;
        let previous_horizontal = if reversing_horizontal {
            current_horizontal
        } else {
            horizontal_target
        };
        let target_horizontal = if horizontal_px == 0.0 {
            horizontal_target
        } else {
            (previous_horizontal + f64::from(horizontal_px))
                .clamp(0.0, f64::from(max_horizontal_px))
        };
        let target_horizontal = horizontal_offset_milli(target_horizontal as f32) as f32 / 1000.0;
        if target == self.target_rows && target_horizontal == self.layout.scroll.horizontal_px {
            return;
        }
        self.from_rows = current;
        self.target_rows = target;
        self.from_horizontal_px = current_horizontal;
        self.started = now;
        self.layout.scroll.first_visible_row = target.floor() as usize;
        self.layout.scroll.horizontal_px = target_horizontal;
    }

    pub(super) fn target_row(self) -> usize {
        self.layout.scroll.first_visible_row
    }

    /// Applies precise input from the displayed position, without an easing tail.
    pub(super) fn scroll_direct(
        &mut self,
        lines: f32,
        horizontal_px: f32,
        max_horizontal_px: f32,
        now: Instant,
    ) {
        self.stop(now);
        self.scroll_by(lines, horizontal_px, max_horizontal_px, now);
        self.from_rows = self.target_rows;
        self.from_horizontal_px = f64::from(self.layout.scroll.horizontal_px);
    }

    pub(super) fn target_horizontal_milli(self) -> u64 {
        horizontal_offset_milli(self.layout.scroll.horizontal_px)
    }

    pub(super) fn set_horizontal_offset(&mut self, offset_milli: u64) {
        self.layout.scroll.horizontal_px = offset_milli as f32 / 1000.0;
        self.from_horizontal_px = f64::from(self.layout.scroll.horizontal_px);
    }

    pub(super) fn animating(self, now: Instant) -> bool {
        (self.from_rows != self.target_rows
            || self.from_horizontal_px != f64::from(self.layout.scroll.horizontal_px))
            && now.saturating_duration_since(self.started) < SCROLL_DURATION
    }

    fn position(self, now: Instant) -> (f64, f64) {
        let progress = (now.saturating_duration_since(self.started).as_secs_f64()
            / SCROLL_DURATION.as_secs_f64())
        .min(1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        (
            self.from_rows + (self.target_rows - self.from_rows) * eased,
            self.from_horizontal_px
                + (f64::from(self.layout.scroll.horizontal_px) - self.from_horizontal_px) * eased,
        )
    }

    pub(super) fn display_layout(self, now: Instant) -> EditorLayout {
        let (position, horizontal_px) = self.position(now);
        let mut layout = self.layout;
        layout.scroll.first_visible_row = position.floor() as usize;
        layout.vertical_offset_px =
            (position.fract() * f64::from(layout.metrics.line_height)) as f32;
        layout.scroll.horizontal_px = horizontal_px as f32;
        layout
    }

    /// Stops at the visible position before a pointer starts selecting text.
    pub(super) fn stop(&mut self, now: Instant) {
        let (rows, horizontal_px) = self.position(now);
        self.target_rows = rows;
        self.from_rows = self.target_rows;
        self.layout.scroll.first_visible_row = self.target_rows.floor() as usize;
        self.layout.scroll.horizontal_px =
            horizontal_offset_milli(horizontal_px as f32) as f32 / 1000.0;
        self.from_horizontal_px = f64::from(self.layout.scroll.horizontal_px);
    }
}

pub(super) fn horizontal_offset_milli(offset_px: f32) -> u64 {
    (f64::from(offset_px.max(0.0)) * 1000.0).round() as u64
}
