//! Cell metrics for the native macOS monospace family.

use std::sync::OnceLock;

use iced::advanced::text::Paragraph as _;
use iced::advanced::{graphics, text};
use iced::{Pixels, Size, alignment};

use super::{EDITOR_FONT, EDITOR_TEXT_SHAPING, FALLBACK_CHARACTER_WIDTH_RATIO};

const REFERENCE_SIZE: f32 = 16.0;

pub(crate) fn editor_character_width(size: f32) -> f32 {
    // The native monospace family is resolved once with the font system.
    // Cache its advance so ASCII geometry agrees with the rendered font.
    static CELL_WIDTH_PER_PIXEL: OnceLock<f32> = OnceLock::new();
    size * CELL_WIDTH_PER_PIXEL.get_or_init(measure_cell_width_ratio)
}

fn measure_cell_width_ratio() -> f32 {
    let paragraph = graphics::text::Paragraph::with_text(text::Text {
        content: "M",
        bounds: Size::new(100.0, 20.0),
        size: Pixels(REFERENCE_SIZE),
        line_height: text::LineHeight::Absolute(Pixels(20.0)),
        font: EDITOR_FONT,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: EDITOR_TEXT_SHAPING,
        wrapping: text::Wrapping::None,
        ellipsis: text::Ellipsis::None,
        hint_factor: None,
    });
    let ratio = paragraph.min_width() / REFERENCE_SIZE;
    if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        FALLBACK_CHARACTER_WIDTH_RATIO
    }
}

#[cfg(test)]
mod tests;
