use iced::Point;
use iced::advanced::input_method;
use iced::advanced::widget::{self};
use iced::time::Instant;
use std::cell::{Cell, RefCell};

use crate::editor::EditorBuffer;
use crate::editor::position::{EditorPosition, SelectionSet};

use super::LineGeometryCache;
use super::cache::RichParagraphCache;

pub(super) const CARET_BLINK_INTERVAL_MS: u128 = 500;
const DOUBLE_CLICK_INTERVAL_MS: u128 = 500;

#[derive(Debug)]
pub struct AdvancedEditorState<Paragraph> {
    pub(super) is_focused: bool,
    pub(super) is_window_focused: bool,
    pub(super) fold_controls_hovered: Cell<bool>,
    pub(super) caret_updated_at: Instant,
    pub(super) caret_now: Cell<Instant>,
    pub(super) drag_anchor: Option<EditorPosition>,
    pub(super) text_drag: Option<TextDrag>,
    pub(super) drag_position: Option<Point>,
    pub(super) drag_scroll_at: Option<Instant>,
    pub(super) last_text_click: Option<TextClick>,
    pub(super) scrollbar_grab_offset_y: Option<f32>,
    pub(super) partial_scroll_lines: f32,
    pub(super) viewport_geometry: Option<(u64, usize, u32, u32, u32, Option<u32>)>,
    pub preedit: Option<input_method::Preedit>,
    pub(super) scroll_fast_until: Cell<Option<Instant>>,
    pub(super) render_frame: Cell<u64>,
    pub(super) rich_paragraphs: RefCell<RichParagraphCache<Paragraph>>,
    pub(super) line_geometries: RefCell<LineGeometryCache<Paragraph>>,
}

impl<Paragraph> Default for AdvancedEditorState<Paragraph> {
    fn default() -> Self {
        let now = Instant::now();

        Self {
            is_focused: false,
            is_window_focused: true,
            fold_controls_hovered: Cell::new(false),
            caret_updated_at: now,
            caret_now: Cell::new(now),
            drag_anchor: None,
            text_drag: None,
            drag_position: None,
            drag_scroll_at: None,
            last_text_click: None,
            scrollbar_grab_offset_y: None,
            partial_scroll_lines: 0.0,
            viewport_geometry: None,
            preedit: None,
            scroll_fast_until: Cell::new(None),
            render_frame: Cell::new(0),
            rich_paragraphs: RefCell::new(RichParagraphCache::default()),
            line_geometries: RefCell::new(LineGeometryCache::default()),
        }
    }
}

impl<Paragraph> AdvancedEditorState<Paragraph> {
    pub(super) fn reset_caret_blink(&mut self) {
        let now = Instant::now();

        self.caret_updated_at = now;
        self.caret_now.set(now);
    }

    pub(super) fn record_text_click(&mut self, position: EditorPosition, now: Instant) -> bool {
        let is_double_click = self
            .last_text_click
            .is_some_and(|click| click.position == position && is_double_click_time(click.at, now));

        self.last_text_click = Some(TextClick { position, at: now });
        is_double_click
    }

    pub(super) fn clear_text_click(&mut self) {
        self.last_text_click = None;
    }

    pub(super) fn cancel_pointer_drag(&mut self) {
        self.drag_anchor = None;
        self.text_drag = None;
        self.drag_position = None;
        self.drag_scroll_at = None;
        self.scrollbar_grab_offset_y = None;
    }

    pub(super) fn is_caret_visible(&self) -> bool {
        caret_visible_at(
            self.is_focused,
            self.is_window_focused,
            self.caret_updated_at,
            self.caret_now.get(),
        )
    }
}

impl<Paragraph> widget::operation::Focusable for AdvancedEditorState<Paragraph> {
    fn is_focused(&self) -> bool {
        self.is_focused
    }

    fn focus(&mut self) {
        self.is_focused = true;
        self.reset_caret_blink();
    }

    fn unfocus(&mut self) {
        self.is_focused = false;
        self.cancel_pointer_drag();
        self.last_text_click = None;
        self.preedit = None;
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TextClick {
    position: EditorPosition,
    at: Instant,
}

#[derive(Debug)]
pub(super) struct TextDrag {
    pub source: SelectionSet,
    pub buffer: EditorBuffer,
    pub pressed_at: Point,
    pub clicked_position: EditorPosition,
    pub clicked_row: Option<usize>,
    pub target: Option<(EditorPosition, Option<usize>)>,
    pub started: bool,
}

fn is_double_click_time(previous: Instant, now: Instant) -> bool {
    (now - previous).as_millis() <= DOUBLE_CLICK_INTERVAL_MS
}

pub(super) fn is_scroll_fast_frame<Paragraph>(state: &AdvancedEditorState<Paragraph>) -> bool {
    let Some(settle_at) = state.scroll_fast_until.get() else {
        return false;
    };

    if Instant::now() < settle_at {
        true
    } else {
        state.scroll_fast_until.set(None);
        false
    }
}

pub(super) fn caret_visible_at(
    is_focused: bool,
    is_window_focused: bool,
    updated_at: Instant,
    now: Instant,
) -> bool {
    is_focused
        && is_window_focused
        && ((now - updated_at).as_millis() / CARET_BLINK_INTERVAL_MS).is_multiple_of(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::time::Duration;

    #[test]
    fn text_click_state_detects_same_position_double_click() {
        let mut state: AdvancedEditorState<()> = AdvancedEditorState::default();
        let now = Instant::now();
        let position = EditorPosition::new(2, 4);

        assert!(!state.record_text_click(position, now));
        assert!(state.record_text_click(
            position,
            now + Duration::from_millis(DOUBLE_CLICK_INTERVAL_MS as u64)
        ));
    }

    #[test]
    fn text_click_state_rejects_slow_or_different_position_clicks() {
        let mut state: AdvancedEditorState<()> = AdvancedEditorState::default();
        let now = Instant::now();

        assert!(!state.record_text_click(EditorPosition::new(0, 1), now));
        assert!(!state.record_text_click(
            EditorPosition::new(0, 1),
            now + Duration::from_millis(DOUBLE_CLICK_INTERVAL_MS as u64 + 1)
        ));
        assert!(!state.record_text_click(
            EditorPosition::new(0, 2),
            now + Duration::from_millis(DOUBLE_CLICK_INTERVAL_MS as u64 + 2)
        ));
    }

    #[test]
    fn losing_editor_focus_cancels_selection_and_scrollbar_drags() {
        use iced::advanced::widget::operation::Focusable;

        let mut state: AdvancedEditorState<()> = AdvancedEditorState::default();
        state.focus();
        state.drag_anchor = Some(EditorPosition::new(3, 2));
        state.drag_position = Some(Point::new(100.0, -20.0));
        state.drag_scroll_at = Some(Instant::now());
        state.scrollbar_grab_offset_y = Some(4.0);

        state.unfocus();

        assert!(!state.is_focused);
        assert!(state.drag_anchor.is_none());
        assert!(state.drag_position.is_none());
        assert!(state.drag_scroll_at.is_none());
        assert!(state.scrollbar_grab_offset_y.is_none());
    }
}
