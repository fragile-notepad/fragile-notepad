use iced::advanced::image;
use iced::{Color, Theme};

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{LazyLock, Mutex};

pub(super) type IconCache<Icon> = LazyLock<Mutex<HashMap<(Icon, [u8; 4]), image::Handle>>>;

pub fn handle_with_color<Icon>(
    icon: Icon,
    color: Color,
    cache: &'static IconCache<Icon>,
    bytes: fn(Icon) -> &'static [u8],
) -> image::Handle
where
    Icon: Copy + Eq + Hash,
{
    let color = color.into_rgba8();
    let mut cache = cache.lock().expect("icon color cache");

    cache
        .entry((icon, color))
        .or_insert_with(|| {
            image::Handle::from_rgba(
                super::ICON_SIZE,
                super::ICON_SIZE,
                colorized_icon_bytes(bytes(icon), color),
            )
        })
        .clone()
}

pub fn theme_text_color(theme: &Theme) -> Color {
    crate::ui::styles::shortcut_text_color(theme)
}

fn colorized_icon_bytes(bytes: &[u8], color: [u8; 4]) -> Vec<u8> {
    bytes
        .chunks_exact(4)
        .flat_map(|pixel| {
            let alpha = ((pixel[3] as u16 * color[3] as u16) / 255) as u8;
            [color[0], color[1], color[2], alpha]
        })
        .collect()
}
