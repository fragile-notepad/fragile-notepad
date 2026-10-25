use super::*;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::Headless;
use iced::widget::Space;

fn row(order: &[u64], drag: Option<DragPosition>) -> TabRow<'static> {
    TabRow {
        keys: order.iter().map(|id| Some(DocumentId::new(*id))).collect(),
        children: order
            .iter()
            .map(|_| {
                Space::new()
                    .width(120)
                    .height(super::super::TAB_HEIGHT)
                    .into()
            })
            .collect(),
        selected: vec![DocumentId::new(1)],
        dragging: drag.is_some(),
        drag,
    }
}

fn renderer() -> iced::Renderer {
    futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn layout_row(row: &mut TabRow<'_>, tree: &mut Tree, renderer: &iced::Renderer) -> layout::Node {
    row.layout(
        tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(600.0, 27.0)),
    )
}

#[test]
fn reorder_keeps_document_positions_and_animates_neighbors_to_their_slots() {
    let renderer = renderer();
    let mut row = row(&[1, 2, 3], None);
    let mut tree = Tree::empty();
    tree.diff(&mut row as &mut dyn Widget<Message, Theme, iced::Renderer>);
    layout_row(&mut row, &mut tree, &renderer);
    row.keys.swap(0, 1);
    row.diff(&mut tree);
    layout_row(&mut row, &mut tree, &renderer);
    let state = tree.state.downcast_ref::<State>();
    let neighbor = &state.positions[&Some(DocumentId::new(2))];
    assert_eq!(neighbor.value(state.now), 120.0);
    assert_eq!(neighbor.target, 0.0);
    let midway = neighbor.value(state.now + DURATION / 2);
    assert!(midway > 0.0 && midway < 120.0);
    assert_eq!(neighbor.value(state.now + DURATION), 0.0);
}

#[test]
fn grabbed_position_tracks_pointer_after_reorder_and_scrolling_then_settles_on_release() {
    let renderer = renderer();
    let mut row = row(
        &[2, 1, 3],
        Some(DragPosition {
            pointer_x: 120.0,
            grab_offset: 23.0,
            group_width: 120.0,
        }),
    );
    let mut tree = Tree::empty();
    tree.diff(&mut row as &mut dyn Widget<Message, Theme, iced::Renderer>);
    let node = layout_row(&mut row, &mut tree, &renderer);
    let layout = Layout::new(&node);
    let viewport = Rectangle {
        x: 81.0,
        y: 0.0,
        width: 250.0,
        height: 27.0,
    };
    let now = Instant::now();
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    row.update(
        &mut tree,
        &Event::Window(window::Event::RedrawRequested(now)),
        layout,
        mouse::Cursor::Unavailable,
        &renderer,
        &mut shell,
        &viewport,
    );
    let state = tree.state.downcast_ref::<State>();
    let delta = row.translation(state, layout, &viewport, 1);
    // Convert the painted tab back to the visible scrolled coordinates.
    assert_eq!(
        layout.child(1).bounds().x + delta.x - viewport.x + 23.0,
        120.0
    );
    let held_x = state.positions[&Some(DocumentId::new(1))].value(now);
    row.drag = None;
    row.dragging = false;
    layout_row(&mut row, &mut tree, &renderer);
    let state = tree.state.downcast_ref::<State>();
    let released = &state.positions[&Some(DocumentId::new(1))];
    assert_eq!(released.value(now), held_x);
    assert_eq!(released.value(now + DURATION), 120.0);
}

#[test]
fn held_tabs_stay_inside_scrolled_viewport_at_both_horizontal_edges() {
    let renderer = renderer();
    for selected in [
        vec![DocumentId::new(1)],
        vec![DocumentId::new(1), DocumentId::new(2)],
    ] {
        for pointer_x in [-1000.0, 1000.0] {
            let mut row = row(
                &[2, 1, 3],
                Some(DragPosition {
                    pointer_x,
                    grab_offset: 23.0,
                    group_width: selected.len() as f32 * 120.0,
                }),
            );
            row.selected = selected.clone();
            let mut tree = Tree::empty();
            tree.diff(&mut row as &mut dyn Widget<Message, Theme, iced::Renderer>);
            let node = layout_row(&mut row, &mut tree, &renderer);
            let layout = Layout::new(&node);
            let viewport = Rectangle {
                x: 81.0,
                y: 0.0,
                width: 250.0,
                height: 27.0,
            };
            let state = tree.state.downcast_ref::<State>();
            for index in 0..row.children.len() {
                if !row.follows(index) {
                    continue;
                }
                let painted =
                    layout.child(index).bounds() + row.translation(state, layout, &viewport, index);
                assert!(painted.x >= viewport.x);
                assert!(painted.x + painted.width <= viewport.x + viewport.width);
            }
        }
    }
}

#[test]
fn wider_selection_keeps_its_grab_point_visible_at_either_edge() {
    for pointer_x in [-1000.0, 1000.0] {
        let drag = DragPosition {
            pointer_x,
            grab_offset: 160.0,
            group_width: 360.0,
        };
        let grabbed = drag.leading_x(360.0, 250.0) + drag.grab_offset;
        assert!((32.0..=218.0).contains(&grabbed));
    }
}

#[test]
fn reversing_an_inflight_move_preserves_its_current_painted_position() {
    let now = Instant::now();
    let mut motion = Motion::new(0.0, now);
    motion.retarget(240.0, now);
    let halfway = now + DURATION / 2;
    let before = motion.value(halfway);
    motion.retarget(0.0, halfway);
    assert_eq!(motion.value(halfway), before);
    assert_eq!(motion.value(halfway + DURATION), 0.0);
}

#[test]
fn input_that_finishes_a_move_requests_the_final_frame() {
    let renderer = renderer();
    let mut row = row(&[1, 2], None);
    let mut tree = Tree::empty();
    tree.diff(&mut row as &mut dyn Widget<Message, Theme, iced::Renderer>);
    let node = layout_row(&mut row, &mut tree, &renderer);
    let state = tree.state.downcast_mut::<State>();
    let start = Instant::now() - DURATION * 2;
    let mut motion = Motion::new(120.0, start);
    motion.retarget(0.0, start);
    state.positions.insert(Some(DocumentId::new(1)), motion);
    state.now = start;
    let mut messages = Vec::new();
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    row.update(
        &mut tree,
        &Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(20.0, 15.0),
        }),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut shell,
        &Rectangle::with_size(node.size()),
    );
    assert_eq!(shell.redraw_request(), window::RedrawRequest::NextFrame);
}

fn check_held_tab_occlusion(renderer: &mut iced::Renderer) {
    use crate::core::Document;
    use iced::Color;

    let mut held = Document::untitled(DocumentId::new(1));
    held.set_path("Alpha.txt");
    let mut neighbor = Document::untitled(DocumentId::new(2));
    neighbor.set_path("XXXXXXXXXXXX.txt");
    let size = Size::new(600.0, 60.0);
    let viewport = Rectangle::with_size(size);
    for theme in [Theme::Light, Theme::Dark] {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut render = |neighbors: bool| {
                let mut row = TabRow {
                    keys: vec![Some(held.id)],
                    children: vec![super::super::tab(
                        &held,
                        true,
                        true,
                        super::super::DragVisual::Dragged,
                    )],
                    selected: vec![held.id],
                    dragging: true,
                    drag: Some(DragPosition {
                        pointer_x: 100.0,
                        grab_offset: 23.0,
                        group_width: 141.0,
                    }),
                };
                if neighbors {
                    row.keys.push(Some(neighbor.id));
                    row.children.push(super::super::tab(
                        &neighbor,
                        false,
                        false,
                        super::super::DragVisual::Idle,
                    ));
                }
                renderer.reset(viewport);
                renderer.hint(scale);
                // Match the real strip behind antialiased rounded tab edges,
                // including the dark theme's background.
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: viewport,
                        ..Default::default()
                    },
                    crate::ui::styles::tab_strip(&theme)
                        .background
                        .expect("tab strip background"),
                );
                let mut tree = Tree::empty();
                tree.diff(&mut row as &mut dyn Widget<Message, Theme, iced::Renderer>);
                let node = row.layout(&mut tree, renderer, &layout::Limits::new(Size::ZERO, size));
                let width = node.children()[0].size().width;
                row.draw(
                    &tree,
                    renderer,
                    &theme,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &viewport,
                );
                let pixels = renderer.screenshot(
                    Size::new((size.width * scale) as u32, (size.height * scale) as u32),
                    scale,
                    Color::WHITE,
                );
                (pixels, width)
            };
            let (actual, width) = render(true);
            let (reference, _) = render(false);
            let pixel_width = (size.width * scale) as usize;
            let accent = crate::ui::styles::accent_color(&theme).into_rgba8();
            let is_accent = |pixel: &[u8]| {
                pixel[..3]
                    .iter()
                    .zip(accent)
                    .all(|(channel, expected)| channel.abs_diff(expected) < 20)
            };
            let mut lower_accent = 0;
            let mut top_accent = 0;
            for y in 0..(super::super::TAB_HEIGHT * scale) as usize {
                for x in (80.0 * scale) as usize..((77.0 + width - 3.0) * scale) as usize {
                    let offset = (y * pixel_width + x) * 4;
                    if is_accent(&actual[offset..offset + 4]) {
                        if y >= (23.0 * scale) as usize {
                            lower_accent += 1;
                        } else if y < (super::super::TAB_TOP_BAR_HEIGHT * scale) as usize {
                            top_accent += 1;
                        }
                    }
                }
            }
            assert_eq!(
                lower_accent,
                0,
                "accent fragments below the pin/close buttons: renderer={}, theme={theme:?}, scale={scale}",
                renderer.name()
            );
            assert!(top_accent > 0, "the top accent must stay visible");
            let mut mismatches = 0;
            for y in (4.0 * scale) as usize..(23.0 * scale) as usize {
                for x in (80.0 * scale) as usize..((77.0 + width - 3.0) * scale) as usize {
                    let offset = (y * pixel_width + x) * 4;
                    mismatches +=
                        usize::from(actual[offset..offset + 4] != reference[offset..offset + 4]);
                }
            }
            assert_eq!(
                mismatches,
                0,
                "neighbor text/icons leaked through the held tab: renderer={}, theme={theme:?}, scale={scale}",
                renderer.name()
            );
        }
    }
}

#[test]
fn neighboring_labels_and_icons_cannot_paint_above_the_held_tab_on_software() {
    let _guard = crate::font_system_test_guard();
    check_held_tab_occlusion(&mut renderer());
}

#[cfg(feature = "hybrid-rendering")]
#[test]
fn neighboring_labels_and_icons_cannot_paint_above_the_held_tab_on_vulkan() {
    let _guard = crate::font_system_test_guard();
    let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("wgpu"),
    ))
    .expect("Vulkan renderer for tab occlusion regression");
    check_held_tab_occlusion(&mut renderer);
}
