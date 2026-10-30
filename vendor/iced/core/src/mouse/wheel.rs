//! Track wheel and touchpad input.
use crate::mouse;
use crate::time::{Duration, Instant};

/// Distinguishes whole wheel steps from precise scrolling by their input
/// pattern. Delta units alone do not identify a device: Windows touchpads also
/// produce line deltas. Fractional and pixel input stay direct, including later
/// whole-line packets within that gesture. Event timing does not establish
/// precision: fast wheel notches and batched events must remain animated.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct WheelScrollInput {
    last_event: Option<Instant>,
    continuous: bool,
    native_gesture: bool,
}

impl WheelScrollInput {
    /// Records an input packet and reports whether it should receive wheel
    /// easing. A pause of 250 ms starts a new gesture; fractional line deltas
    /// and pixel deltas select direct scrolling
    /// for the remainder of the gesture.
    pub fn should_animate(&mut self, delta: mouse::ScrollDelta, now: Instant) -> bool {
        self.should_animate_with_phase(delta, mouse::ScrollPhase::Moved, now)
    }

    /// Uses native gesture boundaries when present. A held gesture stays direct
    /// through pauses; input without boundaries uses the 250 ms idle timeout.
    /// Zero Moved packets do not extend the timeout or change inferred precision.
    /// Contact release restores wheel-step handling; native momentum remains
    /// direct when it arrives after the release.
    pub fn should_animate_with_phase(
        &mut self,
        delta: mouse::ScrollDelta,
        phase: mouse::ScrollPhase,
        now: Instant,
    ) -> bool {
        match phase {
            mouse::ScrollPhase::Started | mouse::ScrollPhase::Momentum => {
                self.native_gesture = true;
                self.continuous = true;
            }
            mouse::ScrollPhase::ContactEnded
            | mouse::ScrollPhase::Ended
            | mouse::ScrollPhase::Cancelled => {
                *self = Self::default();
                return false;
            }
            mouse::ScrollPhase::Moved => {}
        }
        if matches!(
            delta,
            mouse::ScrollDelta::Lines { x: 0.0, y: 0.0 }
                | mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 }
        ) {
            return !self.continuous;
        }
        let gap = self
            .last_event
            .map(|last| now.saturating_duration_since(last));
        if !self.native_gesture && gap.is_none_or(|gap| gap >= Duration::from_millis(250)) {
            self.continuous = false;
        }
        let whole_steps = match delta {
            mouse::ScrollDelta::Lines { x, y } => {
                x.is_finite()
                    && y.is_finite()
                    && x.fract() == 0.0
                    && y.fract() == 0.0
                    && (x != 0.0 || y != 0.0)
            }
            mouse::ScrollDelta::Pixels { .. } => false,
        };
        self.continuous |= !whole_steps;
        self.last_event = Some(now);
        !self.continuous
    }
}

#[cfg(test)]
mod tests {
    use super::WheelScrollInput;
    use crate::mouse;
    use crate::time::{Duration, Instant};

    #[test]
    fn continuous_input_stays_direct_through_whole_line_packets_and_resets_after_pause() {
        let now = Instant::now();
        let line = |y| mouse::ScrollDelta::Lines { x: 0.0, y };
        let mut input = WheelScrollInput::default();
        assert!(!input.should_animate(line(-0.125), now));
        assert!(!input.should_animate(line(-1.0), now + Duration::from_millis(60)));
        assert!(!input.should_animate(line(1.0), now + Duration::from_millis(120)));
        assert!(input.should_animate(line(-1.0), now + Duration::from_millis(400)));
        assert!(input.should_animate(line(-1.0), now + Duration::from_millis(460)));
        assert!(input.should_animate(line(-1.0), now + Duration::from_millis(470)));
        assert!(input.should_animate(line(-1.0), now + Duration::from_millis(570)));
        assert!(!input.should_animate(
            mouse::ScrollDelta::Pixels { x: 0.0, y: -1.0 },
            now + Duration::from_millis(900)
        ));
        assert!(!input.should_animate(line(-1.0), now + Duration::from_millis(950)));
        assert!(!input.should_animate(
            mouse::ScrollDelta::Lines { x: 0.25, y: 0.0 },
            now + Duration::from_millis(1300)
        ));
    }

    #[test]
    fn mouse_wheel_speed_and_batched_delivery_never_change_easing_mode() {
        let now = Instant::now();
        let mut input = WheelScrollInput::default();
        for milliseconds in [0, 0, 8, 24, 63, 104, 165, 200, 500, 501] {
            assert!(input.should_animate(
                mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
                now + Duration::from_millis(milliseconds)
            ));
        }
    }

    #[test]
    fn native_scroll_boundaries_keep_held_gestures_direct_through_pauses() {
        let now = Instant::now();
        let line = mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
        let zero = mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 };
        for end in [
            mouse::ScrollPhase::ContactEnded,
            mouse::ScrollPhase::Ended,
            mouse::ScrollPhase::Cancelled,
        ] {
            let mut input = WheelScrollInput::default();
            assert!(!input.should_animate_with_phase(zero, mouse::ScrollPhase::Started, now));
            assert!(!input.should_animate_with_phase(
                line,
                mouse::ScrollPhase::Moved,
                now + Duration::from_secs(1),
            ));
            assert!(!input.should_animate_with_phase(line, end, now + Duration::from_secs(2),));
            assert!(input.should_animate_with_phase(
                line,
                mouse::ScrollPhase::Moved,
                now + Duration::from_millis(2016),
            ));
        }
    }

    #[test]
    fn native_momentum_stays_direct_after_contact_release_and_a_pause() {
        let now = Instant::now();
        let line = mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
        let zero = mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 };
        let mut input = WheelScrollInput::default();
        assert!(!input.should_animate_with_phase(line, mouse::ScrollPhase::Started, now));
        assert!(!input.should_animate_with_phase(
            zero,
            mouse::ScrollPhase::ContactEnded,
            now + Duration::from_millis(16),
        ));
        assert!(input.should_animate_with_phase(
            line,
            mouse::ScrollPhase::Moved,
            now + Duration::from_millis(32),
        ));
        for milliseconds in [600, 1200] {
            assert!(!input.should_animate_with_phase(
                line,
                mouse::ScrollPhase::Momentum,
                now + Duration::from_millis(milliseconds),
            ));
        }
        assert!(!input.should_animate_with_phase(
            zero,
            mouse::ScrollPhase::Ended,
            now + Duration::from_millis(1216),
        ));
        assert!(input.should_animate_with_phase(
            line,
            mouse::ScrollPhase::Moved,
            now + Duration::from_millis(1232),
        ));
    }

    #[test]
    fn zero_movement_does_not_extend_an_inferred_precise_gesture() {
        let now = Instant::now();
        let mut input = WheelScrollInput::default();
        assert!(!input.should_animate(mouse::ScrollDelta::Lines { x: 0.25, y: 0.0 }, now));
        assert!(!input.should_animate(
            mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 },
            now + Duration::from_millis(240),
        ));
        assert!(input.should_animate(
            mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            now + Duration::from_millis(250),
        ));
    }
}
