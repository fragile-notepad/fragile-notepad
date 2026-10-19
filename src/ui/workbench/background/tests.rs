use super::*;
use crate::core::{Document, DocumentId, EditorSettings};
use crate::ui::editor;
use iced::Color;
use iced::advanced::renderer::Headless;
use iced::widget::{Space, column, container, row};

const SIZE: Size = Size::new(320.0, 240.0);

fn renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn scene<'a>(
    document: Option<&'a Document>,
    settings: &'a EditorSettings,
    sidebar_width: f32,
    fractional: bool,
    optimized: bool,
) -> Element<'a, Message> {
    let editor = if let Some(document) = document {
        let editor = editor::view(document, settings);
        if optimized {
            super::editor(editor)
        } else {
            editor
        }
    } else {
        editor::empty()
    };
    let editor = container(editor)
        .width(iced::Fill)
        .height(iced::Fill)
        .style(move |theme| {
            let mut style = styles::editor_surface(theme);
            if optimized && document.is_some() {
                style.background = None;
            }
            style
        });
    let main: Element<'a, Message> = if sidebar_width > 0.0 {
        let sidebar = container(Space::new().width(iced::Fill).height(iced::Fill))
            .width(sidebar_width)
            .height(iced::Fill)
            .style(styles::function_list_panel);
        let sidebar: Element<'a, Message> = sidebar.into();
        row![editor, sidebar]
            .width(iced::Fill)
            .height(iced::Fill)
            .into()
    } else {
        editor.into()
    };
    let strip = |height: f32, style: fn(&Theme) -> iced::widget::container::Style| {
        container(Space::new())
            .width(iced::Fill)
            .height(height)
            .style(style)
    };
    let (menu_height, toolbar_height, tabs_height, find_height, status_height) = if fractional {
        // A partially revealed find panel and fractional sidebar width stress
        // the horizontal and vertical antialiased joins at both display scales.
        (23.25, 29.5, 27.75, 17.375, 26.25)
    } else {
        (23.0, 30.0, 28.0, 46.0, 26.0)
    };
    let status: Element<'a, Message> = strip(status_height, styles::status_bar).into();
    let content = column![
        strip(menu_height, styles::menu_bar),
        strip(toolbar_height, styles::tool_bar),
        strip(tabs_height, styles::tab_strip),
        strip(find_height, styles::utility_bar),
        main,
        status,
    ];
    let content: Element<'a, Message> = content.into();
    let content = if optimized { shell(content) } else { content };
    container(content)
        .width(iced::Fill)
        .height(iced::Fill)
        .style(move |theme| {
            let style = styles::app_shell(theme);
            if optimized {
                style
            } else {
                style.background(styles::app_shell_background(theme))
            }
        })
        .into()
}

fn snapshot(
    renderer: &mut Renderer,
    mut element: Element<'_, Message>,
    theme: &Theme,
    scale: f32,
) -> Vec<u8> {
    renderer.hint(scale);
    let mut tree = Tree::empty();
    tree.diff(element.as_widget_mut());
    let node =
        element
            .as_widget_mut()
            .layout(&mut tree, renderer, &layout::Limits::new(Size::ZERO, SIZE));
    assert_eq!(node.size(), SIZE);
    let viewport = Rectangle::with_size(SIZE);
    renderer.reset(viewport);
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
        Size::new((SIZE.width * scale) as u32, (SIZE.height * scale) as u32),
        scale,
        Color::from_rgb8(145, 18, 87),
    )
}

#[test]
fn software_fallback_matches_full_fills_at_fractional_joins() {
    let mut renderer = renderer();
    assert_rendering_equivalence(&mut renderer, 0);
}

#[cfg(feature = "hybrid-rendering")]
#[test]
fn gpu_covered_backgrounds_match_full_fills_at_fractional_joins() {
    let Some(mut renderer) = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("wgpu"),
    )) else {
        eprintln!("skipping GPU rendering equivalence: no wgpu adapter available");
        return;
    };
    // Fractional SDF coverage can round one RGB code apart when blended into
    // the GPU's eight-bit UNORM target. Alpha and software output remain exact.
    assert_rendering_equivalence(&mut renderer, 1);
}

fn assert_rendering_equivalence(renderer: &mut Renderer, rgb_tolerance: u8) {
    let settings = EditorSettings::default();
    let document = Document::from_path(DocumentId::new(1), "joins.csv", "first,row\nsecond,row\n");
    let mut max_observed_rgb_difference = 0;
    for (theme, theme_name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for has_document in [false, true] {
                for sidebar_width in [0.0, 37.25, 0.25] {
                    for fractional in [false, true] {
                        let document = has_document.then_some(&document);
                        let before = snapshot(
                            renderer,
                            scene(document, &settings, sidebar_width, fractional, false),
                            &theme,
                            scale,
                        );
                        let after = snapshot(
                            renderer,
                            scene(document, &settings, sidebar_width, fractional, true),
                            &theme,
                            scale,
                        );
                        assert_eq!(before.len(), after.len());
                        let mut max_rgb_difference = 0;
                        let mut mismatch = None;
                        for (pixel, (before, after)) in before
                            .chunks_exact(4)
                            .zip(after.chunks_exact(4))
                            .enumerate()
                        {
                            let rgb_difference = before[..3]
                                .iter()
                                .zip(&after[..3])
                                .map(|(before, after)| before.abs_diff(*after))
                                .max()
                                .unwrap_or(0);
                            max_rgb_difference = max_rgb_difference.max(rgb_difference);
                            if mismatch.is_none()
                                && (rgb_difference > rgb_tolerance || before[3] != after[3])
                            {
                                mismatch = Some((pixel, before, after));
                            }
                        }
                        let context = format!(
                            "{theme_name}, scale={scale}, document={has_document}, \
                             sidebar={sidebar_width}, fractional={fractional}"
                        );
                        if let Some((pixel, before, after)) = mismatch {
                            let width = (SIZE.width * scale) as usize;
                            panic!(
                                "{context}: pixel ({}, {}) differs: {before:?} != {after:?}; \
                                 maximum RGB difference={max_rgb_difference}, \
                                 allowed={rgb_tolerance}; alpha must match exactly",
                                pixel % width,
                                pixel / width,
                            );
                        }
                        max_observed_rgb_difference =
                            max_observed_rgb_difference.max(max_rgb_difference);
                    }
                }
            }
        }
    }
    if max_observed_rgb_difference > 0 {
        eprintln!(
            "background equivalence across all themes, scales, and joins: \
             maximum RGB difference={max_observed_rgb_difference}, \
             allowed={rgb_tolerance}; alpha matches exactly"
        );
    }
}
