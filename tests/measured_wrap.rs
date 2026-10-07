use fragile_notepad::core::{
    Document, DocumentId, DocumentLoadGeneration, EditorSettings, TextEncoding,
};
use fragile_notepad::editor::layout::{visual_column_for, visual_width_with_tab_width};
use fragile_notepad::editor::widget::{
    EDITOR_FONT, EditorFontRun, EditorStyle, editor_font_runs_from_cjk_runs,
};
use fragile_notepad::editor::{
    EditTransaction, EditorAction, EditorMetrics, EditorPosition, EditorSelection,
};
use fragile_notepad::message::Message;
use fragile_notepad::ui::editor;
use iced::advanced::graphics;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::text::{self, Paragraph as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Event, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment};
use unicode_segmentation::UnicodeSegmentation;

const MIXED: &str = include_str!("fixtures/cjk/mixed.txt");

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

fn wrap_document(source: &str, zoom: f32, text_width: f32) -> (Document, EditorSettings) {
    let settings = EditorSettings {
        zoom,
        word_wrap: true,
        wrap_column_limit: Some(120),
        ..EditorSettings::default()
    };
    let mut document = Document::from_path(DocumentId::new(780), "measured.txt", source);
    document.decorations.settings.show_indentation_guides = false;
    document.set_wrap_column_limit(settings.wrap_column_limit);
    document.set_word_wrap(true);
    update_geometry(&mut document, &settings, text_width);
    (document, settings)
}

fn update_geometry(document: &mut Document, settings: &EditorSettings, text_width: f32) {
    document.update_viewport_geometry_with_typography(
        12,
        text_width,
        8.8 * settings.zoom,
        16.0 * settings.zoom,
        None,
    );
}

// This deliberately shapes an independent paragraph instead of asking the
// wrapping implementation for its measurements. Tabs retain the logical line's
// original stops, and regional fonts come from the public editor routing API.
fn paragraph(
    document: &Document,
    line: usize,
    start: usize,
    end: usize,
    zoom: f32,
) -> (graphics::text::Paragraph, Vec<(usize, usize)>) {
    let source = document.buffer.line(line).unwrap();
    let fragment = &source[start..end];
    let routes = document
        .cjk_context()
        .runs_for_fragment(line, start, fragment);
    let original_fonts = editor_font_runs_from_cjk_runs(fragment, &routes);
    let tab_width = document.decorations.settings.indent_width;
    let mut visual_column = visual_column_for(&source, start, tab_width);
    let mut expanded = String::new();
    let mut fonts: Vec<EditorFontRun> = Vec::new();
    let mut boundaries = Vec::new();
    let mut expanded_graphemes = 0;
    for (byte, grapheme) in fragment.grapheme_indices(true) {
        boundaries.push((start + byte, expanded_graphemes));
        let font = original_fonts
            .iter()
            .find(|run| run.byte_range.contains(&byte))
            .expect("Complete contextual font route")
            .font;
        let expanded_start = expanded.len();
        if grapheme == "\t" {
            let spaces = visual_width_with_tab_width('\t', visual_column, tab_width);
            expanded.extend(std::iter::repeat_n(' ', spaces));
            visual_column += spaces;
            expanded_graphemes += spaces;
        } else {
            expanded.push_str(grapheme);
            for ch in grapheme.chars() {
                visual_column += visual_width_with_tab_width(ch, visual_column, tab_width);
            }
            expanded_graphemes += 1;
        }
        if let Some(last) = fonts.last_mut()
            && last.font == font
        {
            last.byte_range.end = expanded.len();
        } else {
            fonts.push(EditorFontRun {
                byte_range: expanded_start..expanded.len(),
                font,
            });
        }
    }
    let spans: Vec<text::Span<'_, (), Font>> = fonts
        .iter()
        .map(|run| text::Span::new(&expanded[run.byte_range.clone()]).font(run.font))
        .collect();
    let paragraph = graphics::text::Paragraph::with_spans(text::Text {
        content: &spans,
        bounds: Size::new(f32::INFINITY, 20.0 * zoom),
        size: Pixels(16.0 * zoom),
        line_height: text::LineHeight::Absolute(Pixels(20.0 * zoom)),
        font: EDITOR_FONT,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
        ellipsis: text::Ellipsis::None,
        hint_factor: None,
    });
    (paragraph, boundaries)
}

fn draw(
    renderer: &mut Renderer,
    tree: &mut Tree,
    document: &Document,
    settings: &EditorSettings,
    theme: &Theme,
    size: Size<u32>,
) -> Vec<u8> {
    let logical = Size::new(size.width as f32, size.height as f32);
    let bounds = Rectangle::with_size(logical);
    let mut content = editor::view(document, settings);
    tree.diff(content.as_widget_mut());
    let node =
        content
            .as_widget_mut()
            .layout(tree, renderer, &layout::Limits::new(logical, logical));
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

fn save_review(name: &str, pixels: &[u8], size: Size<u32>) {
    if std::env::var_os("FRAGILE_SAVE_MEASURED_WRAP_REVIEW").is_none() {
        return;
    }
    std::fs::create_dir_all("target/measured-wrap-review").unwrap();
    tiny_skia::Pixmap::from_vec(
        pixels.to_vec(),
        tiny_skia::IntSize::from_wh(size.width, size.height).unwrap(),
    )
    .unwrap()
    .save_png(format!("target/measured-wrap-review/{name}.png"))
    .unwrap();
}

#[test]
fn screenshot_farewell_line_fits_120_column_guide_with_actual_fonts() {
    let source = MIXED.lines().find(|line| line.starts_with("最後")).unwrap();
    let (document, settings) = wrap_document(source, 1.5, 1_800.0);
    let measured = paragraph(&document, 0, 0, source.len(), settings.zoom).0;
    let guide_width = 120.0 * metrics(&document, &settings).character_width;
    assert!(visual_column_for(source, source.len(), 4) > 120);
    assert!(measured.min_bounds().width < guide_width);
    assert_eq!(document.viewport.visible_row_count(), 1);
    assert_eq!(document.text(), source);
    assert!(!document.can_undo());

    let mut legacy = document.clone();
    legacy.viewport =
        fragile_notepad::editor::ViewportModel::new_wrapped(&legacy.buffer, &legacy.folds, 120, 4);
    assert_eq!(
        legacy.viewport.visible_row_count(),
        2,
        "Exercise the original bug"
    );
    let mut unwrapped = document.clone();
    unwrapped.set_word_wrap(false);
    let unwrapped_settings = EditorSettings {
        word_wrap: false,
        ..settings.clone()
    };
    let mut renderer = software_renderer();
    let size = Size::new(1_800, 140);
    for (theme, name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let after = draw(
            &mut renderer,
            &mut Tree::empty(),
            &document,
            &settings,
            &theme,
            size,
        );
        let reference = draw(
            &mut renderer,
            &mut Tree::empty(),
            &unwrapped,
            &unwrapped_settings,
            &theme,
            size,
        );
        assert_eq!(
            after, reference,
            "A fitting line must render identically to its unwrapped reference"
        );
        save_review(&format!("farewell-after-{name}"), &after, size);
        let before = draw(
            &mut renderer,
            &mut Tree::empty(),
            &legacy,
            &settings,
            &theme,
            size,
        );
        save_review(&format!("farewell-before-{name}"), &before, size);
    }
}

fn assert_rows_fit(document: &Document, settings: &EditorSettings, maximal_cjk: bool) {
    let budget = document.viewport.wrap_columns().unwrap() as f32 * 8.8 * settings.zoom;
    let mut last_line = None;
    let mut previous_end = 0;
    for row in 0..document.viewport.visible_row_count() {
        let line = document.viewport.visible_row_to_document_line(row).unwrap();
        let source = document.buffer.line(line).unwrap();
        let segment = document
            .viewport
            .row_segment(row, &document.buffer)
            .unwrap();
        if last_line != Some(line) {
            previous_end = 0;
        }
        assert_eq!(
            segment.start_column, previous_end,
            "Every source byte belongs to exactly one row"
        );
        let boundaries: Vec<_> = source
            .grapheme_indices(true)
            .map(|(byte, _)| byte)
            .chain([source.len()])
            .collect();
        assert!(boundaries.contains(&segment.start_column));
        assert!(boundaries.contains(&segment.end_column));
        let width = paragraph(
            document,
            line,
            segment.start_column,
            segment.end_column,
            settings.zoom,
        )
        .0
        .min_bounds()
        .width;
        assert!(
            width <= budget + 0.05,
            "Row {row} exceeds the guide: measured {width}, budget {budget}"
        );
        if maximal_cjk && !segment.is_last {
            let next_end = boundaries
                .iter()
                .copied()
                .find(|&byte| byte > segment.end_column)
                .unwrap();
            let wider = paragraph(
                document,
                line,
                segment.start_column,
                next_end,
                settings.zoom,
            )
            .0
            .min_bounds()
            .width;
            assert!(
                wider > budget - 0.05,
                "Row {row} wraps before the next CJK grapheme would reach the guide: {wider} <= {budget}"
            );
        }
        if segment.is_last {
            assert_eq!(segment.end_column, source.len());
        }
        previous_end = segment.end_column;
        last_line = Some(line);
    }
}

#[test]
fn long_cjk_rows_fill_the_measured_width_and_mixed_tabs_preserve_graphemes() {
    let cjk = "漢字天地山川日月海雨風雲".repeat(15);
    for zoom in [1.0, 1.5, 2.0] {
        let (document, settings) = wrap_document(&cjk, zoom, 520.0);
        assert!(document.viewport.visible_row_count() > 3);
        assert_rows_fit(&document, &settings, true);
    }
    let source = "かな骨直令 한글 骨直令 漢字e\u{301}\t→≠世界\t한글 ".repeat(18);
    let (mut document, settings) = wrap_document(&source, 1.5, 430.0);
    let mut decorations = document.decorations.settings;
    decorations.indent_width = 8;
    document.set_decoration_settings(decorations);
    assert_rows_fit(&document, &settings, false);
    assert_eq!(document.text(), source);
}

fn click(
    renderer: &mut Renderer,
    document: &Document,
    settings: &EditorSettings,
    point: Point,
) -> Vec<EditorAction> {
    let size = Size::new(760.0, 480.0);
    let bounds = Rectangle::with_size(size);
    let mut content = editor::view(document, settings);
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());
    let node =
        content
            .as_widget_mut()
            .layout(&mut tree, renderer, &layout::Limits::new(size, size));
    renderer.reset(bounds);
    content.as_widget().draw(
        &tree,
        renderer,
        &Theme::Light,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &bounds,
    );
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    content.as_widget_mut().update(
        &mut tree,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Layout::new(&node),
        mouse::Cursor::Available(point),
        renderer,
        &mut shell,
        &bounds,
    );
    assert!(shell.is_event_captured());
    messages
        .into_iter()
        .filter_map(|message| match message {
            Message::EditorAction(_, action) => Some(action),
            _ => None,
        })
        .collect()
}

#[test]
fn measured_continuations_share_the_widget_caret_and_click_boundaries() {
    let source = "漢字天地山川日月海雨風雲".repeat(5);
    let (mut document, settings) = wrap_document(&source, 1.5, 330.0);
    let metrics = metrics(&document, &settings);
    let mut renderer = software_renderer();
    for row in 0..document.viewport.visible_row_count().min(3) {
        let segment = document
            .viewport
            .row_segment(row, &document.buffer)
            .unwrap();
        let (shaped, boundaries) = paragraph(
            &document,
            0,
            segment.start_column,
            segment.end_column,
            settings.zoom,
        );
        for &(byte, grapheme) in boundaries.iter().step_by(4) {
            let x = shaped.grapheme_position(0, grapheme).unwrap().x;
            let point = Point::new(
                metrics.text_origin_x(&document.decorations) + x + 0.2,
                metrics.padding_top + row as f32 * metrics.line_height + metrics.line_height * 0.5,
            );
            let expected = EditorAction::PlaceCaretOnRow {
                position: EditorPosition::new(0, byte),
                row,
            };
            assert!(
                click(&mut renderer, &document, &settings, point).contains(&expected),
                "Measured boundary {byte} must place the caret on row {row}"
            );
        }
        let end_point = Point::new(
            metrics.text_origin_x(&document.decorations) + shaped.min_bounds().width + 0.5,
            metrics.padding_top + row as f32 * metrics.line_height + metrics.line_height * 0.5,
        );
        assert!(
            click(&mut renderer, &document, &settings, end_point).contains(
                &EditorAction::PlaceCaretOnRow {
                    position: EditorPosition::new(0, segment.end_column),
                    row
                }
            )
        );
    }
    let end = document
        .viewport
        .row_segment(0, &document.buffer)
        .unwrap()
        .end_column;
    let position = EditorPosition::new(0, end);
    document.set_main_selection(EditorSelection::new(position, position));
    assert_eq!(document.caret_visible_row(), Some(1));
    document.set_caret_row_affinity(position, 0);
    assert_eq!(document.caret_visible_row(), Some(0));
}

fn row_ranges(document: &Document) -> Vec<(usize, usize, usize)> {
    (0..document.viewport.visible_row_count())
        .map(|row| {
            let segment = document
                .viewport
                .row_segment(row, &document.buffer)
                .unwrap();
            (
                document.viewport.visible_row_to_document_line(row).unwrap(),
                segment.start_column,
                segment.end_column,
            )
        })
        .collect()
}

#[test]
fn measured_reflow_survives_edit_undo_zoom_and_streamed_appends() {
    let source = "漢字天地山川日月海雨風雲".repeat(8);
    let (mut document, settings) = wrap_document(&source, 1.5, 430.0);
    let original_rows = row_ranges(&document);
    document.defer_analysis = true;
    let position = EditorPosition::new(0, source.len());
    let before = EditorSelection::new(position, position);
    let delta = document.buffer.replace_range(before.range(), &source);
    let position = EditorPosition::new(0, source.len() * 2);
    let after = EditorSelection::new(position, position);
    document.set_main_selection(after);
    document.history.record(EditTransaction {
        delta,
        before_selection: before,
        after_selection: after,
    });
    document.refresh_text_from(0);
    assert!(document.viewport.visible_row_count() > original_rows.len());
    assert_rows_fit(&document, &settings, true);
    assert!(document.undo());
    assert_eq!(row_ranges(&document), original_rows);
    assert!(document.redo());
    assert_rows_fit(&document, &settings, true);
    let before_zoom = document.viewport.visible_row_count();
    let zoomed = EditorSettings {
        zoom: 2.0,
        ..settings.clone()
    };
    update_geometry(&mut document, &zoomed, 430.0);
    assert!(document.viewport.visible_row_count() > before_zoom);
    assert_rows_fit(&document, &zoomed, true);

    let generation = DocumentLoadGeneration::next();
    let mut loading = Document::loading(DocumentId::new(781), "streamed.txt", generation);
    loading.defer_analysis = true;
    loading.set_wrap_column_limit(settings.wrap_column_limit);
    loading.set_word_wrap(true);
    update_geometry(&mut loading, &settings, 430.0);
    let split = source.char_indices().nth(37).unwrap().0;
    assert!(loading.replace_loading_preview(
        generation,
        &source[..split],
        true,
        split as u64,
        Some(source.len() as u64)
    ));
    assert!(loading.replace_loading_preview(
        generation,
        &source[split..],
        false,
        source.len() as u64,
        Some(source.len() as u64)
    ));
    assert_eq!(
        row_ranges(&loading),
        original_rows,
        "Streaming must remeasure an appended CJK line"
    );
    assert!(loading.complete_streaming_load(generation, TextEncoding::Utf8));
    assert_eq!(row_ranges(&loading), original_rows);
    assert_rows_fit(&loading, &settings, true);
    assert!(!loading.can_undo());
}
