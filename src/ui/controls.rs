//! Reusable UI control builders.

use iced::widget::{button, container, text};
use iced::{Element, Fill};

use crate::message::Message;
use crate::ui::styles;

pub fn centered_button_content<'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(content).center(Fill).into()
}

pub fn centered_button_label(label: &str, size: u32) -> Element<'_, Message> {
    text(label).size(size).into()
}

pub fn centered_fill_button_label(label: &str, size: u32) -> Element<'_, Message> {
    container(text(label).size(size)).center_x(Fill).into()
}

pub fn command_button<'a>(
    label: &'static str,
    text_size: u32,
    message: Message,
) -> Element<'a, Message> {
    button(centered_button_label(label, text_size))
        .padding([6, 10])
        .style(styles::command_button)
        .on_press(message)
        .into()
}

pub fn primary_command_button<'a>(
    label: &'static str,
    text_size: u32,
    message: Message,
) -> Element<'a, Message> {
    button(centered_button_label(label, text_size))
        .padding([6, 10])
        .style(styles::primary_command_button)
        .on_press(message)
        .into()
}

pub fn compact_command_button<'a>(
    label: &'static str,
    text_size: u32,
    message: Message,
) -> Element<'a, Message> {
    button(centered_button_label(label, text_size))
        .padding([5, 12])
        .style(styles::command_button)
        .on_press(message)
        .into()
}

pub fn fixed_fill_command_button<'a>(
    label: &'static str,
    text_size: u32,
    width: f32,
    message: Message,
) -> Element<'a, Message> {
    button(centered_fill_button_label(label, text_size))
        .width(width)
        .padding([5, 0])
        .style(styles::command_button)
        .on_press(message)
        .into()
}

pub fn icon_command_button<'a>(
    icon: Element<'a, Message>,
    message: Message,
) -> Element<'a, Message> {
    button(centered_button_content(icon))
        .width(34)
        .height(28)
        .padding(0)
        .style(styles::command_button)
        .on_press(message)
        .into()
}

pub fn icon_toggle_button<'a>(
    icon: Element<'a, Message>,
    message: Message,
) -> Element<'a, Message> {
    button(centered_button_content(icon))
        .width(28)
        .height(28)
        .padding(0)
        .style(styles::icon_button)
        .on_press(message)
        .into()
}

pub fn value_pill<'a>(label: String, text_size: u32, width: f32) -> Element<'a, Message> {
    container(text(label).size(text_size))
        .width(width)
        .center_x(width)
        .into()
}

pub fn category_button<'a>(
    label: &'static str,
    text_size: u32,
    is_active: bool,
    message: Message,
) -> Element<'a, Message> {
    button(centered_fill_button_label(label, text_size))
        .width(Fill)
        .padding([8, 12])
        .style(styles::settings_category_button(is_active))
        .on_press(message)
        .into()
}
