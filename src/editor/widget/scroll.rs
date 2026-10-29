use iced::time::{Duration, Instant};

use crate::editor::layout::EditorLayout;

pub(super) const SCROLL_DURATION: Duration = Duration::from_millis(140);

/// Keeps wheel motion in the widget; the document stores the destination row.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScrollAnimation {
    layout: EditorLayout,
    viewport_key: u64,
    row_count: usize,
    wrap_columns: Option<usize>,
    from_rows: f64,
    target_rows: f64,
    started: Instant,
}

impl ScrollAnimation {
    pub(super) fn new(
        layout: EditorLayout,
        viewport_key: u64,
        row_count: usize,
        wrap_columns: Option<usize>,
        now: Instant,
    ) -> Self {
        Self {
            layout,
            viewport_key,
            row_count,
            wrap_columns,
            from_rows: layout.scroll.first_visible_row as f64,
            target_rows: layout.scroll.first_visible_row as f64,
            started: now,
        }
    }

    pub(super) fn matches(
        self,
        layout: EditorLayout,
        viewport_key: u64,
        row_count: usize,
        wrap_columns: Option<usize>,
    ) -> bool {
        self.layout == layout
            && self.viewport_key == viewport_key
            && self.row_count == row_count
            && self.wrap_columns == wrap_columns
    }

    pub(super) fn scroll_by(&mut self, lines: f32, now: Instant) {
        let max_row = self
            .row_count
            .saturating_sub(self.layout.complete_visible_row_capacity().max(1));
        let current = self.position(now);
        let reversing =
            self.animating(now) && (self.target_rows - current) * f64::from(lines) < 0.0;
        let previous_target = if reversing { current } else { self.target_rows };
        let target = (previous_target + f64::from(lines)).clamp(0.0, max_row as f64);
        if target == self.target_rows {
            return;
        }
        self.from_rows = current;
        self.target_rows = target;
        self.started = now;
        self.layout.scroll.first_visible_row = target.floor() as usize;
    }

    pub(super) fn target_row(self) -> usize {
        self.layout.scroll.first_visible_row
    }

    pub(super) fn animating(self, now: Instant) -> bool {
        self.from_rows != self.target_rows
            && now.saturating_duration_since(self.started) < SCROLL_DURATION
    }

    fn position(self, now: Instant) -> f64 {
        let progress = (now.saturating_duration_since(self.started).as_secs_f64()
            / SCROLL_DURATION.as_secs_f64())
        .min(1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        self.from_rows + (self.target_rows - self.from_rows) * eased
    }

    pub(super) fn display_layout(self, now: Instant) -> EditorLayout {
        let position = self.position(now);
        let mut layout = self.layout;
        layout.scroll.first_visible_row = position.floor() as usize;
        layout.vertical_offset_px =
            (position.fract() * f64::from(layout.metrics.line_height)) as f32;
        layout
    }

    /// Stops at the visible position before a pointer starts selecting text.
    pub(super) fn stop(&mut self, now: Instant) {
        self.target_rows = self.position(now);
        self.from_rows = self.target_rows;
        self.layout.scroll.first_visible_row = self.target_rows.floor() as usize;
    }
}
