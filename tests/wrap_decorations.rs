use fragile_notepad::core::{Document, DocumentId, EditorSettings};
use fragile_notepad::editor::widget::EditorStyle;
use fragile_notepad::editor::{EditorMetrics, ViewportModel};
use fragile_notepad::ui::editor;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, layout, mouse};
use iced::{Rectangle, Renderer, Size, Theme};

fn software_renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("Headless software renderer")
}

fn metrics(document: &Document, settings: &EditorSettings) -> EditorMetrics {
    EditorMetrics {
        line_height: 20.0 * settings.zoom,
        character_width: 8.8 * settings.zoom,
        ..EditorMetrics::default()
    }
    .with_line_count(document.buffer.line_count())
}

fn draw(
    renderer: &mut Renderer,
    tree: &mut Tree,
    document: &Document,
    settings: &EditorSettings,
    theme: &Theme,
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
        mouse::Cursor::Unavailable,
        &bounds,
    );
    renderer.screenshot(size, 1.0, EditorStyle::from_theme(theme).surface)
}

fn changed_pixels(before: &[u8], after: &[u8], width: u32) -> Vec<(usize, usize)> {
    assert_eq!(before.len(), after.len());
    before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .map(|(index, _)| (index % width as usize, index / width as usize))
        .collect()
}

fn guide_difference(
    renderer: &mut Renderer,
    document: &Document,
    settings: &EditorSettings,
    theme: &Theme,
    size: Size<u32>,
) -> Vec<(usize, usize)> {
    let mut tree = Tree::empty();
    let mut without = document.clone();
    without.decorations.settings.show_wrap_guide = false;
    let enabled = draw(renderer, &mut tree, document, settings, theme, size);
    let disabled = draw(renderer, &mut tree, &without, settings, theme, size);
    changed_pixels(&disabled, &enabled, size.width)
}

fn assert_guide_at(changes: &[(usize, usize)], expected_x: f32, height: u32) {
    assert!(
        !changes.is_empty(),
        "The enabled ruler must produce visible pixels"
    );
    assert!(
        changes
            .iter()
            .all(|&(x, _)| (x as f32 - expected_x).abs() <= 1.0),
        "A column ruler must remain a narrow vertical line at x={expected_x}; actual {changes:?}"
    );
    // The first row has the active-line background. Checking it and the bottom
    // of the viewport catches a guide hidden by that background or row clipping.
    assert!(changes.iter().any(|&(_, y)| y == 10));
    assert!(changes.iter().any(|&(_, y)| y == height as usize - 1));
}

fn document(text: &str) -> Document {
    let mut document = Document::from_path(DocumentId::new(710), "wrap.txt", text);
    document.decorations.settings.show_indentation_guides = false;
    document
}

fn save_review(name: &str, pixels: &[u8], size: Size<u32>) {
    if std::env::var_os("FRAGILE_SAVE_WRAP_REVIEW").is_none() {
        return;
    }
    std::fs::create_dir_all("target/wrap-review").expect("Create wrap review directory");
    tiny_skia::Pixmap::from_vec(
        pixels.to_vec(),
        tiny_skia::IntSize::from_wh(size.width, size.height).unwrap(),
    )
    .expect("RGBA review pixels")
    .save_png(format!("target/wrap-review/{name}.png"))
    .expect("Write wrap review image");
}

#[test]
fn unwrapped_editor_shows_custom_and_default_column_rulers_at_every_zoom() {
    let mut renderer = software_renderer();
    for theme in [Theme::Light, Theme::Dark] {
        for zoom in [EditorSettings::MIN_ZOOM, 1.0, EditorSettings::MAX_ZOOM] {
            for show_numbers in [true, false] {
                for column in [Some(80), None] {
                    let settings = EditorSettings {
                        zoom,
                        word_wrap: false,
                        wrap_column_limit: column,
                        ..EditorSettings::default()
                    };
                    let mut document = document("\n\n");
                    document.decorations.settings.show_line_numbers = show_numbers;
                    assert_eq!(document.viewport.wrap_columns(), None);
                    let metrics = metrics(&document, &settings);
                    let x = metrics.text_origin_x(&document.decorations)
                        + column.unwrap_or(EditorSettings::DEFAULT_WRAP_COLUMN) as f32
                            * metrics.character_width;
                    let size = Size::new(x.ceil() as u32 + 40, 140);
                    let changes =
                        guide_difference(&mut renderer, &document, &settings, &theme, size);
                    assert_guide_at(&changes, x, size.height);
                }
            }
        }
    }
}

#[test]
fn unwrapped_ruler_scrolls_with_text_and_clips_outside_text_area() {
    let mut renderer = software_renderer();
    let settings = EditorSettings {
        word_wrap: false,
        wrap_column_limit: Some(80),
        ..EditorSettings::default()
    };
    // Enough logical rows for a scrollbar; the ruler must never cover its track.
    let mut document = document(&"\n".repeat(40));
    let metrics = metrics(&document, &settings);
    let origin = metrics.text_origin_x(&document.decorations);
    let unscrolled_x = origin + 80.0 * metrics.character_width;
    let size = Size::new(520, 160);
    for desired_x in [400.0, 312.0] {
        document.scroll.horizontal_px = unscrolled_x - desired_x;
        let changes = guide_difference(&mut renderer, &document, &settings, &Theme::Light, size);
        assert_guide_at(&changes, desired_x, size.height);
    }
    for desired_x in [origin - 10.0, 514.0, 530.0] {
        document.scroll.horizontal_px = unscrolled_x - desired_x;
        assert!(
            guide_difference(&mut renderer, &document, &settings, &Theme::Light, size).is_empty(),
            "Ruler at x={desired_x} must be clipped before the gutter and scrollbar"
        );
    }
}

#[test]
fn wrapped_ruler_follows_effective_width_and_ignores_horizontal_scroll() {
    let mut renderer = software_renderer();
    for zoom in [1.0, 2.0] {
        let settings = EditorSettings {
            zoom,
            word_wrap: true,
            wrap_column_limit: Some(100),
            ..EditorSettings::default()
        };
        let mut document = document(&"word ".repeat(120));
        document.decorations.settings.show_wrap_indicator = false;
        document.set_wrap_column_limit(settings.wrap_column_limit);
        document.set_word_wrap(true);
        let metrics = metrics(&document, &settings);
        document.update_viewport_geometry(8, 280.0, metrics.character_width);
        let columns = document.viewport.wrap_columns().unwrap();
        assert!(
            columns < 100,
            "Narrow windows must reduce the effective wrap width"
        );
        let x =
            metrics.text_origin_x(&document.decorations) + columns as f32 * metrics.character_width;
        let size = Size::new(520, 160);
        let changes = guide_difference(&mut renderer, &document, &settings, &Theme::Light, size);
        assert_guide_at(&changes, x, size.height);
        document.scroll.horizontal_px = 123.0;
        assert_eq!(
            guide_difference(&mut renderer, &document, &settings, &Theme::Light, size),
            changes,
            "Wrapped content and its ruler must share the same fixed text origin"
        );
    }
}

#[test]
fn wrap_markers_are_muted_gutter_icons_only_on_continuation_rows() {
    let mut renderer = software_renderer();
    for (theme, name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        for zoom in [EditorSettings::MIN_ZOOM, 1.0, EditorSettings::MAX_ZOOM] {
            for show_numbers in [true, false] {
                let settings = EditorSettings {
                    zoom,
                    word_wrap: true,
                    ..EditorSettings::default()
                };
                let mut document = document(&format!("{}\nnext logical line", "word ".repeat(28)));
                document.set_word_wrap(true);
                document.decorations.settings.show_line_numbers = show_numbers;
                document.decorations.settings.show_folding_controls = show_numbers;
                document.decorations.settings.show_wrap_guide = false;
                document.viewport =
                    ViewportModel::new_wrapped(&document.buffer, &document.folds, 20, 4);
                let metrics = metrics(&document, &settings);
                let size = Size::new(520, 360);
                let mut tree = Tree::empty();
                let enabled = draw(&mut renderer, &mut tree, &document, &settings, &theme, size);
                document.decorations.settings.show_wrap_indicator = false;
                let disabled = draw(&mut renderer, &mut tree, &document, &settings, &theme, size);
                let changes = changed_pixels(&disabled, &enabled, size.width);
                assert!(
                    !changes.is_empty(),
                    "Wrapped rows must show continuation markers"
                );
                let origin = metrics.text_origin_x(&document.decorations);
                assert!(
                    changes.iter().all(|&(x, y)| {
                        if x as f32 >= origin || (y as f32) < metrics.padding_top {
                            return false;
                        }
                        let row = ((y as f32 - metrics.padding_top) / metrics.line_height) as usize;
                        document
                            .viewport
                            .row_segment(row, &document.buffer)
                            .is_some_and(|segment| segment.start_column > 0)
                    }),
                    "Markers must stay in the gutter and leave logical first rows unchanged"
                );
                let mut checked_rows = 0;
                for row in 0..document.viewport.visible_row_count() {
                    let segment = document
                        .viewport
                        .row_segment(row, &document.buffer)
                        .unwrap();
                    let top = metrics.padding_top + row as f32 * metrics.line_height;
                    if top + metrics.line_height > size.height as f32 {
                        break;
                    }
                    let row_changes: Vec<_> = changes
                        .iter()
                        .filter(|&&(_, y)| {
                            y as f32 >= top && (y as f32) < top + metrics.line_height
                        })
                        .collect();
                    if segment.start_column == 0 {
                        assert!(row_changes.is_empty());
                    } else {
                        assert!(
                            !row_changes.is_empty(),
                            "Continuation row {row} needs a marker"
                        );
                        let min_x = row_changes.iter().map(|&&(x, _)| x).min().unwrap();
                        let max_x = row_changes.iter().map(|&&(x, _)| x).max().unwrap();
                        assert!((max_x - min_x + 1) as f32 <= metrics.character_width * 2.0);
                        checked_rows += 1;
                    }
                }
                assert!(checked_rows >= 2, "Exercise several continuation rows");
                for &(x, y) in &changes {
                    let rgb = &enabled[(y * size.width as usize + x) * 4..][..3];
                    let spread = rgb.iter().max().unwrap() - rgb.iter().min().unwrap();
                    assert!(
                        spread <= 16,
                        "Marker pixel {rgb:?} must use the muted theme tint"
                    );
                }
                if zoom == 1.0
                    && show_numbers
                    && std::env::var_os("FRAGILE_SAVE_WRAP_REVIEW").is_some()
                {
                    document.decorations.settings.show_wrap_indicator = true;
                    document.decorations.settings.show_wrap_guide = true;
                    let review = draw(&mut renderer, &mut tree, &document, &settings, &theme, size);
                    save_review(&format!("wrap-{name}"), &review, size);
                }
            }
        }
    }
}

#[test]
fn continuation_marker_toggle_has_no_effect_on_unwrapped_lines() {
    let mut renderer = software_renderer();
    let settings = EditorSettings {
        word_wrap: false,
        wrap_column_limit: Some(40),
        ..EditorSettings::default()
    };
    let mut document = document(&format!("{}\nshort line", "unwrapped text ".repeat(12)));
    let size = Size::new(720, 200);
    let mut tree = Tree::empty();
    let enabled = draw(
        &mut renderer,
        &mut tree,
        &document,
        &settings,
        &Theme::Light,
        size,
    );
    document.decorations.settings.show_wrap_indicator = false;
    let disabled = draw(
        &mut renderer,
        &mut tree,
        &document,
        &settings,
        &Theme::Light,
        size,
    );
    assert_eq!(
        enabled, disabled,
        "Logical lines alone must never receive continuation icons"
    );
    save_review("no-wrap-light", &enabled, size);
}
