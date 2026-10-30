use sctk::reexports::client::protocol::wl_pointer::AxisSource;
use sctk::seat::pointer::AxisScroll;

use crate::dpi::{LogicalPosition, PhysicalPosition};
use crate::event::{MouseScrollDelta, ScrollPhase};

#[derive(Debug, Default)]
pub(super) struct PointerScroll {
    source: Option<AxisSource>,
    horizontal_active: bool,
    vertical_active: bool,
}

impl PointerScroll {
    pub(super) fn cancel_for_source_change(
        &mut self,
        horizontal: AxisScroll,
        vertical: AxisScroll,
        source: Option<AxisSource>,
    ) -> Option<(MouseScrollDelta, ScrollPhase)> {
        // Wheel step events can omit axis_source.
        let source = source.or_else(|| {
            (horizontal.value120 != 0
                || vertical.value120 != 0
                || horizontal.discrete != 0
                || vertical.discrete != 0)
                .then_some(AxisSource::Wheel)
        });
        if source.is_some() && source != self.source {
            self.cancel()
        } else {
            None
        }
    }

    pub(super) fn update(
        &mut self,
        horizontal: AxisScroll,
        vertical: AxisScroll,
        source: Option<AxisSource>,
        scale_factor: f64,
    ) -> Option<(MouseScrollDelta, ScrollPhase)> {
        if let Some(source) = source {
            self.source = Some(source);
        }
        let has_value120 = horizontal.value120 != 0 || vertical.value120 != 0;
        let has_discrete = horizontal.discrete != 0 || vertical.discrete != 0;
        let wheel = has_value120
            || has_discrete
            || matches!(self.source, None | Some(AxisSource::Wheel | AxisSource::WheelTilt));
        if wheel {
            self.horizontal_active = false;
            self.vertical_active = false;
            let (x, y) = if has_value120 {
                (-(horizontal.value120 as f32) / 120.0, -(vertical.value120 as f32) / 120.0)
            } else if has_discrete {
                (-(horizontal.discrete as f32), -(vertical.discrete as f32))
            } else {
                // Older pointers report only surface distances. Compositors
                // conventionally use ten units for one wheel step.
                (-horizontal.absolute as f32 / 10.0, -vertical.absolute as f32 / 10.0)
            };
            return (x != 0.0 || y != 0.0)
                .then_some((MouseScrollDelta::LineDelta(x, y), ScrollPhase::Moved));
        }

        let was_active = self.horizontal_active || self.vertical_active;
        let moved = horizontal.absolute != 0.0 || vertical.absolute != 0.0;
        self.horizontal_active =
            (self.horizontal_active || horizontal.absolute != 0.0) && !horizontal.stop;
        self.vertical_active = (self.vertical_active || vertical.absolute != 0.0) && !vertical.stop;
        let active = self.horizontal_active || self.vertical_active;
        let phase = if !active && (was_active || moved) {
            self.source = None;
            ScrollPhase::Ended
        } else if !moved {
            return None;
        } else if !was_active {
            ScrollPhase::Started
        } else {
            ScrollPhase::Moved
        };
        // Wayland distances use surface coordinates; winit pixel events use
        // physical pixels. The runtime converts them back to logical pixels.
        let delta = LogicalPosition::new(-horizontal.absolute, -vertical.absolute)
            .to_physical(scale_factor);
        Some((MouseScrollDelta::PixelDelta(delta), phase))
    }

    pub(super) fn cancel(&mut self) -> Option<(MouseScrollDelta, ScrollPhase)> {
        let active = self.horizontal_active || self.vertical_active;
        *self = Self::default();
        active.then_some((
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.0)),
            ScrollPhase::Cancelled,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance(absolute: f64) -> AxisScroll {
        AxisScroll { absolute, ..AxisScroll::default() }
    }

    #[test]
    fn wheel_sources_keep_steps_and_do_not_open_touchpad_gestures() {
        let mut scroll = PointerScroll::default();
        for (vertical, source, expected) in [
            (distance(10.0), Some(AxisSource::Wheel), -1.0),
            (AxisScroll { absolute: 10.0, discrete: 1, ..AxisScroll::default() }, None, -1.0),
            (AxisScroll { absolute: 2.5, value120: 30, ..AxisScroll::default() }, None, -0.25),
        ] {
            assert_eq!(
                scroll.update(AxisScroll::default(), vertical, source, 2.0),
                Some((MouseScrollDelta::LineDelta(0.0, expected), ScrollPhase::Moved))
            );
        }
        assert!(scroll.cancel().is_none());
        let mut legacy = PointerScroll::default();
        assert_eq!(
            legacy.update(distance(15.0), distance(-20.0), None, 2.0),
            Some((MouseScrollDelta::LineDelta(-1.5, 2.0), ScrollPhase::Moved))
        );
    }

    #[test]
    fn finger_scroll_preserves_both_axes_and_surface_scale() {
        let mut scroll = PointerScroll::default();
        assert_eq!(
            scroll.update(distance(2.5), distance(-3.0), Some(AxisSource::Finger), 2.0),
            Some((
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(-5.0, 6.0)),
                ScrollPhase::Started
            ))
        );
        assert_eq!(
            scroll.update(distance(0.5), AxisScroll::default(), None, 2.0),
            Some((
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(-1.0, 0.0)),
                ScrollPhase::Moved
            ))
        );
    }

    #[test]
    fn stopping_one_axis_keeps_the_other_axis_in_the_same_gesture() {
        let mut scroll = PointerScroll::default();
        scroll.update(distance(1.0), distance(2.0), Some(AxisSource::Continuous), 1.0);
        let stopped = AxisScroll { stop: true, ..AxisScroll::default() };
        assert_eq!(
            scroll.update(stopped, distance(1.0), None, 1.0),
            Some((
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -1.0)),
                ScrollPhase::Moved
            ))
        );
        assert_eq!(
            scroll.update(AxisScroll::default(), stopped, None, 1.0),
            Some((
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.0)),
                ScrollPhase::Ended
            ))
        );
        assert_eq!(
            scroll.update(AxisScroll::default(), distance(2.0), None, 1.0),
            Some((MouseScrollDelta::LineDelta(0.0, -0.2), ScrollPhase::Moved))
        );
        assert_eq!(
            scroll.update(AxisScroll::default(), distance(2.0), Some(AxisSource::Continuous), 1.0),
            Some((
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -2.0)),
                ScrollPhase::Started
            ))
        );
        let mut single_frame = PointerScroll::default();
        assert_eq!(
            single_frame.update(
                AxisScroll::default(),
                AxisScroll { absolute: 2.0, stop: true, ..AxisScroll::default() },
                Some(AxisSource::Finger),
                1.0,
            ),
            Some((
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -2.0)),
                ScrollPhase::Ended
            ))
        );
        assert!(single_frame.cancel().is_none());
    }

    #[test]
    fn source_only_frames_do_not_start_scrolling_and_leaving_cancels_movement() {
        let mut scroll = PointerScroll::default();
        assert!(scroll
            .update(AxisScroll::default(), AxisScroll::default(), Some(AxisSource::Finger), 1.0)
            .is_none());
        assert!(scroll.cancel().is_none());
        scroll.update(distance(1.0), AxisScroll::default(), Some(AxisSource::Finger), 1.0);
        assert_eq!(
            scroll.cancel(),
            Some((
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.0)),
                ScrollPhase::Cancelled
            ))
        );
        assert_eq!(
            scroll.update(AxisScroll::default(), distance(10.0), Some(AxisSource::Wheel), 1.0),
            Some((MouseScrollDelta::LineDelta(0.0, -1.0), ScrollPhase::Moved))
        );
    }

    #[test]
    fn switching_from_finger_to_wheel_cancels_before_delivering_wheel_steps() {
        for (vertical, source) in [
            (distance(10.0), Some(AxisSource::Wheel)),
            (AxisScroll { absolute: 10.0, discrete: 1, ..AxisScroll::default() }, None),
            (AxisScroll { absolute: 10.0, value120: 120, ..AxisScroll::default() }, None),
        ] {
            let mut scroll = PointerScroll::default();
            assert!(matches!(
                scroll.update(distance(1.0), distance(2.0), Some(AxisSource::Finger), 1.0),
                Some((MouseScrollDelta::PixelDelta(_), ScrollPhase::Started))
            ));
            let events = [
                scroll.cancel_for_source_change(AxisScroll::default(), vertical, source),
                scroll.update(AxisScroll::default(), vertical, source, 1.0),
            ];
            assert_eq!(
                events,
                [
                    Some((
                        MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.0)),
                        ScrollPhase::Cancelled
                    )),
                    Some((MouseScrollDelta::LineDelta(0.0, -1.0), ScrollPhase::Moved)),
                ]
            );
            assert!(scroll
                .cancel_for_source_change(AxisScroll::default(), vertical, source)
                .is_none());
            assert_eq!(
                scroll.update(AxisScroll::default(), vertical, source, 1.0),
                Some((MouseScrollDelta::LineDelta(0.0, -1.0), ScrollPhase::Moved))
            );
        }
    }
}
