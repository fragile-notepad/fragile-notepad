//! macOS font routing, text spacing, and optical size tests.

use iced::advanced::graphics::text::cosmic_text::fontdb;
use iced::advanced::{graphics, text};
use iced::{Font, font};

use super::{
    EDITOR_FONT, EDITOR_TEXT_SHAPING, REGIONAL_LANGUAGES, font_weight_is_available, language_index,
    preferred_families, regional_font_routes,
};

#[test]
fn macos_system_font_routes_resolve_without_ascii_fallback() {
    use iced::advanced::graphics::text::cosmic_text;

    let _font_guard = crate::font_system_test_guard();
    let mut system = graphics::text::font_system().write().unwrap();
    let raw = system.raw();
    for (font, content, generic) in [
        (
            Font::DEFAULT,
            "Native UI: café 0123",
            fontdb::Family::SansSerif,
        ),
        (
            Font::DEFAULT.weight(font::Weight::Semibold),
            "Native UI: café 0123",
            fontdb::Family::SansSerif,
        ),
        (EDITOR_FONT, "Il10 O0 -> fn", fontdb::Family::Monospace),
    ] {
        let family = raw.db().family_name(&generic).to_owned();
        let mut buffer = cosmic_text::Buffer::new(raw, cosmic_text::Metrics::new(16.0, 20.0));
        buffer.set_size(Some(1000.0), Some(20.0));
        buffer.set_text(
            content,
            &graphics::text::to_attributes(font),
            cosmic_text::Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(raw, false);
        let glyphs: Vec<_> = buffer.layout_runs().flat_map(|run| run.glyphs).collect();
        assert!(!glyphs.is_empty());
        for glyph in &glyphs {
            assert_ne!(glyph.glyph_id, 0, "Missing glyph in {family}: {content}");
            let face = raw.db().face(glyph.font_id).unwrap();
            assert!(
                face.families.iter().any(|(name, _)| name == &family),
                "Expected {family}, shaped with {:?}",
                face.families
            );
            if generic == fontdb::Family::Monospace {
                assert!(face.monospaced, "Editor face is not monospace: {family}");
                assert!((glyph.w - glyphs[0].w).abs() < 0.01);
            }
        }
        let face = raw.db().face(glyphs[0].font_id).unwrap();
        eprintln!(
            "macOS {:?} {:?}: family={family}, postscript={}, face_weight={}, glyphs={}",
            font.family,
            font.weight,
            face.post_script_name,
            face.weight.0,
            glyphs.len()
        );
    }
}

#[test]
fn macos_keeps_system_cjk_faces_ahead_of_bundled_collection() {
    let _font_guard = crate::font_system_test_guard();
    let routes = regional_font_routes();
    let mut system = graphics::text::font_system().write().unwrap();
    let db = system.raw().db();
    let mut installed_system_families = 0;
    for language in REGIONAL_LANGUAGES {
        let family = preferred_families(language)[0];
        if font_weight_is_available(db, family, 400) {
            let selected = routes.profiles[language_index(language)].preferred_font();
            assert_eq!(selected.family, font::Family::Name(family));
            eprintln!("macOS {language:?}: {selected:?}");
            installed_system_families += 1;
        }
    }
    assert!(
        installed_system_families > 0,
        "No macOS regional system fonts found"
    );
}

#[test]
fn macos_ui_uses_shaped_spacing_for_ascii_labels() {
    use iced::advanced::text::Paragraph as _;
    use iced::{Pixels, Size, alignment};

    let _font_guard = crate::font_system_test_guard();
    let shape = |font, shaping, weight| {
        let paragraph = graphics::text::Paragraph::with_text(text::Text {
            content: "AVATAR Toffee fi 0123",
            bounds: Size::new(1000.0, 20.0),
            size: Pixels(13.0),
            line_height: text::LineHeight::Absolute(Pixels(20.0)),
            font: Font { weight, ..font },
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: None,
        });
        paragraph
            .buffer()
            .layout_runs()
            .flat_map(|run| run.glyphs)
            .map(|glyph| (glyph.font_id, glyph.glyph_id, glyph.x, glyph.w))
            .collect::<Vec<_>>()
    };
    for weight in [font::Weight::Normal, font::Weight::Semibold] {
        let auto = shape(Font::DEFAULT, text::Shaping::Auto, weight);
        assert!(!auto.is_empty());
        assert_eq!(auto, shape(Font::DEFAULT, text::Shaping::Advanced, weight));
    }
    let variable_family = {
        let mut system = graphics::text::font_system().write().unwrap();
        system
            .raw()
            .db()
            .query(&fontdb::Query {
                families: &[fontdb::Family::Name(".SF NS")],
                ..fontdb::Query::default()
            })
            .is_some()
    };
    if variable_family {
        // Retain the variable-font spacing regression even when the UI
        // uses a static text family with identical Basic/Advanced metrics.
        let sf = Font::new(".SF NS");
        let mut differs_from_unshaped = false;
        for weight in [font::Weight::Normal, font::Weight::Semibold] {
            let auto = shape(sf, text::Shaping::Auto, weight);
            assert_eq!(auto, shape(sf, text::Shaping::Advanced, weight));
            differs_from_unshaped |= auto != shape(sf, text::Shaping::Basic, weight);
        }
        assert!(
            differs_from_unshaped,
            "SF spacing regression was not exercised"
        );
    }
}

#[test]
fn macos_ui_shapes_and_rasterizes_the_same_text_optical_size() {
    use iced::advanced::graphics::text::cosmic_text;

    let _font_guard = crate::font_system_test_guard();
    let mut system = graphics::text::font_system().write().unwrap();
    let raw = system.raw();
    let id = raw
        .db()
        .query(&fontdb::Query {
            families: &[fontdb::Family::SansSerif],
            ..fontdb::Query::default()
        })
        .unwrap();
    let font = raw.get_font(id, cosmic_text::Weight::NORMAL).unwrap();
    let opsz = u32::from_be_bytes(*b"opsz");
    if font.as_swash().variations().find_by_tag(opsz).is_none() {
        eprintln!("Installed UI family has no optical-size axis");
        return;
    }
    let mut control =
        cosmic_text::FontSystem::new_with_locale_and_db(raw.locale().to_owned(), raw.db().clone());
    control.set_font_optical_size(id, Some(28.0));
    let shape = |system: &mut cosmic_text::FontSystem, weight| {
        let mut buffer = cosmic_text::Buffer::new(system, cosmic_text::Metrics::new(13.0, 20.0));
        buffer.set_size(Some(1000.0), Some(20.0));
        buffer.set_text(
            "Hamburgefontsiv AVATAR Toffee 0123",
            &cosmic_text::Attrs::new().weight(weight),
            cosmic_text::Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(system, false);
        buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter().cloned())
            .collect::<Vec<_>>()
    };
    for weight in [cosmic_text::Weight::NORMAL, cosmic_text::Weight::SEMIBOLD] {
        let face = raw.get_font(id, weight).unwrap();
        let expected: Vec<_> = face
            .as_swash()
            .variations()
            .normalized_coords([
                (u32::from_be_bytes(*b"wght"), f32::from(weight.0)),
                (opsz, 17.0),
            ])
            .collect();
        assert_eq!(face.normalized_coords().len(), expected.len());
        assert!(
            face.normalized_coords()
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (i32::from(*actual) - i32::from(expected)).abs() <= 1),
            "Shaping must use the text axes"
        );
        let text = shape(raw, weight);
        let display = shape(&mut control, weight);
        assert_eq!(text.len(), display.len());
        assert!(!text.is_empty());
        assert!(
            text.iter()
                .chain(&display)
                .all(|glyph| glyph.font_id == id && glyph.glyph_id != 0)
        );
        let width =
            |glyphs: &[cosmic_text::LayoutGlyph]| glyphs.iter().map(|glyph| glyph.w).sum::<f32>();
        assert!(
            width(&text) > width(&display) * 1.05,
            "Text optical size must open small-label spacing"
        );
        let mut swash = cosmic_text::SwashCache::new();
        let changed = text.iter().any(|glyph| {
            // Keep glyph, size, weight and subpixel bins identical. Changed
            // advances alone must not make the raster comparison pass.
            let key = glyph.physical((0.0, 0.0), 1.5).cache_key;
            let text = swash.get_image_uncached(raw, key);
            let display = swash.get_image_uncached(&mut control, key);
            text.zip(display)
                .is_some_and(|(text, display)| text.data != display.data)
        });
        assert!(changed, "Glyph rasterization must use the text axes too");
        eprintln!(
            "macOS SF text optical size 17 at {:?}: width={} (optical size 28={})",
            weight,
            width(&text),
            width(&display)
        );
    }
}
