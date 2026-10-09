use super::*;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Color, Event, Point, Rectangle, Renderer, Size};

const SIZE: Size = Size::new(640.0, 240.0);

fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn mount(element: &mut Element<'_, Message>, renderer: &Renderer) -> (Tree, layout::Node) {
    let mut tree = Tree::empty();
    tree.diff(element.as_widget_mut());
    let node =
        element
            .as_widget_mut()
            .layout(&mut tree, renderer, &layout::Limits::new(Size::ZERO, SIZE));
    (tree, node)
}

fn event(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    point: Point,
    event: mouse::Event,
) -> Vec<Message> {
    let mut messages = vec![];
    let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
    element.as_widget_mut().update(
        tree,
        &Event::Mouse(event),
        Layout::new(node),
        mouse::Cursor::Available(point),
        renderer,
        &mut shell,
        &Rectangle::with_size(SIZE),
    );
    messages
}

fn click(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    point: Point,
) -> Vec<Message> {
    let mut messages = event(
        element,
        tree,
        node,
        renderer,
        point,
        mouse::Event::ButtonPressed(mouse::Button::Left),
    );
    messages.extend(event(
        element,
        tree,
        node,
        renderer,
        point,
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ));
    messages
}

#[test]
fn controls_target_their_window_and_long_titles_do_not_cover_them() {
    let renderer = renderer();
    let id = window::Id::unique();
    for style in [ControlStyle::Windows, ControlStyle::MacOS] {
        let mut bar = bar(
            id,
            "A very long document name — 日本語 — ".repeat(30),
            style,
            true,
            false,
        );
        let (mut tree, node) = mount(&mut bar, &renderer);
        assert_eq!(node.size(), Size::new(640.0, HEIGHT));
        let targets = match style {
            ControlStyle::Windows => [(617.0, "close"), (571.0, "maximize"), (525.0, "minimize")],
            ControlStyle::MacOS => [(20.0, "close"), (40.0, "minimize"), (60.0, "maximize")],
        };
        for (x, expected) in targets {
            let messages = click(&mut bar, &mut tree, &node, &renderer, Point::new(x, 18.0));
            assert_eq!(messages.len(), 1, "{style:?}: {messages:?}");
            let Message::WindowChrome(target, action) = messages[0] else {
                panic!("wrong message")
            };
            assert_eq!(target, id);
            assert!(matches!(
                (expected, action),
                ("close", Action::Close)
                    | ("maximize", Action::ToggleMaximize)
                    | ("minimize", Action::Minimize)
            ));
        }
        let first = click(
            &mut bar,
            &mut tree,
            &node,
            &renderer,
            Point::new(300.0, 18.0),
        );
        assert!(
            first.is_empty(),
            "stationary clicks must not enter the native drag loop"
        );
        let second = click(
            &mut bar,
            &mut tree,
            &node,
            &renderer,
            Point::new(300.0, 18.0),
        );
        assert!(
            matches!(
                second.as_slice(),
                [Message::WindowChrome(_, Action::ToggleMaximize)]
            ),
            "double click must not also start dragging: {second:?}"
        );
    }
}

#[test]
fn caption_drag_requires_motion_and_release_cancels_pending_drag() {
    let renderer = renderer();
    let mut bar = bar(
        window::Id::unique(),
        "Notes".into(),
        ControlStyle::MacOS,
        true,
        false,
    );
    let (mut tree, node) = mount(&mut bar, &renderer);
    let start = Point::new(300.0, 18.0);
    let moved = Point::new(320.0, 18.0);
    assert!(
        event(
            &mut bar,
            &mut tree,
            &node,
            &renderer,
            start,
            mouse::Event::ButtonPressed(mouse::Button::Left)
        )
        .is_empty()
    );
    let messages = event(
        &mut bar,
        &mut tree,
        &node,
        &renderer,
        moved,
        mouse::Event::CursorMoved { position: moved },
    );
    assert!(matches!(
        messages.as_slice(),
        [Message::WindowChrome(_, Action::Drag)]
    ));
    assert!(
        event(
            &mut bar,
            &mut tree,
            &node,
            &renderer,
            moved,
            mouse::Event::CursorMoved { position: moved }
        )
        .is_empty()
    );
    let _ = event(
        &mut bar,
        &mut tree,
        &node,
        &renderer,
        moved,
        mouse::Event::ButtonReleased(mouse::Button::Left),
    );
    let _ = click(&mut bar, &mut tree, &node, &renderer, start);
    assert!(
        event(
            &mut bar,
            &mut tree,
            &node,
            &renderer,
            moved,
            mouse::Event::CursorMoved { position: moved }
        )
        .is_empty()
    );
}

#[test]
fn resize_edges_do_not_steal_content_clicks_and_disappear_when_maximized() {
    let renderer = renderer();
    let id = window::Id::unique();
    for maximized in [false, true] {
        let content = button(iced::widget::Space::new().width(Fill).height(Fill))
            .padding(0)
            .on_press(Message::NewFile)
            .into();
        let mut frame = frame(
            content,
            id,
            "Notes".into(),
            ControlStyle::Windows,
            true,
            maximized,
        );
        let (mut tree, node) = mount(&mut frame, &renderer);
        assert_eq!(node.size(), SIZE);
        let messages = click(
            &mut frame,
            &mut tree,
            &node,
            &renderer,
            Point::new(100.0, 100.0),
        );
        assert!(matches!(messages.as_slice(), [Message::NewFile]));
        let messages = click(
            &mut frame,
            &mut tree,
            &node,
            &renderer,
            Point::new(1.0, 100.0),
        );
        if !maximized && !cfg!(target_os = "macos") {
            assert!(matches!(
                messages.as_slice(),
                [Message::WindowChrome(
                    _,
                    Action::Resize(window::Direction::West)
                )]
            ));
        } else {
            assert!(matches!(messages.as_slice(), [Message::NewFile]));
        }
    }
}

#[test]
fn hovered_caption_tooltips_are_delayed_and_cover_underlying_content() {
    let mut renderer = renderer();
    for (style, x, name) in [
        (ControlStyle::MacOS, 20.0, "macos"),
        (ControlStyle::Windows, 617.0, "windows"),
    ] {
        for (theme, theme_name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            let mut bar = bar(
                window::Id::unique(),
                "notes.md — Fragile Notepad".into(),
                style,
                true,
                false,
            );
            let (mut tree, node) = mount(&mut bar, &renderer);
            let point = Point::new(x, 18.0);
            let _ = event(
                &mut bar,
                &mut tree,
                &node,
                &renderer,
                point,
                mouse::Event::CursorMoved { position: point },
            );
            let viewport = Rectangle::with_size(SIZE);
            assert!(
                bar.as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        iced::Vector::ZERO
                    )
                    .is_none(),
                "passing over a caption control must not immediately cover the menu"
            );
            std::thread::sleep(std::time::Duration::from_millis(620));
            let _ = event(
                &mut bar,
                &mut tree,
                &node,
                &renderer,
                point,
                mouse::Event::CursorMoved { position: point },
            );
            let mut screenshots = Vec::new();
            for background in [Color::WHITE, Color::BLACK] {
                renderer::Renderer::reset(&mut renderer, viewport);
                bar.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &theme,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &viewport,
                );
                let mut overlay = bar
                    .as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        iced::Vector::ZERO,
                    )
                    .expect("tooltip after hover delay");
                let overlay_node = overlay.as_overlay_mut().layout(&renderer, SIZE);
                overlay.as_overlay().draw(
                    &mut renderer,
                    &theme,
                    &renderer::Style::default(),
                    Layout::new(&overlay_node),
                    mouse::Cursor::Available(point),
                );
                screenshots.push(renderer.screenshot(Size::new(640, 100), 1.0, background));
            }
            let offset = (44 * 640 + x as usize) * 4;
            assert_eq!(
                &screenshots[0][offset..offset + 4],
                &screenshots[1][offset..offset + 4],
                "tooltip must obscure underlying pixels in {style:?} / {theme_name}"
            );
            if let Some(directory) = std::env::var_os("FRAGILE_TITLE_BAR_SNAPSHOTS") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let mut ppm = b"P6\n640 100\n255\n".to_vec();
                for pixel in screenshots[0].chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write(
                    directory.join(format!("{name}-{theme_name}-hover.ppm")),
                    ppm,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn both_control_styles_render_in_light_dark_and_inactive_states() {
    let mut renderer = renderer();
    let id = window::Id::unique();
    for (style, name) in [
        (ControlStyle::Windows, "windows"),
        (ControlStyle::MacOS, "macos"),
    ] {
        for (theme, theme_name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            for focused in [true, false] {
                let mut bar = bar(
                    id,
                    "notes.md — Fragile Notepad".into(),
                    style,
                    focused,
                    false,
                );
                let (tree, node) = mount(&mut bar, &renderer);
                renderer::Renderer::reset(&mut renderer, Rectangle::with_size(SIZE));
                bar.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &theme,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &Rectangle::with_size(SIZE),
                );
                let pixels = renderer.screenshot(Size::new(640, HEIGHT as u32), 1.0, Color::WHITE);
                assert_eq!(pixels.len(), 640 * HEIGHT as usize * 4);
                let pixel = |x: usize, y: usize| &pixels[(y * 640 + x) * 4..(y * 640 + x) * 4 + 3];
                assert_ne!(
                    pixel(20, 16),
                    pixel(110, 2),
                    "controls/title must be visible"
                );
                if style == ControlStyle::MacOS && focused {
                    assert!(
                        pixel(20, 16)[0] > 200 && pixel(20, 16)[1] < 130,
                        "close traffic light is red"
                    );
                    assert!(
                        pixel(60, 16)[1] > 150 && pixel(60, 16)[0] < 80,
                        "zoom traffic light is green"
                    );
                }
                // Optional local artifacts for visual inspection, never a golden-image test.
                if let Some(directory) = std::env::var_os("FRAGILE_TITLE_BAR_SNAPSHOTS") {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    let mut ppm = format!("P6\n640 {}\n255\n", HEIGHT as u32).into_bytes();
                    for pixel in pixels.chunks_exact(4) {
                        ppm.extend_from_slice(&pixel[..3]);
                    }
                    std::fs::write(
                        directory.join(format!("{name}-{theme_name}-{focused}.ppm")),
                        ppm,
                    )
                    .unwrap();
                }
            }
        }
    }
}

fn compare_frame_backgrounds_at_display_scales(renderer: &mut Renderer) {
    fn content() -> Element<'static, Message> {
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(styles::editor_surface)
            .into()
    }

    fn original_frame(id: window::Id, focused: bool, maximized: bool) -> Element<'static, Message> {
        let resize = !maximized && !cfg!(target_os = "macos");
        let border = if resize { RESIZE_BORDER } else { 0.0 };
        // Preserve the previous full-window background and exactly the same
        // caption, content, padding, and resize border geometry as frame().
        container(
            iced::widget::column![
                bar(
                    id,
                    "notes.csv — Fragile Notepad".into(),
                    ControlStyle::Windows,
                    focused,
                    maximized,
                ),
                container(content())
                    .padding(
                        iced::Padding::ZERO
                            .left(border)
                            .right(border)
                            .bottom(border)
                    )
                    .height(Fill),
            ]
            .height(Fill),
        )
        .width(Fill)
        .height(Fill)
        .style(move |theme| styles::window_frame(theme, focused))
        .into()
    }

    fn screenshot(
        mut element: Element<'_, Message>,
        renderer: &mut Renderer,
        theme: &Theme,
        scale: f32,
    ) -> Vec<u8> {
        let viewport = Rectangle::with_size(SIZE);
        renderer::Renderer::reset(renderer, viewport);
        renderer::Renderer::hint(renderer, scale);
        let (tree, node) = mount(&mut element, renderer);
        assert_eq!(node.size(), SIZE);
        element.as_widget().draw(
            &tree,
            renderer,
            theme,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        renderer.screenshot(
            Size::new(
                (SIZE.width * scale).round() as u32,
                (SIZE.height * scale).round() as u32,
            ),
            scale,
            // Expose any holes left between the frame bands and opaque content.
            Color::from_rgb(1.0, 0.0, 1.0),
        )
    }

    let id = window::Id::unique();
    for (theme, theme_name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        for focused in [false, true] {
            for maximized in [false, true] {
                for scale in [1.0, 1.25, 1.5, 1.3] {
                    let optimized = frame(
                        content(),
                        id,
                        "notes.csv — Fragile Notepad".into(),
                        ControlStyle::Windows,
                        focused,
                        maximized,
                    );
                    let actual = screenshot(optimized, renderer, &theme, scale);
                    let expected = screenshot(
                        original_frame(id, focused, maximized),
                        renderer,
                        &theme,
                        scale,
                    );
                    assert_eq!(actual.len(), expected.len());
                    if let Some((index, (actual, expected))) = actual
                        .chunks_exact(4)
                        .zip(expected.chunks_exact(4))
                        .enumerate()
                        .find(|(_, (actual, expected))| actual != expected)
                    {
                        let width = (SIZE.width * scale).round() as usize;
                        panic!(
                            "{theme_name}, focused={focused}, maximized={maximized}, \
                             scale={scale}, pixel=({}, {}): optimized={actual:?}, \
                             original={expected:?}",
                            index % width,
                            index / width,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn clipped_frame_matches_full_background_at_fractional_display_scales() {
    compare_frame_backgrounds_at_display_scales(&mut renderer());
}

#[cfg(feature = "hybrid-rendering")]
#[test]
fn vulkan_clipped_frame_matches_full_background_at_fractional_display_scales() {
    let Some(mut renderer) = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("wgpu"),
    )) else {
        eprintln!("Skipping Vulkan frame validation: no wgpu headless adapter");
        return;
    };
    assert_eq!(renderer.name(), "wgpu");
    renderer::Renderer::hint(&mut renderer, 1.3);
    assert_eq!(renderer::Renderer::scale_factor(&renderer), Some(1.3));
    compare_frame_backgrounds_at_display_scales(&mut renderer);
}
