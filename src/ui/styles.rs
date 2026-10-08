use iced::widget::{button, container, text, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

pub const RADIUS: f32 = 6.0;
pub fn accent_color(theme: &Theme) -> Color {
    VisualPalette::from_theme(theme).accent
}

pub(super) fn search_light_colors(theme: &Theme) -> [Color; 3] {
    let palette = VisualPalette::from_theme(theme);
    [palette.accent, palette.success, palette.danger]
}

pub(super) fn search_activity_color(theme: &Theme) -> Color {
    let palette = VisualPalette::from_theme(theme);
    if palette.is_dark {
        Color::from_rgb8(80, 225, 225)
    } else {
        palette.accent
    }
}

pub fn checkbox(
    theme: &Theme,
    status: iced::widget::checkbox::Status,
) -> iced::widget::checkbox::Style {
    use iced::widget::checkbox::Status;
    let p = VisualPalette::from_theme(theme);
    let (checked, hovered, disabled) = match status {
        Status::Active { is_checked } => (is_checked, false, false),
        Status::Hovered { is_checked } => (is_checked, true, false),
        Status::Disabled { is_checked } => (is_checked, false, true),
    };
    let alpha = if disabled { 0.5 } else { 1.0 };
    iced::widget::checkbox::Style {
        background: (if checked {
            p.accent
        } else if hovered {
            p.surface_low
        } else {
            p.surface
        })
        .scale_alpha(alpha)
        .into(),
        icon_color: p.accent_text,
        border: border(
            1.0,
            (if checked || hovered {
                p.accent
            } else {
                p.border
            })
            .scale_alpha(alpha),
            2.0,
        ),
        text_color: Some(if disabled { p.faint_text } else { p.text }),
    }
}

pub fn toggler(
    theme: &Theme,
    status: iced::widget::toggler::Status,
) -> iced::widget::toggler::Style {
    use iced::widget::toggler::Status;
    let p = VisualPalette::from_theme(theme);
    let (toggled, hovered, disabled) = match status {
        Status::Active { is_toggled } => (is_toggled, false, false),
        Status::Hovered { is_toggled } => (is_toggled, true, false),
        Status::Disabled { is_toggled } => (is_toggled, false, true),
    };
    iced::widget::toggler::Style {
        background: (if toggled {
            if hovered {
                p.accent.mix(p.text, 0.12)
            } else {
                p.accent
            }
        } else if hovered {
            p.muted_text
        } else {
            p.border
        })
        .scale_alpha(if disabled { 0.5 } else { 1.0 })
        .into(),
        foreground: (if disabled {
            p.faint_text
        } else {
            p.switch_thumb
        })
        .into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        text_color: Some(if disabled { p.faint_text } else { p.text }),
        border_radius: None,
        padding_ratio: 0.1,
    }
}

pub fn scrollable(
    theme: &Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable::{AutoScroll, Rail, Scroller, Status, Style};
    let p = VisualPalette::from_theme(theme);
    let (horizontal, vertical) = match status {
        Status::Active { .. } => (false, false),
        Status::Hovered {
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
            ..
        } => (
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
        ),
        Status::Dragged {
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
            ..
        } => (
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
        ),
    };
    let rail = |active| Rail {
        background: None,
        border: border(0.0, Color::TRANSPARENT, 2.0),
        scroller: Scroller {
            background: (if active {
                p.muted_text
            } else {
                p.faint_text.scale_alpha(0.65)
            })
            .into(),
            border: border(0.0, Color::TRANSPARENT, 2.0),
        },
    };
    Style {
        container: container::Style::default(),
        vertical_rail: rail(vertical),
        horizontal_rail: rail(horizontal),
        gap: None,
        auto_scroll: AutoScroll {
            background: p.overlay.scale_alpha(0.9).into(),
            border: border(1.0, p.border, f32::MAX),
            shadow: elevation(p, 0.0, 2.0),
            icon: p.text,
        },
    }
}
const CONTROL_RADIUS: f32 = 5.0;
const TAB_RADIUS: f32 = 4.0;

pub fn title_bar(theme: &Theme, focused: bool) -> container::Style {
    let palette = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(palette.chrome.into()),
        text_color: Some(if focused {
            palette.text
        } else {
            palette.muted_text
        }),
        ..Default::default()
    }
}

pub fn window_frame(theme: &Theme, focused: bool) -> container::Style {
    let palette = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(palette.chrome.into()),
        border: Border {
            width: 1.0,
            color: if focused {
                palette.border
            } else {
                palette.border_soft
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn caption_button(
    theme: &Theme,
    status: button::Status,
    mac: bool,
    close: bool,
    focused: bool,
) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let active = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if mac {
        None
    } else if active && close {
        Some(Color::from_rgb8(196, 43, 28).into())
    } else if active {
        Some(palette.surface_high.into())
    } else {
        None
    };
    button::Style {
        background,
        text_color: if active && close && !mac {
            Color::WHITE
        } else if focused {
            palette.text
        } else {
            palette.muted_text
        },
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabDragVisual {
    Idle,
    Dragged,
    ValidTarget,
    InvalidTarget,
}

#[derive(Debug, Clone, Copy)]
struct VisualPalette {
    app: Color,
    chrome: Color,
    chrome_high: Color,
    surface: Color,
    surface_low: Color,
    surface_high: Color,
    overlay: Color,
    selected: Color,
    switch_thumb: Color,
    text: Color,
    muted_text: Color,
    faint_text: Color,
    border: Color,
    border_soft: Color,
    accent: Color,
    accent_soft: Color,
    accent_text: Color,
    success: Color,
    success_soft: Color,
    danger: Color,
    danger_soft: Color,
    shadow: Color,
    selection: Color,
    is_dark: bool,
}

impl VisualPalette {
    fn from_theme(theme: &Theme) -> Self {
        if theme.palette().is_dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    fn light() -> Self {
        Self {
            app: Color::from_rgb8(247, 247, 247),
            chrome: Color::from_rgb8(240, 240, 240),
            chrome_high: Color::from_rgb8(250, 250, 250),
            surface: Color::from_rgb8(255, 255, 255),
            surface_low: Color::from_rgb8(245, 245, 245),
            surface_high: Color::from_rgb8(237, 238, 240),
            overlay: Color::from_rgb8(255, 255, 255),
            selected: Color::from_rgb8(230, 231, 233),
            switch_thumb: Color::WHITE,
            text: Color::from_rgb8(32, 33, 35),
            muted_text: Color::from_rgb8(96, 98, 102),
            faint_text: Color::from_rgb8(128, 131, 136),
            border: Color::from_rgb8(195, 197, 201),
            border_soft: Color::from_rgb8(225, 226, 228),
            accent: Color::from_rgb8(0, 112, 204),
            accent_soft: Color::from_rgb8(232, 242, 255),
            accent_text: Color::WHITE,
            success: Color::from_rgb8(27, 128, 79),
            success_soft: Color::from_rgb8(221, 244, 232),
            danger: Color::from_rgb8(190, 45, 65),
            danger_soft: Color::from_rgb8(255, 229, 233),
            shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.12),
            selection: Color::from_rgba(0.0, 112.0 / 255.0, 204.0 / 255.0, 0.24),
            is_dark: false,
        }
    }

    fn dark() -> Self {
        Self {
            app: Color::from_rgb8(29, 30, 32),
            chrome: Color::from_rgb8(35, 36, 39),
            chrome_high: Color::from_rgb8(39, 40, 43),
            surface: Color::from_rgb8(26, 27, 29),
            surface_low: Color::from_rgb8(33, 34, 37),
            surface_high: Color::from_rgb8(47, 48, 52),
            overlay: Color::from_rgb8(37, 38, 41),
            selected: Color::from_rgb8(57, 59, 64),
            switch_thumb: Color::from_rgb8(250, 250, 250),
            text: Color::from_rgb8(232, 233, 235),
            muted_text: Color::from_rgb8(172, 175, 181),
            faint_text: Color::from_rgb8(131, 135, 142),
            border: Color::from_rgb8(79, 82, 88),
            border_soft: Color::from_rgb8(53, 55, 60),
            accent: Color::from_rgb8(64, 156, 255),
            accent_soft: Color::from_rgb8(32, 51, 74),
            accent_text: Color::from_rgb8(20, 30, 43),
            success: Color::from_rgb8(93, 214, 145),
            success_soft: Color::from_rgb8(31, 71, 51),
            danger: Color::from_rgb8(255, 121, 137),
            danger_soft: Color::from_rgb8(86, 39, 48),
            shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.34),
            selection: Color::from_rgba(64.0 / 255.0, 156.0 / 255.0, 1.0, 0.28),
            is_dark: true,
        }
    }
}

pub fn modern_theme(appearance: crate::core::AppearanceMode) -> Option<Theme> {
    match appearance {
        // No override lets Iced use the OS theme at startup and follow changes.
        crate::core::AppearanceMode::System => None,
        crate::core::AppearanceMode::Dark => Some(Theme::custom(
            "Fragile Modern Dark",
            iced::theme::palette::Seed {
                background: Color::from_rgb8(29, 30, 32),
                text: Color::from_rgb8(232, 233, 235),
                primary: Color::from_rgb8(64, 156, 255),
                success: Color::from_rgb8(93, 214, 145),
                warning: Color::from_rgb8(245, 190, 91),
                danger: Color::from_rgb8(255, 121, 137),
            },
        )),
        crate::core::AppearanceMode::Light => Some(Theme::custom(
            "Fragile Modern Light",
            iced::theme::palette::Seed {
                background: Color::from_rgb8(247, 247, 247),
                text: Color::from_rgb8(32, 33, 35),
                primary: Color::from_rgb8(0, 112, 204),
                success: Color::from_rgb8(27, 128, 79),
                warning: Color::from_rgb8(181, 118, 20),
                danger: Color::from_rgb8(190, 45, 65),
            },
        )),
    }
}

fn border(width: f32, color: Color, radius: f32) -> Border {
    Border {
        radius: radius.into(),
        width,
        color,
    }
}

fn hairline(color: Color) -> Border {
    border(1.0, color, 0.0)
}

fn elevation(palette: VisualPalette, y: f32, blur: f32) -> Shadow {
    Shadow {
        color: palette.shadow,
        offset: Vector::new(0.0, y),
        blur_radius: blur,
    }
}

pub fn app_shell(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.app)),
        text_color: Some(palette.text),
        ..container::Style::default()
    }
}

pub fn menu_bar(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.chrome)),
        text_color: Some(palette.text),
        ..container::Style::default()
    }
}

pub fn tool_bar(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.chrome)),
        text_color: Some(palette.text),
        border: hairline(palette.border_soft),
        ..container::Style::default()
    }
}

pub fn tab_strip(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.chrome)),
        text_color: Some(palette.text),
        border: hairline(palette.border_soft),
        ..container::Style::default()
    }
}

pub fn tab_container(
    is_active: bool,
    drag_visual: TabDragVisual,
) -> impl Fn(&Theme) -> container::Style {
    move |theme| tab_container_style(theme, is_active, drag_visual)
}

pub fn tab_container_style(
    theme: &Theme,
    is_active: bool,
    drag_visual: TabDragVisual,
) -> container::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = match drag_visual {
        TabDragVisual::Dragged => palette.accent_soft,
        TabDragVisual::ValidTarget => palette.success_soft,
        TabDragVisual::InvalidTarget => palette.danger_soft,
        TabDragVisual::Idle if is_active => palette.surface,
        TabDragVisual::Idle => palette.chrome,
    };
    let border_color = match drag_visual {
        TabDragVisual::Dragged => palette.accent,
        TabDragVisual::ValidTarget => palette.success,
        TabDragVisual::InvalidTarget => palette.danger,
        TabDragVisual::Idle if is_active => palette.border,
        TabDragVisual::Idle => palette.border_soft,
    };

    container::Style {
        background: Some(Background::Color(background)),
        text_color: Some(palette.text),
        border: Border {
            width: if matches!(drag_visual, TabDragVisual::Idle) {
                1.0
            } else {
                2.0
            },
            color: border_color,
            radius: TAB_RADIUS.into(),
        },
        shadow: if is_active {
            elevation(palette, 1.0, 5.0)
        } else {
            Shadow::default()
        },
        ..container::Style::default()
    }
}

pub fn tab_top_bar_style(theme: &Theme, is_active: bool, is_dragged: bool) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(if is_dragged || is_active {
            palette.accent
        } else {
            Color::TRANSPARENT
        })),
        ..container::Style::default()
    }
}

pub fn tab_top_bar(is_active: bool, is_dragged: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| tab_top_bar_style(theme, is_active, is_dragged)
}

pub fn tab_title_area(is_active: bool, is_dragged: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let palette = VisualPalette::from_theme(theme);
        let background = if is_dragged {
            palette.accent_soft
        } else if is_active {
            palette.surface
        } else {
            palette.chrome
        };

        container::Style {
            background: Some(Background::Color(background)),
            text_color: Some(if is_active {
                palette.text
            } else {
                palette.muted_text
            }),
            border: border(0.0, Color::TRANSPARENT, TAB_RADIUS),
            ..container::Style::default()
        }
    }
}

pub fn tab_active_edge(is_active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let palette = VisualPalette::from_theme(theme);

        container::Style {
            background: Some(Background::Color(if is_active {
                palette.accent
            } else {
                Color::TRANSPARENT
            })),
            ..container::Style::default()
        }
    }
}

pub fn utility_bar_background(theme: &Theme) -> Color {
    VisualPalette::from_theme(theme).chrome_high
}

pub fn editor_background(theme: &Theme) -> Color {
    VisualPalette::from_theme(theme).surface
}

pub fn utility_bar(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.chrome_high)),
        text_color: Some(palette.text),
        border: hairline(palette.border_soft),
        ..container::Style::default()
    }
}

pub fn settings_panel(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.app)),
        text_color: Some(palette.text),
        border: border(0.0, Color::TRANSPARENT, 0.0),
        ..container::Style::default()
    }
}

pub fn settings_panel_background(theme: &Theme) -> Color {
    VisualPalette::from_theme(theme).app
}

pub fn listening_notice(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.accent_soft)),
        text_color: Some(palette.accent),
        border: border(1.0, palette.accent.scale_alpha(0.42), 8.0),
        ..container::Style::default()
    }
}

/// Shared surfaces for the window picker, search workspace, and preferences.
pub fn utility_dialog(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(palette.overlay.into()),
        text_color: Some(palette.text),
        border: border(1.0, palette.border_soft, 12.0),
        shadow: elevation(palette, 10.0, 32.0),
        ..Default::default()
    }
}

pub fn utility_card(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(palette.surface.into()),
        text_color: Some(palette.text),
        border: border(1.0, palette.border_soft.scale_alpha(0.65), 8.0),
        ..Default::default()
    }
}

pub fn appearance_preview_frame(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(palette.surface_low.into()),
        border: border(1.0, palette.faint_text.scale_alpha(0.65), 7.0),
        ..Default::default()
    }
}

pub fn utility_notice(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(palette.danger_soft.into()),
        text_color: Some(palette.danger),
        border: border(1.0, palette.danger.scale_alpha(0.4), 8.0),
        ..Default::default()
    }
}

pub fn utility_rule(theme: &Theme) -> iced::widget::rule::Style {
    iced::widget::rule::Style {
        color: VisualPalette::from_theme(theme).border_soft,
        radius: 0.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: true,
    }
}

pub fn utility_selection(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let highlighted = selected || matches!(status, button::Status::Pressed);
        button::Style {
            background: Some(
                if highlighted {
                    palette.selected
                } else if matches!(status, button::Status::Hovered) {
                    palette.surface_high
                } else {
                    palette.surface
                }
                .into(),
            ),
            text_color: palette.text,
            border: border(
                1.0,
                if selected {
                    palette.accent.scale_alpha(0.65)
                } else {
                    palette.border_soft
                },
                8.0,
            ),
            ..Default::default()
        }
    }
}

pub fn settings_category_list(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.chrome)),
        text_color: Some(palette.muted_text),
        ..container::Style::default()
    }
}

pub fn settings_content(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface)),
        text_color: Some(palette.text),
        ..container::Style::default()
    }
}

pub fn editor_surface(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface)),
        text_color: Some(palette.text),
        ..container::Style::default()
    }
}

pub fn function_list_panel(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface)),
        text_color: Some(palette.text),
        border: hairline(palette.border_soft),
        ..container::Style::default()
    }
}

pub fn function_list_header(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.chrome_high)),
        text_color: Some(palette.text),
        ..container::Style::default()
    }
}

pub fn function_list_count(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface_low)),
        text_color: Some(palette.muted_text),
        border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
        ..container::Style::default()
    }
}

pub fn function_list_secondary(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(VisualPalette::from_theme(theme).muted_text),
    }
}

pub fn function_list_kind_label(
    kind: crate::editor::outline::OutlineNodeKind,
) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        use crate::editor::outline::OutlineNodeKind;

        let palette = VisualPalette::from_theme(theme);
        // Related symbols share a color family, with a separate tone per kind.
        // Dark text on light surfaces and pastel text on dark surfaces keep the
        // small badge labels readable, including on selected rows.
        let (light, dark) = match kind {
            OutlineNodeKind::Function => ((0, 93, 184), (109, 180, 255)),
            OutlineNodeKind::Method => ((24, 111, 63), (104, 211, 145)),
            OutlineNodeKind::Constructor => ((77, 111, 15), (180, 210, 104)),
            OutlineNodeKind::Declaration => ((76, 97, 126), (167, 187, 216)),
            OutlineNodeKind::Module => ((119, 70, 171), (193, 157, 245)),
            OutlineNodeKind::Namespace => ((147, 57, 142), (223, 155, 215)),
            OutlineNodeKind::Class => ((132, 93, 12), (229, 195, 106)),
            OutlineNodeKind::Enum => ((161, 74, 16), (244, 172, 102)),
            OutlineNodeKind::EnumMember => ((158, 67, 54), (240, 156, 138)),
            OutlineNodeKind::Interface => ((12, 110, 133), (98, 207, 226)),
            OutlineNodeKind::Trait => ((13, 116, 108), (100, 210, 193)),
            OutlineNodeKind::Impl => ((76, 79, 166), (160, 166, 245)),
            OutlineNodeKind::Tag => ((156, 54, 103), (237, 145, 183)),
            OutlineNodeKind::Section => ((110, 80, 126), (199, 172, 217)),
            OutlineNodeKind::Unknown => ((96, 98, 102), (172, 175, 181)),
        };
        let (r, g, b) = if palette.is_dark { dark } else { light };
        let foreground = Color::from_rgb8(r, g, b);
        let background = palette
            .surface
            .mix(foreground, if palette.is_dark { 0.10 } else { 0.09 });
        container::Style {
            background: Some(background.into()),
            text_color: Some(foreground),
            border: border(0.0, Color::TRANSPARENT, 4.0),
            ..Default::default()
        }
    }
}

pub fn function_list_entry(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = match status {
            button::Status::Pressed => Some(palette.selected.into()),
            button::Status::Hovered => Some(
                if active {
                    palette.surface_high
                } else {
                    palette.surface_low
                }
                .into(),
            ),
            _ if active => Some(palette.selected.into()),
            _ => None,
        };
        button::Style {
            background,
            text_color: if active { palette.accent } else { palette.text },
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..Default::default()
        }
    }
}

pub fn find_status(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface_low)),
        text_color: Some(palette.muted_text),
        border: border(1.0, palette.border_soft, CONTROL_RADIUS),
        ..container::Style::default()
    }
}

/// Compact search controls share their surface with the query field.
pub fn search_field(theme: &Theme) -> container::Style {
    let p = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(p.surface.into()),
        text_color: Some(p.text),
        border: border(1.0, p.border, 7.0),
        ..Default::default()
    }
}

pub fn search_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = input(theme, status);
    style.border = if matches!(status, text_input::Status::Focused { .. }) {
        border(1.0, VisualPalette::from_theme(theme).accent, 4.0)
    } else {
        border(0.0, Color::TRANSPARENT, 4.0)
    };
    style
}

pub fn search_option(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = VisualPalette::from_theme(theme);
        let disabled = matches!(status, button::Status::Disabled);
        button::Style {
            background: Some(
                if active && !disabled {
                    p.accent_soft
                } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    p.surface_high
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: if disabled {
                p.faint_text
            } else if active {
                p.accent
            } else {
                p.muted_text
            },
            border: border(
                1.0,
                if active && !disabled {
                    p.accent.scale_alpha(0.3)
                } else {
                    Color::TRANSPARENT
                },
                4.0,
            ),
            ..Default::default()
        }
    }
}

pub fn search_status(error: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = VisualPalette::from_theme(theme);
        container::Style {
            text_color: Some(if error { p.danger } else { p.text }),
            ..Default::default()
        }
    }
}

pub fn search_bar(theme: &Theme) -> container::Style {
    let p = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(p.chrome_high.into()),
        text_color: Some(p.text),
        border: border(1.0, p.border_soft, 8.0),
        ..Default::default()
    }
}

pub fn search_result(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = VisualPalette::from_theme(theme);
        button::Style {
            background: Some(
                if selected {
                    p.accent_soft
                } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    p.surface_high
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: p.text,
            border: border(
                1.0,
                if selected {
                    p.accent.scale_alpha(0.3)
                } else {
                    Color::TRANSPARENT
                },
                4.0,
            ),
            ..Default::default()
        }
    }
}

pub fn search_result_group(theme: &Theme) -> container::Style {
    let p = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(p.surface_low.into()),
        text_color: Some(p.muted_text),
        border: border(0.0, Color::TRANSPARENT, 4.0),
        ..Default::default()
    }
}

pub fn search_empty_icon(theme: &Theme) -> container::Style {
    let p = VisualPalette::from_theme(theme);
    container::Style {
        background: Some(p.accent_soft.into()),
        text_color: Some(p.accent),
        border: border(0.0, Color::TRANSPARENT, 12.0),
        ..Default::default()
    }
}

pub fn modal_scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(iced::Color::from_rgba(
            0.0, 0.0, 0.0, 0.38,
        ))),
        ..container::Style::default()
    }
}

pub fn modal_dialog(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.overlay)),
        text_color: Some(palette.text),
        border: border(1.0, palette.border_soft, RADIUS),
        shadow: elevation(palette, 8.0, 24.0),
        ..container::Style::default()
    }
}

pub fn info_dialog(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.overlay)),
        text_color: Some(palette.text),
        border: border(1.0, palette.border_soft, 12.0),
        // The software renderer rebuilds a blurred shadow on every header
        // animation frame. The scrim and border already separate this panel.
        ..container::Style::default()
    }
}

pub fn info_card(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface_low)),
        text_color: Some(palette.text),
        border: border(1.0, palette.border_soft, 8.0),
        ..container::Style::default()
    }
}

pub fn info_muted(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        text_color: Some(palette.muted_text),
        ..container::Style::default()
    }
}

pub fn info_badge(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.accent_soft)),
        text_color: Some(palette.accent),
        border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
        ..container::Style::default()
    }
}

pub fn info_tab(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = if active {
        Some(Background::Color(palette.surface))
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Some(Background::Color(palette.surface_low))
    } else {
        None
    };

    button::Style {
        background,
        text_color: if matches!(status, button::Status::Disabled) {
            palette.faint_text
        } else if active {
            palette.accent
        } else {
            palette.muted_text
        },
        border: border(
            if active { 1.0 } else { 0.0 },
            palette.border_soft,
            CONTROL_RADIUS,
        ),
        ..button::Style::default()
    }
}

pub fn tooltip(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(if palette.is_dark {
            palette.surface_high
        } else {
            Color::from_rgb8(39, 46, 56)
        })),
        text_color: Some(if palette.is_dark {
            palette.text
        } else {
            Color::WHITE
        }),
        border: border(1.0, palette.border_soft.scale_alpha(0.7), CONTROL_RADIUS),
        shadow: elevation(palette, 3.0, 12.0),
        ..container::Style::default()
    }
}

pub fn status_bar(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.chrome_high)),
        text_color: Some(palette.muted_text),
        border: hairline(palette.border_soft),
        ..container::Style::default()
    }
}

pub fn status_segment(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface_low)),
        text_color: Some(palette.muted_text),
        border: border(1.0, palette.border_soft, 4.0),
        ..container::Style::default()
    }
}

pub fn status_path(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: None,
        text_color: Some(palette.muted_text),
        ..container::Style::default()
    }
}

pub fn separator(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.border_soft)),
        border: border(0.0, Color::TRANSPARENT, 0.0),
        ..container::Style::default()
    }
}

pub fn menu_button(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = match (is_active, status) {
            (true, _) | (_, button::Status::Hovered) | (_, button::Status::Pressed) => {
                Some(Background::Color(palette.surface_low))
            }
            _ => None,
        };

        button::Style {
            background,
            text_color: palette.text,
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn menu_label(is_active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let palette = VisualPalette::from_theme(theme);

        container::Style {
            background: if is_active {
                Some(Background::Color(palette.surface_low))
            } else {
                None
            },
            text_color: Some(palette.text),
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..container::Style::default()
        }
    }
}

pub fn transparent(_theme: &Theme) -> container::Style {
    container::Style {
        background: None,
        text_color: Some(iced::Color::TRANSPARENT),
        ..container::Style::default()
    }
}

pub fn menu_dropdown_band(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.overlay)),
        text_color: Some(palette.text),
        border: border(1.0, palette.border_soft, RADIUS),
        shadow: elevation(palette, 5.0, 18.0),
        ..container::Style::default()
    }
}

pub fn menu_dropdown_item(theme: &Theme, status: button::Status) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = match status {
        button::Status::Hovered => Some(Background::Color(palette.selected)),
        button::Status::Pressed => Some(Background::Color(palette.selected)),
        _ => None,
    };

    button::Style {
        background,
        text_color: palette.text,
        border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn menu_shortcut_hint(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(menu_shortcut_hint_color(theme)),
    }
}

pub fn menu_shortcut_hint_color(theme: &Theme) -> Color {
    let palette = VisualPalette::from_theme(theme);

    palette.faint_text
}

pub fn shortcut_text_color(theme: &Theme) -> Color {
    let palette = VisualPalette::from_theme(theme);

    palette.text
}

pub fn menu_submenu_item(is_active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let palette = VisualPalette::from_theme(theme);

        container::Style {
            background: if is_active {
                Some(Background::Color(palette.selected))
            } else {
                None
            },
            text_color: Some(palette.text),
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..container::Style::default()
        }
    }
}

pub fn menu_dropdown_disabled(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: None,
        text_color: Some(palette.faint_text),
        ..container::Style::default()
    }
}

pub fn dropdown_trigger(
    theme: &Theme,
    is_open: bool,
    is_focused: bool,
    status: button::Status,
) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let highlighted = is_open || is_focused;

    button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed => palette.surface_low,
            _ => palette.surface,
        })),
        text_color: palette.text,
        border: border(
            1.0,
            if highlighted {
                palette.accent
            } else if matches!(status, button::Status::Hovered) {
                palette.border
            } else {
                palette.border_soft
            },
            CONTROL_RADIUS,
        ),
        ..button::Style::default()
    }
}

pub fn dropdown_option(
    is_selected: bool,
    is_highlighted: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = if is_highlighted
            || matches!(status, button::Status::Hovered | button::Status::Pressed)
        {
            palette.selected
        } else if is_selected {
            palette.surface_low
        } else {
            Color::TRANSPARENT
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if is_selected {
                palette.accent
            } else {
                palette.text
            },
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn dropdown_menu(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.overlay)),
        text_color: Some(palette.text),
        border: border(1.0, palette.border_soft, RADIUS),
        shadow: elevation(palette, 4.0, 14.0),
        ..container::Style::default()
    }
}

pub fn tool_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = match status {
        button::Status::Active => palette.surface_low,
        button::Status::Hovered => palette.surface_high,
        button::Status::Pressed => palette.selected,
        button::Status::Disabled => palette.surface_low.scale_alpha(0.55),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: palette.text,
        border: border(1.0, palette.border_soft, CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn icon_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = match status {
        button::Status::Active => Color::TRANSPARENT,
        button::Status::Hovered => palette.surface_high,
        button::Status::Pressed => palette.selected,
        button::Status::Disabled => Color::TRANSPARENT,
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: if matches!(status, button::Status::Disabled) {
            palette.faint_text
        } else {
            palette.text
        },
        border: border(
            1.0,
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                palette.border
            } else {
                Color::TRANSPARENT
            },
            CONTROL_RADIUS,
        ),
        ..button::Style::default()
    }
}

pub fn settings_category_button(
    is_active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = match (is_active, status) {
            (true, _) => palette.surface,
            (_, button::Status::Hovered) => palette.surface_high,
            (_, button::Status::Pressed) => palette.selected,
            _ => Color::TRANSPARENT,
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if is_active {
                palette.accent
            } else {
                palette.muted_text
            },
            border: border(
                1.0,
                if is_active {
                    palette.border_soft
                } else {
                    Color::TRANSPARENT
                },
                CONTROL_RADIUS,
            ),
            ..button::Style::default()
        }
    }
}

/// Compact segmented tabs used by utility dialogs such as Find/Replace.
pub fn dialog_tab_group(theme: &Theme) -> container::Style {
    let palette = VisualPalette::from_theme(theme);

    container::Style {
        background: Some(Background::Color(palette.surface_low)),
        border: border(1.0, palette.border_soft, 8.0),
        ..container::Style::default()
    }
}

pub fn dialog_tab_button(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = match (is_active, status) {
            (true, _) => palette.selected,
            (false, button::Status::Hovered) => palette.surface_high,
            (false, button::Status::Pressed) => palette.selected,
            _ => Color::TRANSPARENT,
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if is_active {
                palette.accent
            } else {
                palette.muted_text
            },
            border: border(
                1.0,
                if is_active {
                    palette.border_soft
                } else {
                    Color::TRANSPARENT
                },
                CONTROL_RADIUS,
            ),
            ..button::Style::default()
        }
    }
}

/// Navigation tabs in the Preferences sidebar.
pub fn settings_navigation_button(
    is_active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = match (is_active, status) {
            (true, _) => palette.accent_soft,
            (false, button::Status::Hovered) => palette.surface_high,
            (false, button::Status::Pressed) => palette.selected,
            _ => Color::TRANSPARENT,
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if is_active {
                palette.accent
            } else {
                palette.muted_text
            },
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn command_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = match status {
        button::Status::Active => palette.surface_low,
        button::Status::Hovered => palette.surface_high,
        button::Status::Pressed => palette.selected,
        button::Status::Disabled => palette.surface_low.scale_alpha(0.55),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: if matches!(status, button::Status::Disabled) {
            palette.faint_text
        } else {
            palette.text
        },
        border: border(1.0, palette.border_soft, CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn primary_command_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = match status {
        button::Status::Active => palette.accent,
        button::Status::Hovered => palette.accent.mix(palette.surface_high, 0.12),
        button::Status::Pressed => palette
            .accent
            .mix(Color::BLACK, if palette.is_dark { 0.08 } else { 0.16 }),
        button::Status::Disabled => palette.accent.scale_alpha(0.5),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: palette.accent_text,
        border: border(1.0, palette.accent, CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn listening_command_button(pulse: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    let pulse = pulse.clamp(0.0, 1.0);
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let mut style = primary_command_button(theme, status);
        style.border = border(1.0, palette.accent, CONTROL_RADIUS);
        style.shadow = Shadow {
            color: palette.accent.scale_alpha(0.14 + pulse * 0.2),
            offset: Vector::new(0.0, 0.0),
            blur_radius: 3.0 + pulse * 7.0,
        };
        style
    }
}

pub fn danger_command_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let background = match status {
        button::Status::Active => palette.danger_soft,
        button::Status::Hovered => palette.danger,
        button::Status::Pressed => palette.danger.mix(Color::BLACK, 0.16),
        button::Status::Disabled => palette.danger_soft.scale_alpha(0.45),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Color::WHITE
        } else {
            palette.danger
        },
        border: border(1.0, palette.danger.scale_alpha(0.52), CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn text_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = VisualPalette::from_theme(theme);
    let text_color = match status {
        button::Status::Active | button::Status::Pressed => palette.muted_text,
        button::Status::Hovered => palette.accent,
        button::Status::Disabled => palette.faint_text,
    };

    button::Style {
        text_color,
        border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn tab_button(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = if is_active {
            palette.surface
        } else {
            match status {
                button::Status::Hovered => palette.surface_high,
                button::Status::Pressed => palette.selected,
                _ => palette.chrome,
            }
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: palette.text,
            border: border(
                1.0,
                if is_active {
                    palette.border
                } else {
                    palette.border_soft
                },
                TAB_RADIUS,
            ),
            ..button::Style::default()
        }
    }
}

pub fn tab_close_button(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = match (is_active, status) {
            (_, button::Status::Hovered) => palette.danger_soft,
            (_, button::Status::Pressed) => palette.danger,
            (true, _) => palette.surface,
            (false, _) => palette.chrome,
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if matches!(status, button::Status::Pressed) {
                Color::WHITE
            } else if is_active || matches!(status, button::Status::Hovered) {
                palette.text
            } else {
                palette.faint_text
            },
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn tab_pin_button(
    is_active: bool,
    is_pinned: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = VisualPalette::from_theme(theme);
        let background = match (is_active, status) {
            (_, button::Status::Hovered) => palette.surface_high,
            (_, button::Status::Pressed) => palette.selected,
            (true, _) => palette.surface,
            (false, _) => palette.chrome,
        };

        let text_alpha = match (is_pinned, is_active, status) {
            (true, _, _) => 0.9,
            (false, _, button::Status::Hovered | button::Status::Pressed) => 0.74,
            (false, true, _) => 0.45,
            (false, false, _) => 0.24,
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: palette.text.scale_alpha(text_alpha),
            border: border(0.0, Color::TRANSPARENT, CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = VisualPalette::from_theme(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => palette.accent,
        text_input::Status::Hovered => palette.border,
        text_input::Status::Active => palette.border_soft,
        text_input::Status::Disabled => palette.border_soft.scale_alpha(0.55),
    };

    text_input::Style {
        background: Background::Color(if matches!(status, text_input::Status::Disabled) {
            palette.surface_low
        } else {
            palette.surface
        }),
        border: border(1.0, border_color, CONTROL_RADIUS),
        icon: palette.muted_text,
        placeholder: palette.faint_text,
        value: palette.text,
        selection: palette.selection,
    }
}
