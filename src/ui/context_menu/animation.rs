//! Widget-local timing for popup entrance and dismissal.

use std::time::{Duration, Instant};

const ENTER_DURATION: Duration = Duration::from_millis(150);
const EXIT_DURATION: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Transition {
    closing: bool,
    started_at: Option<Instant>,
    from: f32,
    progress: f32,
}

impl Transition {
    pub(super) fn entering() -> Self {
        Self {
            closing: false,
            started_at: None,
            from: 0.0,
            progress: 0.0,
        }
    }

    pub(super) fn dismiss(&mut self) {
        if !self.closing {
            self.closing = true;
            self.started_at = None;
            self.from = self.progress;
        }
    }

    pub(super) fn advance(&mut self, now: Instant) {
        if !self.needs_frames() {
            return;
        }
        let started = *self.started_at.get_or_insert(now);
        let duration = if self.closing {
            EXIT_DURATION
        } else {
            ENTER_DURATION
        };
        let raw = (now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32())
            .min(1.0);
        let target = if self.closing { 0.0 } else { 1.0 };
        self.progress = self.from + (target - self.from) * (1.0 - (1.0 - raw).powi(3));
        if raw >= 1.0 {
            self.progress = target;
            self.from = target;
            self.started_at = None;
        }
    }

    pub(super) fn progress(self) -> f32 {
        self.progress.clamp(0.0, 1.0)
    }

    pub(super) fn is_closing(self) -> bool {
        self.closing
    }

    pub(super) fn needs_frames(self) -> bool {
        let target = if self.closing { 0.0 } else { 1.0 };
        self.started_at.is_some() || self.progress != target
    }

    pub(super) fn is_hidden(self) -> bool {
        self.closing && !self.needs_frames()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrance_and_exit_start_on_the_first_frame_and_settle() {
        let start = Instant::now();
        let mut transition = Transition::entering();
        assert!(transition.needs_frames());
        assert!(!transition.is_closing());
        transition.advance(start);
        assert_eq!(transition.progress(), 0.0);
        transition.advance(start + ENTER_DURATION / 2);
        assert_eq!(transition.progress(), 0.875);
        transition.advance(start + ENTER_DURATION);
        assert_eq!(transition.progress(), 1.0);
        assert!(!transition.needs_frames());
        transition.dismiss();
        assert!(transition.is_closing());
        transition.advance(start + Duration::from_secs(1));
        assert_eq!(transition.progress(), 1.0);
        transition.advance(start + Duration::from_secs(1) + EXIT_DURATION / 2);
        assert_eq!(transition.progress(), 0.125);
        transition.advance(start + Duration::from_secs(1) + EXIT_DURATION);
        assert!(transition.is_hidden());
        assert_eq!(transition.progress(), 0.0);
        assert!(!transition.needs_frames());
    }

    #[test]
    fn dismissal_reverses_from_the_current_progress_and_is_idempotent() {
        let start = Instant::now();
        let mut transition = Transition::entering();
        transition.advance(start);
        transition.advance(start + ENTER_DURATION / 2);
        transition.dismiss();
        transition.advance(start + ENTER_DURATION);
        assert_eq!(transition.progress(), 0.875);
        transition.advance(start + ENTER_DURATION + EXIT_DURATION / 2);
        assert_eq!(transition.progress(), 0.109375);
        let before = transition;
        transition.dismiss();
        assert_eq!(transition, before);
        transition.advance(start + ENTER_DURATION + Duration::from_micros(99_900));
        assert!(
            transition.needs_frames(),
            "finish even if progress rounds to zero"
        );
        transition.advance(start + ENTER_DURATION + EXIT_DURATION);
        assert!(transition.is_hidden());
        let before = transition;
        transition.dismiss();
        transition.advance(start + Duration::from_secs(3));
        assert_eq!(transition, before);
    }

    #[test]
    fn dismissal_before_the_first_frame_needs_no_exit() {
        let mut transition = Transition::entering();
        assert!(!transition.is_hidden());
        transition.dismiss();
        assert!(transition.is_hidden());
        assert!(!transition.needs_frames());
    }
}
