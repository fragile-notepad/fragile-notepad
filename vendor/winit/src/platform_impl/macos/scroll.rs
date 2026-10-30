use crate::event::{ScrollPhase, TouchPhase};

/// AppKit reports finger movement and momentum as separate phases. Keep them
/// connected, and discard old momentum after another input cancels scrolling.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ScrollGesture {
    active: bool,
    ignore_momentum: bool,
    recent_x: f64,
    recent_y: f64,
}

impl ScrollGesture {
    /// Filters native deltas before they are queued. Both axes contribute to the
    /// recent direction, including movement omitted from the returned delta.
    pub(super) fn filter_delta(
        &mut self,
        x: f64,
        y: f64,
        contact: Option<TouchPhase>,
        momentum: Option<TouchPhase>,
    ) -> Option<(f64, f64, ScrollPhase)> {
        let phase = self.phase(contact, momentum)?;
        if (contact.is_none() && momentum.is_none()) || (x == 0.0 && y == 0.0) {
            return Some((x, y, phase));
        }

        // Guide each event using recent absolute movement, allowing
        // diagonal motion and choosing a rail again as the direction changes.
        self.recent_x = self.recent_x * 0.8 + x.abs();
        self.recent_y = self.recent_y * 0.8 + y.abs();
        let (x, y) = if self.recent_x >= self.recent_y * 2.0 {
            (x, 0.0)
        } else if self.recent_y >= self.recent_x * 2.0 {
            (0.0, y)
        } else {
            (x, y)
        };
        Some((x, y, phase))
    }

    fn phase(
        &mut self,
        contact: Option<TouchPhase>,
        momentum: Option<TouchPhase>,
    ) -> Option<ScrollPhase> {
        if contact == Some(TouchPhase::Started) {
            self.ignore_momentum = false;
            self.active = true;
            self.recent_x = 0.0;
            self.recent_y = 0.0;
            return Some(ScrollPhase::Started);
        }
        if momentum.is_some() && self.ignore_momentum {
            return None;
        }
        if contact == Some(TouchPhase::Cancelled) || momentum == Some(TouchPhase::Cancelled) {
            self.cancel();
            return Some(ScrollPhase::Cancelled);
        }
        if let Some(momentum) = momentum {
            self.active = momentum != TouchPhase::Ended;
            return Some(if self.active { ScrollPhase::Momentum } else { ScrollPhase::Ended });
        }
        if let Some(contact) = contact {
            self.active = true;
            return Some(if contact == TouchPhase::Ended {
                ScrollPhase::ContactEnded
            } else {
                ScrollPhase::Moved
            });
        }
        Some(ScrollPhase::Moved)
    }

    /// Returns whether the view must send a zero-delta cancellation event.
    pub(super) fn cancel(&mut self) -> bool {
        self.ignore_momentum = true;
        std::mem::take(&mut self.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_direction_allows_diagonal_motion_and_selects_a_rail_again() {
        let mut gesture = ScrollGesture::default();
        assert_eq!(
            gesture.filter_delta(2.0, 1.0, Some(TouchPhase::Started), None),
            Some((2.0, 0.0, ScrollPhase::Started))
        );
        assert_eq!(
            gesture.filter_delta(1.9, 1.0, Some(TouchPhase::Started), None),
            Some((1.9, 1.0, ScrollPhase::Started))
        );

        let deltas = [
            (0.0, 10.0, TouchPhase::Started),
            (10.0, 10.0, TouchPhase::Moved),
            (10.0, 1.0, TouchPhase::Moved),
            (10.0, 1.0, TouchPhase::Moved),
            (10.0, 1.0, TouchPhase::Moved),
            (1.0, 10.0, TouchPhase::Moved),
            (1.0, 10.0, TouchPhase::Moved),
            (1.0, 10.0, TouchPhase::Moved),
            (1.0, 10.0, TouchPhase::Moved),
        ]
        .map(|(x, y, phase)| gesture.filter_delta(x, y, Some(phase), None));
        assert_eq!(
            deltas,
            [
                Some((0.0, 10.0, ScrollPhase::Started)),
                Some((10.0, 10.0, ScrollPhase::Moved)),
                Some((10.0, 1.0, ScrollPhase::Moved)),
                Some((10.0, 1.0, ScrollPhase::Moved)),
                Some((10.0, 0.0, ScrollPhase::Moved)),
                Some((1.0, 10.0, ScrollPhase::Moved)),
                Some((1.0, 10.0, ScrollPhase::Moved)),
                Some((1.0, 10.0, ScrollPhase::Moved)),
                Some((0.0, 10.0, ScrollPhase::Moved)),
            ]
        );
    }

    #[test]
    fn omitted_minor_movement_still_contributes_to_the_next_direction() {
        let mut gesture = ScrollGesture::default();
        assert_eq!(
            gesture.filter_delta(10.0, 1.0, Some(TouchPhase::Started), None),
            Some((10.0, 0.0, ScrollPhase::Started))
        );
        assert_eq!(
            gesture.filter_delta(1.0, 3.8, Some(TouchPhase::Moved), None),
            Some((1.0, 3.8, ScrollPhase::Moved))
        );
    }

    #[test]
    fn phaseless_input_and_zero_deltas_do_not_change_native_direction_history() {
        let mut gesture = ScrollGesture::default();
        gesture.filter_delta(100.0, 1.0, Some(TouchPhase::Started), None);
        assert_eq!(
            gesture.filter_delta(1.0, 1000.0, None, None),
            Some((1.0, 1000.0, ScrollPhase::Moved))
        );
        for _ in 0..10 {
            gesture.filter_delta(0.0, 0.0, Some(TouchPhase::Moved), None);
        }
        assert_eq!(
            gesture.filter_delta(1.0, 20.0, Some(TouchPhase::Moved), None),
            Some((1.0, 0.0, ScrollPhase::Moved))
        );
    }

    #[test]
    fn momentum_keeps_the_finger_direction_until_a_new_contact_starts() {
        let mut gesture = ScrollGesture::default();
        gesture.filter_delta(100.0, 1.0, Some(TouchPhase::Started), None);
        assert_eq!(
            gesture.filter_delta(0.0, 0.0, Some(TouchPhase::Ended), None),
            Some((0.0, 0.0, ScrollPhase::ContactEnded))
        );
        assert_eq!(
            gesture.filter_delta(1.0, 1.0, None, Some(TouchPhase::Started)),
            Some((1.0, 0.0, ScrollPhase::Momentum))
        );
        assert_eq!(
            gesture.filter_delta(1.0, 10.0, Some(TouchPhase::Started), None),
            Some((0.0, 10.0, ScrollPhase::Started))
        );
    }

    #[test]
    fn finger_lift_and_momentum_continue_one_gesture() {
        let mut gesture = ScrollGesture::default();
        let phases = [
            (Some(TouchPhase::Started), None),
            (Some(TouchPhase::Moved), None),
            (Some(TouchPhase::Ended), None),
            (None, Some(TouchPhase::Started)),
            (None, Some(TouchPhase::Moved)),
            (None, Some(TouchPhase::Ended)),
        ]
        .map(|(contact, momentum)| gesture.phase(contact, momentum));
        assert_eq!(
            phases,
            [
                Some(ScrollPhase::Started),
                Some(ScrollPhase::Moved),
                Some(ScrollPhase::ContactEnded),
                Some(ScrollPhase::Momentum),
                Some(ScrollPhase::Momentum),
                Some(ScrollPhase::Ended),
            ]
        );
        assert!(!gesture.cancel(), "completed inertia must not leave an active gesture");
    }

    #[test]
    fn cancellation_blocks_old_momentum_until_a_new_contact() {
        let mut gesture = ScrollGesture::default();
        gesture.phase(Some(TouchPhase::Started), None);
        gesture.phase(Some(TouchPhase::Ended), None);
        assert!(gesture.cancel());
        assert!(!gesture.cancel());
        assert_eq!(gesture.phase(None, Some(TouchPhase::Started)), None);
        assert_eq!(gesture.phase(None, Some(TouchPhase::Moved)), None);
        assert_eq!(gesture.phase(None, Some(TouchPhase::Ended)), None);
        assert_eq!(
            gesture.phase(None, None),
            Some(ScrollPhase::Moved),
            "ordinary wheels remain available"
        );
        assert_eq!(gesture.phase(Some(TouchPhase::Started), None), Some(ScrollPhase::Started));
        assert_eq!(gesture.phase(None, Some(TouchPhase::Started)), Some(ScrollPhase::Momentum));
    }

    #[test]
    fn native_cancellation_is_distinct_from_a_finger_lift() {
        for (contact, momentum) in
            [(Some(TouchPhase::Cancelled), None), (None, Some(TouchPhase::Cancelled))]
        {
            let mut gesture = ScrollGesture::default();
            gesture.phase(Some(TouchPhase::Started), None);
            assert_eq!(gesture.phase(contact, momentum), Some(ScrollPhase::Cancelled));
            assert!(!gesture.cancel());
            assert_eq!(gesture.phase(None, Some(TouchPhase::Moved)), None);
        }
    }
}
