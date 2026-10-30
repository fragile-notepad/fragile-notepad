use super::*;
use crate::core::DocumentId;
use crate::editor::{EditorPosition, EditorSelection, SelectionSet};
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::{Id, Operation, Tree};
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Color, Event, Point, Rectangle, Renderer, Size, Vector, window};

#[test]
fn document_status_distinguishes_unsaved_missing_changed_and_failed_files() {
    let settings = EditorSettings::default();
    let mut document = Document::untitled(DocumentId::new(1));
    for dirty in [false, true] {
        document.is_dirty = dirty;
        let data = StatusBarData::new(&document, &settings, None);
        assert_eq!(data.document_status, "Not saved");
        assert_eq!(data.tone, StatusTone::Warning);
    }
    document.set_path("notes.txt");
    for (state, label, tone) in [
        (DiskState::Current, "Modified", StatusTone::Warning),
        (DiskState::Missing, "Missing on disk", StatusTone::Danger),
        (DiskState::Changed, "Changed on disk", StatusTone::Warning),
        (
            DiskState::Unreadable(std::io::ErrorKind::PermissionDenied),
            "File unavailable",
            StatusTone::Danger,
        ),
    ] {
        document.disk_state = state;
        let data = StatusBarData::new(&document, &settings, None);
        assert_eq!(data.document_status, label);
        assert_eq!(data.tone, tone);
    }
    document.disk_state = DiskState::Current;
    document.mark_clean();
    assert_eq!(
        StatusBarData::new(&document, &settings, None).document_status,
        "Saved"
    );

    let mut failed = Document::loading(
        DocumentId::new(2),
        "missing.txt",
        crate::core::DocumentLoadGeneration::next(),
    );
    failed.fail_loading(failed.load_generation().unwrap());
    let data = StatusBarData::new(
        &failed,
        &settings,
        Some("Open failed: file or folder not found"),
    );
    assert_eq!(data.document_status, "Load failed");
    assert_eq!(data.tone, StatusTone::Danger);
    assert!(data.file_tooltip.contains("file or folder not found"));
}

#[test]
fn decoding_warning_remains_after_transient_file_status_clears() {
    let document = Document::from_decoded(
        DocumentId::new(1),
        "malformed.txt",
        crate::core::decode_bytes(b"\xef\xbb\xbfhello\xff"),
    );
    assert_eq!(document_status_label(&document, None), "Decoding errors");
}

#[test]
fn cursor_display_column_uses_unicode_and_configured_tab_stops() {
    let mut document = Document::from_path(DocumentId::new(1), "unicode.txt", "é\t好");
    let caret = EditorPosition::new(0, "é\t".len());
    document.set_main_selection(EditorSelection::new(caret, caret));
    for (tab_width, column) in [(2, 3), (4, 5), (8, 9)] {
        assert_eq!(cursor_display_column(&document, tab_width), column);
    }
}

#[test]
fn selection_summary_distinguishes_a_caret_selected_text_and_multiple_cursors() {
    let mut document = Document::from_path(DocumentId::new(1), "notes.txt", "hello");
    let settings = EditorSettings::default();
    assert!(
        StatusBarData::new(&document, &settings, None)
            .selection
            .is_none()
    );

    let start = EditorPosition::new(0, 0);
    let end = EditorPosition::new(0, 5);
    document.set_main_selection(EditorSelection::new(start, end));
    assert_eq!(
        StatusBarData::new(&document, &settings, None)
            .selection
            .as_deref(),
        Some("1 selection"),
    );

    document.set_selection_set(SelectionSet::from_ranges(
        vec![
            EditorSelection::new(start, start),
            EditorSelection::new(end, end),
        ],
        0,
    ));
    assert_eq!(
        StatusBarData::new(&document, &settings, None)
            .selection
            .as_deref(),
        Some("2 cursors"),
    );
    document.set_selection_set(SelectionSet::from_ranges(
        vec![
            EditorSelection::new(start, EditorPosition::new(0, 1)),
            EditorSelection::new(EditorPosition::new(0, 3), end),
        ],
        0,
    ));
    assert_eq!(
        StatusBarData::new(&document, &settings, None)
            .selection
            .as_deref(),
        Some("2 selections"),
    );
}

#[derive(Default)]
struct Labels(Vec<(String, Rectangle)>);

impl Operation for Labels {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
        self.0.push((text.to_owned(), bounds));
    }
}

fn send(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    event: Event,
    cursor: mouse::Cursor,
) -> Vec<Message> {
    let mut messages = Vec::new();
    element.as_widget_mut().update(
        tree,
        &event,
        Layout::new(node),
        cursor,
        renderer,
        &mut Shell::new(&window::Headless, Waker::noop(), &mut messages),
        &Rectangle::with_size(node.size()),
    );
    messages
}

#[test]
fn resizing_keeps_labels_inside_the_bar_and_status_actions_work() {
    let _guard = crate::font_system_test_guard();
    let renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let document = Document::from_path(
        DocumentId::new(1),
        "projects/notes/日本語/a-very-long-document-name-that-needs-shortening.txt",
        "hello\r\nworld",
    );
    let settings = EditorSettings {
        word_wrap: false,
        ..EditorSettings::default()
    };
    let mut content = view(Some(&document), &settings, None);
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());

    for width in [1000.0, 640.0, 360.0, 240.0, 1000.0] {
        let size = Size::new(width, STATUS_BAR_HEIGHT);
        let limits = layout::Limits::new(size, size);
        let node = content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        send(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
            mouse::Cursor::Unavailable,
        );
        let node = content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        assert_eq!(node.size(), size);
        let mut labels = Labels::default();
        content
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut labels);
        assert!(!labels.0.iter().any(|(label, _)| label == "1 selection"));
        for (label, bounds) in &labels.0 {
            assert!(
                bounds.x >= 0.0 && bounds.x + bounds.width <= width,
                "{label}: {bounds:?} at {width}"
            );
            assert!(
                bounds.y >= 1.0 && bounds.y + bounds.height <= STATUS_BAR_HEIGHT,
                "{label}: {bounds:?}"
            );
        }
        for pair in labels.0.windows(2) {
            assert!(
                pair[0].1.x + pair[0].1.width <= pair[1].1.x,
                "overlapping labels at {width}: {pair:?}"
            );
        }

        for (label, expected) in [("Ln 1, Col 1", 0), ("No wrap", 1), ("100%", 2)] {
            let bounds = labels
                .0
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, bounds)| *bounds);
            if width == 1000.0 || expected == 0 {
                let bounds =
                    bounds.unwrap_or_else(|| panic!("{label} should be visible at {width}"));
                let mut messages = Vec::new();
                for event in [
                    mouse::Event::ButtonPressed(mouse::Button::Left),
                    mouse::Event::ButtonReleased(mouse::Button::Left),
                ] {
                    messages.extend(send(
                        &mut content,
                        &mut tree,
                        &node,
                        &renderer,
                        Event::Mouse(event),
                        mouse::Cursor::Available(bounds.center()),
                    ));
                }
                assert!(matches!(
                    (expected, messages.as_slice()),
                    (0, [Message::GoToLineOpened])
                        | (1, [Message::ToggleWordWrap])
                        | (2, [Message::ZoomReset])
                ));
            }
        }
    }
}

#[test]
fn visible_details_fit_the_system_font_without_shortening_labels() {
    let _guard = crate::font_system_test_guard();
    let renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let mut document = Document::from_path(DocumentId::new(1), "notes.txt", "hello");
    // Wide letters expose width estimates that happen to fit narrower labels.
    document.syntax_token = "WWWWWWWW".to_owned();
    let settings = EditorSettings::default();
    let data = StatusBarData::new(&document, &settings, None);

    for width in [1000.0, 640.0, 360.0] {
        let mut content = status_bar_for_width(&data, width);
        let mut tree = Tree::empty();
        tree.diff(content.as_widget_mut());
        let node = content.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(width, STATUS_BAR_HEIGHT)),
        );
        let mut labels = Labels::default();
        content
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut labels);

        for item in visible_status_items(&data, width) {
            let label = item.label(&data);
            let bounds = labels
                .0
                .iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("{label} should be visible at {width}"))
                .1;
            let mut full_text: Element<'_, Message> =
                status_text(label).ellipsis(text::Ellipsis::None).into();
            let mut full_tree = Tree::empty();
            full_tree.diff(full_text.as_widget_mut());
            let full_node =
                full_text
                    .as_widget_mut()
                    .layout(&mut full_tree, &renderer, &layout::Limits::NONE);
            assert!(
                bounds.width >= full_node.size().width,
                "{label}: {bounds:?} is too narrow for {:?} at {width}",
                full_node.size(),
            );
        }
    }
}

#[test]
fn narrow_layout_preserves_position_and_prioritizes_active_selections() {
    let mut document = Document::from_path(DocumentId::new(1), "notes.txt", "hello");
    document.set_main_selection(EditorSelection::new(
        EditorPosition::new(0, 0),
        EditorPosition::new(0, 5),
    ));
    let data = StatusBarData::new(&document, &EditorSettings::default(), None);
    assert_eq!(visible_status_items(&data, 240.0), vec![StatusItem::Cursor]);
    let visible = visible_status_items(&data, 480.0);
    assert!(visible.contains(&StatusItem::Selection));
    assert!(!visible.contains(&StatusItem::LineCount));
}

#[test]
fn tooltip_fades_lifts_reverses_and_stops_requesting_frames() {
    use iced::advanced::Renderer as _;
    use std::time::{Duration, Instant};

    struct Frame {
        millis: u64,
        darkness: u64,
        center_y: f64,
    }

    let _guard = crate::font_system_test_guard();
    let mut renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let size = Size::new(320.0, 180.0);
    let viewport = Rectangle::with_size(size);
    let mut content: Element<'_, Message> = iced::widget::column![
        space::vertical().height(120),
        with_tooltip(
            button(status_text("100%"))
                .width(80)
                .height(22)
                .on_press(Message::ZoomReset),
            "Reset zoom to 100%",
        ),
    ]
    .into();
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());
    let node = content.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let start = Instant::now();
    let background = Color::from_rgb8(220, 220, 220);
    let mut frames = Vec::new();
    for (millis, hovered, visible, expected_frame) in [
        (0, true, false, true),
        (449, true, false, true),
        (450, true, true, true),
        (510, true, true, true),
        (570, true, true, false),
        (571, false, true, true),
        (631, false, true, true),
        (632, true, true, true),
        (692, true, true, false),
        (693, false, true, true),
        (813, false, false, true),
        (814, false, false, false),
    ] {
        let cursor = if hovered {
            mouse::Cursor::Available(Point::new(40.0, 130.0))
        } else {
            mouse::Cursor::Unavailable
        };
        let mut messages = Vec::new();
        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
        content.as_widget_mut().update(
            &mut tree,
            &Event::Window(window::Event::RedrawRequested(
                start + Duration::from_millis(millis),
            )),
            Layout::new(&node),
            cursor,
            &renderer,
            &mut shell,
            &viewport,
        );
        assert_eq!(
            shell.redraw_request() != window::RedrawRequest::Wait,
            expected_frame,
            "at {millis}ms"
        );
        assert!(shell.is_empty());

        let overlay = content.as_widget_mut().overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &viewport,
            Vector::ZERO,
        );
        assert_eq!(overlay.is_some(), visible, "at {millis}ms");
        if let Some(mut overlay) = overlay {
            let overlay_node = overlay.as_overlay_mut().layout(&renderer, size);
            renderer.reset(viewport);
            overlay.as_overlay().draw(
                &mut renderer,
                &iced::Theme::Dark,
                &renderer::Style::default(),
                Layout::new(&overlay_node),
                cursor,
            );
            let pixels = renderer.screenshot(Size::new(320, 180), 1.0, background);
            let (darkness, weighted_y) = pixels.chunks_exact(4).enumerate().fold(
                (0_u64, 0_u64),
                |(darkness, weighted_y), (index, pixel)| {
                    let weight = pixel[..3]
                        .iter()
                        .map(|channel| u64::from(channel.abs_diff(220)))
                        .sum::<u64>();
                    (
                        darkness + weight,
                        weighted_y + (index as u64 / 320) * weight,
                    )
                },
            );
            frames.push(Frame {
                millis,
                darkness,
                center_y: weighted_y as f64 / darkness.max(1) as f64,
            });
        }
    }
    let frame = |time| frames.iter().find(|frame| frame.millis == time).unwrap();
    assert_eq!(frame(450).darkness, 0);
    assert!(frame(510).darkness > 0 && frame(510).darkness < frame(570).darkness);
    assert!(frame(631).darkness > 0 && frame(631).darkness < frame(570).darkness);
    assert_eq!(frame(570).darkness, frame(692).darkness);
    assert!(frame(510).center_y > frame(570).center_y + 1.0);
}
