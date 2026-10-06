//! Regional CJK font hints inferred from text, without changing its bytes.
//!
//! Han is a shared script, not a language identifier. These hints are deliberately
//! contextual: kana and Hangul establish a region, while a small set of Chinese
//! variant pairs only supplies evidence when that context is absent. See Unicode
//! UAX #24, UAX #38 §3.7.1, and https://www.unicode.org/faq/han_cjk.html.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use unicode_segmentation::UnicodeSegmentation;

use super::buffer::EditorBuffer;

const MAX_CONTEXT_BYTES: usize = 1024 * 1024;
const MAX_CONTEXT_LINES: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CjkLanguage {
    SimplifiedChinese,
    TraditionalChinese,
    Japanese,
    Korean,
}

impl CjkLanguage {
    pub const fn language_tag(self) -> &'static str {
        match self {
            Self::SimplifiedChinese => "zh-Hans",
            Self::TraditionalChinese => "zh-Hant",
            Self::Japanese => "ja",
            Self::Korean => "ko",
        }
    }

    fn is_japanese_or_korean(self) -> bool {
        matches!(self, Self::Japanese | Self::Korean)
    }
}

/// A UTF-8 byte range. `None` retains the editor's primary font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CjkRun {
    pub byte_range: Range<usize>,
    pub language: Option<CjkLanguage>,
}

/// Independent logical-line hints; constructing this scans a bounded prefix.
#[derive(Debug, Clone, Default)]
pub struct CjkContext {
    lines: Vec<LineContext>,
}

#[derive(Debug, Clone)]
struct LineContext {
    language: Option<CjkLanguage>,
    runs: Vec<CjkRun>,
}

impl LineContext {
    fn for_text(text: &str) -> Self {
        let evidence = Evidence::for_text(text);
        let language = is_heading(text)
            .then(|| heading_language(text))
            .flatten()
            .or(explicit_line_language(text))
            .or(evidence.language())
            .or(evidence.has_han.then_some(CjkLanguage::SimplifiedChinese));
        Self {
            language,
            runs: cjk_runs(text, language),
        }
    }
}

impl CjkContext {
    pub fn from_buffer(buffer: &EditorBuffer) -> Self {
        let mut sample = String::new();
        for chunk in buffer.chunks() {
            let remaining = MAX_CONTEXT_BYTES.saturating_sub(sample.len());
            if remaining == 0 {
                break;
            }
            let mut end = chunk.len().min(remaining);
            while !chunk.is_char_boundary(end) {
                end -= 1;
            }
            sample.push_str(&chunk[..end]);
            if end < chunk.len() {
                break;
            }
        }

        Self::from_sample(&sample)
    }

    /// Returns only this line's hint. Unsampled lines are detected locally by
    /// `cjk_runs` instead of inheriting evidence from another line.
    pub fn language_for_line(&self, line: usize) -> Option<CjkLanguage> {
        self.lines.get(line).and_then(|line| line.language)
    }

    pub fn runs_for_line(&self, line: usize, text: &str) -> Vec<CjkRun> {
        self.runs_for_fragment(line, 0, text)
    }

    /// Projects cached logical-line routes onto an original, unexpanded byte
    /// fragment. Wrapping and clipping therefore preserve cues outside the
    /// fragment, including Korean Hanja following Hangul on a Japanese line.
    pub fn runs_for_fragment(&self, line: usize, start_byte: usize, text: &str) -> Vec<CjkRun> {
        if text.is_empty() {
            return Vec::new();
        }
        let fallback = || cjk_runs(text, self.language_for_line(line));
        let Some(fragment_end) = start_byte.checked_add(text.len()) else {
            return fallback();
        };
        let Some(cached) = self.lines.get(line).map(|line| line.runs.as_slice()) else {
            return fallback();
        };
        if cached
            .last()
            .is_none_or(|run| run.byte_range.end < fragment_end)
        {
            return fallback();
        }

        let first = cached.partition_point(|run| run.byte_range.end <= start_byte);
        let mut runs = Vec::new();
        for run in cached[first..]
            .iter()
            .take_while(|run| run.byte_range.start < fragment_end)
        {
            let start = run.byte_range.start.max(start_byte) - start_byte;
            let end = run.byte_range.end.min(fragment_end) - start_byte;
            if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                // A modified/expanded fragment is not in logical-line offsets.
                return fallback();
            }
            push_run(&mut runs, start..end, run.language);
        }
        if runs.first().is_none_or(|run| run.byte_range.start != 0)
            || runs
                .last()
                .is_none_or(|run| run.byte_range.end != text.len())
        {
            return fallback();
        }
        runs
    }

    fn from_sample(sample: &str) -> Self {
        Self {
            lines: logical_lines(sample)
                .into_iter()
                .map(LineContext::for_text)
                .collect(),
        }
    }
}

/// A caller supplies both document identity and content revision. Returning an
/// `Arc` lets rendering retain the context without copying the per-line table.
#[derive(Debug, Clone, Default)]
pub struct CjkContextCache {
    key: Option<(u64, u64)>,
    context: Arc<CjkContext>,
}

impl CjkContextCache {
    pub fn get_or_update(
        &mut self,
        buffer: &EditorBuffer,
        document_key: u64,
        revision: u64,
    ) -> Arc<CjkContext> {
        let key = (document_key, revision);
        if self.key != Some(key) {
            self.context = Arc::new(CjkContext::from_buffer(buffer));
            self.key = Some(key);
        }
        Arc::clone(&self.context)
    }
}

/// Local evidence only. `CjkContext` retains logical-line cues across wrapping.
pub fn detect_language(text: &str) -> Option<CjkLanguage> {
    Evidence::for_text(text).language()
}

/// Covers the entire string contiguously, using original UTF-8 offsets. Script
/// cues stay within a sentence/clause or quotation. Ambiguous Han retains its
/// inherited region unless the logical line also establishes Chinese context.
pub fn cjk_runs(text: &str, inherited: Option<CjkLanguage>) -> Vec<CjkRun> {
    if text.is_empty() {
        return Vec::new();
    }
    if text.is_ascii() {
        return vec![CjkRun {
            byte_range: 0..text.len(),
            language: None,
        }];
    }

    let line_evidence = Evidence::for_text(text);
    let contextual_symbols = inherited.is_some() || text.chars().any(is_cjk_character);
    let label = explicit_line_language(text);
    let line_language = label
        .or(line_evidence.script_language())
        .or(inherited.filter(|language| language.is_japanese_or_korean()))
        .or(line_evidence.chinese_language())
        .or(inherited)
        .unwrap_or(CjkLanguage::SimplifiedChinese);

    let clauses = clause_ranges(text);
    let evidence: Vec<_> = clauses
        .iter()
        .map(|clause| Evidence::for_text(&text[clause.range.clone()]))
        .collect();
    let mut chinese_sentences = vec![false; clauses.last().map_or(0, |clause| clause.sentence + 1)];
    for (clause, evidence) in clauses.iter().zip(&evidence) {
        if label.is_none()
            && !clause.quoted
            && evidence.kana == 0
            && evidence.hangul == 0
            && establishes_chinese_context(
                &text[clause.range.clone()],
                *evidence,
                line_evidence.kana > 0 || line_evidence.hangul > 0,
            )
        {
            chinese_sentences[clause.sentence] = true;
        }
    }
    let mut previous = line_language;
    let mut runs = Vec::new();
    for (clause, evidence) in clauses.into_iter().zip(evidence) {
        let range = clause.range;
        let fragment = &text[range.clone()];
        let language = evidence
            .script_language()
            .or(chinese_adverb_context(fragment).then(|| {
                evidence.chinese_language().unwrap_or_else(|| {
                    if previous.is_japanese_or_korean() {
                        CjkLanguage::SimplifiedChinese
                    } else {
                        previous
                    }
                })
            }))
            .or(label.filter(|_| clause.sentence == 0))
            .or(
                (!clause.quoted && chinese_sentences[clause.sentence]).then(|| {
                    evidence.chinese_language().unwrap_or_else(|| {
                        if previous.is_japanese_or_korean() {
                            line_evidence
                                .chinese_language()
                                .unwrap_or(CjkLanguage::SimplifiedChinese)
                        } else {
                            previous
                        }
                    })
                }),
            )
            .or((!previous.is_japanese_or_korean())
                .then(|| evidence.chinese_language())
                .flatten())
            .unwrap_or(previous);
        let split_variants = !language.is_japanese_or_korean()
            && evidence.simplified > 0
            && evidence.traditional > 0;
        for run in clause_runs(fragment, language, contextual_symbols, split_variants) {
            push_run(
                &mut runs,
                range.start + run.byte_range.start..range.start + run.byte_range.end,
                run.language,
            );
        }
        if evidence.has_han || evidence.kana > 0 || evidence.hangul > 0 {
            previous = language;
        }
    }
    runs
}

/// Fullwidth comma is a clause boundary, whereas Japanese enumeration comma
/// and middle dot retain the shared cue for Kanji/Hanja lists.
struct Clause {
    range: Range<usize>,
    quoted: bool,
    sentence: usize,
}

fn clause_ranges(text: &str) -> Vec<Clause> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut quote_depth = 0usize;
    let mut sentence = 0;
    for (byte, grapheme) in text.grapheme_indices(true) {
        let Some(character) = grapheme.chars().next() else {
            continue;
        };
        if matches!(character, '「' | '『' | '“' | '‘') {
            if start < byte {
                ranges.push(Clause {
                    range: start..byte,
                    quoted: quote_depth > 0,
                    sentence,
                });
            }
            start = byte;
            quote_depth += 1;
        }
        if matches!(
            character,
            '。' | '，' | '；' | '！' | '？' | ',' | ';' | '!' | '?' | '」' | '』' | '”' | '’'
        ) {
            let end = byte + grapheme.len();
            ranges.push(Clause {
                range: start..end,
                quoted: quote_depth > 0,
                sentence,
            });
            start = end;
            if matches!(character, '」' | '』' | '”' | '’') {
                quote_depth = quote_depth.saturating_sub(1);
            } else if quote_depth == 0 && matches!(character, '。' | '！' | '？' | '!' | '?') {
                sentence += 1;
            }
        }
    }
    if start < text.len() {
        ranges.push(Clause {
            range: start..text.len(),
            quoted: quote_depth > 0,
            sentence,
        });
    }
    ranges
}

fn explicit_line_language(text: &str) -> Option<CjkLanguage> {
    let delimiter = text.find([':', '：'])?;
    let label = text[..delimiter].trim();
    if label.contains(['。', '，', '；', '「', '『']) {
        return None;
    }
    if label.starts_with("Hanja") || label.starts_with("韓國語") || label.starts_with("한국어")
    {
        Some(CjkLanguage::Korean)
    } else if label.starts_with("日本語") {
        Some(CjkLanguage::Japanese)
    } else {
        None
    }
}

fn establishes_chinese_context(text: &str, evidence: Evidence, allow_particles: bool) -> bool {
    if !evidence.has_han {
        return false;
    }
    if evidence.simplified > 0 && evidence.traditional > 0 {
        return true;
    }
    // These are generic grammatical clues, not a dictionary of fixture
    // phrases. They remain heuristic because Han does not encode language.
    if allow_particles && chinese_particle_context(text) {
        return true;
    }
    text.chars().any(|character| {
        chinese_variant_language(character) == Some(CjkLanguage::SimplifiedChinese)
            && !JAPANESE_SHARED_SIMPLIFIED_FORMS.contains(character)
    })
}

// Current counterpart-table SC forms also carrying kJoyoKanji in Unicode 17
// Unihan: https://www.unicode.org/Public/17.0.0/ucd/Unihan.zip; property semantics
// in UAX #38 https://www.unicode.org/reports/tr38/#kJoyoKanji. 云 is additionally
// retained as ambiguous because it is used in Japanese beyond the common-use
// list. Different Japanese meanings (机/据/云) still make shapes inconclusive.
const JAPANESE_SHARED_SIMPLIFIED_FORMS: &str = "万与会体写区医号国声学将据数旧机来温点画随静麦黄云";

fn chinese_particle_context(text: &str) -> bool {
    let han_count = text.chars().filter(|&character| is_han(character)).count();
    if han_count < 6 {
        return false;
    }
    let mut before = 0;
    for character in text.chars().filter(|&character| is_han(character)) {
        let after = han_count - before - 1;
        if before >= 2
            && after >= 2
            && matches!(
                character,
                '的' | '是' | '這' | '这' | '們' | '们' | '嗎' | '吗'
            )
        {
            return true;
        }
        before += 1;
    }
    false
}

/// 很 between Han characters is a useful grammatical cue even in a short
/// quotation. A lone 很 remains ambiguous, and kana/Hangul still has priority.
fn chinese_adverb_context(text: &str) -> bool {
    let han_count = text.chars().filter(|&character| is_han(character)).count();
    if han_count < 3 {
        return false;
    }
    let mut before = 0;
    for character in text.chars().filter(|&character| is_han(character)) {
        if character == '很' && before > 0 && before + 1 < han_count {
            return true;
        }
        before += 1;
    }
    false
}

fn clause_runs(
    text: &str,
    language: CjkLanguage,
    contextual_symbols: bool,
    split_variants: bool,
) -> Vec<CjkRun> {
    let mut groups: Vec<(Range<usize>, Evidence)> = Vec::new();
    let mut group_start = None;
    for (byte, grapheme) in text.grapheme_indices(true) {
        // Keep attached marks and joiners with their base, including marks
        // outside the CJK-specific blocks. Font coverage applies to the cluster.
        let cjk = grapheme.chars().next().is_some_and(|base| {
            is_cjk_character(base) || (contextual_symbols && is_contextual_symbol(base))
        });
        if cjk {
            group_start.get_or_insert(byte);
        } else if let Some(start) = group_start.take() {
            groups.push((start..byte, Evidence::for_text(&text[start..byte])));
        }
    }
    if let Some(start) = group_start {
        groups.push((start..text.len(), Evidence::for_text(&text[start..])));
    }

    let mut runs = Vec::new();
    let mut end = 0;
    let mut previous_script = None;
    let mut next_script = vec![None; groups.len()];
    let mut upcoming = None;
    for index in (0..groups.len()).rev() {
        upcoming = groups[index].1.script_language().or(upcoming);
        next_script[index] = upcoming;
    }
    for (index, (range, evidence)) in groups.iter().enumerate() {
        push_run(&mut runs, end..range.start, None);
        let nearby_script = previous_script.or(next_script[index]);
        let language = evidence
            .script_language()
            .or(nearby_script)
            .unwrap_or(language);

        let comparisons = evidence.simplified > 0
            && evidence.traditional > 0
            && CHINESE_VARIANT_PAIRS
                .iter()
                .any(|&(simplified, traditional)| {
                    text[range.clone()].contains(simplified)
                        && text[range.clone()].contains(traditional)
                });
        if (evidence.kana > 0 && evidence.hangul > 0) || split_variants || comparisons {
            push_grapheme_routes(
                &mut runs,
                text,
                range.clone(),
                language,
                split_variants || comparisons,
            );
        } else {
            push_run(&mut runs, range.clone(), Some(language));
        }
        previous_script = evidence.script_language().or(previous_script);
        end = range.end;
    }
    push_run(&mut runs, end..text.len(), None);
    runs
}

fn push_grapheme_routes(
    runs: &mut Vec<CjkRun>,
    text: &str,
    range: Range<usize>,
    fallback: CjkLanguage,
    split_variants: bool,
) {
    let graphemes: Vec<_> = text[range.clone()]
        .grapheme_indices(true)
        .map(|(byte, grapheme)| {
            let explicit = grapheme.chars().find_map(script_language).or_else(|| {
                split_variants
                    .then(|| grapheme.chars().find_map(chinese_variant_language))
                    .flatten()
            });
            (byte, grapheme, explicit)
        })
        .collect();
    let mut next = vec![None; graphemes.len()];
    let mut next_language = None;
    for index in (0..graphemes.len()).rev() {
        next_language = graphemes[index].2.or(next_language);
        next[index] = next_language;
    }
    let mut previous = None;
    for (index, &(relative_byte, grapheme, explicit)) in graphemes.iter().enumerate() {
        let language = explicit.or(previous).or(next[index]).unwrap_or(fallback);
        let byte = range.start + relative_byte;
        push_run(runs, byte..byte + grapheme.len(), Some(language));
        previous = explicit.or(previous);
    }
}

fn push_run(runs: &mut Vec<CjkRun>, byte_range: Range<usize>, language: Option<CjkLanguage>) {
    if byte_range.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.language == language
        && last.byte_range.end == byte_range.start
    {
        last.byte_range.end = byte_range.end;
        return;
    }
    runs.push(CjkRun {
        byte_range,
        language,
    });
}

#[derive(Debug, Default, Clone, Copy)]
struct Evidence {
    kana: usize,
    hangul: usize,
    simplified: usize,
    traditional: usize,
    bopomofo: usize,
    has_han: bool,
}

impl Evidence {
    fn for_text(text: &str) -> Self {
        let mut evidence = Self::default();
        for character in text.chars() {
            if character.is_ascii() {
                continue;
            }
            evidence.has_han |= is_han(character);
            if is_kana(character) {
                evidence.kana += 1;
            } else if is_hangul(character) {
                evidence.hangul += 1;
            } else if is_bopomofo(character) {
                evidence.bopomofo += 1;
            } else if let Some(language) = chinese_variant_language(character) {
                evidence.simplified += usize::from(language == CjkLanguage::SimplifiedChinese);
                evidence.traditional += usize::from(language == CjkLanguage::TraditionalChinese);
            }
        }
        evidence
    }

    fn script_language(self) -> Option<CjkLanguage> {
        if self.kana > 0 && self.kana >= self.hangul {
            Some(CjkLanguage::Japanese)
        } else if self.hangul > 0 {
            Some(CjkLanguage::Korean)
        } else if self.bopomofo > 0 {
            Some(CjkLanguage::TraditionalChinese)
        } else {
            None
        }
    }

    fn chinese_language(self) -> Option<CjkLanguage> {
        match self.simplified.cmp(&self.traditional) {
            std::cmp::Ordering::Greater => Some(CjkLanguage::SimplifiedChinese),
            std::cmp::Ordering::Less => Some(CjkLanguage::TraditionalChinese),
            std::cmp::Ordering::Equal => None,
        }
    }

    fn language(self) -> Option<CjkLanguage> {
        self.script_language().or(self.chinese_language())
    }
}

// Common, distinct SC/TC counterpart forms from the Unihan variant model.
// Context-dependent forms such as 后/台/面/干/里 are deliberately excluded.
// This is evidence, not a complete conversion table or a language classifier.
const CHINESE_VARIANT_PAIRS: &[(char, char)] = &[
    ('汉', '漢'),
    ('语', '語'),
    ('书', '書'),
    ('笔', '筆'),
    ('热', '熱'),
    ('树', '樹'),
    ('叶', '葉'),
    ('随', '隨'),
    ('风', '風'),
    ('轻', '輕'),
    ('摇', '搖'),
    ('动', '動'),
    ('远', '遠'),
    ('处', '處'),
    ('传', '傳'),
    ('渐', '漸'),
    ('声', '聲'),
    ('黄', '黃'),
    ('张', '張'),
    ('东', '東'),
    ('万', '萬'),
    ('识', '識'),
    ('现', '現'),
    ('历', '歷'),
    ('学', '學'),
    ('习', '習'),
    ('艺', '藝'),
    ('术', '術'),
    ('经', '經'),
    ('济', '濟'),
    ('会', '會'),
    ('时', '時'),
    ('间', '間'),
    ('为', '為'),
    ('价', '價'),
    ('号', '號'),
    ('国', '國'),
    ('门', '門'),
    ('车', '車'),
    ('马', '馬'),
    ('龙', '龍'),
    ('云', '雲'),
    ('电', '電'),
    ('气', '氣'),
    ('见', '見'),
    ('贝', '貝'),
    ('页', '頁'),
    ('鸟', '鳥'),
    ('鱼', '魚'),
    ('齐', '齊'),
    ('麦', '麥'),
    ('过', '過'),
    ('这', '這'),
    ('们', '們'),
    ('说', '說'),
    ('读', '讀'),
    ('写', '寫'),
    ('应', '應'),
    ('让', '讓'),
    ('从', '從'),
    ('众', '眾'),
    ('长', '長'),
    ('发', '發'),
    ('边', '邊'),
    ('开', '開'),
    ('关', '關'),
    ('实', '實'),
    ('对', '對'),
    ('乐', '樂'),
    ('静', '靜'),
    ('钢', '鋼'),
    ('铁', '鐵'),
    ('户', '戶'),
    ('编', '編'),
    ('构', '構'),
    ('与', '與'),
    ('观', '觀'),
    ('听', '聽'),
    ('乡', '鄉'),
    ('画', '畫'),
    ('机', '機'),
    ('无', '無'),
    ('体', '體'),
    ('来', '來'),
    ('网', '網'),
    ('线', '線'),
    ('点', '點'),
    ('旧', '舊'),
    ('亲', '親'),
    ('爱', '愛'),
    ('头', '頭'),
    ('广', '廣'),
    ('种', '種'),
    ('将', '將'),
    ('办', '辦'),
    ('业', '業'),
    ('认', '認'),
    ('达', '達'),
    ('选', '選'),
    ('阶', '階'),
    ('进', '進'),
    ('约', '約'),
    ('师', '師'),
    ('课', '課'),
    ('纸', '紙'),
    ('级', '級'),
    ('义', '義'),
    ('据', '據'),
    ('译', '譯'),
    ('减', '減'),
    ('数', '數'),
    ('变', '變'),
    ('战', '戰'),
    ('区', '區'),
    ('圆', '圓'),
    ('满', '滿'),
    ('医', '醫'),
    ('结', '結'),
    ('备', '備'),
    ('岁', '歲'),
    ('啰', '囉'),
    ('吗', '嗎'),
    ('唤', '喚'),
    ('欢', '歡'),
    ('阳', '陽'),
    ('觉', '覺'),
    ('斋', '齋'),
    ('册', '冊'),
    ('温', '溫'),
    ('简', '簡'),
    ('梦', '夢'),
    ('测', '測'),
    ('试', '試'),
];

fn chinese_variant_language(character: char) -> Option<CjkLanguage> {
    static MARKERS: OnceLock<HashMap<char, CjkLanguage>> = OnceLock::new();
    MARKERS
        .get_or_init(|| {
            CHINESE_VARIANT_PAIRS
                .iter()
                .flat_map(|&(simplified, traditional)| {
                    [
                        (simplified, CjkLanguage::SimplifiedChinese),
                        (traditional, CjkLanguage::TraditionalChinese),
                    ]
                })
                .collect()
        })
        .get(&character)
        .copied()
}

fn script_language(character: char) -> Option<CjkLanguage> {
    if is_kana(character) {
        Some(CjkLanguage::Japanese)
    } else if is_hangul(character) {
        Some(CjkLanguage::Korean)
    } else {
        None
    }
}

fn is_kana(character: char) -> bool {
    matches!(character as u32,
        0x3041..=0x3096 | 0x309d..=0x309f | 0x30a1..=0x30fa |
        0x30fd..=0x30ff | 0x31f0..=0x31ff | 0xff66..=0xff9d |
        0x1aff0..=0x1afff | 0x1b000..=0x1b122 | 0x1b130..=0x1b16f)
}

fn is_hangul(character: char) -> bool {
    matches!(character as u32,
        0x1100..=0x11ff | 0x3131..=0x318e | 0xa960..=0xa97c |
        0xac00..=0xd7a3 | 0xd7b0..=0xd7c6 | 0xd7cb..=0xd7fb |
        0xffa0..=0xffdc)
}

fn is_han(character: char) -> bool {
    matches!(character as u32,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff |
        0x20000..=0x2ee5f | 0x2f800..=0x2fa1f | 0x30000..=0x3347f |
        0x3005 | 0x3007)
}

fn is_bopomofo(character: char) -> bool {
    matches!(character as u32, 0x3105..=0x312f | 0x31a0..=0x31bf)
}

fn is_cjk_character(character: char) -> bool {
    is_han(character)
        || is_kana(character)
        || is_hangul(character)
        || is_bopomofo(character)
        || matches!(character as u32,
            0x2e80..=0x2eff | 0x2f00..=0x2fdf | 0x3000..=0x303f |
            0x3040..=0x30ff |
            0x3200..=0x33ff | 0xfe30..=0xfe4f | 0xff01..=0xff60 |
            0xff61..=0xff65 | 0xffe0..=0xffe6)
}

fn is_contextual_symbol(character: char) -> bool {
    matches!(character, '¥' | '·' | '—' | '…') || matches!(character as u32, 0x2460..=0x24ff)
}

fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(u8::is_ascii_whitespace)
}

fn heading_language(line: &str) -> Option<CjkLanguage> {
    if line.contains("日本語") || line.contains("Japanese") {
        Some(CjkLanguage::Japanese)
    } else if line.contains("한국어") || line.contains("Korean") {
        Some(CjkLanguage::Korean)
    } else if line.contains("繁體") || line.contains("繁体") || line.contains("Traditional Chinese")
    {
        Some(CjkLanguage::TraditionalChinese)
    } else if line.contains("简体") || line.contains("簡體") || line.contains("Simplified Chinese")
    {
        Some(CjkLanguage::SimplifiedChinese)
    } else {
        None
    }
}

/// EditorBuffer treats CRLF and LFCR as one logical ending; mirror that here.
fn logical_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() && lines.len() < MAX_CONTEXT_LINES {
        if matches!(bytes[index], b'\r' | b'\n') {
            lines.push(&text[start..index]);
            let ending = bytes[index];
            index += 1;
            if index < bytes.len()
                && matches!(bytes[index], b'\r' | b'\n')
                && bytes[index] != ending
            {
                index += 1;
            }
            start = index;
        } else {
            index += 1;
        }
    }
    if lines.len() < MAX_CONTEXT_LINES {
        lines.push(&text[start..]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_needle_language(
        context: &CjkContext,
        line: usize,
        text: &str,
        needle: &str,
        language: CjkLanguage,
    ) {
        let start = text.find(needle).expect("fixture probe");
        let runs = context.runs_for_fragment(line, start, needle);
        for (byte, character) in needle
            .char_indices()
            .filter(|(_, character)| is_cjk_character(*character))
        {
            assert_eq!(
                runs.iter()
                    .find(|run| run.byte_range.contains(&byte))
                    .unwrap()
                    .language,
                Some(language),
                "{needle}: {character}"
            );
        }
    }

    #[test]
    fn exact_mixed_fixture_keeps_sentence_and_quoted_language_context() {
        let fixture = include_str!("../../tests/fixtures/cjk/mixed.txt");
        let buffer = EditorBuffer::from_text(fixture);
        let context = CjkContext::from_buffer(&buffer);
        let first = buffer.line_text(0).unwrap();
        for (needle, language) in [
            ("清晨的城市", CjkLanguage::TraditionalChinese),
            ("漸", CjkLanguage::TraditionalChinese),
            ("经", CjkLanguage::SimplifiedChinese),
            ("發", CjkLanguage::TraditionalChinese),
            ("東京駅", CjkLanguage::Japanese),
            ("学生", CjkLanguage::Japanese),
            ("学校", CjkLanguage::Japanese),
            ("地下鐵", CjkLanguage::Korean),
            ("驛", CjkLanguage::Korean),
            ("會社", CjkLanguage::Korean),
            ("學校", CjkLanguage::Korean),
        ] {
            assert_needle_language(&context, 0, &first, needle, language);
        }
        let second = buffer.line_text(2).unwrap();
        assert_needle_language(
            &context,
            2,
            &second,
            "歡迎光臨",
            CjkLanguage::TraditionalChinese,
        );
        assert_needle_language(&context, 2, &second, "旁边", CjkLanguage::SimplifiedChinese);
        assert_needle_language(
            &context,
            2,
            &second,
            "文化・藝術・科學技術",
            CjkLanguage::Korean,
        );
        let signs = buffer.line_text(4).unwrap();
        for needle in ["博物館入口", "東門", "圖書館", "西門", "出口 出口 出口"] {
            assert_needle_language(&context, 4, &signs, needle, CjkLanguage::Japanese);
        }
        let last_line = (0..buffer.line_count())
            .find(|&line| buffer.line_text(line).unwrap().starts_with("最後"))
            .unwrap();
        let last = buffer.line_text(last_line).unwrap();
        for (needle, language) in [
            ("大家在門口互相說", CjkLanguage::TraditionalChinese),
            ("再見", CjkLanguage::TraditionalChinese),
            ("再见", CjkLanguage::SimplifiedChinese),
            ("また会いましょう", CjkLanguage::Japanese),
            ("다음에 또 만나요", CjkLanguage::Korean),
            (
                "然後各自走向不同的城市與新的旅程",
                CjkLanguage::TraditionalChinese,
            ),
        ] {
            assert_needle_language(&context, last_line, &last, needle, language);
        }
    }

    #[test]
    fn explicit_variant_comparisons_and_language_labels_are_local() {
        let fixture = include_str!("../../tests/fixtures/cjk/mixed.txt");
        let buffer = EditorBuffer::from_text(fixture);
        let context = CjkContext::from_buffer(&buffer);
        let pairs = buffer.line_text(6).unwrap();
        for (traditional, simplified) in [
            ("國", "国"),
            ("學", "学"),
            ("體", "体"),
            ("夢", "梦"),
            ("廣", "广"),
        ] {
            assert_needle_language(
                &context,
                6,
                &pairs,
                traditional,
                CjkLanguage::TraditionalChinese,
            );
            assert_needle_language(
                &context,
                6,
                &pairs,
                simplified,
                CjkLanguage::SimplifiedChinese,
            );
        }
        let japanese = buffer.line_text(7).unwrap();
        for needle in ["電車", "駅", "図書館", "東京", "京都"] {
            assert_needle_language(&context, 7, &japanese, needle, CjkLanguage::Japanese);
        }
        let korean = buffer.line_text(8).unwrap();
        for needle in ["韓國", "文化", "歷史", "學校", "時間", "未來"] {
            assert_needle_language(&context, 8, &korean, needle, CjkLanguage::Korean);
        }
        let quoted = buffer.line_text(11).unwrap();
        for (needle, language) in [
            ("學", CjkLanguage::TraditionalChinese),
            ("学", CjkLanguage::SimplifiedChinese),
            ("학", CjkLanguage::Korean),
        ] {
            assert_needle_language(&context, 11, &quoted, needle, language);
        }
        let worlds = buffer.line_text(10).unwrap();
        assert_needle_language(&context, 10, &worlds, "世界は広い", CjkLanguage::Japanese);
        assert_needle_language(
            &context,
            10,
            &worlds,
            "世界很大",
            CjkLanguage::SimplifiedChinese,
        );
        assert_needle_language(&context, 10, &worlds, "세계는 넓다", CjkLanguage::Korean);
        let label = buffer.line_text(13).unwrap();
        for (needle, language) in [
            ("測試", CjkLanguage::TraditionalChinese),
            ("测试", CjkLanguage::SimplifiedChinese),
            ("テスト", CjkLanguage::Japanese),
            ("시험", CjkLanguage::Korean),
        ] {
            assert_needle_language(&context, 13, &label, needle, language);
        }
    }

    #[test]
    fn grammatical_han_words_do_not_override_japanese_or_korean_context() {
        for language in [CjkLanguage::Japanese, CjkLanguage::Korean] {
            for text in [
                "目的地",
                "是非",
                "世界目的地",
                "書齋藝術天地",
                "温",
                "机",
                "写",
                "黄",
                "声",
                "随",
                "号",
                "医",
                "云",
                "很",
            ] {
                assert!(
                    cjk_runs(text, Some(language))
                        .iter()
                        .all(|run| run.language == Some(language)),
                    "{text}: {language:?}"
                );
            }
        }
        let text = "日本語では「目的地」「是非」を使う。";
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(text));
        for needle in ["目的地", "是非"] {
            assert_needle_language(&context, 0, text, needle, CjkLanguage::Japanese);
        }
        let text = "日本語で「温」「机」「写」「声」「随」「黄」と書く。";
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(text));
        for needle in ["温", "机", "写", "声", "随", "黄"] {
            assert_needle_language(&context, 0, text, needle, CjkLanguage::Japanese);
        }
        assert_eq!(
            explicit_line_language("我們研究日本語和韓國語：文字的歷史"),
            None
        );
        let text = "日本語で「很」「目的地」と書く。";
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(text));
        for needle in ["很", "目的地"] {
            assert_needle_language(&context, 0, text, needle, CjkLanguage::Japanese);
        }
        let ambiguous = "「世界は広い，世界天地，세계는 넓다。」";
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(ambiguous));
        // Shared Han plus no language-specific evidence retains the preceding
        // Japanese context. This is a fallback, not proof that it is Japanese.
        assert_needle_language(&context, 0, ambiguous, "世界天地", CjkLanguage::Japanese);
    }

    #[test]
    fn script_evidence_overrides_shared_han_forms() {
        assert_eq!(
            detect_language("清晨，陽光穿過窗戶，照在安靜的書桌上。"),
            Some(CjkLanguage::TraditionalChinese)
        );
        assert_eq!(
            detect_language("清晨，阳光穿过窗户，照在安静的书桌上。"),
            Some(CjkLanguage::SimplifiedChinese)
        );
        assert_eq!(
            detect_language("早朝、陽光が窓から差し込み、静かな書斎。"),
            Some(CjkLanguage::Japanese)
        );
        assert_eq!(
            detect_language("早朝의 햇살이 窓門을 지나 고요한 書齋。"),
            Some(CjkLanguage::Korean)
        );
        assert_eq!(detect_language("天地宇宙日月"), None);
        assert_eq!(detect_language("let ordinary_ascii = 123;"), None);
    }

    #[test]
    fn neutral_han_is_not_chinese_variant_evidence() {
        assert_eq!(
            detect_language("天地宇宙日月窗口甦醒社會"),
            Some(CjkLanguage::TraditionalChinese)
        );
        assert_eq!(
            detect_language("天地宇宙日月窗口醒社会"),
            Some(CjkLanguage::SimplifiedChinese)
        );
        assert_eq!(detect_language("天地宇宙日月窗口甦醒社"), None);
        let simplified: std::collections::HashSet<_> = CHINESE_VARIANT_PAIRS
            .iter()
            .map(|&(simplified, _)| simplified)
            .collect();
        let traditional: std::collections::HashSet<_> = CHINESE_VARIANT_PAIRS
            .iter()
            .map(|&(_, traditional)| traditional)
            .collect();
        assert_eq!(simplified.len(), CHINESE_VARIANT_PAIRS.len());
        assert_eq!(traditional.len(), CHINESE_VARIANT_PAIRS.len());
        assert!(simplified.is_disjoint(&traditional));
    }

    #[test]
    fn adjacent_lines_detect_their_own_language_without_document_fallback() {
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(
            "日本語の漢字かな\n清晨，陽光穿過窗戶，照在安靜的書桌上。\n清晨，阳光穿过窗户，照在安静的书桌上。\n天地宇宙日月\nASCII\nPrice ¥12 — ①",
        ));
        assert_eq!(context.language_for_line(0), Some(CjkLanguage::Japanese));
        assert_eq!(
            context.language_for_line(1),
            Some(CjkLanguage::TraditionalChinese)
        );
        assert_eq!(
            context.language_for_line(2),
            Some(CjkLanguage::SimplifiedChinese)
        );
        assert_eq!(
            context.language_for_line(3),
            Some(CjkLanguage::SimplifiedChinese)
        );
        assert!(
            context
                .runs_for_line(3, "天地宇宙日月")
                .iter()
                .all(|run| { run.language == Some(CjkLanguage::SimplifiedChinese) })
        );
        assert_eq!(context.language_for_line(4), None);
        assert_eq!(context.language_for_line(5), None);
        assert!(
            context
                .runs_for_line(5, "Price ¥12 — ①")
                .iter()
                .all(|run| { run.language.is_none() })
        );
    }

    #[test]
    fn generic_heading_does_not_override_independent_lines() {
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(
            "# Notes\n\n繁體中文，書與藝術。\n\n日本語の漢字かな。\n\n한국어와 漢字。",
        ));
        assert_eq!(
            context.language_for_line(2),
            Some(CjkLanguage::TraditionalChinese)
        );
        assert_eq!(context.language_for_line(4), Some(CjkLanguage::Japanese));
        assert_eq!(context.language_for_line(6), Some(CjkLanguage::Korean));
    }

    #[test]
    fn cjk_symbols_follow_the_region_but_latin_only_symbols_keep_the_primary_font() {
        let text = "書，¥12、①②③，文化·歷史——……";
        let runs = cjk_runs(text, Some(CjkLanguage::TraditionalChinese));
        for (byte, character) in text.char_indices() {
            if is_contextual_symbol(character) {
                let run = runs
                    .iter()
                    .find(|run| run.byte_range.contains(&byte))
                    .unwrap();
                assert_eq!(run.language, Some(CjkLanguage::TraditionalChinese));
            }
        }
        let latin = "Price ¥12 · note — next … ①";
        assert_eq!(
            cjk_runs(latin, None),
            vec![CjkRun {
                byte_range: 0..latin.len(),
                language: None
            }]
        );
        let japanese = "文化・歴史のキロメートル";
        assert_eq!(
            cjk_runs(japanese, None),
            vec![CjkRun {
                byte_range: 0..japanese.len(),
                language: Some(CjkLanguage::Japanese)
            }]
        );
    }

    #[test]
    fn headings_only_route_their_own_line() {
        let text = "### 中文｜汉字\n\n陽光穿過窗戶，書與藝術。\n\n天地宇宙日月\n\n### 日本語｜漢字＋かな\n\n早朝、陽光が窓から差し込む。\n\n書齋藝術天地\n\n### 한국어｜한글＋漢字\n\n早朝의 햇살이 窓門을 지나。\n\n書齋藝術天地";
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(text));
        for (line, language) in [
            (0, CjkLanguage::SimplifiedChinese),
            (2, CjkLanguage::TraditionalChinese),
            (4, CjkLanguage::SimplifiedChinese),
            (6, CjkLanguage::Japanese),
            (8, CjkLanguage::Japanese),
            (10, CjkLanguage::TraditionalChinese),
            (12, CjkLanguage::Korean),
            (14, CjkLanguage::Korean),
            (16, CjkLanguage::TraditionalChinese),
        ] {
            assert_eq!(context.language_for_line(line), Some(language));
        }
        for line in (1..16).step_by(2) {
            assert_eq!(context.language_for_line(line), None);
        }
    }

    #[test]
    fn neighboring_kana_and_hangul_do_not_override_han_only_line() {
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(
            "書齋藝術天地\n早朝、陽光が窓から差し込む。\n早朝의 햇살。",
        ));
        assert_eq!(
            context.language_for_line(0),
            Some(CjkLanguage::TraditionalChinese)
        );
        assert_eq!(context.language_for_line(1), Some(CjkLanguage::Japanese));
        assert_eq!(context.language_for_line(2), Some(CjkLanguage::Korean));
    }

    #[test]
    fn byte_ranges_preserve_latin_and_unicode_sequences() {
        let text = "A漢\u{e0100}字かな / 書齋한글\u{11a8} B";
        let runs = cjk_runs(text, None);
        assert_eq!(runs.first().unwrap().byte_range.start, 0);
        assert_eq!(runs.last().unwrap().byte_range.end, text.len());
        for pair in runs.windows(2) {
            assert_eq!(pair[0].byte_range.end, pair[1].byte_range.start);
        }
        for run in &runs {
            assert!(text.is_char_boundary(run.byte_range.start));
            assert!(text.is_char_boundary(run.byte_range.end));
        }
        assert!(
            runs.iter()
                .any(|run| run.language == Some(CjkLanguage::Japanese))
        );
        assert!(
            runs.iter()
                .any(|run| run.language == Some(CjkLanguage::Korean))
        );
        assert!(
            runs.iter()
                .filter(|run| run.language.is_none())
                .all(|run| text[run.byte_range.clone()].is_ascii())
        );
        assert!(
            runs.iter()
                .any(|run| text[run.byte_range.clone()].contains("漢\u{e0100}"))
        );
    }

    #[test]
    fn cjk_graphemes_keep_attached_marks_and_joiners_with_ascii_neighbors() {
        for (cluster, language) in [
            ("가\u{1ab0}", CjkLanguage::Korean),
            ("가\u{200d}", CjkLanguage::Korean),
            ("한\u{0301}", CjkLanguage::Korean),
            ("가\u{3099}", CjkLanguage::Korean),
            ("漢\u{e0100}", CjkLanguage::Japanese),
            ("か\u{3099}", CjkLanguage::Japanese),
        ] {
            assert_eq!(cluster.graphemes(true).count(), 1);
            let text = format!("A{cluster}B");
            assert_eq!(
                cjk_runs(&text, Some(language)),
                vec![
                    CjkRun {
                        byte_range: 0..1,
                        language: None,
                    },
                    CjkRun {
                        byte_range: 1..1 + cluster.len(),
                        language: Some(language),
                    },
                    CjkRun {
                        byte_range: 1 + cluster.len()..text.len(),
                        language: None,
                    },
                ],
                "{cluster:?}"
            );
        }
        for latin in ["A\u{1ab0}B", "a\u{3099}B", "é\u{3099}B"] {
            assert_eq!(
                cjk_runs(latin, None),
                vec![CjkRun {
                    byte_range: 0..latin.len(),
                    language: None,
                }],
                "{latin:?}"
            );
        }
    }

    #[test]
    fn mixed_script_and_variant_routes_end_only_at_grapheme_boundaries() {
        let text = "A國\u{e0100}国\u{0301}かな한\u{3099}\u{1ab0}B";
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(text));
        let boundaries: Vec<_> = text
            .grapheme_indices(true)
            .map(|(byte, _)| byte)
            .chain(std::iter::once(text.len()))
            .collect();
        for runs in [cjk_runs(text, None), context.runs_for_line(0, text)] {
            assert_eq!(runs.first().unwrap().byte_range.start, 0);
            assert_eq!(runs.last().unwrap().byte_range.end, text.len());
            for run in &runs {
                assert!(boundaries.contains(&run.byte_range.start));
                assert!(boundaries.contains(&run.byte_range.end));
            }
            for pair in runs.windows(2) {
                assert_eq!(pair[0].byte_range.end, pair[1].byte_range.start);
            }
            assert!(runs.iter().any(|run| {
                run.language == Some(CjkLanguage::Korean)
                    && text[run.byte_range.clone()].contains("한\u{3099}\u{1ab0}")
            }));
        }
    }

    #[test]
    fn punctuation_and_quote_clauses_preserve_attached_grapheme_marks() {
        for text in [
            "A한。\u{1ab0}글B",
            "A「한」\u{1ab0}B",
            "A「한」\u{200d}B",
            "A한，\u{0301}글B",
            "A한!\u{1ab0}글B",
        ] {
            let boundaries: Vec<_> = text
                .grapheme_indices(true)
                .map(|(byte, _)| byte)
                .chain(std::iter::once(text.len()))
                .collect();
            let clauses = clause_ranges(text);
            for clause in &clauses {
                assert!(boundaries.contains(&clause.range.start), "{text:?}");
                assert!(boundaries.contains(&clause.range.end), "{text:?}");
            }
            for pair in clauses.windows(2) {
                assert_eq!(pair[0].range.end, pair[1].range.start);
            }
            let runs = cjk_runs(text, None);
            for run in &runs {
                assert!(boundaries.contains(&run.byte_range.start), "{text:?}");
                assert!(boundaries.contains(&run.byte_range.end), "{text:?}");
            }
            for pair in runs.windows(2) {
                assert_eq!(pair[0].byte_range.end, pair[1].byte_range.start);
            }
        }
    }

    #[test]
    fn han_only_runs_inherit_japanese_and_korean_over_chinese_variant_markers() {
        for language in [CjkLanguage::Japanese, CjkLanguage::Korean] {
            let runs = cjk_runs("書齋藝術天地", Some(language));
            assert_eq!(
                runs,
                vec![CjkRun {
                    byte_range: 0.."書齋藝術天地".len(),
                    language: Some(language)
                }]
            );
        }
    }

    #[test]
    fn mixed_scripts_with_no_separator_get_independent_font_runs() {
        let text = "日本語かな한국어漢字";
        let runs = cjk_runs(text, None);
        assert_eq!(runs.len(), 2);
        assert_eq!(&text[runs[0].byte_range.clone()], "日本語かな");
        assert_eq!(runs[0].language, Some(CjkLanguage::Japanese));
        assert_eq!(&text[runs[1].byte_range.clone()], "한국어漢字");
        assert_eq!(runs[1].language, Some(CjkLanguage::Korean));
    }

    #[test]
    fn wrapped_han_fragment_retains_the_original_nearby_script() {
        for (prefix, cue, dominant, trailing) in [
            ("かな", "한국어", CjkLanguage::Japanese, CjkLanguage::Korean),
            ("한국어", "かな", CjkLanguage::Korean, CjkLanguage::Japanese),
        ] {
            let suffix = "天地書齋".repeat(100);
            let text = format!("{} / {cue} {suffix}", prefix.repeat(16));
            let context = CjkContext::from_buffer(&EditorBuffer::from_text(text.clone()));
            assert_eq!(context.language_for_line(0), Some(dominant));
            let start = text.len() - suffix.len() + "天".len();
            let fragment = &text[start..text.len() - "齋".len()];
            let runs = context.runs_for_fragment(0, start, fragment);
            assert_eq!(
                runs,
                vec![CjkRun {
                    byte_range: 0..fragment.len(),
                    language: Some(trailing),
                }]
            );
            // Re-detecting only this Han fragment would lose the nearby cue.
            assert_ne!(runs, cjk_runs(fragment, Some(dominant)));
        }
    }

    #[test]
    fn clipped_cached_runs_cover_relative_utf8_offsets_contiguously() {
        let text = "日本語かな / 한국어 天地書齋 / ASCII";
        let context = CjkContext::from_buffer(&EditorBuffer::from_text(text));
        let start = "日本語".len();
        let end = text.len() - "ASCII".len();
        let fragment = &text[start..end];
        let runs = context.runs_for_fragment(0, start, fragment);
        assert_eq!(runs.first().unwrap().byte_range.start, 0);
        assert_eq!(runs.last().unwrap().byte_range.end, fragment.len());
        for pair in runs.windows(2) {
            assert_eq!(pair[0].byte_range.end, pair[1].byte_range.start);
        }
        for run in &runs {
            assert!(fragment.is_char_boundary(run.byte_range.start));
            assert!(fragment.is_char_boundary(run.byte_range.end));
        }
        assert!(
            runs.iter()
                .any(|run| run.language == Some(CjkLanguage::Japanese))
        );
        assert!(
            runs.iter()
                .any(|run| run.language == Some(CjkLanguage::Korean))
        );
        assert!(runs.iter().any(|run| run.language.is_none()));
        let han_byte = fragment.find("天地").unwrap();
        assert_eq!(
            runs.iter()
                .find(|run| run.byte_range.contains(&han_byte))
                .unwrap()
                .language,
            Some(CjkLanguage::Korean)
        );
    }

    #[test]
    fn incomplete_cached_fragment_coverage_uses_local_evidence() {
        let context = CjkContext::from_sample("日本語かな");
        let korean = "한국어漢字";
        let expected = cjk_runs(korean, context.language_for_line(0));
        assert_eq!(
            context.runs_for_fragment(0, "日本語かな".len(), korean),
            expected
        );
        assert_eq!(context.runs_for_fragment(99, 0, korean), expected);
        assert_eq!(context.runs_for_fragment(0, usize::MAX, korean), expected);
    }

    #[test]
    fn all_editor_line_endings_keep_the_same_line_indices() {
        let text = "### 日本語\r\n書齋\n\rかな\r漢字\n";
        let buffer = EditorBuffer::from_text(text);
        let context = CjkContext::from_buffer(&buffer);
        assert_eq!(context.lines.len(), buffer.line_count());
        assert_eq!(
            (0..buffer.line_count())
                .map(|line| context.language_for_line(line))
                .collect::<Vec<_>>(),
            vec![
                Some(CjkLanguage::Japanese),
                Some(CjkLanguage::TraditionalChinese),
                Some(CjkLanguage::Japanese),
                Some(CjkLanguage::TraditionalChinese),
                None,
            ]
        );
    }

    #[test]
    fn cache_reuses_a_revision_and_invalidates_on_document_identity_or_edit() {
        let mut cache = CjkContextCache::default();
        let japanese = EditorBuffer::from_text("日本語かな");
        let korean = EditorBuffer::from_text("한국어漢字");
        let first = cache.get_or_update(&japanese, 1, 0);
        let repeated = cache.get_or_update(&japanese, 1, 0);
        assert!(Arc::ptr_eq(&first, &repeated));
        let another_document = cache.get_or_update(&korean, 2, 0);
        assert!(!Arc::ptr_eq(&first, &another_document));
        assert_eq!(
            another_document.language_for_line(0),
            Some(CjkLanguage::Korean)
        );
        let edited = cache.get_or_update(&japanese, 2, 1);
        assert!(!Arc::ptr_eq(&another_document, &edited));
        assert_eq!(edited.language_for_line(0), Some(CjkLanguage::Japanese));
    }

    #[test]
    fn bounded_context_still_detects_unsampled_local_script() {
        let buffer =
            EditorBuffer::from_text(format!("{}\n한국어漢字", "天".repeat(MAX_CONTEXT_BYTES)));
        let context = CjkContext::from_buffer(&buffer);
        assert_eq!(
            context.language_for_line(0),
            Some(CjkLanguage::SimplifiedChinese)
        );
        let runs = context.runs_for_line(1, "한국어漢字");
        assert!(
            runs.iter()
                .all(|run| run.language == Some(CjkLanguage::Korean))
        );
        assert!(context.lines.len() <= MAX_CONTEXT_LINES);
    }

    #[test]
    fn unsampled_lines_do_not_inherit_cached_language() {
        let buffer = EditorBuffer::from_text(format!(
            "{}骨直令\n한국어漢字",
            "かな\n".repeat(MAX_CONTEXT_LINES)
        ));
        let context = CjkContext::from_buffer(&buffer);
        assert_eq!(context.language_for_line(MAX_CONTEXT_LINES), None);
        assert!(
            context
                .runs_for_line(MAX_CONTEXT_LINES, "骨直令")
                .iter()
                .all(|run| { run.language == Some(CjkLanguage::SimplifiedChinese) })
        );
        assert!(
            context
                .runs_for_line(MAX_CONTEXT_LINES + 1, "한국어漢字")
                .iter()
                .all(|run| { run.language == Some(CjkLanguage::Korean) })
        );
    }
}
