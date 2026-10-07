//! Render CJK fixtures through the real editor, including its wrap geometry.
//! `cargo run --locked --example preview_cjk`
//! `cargo run --locked --example preview_cjk -- --vulkan`
//! Add `--weights` to compare actual native font faces and optical weight.
//! Add `--hangul-weights` to compare Hangul-only calibration with regular Hanja.
//! Editor screenshots stay under target/cjk-review/ or target/cjk-review-vulkan/.
//! Weight comparisons stay under target/cjk-weight-review/ (or its Vulkan sibling).

use fragile_notepad::{
    core::{AppearanceMode, Document, DocumentId, EditorSettings},
    editor::{
        EditorAction, EditorBuffer,
        cjk::{CjkContext, CjkLanguage, cjk_runs},
        widget::{
            EDITOR_FONT, EditorFontRun, EditorStyle, editor_font_runs,
            editor_font_runs_from_cjk_runs, regional_cjk_font,
        },
    },
    message::Message,
    ui::{editor, styles},
};
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, graphics, layout, mouse, text};
use iced::{
    Color, Event, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, font, window,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

const CHINESE: &str = include_str!("../tests/fixtures/cjk/chinese.txt");
const JAPANESE: &str = include_str!("../tests/fixtures/cjk/japanese.txt");
const KOREAN: &str = include_str!("../tests/fixtures/cjk/korean.txt");
const USER_MIXED: &str = include_str!("../tests/fixtures/cjk/mixed.txt");

// Each Han line carries its own regional cue. The identical characters make
// regional glyph differences easy to inspect without neighboring-line hints.
const REGIONAL_SHAPES: &str = "Traditional Chinese / 中文繁體漢字\n繁體：骨 直 令 曜 角 返 門 海 雨\n\nSimplified Chinese / 中文简体汉字\n简体：骨 直 令 曜 角 返 门 海 雨\n\nJapanese / 日本語の漢字\n日本語：骨 直 令 曜 角 返 門 海 雨\n\nKorean / 한국어 한글과 漢字\n한국어：骨 直 令 曜 角 返 門 海 雨\n";

// The final Han-only fragments wrap after the final script cue. Their font
// must retain that cue even when the cue is absent from the displayed row.
const MIXED_SCRIPT_LINES: &str = "Japanese to Korean / JP -> KR\n日本語のかな、あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほ 한국어: 骨 直 令 曜 天地宇宙 日月星辰 春夏秋冬 東西南北 山川河海 風雨雷電 骨 直 令 曜 天地宇宙 日月星辰 春夏秋冬 東西南北 山川河海 風雨雷電 骨 直 令 曜\n\nKorean to Japanese / KR -> JP\n한국어 한글과 한국어 한글과 한국어 한글과 한국어 한글과 한국어 한글과 한국어 한글과 日本語のかな: 骨 直 令 曜 天地宇宙 日月星辰 春夏秋冬 東西南北 山川河海 風雨雷電 骨 直 令 曜 天地宇宙 日月星辰 春夏秋冬 東西南北 山川河海 風雨雷電 骨 直 令 曜\n";

fn main() {
    let vulkan = std::env::args().any(|argument| argument == "--vulkan");
    let weights = std::env::args().any(|argument| argument == "--weights");
    let hangul_weights = std::env::args().any(|argument| argument == "--hangul-weights");
    if vulkan {
        // Set the backend before a renderer or any worker threads exist.
        unsafe { std::env::set_var("WGPU_BACKEND", "vulkan") };
    }
    let output = Path::new(if hangul_weights && vulkan {
        "target/cjk-hangul-weight-review-vulkan"
    } else if hangul_weights {
        "target/cjk-hangul-weight-review"
    } else if weights && vulkan {
        "target/cjk-weight-review-vulkan"
    } else if weights {
        "target/cjk-weight-review"
    } else if vulkan {
        "target/cjk-review-vulkan"
    } else {
        "target/cjk-review"
    });
    std::fs::create_dir_all(output).expect("create screenshot directory");
    let mut renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some(if vulkan { "wgpu" } else { "tiny-skia" }),
    ))
    .expect("requested renderer must be available (Vulkan requires hybrid-rendering)");
    if hangul_weights {
        for (appearance, name) in [
            (AppearanceMode::Light, "light"),
            (AppearanceMode::Dark, "dark"),
        ] {
            render_hangul_comparison(
                &mut renderer,
                &styles::modern_theme(appearance).expect("explicit theme"),
                output,
                name,
            );
        }
        println!("Hangul weight comparisons: {}", output.display());
        return;
    }
    if weights {
        for (appearance, name) in [
            (AppearanceMode::Light, "light"),
            (AppearanceMode::Dark, "dark"),
        ] {
            let theme = styles::modern_theme(appearance).expect("explicit theme");
            render_weight_matrix(&mut renderer, &theme, output, name);
            render_optical_comparison(&mut renderer, &theme, output, name);
            render_coverage_comparison(&mut renderer, &theme, output, name);
        }
        println!("Weight comparisons: {}", output.display());
        return;
    }

    let mixed = [CHINESE, JAPANESE, KOREAN]
        .into_iter()
        .map(|fixture| {
            let first_paragraph = fixture
                .split("\n\n")
                .take(2)
                .collect::<Vec<_>>()
                .join("\n\n");
            format!(
                "{first_paragraph}\n天地 宇宙 日月 春夏秋冬 東西南北 山川河海 風雨雷電 骨 直 令 曜"
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let mixed_markdown = format!(
        "# Adaptive CJK\n\n[中文繁體漢字](https://example.invalid) / **日本語の漢字** / *한국어 한글*\n\n{mixed}"
    );

    for (name, fixture) in [
        ("chinese", CHINESE),
        ("japanese", JAPANESE),
        ("korean", KOREAN),
        ("mixed", mixed.as_str()),
        ("mixed-markdown", mixed_markdown.as_str()),
        ("mixed-script-lines", MIXED_SCRIPT_LINES),
        ("user-mixed", USER_MIXED),
    ] {
        verify_fonts(fixture, name);
        for (appearance, theme_name, zoom, pixels, layout_name) in [
            (
                AppearanceMode::Light,
                "light",
                1.0,
                Size::new(1200, 760),
                "normal",
            ),
            (
                AppearanceMode::Dark,
                "dark",
                1.5,
                Size::new(680, 1120),
                "narrow",
            ),
        ] {
            render_document(
                &mut renderer,
                fixture,
                &styles::modern_theme(appearance).expect("explicit theme"),
                zoom,
                pixels,
                output,
                &format!("{name}-{theme_name}-{layout_name}"),
            );
        }
    }

    let mixed_examples = USER_MIXED
        .split("\n\n")
        .find(|paragraph| paragraph.contains("examples：國国"))
        .expect("the supplied mixed fixture includes the regional example lists");
    render_document(
        &mut renderer,
        mixed_examples,
        &styles::modern_theme(AppearanceMode::Light).expect("explicit theme"),
        2.0,
        Size::new(1400, 600),
        output,
        "user-mixed-examples-light-enlarged",
    );

    verify_fonts(REGIONAL_SHAPES, "regional-shapes");
    for (appearance, theme_name) in [
        (AppearanceMode::Light, "light"),
        (AppearanceMode::Dark, "dark"),
    ] {
        render_document(
            &mut renderer,
            REGIONAL_SHAPES,
            &styles::modern_theme(appearance).expect("explicit theme"),
            3.0,
            Size::new(1400, 1000),
            output,
            &format!("regional-shapes-{theme_name}"),
        );
    }
    println!("CJK screenshots: {}", output.display());
}

fn verify_fonts(source: &str, name: &str) {
    use text::Paragraph as _;

    let buffer = EditorBuffer::from_text(source);
    let context = CjkContext::from_buffer(&buffer);
    if name == "user-mixed" {
        audit_user_mixed_routes(source, &context);
    }
    let mut families = BTreeMap::<String, usize>::new();
    let mut glyph_count = 0;
    for (line_number, line) in source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
    {
        let language = context.language_for_line(line_number);
        // Inspect the exact cached logical routes consumed by the widget. A
        // single inherited enum cannot represent mixed passages on one line.
        let language_runs = context.runs_for_line(line_number, line);
        let runs = editor_font_runs_from_cjk_runs(line, &language_runs);
        if name == "user-mixed" {
            for route in &language_runs {
                if let Some(language) = route.language {
                    let sample: String = line[route.byte_range.clone()].chars().take(40).collect();
                    println!(
                        "CJK_ROUTE document={name} line={} bytes={}..{} language={} family={:?} text={sample:?}",
                        line_number + 1,
                        route.byte_range.start,
                        route.byte_range.end,
                        language.language_tag(),
                        regional_cjk_font(language).family
                    );
                }
            }
        }
        let spans: Vec<text::Span<'_>> = runs
            .iter()
            .map(|run| text::Span::new(&line[run.byte_range.clone()]).font(run.font))
            .collect();
        let paragraph = graphics::text::Paragraph::with_spans(text::Text {
            content: spans.as_slice(),
            bounds: Size::new(100_000.0, 100.0),
            size: Pixels(16.0),
            line_height: text::LineHeight::Absolute(Pixels(20.0)),
            font: EDITOR_FONT,
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: None,
        });
        let mut font_system = graphics::text::font_system()
            .write()
            .expect("inspect shaped fonts");
        let database = font_system.raw().db();
        for run in paragraph.buffer().layout_runs() {
            for glyph in run.glyphs {
                let cluster = &line[glyph.start..glyph.end];
                assert_ne!(
                    glyph.glyph_id,
                    0,
                    "{name}, line {}: missing glyph for {cluster:?}",
                    line_number + 1
                );
                let family = database
                    .face(glyph.font_id)
                    .and_then(|face| face.families.first())
                    .map(|(family, _)| family.clone())
                    .unwrap_or_else(|| format!("{:?}", glyph.font_id));
                *families.entry(family.clone()).or_default() += 1;
                glyph_count += 1;
                if name == "regional-shapes" && cluster.chars().any(|ch| "骨直令曜".contains(ch))
                    || name == "mixed-script-lines" && cluster == "骨"
                    || name == "user-mixed"
                        && line.contains("examples：國国")
                        && cluster.chars().any(|ch| {
                            "國国學学體体龍龙門门風风雲云書书畫画愛爱夢梦廣广萬万樂乐".contains(ch)
                        })
                    || name == "user-mixed"
                        && line_number == 0
                        && ["清晨", "已经", "東京駅", "地下鐵", "驛", "會社", "學校"]
                            .iter()
                            .filter_map(|needle| {
                                line.find(needle).map(|start| start..start + needle.len())
                            })
                            .any(|range| range.start < glyph.end && glyph.start < range.end)
                    || name == "user-mixed"
                        && line_number == 0
                        && cluster
                            .chars()
                            .any(|ch| matches!(ch as u32, 0xAC00..=0xD7A3))
                {
                    let face = database.face(glyph.font_id).expect("shaped face metadata");
                    println!(
                        "CJK_GLYPH document={name} line={} byte={} line_language={language:?} cluster={cluster:?} actual_family={family:?} font_id={:?} glyph_id={} actual_weight={} style={:?} postscript={:?} file={:?}",
                        line_number + 1,
                        glyph.start,
                        glyph.font_id,
                        glyph.glyph_id,
                        face.weight.0,
                        face.style,
                        face.post_script_name,
                        font_source_path(&face.source),
                    );
                }
            }
        }
    }
    println!("CJK_FONTS name={name} glyphs={glyph_count} missing=0 actual_families={families:?}");
}

fn font_source_path(source: &graphics::text::cosmic_text::fontdb::Source) -> String {
    use graphics::text::cosmic_text::fontdb::Source;
    match source {
        Source::File(path) | Source::SharedFile(path, _) => path.display().to_string(),
        Source::Binary(_) => "<in-memory>".into(),
    }
}

const SHARED_STEMS: &str = "一二十丁骨直令曜";
const WEIGHT_REGIONS: [(CjkLanguage, &str); 4] = [
    (CjkLanguage::SimplifiedChinese, "SC"),
    (CjkLanguage::TraditionalChinese, "TC"),
    (CjkLanguage::Japanese, "JP"),
    (CjkLanguage::Korean, "KR"),
];

fn comparison_paragraph(
    source: &str,
    runs: &[EditorFontRun],
    size: f32,
    width: f32,
    wrapping: text::Wrapping,
) -> graphics::text::Paragraph {
    use text::Paragraph as _;
    let spans: Vec<text::Span<'_>> = runs
        .iter()
        .map(|run| text::Span::new(&source[run.byte_range.clone()]).font(run.font))
        .collect();
    let paragraph = graphics::text::Paragraph::with_spans(text::Text {
        content: spans.as_slice(),
        bounds: Size::new(width, 500.0),
        size: Pixels(size),
        line_height: text::LineHeight::Absolute(Pixels(size * 1.25)),
        font: EDITOR_FONT.weight(font::Weight::Normal),
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping,
        ellipsis: text::Ellipsis::None,
        hint_factor: Some(1.0),
    });
    for run in paragraph.buffer().layout_runs() {
        for glyph in run.glyphs {
            assert_ne!(
                glyph.glyph_id,
                0,
                "weight comparison is missing a glyph for {:?}",
                &source[glyph.start..glyph.end]
            );
        }
    }
    paragraph
}

fn fill_comparison(
    renderer: &mut Renderer,
    retained: &mut Vec<graphics::text::Paragraph>,
    paragraph: &graphics::text::Paragraph,
    position: Point,
    color: Color,
    viewport: Rectangle,
) {
    text::Renderer::fill_paragraph(renderer, paragraph, position, color, viewport);
    // Both renderers keep weak paragraph handles until screenshot submission.
    retained.push(paragraph.clone());
}

fn label(
    renderer: &mut Renderer,
    retained: &mut Vec<graphics::text::Paragraph>,
    content: &str,
    size: f32,
    position: Point,
    color: Color,
    viewport: Rectangle,
) {
    let runs = [EditorFontRun {
        byte_range: 0..content.len(),
        font: EDITOR_FONT.weight(font::Weight::Normal),
    }];
    let paragraph =
        comparison_paragraph(content, &runs, size, viewport.width, text::Wrapping::None);
    fill_comparison(renderer, retained, &paragraph, position, color, viewport);
}

fn report_weight_face(
    paragraph: &graphics::text::Paragraph,
    source: &str,
    label: &str,
    requested: font::Weight,
) -> String {
    let glyph = paragraph
        .buffer()
        .layout_runs()
        .flat_map(|run| run.glyphs)
        .find(|glyph| {
            source[glyph.start..glyph.end]
                .chars()
                .any(|ch| !ch.is_ascii())
        })
        .or_else(|| {
            paragraph
                .buffer()
                .layout_runs()
                .flat_map(|run| run.glyphs)
                .next()
        })
        .expect("comparison includes glyphs");
    let mut font_system = graphics::text::font_system()
        .write()
        .expect("inspect comparison face");
    let face = font_system
        .raw()
        .db()
        .face(glyph.font_id)
        .expect("comparison face metadata");
    let summary = format!("actual {} / {}", face.weight.0, face.post_script_name);
    println!(
        "CJK_WEIGHT sample={label:?} requested={requested:?} actual_weight={} style={:?} family={:?} postscript={:?} font_id={:?} glyph_id={} file={:?}",
        face.weight.0,
        face.style,
        face.families.first().map(|family| &family.0),
        face.post_script_name,
        glyph.font_id,
        glyph.glyph_id,
        font_source_path(&face.source)
    );
    summary
}

fn save_comparison(
    renderer: &mut Renderer,
    pixels: Size<u32>,
    background: Color,
    output: &Path,
    name: &str,
) {
    let bytes = renderer.screenshot(pixels, 1.0, background);
    tiny_skia::Pixmap::from_vec(
        bytes,
        tiny_skia::IntSize::from_wh(pixels.width, pixels.height).unwrap(),
    )
    .expect("comparison screenshot")
    .save_png(output.join(format!("{name}.png")))
    .expect("save comparison");
}

fn render_weight_matrix(renderer: &mut Renderer, theme: &Theme, output: &Path, theme_name: &str) {
    let mut retained = Vec::new();
    let pixels = Size::new(1840, 1000);
    let viewport = Rectangle::with_size(Size::new(pixels.width as f32, pixels.height as f32));
    renderer.reset(viewport);
    let style = EditorStyle::from_theme(theme);
    label(
        renderer,
        &mut retained,
        "Native CJK faces: requested Light300 / Normal400 / Medium500 / Semibold600",
        24.0,
        Point::new(24.0, 16.0),
        style.text,
        viewport,
    );
    let latin = "Latin always Consolas400: ABC abc 0123456789";
    let latin_runs = [EditorFontRun {
        byte_range: 0..latin.len(),
        font: EDITOR_FONT.weight(font::Weight::Normal),
    }];
    let latin_paragraph =
        comparison_paragraph(latin, &latin_runs, 24.0, 1780.0, text::Wrapping::None);
    fill_comparison(
        renderer,
        &mut retained,
        &latin_paragraph,
        Point::new(24.0, 60.0),
        style.text,
        viewport,
    );
    report_weight_face(
        &latin_paragraph,
        latin,
        "primary Latin",
        font::Weight::Normal,
    );
    for (column, (weight, weight_label)) in [
        (font::Weight::Light, "Light300"),
        (font::Weight::Normal, "Normal400"),
        (font::Weight::Medium, "Medium500"),
        (font::Weight::Semibold, "Semibold600"),
    ]
    .into_iter()
    .enumerate()
    {
        let x = 24.0 + column as f32 * 450.0;
        label(
            renderer,
            &mut retained,
            weight_label,
            20.0,
            Point::new(x, 110.0),
            style.text,
            viewport,
        );
        for (row, (language, region)) in WEIGHT_REGIONS.into_iter().enumerate() {
            let y = 152.0 + row as f32 * 206.0;
            let face_font = regional_cjk_font(language).weight(weight);
            label(
                renderer,
                &mut retained,
                &format!("{region} / {}", face_font.family),
                16.0,
                Point::new(x, y),
                style.text,
                viewport,
            );
            for (index, size) in [16.0, 24.0, 32.0].into_iter().enumerate() {
                let source = format!("Aa01 / {SHARED_STEMS}");
                let runs = [
                    EditorFontRun {
                        byte_range: 0..7,
                        font: EDITOR_FONT.weight(font::Weight::Normal),
                    },
                    EditorFontRun {
                        byte_range: 7..source.len(),
                        font: face_font,
                    },
                ];
                let paragraph =
                    comparison_paragraph(&source, &runs, size, 420.0, text::Wrapping::None);
                fill_comparison(
                    renderer,
                    &mut retained,
                    &paragraph,
                    Point::new(x, y + 27.0 + index as f32 * 46.0),
                    style.text,
                    viewport,
                );
                if index == 1 {
                    let summary = report_weight_face(
                        &paragraph,
                        &source,
                        &format!("{region} {weight_label}"),
                        weight,
                    );
                    label(
                        renderer,
                        &mut retained,
                        &summary,
                        12.0,
                        Point::new(x, y + 176.0),
                        style.line_numbers,
                        viewport,
                    );
                }
            }
        }
    }
    save_comparison(
        renderer,
        pixels,
        style.surface,
        output,
        &format!("native-weights-{theme_name}"),
    );
}

fn optical_weight(language: CjkLanguage, policy: usize) -> font::Weight {
    if policy == 1 && language == CjkLanguage::Japanese {
        font::Weight::Medium
    } else {
        font::Weight::Normal
    }
}

fn forced_regional_font_runs(
    source: &str,
    language: CjkLanguage,
    regional: Font,
) -> Vec<EditorFontRun> {
    cjk_runs(source, Some(language))
        .into_iter()
        .map(|run| EditorFontRun {
            byte_range: run.byte_range,
            font: if run.language.is_some() {
                regional
            } else {
                EDITOR_FONT
            },
        })
        .collect()
}

// The candidate uses the app's exact Korean cluster guards. The baseline
// forces the full Regular body face so it remains useful after calibration.
fn candidate_hangul_runs(source: &str, light_hangul: bool) -> Vec<EditorFontRun> {
    if light_hangul {
        return editor_font_runs_from_cjk_runs(
            source,
            &cjk_runs(source, Some(CjkLanguage::Korean)),
        );
    }
    let mut runs: Vec<EditorFontRun> = Vec::new();
    for (start, cluster) in source.grapheme_indices(true) {
        let selected = if cluster.is_ascii() {
            EDITOR_FONT.weight(font::Weight::Normal)
        } else {
            Font::new("Malgun Gothic").weight(font::Weight::Normal)
        };
        if let Some(previous) = runs.last_mut().filter(|run| run.font == selected) {
            previous.byte_range.end = start + cluster.len();
        } else {
            runs.push(EditorFontRun {
                byte_range: start..start + cluster.len(),
                font: selected,
            });
        }
    }
    runs
}

fn render_hangul_comparison(
    renderer: &mut Renderer,
    theme: &Theme,
    output: &Path,
    theme_name: &str,
) {
    use text::Paragraph as _;
    let pixels = Size::new(1600, 1500);
    let viewport = Rectangle::with_size(Size::new(pixels.width as f32, pixels.height as f32));
    let mut retained = Vec::new();
    renderer.reset(viewport);
    let style = EditorStyle::from_theme(theme);
    for (policy, title) in [
        "Korean: all Regular400",
        "Hangul300 / Hanja + historical clusters400",
    ]
    .into_iter()
    .enumerate()
    {
        let x = 24.0 + policy as f32 * 792.0;
        label(
            renderer,
            &mut retained,
            title,
            19.0,
            Point::new(x, 16.0),
            style.text,
            viewport,
        );
        label(
            renderer,
            &mut retained,
            "Actual native faces; Latin stays Consolas400",
            14.0,
            Point::new(x, 49.0),
            style.line_numbers,
            viewport,
        );
        let mut y = 89.0;
        for size in [16.0, 24.0, 32.0] {
            label(
                renderer,
                &mut retained,
                &format!("{size:.0}px"),
                16.0,
                Point::new(x, y),
                style.line_numbers,
                viewport,
            );
            y += 29.0;
            for (sample, source) in [
                ("Modern syllables", "한글 가나다라 마바사아 자차카타 파하"),
                (
                    "Hanja + punctuation",
                    "天地 宇宙 日月 骨 直 令 曜 學校 會社「」·、！？",
                ),
                ("Modern NFD + compatibility jamo", "한글 / ㄱㄴㄷㅏㅓㅗㅜ"),
                (
                    "Historical + tone-mark clusters (Regular400)",
                    "ᅚᅡ ꥠᅡ 가ퟋ ᄒᆞᆫ 한〮 한〮 ᄀ가 각ᆨ",
                ),
                (
                    "Mixed Korean paragraph",
                    "서울의 地下鐵 驛에서는 사람들이 會社와 學校로 빠르게 이동한다.",
                ),
            ] {
                label(
                    renderer,
                    &mut retained,
                    sample,
                    13.0,
                    Point::new(x, y),
                    style.line_numbers,
                    viewport,
                );
                y += 19.0;
                let runs = candidate_hangul_runs(source, policy == 1);
                let paragraph =
                    comparison_paragraph(source, &runs, size, 750.0, text::Wrapping::WordOrGlyph);
                fill_comparison(
                    renderer,
                    &mut retained,
                    &paragraph,
                    Point::new(x, y),
                    style.text,
                    viewport,
                );
                if size == 24.0 {
                    let first_cjk_byte = source
                        .char_indices()
                        .find(|(_, ch)| !ch.is_ascii())
                        .expect("comparison sample includes CJK")
                        .0;
                    let requested = runs
                        .iter()
                        .find(|run| run.byte_range.contains(&first_cjk_byte))
                        .expect("comparison fonts cover the source")
                        .font
                        .weight;
                    report_weight_face(
                        &paragraph,
                        source,
                        &format!("{title} / {sample}"),
                        requested,
                    );
                    let mut font_system = graphics::text::font_system()
                        .write()
                        .expect("inspect Hangul comparison faces");
                    let db = font_system.raw().db();
                    let actual: BTreeMap<_, _> = paragraph
                        .buffer()
                        .layout_runs()
                        .flat_map(|row| row.glyphs)
                        .filter_map(|glyph| {
                            let cluster = &source[glyph.start..glyph.end];
                            (!cluster.is_ascii()).then(|| {
                                let face = db
                                    .face(glyph.font_id)
                                    .expect("Hangul comparison shaped face");
                                (
                                    (cluster.to_owned(), face.post_script_name.clone()),
                                    face.weight.0,
                                )
                            })
                        })
                        .collect();
                    if sample.starts_with("Modern") || sample.starts_with("Historical") {
                        let expected = if policy == 1 && sample.starts_with("Modern") {
                            300
                        } else {
                            400
                        };
                        assert!(
                            actual.values().all(|weight| *weight == expected),
                            "{title}, {sample}: every shaped cluster must retain weight {expected}: {actual:?}"
                        );
                    }
                    println!(
                        "CJK_HANGUL_WEIGHT policy={policy} sample={sample:?} actual_faces={actual:?}"
                    );
                }
                y += paragraph.min_height() + 18.0;
            }
            y += 20.0;
        }
        assert!(
            y <= viewport.height,
            "Hangul weight comparison must fit: {y}"
        );
    }
    save_comparison(
        renderer,
        pixels,
        style.surface,
        output,
        &format!("hangul-comparison-{theme_name}"),
    );
}

fn render_optical_comparison(
    renderer: &mut Renderer,
    theme: &Theme,
    output: &Path,
    theme_name: &str,
) {
    let mut retained = Vec::new();
    let pixels = Size::new(1540, 1250);
    let viewport = Rectangle::with_size(Size::new(pixels.width as f32, pixels.height as f32));
    renderer.reset(viewport);
    let style = EditorStyle::from_theme(theme);
    let mixed = "清晨的城市漸漸醒來，東京駅では電車が到着し、서울의 地下鐵 驛에서는";
    let context = CjkContext::from_buffer(&EditorBuffer::from_text(mixed));
    for (policy, title) in [
        "Baseline: all400",
        "Japanese500 only",
        "Production weight profiles",
    ]
    .into_iter()
    .enumerate()
    {
        let x = 24.0 + policy as f32 * 508.0;
        label(
            renderer,
            &mut retained,
            title,
            18.0,
            Point::new(x, 20.0),
            style.text,
            viewport,
        );
        label(
            renderer,
            &mut retained,
            "Latin stays Consolas400",
            14.0,
            Point::new(x, 52.0),
            style.line_numbers,
            viewport,
        );
        let mut y = 90.0;
        for size in [16.0, 24.0, 32.0] {
            label(
                renderer,
                &mut retained,
                &format!("{size:.0}px"),
                15.0,
                Point::new(x, y),
                style.line_numbers,
                viewport,
            );
            y += 24.0;
            for (language, region) in WEIGHT_REGIONS {
                let source = format!("{region} {SHARED_STEMS}");
                let candidate = if policy == 2 {
                    regional_cjk_font(language)
                } else {
                    regional_cjk_font(language).weight(optical_weight(language, policy))
                };
                let runs = if policy == 2 {
                    editor_font_runs(&source, Some(language))
                } else {
                    forced_regional_font_runs(&source, language, candidate)
                };
                let paragraph =
                    comparison_paragraph(&source, &runs, size, 475.0, text::Wrapping::None);
                fill_comparison(
                    renderer,
                    &mut retained,
                    &paragraph,
                    Point::new(x, y),
                    style.text,
                    viewport,
                );
                if size == 24.0 {
                    report_weight_face(
                        &paragraph,
                        &source,
                        &format!("{title} {region}"),
                        candidate.weight,
                    );
                }
                y += size * 1.25 + 8.0;
            }
            y += 10.0;
            let language_runs = context.runs_for_line(0, mixed);
            let runs: Vec<_> = if policy == 2 {
                editor_font_runs_from_cjk_runs(mixed, &language_runs)
            } else {
                language_runs
                    .into_iter()
                    .map(|run| EditorFontRun {
                        byte_range: run.byte_range,
                        font: run.language.map_or(
                            EDITOR_FONT.weight(font::Weight::Normal),
                            |language| {
                                regional_cjk_font(language).weight(optical_weight(language, policy))
                            },
                        ),
                    })
                    .collect()
            };
            let paragraph =
                comparison_paragraph(mixed, &runs, size, 475.0, text::Wrapping::WordOrGlyph);
            use text::Paragraph as _;
            fill_comparison(
                renderer,
                &mut retained,
                &paragraph,
                Point::new(x, y),
                style.text,
                viewport,
            );
            y += paragraph.min_height() + 32.0;
        }
        assert!(
            y <= viewport.height,
            "the optical comparison must fit on the canvas"
        );
    }
    save_comparison(
        renderer,
        pixels,
        style.surface,
        output,
        &format!("optical-comparison-{theme_name}"),
    );
}

fn render_coverage_comparison(
    renderer: &mut Renderer,
    theme: &Theme,
    output: &Path,
    theme_name: &str,
) {
    use text::Paragraph as _;

    let mut retained = Vec::new();
    let pixels = Size::new(1540, 520);
    let viewport = Rectangle::with_size(Size::new(pixels.width as f32, pixels.height as f32));
    renderer.reset(viewport);
    let style = EditorStyle::from_theme(theme);
    let language = CjkLanguage::SimplifiedChinese;
    let selected = regional_cjk_font(language);
    let source = "汉 \u{2e80} \u{2f00} \u{322a} 骨";
    for (policy, title) in [
        "Regular body face",
        "Uniform adjusted face",
        "Production: safe coverage",
    ]
    .into_iter()
    .enumerate()
    {
        let x = 24.0 + policy as f32 * 508.0;
        label(
            renderer,
            &mut retained,
            title,
            18.0,
            Point::new(x, 20.0),
            style.text,
            viewport,
        );
        label(
            renderer,
            &mut retained,
            "Han, radicals, enclosed symbol",
            14.0,
            Point::new(x, 53.0),
            style.line_numbers,
            viewport,
        );
        let runs = match policy {
            0 => forced_regional_font_runs(source, language, selected.weight(font::Weight::Normal)),
            1 => forced_regional_font_runs(source, language, selected),
            _ => editor_font_runs(source, Some(language)),
        };
        let mut y = 97.0;
        for size in [16.0, 24.0, 32.0] {
            label(
                renderer,
                &mut retained,
                &format!("{size:.0}px"),
                15.0,
                Point::new(x, y),
                style.line_numbers,
                viewport,
            );
            y += 25.0;
            let paragraph = comparison_paragraph(source, &runs, size, 475.0, text::Wrapping::None);
            fill_comparison(
                renderer,
                &mut retained,
                &paragraph,
                Point::new(x, y),
                style.text,
                viewport,
            );
            y += paragraph.min_height() + 40.0;
            if size == 24.0 {
                let mut system = graphics::text::font_system()
                    .write()
                    .expect("inspect coverage faces");
                let db = system.raw().db();
                for glyph in paragraph.buffer().layout_runs().flat_map(|row| row.glyphs) {
                    let cluster = &source[glyph.start..glyph.end];
                    if cluster.is_ascii() {
                        continue;
                    }
                    let face = db.face(glyph.font_id).expect("coverage comparison face");
                    println!(
                        "CJK_COVERAGE policy={policy} cluster={cluster:?} actual_family={:?} actual_weight={} postscript={:?}",
                        face.families.first().map(|family| &family.0),
                        face.weight.0,
                        face.post_script_name,
                    );
                }
            }
        }
    }
    save_comparison(
        renderer,
        pixels,
        style.surface,
        output,
        &format!("coverage-comparison-{theme_name}"),
    );
}

fn audit_user_mixed_routes(source: &str, context: &CjkContext) {
    use CjkLanguage::{Japanese, Korean, SimplifiedChinese, TraditionalChinese};

    for (needle, expected) in [
        ("漸", TraditionalChinese),
        ("经", SimplifiedChinese),
        ("發", TraditionalChinese),
        ("東京駅", Japanese),
        ("地下鐵", Korean),
        ("會社", Korean),
        ("學校", Korean),
        ("電車", Japanese),
        ("韓國", Korean),
        ("文化・藝術・科學技術", Korean),
        ("博物館入口", Japanese),
        ("圖書館", Japanese),
        ("出口", Japanese),
        ("世界は広い", Japanese),
        ("세계는 넓다", Korean),
        ("再見", TraditionalChinese),
        ("再见", SimplifiedChinese),
        ("また会いましょう", Japanese),
        ("다음에 또 만나요", Korean),
    ] {
        let mut found = 0;
        for (line_number, line) in source.lines().enumerate() {
            let runs = context.runs_for_line(line_number, line);
            for (start, _) in line.match_indices(needle) {
                for (relative_byte, character) in needle.char_indices() {
                    if character == '・' || character.is_ascii() {
                        continue;
                    }
                    let byte = start + relative_byte;
                    let language = runs
                        .iter()
                        .find(|run| run.byte_range.contains(&byte))
                        .and_then(|run| run.language);
                    assert_eq!(
                        language,
                        Some(expected),
                        "user-mixed line {}, {needle:?}, character {character}",
                        line_number + 1
                    );
                }
                found += 1;
                println!(
                    "CJK_PROBE line={} text={needle:?} expected={} family={:?} pass=true",
                    line_number + 1,
                    expected.language_tag(),
                    regional_cjk_font(expected).family
                );
            }
        }
        assert!(
            found > 0,
            "the exact mixed fixture contains probe {needle:?}"
        );
    }

    let chinese_world = "世界很大";
    let (world_line_number, world_line) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(chinese_world))
        .expect("Chinese clause in the quoted three-language comparison");
    let world_start = world_line.find(chinese_world).unwrap();
    let world_runs = context.runs_for_line(world_line_number, world_line);
    for (relative_byte, character) in chinese_world.char_indices() {
        let byte = world_start + relative_byte;
        let language = world_runs
            .iter()
            .find(|run| run.byte_range.contains(&byte))
            .and_then(|run| run.language);
        assert!(
            matches!(language, Some(SimplifiedChinese | TraditionalChinese)),
            "Chinese clause character {character} has route {language:?}"
        );
    }
    println!(
        "CJK_PROBE line={} text={chinese_world:?} expected=zh-Hant/zh-Hans pass=true",
        world_line_number + 1
    );

    let (line_number, line) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("examples：國国"))
        .expect("Chinese variant comparison list");
    let runs = context.runs_for_line(line_number, line);
    for pair in [
        "國国", "學学", "體体", "龍龙", "門门", "風风", "雲云", "書书", "畫画", "愛爱", "夢梦",
        "廣广", "萬万", "樂乐",
    ] {
        let start = line.find(pair).expect("supplied variant pair");
        for ((relative_byte, character), expected) in pair
            .char_indices()
            .zip([TraditionalChinese, SimplifiedChinese])
        {
            let byte = start + relative_byte;
            let language = runs
                .iter()
                .find(|run| run.byte_range.contains(&byte))
                .and_then(|run| run.language);
            assert_eq!(
                language,
                Some(expected),
                "variant {character} in pair {pair}"
            );
        }
    }
    println!(
        "CJK_PROBE line={} variant_pairs=14 expected=zh-Hant/zh-Hans pass=true",
        line_number + 1
    );
}

fn render_document(
    renderer: &mut Renderer,
    source: &str,
    theme: &Theme,
    zoom: f32,
    mut pixels: Size<u32>,
    output: &Path,
    name: &str,
) {
    let path = if name.starts_with("mixed-markdown") {
        "cjk-review.md"
    } else {
        "cjk-review.txt"
    };
    let mut document = Document::from_path(DocumentId::new(1), path, source);
    let mut settings = EditorSettings {
        zoom,
        word_wrap: true,
        ..EditorSettings::default()
    };
    settings.set_appearance(if theme.palette().is_dark {
        AppearanceMode::Dark
    } else {
        AppearanceMode::Light
    });
    document.ensure_syntax_cache(settings.syntax_theme);
    document.set_word_wrap(settings.word_wrap);
    document.set_wrap_column_limit(settings.wrap_column_limit);
    let mut tree = Tree::empty();
    let start = Instant::now();

    // Use the widget's own ViewportChanged measurements and the same document
    // update as the app. Rebuild after applying them so screenshot wrapping is
    // based on the measured viewport, rather than provisional document defaults.
    for pass in 0..3 {
        let size = Size::new(pixels.width as f32, pixels.height as f32);
        let viewport = Rectangle::with_size(size);
        let mut messages = Vec::new();
        {
            let mut content = editor::view(&document, &settings);
            tree.diff(content.as_widget_mut());
            let node = content.as_widget_mut().layout(
                &mut tree,
                renderer,
                &layout::Limits::new(size, size),
            );
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            content.as_widget_mut().update(
                &mut tree,
                &Event::Window(window::Event::RedrawRequested(
                    start + Duration::from_millis(200 * pass),
                )),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                renderer,
                &mut shell,
                &viewport,
            );
            if pass == 2 {
                renderer.reset(viewport);
                content.as_widget().draw(
                    &tree,
                    renderer,
                    theme,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &viewport,
                );
            }
        }
        for message in messages {
            if let Message::EditorAction(
                _,
                EditorAction::ViewportChanged {
                    visible_rows,
                    text_width,
                    character_width_milli,
                    font_size_milli,
                    hint_factor_milli,
                },
            ) = message
            {
                document.update_viewport_geometry_with_typography(
                    visible_rows,
                    text_width as f32,
                    character_width_milli as f32 / 1000.0,
                    font_size_milli as f32 / 1000.0,
                    hint_factor_milli.map(|scale| scale as f32 / 1000.0),
                );
                document.set_word_wrap(settings.word_wrap);
            }
        }
        if pass == 0 && name.starts_with("user-mixed") {
            // Grow the canvas from the actual measured editor wrap layout, so
            // the exact fixture and its trailing-space rows are all visible.
            let required_height =
                (document.viewport.visible_row_count() as f32 * 20.0 * zoom + 6.0).ceil() as u32;
            pixels.height = pixels.height.max(required_height);
        }
    }
    assert!(
        document.viewport.visible_row_count() <= document.viewport_visible_rows,
        "{name}: the screenshot must show the full document ({} rows, {} visible)",
        document.viewport.visible_row_count(),
        document.viewport_visible_rows,
    );
    let bytes = renderer.screenshot(pixels, 1.0, EditorStyle::from_theme(theme).surface);
    tiny_skia::Pixmap::from_vec(
        bytes,
        tiny_skia::IntSize::from_wh(pixels.width, pixels.height).expect("valid dimensions"),
    )
    .expect("RGBA screenshot")
    .save_png(output.join(format!("{name}.png")))
    .expect("save screenshot");
    println!(
        "CJK_PREVIEW name={name} zoom={zoom:.1} dimensions={}x{} logical_lines={} wrapped_rows={} wrap_columns={}",
        pixels.width,
        pixels.height,
        document.buffer.line_count(),
        document.viewport.visible_row_count(),
        document.viewport.wrap_columns().expect("wrapped preview"),
    );
}
