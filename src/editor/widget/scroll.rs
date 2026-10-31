use iced::time::{Duration, Instant};

use crate::editor::layout::{EditorLayout, ScrollOffset};

pub(super) const SCROLL_DURATION: Duration = Duration::from_millis(140);

/// Keeps wheel and touchpad motion in the widget; the document stores destination offsets.
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
    duration: Duration,
    precise: bool,
    // Velocities are measured in rows/second and pixels/second.
    velocity_rows: f64,
    velocity_horizontal: f64,
    last_precise_input: Option<Instant>,
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
            duration: SCROLL_DURATION,
            precise: false,
            velocity_rows: 0.0,
            velocity_horizontal: 0.0,
            last_precise_input: None,
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
    ) -> bool {
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
            return false;
        }
        self.from_rows = current;
        self.target_rows = target;
        self.from_horizontal_px = current_horizontal;
        self.started = now;
        self.duration = SCROLL_DURATION;
        self.precise = false;
        self.last_precise_input = None;
        self.layout.scroll.first_visible_row = target.floor() as usize;
        self.layout.scroll.horizontal_px = target_horizontal;
        true
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
    ) -> bool {
        let was_animating = self.animating(now);
        self.stop(now);
        let changed = self.scroll_by(lines, horizontal_px, max_horizontal_px, now);
        self.from_rows = self.target_rows;
        self.from_horizontal_px = f64::from(self.layout.scroll.horizontal_px);
        changed || was_animating
    }

    /// Applies whole wheel lines without animation, allowing the last row at the top.
    pub(super) fn scroll_lines(&mut self, lines: i32) -> bool {
        let target = self
            .layout
            .scroll
            .first_visible_row
            .saturating_add_signed(lines as isize)
            .min(self.row_count.saturating_sub(1));
        if target == self.layout.scroll.first_visible_row {
            return false;
        }
        self.layout.scroll.first_visible_row = target;
        self.target_rows = target as f64;
        self.from_rows = self.target_rows;
        self.precise = false;
        self.last_precise_input = None;
        true
    }

    /// Spreads precise packets across a short interval while keeping their sum.
    /// Native momentum is already included in the packets; this adds none.
    pub(super) fn scroll_precise(
        &mut self,
        lines: f32,
        horizontal_px: f32,
        max_horizontal_px: f32,
        now: Instant,
    ) -> bool {
        let ((current, current_horizontal), (velocity_rows, velocity_horizontal)) =
            self.position_and_velocity(now);
        let (previous_rows, previous_horizontal) = if self.precise {
            (
                self.target_rows,
                f64::from(self.layout.scroll.horizontal_px),
            )
        } else {
            (current, current_horizontal)
        };
        let max_row = self
            .row_count
            .saturating_sub(self.layout.complete_visible_row_capacity().max(1));
        let target_rows = (previous_rows + f64::from(lines)).clamp(0.0, max_row as f64);
        let horizontal = (previous_horizontal + f64::from(horizontal_px))
            .clamp(0.0, f64::from(max_horizontal_px));
        let target_horizontal = horizontal_offset_milli(horizontal as f32) as f32 / 1000.0;
        if target_rows == self.target_rows && target_horizontal == self.layout.scroll.horizontal_px
        {
            // Preserve packet timing without restarting motion already approaching a limit.
            self.last_precise_input = Some(now);
            return false;
        }
        self.from_rows = current;
        self.from_horizontal_px = current_horizontal;
        self.target_rows = target_rows;
        self.layout.scroll.first_visible_row = target_rows.floor() as usize;
        self.layout.scroll.horizontal_px = target_horizontal;
        self.duration = self
            .last_precise_input
            .map(|previous| now.saturating_duration_since(previous))
            .unwrap_or(Duration::from_millis(16))
            .clamp(Duration::from_millis(8), Duration::from_millis(24))
            * 2;
        let omega = 8.0 / self.duration.as_secs_f64();
        let forward_velocity = |velocity: f64, distance: f64| {
            distance.signum() * (velocity * distance.signum()).clamp(0.0, omega * distance.abs())
        };
        self.velocity_rows = forward_velocity(velocity_rows, self.target_rows - current);
        self.velocity_horizontal = forward_velocity(
            velocity_horizontal,
            f64::from(self.layout.scroll.horizontal_px) - current_horizontal,
        );
        self.started = now;
        self.last_precise_input = Some(now);
        self.precise = true;
        true
    }

    /// Ends interpolation at the exact native destination on release or cancel.
    pub(super) fn finish_precise(&mut self) -> bool {
        if !self.precise {
            return false;
        }
        self.from_rows = self.target_rows;
        self.from_horizontal_px = f64::from(self.layout.scroll.horizontal_px);
        self.precise = false;
        self.last_precise_input = None;
        true
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
            && now.saturating_duration_since(self.started) < self.duration
    }

    fn position(self, now: Instant) -> (f64, f64) {
        self.position_and_velocity(now).0
    }

    /// Returns the row/pixel position and its velocity per second.
    fn position_and_velocity(self, now: Instant) -> ((f64, f64), (f64, f64)) {
        let elapsed = now.saturating_duration_since(self.started);
        if self.precise {
            let (rows, velocity_rows) = precise_axis(
                self.from_rows,
                self.target_rows,
                self.velocity_rows,
                elapsed,
                self.duration,
            );
            let (horizontal, velocity_horizontal) = precise_axis(
                self.from_horizontal_px,
                f64::from(self.layout.scroll.horizontal_px),
                self.velocity_horizontal,
                elapsed,
                self.duration,
            );
            return ((rows, horizontal), (velocity_rows, velocity_horizontal));
        }
        let progress = (elapsed.as_secs_f64() / self.duration.as_secs_f64()).min(1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        let slope = 3.0 * (1.0 - progress).powi(2) / self.duration.as_secs_f64();
        let rows = self.target_rows - self.from_rows;
        let horizontal = f64::from(self.layout.scroll.horizontal_px) - self.from_horizontal_px;
        (
            (
                self.from_rows + rows * eased,
                self.from_horizontal_px + horizontal * eased,
            ),
            (rows * slope, horizontal * slope),
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
        self.precise = false;
        self.last_precise_input = None;
    }
}

/// Evaluates a critically damped approach and finishes at the received target.
fn precise_axis(
    from: f64,
    target: f64,
    velocity: f64,
    elapsed: Duration,
    duration: Duration,
) -> (f64, f64) {
    if elapsed >= duration {
        return (target, 0.0);
    }
    let seconds = elapsed.as_secs_f64();
    let omega = 8.0 / duration.as_secs_f64();
    let offset = from - target;
    let coefficient = velocity + omega * offset;
    let decay = (-omega * seconds).exp();
    (
        target + (offset + coefficient * seconds) * decay,
        (velocity - omega * coefficient * seconds) * decay,
    )
}

pub(super) fn horizontal_offset_milli(offset_px: f32) -> u64 {
    (f64::from(offset_px.max(0.0)) * 1000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::layout::EditorMetrics;

    fn scroll(now: Instant) -> ScrollAnimation {
        let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 400.0, 94.0);
        ScrollAnimation::new(layout, 1, 1000, None, 94.0, now)
    }

    #[test]
    fn unchanged_targets_preserve_motion_while_fractional_changes_are_reported() {
        let started = Instant::now();
        let mut motion = scroll(started);
        assert!(!motion.scroll_by(-1.0, -1.0, 10.0, started));
        assert!(!motion.scroll_direct(-1.0, -1.0, 10.0, started));
        assert!(!motion.scroll_precise(-1.0, -1.0, 10.0, started));
        assert!(motion.scroll_by(0.25, 0.0, 10.0, started));
        assert_eq!(motion.target_row(), 0);

        let mut motion = scroll(started);
        assert!(motion.scroll_precise(2000.0, 2000.0, 10.0, started));
        let previous = motion;
        let next = started + Duration::from_millis(16);
        assert!(!motion.scroll_precise(1.0, 1.0, 10.0, next));
        for elapsed in [16, 24, 32, 48] {
            let now = started + Duration::from_millis(elapsed);
            assert_eq!(motion.display_layout(now), previous.display_layout(now));
        }
    }

    #[test]
    fn precise_motion_advances_between_packets_and_keeps_their_total() {
        let started = Instant::now();
        let mut scroll = scroll(started);
        let packet_interval = Duration::from_nanos(1_000_000_000 / 60);
        for packet in 0..6 {
            let now = started + packet_interval * packet;
            scroll.scroll_precise(1.0, 8.0, 1000.0, now);
            let first = scroll.position(now);
            let middle = scroll.position(now + packet_interval / 2);
            let last = scroll.position(now + packet_interval);
            assert!(first.0 < middle.0 && middle.0 < last.0);
            assert!(first.1 < middle.1 && middle.1 < last.1);
            assert!(last.0 <= f64::from(packet + 1));
            assert!(last.1 <= f64::from(packet + 1) * 8.0);
        }
        let settled = started + packet_interval * 6 + Duration::from_millis(48);
        assert_eq!(scroll.position(settled), (6.0, 48.0));
        assert!(!scroll.animating(settled));
    }

    #[test]
    fn precise_retargets_preserve_velocity_and_reverse_without_crossing_the_target() {
        let started = Instant::now();
        let mut scroll = scroll(started);
        scroll.scroll_precise(1.0, 8.0, 1000.0, started);
        let next = started + Duration::from_millis(8);
        let (position, velocity) = scroll.position_and_velocity(next);
        scroll.scroll_precise(1.0, 8.0, 1000.0, next);
        let retargeted = scroll.position(next);
        assert!((retargeted.0 - position.0).abs() < 1e-12);
        assert!((retargeted.1 - position.1).abs() < 1e-12);
        assert_eq!(scroll.position_and_velocity(next).1, velocity);
        assert_eq!(
            scroll.position(next + Duration::from_millis(48)),
            (2.0, 16.0)
        );

        let reversed = next + Duration::from_millis(4);
        let position = scroll.position(reversed);
        scroll.scroll_precise(-3.0, -20.0, 1000.0, reversed);
        assert_eq!(scroll.position_and_velocity(reversed).1, (0.0, 0.0));
        for millis in 0..=48 {
            let current = scroll.position(reversed + Duration::from_millis(millis));
            assert!(current.0 >= 0.0 && current.0 <= position.0 + 1e-12);
            assert!(current.1 >= 0.0 && current.1 <= position.1 + 1e-12);
        }
        assert_eq!(
            scroll.position(reversed + Duration::from_millis(48)),
            (0.0, 0.0)
        );
    }

    #[test]
    fn precise_packets_in_one_batch_accumulate_and_respect_scroll_limits() {
        let started = Instant::now();
        let mut scroll = scroll(started);
        scroll.scroll_precise(1.0, 8.0, 10.0, started);
        scroll.scroll_precise(1.0, 8.0, 10.0, started);
        assert_eq!(
            scroll.position(started + Duration::from_millis(48)),
            (2.0, 10.0)
        );
        scroll.scroll_precise(2000.0, 2000.0, 10.0, started);
        assert_eq!(
            scroll.target_row(),
            1000 - scroll.layout.complete_visible_row_capacity()
        );
        assert_eq!(scroll.target_horizontal_milli(), 10_000);
        assert!(scroll.finish_precise());
        assert!(!scroll.animating(started));
        assert_eq!(scroll.position(started).1, 10.0);
    }

    #[test]
    fn stopping_precise_motion_keeps_the_displayed_position() {
        let started = Instant::now();
        let mut scroll = scroll(started);
        scroll.scroll_precise(2.0, 20.0, 1000.0, started);
        let clicked = started + Duration::from_millis(8);
        let displayed = scroll.position(clicked);
        scroll.stop(clicked);
        let stopped = scroll.position(clicked + Duration::from_millis(100));
        assert!((stopped.0 - displayed.0).abs() < 1e-12);
        assert!((stopped.1 - displayed.1).abs() < 0.001);
        assert!(!scroll.animating(clicked));
    }
}
