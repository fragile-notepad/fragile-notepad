//! Shared coordinates for tab painting, insertion, and edge scrolling.

use iced::Point;

use crate::core::DocumentId;

use super::StripGeometry;

const DETACH_MARGIN: f32 = 36.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct DragPosition {
    pub pointer_x: f32,
    pub grab_offset: f32,
    pub group_width: f32,
}

impl DragPosition {
    pub(super) fn overflow_x(self, viewport_width: f32) -> f32 {
        self.pointer_x - self.grab_offset - self.leading_x(self.group_width, viewport_width)
    }

    pub(super) fn leading_x(self, group_width: f32, viewport_width: f32) -> f32 {
        let leading = self.pointer_x - self.grab_offset;
        if group_width <= viewport_width {
            leading.clamp(0.0, (viewport_width - group_width).max(0.0))
        } else {
            // Keep the grab point visible when the selection exceeds the viewport.
            let inset = 32.0_f32.min(viewport_width / 2.0);
            leading.clamp(
                inset - self.grab_offset,
                viewport_width - inset - self.grab_offset,
            )
        }
    }
}

impl StripGeometry {
    /// Horizontal exits retain the drag; vertical exits use a larger detach margin.
    pub(crate) fn retains_drag(&self, point: Point) -> bool {
        point.y >= self.bounds.y - DETACH_MARGIN
            && point.y <= self.bounds.y + self.bounds.height + DETACH_MARGIN
    }

    pub(crate) fn drag_position(
        &self,
        point: Point,
        selected: &[DocumentId],
        grab_offset: f32,
    ) -> DragPosition {
        DragPosition {
            pointer_x: point.x - self.bounds.x,
            grab_offset,
            group_width: self
                .tabs
                .iter()
                .filter(|(id, _)| selected.contains(id))
                .map(|(_, bounds)| bounds.width)
                .sum(),
        }
    }

    /// Exclude the held group so insertion thresholds stay fixed after reordering.
    pub(crate) fn drag_insertion(
        &self,
        point: Point,
        selected: &[DocumentId],
        grab_offset: f32,
    ) -> usize {
        let drag = self.drag_position(point, selected, grab_offset);
        let leading = self.bounds.x + drag.leading_x(drag.group_width, self.bounds.width);
        let mut x = self.tabs.first().map_or(self.bounds.x, |(_, rect)| rect.x);
        for (index, (id, rect)) in self.tabs.iter().enumerate() {
            if selected.contains(id) {
                continue;
            }
            if leading < x + rect.width / 2.0 {
                return index;
            }
            x += rect.width;
        }
        self.tabs.len()
    }
}
