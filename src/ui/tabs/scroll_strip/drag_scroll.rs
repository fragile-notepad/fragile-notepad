//! Pointer displacement beyond the held group's visible bounds becomes scrolling.

use std::time::Instant;

use super::super::DragPosition;

#[derive(Default)]
pub(super) struct DragScroll {
    pub(super) last_scroll: Option<Instant>,
    pub(super) last_pointer: Option<(f32, Instant)>,
    pub(super) velocity: Option<f32>,
}

impl DragScroll {
    pub(super) fn press(&mut self, pointer_x: f32, now: Instant) {
        self.last_pointer = Some((pointer_x, now));
    }

    pub(super) fn reset(&mut self, keep_pointer: bool) {
        self.last_scroll = None;
        self.velocity = None;
        if !keep_pointer {
            self.last_pointer = None;
        }
    }

    pub(super) fn delta(
        &mut self,
        drag: DragPosition,
        viewport_width: f32,
        now: Instant,
        redraw: bool,
    ) -> f32 {
        let overflow = drag.overflow_x(viewport_width);
        let previous = self.last_pointer;
        let moved = previous.is_some_and(|(x, _)| x != drag.pointer_x);
        let frame_elapsed = self.last_scroll.map_or(0.016, |last| {
            now.saturating_duration_since(last).as_secs_f32().min(0.05)
        });
        let delta = if moved {
            let (x, sampled_at) = previous.unwrap();
            let elapsed = now
                .saturating_duration_since(sampled_at)
                .as_secs_f32()
                .max(0.001);
            self.velocity = Some((drag.pointer_x - x) / elapsed);
            overflow
                - DragPosition {
                    pointer_x: x,
                    ..drag
                }
                .overflow_x(viewport_width)
        } else if overflow != 0.0 && redraw {
            let direction = overflow.signum();
            // Continue at the last outward speed; reversal stops that continuation.
            let speed = self
                .velocity
                .map_or(480.0, |speed| (speed * direction).max(0.0));
            direction * speed * frame_elapsed
        } else {
            0.0
        };
        if moved || previous.is_none() {
            self.last_pointer = Some((drag.pointer_x, now));
        }
        if overflow == 0.0 {
            self.last_scroll = None;
        } else if moved || redraw {
            self.last_scroll = Some(now);
        }
        delta
    }
}
