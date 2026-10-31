//! Headless editor input harness and input-method positioning checks.

mod pointer;
mod scrolling;

use super::*;
use crate::editor::decoration::DecorationSettings;
use crate::editor::fold::{FoldModel, FoldRange};
use crate::editor::layout::{EditorMetrics, ScrollOffset};
use crate::editor::position::{EditorPosition, EditorSelection, SelectionRange};
use iced::Point;
use iced::advanced::graphics::core::shell::Waker;

struct TestEditor {
    buffer: EditorBuffer,
    viewport: ViewportModel,
    decorations: DecorationModel,
    selections: SelectionSet,
    caret_row: Option<usize>,
    shortcuts: ShortcutMap,
    state: AdvancedEditorState<()>,
    now: Instant,
    layout: EditorLayout,
    bounds: Rectangle,
    last_redraw: window::RedrawRequest,
    last_input_method: InputMethod,
    smooth_scrolling: bool,
    scroll_speed: f32,
    max_horizontal_px: f32,
    viewport_key: u64,
}

impl TestEditor {
    fn new(text: &str) -> Self {
        Self::with_folds(text, FoldModel::default())
    }

    fn with_folds(text: &str, folds: FoldModel) -> Self {
        let buffer = EditorBuffer::from_text(text);
        let viewport = ViewportModel::new(buffer.line_count(), &folds);
        let decorations = DecorationModel::from_folds(
            DecorationSettings::default(),
            buffer.line_count(),
            &folds,
            vec![],
        );
        let layout = EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 400.0, 94.0);

        Self {
            buffer,
            viewport,
            decorations,
            selections: SelectionSet::new(EditorSelection::new(
                EditorPosition::new(0, 0),
                EditorPosition::new(0, 0),
            )),
            caret_row: None,
            shortcuts: ShortcutMap::default(),
            state: AdvancedEditorState::default(),
            now: Instant::now(),
            layout,
            bounds: Rectangle {
                x: 10.0,
                y: 20.0,
                width: layout.width,
                height: layout.height,
            },
            last_redraw: window::RedrawRequest::Wait,
            last_input_method: InputMethod::Disabled,
            smooth_scrolling: false,
            scroll_speed: 1.5,
            max_horizontal_px: 0.0,
            viewport_key: 0,
        }
    }

    fn text_point(&self, row: usize, column: usize) -> Point {
        Point::new(
            self.bounds.x
                + self.layout.metrics.text_origin_x(&self.decorations)
                + column as f32 * self.layout.metrics.character_width
                + 0.5,
            self.bounds.y
                + self.layout.metrics.padding_top
                + row as f32 * self.layout.metrics.line_height
                + 1.0,
        )
    }

    fn displayed_layout(&self) -> EditorLayout {
        self.state
            .smooth_scroll
            .map_or(self.layout, |scroll| scroll.display_layout(self.now))
    }

    fn dispatch_without_app_update(
        &mut self,
        event: Event,
        cursor: mouse::Cursor,
    ) -> Vec<EditorAction> {
        // Simulated input timing must not depend on font loading or runner load.
        self.now = match &event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => self.now + Duration::from_millis(16),
        };
        let mut messages = Vec::new();
        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
        handle_event(
            InteractionContext {
                now: self.now,
                buffer: &self.buffer,
                viewport: &self.viewport,
                decorations: &self.decorations,
                selections: &self.selections,
                metrics: self.layout.metrics,
                caret_row: self.caret_row,
                scroll_speed: self.scroll_speed,
                smooth_scrolling: self.smooth_scrolling,
                max_horizontal_px: self.max_horizontal_px,
                viewport_key: self.viewport_key,
                cjk_context: None,
                shortcuts: &self.shortcuts,
                on_action: &std::convert::identity,
            },
            &mut self.state,
            &event,
            self.bounds,
            cursor,
            self.layout,
            &(),
            &mut shell,
        );
        self.last_redraw = shell.redraw_request();
        self.last_input_method = shell.input_method().clone();
        messages
    }

    fn apply_messages(&mut self, messages: &[EditorAction]) {
        for action in messages {
            match action {
                EditorAction::PlaceCaret(position) => {
                    self.caret_row = None;
                    self.selections = SelectionSet::new(EditorSelection::new(*position, *position));
                }
                EditorAction::SelectRegion(selection) => {
                    self.caret_row = None;
                    self.selections = SelectionSet::new(*selection);
                }
                EditorAction::PlaceCaretOnRow { position, row } => {
                    self.caret_row = Some(*row);
                    self.selections = SelectionSet::new(EditorSelection::new(*position, *position));
                }
                EditorAction::SelectRegionOnRow { selection, row } => {
                    self.caret_row = Some(*row);
                    self.selections = SelectionSet::new(*selection);
                }
                EditorAction::ScrollToRow(row) => self.layout.scroll.first_visible_row = *row,
                EditorAction::ScrollToHorizontal { offset_milli } => {
                    self.layout.scroll.horizontal_px = *offset_milli as f32 / 1000.0;
                }
                EditorAction::ScrollLines(lines) => {
                    let max_row = self.viewport.visible_row_count().saturating_sub(1);
                    self.layout.scroll.first_visible_row = self
                        .layout
                        .scroll
                        .first_visible_row
                        .saturating_add_signed(*lines as isize)
                        .min(max_row);
                }
                _ => {}
            }
        }
    }

    fn dispatch(&mut self, event: Event, cursor: mouse::Cursor) -> Vec<EditorAction> {
        let messages = self.dispatch_without_app_update(event, cursor);
        self.apply_messages(&messages);
        messages
    }

    fn press(&mut self, button: mouse::Button, point: Point) -> Vec<EditorAction> {
        self.dispatch(
            Event::Mouse(mouse::Event::ButtonPressed(button)),
            mouse::Cursor::Available(point),
        )
    }

    fn wheel(&mut self, delta: mouse::ScrollDelta) -> Vec<EditorAction> {
        self.dispatch(
            Event::Mouse(mouse::Event::WheelScrolled {
                delta,
                phase: mouse::ScrollPhase::Moved,
            }),
            mouse::Cursor::Available(self.text_point(0, 0)),
        )
    }

    fn settle_precise(&mut self) {
        self.dispatch(
            Event::Window(window::Event::RedrawRequested(
                self.now + Duration::from_millis(64),
            )),
            mouse::Cursor::Unavailable,
        );
    }

    fn drag_to(&mut self, point: Point) -> Vec<EditorAction> {
        self.dispatch(
            Event::Mouse(mouse::Event::CursorMoved { position: point }),
            mouse::Cursor::Available(point),
        )
    }
}

#[test]
fn focused_redraws_retain_caret_shaping_and_update_ime_coordinates() {
    let _font_guard = crate::font_system_test_guard();
    let mut editor = TestEditor::new("caf\u{e9}\t漢字\n".repeat(20).as_str());
    let cursor = EditorPosition::new(8, "caf\u{e9}\t漢".len());
    editor.selections = SelectionSet::new(EditorSelection::new(cursor, cursor));
    editor.state.is_focused = true;
    let now = Instant::now();
    let mut first = None;
    for (frame, first_visible_row) in [0, 4, 8, 4, 0].into_iter().enumerate() {
        editor.layout.scroll.first_visible_row = first_visible_row;
        editor.layout.scroll.horizontal_px = if first_visible_row == 8 { 12.0 } else { 0.0 };
        for tick in 0..3 {
            editor.dispatch(
                Event::Window(window::Event::RedrawRequested(
                    now + Duration::from_millis((frame * 3 + tick) as u64 * 16),
                )),
                mouse::Cursor::Unavailable,
            );
            let expected = super::super::line_cache::measured_position_point_with_context(
                &editor.buffer,
                &editor.viewport,
                &editor.decorations,
                editor.layout,
                cursor,
                None,
                &(),
                None,
            );
            let InputMethod::Enabled {
                cursor, purpose, ..
            } = editor.last_input_method
            else {
                panic!("focused editor must keep the input method enabled");
            };
            assert_eq!(purpose, input_method::Purpose::Normal);
            assert_eq!(cursor.x, editor.bounds.x + expected.x);
            assert_eq!(cursor.y, editor.bounds.y + expected.y);
            if frame == 0 {
                first = Some(cursor);
            }
            if frame == 4 {
                assert_eq!(Some(cursor), first);
            }
        }
    }
    assert_eq!(
        editor.state.line_geometries.borrow().build_count(),
        1,
        "forced redraws and scroll-only changes must not reshape the caret paragraph"
    );
    editor.dispatch(
        Event::InputMethod(input_method::Event::Preedit(
            "候補".to_owned(),
            Some(0.."候補".len()),
        )),
        mouse::Cursor::Unavailable,
    );
    let InputMethod::Enabled {
        preedit: Some(ref preedit),
        ..
    } = editor.last_input_method
    else {
        panic!("preedit must still be published alongside retained caret geometry");
    };
    assert_eq!(preedit.content, "候補");
    assert_eq!(preedit.selection, Some(0.."候補".len()));
    assert_eq!(editor.state.line_geometries.borrow().build_count(), 1);
    editor.dispatch(
        Event::Window(window::Event::Unfocused),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(editor.last_redraw, window::RedrawRequest::NextFrame);
}
