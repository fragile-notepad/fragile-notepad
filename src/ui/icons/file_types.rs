//! File-type artwork selected by the source catalog's file extensions.

use std::path::Path;
use std::sync::LazyLock;

use iced::widget::image;
use iced::{Element, Length};

include!(concat!(env!("OUT_DIR"), "/file_type_icons.rs"));

pub fn icon_for_path<Message: 'static>(
    path: Option<&Path>,
    size: u16,
) -> Element<'static, Message> {
    icon_for_path_with_opacity(path, size, 1.0)
}

pub fn icon_for_path_with_opacity<Message: 'static>(
    path: Option<&Path>,
    size: u16,
    opacity: f32,
) -> Element<'static, Message> {
    let handle = match path {
        None => &*TEXT,
        Some(path) => handle_for_extension(
            &path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
    };
    image(handle.clone())
        .width(Length::Fixed(f32::from(size)))
        .height(Length::Fixed(f32::from(size)))
        .opacity(opacity.clamp(0.0, 1.0))
        .into()
}
