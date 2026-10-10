use iced::widget::{button, column, container, opaque, row, rule, scrollable, space, stack, text};
use iced::{Center, Element, Fill, Length};

use crate::message::{AboutTab, Message};
use crate::ui::controls::{centered_button_label, centered_fill_button_label};
use crate::ui::motion::{fade_button, fade_container};
use crate::ui::{info_vfx, motion, raster_cache, styles};

const APP_NAME: &str = "Fragile Notepad";

fn fade_scrollable(mut style: scrollable::Style, opacity: f32) -> scrollable::Style {
    style.container = fade_container(style.container, opacity);
    for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
        rail.background = rail.background.map(|color| color.scale_alpha(opacity));
        rail.border.color = rail.border.color.scale_alpha(opacity);
        rail.scroller.background = rail.scroller.background.scale_alpha(opacity);
        rail.scroller.border.color = rail.scroller.border.color.scale_alpha(opacity);
    }
    style.gap = style.gap.map(|color| color.scale_alpha(opacity));
    style.auto_scroll.background = style.auto_scroll.background.scale_alpha(opacity);
    style.auto_scroll.border.color = style.auto_scroll.border.color.scale_alpha(opacity);
    style.auto_scroll.shadow.color = style.auto_scroll.shadow.color.scale_alpha(opacity);
    style.auto_scroll.icon = style.auto_scroll.icon.scale_alpha(opacity);
    style
}

#[derive(Debug, Clone)]
pub struct RenderingDebugInfo {
    pub current_renderer: String,
    pub rendering_policy: String,
    pub title_bar_style: crate::ui::title_bar::ControlStyle,
}

struct AuthorEntry {
    name: &'static str,
    email: &'static str,
}

const AUTHORS: &[AuthorEntry] = &[
    AuthorEntry {
        name: "Rachel Fragile",
        email: "rabbit0w0@outlook.com",
    },
    AuthorEntry {
        name: "SoundRain",
        email: "soundstarrain@outlook.com",
    },
];

struct LicenseEntry {
    name: &'static str,
    version: &'static str,
    license: &'static str,
    notes: &'static str,
}

const LICENSES: &[LicenseEntry] = &[
    LicenseEntry {
        name: "blake3",
        version: "1.8.7",
        license: "CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception",
        notes: "File fingerprints for detecting changes made outside the editor.",
    },
    LicenseEntry {
        name: "iced",
        version: "0.15.0-dev",
        license: "MIT",
        notes: "GUI toolkit, vendored under vendor/iced/LICENSE.",
    },
    LicenseEntry {
        name: "encoding_rs",
        version: "0.8.35",
        license: "MIT OR Apache-2.0, with WHATWG encoding data terms",
        notes: "Text encoding support, vendored under vendor/encoding_rs/.",
    },
    LicenseEntry {
        name: "rfd",
        version: "0.16.0",
        license: "MIT OR Apache-2.0",
        notes: "Native file dialogs.",
    },
    LicenseEntry {
        name: "tokio",
        version: "1.52.3",
        license: "MIT",
        notes: "Async runtime used by filesystem tasks.",
    },
    LicenseEntry {
        name: "unicode-segmentation",
        version: "1.13.3",
        license: "MIT OR Apache-2.0",
        notes: "Unicode grapheme segmentation.",
    },
    LicenseEntry {
        name: "unicode-width",
        version: "0.2.2",
        license: "MIT OR Apache-2.0",
        notes: "Display width calculations.",
    },
    LicenseEntry {
        name: "tiny-skia",
        version: "0.11.4",
        license: "BSD-3-Clause",
        notes: "Software raster rendering path used by iced and tests.",
    },
    LicenseEntry {
        name: "Heroicons",
        version: "Refined outline icons",
        license: "MIT",
        notes: "Locally refined controls; assets/icons/heroicons/LICENSE.",
    },
    LicenseEntry {
        name: "Bootstrap Icons",
        version: "Refined SVG icons",
        license: "MIT",
        notes: "Locally refined shortcuts and pins; assets/icons/bootstrap/LICENSE.",
    },
    LicenseEntry {
        name: "Fragile Notepad artwork",
        version: "Original icons and illustrations",
        license: "All rights reserved",
        notes: "See assets/icons/colored/LICENSE and assets/illustrations/LICENSE.",
    },
];

pub fn view(
    active_tab: AboutTab,
    rendering: RenderingDebugInfo,
    progress: f32,
    interactive: bool,
) -> Element<'static, Message> {
    let progress = progress.clamp(0.0, 1.0);
    let content = stack![
        opaque(
            container(space::vertical())
                .width(Fill)
                .height(Fill)
                .style(move |theme| fade_container(styles::modal_scrim(theme), progress))
        ),
        container(dialog(active_tab, rendering, progress, interactive))
            .padding(16)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill),
    ];
    // Keep the modal input barrier in place throughout the closing fade.
    opaque(motion::fade(
        content,
        1.0,
        styles::editor_background,
        interactive,
    ))
}

fn dialog(
    active_tab: AboutTab,
    rendering: RenderingDebugInfo,
    progress: f32,
    interactive: bool,
) -> Element<'static, Message> {
    let content = match active_tab {
        AboutTab::About => about_content(progress),
        AboutTab::Debug => debug_content(rendering, progress),
        AboutTab::Licenses => licenses_content(progress),
    };

    container(
        column![
            container(
                column![
                    header(progress, interactive),
                    raster_cache::cached(tabs(active_tab, progress))
                ]
                .spacing(12)
            )
            .padding([20, 28]),
            container(raster_cache::cached_animated(content))
                .padding([0, 28])
                .height(Fill)
                .width(Fill),
            raster_cache::cached(
                container(
                    column![
                        divider(progress),
                        row![
                            muted(
                                text(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(12),
                                progress
                            ),
                            space::horizontal(),
                            button(centered_button_label("Close", 13))
                                .padding([8, 24])
                                .style(move |theme, status| fade_button(
                                    styles::primary_command_button(theme, status),
                                    progress
                                ))
                                .on_press(Message::AboutClosed),
                        ]
                        .spacing(10)
                        .align_y(Center)
                        .width(Fill),
                    ]
                    .spacing(14)
                )
                .padding([16, 28])
            ),
        ]
        .height(Fill)
        .width(Fill),
    )
    .width(Length::Fixed(600.0))
    .height(Length::Fixed(500.0))
    .clip(true)
    .style(move |theme| fade_container(styles::info_dialog(theme), progress))
    .into()
}

fn header(progress: f32, effects_running: bool) -> Element<'static, Message> {
    stack![
        info_vfx::view(progress, effects_running),
        raster_cache::cached(
            container(
                row![
                    // The shared VFX widget draws and animates the logo in this slot.
                    space()
                        .width(info_vfx::LOGO_SIZE)
                        .height(info_vfx::LOGO_SIZE),
                    column![
                        text(APP_NAME).size(23).font(iced::Font {
                            weight: iced::font::Weight::Semibold,
                            ..iced::Font::DEFAULT
                        }),
                        muted(
                            text("A lightweight editor for everyday text.").size(13),
                            progress
                        ),
                    ]
                    .spacing(6)
                    .width(Fill),
                ]
                .spacing(14)
                .align_y(Center)
            )
            .width(Fill)
            .height(info_vfx::HEADER_HEIGHT)
            .center_y(Fill)
        ),
    ]
    .into()
}

fn tabs(active_tab: AboutTab, progress: f32) -> Element<'static, Message> {
    container(
        row![
            tab_button("About", AboutTab::About, active_tab, progress),
            tab_button("Debug", AboutTab::Debug, active_tab, progress),
            tab_button("Licenses", AboutTab::Licenses, active_tab, progress),
        ]
        .spacing(4)
        .width(Fill),
    )
    .padding(4)
    .style(move |theme| fade_container(styles::info_card(theme), progress))
    .into()
}

fn tab_button(
    label: &'static str,
    tab: AboutTab,
    active_tab: AboutTab,
    progress: f32,
) -> Element<'static, Message> {
    button(centered_fill_button_label(label, 13))
        .padding([7, 14])
        .width(Fill)
        .style(move |theme, status| {
            let style = styles::info_tab(theme, status, tab == active_tab);
            fade_button(style, progress)
        })
        .on_press(Message::AboutTabSelected(tab))
        .into()
}

fn about_content(progress: f32) -> Element<'static, Message> {
    scrollable(column![
        column![
            text("A little space for your words.").size(25).font(iced::Font {
                weight: iced::font::Weight::Medium,
                ..iced::Font::DEFAULT
            }),
            muted(text("Quick notes, source code, and everything in between.\nSimple tools for working with local text files.")
                .size(14).line_height(1.55), progress),
        ].spacing(10),
        container(
            row![
                container(space()).width(3).height(Fill)
                    .style(move |theme: &iced::Theme| fade_container(container::Style {
                        background: Some(styles::accent_color(theme).scale_alpha(0.4).into()),
                        border: iced::Border::default().rounded(2),
                        ..container::Style::default()
                    }, progress)),
                column![
                    muted(text("CREATED BY").size(11), progress),
                    column(
                        AUTHORS
                                .iter()
                                .map(|x| author_entry(x.name, x.email, progress)),
                    ).spacing(8).width(Fill),
                ]
                .spacing(6),
            ]
            .spacing(16)
            .align_y(Center)
            .height(Length::Shrink),
        )
        .padding(20)
        .width(Fill)
        .style(move |theme| fade_container(styles::info_card(theme), progress)),
    ].spacing(22).padding([2, 0]).width(Fill)).smooth_scroll(true)
        .style(move |theme, status| fade_scrollable(styles::scrollable(theme, status), progress))
        .height(Fill)
        .width(Fill)
        .into()
}

fn author_entry(
    name: &'static str,
    email: &'static str,
    progress: f32,
) -> Element<'static, Message> {
    container(column![
        text(name).size(16).font(iced::Font {
            weight: iced::font::Weight::Medium,
            ..iced::Font::DEFAULT
        }),
        muted(text(email).size(13), progress),
    ])
    .into()
}

fn muted(
    content: impl Into<Element<'static, Message>>,
    progress: f32,
) -> Element<'static, Message> {
    container(content)
        .style(move |theme| fade_container(styles::info_muted(theme), progress))
        .into()
}

fn divider(progress: f32) -> Element<'static, Message> {
    rule::horizontal(1)
        .style(move |theme| {
            let mut style = styles::utility_rule(theme);
            style.color = style.color.scale_alpha(progress * 0.5);
            style
        })
        .into()
}

fn debug_content(rendering: RenderingDebugInfo, progress: f32) -> Element<'static, Message> {
    let build_profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    let panic_strategy = if cfg!(panic = "abort") {
        "abort"
    } else {
        "unwind"
    };

    let startup_probe = if crate::startup::startup_probe_enabled() {
        "enabled"
    } else {
        "disabled"
    };

    let sections = column![
        title_bar_preview(&rendering, progress),
        debug_section(
            progress,
            "Application",
            &[
                ("Name", env!("CARGO_PKG_NAME").to_owned()),
                ("Version", env!("CARGO_PKG_VERSION").to_owned()),
                ("Authors", env!("CARGO_PKG_AUTHORS").to_owned()),
                ("Build profile", build_profile.to_owned()),
                ("Panic strategy", panic_strategy.to_owned()),
            ],
        ),
        debug_section(
            progress,
            "Runtime",
            &[
                ("Operating system", std::env::consts::OS.to_owned()),
                ("Architecture", std::env::consts::ARCH.to_owned()),
                ("Platform family", std::env::consts::FAMILY.to_owned()),
                ("Startup probe", startup_probe.to_string(),),
            ],
        ),
        debug_section(
            progress,
            "Rendering",
            &[
                ("Current renderer", rendering.current_renderer),
                ("Rendering policy", rendering.rendering_policy),
            ],
        ),
    ]
    .spacing(12)
    .padding(iced::Padding::new(0.0).right(10))
    .width(Fill);
    scrollable(sections)
        .smooth_scroll(true)
        .style(move |theme, status| fade_scrollable(styles::scrollable(theme, status), progress))
        .height(Fill)
        .width(Fill)
        .into()
}

fn title_bar_preview(rendering: &RenderingDebugInfo, progress: f32) -> Element<'static, Message> {
    #[cfg(debug_assertions)]
    {
        let label = match rendering.title_bar_style {
            crate::ui::title_bar::ControlStyle::Windows => "Switch to macOS traffic lights",
            crate::ui::title_bar::ControlStyle::MacOS => "Switch to Windows controls",
        };
        container(
            row![
                muted(text("Window controls").size(12), progress),
                space::horizontal(),
                button(text(label).size(12))
                    .padding([6, 12])
                    .style(move |theme, status| fade_button(
                        styles::command_button(theme, status),
                        progress
                    ))
                    .on_press(Message::ToggleTitleBarStyle),
            ]
            .align_y(Center),
        )
        .padding(12)
        .style(move |theme| fade_container(styles::utility_bar(theme), progress))
        .into()
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (rendering, progress);
        space::vertical().height(0).into()
    }
}

fn debug_section(
    progress: f32,
    heading: &'static str,
    rows: &[(&'static str, String)],
) -> Element<'static, Message> {
    let rows = rows
        .iter()
        .fold(column![].spacing(9), |column, (label, value)| {
            column.push(
                row![
                    container(muted(text(*label).size(12), progress)).width(140),
                    text(value.clone()).size(13).width(Fill),
                ]
                .spacing(10)
                .align_y(iced::Top),
            )
        });

    container(column![text(heading.to_owned()).size(15), divider(progress), rows].spacing(12))
        .padding(16)
        .width(Fill)
        .style(move |theme| fade_container(styles::info_card(theme), progress))
        .into()
}

fn licenses_content(progress: f32) -> Element<'static, Message> {
    scrollable(
        column![
            column![
                text("Open-source acknowledgements").size(18),
                muted(
                    text("Fragile Notepad is built with these libraries and assets.").size(13),
                    progress
                )
            ]
            .spacing(6),
            column(LICENSES.iter().map(|entry| license_entry(entry, progress)),)
                .spacing(10)
                .width(Fill),
        ]
        .spacing(18)
        .padding(iced::Padding::new(0.0).right(10))
        .width(Fill),
    )
    .smooth_scroll(true)
    .style(move |theme, status| fade_scrollable(styles::scrollable(theme, status), progress))
    .height(Fill)
    .width(Fill)
    .into()
}

fn license_entry(entry: &LicenseEntry, progress: f32) -> Element<'static, Message> {
    container(
        column![
            row![
                text(entry.name).size(15).width(Fill),
                muted(text(entry.version).size(12), progress)
            ]
            .spacing(8)
            .align_y(Center),
            container(text(entry.license).size(11).width(Fill))
                .padding([4, 8])
                .style(move |theme| fade_container(styles::info_badge(theme), progress)),
            muted(text(entry.notes).size(12).width(Fill), progress),
        ]
        .spacing(8),
    )
    .padding(16)
    .width(Fill)
    .style(move |theme| fade_container(styles::info_card(theme), progress))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::{self, Headless};
    use iced::advanced::widget::{self, Operation, Tree, operation};
    use iced::advanced::{Layout, Renderer as _, Shell, layout, mouse};
    use iced::{Color, Event, Point, Rectangle, Renderer, Size, Theme, Vector, window};

    const VIEWPORT: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    fn renderer() -> Renderer {
        futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("CPU headless renderer must be available")
    }

    fn rendering_info() -> RenderingDebugInfo {
        RenderingDebugInfo {
            current_renderer: String::from("Software"),
            rendering_policy: String::from("Software only"),
            title_bar_style: crate::ui::title_bar::ControlStyle::Windows,
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    fn debug_window_control_switch_emits_the_preview_action_for_both_styles() {
        let renderer = renderer();
        for style in [
            crate::ui::title_bar::ControlStyle::Windows,
            crate::ui::title_bar::ControlStyle::MacOS,
        ] {
            let mut info = rendering_info();
            info.title_bar_style = style;
            let mut content = title_bar_preview(&info, 1.0);
            let (mut tree, node) = mount_in(&mut content, &renderer, Size::new(540.0, 100.0));
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
                    mouse::Cursor::Available(Point::new(480.0, 24.0)),
                    &renderer,
                    &mut shell,
                    &VIEWPORT,
                );
            }
            assert!(matches!(
                messages.as_slice(),
                [Message::ToggleTitleBarStyle]
            ));
        }
    }

    fn mount(content: &mut Element<'_, Message>, renderer: &Renderer) -> (Tree, layout::Node) {
        mount_in(content, renderer, VIEWPORT.size())
    }

    fn mount_in(
        content: &mut Element<'_, Message>,
        renderer: &Renderer,
        size: Size,
    ) -> (Tree, layout::Node) {
        let mut tree = Tree::empty();
        tree.diff(content.as_widget_mut());
        let node = content.as_widget_mut().layout(
            &mut tree,
            renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        (tree, node)
    }

    #[derive(Default)]
    struct DialogLayout {
        close: Option<Rectangle>,
        scroll_regions: Vec<(Rectangle, Rectangle)>,
    }

    impl Operation for DialogLayout {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }

        fn text(&mut self, _id: Option<&widget::Id>, bounds: Rectangle, text: &str) {
            if text == "Close" {
                self.close = Some(bounds);
            }
        }

        fn scrollable(
            &mut self,
            _id: Option<&widget::Id>,
            bounds: Rectangle,
            content_bounds: Rectangle,
            _translation: Vector,
            _state: &mut dyn operation::Scrollable,
        ) {
            self.scroll_regions.push((bounds, content_bounds));
        }
    }

    fn inspect_layout(
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
    ) -> DialogLayout {
        let mut inspection = DialogLayout::default();
        content
            .as_widget_mut()
            .operate(tree, Layout::new(node), renderer, &mut inspection);
        inspection
    }

    #[test]
    fn small_windows_keep_the_footer_fixed_and_content_scrollable() {
        let renderer = renderer();
        for size in [Size::new(480.0, 360.0), Size::new(640.0, 480.0)] {
            let mut footer = None;
            for tab in [AboutTab::About, AboutTab::Debug, AboutTab::Licenses] {
                let mut content = view(tab, rendering_info(), 1.0, true);
                let (mut tree, node) = mount_in(&mut content, &renderer, size);
                let inspection = inspect_layout(&mut content, &mut tree, &node, &renderer);
                let close = inspection.close.expect("Close control must be present");
                assert!(close.width > 0.0 && close.height > 0.0);
                assert!(
                    close.x >= 0.0
                        && close.y >= 0.0
                        && close.x + close.width <= size.width
                        && close.y + close.height <= size.height,
                    "{tab:?}: Close must stay inside {size:?}, got {close:?}"
                );
                if let Some(expected) = footer {
                    assert_eq!(
                        close, expected,
                        "switching tabs must not move the footer at {size:?}"
                    );
                } else {
                    footer = Some(close);
                }
                assert_eq!(
                    inspection.scroll_regions.len(),
                    1,
                    "each tab must have one scrolling body"
                );
                let (viewport, body) = inspection.scroll_regions[0];
                assert!(
                    viewport.width > 0.0 && viewport.height > 0.0,
                    "{tab:?}: scrolling body needs visible space"
                );
                assert!(
                    viewport.x >= 0.0
                        && viewport.x + viewport.width <= size.width
                        && viewport.y >= 0.0
                        && viewport.y + viewport.height <= close.y,
                    "{tab:?}: scrolling body must fit above the fixed footer"
                );
                if !matches!(tab, AboutTab::About) {
                    assert!(
                        body.height > viewport.height,
                        "{tab:?}: long content must remain available through scrolling"
                    );
                }
            }
        }
    }

    #[test]
    fn all_tabs_fade_their_paint_without_replacing_the_backdrop() {
        let mut renderer = renderer();
        let backdrop = Color::from_rgb8(23, 61, 97);
        renderer.reset(VIEWPORT);
        let background = renderer.screenshot(Size::new(800, 600), 1.0, backdrop);

        for theme in [Theme::Light, Theme::Dark] {
            for tab in [AboutTab::About, AboutTab::Debug, AboutTab::Licenses] {
                let mut snapshots = Vec::new();
                for progress in [0.0, 0.5, 1.0] {
                    let mut content = view(tab, rendering_info(), progress, false);
                    let (tree, node) = mount(&mut content, &renderer);
                    renderer.reset(VIEWPORT);
                    content.as_widget().draw(
                        &tree,
                        &mut renderer,
                        &theme,
                        &renderer::Style::default(),
                        Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &VIEWPORT,
                    );
                    snapshots.push(renderer.screenshot(Size::new(800, 600), 1.0, backdrop));
                }

                assert!(
                    snapshots[0] == background,
                    "{theme:?}/{tab:?}: zero opacity must preserve every backdrop pixel"
                );
                assert!(
                    snapshots[1] != background,
                    "{theme:?}/{tab:?}: intermediate opacity must be visible"
                );
                assert!(
                    snapshots[1] != snapshots[2],
                    "{theme:?}/{tab:?}: intermediate opacity must differ from the settled dialog"
                );
                assert!(
                    snapshots[2] != background,
                    "{theme:?}/{tab:?}: settled dialog must remain visible"
                );
            }
        }
    }

    #[test]
    fn closing_modal_blocks_background_clicks_and_disables_its_buttons() {
        let renderer = renderer();
        for interactive in [true, false] {
            for on_close in [false, true] {
                let mut content: Element<'_, Message> = stack![
                    button(space::vertical().width(Fill).height(Fill))
                        .width(Fill)
                        .height(Fill)
                        .on_press(Message::NewFile),
                    view(AboutTab::About, rendering_info(), 0.5, interactive),
                ]
                .into();
                let (mut tree, node) = mount(&mut content, &renderer);
                let point = if on_close {
                    inspect_layout(&mut content, &mut tree, &node, &renderer)
                        .close
                        .expect("Close control must be present")
                        .center()
                } else {
                    Point::new(10.0, 10.0)
                };
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
                        mouse::Cursor::Available(point),
                        &renderer,
                        &mut shell,
                        &VIEWPORT,
                    );
                }
                assert!(
                    !messages
                        .iter()
                        .any(|message| matches!(message, Message::NewFile)),
                    "modal must block background clicks while fading"
                );
                if interactive && on_close {
                    assert!(
                        messages
                            .iter()
                            .any(|message| matches!(message, Message::AboutClosed)),
                        "control must be clickable before closing"
                    );
                } else {
                    assert!(
                        messages.is_empty(),
                        "closing controls must not publish actions: {messages:?}"
                    );
                }
            }
        }
    }
}
