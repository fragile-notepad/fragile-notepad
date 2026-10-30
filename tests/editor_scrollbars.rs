use fragile_notepad::core::{Document, DocumentId, EditorSettings};
use fragile_notepad::editor::EditorAction;
use fragile_notepad::editor::widget::EditorStyle;
use fragile_notepad::message::Message;
use fragile_notepad::ui::editor;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Event, Point, Rectangle, Renderer, Size, Theme};

const SIZE: Size<u32> = Size {
    width: 640,
    height: 360,
};

fn software_renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("Headless software renderer")
}

fn document_and_settings() -> (Document, EditorSettings) {
    let mut settings = EditorSettings {
        word_wrap: false,
        ..EditorSettings::default()
    };
    settings.decorations.show_indentation_guides = false;
    settings.decorations.show_wrap_guide = false;
    let text = (0..60)
        .map(|line| format!("{line:02}: {}", "abcdefghijklmnopqrst".repeat(6)))
        .collect::<Vec<_>>()
        .join("\n");
    let mut document = Document::from_path(DocumentId::new(780), "scrollbars.txt", &text);
    document.set_decoration_settings(settings.decoration_settings());
    (document, settings)
}

fn draw(
    renderer: &mut Renderer,
    tree: &mut Tree,
    document: &Document,
    settings: &EditorSettings,
    theme: &Theme,
    cursor: mouse::Cursor,
) -> Vec<u8> {
    let logical_size = Size::new(SIZE.width as f32, SIZE.height as f32);
    let bounds = Rectangle::with_size(logical_size);
    let mut content = editor::view(document, settings);
    tree.diff(content.as_widget_mut());
    let node = content.as_widget_mut().layout(
        tree,
        renderer,
        &layout::Limits::new(logical_size, logical_size),
    );
    renderer.reset(bounds);
    content.as_widget().draw(
        tree,
        renderer,
        theme,
        &renderer::Style::default(),
        Layout::new(&node),
        cursor,
        &bounds,
    );
    renderer.screenshot(SIZE, 1.0, EditorStyle::from_theme(theme).surface)
}

fn dispatch(
    renderer: &Renderer,
    tree: &mut Tree,
    document: &Document,
    settings: &EditorSettings,
    event: Event,
    point: Point,
) -> Vec<Message> {
    let logical_size = Size::new(SIZE.width as f32, SIZE.height as f32);
    let bounds = Rectangle::with_size(logical_size);
    let mut content = editor::view(document, settings);
    tree.diff(content.as_widget_mut());
    let node = content.as_widget_mut().layout(
        tree,
        renderer,
        &layout::Limits::new(logical_size, logical_size),
    );
    let mut messages = Vec::new();
    content.as_widget_mut().update(
        tree,
        &event,
        Layout::new(&node),
        mouse::Cursor::Available(point),
        renderer,
        &mut Shell::new(&iced::window::Headless, Waker::noop(), &mut messages),
        &bounds,
    );
    messages
}

fn assert_changes_within(before: &[u8], after: &[u8], area: Rectangle) {
    let changes = before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .map(|(index, _)| {
            Point::new(
                (index % SIZE.width as usize) as f32 + 0.5,
                (index / SIZE.width as usize) as f32 + 0.5,
            )
        })
        .collect::<Vec<_>>();
    assert!(!changes.is_empty(), "Hover must change the scrollbar paint");
    assert!(
        changes.iter().all(|&point| area.contains(point)),
        "Scrollbar hover changed pixels outside {area:?}: {:?}",
        changes.iter().find(|&&point| !area.contains(point)),
    );
}

fn save_review(name: &str, pixels: &[u8]) {
    if std::env::var_os("FRAGILE_SAVE_SCROLL_REVIEW").is_none() {
        return;
    }
    std::fs::create_dir_all("target/scrollbar-review").expect("Create scrollbar review directory");
    tiny_skia::Pixmap::from_vec(
        pixels.to_vec(),
        tiny_skia::IntSize::from_wh(SIZE.width, SIZE.height).unwrap(),
    )
    .expect("RGBA review pixels")
    .save_png(format!("target/scrollbar-review/{name}.png"))
    .expect("Write scrollbar review image");
}

#[test]
fn hovering_each_scrollbar_changes_only_its_own_pixels_in_both_themes() {
    let mut renderer = software_renderer();
    let (document, settings) = document_and_settings();
    let text_origin = editor::metrics(settings.zoom)
        .with_line_count(document.buffer.line_count())
        .text_origin_x(&document.decorations);
    for (name, theme) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let mut tree = Tree::empty();
        let idle = draw(
            &mut renderer,
            &mut tree,
            &document,
            &settings,
            &theme,
            mouse::Cursor::Unavailable,
        );
        save_review(&format!("{name}-idle"), &idle);
        let vertical = Point::new(SIZE.width as f32 - 6.0, 30.0);
        dispatch(
            &renderer,
            &mut tree,
            &document,
            &settings,
            Event::Mouse(mouse::Event::CursorMoved { position: vertical }),
            vertical,
        );
        let hovered = draw(
            &mut renderer,
            &mut tree,
            &document,
            &settings,
            &theme,
            mouse::Cursor::Available(vertical),
        );
        assert_changes_within(
            &idle,
            &hovered,
            Rectangle {
                x: SIZE.width as f32 - 12.0,
                y: 0.0,
                width: 12.0,
                height: SIZE.height as f32 - 14.0,
            },
        );
        save_review(&format!("{name}-hover-vertical"), &hovered);

        let horizontal = Point::new(text_origin + 30.0, SIZE.height as f32 - 7.0);
        dispatch(
            &renderer,
            &mut tree,
            &document,
            &settings,
            Event::Mouse(mouse::Event::CursorMoved {
                position: horizontal,
            }),
            horizontal,
        );
        let hovered = draw(
            &mut renderer,
            &mut tree,
            &document,
            &settings,
            &theme,
            mouse::Cursor::Available(horizontal),
        );
        assert_changes_within(
            &idle,
            &hovered,
            Rectangle {
                x: text_origin,
                y: SIZE.height as f32 - 14.0,
                width: SIZE.width as f32 - text_origin - 14.0,
                height: 14.0,
            },
        );
        save_review(&format!("{name}-hover-horizontal"), &hovered);
    }
}

#[test]
fn thumbs_can_be_dragged_from_the_hit_area_outside_the_thin_idle_paint() {
    let mut renderer = software_renderer();
    let (document, settings) = document_and_settings();
    let text_origin = editor::metrics(settings.zoom)
        .with_line_count(document.buffer.line_count())
        .text_origin_x(&document.decorations);
    for (name, theme) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        for (axis, press, destination) in [
            (
                "vertical",
                Point::new(SIZE.width as f32 - 9.0, 30.0),
                Point::new(SIZE.width as f32 - 9.0, SIZE.height as f32 - 30.0),
            ),
            (
                "horizontal",
                Point::new(text_origin + 20.0, SIZE.height as f32 - 9.5),
                Point::new(SIZE.width as f32 - 30.0, SIZE.height as f32 - 9.5),
            ),
        ] {
            let mut tree = Tree::empty();
            let idle = draw(
                &mut renderer,
                &mut tree,
                &document,
                &settings,
                &theme,
                mouse::Cursor::Unavailable,
            );
            let offset =
                (press.y.floor() as usize * SIZE.width as usize + press.x.floor() as usize) * 4;
            assert_eq!(
                &idle[offset..offset + 4],
                EditorStyle::from_theme(&theme).surface.into_rgba8(),
                "The grab point must lie outside the thin idle thumb"
            );
            let pressed = dispatch(
                &renderer,
                &mut tree,
                &document,
                &settings,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                press,
            );
            let dragged = dispatch(
                &renderer,
                &mut tree,
                &document,
                &settings,
                Event::Mouse(mouse::Event::CursorMoved {
                    position: destination,
                }),
                destination,
            );
            assert!(
                dragged.iter().any(|message| match message {
                    Message::EditorAction(_, EditorAction::ScrollToRow(row)) =>
                        axis == "vertical" && *row > 0,
                    Message::EditorAction(_, EditorAction::ScrollToHorizontal { offset_milli }) =>
                        axis == "horizontal" && *offset_milli > 0,
                    _ => false,
                }),
                "Dragging the {axis} thumb must scroll"
            );
            assert!(
                !pressed.iter().chain(&dragged).any(|message| matches!(
                    message,
                    Message::EditorAction(
                        _,
                        EditorAction::PlaceCaret(_) | EditorAction::PlaceCaretOnRow { .. }
                    )
                )),
                "Dragging a scrollbar must preserve the caret"
            );
            let pixels = draw(
                &mut renderer,
                &mut tree,
                &document,
                &settings,
                &theme,
                mouse::Cursor::Available(destination),
            );
            save_review(&format!("{name}-drag-{axis}"), &pixels);
            dispatch(
                &renderer,
                &mut tree,
                &document,
                &settings,
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                destination,
            );
        }
    }
}
