use super::*;
use crate::core::DocumentId;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::{Id, Operation, Tree};
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Event, Rectangle, Renderer, Size, window};

#[derive(Default)]
struct NoticeText(Vec<(String, Rectangle)>);

impl Operation for NoticeText {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
        self.0.push((text.to_owned(), bounds));
    }
}

#[test]
fn disk_notice_wraps_and_save_as_remains_clickable_in_a_narrow_window() {
    let renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("CPU headless renderer must be available");
    let mut document = Document::untitled(DocumentId::new(7));
    document.set_path("notes.txt");

    for state in [
        DiskState::Missing,
        DiskState::Changed,
        DiskState::Unreadable(std::io::ErrorKind::PermissionDenied),
        DiskState::Unreadable(std::io::ErrorKind::Other),
    ] {
        document.disk_state = state;
        let mut heights = Vec::new();
        for width in [240.0, 800.0] {
            let viewport = Rectangle::with_size(Size::new(width, 600.0));
            let mut content = disk_notice_slot(Some(&document));
            let mut tree = Tree::empty();
            tree.diff(content.as_widget_mut());
            let node = content.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, viewport.size()),
            );
            heights.push(node.bounds().height);
            let mut text = NoticeText::default();
            content
                .as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut text);
            assert!(
                text.0
                    .iter()
                    .any(|(label, _)| label.contains("keep editing"))
            );
            for (_, bounds) in &text.0 {
                assert!(bounds.width > 0.0 && bounds.height > 0.0);
                assert!(bounds.x >= 0.0 && bounds.x + bounds.width <= width);
                assert!(bounds.y >= 0.0 && bounds.y + bounds.height <= node.bounds().height);
            }
            let button = text
                .0
                .iter()
                .find(|(label, _)| label == "Save As")
                .expect("the notice must offer Save As")
                .1;
            let mut messages = Vec::new();
            for event in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
                content.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(event),
                    Layout::new(&node),
                    mouse::Cursor::Available(button.center()),
                    &renderer,
                    &mut shell,
                    &viewport,
                );
            }
            assert!(matches!(messages.as_slice(), [Message::SaveFileAs]));
        }
        assert!(
            heights[0] > heights[1],
            "the notice must wrap at narrow widths"
        );
        assert!(
            heights[1] <= 32.0,
            "the notice should stay compact when its text fits"
        );
    }

    document.disk_state = DiskState::Current;
    let mut content = disk_notice_slot(Some(&document));
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());
    let node = content.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(240.0, 600.0)),
    );
    assert_eq!(node.bounds().height, 0.0);
}
