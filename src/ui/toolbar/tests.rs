//! Toolbar commands exercised through real widget layout, clicks, and rendering.
use crate::core::{Document, DocumentId, EditorSettings};
use crate::editor::{
    DecorationSettings, EditTransaction, EditorPosition, EditorRange, EditorSelection,
};
use crate::message::{BulkScope, Message};
use crate::ui::toolbar;
use iced::advanced::{
    Layout, Shell,
    graphics::core::shell::Waker,
    layout, mouse,
    renderer::{self, Headless},
    widget::Tree,
};
use iced::{Color, Element, Event, Rectangle, Size, Theme};

const SIZE: Size = Size::new(640.0, 36.0);

fn renderer() -> iced::Renderer {
    futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn settle(
    element: &mut Element<'_, Message>,
    renderer: &iced::Renderer,
) -> (Tree, layout::Node, Vec<Rectangle>) {
    let mut tree = Tree::empty();
    tree.diff(element.as_widget_mut());
    let node =
        element
            .as_widget_mut()
            .layout(&mut tree, renderer, &layout::Limits::new(Size::ZERO, SIZE));
    let mut messages = Vec::new();
    element.as_widget_mut().update(
        &mut tree,
        &Event::Window(iced::window::Event::RedrawRequested(
            std::time::Instant::now(),
        )),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        renderer,
        &mut Shell::new(&iced::window::Headless, Waker::noop(), &mut messages),
        &Rectangle::with_size(SIZE),
    );
    fn collect(layout: Layout<'_>, bounds: &mut Vec<Rectangle>) {
        let current = layout.bounds();
        if current.size() == Size::new(28.0, 28.0) && !bounds.contains(&current) {
            bounds.push(current);
        }
        for child in layout.children() {
            collect(child, bounds);
        }
    }
    let mut buttons = Vec::new();
    collect(Layout::new(&node), &mut buttons);
    buttons.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(buttons.len(), 19, "all toolbar buttons must be laid out");
    assert_eq!(node.size(), SIZE);
    for button in &buttons {
        assert!(button.x >= 0.0 && button.x + button.width <= SIZE.width);
        assert!(button.y >= 0.0 && button.y + button.height <= SIZE.height);
    }
    (tree, node, buttons)
}

fn click(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &iced::Renderer,
    bounds: Rectangle,
) -> Vec<Message> {
    let mut messages = Vec::new();
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        element.as_widget_mut().update(
            tree,
            &Event::Mouse(event),
            Layout::new(node),
            mouse::Cursor::Available(bounds.center()),
            renderer,
            &mut Shell::new(&iced::window::Headless, Waker::noop(), &mut messages),
            &Rectangle::with_size(SIZE),
        );
    }
    messages
}

fn command_name(message: &Message) -> &'static str {
    match message {
        Message::NewFile => "new",
        Message::OpenFile => "open",
        Message::SaveFile => "save",
        Message::SaveScoped(BulkScope::ThisWindow) => "save-window",
        Message::CloseFile => "close",
        Message::CloseScoped(BulkScope::ThisWindow) => "close-window",
        Message::Undo => "undo",
        Message::Redo => "redo",
        Message::Cut => "cut",
        Message::Copy => "copy",
        Message::Paste => "paste",
        Message::ToggleFind => "find",
        Message::ShowInlineReplace => "replace",
        Message::ZoomOut => "zoom-out",
        Message::ZoomIn => "zoom-in",
        Message::ToggleWordWrap => "wrap",
        Message::ToggleAllCharacters => "characters",
        Message::ToggleIndentationGuides => "indent-guides",
        Message::ToggleFunctionList => "functions",
        other => panic!("unexpected toolbar command: {other:?}"),
    }
}

fn append(document: &mut Document, text: &str) {
    let before = document.main_selection();
    let start = before.cursor;
    let end = EditorPosition::new(start.line, start.column + text.len());
    let after = EditorSelection::new(end, end);
    let delta = document
        .buffer
        .replace_range(EditorRange::new(start, start), text);
    document.history.record(EditTransaction {
        delta,
        before_selection: before,
        after_selection: after,
    });
    document.set_main_selection(after);
}

#[test]
fn all_toolbar_commands_fit_and_receive_clicks_at_minimum_window_width() {
    let _guard = crate::font_system_test_guard();
    let renderer = renderer();
    let settings = EditorSettings::default();
    let mut document = Document::untitled(DocumentId::new(1));
    append(&mut document, "notes");
    append(&mut document, "!");
    assert!(document.undo());
    document.set_main_selection(EditorSelection::new(
        EditorPosition::new(0, 0),
        EditorPosition::new(0, 2),
    ));
    let mut element = toolbar::tool_bar(Some(&document), &settings, false);
    let (mut tree, node, buttons) = settle(&mut element, &renderer);
    let expected = [
        "new",
        "open",
        "save",
        "save-window",
        "close",
        "close-window",
        "undo",
        "redo",
        "cut",
        "copy",
        "paste",
        "find",
        "replace",
        "zoom-out",
        "zoom-in",
        "wrap",
        "characters",
        "indent-guides",
        "functions",
    ];
    for (bounds, expected) in buttons.into_iter().zip(expected) {
        let messages = click(&mut element, &mut tree, &node, &renderer, bounds);
        assert_eq!(messages.len(), 1, "click must emit {expected}");
        assert_eq!(command_name(&messages[0]), expected);
    }
}

#[test]
fn editing_buttons_follow_document_text_and_history() {
    let _guard = crate::font_system_test_guard();
    let renderer = renderer();
    let settings = EditorSettings::default();
    let empty = Document::untitled(DocumentId::new(1));
    let text = Document::from_path(DocumentId::new(2), "notes.txt", "notes");
    for (document, enabled) in [
        (None, [false, false, false, false, false]),
        (Some(&empty), [false, false, false, false, true]),
        (Some(&text), [false, false, true, true, true]),
    ] {
        let mut element = toolbar::tool_bar(document, &settings, false);
        let (mut tree, node, buttons) = settle(&mut element, &renderer);
        // Cut and Copy also work on the current line without a selection.
        for (bounds, enabled) in buttons[6..11].iter().zip(enabled) {
            let messages = click(&mut element, &mut tree, &node, &renderer, *bounds);
            assert_eq!(messages.len(), usize::from(enabled));
        }
    }
}

fn snapshot(
    renderer: &mut iced::Renderer,
    settings: &EditorSettings,
    functions_visible: bool,
    theme: &Theme,
) -> (Vec<u8>, Vec<Rectangle>) {
    let mut element = toolbar::tool_bar(None, settings, functions_visible);
    let (tree, node, buttons) = settle(&mut element, renderer);
    let viewport = Rectangle::with_size(SIZE);
    renderer::Renderer::reset(renderer, viewport);
    element.as_widget().draw(
        &tree,
        renderer,
        theme,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &viewport,
    );
    (
        renderer.screenshot(Size::new(640, 36), 1.0, Color::TRANSPARENT),
        buttons,
    )
}

#[test]
fn view_buttons_show_their_current_settings_in_both_themes() {
    let _guard = crate::font_system_test_guard();
    let mut renderer = renderer();
    let settings = EditorSettings {
        word_wrap: false,
        decorations: DecorationSettings {
            show_spaces: false,
            show_tabs: false,
            show_end_of_line_markers: false,
            show_indentation_guides: false,
            ..DecorationSettings::default()
        },
        ..EditorSettings::default()
    };
    for theme in [Theme::Light, Theme::Dark] {
        let (off, buttons) = snapshot(&mut renderer, &settings, false, &theme);
        for (index, bounds) in buttons.iter().enumerate().skip(15) {
            let mut changed = settings.clone();
            match index {
                15 => changed.word_wrap = true,
                16 => {
                    changed.decorations.show_spaces = true;
                    changed.decorations.show_tabs = true;
                    changed.decorations.show_end_of_line_markers = true;
                }
                17 => changed.decorations.show_indentation_guides = true,
                18 => {}
                _ => unreachable!(),
            }
            let (on, _) = snapshot(&mut renderer, &changed, index == 18, &theme);
            assert!(
                (bounds.y as usize..(bounds.y + bounds.height) as usize).any(|y| {
                    let start = (y * 640 + bounds.x as usize) * 4;
                    let end = start + bounds.width as usize * 4;
                    off[start..end] != on[start..end]
                }),
                "toggle {index} must show a selected state in {theme:?}"
            );
        }
    }
}
