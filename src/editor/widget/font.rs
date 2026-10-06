use iced::Font;
use iced::advanced::graphics::text::cosmic_text::fontdb;
use iced::advanced::{graphics, text};
#[cfg(test)]
use iced::font;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::{Arc, OnceLock, RwLock};
use unicode_segmentation::UnicodeSegmentation;

use crate::editor::cjk::{CjkContext, CjkLanguage, CjkRun, cjk_runs};

mod profile;

use profile::{FontProfile, font_weight_is_available};

pub const EDITOR_TEXT_SHAPING: text::Shaping = text::Shaping::Auto;

const REGIONAL_LANGUAGES: [CjkLanguage; 4] = [
    CjkLanguage::SimplifiedChinese,
    CjkLanguage::TraditionalChinese,
    CjkLanguage::Japanese,
    CjkLanguage::Korean,
];

// The build-time catalog supplies the same regional order and family priorities
// to profile generation and runtime routing.
const REGIONAL_FAMILIES: &[&[&str]; 4] = include!(concat!(env!("OUT_DIR"), "/font_families.rs"));
const COHERENT_COLLECTIONS: &[[&[&str]; 4]] =
    include!(concat!(env!("OUT_DIR"), "/font_collections.rs"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorFontRoute {
    pub primary: Font,
    pub cjk_fallback_families: &'static [&'static str],
}

#[cfg(target_os = "windows")]
pub const EDITOR_FONT_ROUTE: EditorFontRoute = EditorFontRoute {
    primary: Font::new("Consolas"),
    cjk_fallback_families: &[
        "Yu Gothic",
        "Malgun Gothic",
        "MingLiU_HKSCS",
        "Microsoft JhengHei UI",
        "Microsoft YaHei UI",
        "Microsoft YaHei",
    ],
};

#[cfg(target_os = "macos")]
pub const EDITOR_FONT_ROUTE: EditorFontRoute = EditorFontRoute {
    primary: Font::new("Menlo"),
    cjk_fallback_families: &[
        "Hiragino Sans",
        "Apple SD Gothic Neo",
        "PingFang HK",
        "PingFang TC",
        "PingFang SC",
    ],
};

#[cfg(all(unix, not(target_os = "macos")))]
pub const EDITOR_FONT_ROUTE: EditorFontRoute = EditorFontRoute {
    primary: Font::MONOSPACE,
    cjk_fallback_families: &[
        "Noto Sans CJK SC",
        "Noto Sans CJK TC",
        "Noto Sans CJK HK",
        "Noto Sans CJK JP",
        "Noto Sans CJK KR",
    ],
};

#[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
pub const EDITOR_FONT_ROUTE: EditorFontRoute = EditorFontRoute {
    primary: Font::MONOSPACE,
    cjk_fallback_families: &[],
};

pub const EDITOR_FONT: Font = EDITOR_FONT_ROUTE.primary;

/// A contiguous byte range rendered with one installed font family and weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorFontRun {
    pub byte_range: Range<usize>,
    pub font: Font,
}

/// Routes CJK glyphs by language while retaining the editor's Latin monospace.
/// The inherited language comes from the same logical line, before
/// wrapping and horizontal clipping remove its language-identifying letters.
pub fn editor_font_runs(text: &str, inherited: Option<CjkLanguage>) -> Vec<EditorFontRun> {
    editor_font_runs_from_cjk_runs(text, &cjk_runs(text, inherited))
}

/// Retains the language of each logical run when a wrapped or clipped fragment
/// no longer includes its own kana or Hangul cue.
pub(super) fn editor_font_runs_for_fragment(
    text: &str,
    context: Option<&CjkContext>,
    line: usize,
    start_byte: usize,
) -> Vec<EditorFontRun> {
    let runs = context.map_or_else(
        || cjk_runs(text, None),
        |context| context.runs_for_fragment(line, start_byte, text),
    );
    editor_font_runs_from_cjk_runs(text, &runs)
}

/// Translates original source ranges to expanded text after tab replacement.
pub(super) fn remap_font_runs(
    runs: &[EditorFontRun],
    byte_offsets: &[usize],
) -> Vec<EditorFontRun> {
    runs.iter()
        .map(|run| EditorFontRun {
            byte_range: byte_offsets[run.byte_range.start]..byte_offsets[run.byte_range.end],
            font: run.font,
        })
        .filter(|run| !run.byte_range.is_empty())
        .collect()
}

/// Keeps intersecting font ranges, relative to the beginning of a text slice.
pub(super) fn clipped_font_runs(runs: &[EditorFontRun], range: Range<usize>) -> Vec<EditorFontRun> {
    runs.iter()
        .filter_map(|run| {
            let start = run.byte_range.start.max(range.start);
            let end = run.byte_range.end.min(range.end);
            (start < end).then(|| EditorFontRun {
                byte_range: (start - range.start)..(end - range.start),
                font: run.font,
            })
        })
        .collect()
}

/// Applies the editor's font profile to already detected logical language runs.
/// Call this after projecting context across wrapping or clipping, so Hangul
/// and Hanja share the same font decisions in drawing, geometry, and previews.
pub fn editor_font_runs_from_cjk_runs(text: &str, runs: &[CjkRun]) -> Vec<EditorFontRun> {
    let routes = runs
        .iter()
        .any(|run| run.language.is_some())
        .then(regional_font_routes);
    let mut fonts: Vec<EditorFontRun> = Vec::with_capacity(runs.len());

    for run in runs {
        if let Some(language) = run.language {
            let profile =
                &routes.as_ref().expect("CJK font routes").profiles[language_index(language)];
            append_profile_runs(&mut fonts, text, run.byte_range.clone(), profile);
        } else {
            append_font_run(&mut fonts, run.byte_range.clone(), EDITOR_FONT);
        }
    }

    fonts
}

/// Returns the representative installed regional face, retaining system fallback
/// when no preferred family is available. Use `editor_font_runs` for text-aware
/// coverage fallback and the Korean Hangul/Hanja weight distinction.
pub fn regional_cjk_font(language: CjkLanguage) -> Font {
    regional_font_routes().profiles[language_index(language)].preferred_font()
}

#[derive(Debug)]
struct RegionalFontRoutes {
    version: graphics::text::Version,
    profiles: [FontProfile; 4],
}

fn regional_font_routes() -> Arc<RegionalFontRoutes> {
    static ROUTES: OnceLock<RwLock<Option<Arc<RegionalFontRoutes>>>> = OnceLock::new();

    ensure_build_fonts();

    let font_system = graphics::text::font_system()
        .read()
        .expect("Read editor font system");
    let version = font_system.version();
    let cache = ROUTES.get_or_init(|| RwLock::new(None));

    if let Some(routes) = cached_regional_font_routes(cache, version) {
        return routes;
    }

    drop(font_system);

    // Access to the raw database requires a write guard. Keep this off the
    // cache-hit path, and read the version again after acquiring it in case
    // a custom font was loaded between the two guards.
    let mut font_system = graphics::text::font_system()
        .write()
        .expect("Inspect regional font faces");
    let version = font_system.version();
    if let Some(routes) = cached_regional_font_routes(cache, version) {
        return routes;
    }

    // Enumerate only when the font database changes. Loading custom fonts
    // increments this version, so newly available regional families take
    // effect without a process restart.
    let db = font_system.raw().db();
    let profiles =
        coherent_font_collection(|family, weight| font_weight_is_available(db, family, weight))
            .map(|fonts| fonts.map(FontProfile::regular))
            .unwrap_or_else(|| native_font_routes(db).map(|font| FontProfile::resolve(db, font)));
    let routes = Arc::new(RegionalFontRoutes { version, profiles });
    *cache.write().expect("Write regional font cache") = Some(Arc::clone(&routes));

    routes
}

fn cached_regional_font_routes(
    cache: &RwLock<Option<Arc<RegionalFontRoutes>>>,
    version: graphics::text::Version,
) -> Option<Arc<RegionalFontRoutes>> {
    cache
        .read()
        .expect("Read regional font cache")
        .as_ref()
        .filter(|routes| routes.version == version)
        .map(Arc::clone)
}

fn ensure_build_fonts() {
    static LOADED: OnceLock<()> = OnceLock::new();
    const FONTS: &[&[u8]] = include!(concat!(env!("OUT_DIR"), "/font_assets.rs"));

    LOADED.get_or_init(|| {
        if !FONTS.is_empty() {
            let mut system = graphics::text::font_system()
                .write()
                .expect("Load generated CJK fallback fonts");
            for bytes in FONTS {
                system.load_font(std::borrow::Cow::Borrowed(bytes));
            }
        }
    });
}

fn append_font_run(fonts: &mut Vec<EditorFontRun>, range: Range<usize>, font: Font) {
    if let Some(previous) = fonts.last_mut()
        && previous.byte_range.end == range.start
        && previous.font == font
    {
        previous.byte_range.end = range.end;
    } else {
        fonts.push(EditorFontRun {
            byte_range: range,
            font,
        });
    }
}

fn append_profile_runs(
    fonts: &mut Vec<EditorFontRun>,
    text: &str,
    range: Range<usize>,
    profile: &FontProfile,
) {
    if !profile.has_adjustment() {
        append_font_run(fonts, range, profile.preferred_font());
        return;
    }
    for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
        let start = range.start + offset;
        append_font_run(
            fonts,
            start..start + grapheme.len(),
            profile.font_for_grapheme(grapheme),
        );
    }
}

fn native_font_routes(db: &fontdb::Database) -> [Font; 4] {
    // A family with only a bold face must not silently become body text.
    // Variable fonts count when their weight axis supports Regular (400).
    let candidates: HashSet<&str> = REGIONAL_LANGUAGES
        .iter()
        .flat_map(|language| preferred_families(*language).iter().copied())
        .collect();
    let installed: HashSet<&str> = candidates
        .into_iter()
        .filter(|family| font_weight_is_available(db, family, 400))
        .collect();
    REGIONAL_LANGUAGES
        .map(|language| first_installed_font(preferred_families(language), &installed))
}

/// Prefer one complete regional design over a mix of unrelated families.
/// Each collection is selected as a whole, including when fonts are loaded
/// later. A lone installed Noto Sans SC does not displace available native faces.
fn coherent_font_collection(available: impl Fn(&str, u16) -> bool) -> Option<[Font; 4]> {
    COHERENT_COLLECTIONS.iter().find_map(|collection| {
        match collection.map(|families| {
            families
                .iter()
                .find(|family| available(family, 400))
                .map(|family| Font::new(family))
        }) {
            [Some(sc), Some(tc), Some(jp), Some(kr)] => Some([sc, tc, jp, kr]),
            _ => None,
        }
    })
}

fn language_index(language: CjkLanguage) -> usize {
    match language {
        CjkLanguage::SimplifiedChinese => 0,
        CjkLanguage::TraditionalChinese => 1,
        CjkLanguage::Japanese => 2,
        CjkLanguage::Korean => 3,
    }
}

fn first_installed_font(preferred: &'static [&'static str], installed: &HashSet<&str>) -> Font {
    preferred
        .iter()
        .find(|family| installed.contains(**family))
        .map(|family| Font::new(family))
        .unwrap_or(EDITOR_FONT)
}

fn preferred_families(language: CjkLanguage) -> &'static [&'static str] {
    REGIONAL_FAMILIES[language_index(language)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_test_face(db: &mut fontdb::Database, family: &str, weight: u16, style: fontdb::Style) {
        db.push_face_info(fontdb::FaceInfo {
            id: fontdb::ID::dummy(),
            source: fontdb::Source::Binary(std::sync::Arc::new(Vec::<u8>::new())),
            index: 0,
            families: vec![(family.to_owned(), fontdb::Language::English_UnitedStates)],
            post_script_name: format!("{family}-{weight}"),
            style,
            weight: fontdb::Weight(weight),
            stretch: fontdb::Stretch::Normal,
            monospaced: false,
        });
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn light_modern_hangul_shapes_nfd_and_nfc_as_the_same_syllable() {
        use iced::advanced::text::Paragraph as _;
        use iced::{Pixels, Size, alignment};

        let routes = regional_font_routes();
        let font = routes.profiles[language_index(CjkLanguage::Korean)].font_for_grapheme("각");
        if font.weight != font::Weight::Light {
            return;
        }
        let shape = |content: &str| {
            graphics::text::Paragraph::with_text(text::Text {
                content,
                bounds: Size::new(f32::INFINITY, 32.0),
                size: Pixels(24.0),
                line_height: text::LineHeight::Absolute(Pixels(32.0)),
                font,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
                ellipsis: text::Ellipsis::None,
                hint_factor: None,
            })
        };
        let nfc = shape("각");
        let nfd = shape("각");
        let inspect = |paragraph: &graphics::text::Paragraph| {
            paragraph
                .buffer()
                .layout_runs()
                .flat_map(|run| {
                    run.glyphs
                        .iter()
                        .map(|glyph| (glyph.font_id, glyph.glyph_id, glyph.x, glyph.w))
                })
                .collect::<Vec<_>>()
        };
        let nfc_glyphs = inspect(&nfc);
        let nfd_glyphs = inspect(&nfd);
        assert_eq!(nfc_glyphs.len(), 1);
        assert_ne!(nfc_glyphs[0].1, 0);
        assert_eq!(nfd_glyphs, nfc_glyphs);
        let mut system = graphics::text::font_system().write().unwrap();
        let face = system.raw().db().face(nfc_glyphs[0].0).unwrap();
        assert_eq!(face.weight.0, 300);
        assert!(
            face.families
                .iter()
                .any(|(name, _)| name == "Malgun Gothic")
        );
    }

    #[test]
    fn coherent_collection_requires_every_region_at_regular_weight() {
        let mut db = fontdb::Database::new();
        for family in ["Noto Sans SC", "Noto Sans TC", "Noto Sans JP"] {
            add_test_face(&mut db, family, 400, fontdb::Style::Normal);
        }
        add_test_face(&mut db, "Noto Sans KR", 700, fontdb::Style::Normal);
        assert_eq!(
            coherent_font_collection(|family, weight| {
                font_weight_is_available(&db, family, weight)
            }),
            None
        );

        add_test_face(&mut db, "Noto Sans KR", 400, fontdb::Style::Normal);
        assert_eq!(
            coherent_font_collection(|family, weight| {
                font_weight_is_available(&db, family, weight)
            }),
            Some([
                Font::new("Noto Sans SC"),
                Font::new("Noto Sans TC"),
                Font::new("Noto Sans JP"),
                Font::new("Noto Sans KR"),
            ])
        );
    }

    #[test]
    fn availability_excludes_other_styles_and_missing_weights() {
        let mut db = fontdb::Database::new();
        add_test_face(&mut db, "Yu Gothic", 400, fontdb::Style::Normal);
        add_test_face(&mut db, "Yu Gothic", 500, fontdb::Style::Italic);
        add_test_face(&mut db, "Yu Gothic", 700, fontdb::Style::Normal);
        assert!(font_weight_is_available(&db, "Yu Gothic", 400));
        assert!(!font_weight_is_available(&db, "Yu Gothic", 500));
        assert!(!font_weight_is_available(&db, "Missing", 400));
        assert!(!font_weight_is_available(&db, "Yu Gothic Medium", 500));
    }

    #[test]
    fn missing_or_unreadable_companions_keep_regular_profiles() {
        let mut db = fontdb::Database::new();
        for (family, weight) in [
            ("Microsoft YaHei", 290),
            ("Yu Gothic", 500),
            ("Malgun Gothic", 300),
        ] {
            let body = Font::new(family);
            add_test_face(&mut db, family, 400, fontdb::Style::Normal);
            add_test_face(&mut db, family, 700, fontdb::Style::Normal);
            let profile = FontProfile::resolve(&db, body);
            assert!(!profile.has_adjustment());
            assert_eq!(profile.preferred_font(), body);
            add_test_face(&mut db, family, weight, fontdb::Style::Normal);
            let profile = FontProfile::resolve(&db, body);
            assert!(!profile.has_adjustment(), "Unreadable font data: {family}");
            assert_eq!(profile.font_for_grapheme("骨"), body);
            assert_eq!(profile.font_for_grapheme("한"), body);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_body_routes_skip_bold_only_families() {
        let mut db = fontdb::Database::new();
        add_test_face(&mut db, "Microsoft YaHei", 700, fontdb::Style::Normal);
        add_test_face(&mut db, "Noto Sans SC", 400, fontdb::Style::Normal);
        assert_eq!(
            native_font_routes(&db)[language_index(CjkLanguage::SimplifiedChinese)],
            Font::new("Noto Sans SC")
        );
    }

    #[test]
    fn regional_family_selection_skips_missing_fonts_in_preference_order() {
        let installed = HashSet::from(["Second", "Third"]);
        assert_eq!(
            first_installed_font(&["Missing", "Second", "Third"], &installed),
            Font::new("Second")
        );
        assert_eq!(first_installed_font(&["Missing"], &installed), EDITOR_FONT);
    }

    #[test]
    fn missing_korean_body_family_does_not_select_hangul_only_semilight() {
        let installed = HashSet::from(["Malgun Gothic Semilight", "Noto Sans CJK KR"]);
        assert_eq!(
            first_installed_font(preferred_families(CjkLanguage::Korean), &installed),
            Font::new("Noto Sans CJK KR")
        );
    }

    #[test]
    fn mixed_runs_preserve_latin_and_cover_every_utf8_byte() {
        let text = "Rust 2026 字かな end";
        let runs = editor_font_runs(text, Some(CjkLanguage::Japanese));
        let mut end = 0;
        for run in runs {
            assert_eq!(run.byte_range.start, end);
            assert!(text.is_char_boundary(run.byte_range.start));
            assert!(text.is_char_boundary(run.byte_range.end));
            for ch in text[run.byte_range.clone()]
                .chars()
                .filter(char::is_ascii_alphanumeric)
            {
                assert_eq!(run.font, EDITOR_FONT, "Latin character {ch}");
            }
            end = run.byte_range.end;
        }
        assert_eq!(end, text.len());
    }
}
