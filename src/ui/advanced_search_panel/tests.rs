use super::*;
use crate::core::DocumentId;
use crate::editor::{EditorPosition, EditorSelection};
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Event, Point, Rectangle, Renderer, Size, window};

#[test]
fn result_click_highlights_and_double_click_navigates_across_rebuild() {
    let renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let result = SearchResult {
        document_id: DocumentId::new(1),
        document_title: "notes.txt".into(),
        selection: EditorSelection::new(EditorPosition::new(2, 0), EditorPosition::new(2, 6)),
        preview: "needle in a line".into(),
        preview_match: 0..6,
    };
    let viewport = Rectangle::with_size(Size::new(400.0, 100.0));

    // Both the preview and the row padding should respond to the same clicks.
    for point in [Point::new(100.0, 12.0), Point::new(2.0, 2.0)] {
        let mut tree = Tree::empty();
        for selected in [false, true] {
            let mut element = result_row(&result, selected);
            tree.diff(element.as_widget_mut());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, viewport.size()),
            );
            let cursor = mouse::Cursor::Available(point);
            assert_eq!(
                element.as_widget().mouse_interaction(
                    &tree,
                    Layout::new(&node),
                    cursor,
                    &viewport,
                    &renderer,
                ),
                mouse::Interaction::Pointer,
            );

            let mut messages = Vec::new();
            for event in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
                element.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(event),
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut shell,
                    &viewport,
                );
            }

            if selected {
                assert!(
                    matches!(
                        messages.as_slice(),
                        [Message::AdvancedSearchResultHighlighted(id, selection),
                            Message::AdvancedSearchResultSelected(target, range)]
                            if *id == result.document_id && *target == result.document_id
                                && *selection == result.selection && *range == result.selection
                    ),
                    "second click: {messages:?}"
                );
            } else {
                assert!(
                    matches!(
                        messages.as_slice(),
                        [Message::AdvancedSearchResultHighlighted(id, selection)]
                            if *id == result.document_id && *selection == result.selection
                    ),
                    "first click: {messages:?}"
                );
            }
        }
    }
}
