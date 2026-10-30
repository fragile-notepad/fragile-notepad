use fragile_notepad::editor::widget::{EDITOR_FONT, EDITOR_FONT_ROUTE};
use fragile_notepad::startup::iced_settings;

use iced::Backend;
use iced::advanced::graphics::text::{self as graphics_text, cosmic_text, font_system};

#[test]
fn startup_settings_keep_first_paint_on_software_rendering() {
    let settings = iced_settings();

    assert_eq!(settings.backend, Backend::Software);
    assert!(!settings.antialiasing);
    assert!(settings.vsync);
    assert_eq!(settings.max_fps, 120);
}

#[test]
fn editor_font_route_keeps_primary_font_and_platform_cjk_fallback() {
    let mut font_system = font_system().write().expect("write font system");

    let ascii_families = shaped_font_families(font_system.raw(), "A");
    if let iced::font::Family::Name(primary) = EDITOR_FONT_ROUTE.primary.family {
        assert!(
            ascii_families.iter().any(|name| name == primary),
            "ASCII editor glyph should use primary route font {primary}, got families {ascii_families:?}"
        );
    } else {
        assert!(
            !ascii_families.is_empty(),
            "ASCII editor glyph should resolve through generic primary route font"
        );
    }

    let han_families = shaped_font_families(font_system.raw(), "\u{6c49}");
    assert!(
        !han_families.is_empty(),
        "Han glyph should resolve to a platform fallback font"
    );
}

fn shaped_font_families(raw: &mut cosmic_text::FontSystem, content: &str) -> Vec<String> {
    let mut buffer = cosmic_text::Buffer::new(raw, cosmic_text::Metrics::new(16.0, 20.0));
    buffer.set_size(Some(100.0), Some(20.0));
    buffer.set_text(
        content,
        &graphics_text::to_attributes(EDITOR_FONT),
        cosmic_text::Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(raw, false);

    let font_id = buffer
        .layout_runs()
        .next()
        .and_then(|run| run.glyphs.first())
        .map(|glyph| glyph.font_id)
        .expect("shaped glyph");
    let face = raw.db().face(font_id).expect("glyph font face");

    face.families
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
