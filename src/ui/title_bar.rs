//! Application-owned window chrome, shared by all three application windows.

mod interaction;
mod symbol;
#[cfg(test)]
mod tests;

use iced::widget::{button, column, container, row, text, tooltip};
use iced::{Center, Element, Fill, Theme, window};

use super::styles;
use crate::message::Message;

pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux", target_os = "macos"));
pub const HEIGHT: f32 = 32.0;
const MAC_CONTROL_WIDTH: f32 = 20.0;
const RESIZE_BORDER: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlStyle {
    Windows,
    MacOS,
}

impl ControlStyle {
    pub fn startup() -> Self {
        #[cfg(debug_assertions)]
        match std::env::var("FRAGILE_NOTEPAD_TITLE_BAR").as_deref() {
            Ok("windows") => return Self::Windows,
            Ok("macos") => return Self::MacOS,
            _ => {}
        }
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else {
            Self::Windows
        }
    }

    #[cfg(debug_assertions)]
    pub fn toggled(self) -> Self {
        match self {
            Self::Windows => Self::MacOS,
            Self::MacOS => Self::Windows,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Drag,
    Resize(window::Direction),
    Minimize,
    ToggleMaximize,
    Close,
    SystemMenu,
}

pub fn frame<'a>(
    content: Element<'a, Message>,
    id: window::Id,
    title: String,
    style: ControlStyle,
    focused: bool,
    maximized: bool,
) -> Element<'a, Message> {
    // AppKit supplies native resize hit-testing for resizable borderless windows.
    // winit does not implement drag_resize on macOS; do not intercept its edges.
    let resize = !maximized && !cfg!(target_os = "macos");
    let border = if resize { RESIZE_BORDER } else { 0.0 };
    let surface = container(
        column![
            bar(id, title, style, focused, maximized),
            container(content)
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
    .height(Fill);
    interaction::frame(surface.into(), id, border, focused)
}

fn bar<'a>(
    id: window::Id,
    title: String,
    style: ControlStyle,
    focused: bool,
    maximized: bool,
) -> Element<'a, Message> {
    let caption = text(title)
        .size(12)
        .wrapping(text::Wrapping::None)
        .ellipsis(text::Ellipsis::End)
        .width(Fill)
        .align_x(if style == ControlStyle::MacOS {
            iced::alignment::Horizontal::Center
        } else {
            iced::alignment::Horizontal::Left
        });
    let caption = container(caption)
        .padding([0, 12])
        .center_y(HEIGHT)
        .width(Fill)
        .clip(true);

    let minimize = control(id, Action::Minimize, "Minimize", style, focused, maximized);
    let maximize = control(
        id,
        Action::ToggleMaximize,
        if maximized { "Restore" } else { "Maximize" },
        style,
        focused,
        maximized,
    );
    let close = control(id, Action::Close, "Close window", style, focused, maximized);

    // Reserve equal side widths in the traffic-light layout so the caption
    // remains centered in the window, independent of the controls' width.
    let controls = match style {
        ControlStyle::Windows => row![minimize, maximize, close],
        ControlStyle::MacOS => row![close, minimize, maximize].padding([0, 10]),
    }
    .height(HEIGHT)
    .align_y(Center);

    let contents = match style {
        ControlStyle::Windows => row![
            container(title_icon(focused))
                .padding(iced::Padding::ZERO.left(12).right(6))
                .center_y(HEIGHT),
            caption,
            controls,
        ],
        ControlStyle::MacOS => row![
            container(controls).width(84),
            caption,
            container(title_icon(focused))
                .width(84)
                .padding(iced::Padding::ZERO.right(12))
                .align_x(iced::alignment::Horizontal::Right)
                .center_y(HEIGHT),
        ],
    };
    interaction::drag_region(
        container(contents.align_y(Center).height(HEIGHT))
            .width(Fill)
            .style(move |theme| styles::title_bar(theme, focused))
            .into(),
        id,
    )
}

fn title_icon(focused: bool) -> iced::widget::Image {
    iced::widget::image(crate::assets::title_icon_handle())
        .width(24)
        .height(24)
        .opacity(if focused { 1.0 } else { 0.55 })
}

fn control<'a>(
    id: window::Id,
    action: Action,
    label: &'static str,
    style: ControlStyle,
    focused: bool,
    maximized: bool,
) -> Element<'a, Message> {
    let glyph = symbol::Symbol {
        action,
        style,
        focused,
        maximized,
    };
    tooltip(
        button(Element::new(glyph))
            .padding(0)
            .on_press(Message::WindowChrome(id, action))
            .style(move |theme: &Theme, status| {
                styles::caption_button(
                    theme,
                    status,
                    style == ControlStyle::MacOS,
                    matches!(action, Action::Close),
                    focused,
                )
            }),
        text(label).size(12),
        tooltip::Position::Bottom,
    )
    .style(styles::tooltip)
    .delay(std::time::Duration::from_millis(600))
    .gap(4)
    .into()
}
