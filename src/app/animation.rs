//! Time-based reveal state. This module does not depend on widgets or messages.

use std::time::{Duration, Instant};

const CHROME_REVEAL_ANIMATION_DURATION: Duration = Duration::from_millis(140);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ChromeAnimation {
    pub(super) find: RevealAnimation,
    pub(super) inline_replace: RevealAnimation,
    pub(super) function_list: RevealAnimation,
    pub(super) about: RevealAnimation,
    pub(super) selector: RevealAnimation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RevealAnimation {
    rendered_visible: bool,
    target_visible: bool,
    started_at: Option<Instant>,
    from: f32,
    progress: f32,
}

impl ChromeAnimation {
    pub(super) const fn new() -> Self {
        Self {
            find: RevealAnimation::hidden(),
            inline_replace: RevealAnimation::hidden(),
            function_list: RevealAnimation::hidden(),
            about: RevealAnimation::hidden(),
            selector: RevealAnimation::hidden(),
        }
    }

    pub(super) fn needs_frames(self) -> bool {
        self.find.needs_frames()
            || self.inline_replace.needs_frames()
            || self.function_list.needs_frames()
            || self.about.needs_frames()
            || self.selector.needs_frames()
    }

    pub(super) fn update_frame(&mut self, at: Instant) {
        self.find.update_frame(at);
        self.inline_replace.update_frame(at);
        self.function_list.update_frame(at);
        self.about.update_frame(at);
        self.selector.update_frame(at);
    }
}

impl RevealAnimation {
    pub(super) const fn hidden() -> Self {
        Self {
            rendered_visible: false,
            target_visible: false,
            started_at: None,
            from: 0.0,
            progress: 0.0,
        }
    }

    pub(super) fn set_visible(&mut self, visible: bool) {
        let target = if visible { 1.0 } else { 0.0 };

        if (self.progress - target).abs() <= f32::EPSILON {
            self.target_visible = visible;
            self.rendered_visible = visible;
            self.started_at = None;
            self.from = target;
            return;
        }

        if self.target_visible == visible {
            return;
        }

        self.target_visible = visible;
        self.rendered_visible = self.rendered_visible || visible || self.progress > 0.0;
        self.started_at = None;
        self.from = self.progress;
    }

    pub(super) fn needs_frames(self) -> bool {
        let target = if self.target_visible { 1.0 } else { 0.0 };

        // Easing can round to the target before the duration has elapsed.
        // Keep an active transition scheduled until update_frame finalizes its
        // visibility; otherwise an invisible modal can keep blocking input.
        self.rendered_visible
            && (self.started_at.is_some() || (self.progress - target).abs() > f32::EPSILON)
    }

    pub(super) fn update_frame(&mut self, at: Instant) {
        if !self.needs_frames() {
            return;
        }

        let started_at = match self.started_at {
            Some(started_at) => started_at,
            None => {
                self.started_at = Some(at);
                return;
            }
        };

        let elapsed = at.saturating_duration_since(started_at);
        let raw = (elapsed.as_secs_f32() / CHROME_REVEAL_ANIMATION_DURATION.as_secs_f32()).min(1.0);
        let eased = ease_out_cubic(raw);
        let target = if self.target_visible { 1.0 } else { 0.0 };

        self.progress = self.from + ((target - self.from) * eased);

        if raw >= 1.0 {
            self.progress = target;
            self.started_at = None;
            self.rendered_visible = self.target_visible;
            self.from = target;
        }
    }
}

impl RevealAnimation {
    pub(super) fn rendered_visible(self) -> bool {
        self.rendered_visible
    }
    pub(super) fn target_visible(self) -> bool {
        self.target_visible
    }
    pub(super) fn progress(self) -> f32 {
        self.progress.clamp(0.0, 1.0)
    }
}

fn ease_out_cubic(progress: f32) -> f32 {
    let inverse = 1.0 - progress.clamp(0.0, 1.0);

    1.0 - (inverse * inverse * inverse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_endpoints_still_complete_opening_closing_and_reversed_transitions() {
        let start = Instant::now();
        for reverse_at in [Duration::from_millis(35), CHROME_REVEAL_ANIMATION_DURATION] {
            let mut animation = RevealAnimation::hidden();
            animation.set_visible(true);
            animation.update_frame(start);
            animation.update_frame(start + reverse_at);
            animation.set_visible(false);
            let close_start = start + reverse_at;
            animation.update_frame(close_start);
            animation.update_frame(close_start + Duration::from_micros(139_900));
            assert!(animation.progress() <= f32::EPSILON);
            animation.update_frame(close_start + CHROME_REVEAL_ANIMATION_DURATION);
            assert!(
                !animation.rendered_visible(),
                "closing must remove the surface"
            );
            assert!(!animation.needs_frames());

            let reopen = close_start + Duration::from_secs(1);
            animation.set_visible(true);
            animation.update_frame(reopen);
            animation.update_frame(reopen + Duration::from_micros(139_900));
            assert_eq!(animation.progress(), 1.0);
            assert!(animation.needs_frames(), "finish the opening lifecycle too");
            animation.update_frame(reopen + CHROME_REVEAL_ANIMATION_DURATION);
            assert!(animation.rendered_visible());
            assert!(!animation.needs_frames());
        }
    }
}
