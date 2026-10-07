use fragile_notepad::core::{Document, DocumentId, EditorSettings};
use fragile_notepad::editor::EditorMetrics;
use fragile_notepad::editor::widget::EditorStyle;
use fragile_notepad::ui::editor;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, layout, mouse};
use iced::{Point, Rectangle, Renderer, Size, Theme};

fn software_renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("Headless software renderer")
}

fn draw(
    renderer: &mut Renderer,
    tree: &mut Tree,
    document: &Document,
    settings: &EditorSettings,
    theme: &Theme,
    cursor: mouse::Cursor,
    size: Size<u32>,
) -> Vec<u8> {
    let logical_size = Size::new(size.width as f32, size.height as f32);
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
    renderer.screenshot(size, 1.0, EditorStyle::from_theme(theme).surface)
}

fn changed_pixels(before: &[u8], after: &[u8], width: u32) -> Vec<(usize, usize)> {
    before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .map(|(index, _)| (index % width as usize, index / width as usize))
        .collect()
}

fn assert_plain_gutter(pixels: &[u8], size: Size<u32>, left: f32, right: f32, row: (f32, f32)) {
    let background = &pixels[..4];
    for y in row.0.ceil() as usize..row.1.floor() as usize {
        for x in left.ceil() as usize..right.floor() as usize {
            let offset = (y * size.width as usize + x) * 4;
            assert_eq!(
                &pixels[offset..offset + 4],
                background,
                "Gutter at ({x}, {y})"
            );
        }
    }
}

fn save_review(name: &str, pixels: &[u8], size: Size<u32>) {
    if std::env::var_os("FRAGILE_SAVE_FOLD_REVIEW").is_none() {
        return;
    }
    std::fs::create_dir_all("target/fold-review").expect("Create fold review directory");
    tiny_skia::Pixmap::from_vec(
        pixels.to_vec(),
        tiny_skia::IntSize::from_wh(size.width, size.height).unwrap(),
    )
    .expect("RGBA review pixels")
    .save_png(format!("target/fold-review/{name}.png"))
    .expect("Write fold review image");
}

#[test]
fn expanded_folds_reveal_on_gutter_hover_and_collapsed_folds_remain_visible() {
    let mut renderer = software_renderer();
    let size = Size::new(600, 280);
    for (theme, name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        for zoom in [EditorSettings::MIN_ZOOM, 1.0, EditorSettings::MAX_ZOOM] {
            for show_numbers in [true, false] {
                let settings = EditorSettings {
                    zoom,
                    ..EditorSettings::default()
                };
                let mut document = Document::from_path(
                    DocumentId::new(720),
                    "fold.rs",
                    "fn main() {\n    run();\n}\nafter();",
                );
                document.decorations.settings.show_line_numbers = show_numbers;
                document.decorations.settings.show_indentation_guides = false;
                document.decorations.settings.show_wrap_guide = false;
                document.restore_collapsed_folds(&[]);
                let metrics = EditorMetrics {
                    line_height: 20.0 * zoom,
                    character_width: 8.8 * zoom,
                    ..EditorMetrics::default()
                }
                .with_line_count(document.buffer.line_count());
                let fold_left = metrics.padding_left
                    + if show_numbers {
                        metrics.line_number_width
                    } else {
                        0.0
                    };
                let fold_right = fold_left + metrics.fold_lane_width;
                let row = (
                    metrics.padding_top,
                    metrics.padding_top + metrics.line_height,
                );
                let gutter_cursor = mouse::Cursor::Available(Point::new(
                    fold_left + metrics.fold_lane_width / 2.0,
                    metrics.padding_top + metrics.line_height / 2.0,
                ));
                let text_cursor = mouse::Cursor::Available(Point::new(
                    metrics.text_origin_x(&document.decorations) + 4.0,
                    metrics.padding_top + metrics.line_height / 2.0,
                ));
                let mut tree = Tree::empty();
                let mut render = |document: &Document, cursor| {
                    draw(
                        &mut renderer,
                        &mut tree,
                        document,
                        &settings,
                        &theme,
                        cursor,
                        size,
                    )
                };
                let at_rest = render(&document, mouse::Cursor::Unavailable);
                assert_plain_gutter(&at_rest, size, fold_left, fold_right, row);
                let over_text = render(&document, text_cursor);
                assert!(
                    at_rest == over_text,
                    "Text hover must keep expanded controls hidden"
                );
                let hovered = render(&document, gutter_cursor);
                let changes = changed_pixels(&at_rest, &hovered, size.width);
                assert!(
                    !changes.is_empty(),
                    "Gutter hover must reveal the down chevron"
                );
                assert!(
                    changes.iter().all(|&(x, y)| {
                        x as f32 >= fold_left
                            && (x as f32) < fold_right
                            && y as f32 >= row.0
                            && (y as f32) < row.1
                    }),
                    "Hover should change only the fold control lane: {changes:?}"
                );
                let left_gutter = render(&document, mouse::Cursor::Unavailable);
                assert!(
                    at_rest == left_gutter,
                    "Leaving the gutter must hide expanded controls"
                );

                document.restore_collapsed_folds(&[(0, 2)]);
                let collapsed = render(&document, mouse::Cursor::Unavailable);
                let collapsed_hovered = render(&document, gutter_cursor);
                assert!(
                    collapsed == collapsed_hovered,
                    "Collapsed chevrons must remain visible without hovering: {:?}; folds {:?}",
                    changed_pixels(&collapsed, &collapsed_hovered, size.width)
                        .into_iter()
                        .take(30)
                        .collect::<Vec<_>>(),
                    document.folds.ranges(),
                );
                assert!(
                    changed_pixels(&at_rest, &collapsed, size.width)
                        .iter()
                        .any(|&(x, y)| {
                            x as f32 >= fold_left
                                && (x as f32) < fold_right
                                && y as f32 >= row.0
                                && (y as f32) < row.1
                        }),
                    "Collapsed folds must have a visible right chevron"
                );
                assert_plain_gutter(
                    &collapsed,
                    size,
                    fold_right,
                    metrics.text_origin_x(&document.decorations),
                    row,
                );

                if zoom == 1.0 && show_numbers {
                    save_review(&format!("{name}-expanded"), &at_rest, size);
                    save_review(&format!("{name}-hovered"), &hovered, size);
                    save_review(&format!("{name}-collapsed"), &collapsed, size);
                }
            }
        }
    }
}
