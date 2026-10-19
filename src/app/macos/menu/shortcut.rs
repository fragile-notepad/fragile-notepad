//! Conversion from shortcut display hints to AppKit key equivalents.

use objc2_app_kit::NSEventModifierFlags;

use crate::core::{ShortcutDisplayPart, ShortcutModifierIcon};
use crate::ui::menu::MenuShortcutHint;

pub(super) fn key_equivalent(hint: Option<&MenuShortcutHint>) -> (String, NSEventModifierFlags) {
    let mut modifiers = NSEventModifierFlags::empty();
    let key = match hint {
        Some(MenuShortcutHint::Binding(binding)) => {
            for part in &binding.modifiers {
                modifiers |= match part {
                    ShortcutDisplayPart::Icon(
                        ShortcutModifierIcon::Command | ShortcutModifierIcon::Windows,
                    ) => NSEventModifierFlags::Command,
                    ShortcutDisplayPart::Icon(ShortcutModifierIcon::Option) => {
                        NSEventModifierFlags::Option
                    }
                    ShortcutDisplayPart::Icon(ShortcutModifierIcon::Shift) => {
                        NSEventModifierFlags::Shift
                    }
                    ShortcutDisplayPart::Text("Ctrl") => NSEventModifierFlags::Control,
                    _ => NSEventModifierFlags::empty(),
                };
            }
            binding.key.as_str()
        }
        Some(MenuShortcutHint::Text(text)) => text.as_str(),
        None => "",
    };
    let key = match key {
        "Tab" => "\t".into(),
        "Enter" => "\r".into(),
        "Space" => " ".into(),
        "Backspace" => "\u{8}".into(),
        "Delete" | "Del" => "\u{f728}".into(),
        "Home" => "\u{f729}".into(),
        "End" => "\u{f72b}".into(),
        "Page Up" => "\u{f72c}".into(),
        "Page Down" => "\u{f72d}".into(),
        "Left" => "\u{f702}".into(),
        "Right" => "\u{f703}".into(),
        "Up" => "\u{f700}".into(),
        "Down" => "\u{f701}".into(),
        "Esc" => "\u{1b}".into(),
        key if key.starts_with('F')
            && key[1..].parse::<u32>().is_ok_and(|n| (1..=12).contains(&n)) =>
        {
            char::from_u32(0xf703 + key[1..].parse::<u32>().unwrap())
                .unwrap()
                .to_string()
        }
        key if key.chars().count() == 1 => key.to_lowercase(),
        _ => String::new(),
    };
    (key, modifiers)
}
