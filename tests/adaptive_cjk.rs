use fragile_notepad::core::{Document, DocumentId, EditorSettings, IndentationMode};
use fragile_notepad::editor::cjk::CjkLanguage;
use fragile_notepad::editor::layout::visual_width_with_tab_width;
use fragile_notepad::editor::widget::EditorStyle;
use fragile_notepad::editor::widget::{
    EDITOR_FONT, EditorFontRun, editor_font_runs, editor_font_runs_from_cjk_runs, regional_cjk_font,
};
use fragile_notepad::editor::{
    EditorAction, EditorMetrics, EditorPosition, ViewportModel, text_baseline_offset,
};
use fragile_notepad::message::Message;
use fragile_notepad::ui::editor;
use iced::advanced::graphics;
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::text::Renderer as _;
use iced::advanced::text::{self, Paragraph as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Event, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, font};
use unicode_segmentation::UnicodeSegmentation;

const MIXED: &str = include_str!("fixtures/cjk/mixed.txt");

fn font_run_paragraph(
    source: &str,
    runs: &[EditorFontRun],
    size: f32,
    line_height: f32,
) -> graphics::text::Paragraph {
    let spans: Vec<text::Span<'_, (), Font>> = runs
        .iter()
        .map(|run| text::Span::new(&source[run.byte_range.clone()]).font(run.font))
        .collect();
    graphics::text::Paragraph::with_spans(text::Text {
        content: &spans,
        bounds: Size::new(f32::INFINITY, line_height),
        size: Pixels(size),
        line_height: text::LineHeight::Absolute(Pixels(line_height)),
        font: EDITOR_FONT,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
        ellipsis: text::Ellipsis::None,
        hint_factor: None,
    })
}

fn contextual_paragraph(
    document: &Document,
    line: usize,
    start_byte: usize,
    fragment: &str,
    zoom: f32,
) -> graphics::text::Paragraph {
    let runs = document
        .cjk_context()
        .runs_for_fragment(line, start_byte, fragment);
    let font_runs = editor_font_runs_from_cjk_runs(fragment, &runs);
    font_run_paragraph(fragment, &font_runs, 16.0 * zoom, 20.0 * zoom)
}

fn is_cjk_letter(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF)
        && ch.is_alphabetic()
}

fn assert_contextual_anchor(
    document: &Document,
    line_marker: &str,
    anchor: &str,
    languages: &[CjkLanguage],
) {
    let line_number = MIXED
        .lines()
        .position(|line| line.contains(line_marker))
        .expect("Fixture line marker");
    let line = document.buffer.line(line_number).unwrap();
    let start = line.find(anchor).expect("Fixture language anchor");
    let end = start + anchor.len();
    let routes = document.cjk_context().runs_for_line(line_number, &line);
    let paragraph = contextual_paragraph(document, line_number, 0, &line, 1.5);
    let regional_fonts: Vec<_> = languages
        .iter()
        .map(|language| (*language, regional_cjk_font(*language)))
        .collect();
    let mut system = graphics::text::font_system()
        .write()
        .expect("Inspect contextual fixture fonts");
    let db = system.raw().db();
    let mut checked = 0;
    for layout in paragraph.buffer().layout_runs() {
        for glyph in layout.glyphs {
            if glyph.end <= start
                || glyph.start >= end
                || !line[glyph.start..glyph.end].chars().any(is_cjk_letter)
            {
                continue;
            }
            let route = routes
                .iter()
                .find(|route| route.byte_range.contains(&glyph.start))
                .expect("Complete context route");
            let language = route.language.expect("CJK anchor has a language");
            assert!(
                languages.contains(&language),
                "{anchor:?} cluster {:?} should use {languages:?}, actual {language:?}",
                &line[glyph.start..glyph.end]
            );
            let regional = regional_fonts
                .iter()
                .find(|(candidate, _)| *candidate == language)
                .expect("Expected language font")
                .1;
            if regional != EDITOR_FONT {
                let font::Family::Name(family) = regional.family else {
                    panic!("Named regional font")
                };
                let face = db.face(glyph.font_id).unwrap();
                assert_ne!(glyph.glyph_id, 0, "Missing glyph in {anchor:?}");
                assert!(
                    face.families.iter().any(|(actual, _)| actual == family),
                    "{anchor:?} cluster {:?} uses {family}; actual {:?}",
                    &line[glyph.start..glyph.end],
                    face.families
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "Anchor must inspect actual shaped CJK glyphs");
}

#[test]
fn mixed_fixture_shapes_chinese_japanese_korean_and_variant_pairs_in_context() {
    use CjkLanguage::{
        Japanese as Jp, Korean as Kr, SimplifiedChinese as Sc, TraditionalChinese as Tc,
    };
    let document = Document::from_path(DocumentId::new(30), "mixed.txt", MIXED);
    assert_contextual_anchor(
        &document,
        "清晨",
        "清晨的城市漸漸醒來，陽光穿過高樓之間，街道上已经有很多人出發",
        &[Sc, Tc],
    );
    for anchor in ["東京駅", "電車", "学生", "学校"] {
        assert_contextual_anchor(&document, "清晨", anchor, &[Jp]);
    }
    for anchor in ["地下鐵", "驛", "會社", "學校"] {
        assert_contextual_anchor(&document, "清晨", anchor, &[Kr]);
    }
    assert_contextual_anchor(&document, "街角", "歡迎光臨", &[Tc]);
    assert_contextual_anchor(&document, "街角", "旁边还有一块招牌", &[Sc]);
    assert_contextual_anchor(&document, "街角", "文化・藝術・科學技術", &[Kr]);
    for anchor in ["博物館入口", "東門", "圖書館", "西門", "出口"] {
        assert_contextual_anchor(&document, "今天", anchor, &[Jp]);
    }
    for anchor in ["電車", "駅", "図書館", "東京", "京都", "世界"] {
        assert_contextual_anchor(&document, "日本語 examples", anchor, &[Jp]);
    }
    for anchor in [
        "韓國語",
        "韓國",
        "文化",
        "歷史",
        "學校",
        "時間",
        "世界",
        "未來",
        "希望",
        "和平",
        "愛情",
    ] {
        assert_contextual_anchor(&document, "Hanja examples", anchor, &[Kr]);
    }
    for (traditional, simplified) in [
        ("漢字", "汉字"),
        ("國", "国"),
        ("學", "学"),
        ("體", "体"),
        ("龍", "龙"),
        ("門", "门"),
        ("風", "风"),
        ("雲", "云"),
        ("書", "书"),
        ("畫", "画"),
        ("愛", "爱"),
        ("夢", "梦"),
        ("廣", "广"),
        ("萬", "万"),
        ("樂", "乐"),
    ] {
        assert_contextual_anchor(&document, "漢字／汉字 examples", traditional, &[Tc]);
        assert_contextual_anchor(&document, "漢字／汉字 examples", simplified, &[Sc]);
    }
    for (anchor, language) in [("測試", Tc), ("测试", Sc), ("テスト", Jp), ("시험", Kr)] {
        assert_contextual_anchor(&document, "測試／测试／テスト／시험", anchor, &[language]);
    }
    for (anchor, language) in [
        ("再見", Tc),
        ("再见", Sc),
        ("また会いましょう", Jp),
        ("다음에 또 만나요", Kr),
    ] {
        assert_contextual_anchor(&document, "最後", anchor, &[language]);
    }
    for anchor in ["大家在門口互相說", "然後各自走向不同的城市與新的旅程"] {
        assert_contextual_anchor(&document, "最後", anchor, &[Sc, Tc]);
    }
    assert_contextual_anchor(&document, "世界は広い", "世界は広い", &[Jp]);
    assert_contextual_anchor(&document, "世界は広い", "世界很大", &[Sc, Tc]);
    assert_contextual_anchor(&document, "世界は広い", "세계는 넓다", &[Kr]);

    let all_installed = [Sc, Tc, Jp, Kr]
        .into_iter()
        .all(|language| regional_cjk_font(language) != EDITOR_FONT);
    if all_installed {
        for (line_number, line) in MIXED
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.is_empty())
        {
            let paragraph = contextual_paragraph(&document, line_number, 0, line, 1.5);
            for layout in paragraph.buffer().layout_runs() {
                for glyph in layout.glyphs {
                    assert_ne!(
                        glyph.glyph_id,
                        0,
                        "Mixed fixture line {} missing glyph for {:?}",
                        line_number + 1,
                        &line[glyph.start..glyph.end]
                    );
                }
            }
        }
    }
}

fn shaped_fixture_uses_regional_glyphs(fixture: &str, language: CjkLanguage) {
    let regional = regional_cjk_font(language);
    if regional == EDITOR_FONT {
        // Font packages differ between developer machines and headless CI.
        // Preference and unavailable-font behavior have platform-independent
        // unit coverage; this test requires an actual installed CJK family.
        eprintln!("No installed regional font for {language:?}; skipping glyph inspection");
        return;
    }
    let mut regional_han = 0;
    let mut primary_ascii = 0;

    for line in fixture
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with("###"))
    {
        let runs = editor_font_runs(line, Some(language));
        let paragraph = font_run_paragraph(line, &runs, 24.0, 40.0);

        let mut system = graphics::text::font_system()
            .write()
            .expect("Inspect shaped glyph font families");
        let db = system.raw().db();
        for layout in paragraph.buffer().layout_runs() {
            for glyph in layout.glyphs {
                let cluster = &line[glyph.start..glyph.end];
                assert_ne!(
                    glyph.glyph_id, 0,
                    "Missing glyph for {cluster:?} in {language:?} fixture"
                );
                let face = db.face(glyph.font_id).expect("Glyph font exists");
                let family_matches =
                    |name: &str| face.families.iter().any(|(family, _)| family == name);

                if cluster
                    .chars()
                    .any(|ch| matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF))
                {
                    // A fixture's heading or local variant can select another
                    // Chinese region on that line. Verify the font actually
                    // routed for this cluster, including weight fallbacks.
                    let routed = runs[glyph.metadata].font;
                    if routed != EDITOR_FONT {
                        let font::Family::Name(family) = routed.family else {
                            panic!("Regional routes must name an installed family");
                        };
                        assert!(
                            family_matches(family),
                            "Han cluster {cluster:?} should use {family}; actual families: {:?}",
                            face.families
                        );
                    }
                    regional_han += 1;
                }

                if cluster.chars().any(|ch| ch.is_ascii_alphanumeric()) {
                    assert_eq!(runs[glyph.metadata].font, EDITOR_FONT);
                    if let font::Family::Name(primary_family) = EDITOR_FONT.family {
                        assert!(
                            family_matches(primary_family),
                            "ASCII cluster {cluster:?} should retain {primary_family}"
                        );
                    }
                    primary_ascii += 1;
                }
            }
        }
    }
    assert!(regional_han > 0, "Fixture must exercise shared Han glyphs");
    assert!(primary_ascii > 0, "Fixture must exercise Latin monospace");
}

#[test]
fn regional_fixtures_shape_han_and_keep_primary_ascii() {
    for (fixture, language) in [
        (
            include_str!("fixtures/cjk/chinese.txt"),
            CjkLanguage::TraditionalChinese,
        ),
        (
            include_str!("fixtures/cjk/japanese.txt"),
            CjkLanguage::Japanese,
        ),
        (include_str!("fixtures/cjk/korean.txt"), CjkLanguage::Korean),
    ] {
        shaped_fixture_uses_regional_glyphs(fixture, language);
    }
}

#[cfg(target_os = "windows")]
#[test]
fn native_weight_profile_uses_real_faces_and_keeps_latin_regular() {
    use graphics::text::cosmic_text::fontdb;

    for (language, family) in [
        (CjkLanguage::SimplifiedChinese, "Microsoft YaHei"),
        (CjkLanguage::TraditionalChinese, "Microsoft JhengHei"),
        (CjkLanguage::Japanese, "Yu Gothic"),
        (CjkLanguage::Korean, "Malgun Gothic"),
    ] {
        let selected = regional_cjk_font(language);
        if selected.family != font::Family::Name(family) {
            // A complete installed regional collection takes precedence over
            // the native profile; its selection is covered by unit tests.
            continue;
        }
        // Identical, region-neutral Han exercises the actual shaped face,
        // including a Regular route when calibration has insufficient evidence.
        // Face availability alone must not mandate an optical adjustment.
        let source = "A骨直令曜Z";
        let runs = editor_font_runs(source, Some(language));
        let paragraph = routed_paragraph(source, language, 1.5);
        let mut system = graphics::text::font_system().write().unwrap();
        let db = system.raw().db();
        let mut checked = 0;
        for layout in paragraph.buffer().layout_runs() {
            for glyph in layout.glyphs {
                let cluster = &source[glyph.start..glyph.end];
                let face = db.face(glyph.font_id).unwrap();
                assert_ne!(glyph.glyph_id, 0);
                if cluster.chars().any(is_cjk_letter) {
                    let route = runs
                        .iter()
                        .find(|run| run.byte_range.contains(&glyph.start))
                        .expect("Complete font route for shaped Han");
                    assert_eq!(route.font.family, font::Family::Name(family));
                    let requested_weight = match route.font.weight {
                        font::Weight::Thin => 100,
                        font::Weight::ExtraLight => 200,
                        font::Weight::Light => 300,
                        font::Weight::Normal => 400,
                        font::Weight::Medium => 500,
                        font::Weight::Semibold => 600,
                        font::Weight::Bold => 700,
                        font::Weight::ExtraBold => 800,
                        font::Weight::Black => 900,
                    };
                    let expected_face = db
                        .query(&fontdb::Query {
                            families: &[fontdb::Family::Name(family)],
                            weight: fontdb::Weight(requested_weight),
                            stretch: fontdb::Stretch::Normal,
                            style: fontdb::Style::Normal,
                        })
                        .expect("Selected native face exists");
                    assert_eq!(glyph.font_id, expected_face, "{language:?} {cluster:?}");
                    assert!(face.families.iter().any(|(name, _)| name == family));
                    checked += 1;
                } else if cluster.chars().any(|ch| ch.is_ascii_alphanumeric()) {
                    assert_eq!(runs[glyph.metadata].font, EDITOR_FONT);
                    assert_eq!(face.weight.0, 400, "Latin remains regular");
                }
            }
        }
        assert_eq!(checked, 4);
    }
}

#[cfg(target_os = "windows")]
fn native_yahei_light_available() -> bool {
    use graphics::text::cosmic_text::fontdb;

    let selected = regional_cjk_font(CjkLanguage::SimplifiedChinese);
    if selected.family != font::Family::Name("Microsoft YaHei")
        || selected.weight != font::Weight::Light
    {
        eprintln!("Native YaHei Light profile required for coverage inspection");
        return false;
    }
    let mut system = graphics::text::font_system().write().unwrap();
    let db = system.raw().db();
    db.query(&fontdb::Query {
        families: &[fontdb::Family::Name("Microsoft YaHei")],
        weight: fontdb::Weight(300),
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    })
    .and_then(|id| db.face(id))
    .is_some_and(|face| face.weight.0 == 290)
}

#[cfg(target_os = "windows")]
#[test]
fn native_chinese_weight_fallback_keeps_radicals_and_enclosed_symbols_in_yahei() {
    use graphics::text::cosmic_text::{fontdb, skrifa};
    use skrifa::MetadataProvider;

    if !native_yahei_light_available() {
        return;
    }
    // These radicals and enclosed symbols are in YaHei Regular, but some
    // released Light faces omit them. The old availability-only calibration
    // sent them to DengXian or Yu Gothic even though their own family has them.
    let source = "A汉\u{2e80}\u{2f00}\u{322a}骨Z";
    let expected: Vec<_> = {
        let mut system = graphics::text::font_system().write().unwrap();
        let db = system.raw().db();
        let light = db
            .query(&fontdb::Query {
                families: &[fontdb::Family::Name("Microsoft YaHei")],
                weight: fontdb::Weight(300),
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            })
            .unwrap();
        db.with_face_data(light, |data, index| {
            let face = skrifa::FontRef::from_index(data, index).unwrap();
            source
                .char_indices()
                .filter(|(_, ch)| !ch.is_ascii())
                .map(|(byte, ch)| {
                    let covered = face
                        .charmap()
                        .map(ch as u32)
                        .is_some_and(|glyph| glyph.to_u32() != 0);
                    (byte, if covered { 290 } else { 400 })
                })
                .collect()
        })
        .unwrap()
    };
    let paragraph = routed_paragraph(source, CjkLanguage::SimplifiedChinese, 1.5);
    let mut system = graphics::text::font_system().write().unwrap();
    let db = system.raw().db();
    let mut checked = vec![false; expected.len()];
    for layout in paragraph.buffer().layout_runs() {
        for glyph in layout.glyphs {
            let Some((index, (_, expected_weight))) = expected
                .iter()
                .enumerate()
                .find(|(_, (byte, _))| *byte == glyph.start)
            else {
                continue;
            };
            let cluster = &source[glyph.start..glyph.end];
            let face = db.face(glyph.font_id).unwrap();
            assert_ne!(glyph.glyph_id, 0, "Covered native symbol {cluster:?}");
            assert!(
                face.families
                    .iter()
                    .any(|(name, _)| name == "Microsoft YaHei"),
                "Coverage fallback for {cluster:?} must retain YaHei; actual {:?}",
                face.families
            );
            assert_eq!(face.weight.0, *expected_weight, "Cluster {cluster:?}");
            checked[index] = true;
        }
    }
    assert!(checked.into_iter().all(|checked| checked));
}

#[cfg(target_os = "windows")]
#[test]
fn native_chinese_weight_fallback_keeps_complete_mark_and_variation_clusters() {
    if !native_yahei_light_available() {
        return;
    }
    // An attached mark or unsupported variation sequence must move the base
    // with it to Regular. Choosing Light just for the Han base would split
    // the shaping cluster and change fallback behavior at the mark.
    for cluster in ["骨\u{0301}", "骨\u{fe0f}", "骨\u{e0100}"] {
        let source = format!("汉{cluster}汉");
        let runs = editor_font_runs(&source, Some(CjkLanguage::SimplifiedChinese));
        let start = "汉".len();
        let end = start + cluster.len();
        assert!(
            runs.iter().any(|run| {
                run.byte_range.start == start
                    && run.byte_range.end == end
                    && run.font == Font::new("Microsoft YaHei")
            }),
            "Keep complete unsupported cluster {cluster:?} in Regular: {runs:?}"
        );
        assert_eq!(runs.first().unwrap().font.weight, font::Weight::Light);
        assert_eq!(runs.last().unwrap().font.weight, font::Weight::Light);
        assert!(runs.iter().all(|run| {
            source
                .grapheme_indices(true)
                .any(|(byte, _)| byte == run.byte_range.start)
                && (run.byte_range.end == source.len()
                    || source
                        .grapheme_indices(true)
                        .any(|(byte, _)| byte == run.byte_range.end))
        }));
    }
}

#[cfg(target_os = "windows")]
#[test]
fn native_hangul_profile_preserves_hanja_and_historical_clusters() {
    if regional_cjk_font(CjkLanguage::Korean).family != font::Family::Name("Malgun Gothic") {
        return;
    }
    // Automatic calibration may safely retain Regular even when Semilight is
    // installed. Historical clusters and Hanja must stay Regular either way.
    let modern = editor_font_runs("한", Some(CjkLanguage::Korean));
    let modern_weight = if modern[0].font.weight == font::Weight::Light {
        300
    } else {
        400
    };

    // Semilight has modern syllables/jamo, but no Hanja, tone marks, or
    // Extended-B jamo. Covered archaic jamo also need Regular's composition
    // features. Keep each complete cluster rather than mix weights or fallback.
    let cases = [
        ("한글", modern_weight),
        ("한", modern_weight),
        ("ㄱㄴㅏ", modern_weight),
        ("骨直令「世界」", 400),
        ("가\u{302E}", 400),
        ("\u{1112}\u{119E}\u{11AB}", 400),
        ("\u{1100}\u{1100}\u{1161}", 400),
        ("각\u{11A8}", 400),
        ("가\u{D7CB}", 400),
        ("\u{115A}\u{1161}", 400),
        ("\u{115F}\u{1160}\u{D7B0}", 400),
    ];
    let source = format!("A{}Z", cases.map(|(text, _)| text).concat());
    let paragraph = routed_paragraph(&source, CjkLanguage::Korean, 1.5);
    let runs = editor_font_runs(&source, Some(CjkLanguage::Korean));
    let mut expected = Vec::new();
    let mut start = 1;
    for (sample, weight) in cases {
        let end = start + sample.len();
        expected.push((start..end, weight));
        for (_, grapheme) in sample.grapheme_indices(true) {
            assert!(
                runs.iter().any(|run| {
                    run.byte_range.start <= start
                        && run.byte_range.end >= start + grapheme.len()
                        && run.font.weight
                            == if weight == 300 {
                                font::Weight::Light
                            } else {
                                font::Weight::Normal
                            }
                }),
                "Keep the complete {grapheme:?} cluster at weight {weight}"
            );
            start += grapheme.len();
        }
        assert_eq!(start, end);
    }

    let mut system = graphics::text::font_system().write().unwrap();
    let db = system.raw().db();
    let mut checked = vec![0; cases.len()];
    let mut ascii = 0;
    for layout in paragraph.buffer().layout_runs() {
        for glyph in layout.glyphs {
            assert_ne!(glyph.glyph_id, 0, "Complete native Korean glyph coverage");
            let face = db.face(glyph.font_id).unwrap();
            if let Some((index, (_, weight))) = expected
                .iter()
                .enumerate()
                .find(|(_, (range, _))| range.contains(&glyph.start))
            {
                assert_eq!(
                    face.weight.0,
                    *weight,
                    "{:?}",
                    &source[glyph.start..glyph.end]
                );
                assert!(
                    face.families
                        .iter()
                        .any(|(name, _)| name == "Malgun Gothic"),
                    "Korean Hanja and Hangul retain Korean shapes: {:?}",
                    face.families
                );
                checked[index] += 1;
            } else {
                assert_eq!(face.weight.0, 400, "Latin remains Regular");
                assert_eq!(runs[glyph.metadata].font, EDITOR_FONT);
                ascii += 1;
            }
        }
    }
    assert!(checked.into_iter().all(|count| count > 0));
    assert_eq!(ascii, 2);
}

fn software_renderer() -> Renderer {
    futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("Headless software renderer")
}

fn editor_metrics(document: &Document, settings: &EditorSettings) -> EditorMetrics {
    EditorMetrics {
        line_height: 16.0 * settings.zoom * 1.25,
        character_width: 16.0 * settings.zoom * 0.55,
        ..EditorMetrics::default()
    }
    .with_line_count(document.buffer.line_count())
}

fn routed_paragraph(text: &str, language: CjkLanguage, zoom: f32) -> graphics::text::Paragraph {
    let runs = editor_font_runs(text, Some(language));
    font_run_paragraph(text, &runs, 16.0 * zoom, 20.0 * zoom)
}

fn widget_click(document: &Document, settings: &EditorSettings, point: Point) -> Vec<EditorAction> {
    let mut renderer = software_renderer();
    let size = Size::new(680.0, 220.0);
    let bounds = Rectangle::with_size(size);
    let mut content = editor::view(document, settings);
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());
    let node =
        content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
    renderer.reset(bounds);
    content.as_widget().draw(
        &tree,
        &mut renderer,
        &Theme::Light,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &bounds,
    );
    let mut messages = Vec::new();
    let mut shell = Shell::new(&iced::window::Headless, Waker::noop(), &mut messages);
    content.as_widget_mut().update(
        &mut tree,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Layout::new(&node),
        mouse::Cursor::Available(point),
        &renderer,
        &mut shell,
        &bounds,
    );
    assert!(
        shell.is_event_captured(),
        "Editor should capture text click"
    );
    messages
        .into_iter()
        .filter_map(|message| match message {
            Message::EditorAction(_, action) => Some(action),
            _ => None,
        })
        .collect()
}

fn assert_fragment_hits(
    document: &Document,
    settings: &EditorSettings,
    row: usize,
    language: CjkLanguage,
) {
    let line = document.viewport.visible_row_to_document_line(row).unwrap();
    let segment = document
        .viewport
        .row_segment(row, &document.buffer)
        .unwrap();
    let source = document.buffer.line(line).unwrap();
    let fragment = &source[segment.start_column..segment.end_column];
    let mut expanded = String::new();
    let mut boundaries = Vec::new();
    let mut visual_column = segment.start_visual_column;
    let mut expanded_grapheme = 0;
    for (byte, grapheme) in fragment.grapheme_indices(true) {
        boundaries.push((byte, expanded_grapheme));
        if grapheme == "\t" {
            let spaces = visual_width_with_tab_width(
                '\t',
                visual_column,
                document.decorations.settings.indent_width,
            );
            expanded.extend(std::iter::repeat_n(' ', spaces));
            visual_column += spaces;
            expanded_grapheme += spaces;
        } else {
            expanded.push_str(grapheme);
            for ch in grapheme.chars() {
                visual_column += visual_width_with_tab_width(
                    ch,
                    visual_column,
                    document.decorations.settings.indent_width,
                );
            }
            expanded_grapheme += 1;
        }
    }
    let paragraph = routed_paragraph(&expanded, language, settings.zoom);
    let metrics = editor_metrics(document, settings);
    let scroll = if document.viewport.wrap_columns().is_some() {
        0.0
    } else {
        document.scroll.horizontal_px
    };
    let text_origin = metrics.text_origin_x(&document.decorations);
    let mut clicks = 0;
    for (byte, grapheme) in boundaries {
        let x = paragraph.grapheme_position(0, grapheme).unwrap().x;
        if x < scroll {
            continue;
        }
        let point = Point::new(
            text_origin + x - scroll + 0.2,
            metrics.padding_top
                + (row - document.scroll.first_visible_row) as f32 * metrics.line_height
                + metrics.line_height * 0.5,
        );
        let position = EditorPosition::new(line, segment.start_column + byte);
        let actions = widget_click(document, settings, point);
        let expected = if document.viewport.wrap_columns().is_some() {
            EditorAction::PlaceCaretOnRow { position, row }
        } else {
            EditorAction::PlaceCaret(position)
        };
        assert!(
            actions.contains(&expected),
            "At shaped boundary {byte} ({point:?}), expected {expected:?}; actions: {actions:?}"
        );
        clicks += 1;
    }
    assert!(
        clicks >= 3,
        "Exercise several Han and punctuation boundaries"
    );
}

#[test]
fn ui_editor_hits_tabbed_cjk_with_custom_stops_on_wrapped_continuation() {
    let settings = EditorSettings {
        zoom: 2.0,
        word_wrap: true,
        indentation: IndentationMode::Spaces(8),
        ..EditorSettings::default()
    };
    let mut document = Document::from_path(
        DocumentId::new(23),
        "tabbed-cjk.txt",
        "prefixxxx\t骨直令x\t「日月」かな",
    );
    document.decorations.settings.indent_width = 8;
    document.set_word_wrap(true);
    document.viewport = ViewportModel::new_wrapped(&document.buffer, &document.folds, 12, 8);
    let segment = document.viewport.row_segment(1, &document.buffer).unwrap();
    let source = document.buffer.line(0).unwrap();
    let fragment = &source[segment.start_column..segment.end_column];
    assert_eq!(segment.start_visual_column, 9);
    assert!(fragment.contains('\t') && fragment.contains('骨'));
    assert_fragment_hits(&document, &settings, 1, CjkLanguage::Japanese);
}

#[test]
fn ui_editor_hits_mixed_hangul_and_hanja_weights_with_tabs_and_wrapping() {
    for wrapped in [false, true] {
        let settings = EditorSettings {
            zoom: 1.5,
            word_wrap: wrapped,
            indentation: IndentationMode::Spaces(8),
            ..EditorSettings::default()
        };
        let source = "한글\t骨直令「世界」\t한글";
        let mut document = Document::from_path(DocumentId::new(32), "hangul.txt", source);
        document.decorations.settings.indent_width = 8;
        document.set_word_wrap(wrapped);
        if wrapped {
            document.viewport =
                ViewportModel::new_wrapped(&document.buffer, &document.folds, 10, 8);
            let segment = document.viewport.row_segment(0, &document.buffer).unwrap();
            let fragment = &source[segment.start_column..segment.end_column];
            assert!(fragment.contains("한글") && fragment.contains('骨'));
        } else {
            document.scroll.horizontal_px = 35.0;
        }
        assert_fragment_hits(&document, &settings, 0, CjkLanguage::Korean);
    }
}

#[test]
fn ui_editor_hits_chinese_weight_fallback_with_tabs_and_wrapping() {
    for wrapped in [false, true] {
        let settings = EditorSettings {
            zoom: 1.5,
            word_wrap: wrapped,
            indentation: IndentationMode::Spaces(8),
            ..EditorSettings::default()
        };
        let source = "汉\t\u{2e80}\u{2f00}\u{322a}骨\t汉字";
        let mut document = Document::from_path(DocumentId::new(33), "chinese.txt", source);
        document.decorations.settings.indent_width = 8;
        document.set_word_wrap(wrapped);
        if wrapped {
            document.viewport =
                ViewportModel::new_wrapped(&document.buffer, &document.folds, 18, 8);
            let segment = document.viewport.row_segment(0, &document.buffer).unwrap();
            let fragment = &source[segment.start_column..segment.end_column];
            assert!(fragment.contains('\u{2e80}') && fragment.contains('骨'));
        } else {
            document.scroll.horizontal_px = 10.0;
        }
        assert_fragment_hits(&document, &settings, 0, CjkLanguage::SimplifiedChinese);
    }
}

#[test]
fn ui_editor_hits_actual_glyph_boundaries_after_horizontal_scroll() {
    let settings = EditorSettings {
        zoom: 2.0,
        ..EditorSettings::default()
    };
    let mut document = Document::from_path(
        DocumentId::new(20),
        "horizontal-cjk.txt",
        "かな骨直令¥①②「日月」！？天地",
    );
    document.scroll.horizontal_px = 35.0;
    assert_eq!(
        document.cjk_context().language_for_line(0),
        Some(CjkLanguage::Japanese)
    );
    assert_fragment_hits(&document, &settings, 0, CjkLanguage::Japanese);
}

#[test]
fn ui_editor_hits_wrapped_han_when_language_cue_is_outside_fragment() {
    let settings = EditorSettings {
        zoom: 2.0,
        word_wrap: true,
        ..EditorSettings::default()
    };
    for (source, language) in [
        ("骨直令「日月」かな", CjkLanguage::Japanese),
        ("骨直令「日月」한글", CjkLanguage::Korean),
    ] {
        let mut document = Document::from_path(DocumentId::new(21), "wrapped-cjk.txt", source);
        document.set_word_wrap(true);
        document.viewport = ViewportModel::new_wrapped(&document.buffer, &document.folds, 8, 4);
        let segment = document.viewport.row_segment(0, &document.buffer).unwrap();
        let first = &source[segment.start_column..segment.end_column];
        assert!(!first.contains("かな") && !first.contains("한글"));
        assert_eq!(document.cjk_context().language_for_line(0), Some(language));
        assert_fragment_hits(&document, &settings, 0, language);
    }
}

fn draw_editor_pixels(
    document: &Document,
    settings: &EditorSettings,
    tree: &mut Tree,
    renderer: &mut Renderer,
) -> Vec<u8> {
    let size = Size::new(520.0, 360.0);
    let bounds = Rectangle::with_size(size);
    let mut content = editor::view(document, settings);
    tree.diff(content.as_widget_mut());
    let node = content
        .as_widget_mut()
        .layout(tree, renderer, &layout::Limits::new(size, size));
    renderer.reset(bounds);
    content.as_widget().draw(
        tree,
        renderer,
        &Theme::Light,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &bounds,
    );
    renderer.screenshot(Size::new(520, 360), 1.0, iced::Color::WHITE)
}

fn shared_han_row_pixels(bytes: &[u8]) -> Vec<u8> {
    // At zoom 2, the Han row spans y=44..84 and its paragraph starts at y=48.
    // Skip the top padding, where Noto CJK glyphs from the cue row can overhang,
    // and exclude the gutter and any region-specific script in the changed cue.
    (48..84)
        .flat_map(|y| {
            bytes[(y * 520 + 90) * 4..(y * 520 + 500) * 4]
                .iter()
                .copied()
        })
        .collect()
}

#[test]
fn neighboring_line_edits_leave_cached_han_glyphs_unchanged() {
    let settings = EditorSettings {
        zoom: 2.0,
        ..EditorSettings::default()
    };
    let japanese = Document::from_path(
        DocumentId::new(22),
        "context.txt",
        "かな\n骨直令曜角返門海雨",
    );
    let korean = Document::from_path(
        DocumentId::new(22),
        "context.txt",
        "한글\n骨直令曜角返門海雨",
    );
    assert_eq!(japanese.buffer.line(1), korean.buffer.line(1));
    let mut renderer = software_renderer();
    let mut retained_tree = Tree::empty();
    let before = draw_editor_pixels(&japanese, &settings, &mut retained_tree, &mut renderer);
    let after = draw_editor_pixels(&korean, &settings, &mut retained_tree, &mut renderer);
    let fresh = draw_editor_pixels(&korean, &settings, &mut Tree::empty(), &mut renderer);
    assert_eq!(
        shared_han_row_pixels(&before),
        shared_han_row_pixels(&fresh),
        "Shared Han must not inherit kana or Hangul from a different line"
    );
    assert_eq!(
        shared_han_row_pixels(&after),
        shared_han_row_pixels(&fresh),
        "Retained widget cache must preserve the unchanged line's own font"
    );
}

#[test]
fn unchanged_wrapped_han_row_refreshes_when_its_logical_line_cue_changes() {
    if regional_cjk_font(CjkLanguage::Japanese) == regional_cjk_font(CjkLanguage::Korean) {
        eprintln!("Distinct regional installed fonts required for pixel comparison");
        return;
    }
    let settings = EditorSettings {
        zoom: 2.0,
        word_wrap: true,
        ..EditorSettings::default()
    };
    let make_document = |cue: &str| {
        let mut document = Document::from_path(
            DocumentId::new(22),
            "context.txt",
            &format!("ASCII\n骨直令曜角返門海雨 {cue}"),
        );
        document.set_word_wrap(true);
        document.viewport = ViewportModel::new_wrapped(&document.buffer, &document.folds, 18, 4);
        document
    };
    let japanese = make_document("かな");
    let korean = make_document("한글");
    for document in [&japanese, &korean] {
        let segment = document.viewport.row_segment(1, &document.buffer).unwrap();
        let source = document.buffer.line(1).unwrap();
        assert_eq!(
            &source[segment.start_column..segment.end_column],
            "骨直令曜角返門海雨"
        );
    }
    let mut renderer = software_renderer();
    let mut retained_tree = Tree::empty();
    let before = draw_editor_pixels(&japanese, &settings, &mut retained_tree, &mut renderer);
    let after = draw_editor_pixels(&korean, &settings, &mut retained_tree, &mut renderer);
    let fresh = draw_editor_pixels(&korean, &settings, &mut Tree::empty(), &mut renderer);
    assert_ne!(
        shared_han_row_pixels(&before),
        shared_han_row_pixels(&fresh)
    );
    assert_eq!(shared_han_row_pixels(&after), shared_han_row_pixels(&fresh));
}

#[test]
fn wrapped_mixed_language_han_retains_each_logical_run_font() {
    if regional_cjk_font(CjkLanguage::Japanese) == regional_cjk_font(CjkLanguage::Korean) {
        eprintln!("Distinct regional installed fonts required for pixel comparison");
        return;
    }
    let source = "かな 骨直令 한글 骨直令";
    let settings = EditorSettings {
        zoom: 2.0,
        word_wrap: true,
        ..EditorSettings::default()
    };
    let mut document = Document::from_path(DocumentId::new(24), "mixed.txt", source);
    document.set_word_wrap(true);
    document.decorations.settings.show_wrap_guide = false;
    document.decorations.settings.show_wrap_indicator = false;
    document.viewport = ViewportModel::new_wrapped(&document.buffer, &document.folds, 4, 4);
    let mut renderer = software_renderer();
    let actual = draw_editor_pixels(&document, &settings, &mut Tree::empty(), &mut renderer);
    let korean_cue = source.find("한글").unwrap();
    let mut checked = [false; 2];

    for row in 0..document.viewport.visible_row_count() {
        let segment = document
            .viewport
            .row_segment(row, &document.buffer)
            .unwrap();
        let fragment = &source[segment.start_column..segment.end_column];
        if !fragment.chars().any(|ch| "骨直令".contains(ch))
            || !fragment
                .chars()
                .all(|ch| ch.is_whitespace() || "骨直令".contains(ch))
        {
            continue;
        }
        let (language, index) = if segment.start_column < korean_cue {
            (CjkLanguage::Japanese, 0)
        } else {
            (CjkLanguage::Korean, 1)
        };
        let metrics = editor_metrics(&document, &settings);
        let origin = metrics.text_origin_x(&document.decorations) as usize;
        let paragraph = routed_paragraph(fragment, language, settings.zoom);
        let width = paragraph.min_bounds().width.ceil() as usize + 2;
        let bounds = Rectangle::with_size(Size::new(520.0, 360.0));
        let style = EditorStyle::from_theme(&Theme::Light);
        renderer.reset(bounds);
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            style.active_line,
        );
        renderer.fill_paragraph(
            &paragraph,
            Point::new(
                origin as f32,
                metrics.padding_top + metrics.line_height + text_baseline_offset(metrics),
            ),
            style.syntax_fallback_text,
            bounds,
        );
        let expected = renderer.screenshot(Size::new(520, 360), 1.0, style.active_line);
        let crop = |bytes: &[u8], row: usize| -> Vec<u8> {
            ((4 + row * 40)..(4 + (row + 1) * 40))
                .flat_map(|y| {
                    bytes[(y * 520 + origin) * 4..(y * 520 + origin + width) * 4]
                        .iter()
                        .copied()
                })
                .collect()
        };
        assert!(
            crop(&actual, row) == crop(&expected, 1),
            "Wrapped Han-only fragment {fragment:?} on row {row} must retain {language:?} from its logical run"
        );
        checked[index] = true;
    }
    assert_eq!(
        checked,
        [true, true],
        "Exercise Han-only fragments from both language runs"
    );
}

#[test]
fn complex_mixed_fixture_wraps_with_contextual_pixels_and_caret_geometry() {
    let settings = EditorSettings {
        zoom: 1.5,
        word_wrap: true,
        ..EditorSettings::default()
    };
    let mut document = Document::from_path(DocumentId::new(31), "mixed.txt", MIXED);
    document.set_word_wrap(true);
    document.decorations.settings.show_wrap_guide = false;
    document.decorations.settings.show_wrap_indicator = false;
    document.decorations.settings.show_indentation_guides = false;
    document.viewport = ViewportModel::new_wrapped(&document.buffer, &document.folds, 18, 4);
    let mut renderer = software_renderer();
    let mut checked = [false; 5];
    let context = document.cjk_context();
    let size = Size::new(520.0, 360.0);
    let bounds = Rectangle::with_size(size);
    let style = EditorStyle::from_theme(&Theme::Light);

    for row in 0..document.viewport.visible_row_count() {
        let line = document.viewport.visible_row_to_document_line(row).unwrap();
        let segment = document
            .viewport
            .row_segment(row, &document.buffer)
            .unwrap();
        let source = document.buffer.line(line).unwrap();
        let fragment = &source[segment.start_column..segment.end_column];
        if !fragment.chars().any(is_cjk_letter) {
            continue;
        }
        let mask = context
            .runs_for_fragment(line, segment.start_column, fragment)
            .iter()
            .fold(0_u8, |mask, run| {
                mask | match run.language {
                    Some(CjkLanguage::SimplifiedChinese) => 1,
                    Some(CjkLanguage::TraditionalChinese) => 2,
                    Some(CjkLanguage::Japanese) => 4,
                    Some(CjkLanguage::Korean) => 8,
                    None => 0,
                }
            });
        let category = match mask {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            mixed if mixed.count_ones() > 1 && mixed & 12 != 0 => 4,
            _ => continue,
        };
        if checked[category] {
            continue;
        }
        document.scroll.first_visible_row = row;
        let paragraph = contextual_paragraph(
            &document,
            line,
            segment.start_column,
            fragment,
            settings.zoom,
        );
        let metrics = editor_metrics(&document, &settings);
        let origin = metrics.text_origin_x(&document.decorations);
        let width = paragraph.min_bounds().width.ceil() as usize + 2;
        assert!(
            origin as usize + width < 520,
            "Wrapped fragment fits image crop"
        );
        let actual = draw_editor_pixels(&document, &settings, &mut Tree::empty(), &mut renderer);

        renderer.reset(bounds);
        let background = if line == document.selection_set().main().cursor.line {
            style.active_line
        } else {
            style.surface
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            background,
        );
        renderer.fill_paragraph(
            &paragraph,
            Point::new(origin, metrics.padding_top + text_baseline_offset(metrics)),
            style.syntax_fallback_text,
            bounds,
        );
        let expected = renderer.screenshot(Size::new(520, 360), 1.0, background);
        let crop = |bytes: &[u8]| -> Vec<u8> {
            ((metrics.padding_top as usize)..((metrics.padding_top + metrics.line_height) as usize))
                .flat_map(|y| {
                    bytes[(y * 520 + origin as usize) * 4..(y * 520 + origin as usize + width) * 4]
                        .iter()
                        .copied()
                })
                .collect()
        };
        let actual_crop = crop(&actual);
        let expected_crop = crop(&expected);
        if actual_crop != expected_crop {
            std::fs::create_dir_all("target/cjk-test-review").unwrap();
            for (name, bytes) in [("actual", &actual), ("expected", &expected)] {
                tiny_skia::Pixmap::from_vec(
                    bytes.clone(),
                    tiny_skia::IntSize::from_wh(520, 360).unwrap(),
                )
                .unwrap()
                .save_png(format!("target/cjk-test-review/{name}.png"))
                .unwrap();
            }
        }
        assert!(
            actual_crop == expected_crop,
            "Actual wrapped fragment {fragment:?} must render original logical context routes, mask={mask}; first unequal byte {:?}",
            actual_crop
                .iter()
                .zip(&expected_crop)
                .position(|(actual, expected)| actual != expected)
        );

        let mut clicks = 0;
        for (grapheme, (byte, character)) in fragment.grapheme_indices(true).enumerate() {
            if !character.chars().any(is_cjk_letter) {
                continue;
            }
            let x = paragraph.grapheme_position(0, grapheme).unwrap().x;
            let point = Point::new(
                origin + x + 0.2,
                metrics.padding_top + metrics.line_height * 0.5,
            );
            let position = EditorPosition::new(line, segment.start_column + byte);
            let actions = widget_click(&document, &settings, point);
            assert!(
                actions.contains(&EditorAction::PlaceCaretOnRow { position, row }),
                "Complex contextual boundary {position:?} must match shaped advance; actions {actions:?}"
            );
            clicks += 1;
            if clicks == 3 {
                break;
            }
        }
        assert!(clicks > 0);
        checked[category] = true;
        if checked.into_iter().all(|checked| checked) {
            break;
        }
    }
    assert_eq!(
        checked, [true; 5],
        "Exercise SC, TC, JP, KR, and a script transition in wrapped mixed paragraphs"
    );
}
