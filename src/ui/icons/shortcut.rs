use iced::{Color, Element, Theme};

use super::vector::{self, compiled};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutIcon {
    Command,
    Option,
    PinAngle,
    PinAngleFill,
    Shift,
    Windows,
}

pub fn icon_with_color<Message: 'static>(
    icon: ShortcutIcon,
    size: u32,
    color: fn(&Theme) -> Color,
) -> Element<'static, Message> {
    let asset = match icon {
        ShortcutIcon::Command => &compiled::BOOTSTRAP_COMMAND,
        ShortcutIcon::Option => &compiled::BOOTSTRAP_OPTION,
        ShortcutIcon::PinAngle => &compiled::BOOTSTRAP_PIN_ANGLE,
        ShortcutIcon::PinAngleFill => &compiled::BOOTSTRAP_PIN_ANGLE_FILL,
        ShortcutIcon::Shift => &compiled::BOOTSTRAP_SHIFT,
        ShortcutIcon::Windows => &compiled::BOOTSTRAP_WINDOWS,
    };
    vector::icon(asset, size, color, 1.0)
}
