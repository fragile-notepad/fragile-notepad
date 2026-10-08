use super::super::render::SyntaxRenderSpan;
use super::*;
use crate::editor::action::EditorAction;
use crate::editor::{DecorationSettings, EditorPosition, EditorSelection, FoldModel};
use iced::advanced::graphics::core::shell::Waker;
use std::cell::Cell;

struct FoldPointerFixture {
    buffer: EditorBuffer,
    viewport: ViewportModel,
    decorations: DecorationModel,
    syntax_cache: RefCell<SyntaxLineCache>,
}

impl FoldPointerFixture {
    fn new(text: &str, folds: FoldModel) -> Self {
        let buffer = EditorBuffer::from_text(text);
        let viewport = ViewportModel::new(buffer.line_count(), &folds);
        let decorations = DecorationModel::from_folds(
            DecorationSettings::default(),
            buffer.line_count(),
            &folds,
            vec![],
        );

        Self {
            buffer,
            viewport,
            decorations,
            syntax_cache: RefCell::new(SyntaxLineCache::default()),
        }
    }

    fn editor(&self) -> AdvancedEditor<'_, EditorAction> {
        AdvancedEditor::new(
            &self.buffer,
            &self.viewport,
            &self.decorations,
            &self.syntax_cache,
            highlighter::Settings {
                token: "txt".to_owned(),
                theme: highlighter::Theme::InspiredGitHub,
            },
            EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 1)),
            std::convert::identity,
        )
    }
}

fn fold_test_node() -> layout::Node {
    layout::Node::new(Size::new(300.0, 94.0)).move_to(Point::new(10.0, 20.0))
}

fn first_fold_indicator_center(
    editor: &AdvancedEditor<'_, EditorAction>,
    node: &layout::Node,
) -> Point {
    let line = editor.buffer.line(0).expect("header");
    let editor_layout = editor.editor_layout(node.bounds());
    let end = super::super::layout::caret_x(&line, line.len(), editor_layout, editor.decorations);
    let indicator = collapsed_fold_indicator_bounds(
        editor.metrics,
        editor.metrics.padding_top,
        end,
        editor.decorations.settings.show_end_of_line_markers,
    );

    Point::new(
        node.bounds().x + indicator.center_x(),
        node.bounds().y + indicator.center_y(),
    )
}

fn press_fold_test_editor(
    editor: &mut AdvancedEditor<'_, EditorAction>,
    tree: &mut widget::Tree,
    node: &layout::Node,
    point: Point,
) -> (Vec<EditorAction>, bool) {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    Widget::<EditorAction, Theme, ()>::update(
        editor,
        tree,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Layout::new(node),
        mouse::Cursor::Available(point),
        &(),
        &mut shell,
        &node.bounds(),
    );
    let captured = shell.is_event_captured();

    (messages, captured)
}

#[test]
fn collapsed_indicator_click_expands_fold_without_changing_selection() {
    let range = FoldRange::new(0, 2);
    let mut folds = FoldModel::new(vec![range]);
    folds.set_collapsed(range, true);
    let fixture = FoldPointerFixture::new("{\n    child\n}\nafter", folds);
    let mut editor = fixture.editor();
    let original_selection = editor.selections.clone();
    let node = fold_test_node();
    let point = first_fold_indicator_center(&editor, &node);
    let mut tree = widget::Tree::new(&editor as &dyn Widget<EditorAction, Theme, ()>);
    let state = tree.state.downcast_mut::<AdvancedEditorState<()>>();
    state.drag_anchor = Some(EditorPosition::new(0, 0));
    state.drag_position = Some(point);
    state.drag_scroll_at = Some(Instant::now());
    state.scrollbar_grab_offset_y = Some(4.0);
    state.record_text_click(EditorPosition::new(0, 0), Instant::now());

    let (messages, captured) = press_fold_test_editor(&mut editor, &mut tree, &node, point);

    assert_eq!(
        messages,
        [EditorAction::Focus, EditorAction::ToggleFold(range)]
    );
    assert!(captured);
    assert_eq!(editor.selections, original_selection);
    let state = tree.state.downcast_ref::<AdvancedEditorState<()>>();
    assert!(state.is_focused);
    assert!(state.is_caret_visible());
    assert!(state.drag_anchor.is_none());
    assert!(state.drag_position.is_none());
    assert!(state.drag_scroll_at.is_none());
    assert!(state.scrollbar_grab_offset_y.is_none());
    assert!(state.last_text_click.is_none());
    assert_eq!(
        Widget::<EditorAction, Theme, ()>::mouse_interaction(
            &editor,
            &tree,
            Layout::new(&node),
            mouse::Cursor::Available(point),
            &node.bounds(),
            &(),
        ),
        mouse::Interaction::Pointer,
    );
}

#[test]
fn wrapped_fold_indicator_hits_only_the_final_header_fragment() {
    let range = FoldRange::new(0, 2);
    let mut folds = FoldModel::new(vec![range]);
    folds.set_collapsed(range, true);
    let mut fixture = FoldPointerFixture::new("abcdefghij\nchild\n}\nafter", folds.clone());
    fixture.viewport = ViewportModel::new_wrapped(&fixture.buffer, &folds, 8, 4);
    let editor = fixture.editor();
    let node = fold_test_node();
    let editor_layout = editor.editor_layout(node.bounds());
    let last_row = fixture
        .viewport
        .position_to_visible_row(EditorPosition::new(
            0,
            fixture.buffer.line(0).unwrap().len(),
        ))
        .unwrap();
    let fragment = fixture
        .viewport
        .row_segment(last_row, &fixture.buffer)
        .unwrap();
    let indicator = collapsed_fold_indicator_bounds(
        editor.metrics,
        row_y(last_row, editor_layout),
        editor.metrics.text_origin_x(editor.decorations)
            + (fragment.end_column - fragment.start_column) as f32 * editor.metrics.character_width,
        false,
    );
    let last_point = indicator.center() + iced::Vector::new(node.bounds().x, node.bounds().y);
    assert_eq!(
        editor.hit_collapsed_indicator(
            Layout::new(&node),
            mouse::Cursor::Available(last_point),
            &()
        ),
        Some(range)
    );
    let first_point = Point::new(
        last_point.x,
        last_point.y - last_row as f32 * editor.metrics.line_height,
    );
    assert_eq!(
        editor.hit_collapsed_indicator(
            Layout::new(&node),
            mouse::Cursor::Available(first_point),
            &()
        ),
        None
    );
    let fold_x = editor.metrics.padding_left + editor.metrics.line_number_width + 1.0;
    assert_eq!(
        hit_test(
            fold_x,
            row_y(1, editor_layout) + 1.0,
            editor_layout,
            editor.buffer,
            editor.viewport,
            editor.decorations
        ),
        HitTarget::GutterLine { line: 0 }
    );
}

#[test]
fn drawing_a_deep_uncached_viewport_never_runs_the_syntax_parser() {
    let fixture = FoldPointerFixture::new(&"<p>hello</p>\n".repeat(5000), FoldModel::default());
    let mut editor = fixture.editor();
    editor.syntax_settings.token = "html".into();
    editor.scroll.first_visible_row = 4000;
    let tree = widget::Tree::new(&editor as &dyn Widget<EditorAction, Theme, ()>);
    let node = fold_test_node();
    <AdvancedEditor<'_, EditorAction> as Widget<EditorAction, Theme, ()>>::draw(
        &editor,
        &tree,
        &mut (),
        &Theme::Light,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &node.bounds(),
    );
    assert_eq!(fixture.syntax_cache.borrow().cached_line_count(), 0);
}

#[test]
fn wrapped_fragments_render_distinct_geometry_with_software_renderer() {
    use iced::advanced::renderer::Headless;
    let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let buffer = EditorBuffer::from_text("éiii界WWWabcdefgh");
    let folds = FoldModel::default();
    let viewport = ViewportModel::new_wrapped(&buffer, &folds, 4, 4);
    let decorations = DecorationModel::from_folds(
        DecorationSettings {
            show_line_numbers: false,
            show_folding_controls: false,
            ..DecorationSettings::default()
        },
        buffer.line_count(),
        &folds,
        vec![],
    );
    let metrics = EditorMetrics::default();
    let bounds = Rectangle::with_size(Size::new(220.0, 140.0));
    let layout = EditorLayout::new(metrics, ScrollOffset::ZERO, bounds.width, bounds.height);
    let plan = build_render_plan_for_selection_set_with_cache_and_caret_row(
        &buffer,
        &viewport,
        &decorations,
        EditorSelection::new(
            EditorPosition::new(0, 0),
            EditorPosition::new(0, buffer.len_bytes()),
        )
        .into(),
        layout,
        &SyntaxLineCache::default(),
        None,
    );
    assert!(plan.rows.len() >= 4);
    let mut separated = plan.clone();
    for (index, row) in separated.rows.iter_mut().enumerate() {
        row.line = index;
        for selection in &mut separated.selections {
            if selection.y == row.y {
                selection.line = index;
            }
        }
    }
    let mut render = |plan: &super::super::render::RenderPlan| {
        renderer::Renderer::reset(&mut renderer, bounds);
        draw_plan(
            &mut renderer,
            bounds,
            layout,
            &decorations,
            plan,
            EditorStyle::from_theme(&Theme::Light),
            viewport.visible_row_count(),
            viewport.wrap_columns(),
            false,
            false,
            0.0,
            1,
            &mut RichParagraphCache::default(),
            &mut LineGeometryCache::default(),
            None,
        );
        renderer.screenshot(Size::new(220, 140), 1.0, Color::WHITE)
    };
    let wrapped_pixels = render(&plan);
    let separated_pixels = render(&separated);
    assert!(
        wrapped_pixels
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != [255, 255, 255])
    );
    assert_eq!(
        wrapped_pixels, separated_pixels,
        "wrapped fragments of one line must retain each row's measured geometry"
    );
}

#[test]
fn expanded_or_absent_fold_keeps_normal_text_click_behavior() {
    for folds in [
        FoldModel::default(),
        FoldModel::new(vec![FoldRange::new(0, 2)]),
    ] {
        let fixture = FoldPointerFixture::new("{\n    child\n}\nafter", folds);
        let mut editor = fixture.editor();
        let node = fold_test_node();
        let point = first_fold_indicator_center(&editor, &node);
        let mut tree = widget::Tree::new(&editor as &dyn Widget<EditorAction, Theme, ()>);

        let (messages, captured) = press_fold_test_editor(&mut editor, &mut tree, &node, point);

        assert!(captured);
        assert!(messages.contains(&EditorAction::PlaceCaret(EditorPosition::new(0, 1))));
        assert!(
            !messages
                .iter()
                .any(|action| matches!(action, EditorAction::ToggleFold(_)))
        );
        assert_eq!(
            Widget::<EditorAction, Theme, ()>::mouse_interaction(
                &editor,
                &tree,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &node.bounds(),
                &(),
            ),
            mouse::Interaction::Text,
        );
    }
}

#[test]
fn collapsed_indicator_hit_follows_scroll_and_excludes_gutter_and_scrollbar() {
    let range = FoldRange::new(0, 2);
    let mut folds = FoldModel::new(vec![range]);
    folds.set_collapsed(range, true);
    let fixture = FoldPointerFixture::new(
        "header\nchild\n}\nafter\nafter\nafter\nafter\nafter\nafter",
        folds,
    );
    let node = fold_test_node();
    let mut editor = fixture.editor();
    let original_point = first_fold_indicator_center(&editor, &node);
    editor.scroll.horizontal_px = 32.0;
    let scrolled_point = first_fold_indicator_center(&editor, &node);

    assert_eq!(original_point.x - scrolled_point.x, 32.0);
    assert_eq!(
        editor.hit_collapsed_indicator(
            Layout::new(&node),
            mouse::Cursor::Available(scrolled_point),
            &()
        ),
        Some(range),
    );
    assert_eq!(
        editor.hit_collapsed_indicator(
            Layout::new(&node),
            mouse::Cursor::Available(original_point),
            &()
        ),
        None,
    );

    editor.scroll.horizontal_px = 80.0;
    let hidden_in_gutter = first_fold_indicator_center(&editor, &node);
    assert!(
        hidden_in_gutter.x < node.bounds().x + editor.metrics.text_origin_x(editor.decorations)
    );
    assert_eq!(
        editor.hit_collapsed_indicator(
            Layout::new(&node),
            mouse::Cursor::Available(hidden_in_gutter),
            &()
        ),
        None,
    );

    editor.scroll.horizontal_px = 0.0;
    let narrow_node = layout::Node::new(Size::new(150.0, 94.0)).move_to(node.bounds().position());
    let over_scrollbar = first_fold_indicator_center(&editor, &narrow_node);
    assert!(over_scrollbar.x > narrow_node.bounds().x + 138.0);
    assert_eq!(
        editor.hit_collapsed_indicator(
            Layout::new(&narrow_node),
            mouse::Cursor::Available(over_scrollbar),
            &()
        ),
        None,
    );
}

#[test]
fn collapsed_indicator_uses_outer_span_when_folds_share_a_header() {
    let inner = FoldRange::new(0, 1);
    let outer = FoldRange::new(0, 2);
    let mut folds = FoldModel::new(vec![inner, outer]);
    folds.set_all_collapsed(true);
    for controls in [false, true] {
        let mut fixture = FoldPointerFixture::new("header\nchild\n}\nafter", folds.clone());
        fixture.decorations.settings.show_folding_controls = controls;
        let mut editor = fixture.editor();
        let node = fold_test_node();
        let point = first_fold_indicator_center(&editor, &node);

        assert_eq!(
            editor.hit_collapsed_indicator(
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &()
            ),
            Some(outer),
        );
        if controls {
            let mut tree = widget::Tree::new(&editor as &dyn Widget<EditorAction, Theme, ()>);
            let gutter = Point::new(
                node.bounds().x
                    + editor.metrics.padding_left
                    + editor.metrics.line_number_width
                    + 4.0,
                node.bounds().y + editor.metrics.padding_top + 4.0,
            );
            let (messages, captured) =
                press_fold_test_editor(&mut editor, &mut tree, &node, gutter);
            assert!(messages.ends_with(&[EditorAction::Focus, EditorAction::ToggleFold(outer)]));
            assert!(captured);
        }
    }
}

#[test]
fn fold_gutter_control_uses_pointer_cursor() {
    let fixture =
        FoldPointerFixture::new("{\nchild\n}", FoldModel::new(vec![FoldRange::new(0, 2)]));
    let editor = fixture.editor();
    let node = fold_test_node();
    let tree = widget::Tree::new(&editor as &dyn Widget<EditorAction, Theme, ()>);
    let point = Point::new(
        node.bounds().x + editor.metrics.padding_left + editor.metrics.line_number_width + 4.0,
        node.bounds().y + editor.metrics.padding_top + 4.0,
    );

    assert_eq!(
        Widget::<EditorAction, Theme, ()>::mouse_interaction(
            &editor,
            &tree,
            Layout::new(&node),
            mouse::Cursor::Available(point),
            &node.bounds(),
            &(),
        ),
        mouse::Interaction::Pointer,
    );
}

#[test]
fn matched_fold_placeholder_clicks_expand_from_the_opener_through_the_closer() {
    let source = "fn 字_long_function() {\n    run();\n}\nafter";
    let range = FoldRange::new(0, 2);
    for wrapped in [false, true] {
        let buffer = EditorBuffer::from_text(source);
        let mut folds =
            crate::editor::IndentBraceFoldProvider::for_syntax(4, "rs").compute_fold_model(&buffer);
        folds.set_collapsed(range, true);
        let mut fixture = FoldPointerFixture::new(source, folds.clone());
        if wrapped {
            fixture.viewport = ViewportModel::new_wrapped(&fixture.buffer, &folds, 12, 4);
        }
        let mut editor = fixture.editor();
        let node = layout::Node::new(Size::new(360.0, 180.0)).move_to(Point::new(10.0, 20.0));
        let editor_layout = editor.editor_layout(node.bounds());
        let plan = build_render_plan_for_selection_set_with_cache_and_caret_row(
            &fixture.buffer,
            &fixture.viewport,
            &fixture.decorations,
            editor.selections.clone(),
            editor_layout,
            &SyntaxLineCache::default(),
            None,
        );
        let row = plan
            .rows
            .iter()
            .find(|row| row.hidden_lines.is_some())
            .unwrap();
        let geometry = LineGeometry::new_with_font_runs(
            &row.text,
            editor.metrics,
            &(),
            row.start_visual_column,
            4,
            &font::editor_font_runs_for_fragment(&row.text, None, row.line, row.start_column),
        );
        let anchor_x = measured_caret_x(
            &geometry,
            row.collapsed_indicator_column(),
            editor_layout,
            &fixture.decorations,
        );
        let indicator = row
            .collapsed_indicator_bounds(editor.metrics, anchor_x)
            .unwrap();
        for x in [
            indicator.x + 0.5,
            indicator.center_x(),
            indicator.x + indicator.width - 0.5,
        ] {
            let point = Point::new(node.bounds().x + x, node.bounds().y + indicator.center_y());
            assert_eq!(
                editor.hit_collapsed_indicator(
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &()
                ),
                Some(range),
                "wrapped={wrapped}, point={point:?}, indicator={indicator:?}, row={row:?}"
            );
        }
        let mut tree = widget::Tree::new(&editor as &dyn Widget<EditorAction, Theme, ()>);
        let point = indicator.center() + iced::Vector::new(node.bounds().x, node.bounds().y);
        let (messages, captured) = press_fold_test_editor(&mut editor, &mut tree, &node, point);
        assert!(captured);
        assert_eq!(
            messages,
            [EditorAction::Focus, EditorAction::ToggleFold(range)]
        );
    }
}

#[test]
fn folding_gutter_hover_schedules_frames_until_fade_settles() {
    for enabled in [true, false] {
        let mut fixture =
            FoldPointerFixture::new("{\nchild\n}", FoldModel::new(vec![FoldRange::new(0, 2)]));
        fixture.decorations.settings.show_folding_controls = enabled;
        let mut editor = fixture.editor();
        let node = fold_test_node();
        let mut tree = widget::Tree::new(&editor as &dyn Widget<EditorAction, Theme, ()>);
        let gutter = Point::new(node.bounds().x + 4.0, node.bounds().y + 8.0);
        let fold_lane = Point::new(
            node.bounds().x + editor.metrics.padding_left + editor.metrics.line_number_width + 4.0,
            gutter.y,
        );
        let text = Point::new(
            node.bounds().x + editor.metrics.text_origin_x(editor.decorations) + 20.0,
            gutter.y,
        );
        for (cursor, changed) in [
            (mouse::Cursor::Available(text), false),
            (mouse::Cursor::Available(gutter), true),
            (mouse::Cursor::Available(fold_lane), false),
            (mouse::Cursor::Available(text), true),
            (mouse::Cursor::Available(gutter), true),
            (mouse::Cursor::Unavailable, true),
            (mouse::Cursor::Unavailable, false),
        ] {
            let event = cursor
                .position()
                .map_or(Event::Mouse(mouse::Event::CursorLeft), |position| {
                    Event::Mouse(mouse::Event::CursorMoved { position })
                });
            let mut messages = Vec::new();
            let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
            Widget::<EditorAction, Theme, ()>::update(
                &mut editor,
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &(),
                &mut shell,
                &node.bounds(),
            );
            let fading = tree
                .state
                .downcast_ref::<AdvancedEditorState<()>>()
                .fold_fade
                .get()
                .animating(Instant::now());
            let request = shell.redraw_request();
            if enabled && changed {
                assert_eq!(request, iced::window::RedrawRequest::NextFrame);
            } else if fading {
                assert!(matches!(request, iced::window::RedrawRequest::At(_)));
            } else {
                assert_eq!(request, iced::window::RedrawRequest::Wait);
            }
            assert!(!shell.is_event_captured());
        }
    }
}

#[test]
fn projected_suffix_text_and_each_placeholder_keep_their_source_targets() {
    let source = "if condition {\n    first();\n} else {\n    second();\n}; // tail";
    let first = FoldRange::new(0, 2);
    let second = FoldRange::new(2, 4);
    let buffer = EditorBuffer::from_text(source);
    let mut folds =
        crate::editor::IndentBraceFoldProvider::for_syntax(4, "rs").compute_fold_model(&buffer);
    folds.set_collapsed(first, true);
    folds.set_collapsed(second, true);
    for wrapped in [false, true] {
        let mut fixture = FoldPointerFixture::new(source, folds.clone());
        fixture.viewport = if wrapped {
            ViewportModel::new_wrapped(&fixture.buffer, &folds, 12, 4)
        } else {
            ViewportModel::new_with_buffer(&fixture.buffer, &folds, 4)
        };
        let mut editor = fixture.editor();
        let source_caret = EditorPosition::new(2, 4);
        editor.selections = EditorSelection::new(source_caret, source_caret).into();
        let node = layout::Node::new(Size::new(600.0, 240.0)).move_to(Point::new(10.0, 20.0));
        let editor_layout = editor.editor_layout(node.bounds());
        let plan = build_render_plan_for_selection_set_with_cache_and_caret_row(
            &fixture.buffer,
            &fixture.viewport,
            &fixture.decorations,
            editor.selections.clone(),
            editor_layout,
            &SyntaxLineCache::default(),
            None,
        );
        assert_eq!(plan.caret.unwrap().position, source_caret);
        let suffix_point = line_cache::measured_position_point_with_context(
            &fixture.buffer,
            &fixture.viewport,
            &fixture.decorations,
            editor_layout,
            source_caret,
            None,
            &(),
            None,
        );
        assert_eq!(
            line_cache::measured_text_hit_target_with_context(
                suffix_point,
                editor_layout,
                &fixture.buffer,
                &fixture.viewport,
                &fixture.decorations,
                &(),
                None,
            ),
            HitTarget::Text(source_caret),
        );
        let mut hit_ranges = Vec::new();
        for row in &plan.rows {
            let geometry = LineGeometry::new_with_font_runs(
                &row.text,
                editor.metrics,
                &(),
                row.start_visual_column,
                fixture.decorations.settings.indent_width,
                &font::editor_font_runs_for_row(row, None),
            );
            for fragment in &row.projection {
                if let ProjectionFragment::Placeholder {
                    display_range,
                    range,
                    ..
                } = fragment
                {
                    let left = measured_caret_x(
                        &geometry,
                        display_range.start,
                        editor_layout,
                        &fixture.decorations,
                    );
                    let right = measured_caret_x(
                        &geometry,
                        display_range.end,
                        editor_layout,
                        &fixture.decorations,
                    );
                    let point = Point::new(
                        node.bounds().x + (left + right) / 2.0,
                        node.bounds().y + row.y + editor.metrics.line_height / 2.0,
                    );
                    assert_eq!(
                        editor.hit_collapsed_indicator(
                            Layout::new(&node),
                            mouse::Cursor::Available(point),
                            &()
                        ),
                        Some(*range)
                    );
                    hit_ranges.push(*range);
                }
            }
        }
        assert!(hit_ranges.contains(&first));
        assert!(hit_ranges.contains(&second));
    }
}

#[test]
fn caret_after_a_collapsed_opener_renders_after_the_whole_placeholder() {
    use iced::advanced::renderer::Headless;
    let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    for source in [
        "fn main() {\n    run();\n}",
        "fn main() {                          \t\n    run();\n}",
    ] {
        let buffer = EditorBuffer::from_text(source);
        let mut folds =
            crate::editor::IndentBraceFoldProvider::for_syntax(4, "rs").compute_fold_model(&buffer);
        folds.set_collapsed(FoldRange::new(0, 2), true);
        let fixture = FoldPointerFixture::new(source, folds);
        let position = EditorPosition::new(0, buffer.line(0).unwrap().len());
        let size = Size::new(520, 180);
        let bounds = Rectangle::with_size(Size::new(size.width as f32, size.height as f32));
        for zoom in [0.5, 1.0, 3.0] {
            let metrics = EditorMetrics::new(20.0 * zoom, 8.8 * zoom);
            let layout =
                EditorLayout::new(metrics, ScrollOffset::ZERO, bounds.width, bounds.height);
            let plan = build_render_plan_for_selection_set_with_cache_and_caret_row(
                &fixture.buffer,
                &fixture.viewport,
                &fixture.decorations,
                EditorSelection::new(position, position).into(),
                layout,
                &SyntaxLineCache::default(),
                None,
            );
            let row = &plan.rows[0];
            let anchor = super::super::layout::caret_x(
                &row.text,
                row.collapsed_indicator_column(),
                layout,
                &fixture.decorations,
            );
            let indicator = row.collapsed_indicator_bounds(metrics, anchor).unwrap();
            let mut draw = |caret_visible| {
                renderer::Renderer::reset(&mut renderer, bounds);
                draw_plan(
                    &mut renderer,
                    bounds,
                    layout,
                    &fixture.decorations,
                    &plan,
                    EditorStyle::from_theme(&Theme::Light),
                    fixture.viewport.visible_row_count(),
                    None,
                    false,
                    caret_visible,
                    0.0,
                    1,
                    &mut RichParagraphCache::default(),
                    &mut LineGeometryCache::default(),
                    None,
                );
                renderer.screenshot(size, 1.0, Color::WHITE)
            };
            let hidden = draw(false);
            let visible = draw(true);
            let changes: Vec<_> = hidden
                .chunks_exact(4)
                .zip(visible.chunks_exact(4))
                .enumerate()
                .filter(|(_, (before, after))| before != after)
                .map(|(index, _)| index % size.width as usize)
                .collect();
            assert!(!changes.is_empty());
            assert!(
                changes.iter().all(|&x| {
                    x as f32 >= (indicator.x + indicator.width).floor()
                        && x as f32 <= (indicator.x + indicator.width).ceil() + 2.0
                }),
                "Caret must stay after the placeholder: zoom={zoom}, pixels={changes:?}"
            );
        }
    }
}

fn span_key(line: usize) -> Vec<SyntaxSpanKey> {
    vec![SyntaxSpanKey {
        start: 0,
        end: line.to_string().len(),
        color: Some(Color::from_rgb(0.8, 0.2, 0.1)),
    }]
}

#[test]
fn rich_paragraph_cache_reuses_page_rows_across_wheel_scroll_frames() {
    let mut cache = RichParagraphCache::default();
    let builds = Cell::new(0usize);
    let bounds = Size::new(360.0, 18.0);
    let size = Pixels(14.0);

    for frame in 1..=2 {
        let first_line = frame - 1;
        for line in first_line..first_line + 37 {
            let text = format!("line {line}");
            let syntax_spans = span_key(line);
            cache.get_or_insert_with(
                line,
                &text,
                &syntax_spans,
                0,
                bounds,
                size,
                18.0,
                None,
                frame as u64,
                || {
                    let build = builds.get() + 1;
                    builds.set(build);
                    build
                },
            );
        }
    }

    assert_eq!(
        builds.get(),
        38,
        "second scroll frame should reuse 36 of 37 shaped rows"
    );
    assert_eq!(
        cache.probe_count(),
        74,
        "cache lookup should be direct-mapped: one probe per visible row access"
    );
}

#[test]
fn caret_visibility_follows_blink_interval_and_focus() {
    let updated_at = Instant::now();

    assert!(caret_visible_at(true, true, updated_at, updated_at));
    assert!(!caret_visible_at(
        true,
        true,
        updated_at,
        updated_at + Duration::from_millis(CARET_BLINK_INTERVAL_MS as u64)
    ));
    assert!(caret_visible_at(
        true,
        true,
        updated_at,
        updated_at + Duration::from_millis((CARET_BLINK_INTERVAL_MS * 2) as u64)
    ));
    assert!(!caret_visible_at(false, true, updated_at, updated_at));
    assert!(!caret_visible_at(true, false, updated_at, updated_at));
}

#[derive(Debug, Default)]
struct TestParagraph {
    positions: Vec<f32>,
    min_width: f32,
}

impl text::Paragraph for TestParagraph {
    type Font = Font;

    fn with_text(_text: text::Text<&str, Self::Font>) -> Self {
        Self::default()
    }

    fn with_spans<Link>(
        _text: text::Text<&[text::Span<'_, Link, Self::Font>], Self::Font>,
    ) -> Self {
        Self::default()
    }

    fn resize(&mut self, _new_bounds: Size) {}

    fn compare(&self, _text: text::Text<(), Self::Font>) -> text::Difference {
        text::Difference::None
    }

    fn size(&self) -> Pixels {
        Pixels(16.0)
    }

    fn hint_factor(&self) -> Option<f32> {
        None
    }

    fn font(&self) -> Font {
        EDITOR_FONT
    }

    fn line_height(&self) -> text::LineHeight {
        text::LineHeight::default()
    }

    fn align_x(&self) -> text::Alignment {
        text::Alignment::Left
    }

    fn align_y(&self) -> alignment::Vertical {
        alignment::Vertical::Top
    }

    fn wrapping(&self) -> text::Wrapping {
        text::Wrapping::None
    }

    fn ellipsis(&self) -> text::Ellipsis {
        text::Ellipsis::None
    }

    fn shaping(&self) -> text::Shaping {
        EDITOR_TEXT_SHAPING
    }

    fn bounds(&self) -> Size {
        Size::new(f32::INFINITY, 18.0)
    }

    fn min_bounds(&self) -> Size {
        Size::new(self.min_width, 18.0)
    }

    fn hit_test(&self, _point: Point) -> Option<text::Hit> {
        None
    }

    fn hit_span(&self, _point: Point) -> Option<usize> {
        None
    }

    fn span_bounds(&self, _index: usize) -> Vec<Rectangle> {
        Vec::new()
    }

    fn grapheme_position(&self, _line: usize, index: usize) -> Option<Point> {
        self.positions.get(index).map(|x| Point::new(*x, 0.0))
    }
}

#[test]
fn measured_selection_bounds_use_unicode_glyph_advances() {
    let text = "a\u{6c49}b";
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let layout = EditorLayout::new(
        metrics,
        ScrollOffset {
            first_visible_row: 0,
            horizontal_px: 3.0,
        },
        400.0,
        200.0,
    );
    let decorations = DecorationModel::from_folds(
        super::super::decoration::DecorationSettings::default(),
        1,
        &super::super::fold::FoldModel::default(),
        vec![],
    );
    let selection = SelectionRenderPlan {
        line: 0,
        start_column: "a".len(),
        end_column: "a\u{6c49}".len(),
        start_visual_column: 1,
        end_visual_column: 2,
        start_virtual_column: None,
        end_virtual_column: None,
        y: 0.0,
        x: 999.0,
        width: 999.0,
    };
    let line_geometry = LineGeometry::Measured {
        text: text.to_owned(),
        paragraph: TestParagraph {
            positions: vec![0.0, 10.0, 27.0, 37.0],
            min_width: 37.0,
        },
        byte_to_grapheme: byte_to_grapheme_table(text),
        fallback_character_width: metrics.character_width,
        start_visual_column: 0,
        expanded_byte_offsets: None,
    };

    let (x, width) =
        measured_selection_x_and_width(&selection, &line_geometry, layout, &decorations);

    assert_eq!(x, scrolled_text_origin_x(layout, &decorations) + 10.0);
    assert_eq!(width, 17.0);
}

#[test]
fn wheel_line_delta_scrolls_a_little_faster_than_raw_delta() {
    assert_eq!(
        scroll_delta_lines(mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 }, 1.5),
        3.0
    );
}

#[test]
fn wheel_pixel_delta_keeps_fractional_scroll_accumulation() {
    assert_eq!(
        scroll_delta_lines(mouse::ScrollDelta::Pixels { x: 0.0, y: -8.0 }, 1.5),
        0.75
    );
}

#[test]
fn wheel_delta_uses_configured_scroll_speed() {
    assert_eq!(
        scroll_delta_lines(mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 }, 0.5),
        1.0
    );
}

#[test]
fn rich_text_visible_range_limits_long_ascii_lines_to_clip_columns() {
    let row = RowRenderPlan {
        visible_row: 0,
        line: 0,
        start_column: 0,
        start_visual_column: 0,
        y: 0.0,
        text_x: 0.0,
        text: "x".repeat(1_000),
        line_number: None,
        is_active_line: false,
        fold: None,
        hidden_lines: None,
        whitespace: Vec::new(),
        eol: None,
        indent_guides: Vec::new(),
        projection: Vec::new(),
        syntax_spans: Vec::new(),
    };
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let range = visible_rich_text_range(
        &row,
        0.0,
        metrics,
        Rectangle {
            x: 100.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
        },
    );

    assert_eq!(range, 2..23);
}

#[test]
fn rich_text_visible_range_backs_up_to_syntax_boundary() {
    let row = RowRenderPlan {
        visible_row: 0,
        line: 0,
        start_column: 0,
        start_visual_column: 0,
        y: 0.0,
        text_x: 0.0,
        text: "a".repeat(120),
        line_number: None,
        is_active_line: false,
        fold: None,
        hidden_lines: None,
        whitespace: Vec::new(),
        eol: None,
        indent_guides: Vec::new(),
        projection: Vec::new(),
        syntax_spans: vec![
            SyntaxRenderSpan {
                range: 0..16,
                color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
            },
            SyntaxRenderSpan {
                range: 16..80,
                color: Some(Color::from_rgb(0.0, 1.0, 0.0)),
            },
            SyntaxRenderSpan {
                range: 80..120,
                color: Some(Color::from_rgb(0.0, 0.0, 1.0)),
            },
        ],
    };
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let range = visible_styled_text_range(
        &row,
        0.0,
        metrics,
        Rectangle {
            x: 330.0,
            y: 0.0,
            width: 60.0,
            height: 20.0,
        },
    );

    assert_eq!(range, 16..47);
}

#[test]
fn rich_text_visible_range_subdivides_long_syntax_runs() {
    let row = RowRenderPlan {
        visible_row: 0,
        line: 0,
        start_column: 0,
        start_visual_column: 0,
        y: 0.0,
        text_x: 0.0,
        text: "a".repeat(1_000),
        line_number: None,
        is_active_line: false,
        fold: None,
        hidden_lines: None,
        whitespace: Vec::new(),
        eol: None,
        indent_guides: Vec::new(),
        projection: Vec::new(),
        syntax_spans: vec![SyntaxRenderSpan {
            range: 0..1_000,
            color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
        }],
    };
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };
    let range = visible_styled_text_range(
        &row,
        0.0,
        metrics,
        Rectangle {
            x: 3_330.0,
            y: 0.0,
            width: 60.0,
            height: 20.0,
        },
    );

    assert_eq!(range, 300..347);
}

#[test]
fn first_visible_syntax_span_skips_offscreen_spans() {
    let syntax_spans = (0..1_000)
        .map(|index| SyntaxRenderSpan {
            range: index * 8..index * 8 + 8,
            color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
        })
        .collect();
    let row = RowRenderPlan {
        visible_row: 0,
        line: 0,
        start_column: 0,
        start_visual_column: 0,
        y: 0.0,
        text_x: 0.0,
        text: "a".repeat(8_000),
        line_number: None,
        is_active_line: false,
        fold: None,
        hidden_lines: None,
        whitespace: Vec::new(),
        eol: None,
        indent_guides: Vec::new(),
        projection: Vec::new(),
        syntax_spans,
    };

    assert_eq!(first_visible_syntax_span(&row, 3_200), 400);
}

#[test]
fn rich_text_visible_range_clips_tabbed_lines_to_visible_columns() {
    let row = RowRenderPlan {
        visible_row: 0,
        line: 0,
        start_column: 0,
        start_visual_column: 0,
        y: 0.0,
        text_x: 0.0,
        text: format!("{}\t{}", "a".repeat(40), "b".repeat(40)),
        line_number: None,
        is_active_line: false,
        fold: None,
        hidden_lines: None,
        whitespace: Vec::new(),
        eol: None,
        indent_guides: Vec::new(),
        projection: Vec::new(),
        syntax_spans: Vec::new(),
    };
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };

    assert_eq!(
        visible_styled_text_range(
            &row,
            0.0,
            metrics,
            Rectangle {
                x: 100.0,
                y: 0.0,
                width: 50.0,
                height: 20.0,
            },
        ),
        2..23
    );
}

#[test]
fn rich_text_visible_range_clips_non_ascii_lines_on_utf8_boundaries() {
    let row = RowRenderPlan {
        visible_row: 0,
        line: 0,
        start_column: 0,
        start_visual_column: 0,
        y: 0.0,
        text_x: 0.0,
        text: format!("{}婵{}", "a".repeat(40), "b".repeat(40)),
        line_number: None,
        is_active_line: false,
        fold: None,
        hidden_lines: None,
        whitespace: Vec::new(),
        eol: None,
        indent_guides: Vec::new(),
        projection: Vec::new(),
        syntax_spans: Vec::new(),
    };
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };

    assert_eq!(
        visible_styled_text_range(
            &row,
            0.0,
            metrics,
            Rectangle {
                x: 100.0,
                y: 0.0,
                width: 50.0,
                height: 20.0,
            },
        ),
        2..23
    );
}

#[test]
fn rich_text_visible_range_falls_back_for_invalid_syntax_spans() {
    let row = RowRenderPlan {
        visible_row: 0,
        line: 0,
        start_column: 0,
        start_visual_column: 0,
        y: 0.0,
        text_x: 0.0,
        text: "a".repeat(120),
        line_number: None,
        is_active_line: false,
        fold: None,
        hidden_lines: None,
        whitespace: Vec::new(),
        eol: None,
        indent_guides: Vec::new(),
        projection: Vec::new(),
        syntax_spans: vec![SyntaxRenderSpan {
            range: 12..128,
            color: Some(Color::from_rgb(1.0, 0.0, 0.0)),
        }],
    };
    let metrics = EditorMetrics {
        character_width: 10.0,
        ..EditorMetrics::default()
    };

    assert_eq!(
        visible_styled_text_range(
            &row,
            0.0,
            metrics,
            Rectangle {
                x: 100.0,
                y: 0.0,
                width: 50.0,
                height: 20.0,
            },
        ),
        0..row.text.len()
    );
}
