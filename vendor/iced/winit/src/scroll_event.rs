use crate::core::{Event, mouse, window};

/// Joins legacy Windows messages that report the two axes separately.
/// Native pixel vectors and other platforms keep their original packet order.
/// Each line axis is kept once, preserving fractional input and boundaries.
pub(super) fn push_event(events: &mut Vec<(window::Id, Event)>, id: window::Id, event: Event) {
    if cfg!(target_os = "windows")
        && let Some((
            previous_id,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: previous,
                phase: mouse::ScrollPhase::Moved,
            }),
        )) = events.last_mut()
        && *previous_id == id
        && let Event::Mouse(mouse::Event::WheelScrolled {
            delta,
            phase: mouse::ScrollPhase::Moved,
        }) = &event
        && let Some(combined) = join_axes(*previous, *delta)
    {
        *previous = combined;
        return;
    }

    events.push((id, event));
}

fn join_axes(previous: mouse::ScrollDelta, next: mouse::ScrollDelta) -> Option<mouse::ScrollDelta> {
    use mouse::ScrollDelta;

    let (
        ScrollDelta::Lines { x, y },
        ScrollDelta::Lines {
            x: next_x,
            y: next_y,
        },
    ) = (previous, next)
    else {
        return None;
    };

    if !((x != 0.0 && y == 0.0 && next_x == 0.0 && next_y != 0.0)
        || (x == 0.0 && y != 0.0 && next_x != 0.0 && next_y == 0.0))
    {
        return None;
    }

    Some(ScrollDelta::Lines {
        x: x + next_x,
        y: y + next_y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(delta: mouse::ScrollDelta, phase: mouse::ScrollPhase) -> Event {
        Event::Mouse(mouse::Event::WheelScrolled { delta, phase })
    }

    #[test]
    fn only_windows_legacy_line_axes_join_in_either_order() {
        let id = window::Id::unique();
        let delta = |x, y| mouse::ScrollDelta::Lines { x, y };
        for reverse in [false, true] {
            let mut events = Vec::new();
            let mut pair = [delta(0.25, 0.0), delta(0.0, -0.5)];
            if reverse {
                pair.reverse();
            }
            for input in pair {
                push_event(&mut events, id, wheel(input, mouse::ScrollPhase::Moved));
            }
            if cfg!(target_os = "windows") {
                assert_eq!(
                    events,
                    [(id, wheel(delta(0.25, -0.5), mouse::ScrollPhase::Moved))]
                );
            } else {
                assert_eq!(
                    events,
                    pair.map(|delta| (id, wheel(delta, mouse::ScrollPhase::Moved)))
                );
            }
        }
    }

    #[test]
    fn native_pixel_vectors_keep_their_order_instead_of_forming_a_new_diagonal() {
        let id = window::Id::unique();
        for reverse in [false, true] {
            let mut pair = [
                mouse::ScrollDelta::Pixels { x: 0.0, y: 2.0 },
                mouse::ScrollDelta::Pixels { x: 1.0, y: 0.0 },
            ];
            if reverse {
                pair.reverse();
            }
            let mut events = Vec::new();
            for input in pair {
                push_event(&mut events, id, wheel(input, mouse::ScrollPhase::Moved));
            }
            assert_eq!(
                events,
                pair.map(|delta| (id, wheel(delta, mouse::ScrollPhase::Moved)))
            );
        }
    }

    #[test]
    fn merging_keeps_units_gesture_boundaries_windows_and_other_input_separate() {
        let id = window::Id::unique();
        let other = window::Id::unique();
        let vertical = mouse::ScrollDelta::Lines { x: 0.0, y: 0.5 };
        let horizontal = mouse::ScrollDelta::Lines { x: 0.25, y: 0.0 };
        let first = wheel(vertical, mouse::ScrollPhase::Moved);
        for (next_id, event) in [
            (other, wheel(horizontal, mouse::ScrollPhase::Moved)),
            (id, wheel(horizontal, mouse::ScrollPhase::Started)),
            (id, wheel(horizontal, mouse::ScrollPhase::ContactEnded)),
            (id, wheel(horizontal, mouse::ScrollPhase::Momentum)),
            (id, wheel(horizontal, mouse::ScrollPhase::Ended)),
            (id, wheel(horizontal, mouse::ScrollPhase::Cancelled)),
            (
                id,
                wheel(
                    mouse::ScrollDelta::Pixels { x: 0.25, y: 0.0 },
                    mouse::ScrollPhase::Moved,
                ),
            ),
            (id, wheel(vertical, mouse::ScrollPhase::Moved)),
            (
                id,
                wheel(
                    mouse::ScrollDelta::Lines { x: 0.25, y: 0.5 },
                    mouse::ScrollPhase::Moved,
                ),
            ),
            (id, Event::Mouse(mouse::Event::CursorLeft)),
        ] {
            let mut events = vec![(id, first.clone())];
            push_event(&mut events, next_id, event.clone());
            assert_eq!(events, [(id, first.clone()), (next_id, event)]);
        }
        let mut events = vec![
            (id, first.clone()),
            (id, Event::Mouse(mouse::Event::CursorLeft)),
        ];
        let last = wheel(horizontal, mouse::ScrollPhase::Moved);
        push_event(&mut events, id, last.clone());
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], (id, first));
        assert_eq!(events[2], (id, last));
    }
}
