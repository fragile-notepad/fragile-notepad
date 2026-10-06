//! Calibrated optical weights with a regular face as the cluster fallback.

use iced::advanced::graphics::text::cosmic_text::{fontdb, skrifa};
use iced::{Font, font};
use skrifa::MetadataProvider;
use std::ops::Range;

#[derive(Debug)]
pub(super) struct FontProfile {
    body: Font,
    adjustment: Option<WeightAdjustment>,
}

impl FontProfile {
    pub(super) fn regular(body: Font) -> Self {
        Self {
            body,
            adjustment: None,
        }
    }

    pub(super) fn resolve(db: &fontdb::Database, body: Font) -> Self {
        let adjustment = WEIGHT_RULES
            .iter()
            .find(|rule| body.family == font::Family::Name(rule.family))
            .and_then(|rule| WeightAdjustment::load(db, body, rule));
        Self { body, adjustment }
    }

    /// A representative regional face. Subset adjustments need the text-aware
    /// route, so Korean Hanja/punctuation continue to expose the body face.
    pub(super) fn preferred_font(&self) -> Font {
        self.adjustment
            .as_ref()
            .filter(|adjustment| adjustment.scope == Scope::All)
            .map_or(self.body, |adjustment| adjustment.font)
    }

    pub(super) fn has_adjustment(&self) -> bool {
        self.adjustment.is_some()
    }

    pub(super) fn font_for_grapheme(&self, grapheme: &str) -> Font {
        self.adjustment
            .as_ref()
            .filter(|adjustment| adjustment.supports(grapheme))
            .map_or(self.body, |adjustment| adjustment.font)
    }
}

#[derive(Debug)]
struct WeightAdjustment {
    font: Font,
    coverage: Vec<Range<u32>>,
    scope: Scope,
}

impl WeightAdjustment {
    fn load(db: &fontdb::Database, body: Font, rule: &WeightRule) -> Option<Self> {
        let weight = iced_weight(rule.requested_weight)?;
        if weight == body.weight {
            return None;
        }
        // Query the attributes sent to the renderer, not just the desired
        // companion's metadata. YaHei requests 300 but its static Light is 290.
        let id = verified_face(db, rule.family, rule.requested_weight, rule.face_weight)?;
        let coverage = db.with_face_data(id, |data, index| {
            let font = skrifa::FontRef::from_index(data, index).ok()?;
            let charmap = font.charmap();
            if !charmap.has_map() {
                return None;
            }
            Some(codepoint_ranges(
                charmap
                    .mappings()
                    .filter(|(_, glyph)| glyph.to_u32() != 0)
                    .map(|(codepoint, _)| codepoint)
                    .collect(),
            ))
        })??;
        if coverage.is_empty()
            || (rule.scope == Scope::ModernHangul
                && !(0xac00..=0xd7a3).all(|codepoint| coverage_contains(&coverage, codepoint)))
        {
            return None;
        }

        Some(Self {
            font: body.weight(weight),
            coverage,
            scope: rule.scope,
        })
    }

    fn supports(&self, grapheme: &str) -> bool {
        if self.scope == Scope::ModernHangul
            && (!is_modern_hangul_cluster(grapheme)
                || !grapheme.chars().all(|character| {
                    is_modern_hangul_letter(character) || is_hangul_mark(character)
                }))
        {
            return false;
        }

        // A nominal cmap is insufficient for historical Hangul composition,
        // hence the scope guard above. Unknown marks, joiners and variation
        // selectors conservatively keep the entire cluster in the body face.
        !grapheme.is_empty()
            && grapheme
                .chars()
                .all(|character| coverage_contains(&self.coverage, character as u32))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    ModernHangul,
}

#[derive(Debug)]
struct WeightRule {
    family: &'static str,
    requested_weight: u16,
    face_weight: u16,
    scope: Scope,
}

// Generated during the build; JSON and font downloads remain outside Git.
const WEIGHT_RULES: &[WeightRule] = include!(concat!(env!("OUT_DIR"), "/font_profiles.rs"));

fn iced_weight(weight: u16) -> Option<font::Weight> {
    Some(match weight {
        100 => font::Weight::Thin,
        200 => font::Weight::ExtraLight,
        300 => font::Weight::Light,
        400 => font::Weight::Normal,
        500 => font::Weight::Medium,
        600 => font::Weight::Semibold,
        700 => font::Weight::Bold,
        800 => font::Weight::ExtraBold,
        900 => font::Weight::Black,
        _ => return None,
    })
}

pub(super) fn font_weight_is_available(db: &fontdb::Database, family: &str, weight: u16) -> bool {
    verified_face(db, family, weight, weight).is_some()
}

fn verified_face(
    db: &fontdb::Database,
    family: &str,
    requested_weight: u16,
    face_weight: u16,
) -> Option<fontdb::ID> {
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: fontdb::Weight(requested_weight),
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    })?;
    let face = db.face(id)?;
    if face.style != fontdb::Style::Normal || face.stretch != fontdb::Stretch::Normal {
        return None;
    }
    if requested_weight == face_weight && face.weight.0 == face_weight {
        return Some(id);
    }

    // A variable face is rendered at the requested axis value. A differing
    // OS/2 weight is only an acceptable calibration for a static face.
    db.with_face_data(id, |data, index| {
        let font = skrifa::FontRef::from_index(data, index).ok()?;
        match font.axes().get_by_tag(skrifa::Tag::new(b"wght")) {
            Some(axis)
                if requested_weight == face_weight
                    && (axis.min_value()..=axis.max_value())
                        .contains(&(requested_weight as f32)) =>
            {
                Some(id)
            }
            None if face.weight.0 == face_weight => Some(id),
            _ => None,
        }
    })?
}

fn codepoint_ranges(mut codepoints: Vec<u32>) -> Vec<Range<u32>> {
    codepoints.sort_unstable();
    codepoints.dedup();
    let mut ranges: Vec<Range<u32>> = Vec::new();
    for codepoint in codepoints {
        if let Some(previous) = ranges.last_mut()
            && previous.end == codepoint
        {
            previous.end += 1;
        } else {
            ranges.push(codepoint..codepoint + 1);
        }
    }
    ranges
}

fn coverage_contains(coverage: &[Range<u32>], codepoint: u32) -> bool {
    coverage
        .get(coverage.partition_point(|range| range.end <= codepoint))
        .is_some_and(|range| range.contains(&codepoint))
}

fn is_modern_hangul_letter(character: char) -> bool {
    matches!(character as u32,
        0x1100..=0x1112 | 0x1161..=0x1175 | 0x11a8..=0x11c2 |
        0x3131..=0x3163 | 0xac00..=0xd7a3)
}

fn is_modern_hangul_cluster(grapheme: &str) -> bool {
    let mut letters = grapheme
        .chars()
        .filter(|&character| !is_hangul_mark(character));
    let characters = (
        letters.next(),
        letters.next(),
        letters.next(),
        letters.next(),
    );
    let leading = |character: char| (0x1100..=0x1112).contains(&(character as u32));
    let vowel = |character: char| (0x1161..=0x1175).contains(&(character as u32));
    let trailing = |character: char| (0x11a8..=0x11c2).contains(&(character as u32));

    match characters {
        (Some(character), None, None, None) => is_modern_hangul_letter(character),
        (Some(first), Some(second), None, None) => {
            (leading(first) && vowel(second))
                || ((0xac00..=0xd7a3).contains(&(first as u32))
                    && (first as u32 - 0xac00) % 28 == 0
                    && trailing(second))
        }
        (Some(first), Some(second), Some(third), None) => {
            leading(first) && vowel(second) && trailing(third)
        }
        _ => false,
    }
}

fn is_hangul_mark(character: char) -> bool {
    matches!(character as u32,
        0x0300..=0x036f | 0x302e..=0x302f | 0xfe00..=0xfe0f |
        0xe0100..=0xe01ef)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_glyph_keeps_complete_grapheme_in_regular_face() {
        let body = Font::new("Body");
        let light = body.weight(font::Weight::Light);
        let profile = FontProfile {
            body,
            adjustment: Some(WeightAdjustment {
                font: light,
                coverage: codepoint_ranges(
                    "骨漢「」\u{0300}".chars().map(|ch| ch as u32).collect(),
                ),
                scope: Scope::All,
            }),
        };
        assert_eq!(profile.font_for_grapheme("骨"), light);
        assert_eq!(profile.font_for_grapheme("骨\u{0300}"), light);
        for grapheme in ["⺀", "骨\u{0301}", "骨\u{e0100}", "骨\u{200d}"] {
            assert_eq!(profile.font_for_grapheme(grapheme), body, "{grapheme}");
        }
    }

    #[test]
    fn korean_profile_preserves_hanja_and_historical_clusters_even_if_covered() {
        let text = concat!(
            "한글 學校「」 ㄱㅏ 한 각 ",
            "ᄒᆞᆫ ᄀ가 각ᆨ ᅚᅡ ꥠᅡퟋ ᅟᅠힰ ",
            "한〮 한\u{0301} 한\u{1ab0} 한\u{200d} ㅥ"
        );
        let body = Font::new("Korean body");
        let light = body.weight(font::Weight::Light);
        let profile = FontProfile {
            body,
            adjustment: Some(WeightAdjustment {
                font: light,
                coverage: codepoint_ranges(
                    text.chars()
                        .filter(|&ch| ch != '\u{302e}' && ch != '\u{0301}')
                        .map(|ch| ch as u32)
                        .collect(),
                ),
                scope: Scope::ModernHangul,
            }),
        };
        for cluster in ["한", "글", "ㄱ", "ㅏ", "한", "각"] {
            assert_eq!(profile.font_for_grapheme(cluster), light, "{cluster}");
        }
        for cluster in [
            "學",
            "「",
            "ᄒᆞᆫ",
            "ᄀ가",
            "각ᆨ",
            "ᅚᅡ",
            "ꥠᅡퟋ",
            "ᅟᅠힰ",
            "한〮",
            "한\u{0301}",
            "한\u{1ab0}",
            "한\u{200d}",
            "ㅥ",
        ] {
            assert_eq!(profile.font_for_grapheme(cluster), body, "{cluster}");
        }
        assert_eq!(profile.preferred_font(), body);
    }

    #[test]
    fn queried_weight_must_resolve_to_the_profile_face() {
        let mut db = fontdb::Database::new();
        for weight in [290, 300, 400] {
            db.push_face_info(fontdb::FaceInfo {
                id: fontdb::ID::dummy(),
                source: fontdb::Source::Binary(std::sync::Arc::new(Vec::<u8>::new())),
                index: 0,
                families: vec![("Test".to_owned(), fontdb::Language::English_UnitedStates)],
                post_script_name: format!("Test-{weight}"),
                style: fontdb::Style::Normal,
                weight: fontdb::Weight(weight),
                stretch: fontdb::Stretch::Normal,
                monospaced: false,
            });
        }
        // Checking that 290 exists would succeed, but the request picks 300.
        assert!(verified_face(&db, "Test", 300, 290).is_none());
        assert!(verified_face(&db, "Test", 300, 300).is_some());
        assert!(font_weight_is_available(&db, "Test", 400));
    }
}
