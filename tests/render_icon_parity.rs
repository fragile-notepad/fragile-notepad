use iced::advanced::image;
use iced::advanced::renderer::{self, Headless};
use iced::{Color, Rectangle, Size};

use fragile_notepad::ui::icons::ICON_SIZE;
use fragile_notepad::ui::icons::colored::ColoredIconAsset;
use fragile_notepad::ui::icons::hero::HeroIconAsset;
use fragile_notepad::ui::icons::shortcut::ShortcutIconAsset;

const SURFACE_SIZE: u32 = 32;

#[derive(Debug, Clone, Copy)]
enum TestIconAsset {
    Hero(HeroIconAsset),
    Shortcut(ShortcutIconAsset),
    Colored(ColoredIconAsset),
}

impl TestIconAsset {
    fn rgba_bytes(self) -> &'static [u8] {
        match self {
            Self::Hero(asset) => asset.rgba_bytes(),
            Self::Shortcut(asset) => asset.rgba_bytes(),
            Self::Colored(asset) => asset.rgba_bytes(),
        }
    }
}

const ICONS: &[(&str, TestIconAsset)] = &[
    (
        "bootstrap/command",
        TestIconAsset::Shortcut(ShortcutIconAsset::Command),
    ),
    (
        "bootstrap/option",
        TestIconAsset::Shortcut(ShortcutIconAsset::Option),
    ),
    (
        "bootstrap/pin-angle-fill",
        TestIconAsset::Shortcut(ShortcutIconAsset::PinAngleFill),
    ),
    (
        "bootstrap/pin-angle",
        TestIconAsset::Shortcut(ShortcutIconAsset::PinAngle),
    ),
    (
        "bootstrap/shift-fill",
        TestIconAsset::Shortcut(ShortcutIconAsset::ShiftFill),
    ),
    (
        "bootstrap/shift",
        TestIconAsset::Shortcut(ShortcutIconAsset::Shift),
    ),
    (
        "bootstrap/windows",
        TestIconAsset::Shortcut(ShortcutIconAsset::Windows),
    ),
    (
        "heroicons/arrow-turn-down-left",
        TestIconAsset::Hero(HeroIconAsset::ArrowTurnDownLeft),
    ),
    (
        "heroicons/arrow-turn-down-right",
        TestIconAsset::Hero(HeroIconAsset::ArrowTurnDownRight),
    ),
    (
        "heroicons/chevron-down",
        TestIconAsset::Hero(HeroIconAsset::ChevronDown),
    ),
    (
        "heroicons/chevron-right",
        TestIconAsset::Hero(HeroIconAsset::ChevronRight),
    ),
    ("heroicons/minus", TestIconAsset::Hero(HeroIconAsset::Minus)),
    ("heroicons/plus", TestIconAsset::Hero(HeroIconAsset::Plus)),
    (
        "heroicons/question-mark-circle",
        TestIconAsset::Hero(HeroIconAsset::QuestionMarkCircle),
    ),
    (
        "heroicons/x-mark",
        TestIconAsset::Hero(HeroIconAsset::XMark),
    ),
    (
        "colored/accessories-character-map",
        TestIconAsset::Colored(ColoredIconAsset::AccessoriesCharacterMap),
    ),
    (
        "colored/document-new",
        TestIconAsset::Colored(ColoredIconAsset::DocumentNew),
    ),
    (
        "colored/document-close",
        TestIconAsset::Colored(ColoredIconAsset::DocumentClose),
    ),
    (
        "colored/document-close-all",
        TestIconAsset::Colored(ColoredIconAsset::DocumentCloseAll),
    ),
    (
        "colored/document-save-all",
        TestIconAsset::Colored(ColoredIconAsset::DocumentSaveAll),
    ),
    (
        "colored/document-open",
        TestIconAsset::Colored(ColoredIconAsset::DocumentOpen),
    ),
    (
        "colored/document-print",
        TestIconAsset::Colored(ColoredIconAsset::DocumentPrint),
    ),
    (
        "colored/document-save-as",
        TestIconAsset::Colored(ColoredIconAsset::DocumentSaveAs),
    ),
    (
        "colored/document-save",
        TestIconAsset::Colored(ColoredIconAsset::DocumentSave),
    ),
    (
        "colored/edit-copy",
        TestIconAsset::Colored(ColoredIconAsset::EditCopy),
    ),
    (
        "colored/edit-cut",
        TestIconAsset::Colored(ColoredIconAsset::EditCut),
    ),
    (
        "colored/edit-delete",
        TestIconAsset::Colored(ColoredIconAsset::EditDelete),
    ),
    (
        "colored/edit-find-replace",
        TestIconAsset::Colored(ColoredIconAsset::EditFindReplace),
    ),
    (
        "colored/edit-find",
        TestIconAsset::Colored(ColoredIconAsset::EditFind),
    ),
    (
        "colored/edit-paste",
        TestIconAsset::Colored(ColoredIconAsset::EditPaste),
    ),
    (
        "colored/edit-redo",
        TestIconAsset::Colored(ColoredIconAsset::EditRedo),
    ),
    (
        "colored/edit-undo",
        TestIconAsset::Colored(ColoredIconAsset::EditUndo),
    ),
    (
        "colored/emblem-favorite",
        TestIconAsset::Colored(ColoredIconAsset::EmblemFavorite),
    ),
    (
        "colored/emblem-important",
        TestIconAsset::Colored(ColoredIconAsset::EmblemImportant),
    ),
    (
        "colored/format-indent-more",
        TestIconAsset::Colored(ColoredIconAsset::FormatIndentMore),
    ),
    (
        "colored/format-justify-fill",
        TestIconAsset::Colored(ColoredIconAsset::FormatJustifyFill),
    ),
    (
        "colored/process-stop",
        TestIconAsset::Colored(ColoredIconAsset::ProcessStop),
    ),
    (
        "colored/tab-close",
        TestIconAsset::Colored(ColoredIconAsset::TabClose),
    ),
    (
        "colored/tab-document-monitoring",
        TestIconAsset::Colored(ColoredIconAsset::TabDocumentMonitoring),
    ),
    (
        "colored/tab-document-read-only",
        TestIconAsset::Colored(ColoredIconAsset::TabDocumentReadOnly),
    ),
    (
        "colored/tab-document-saved",
        TestIconAsset::Colored(ColoredIconAsset::TabDocumentSaved),
    ),
    (
        "colored/tab-document-system-read-only",
        TestIconAsset::Colored(ColoredIconAsset::TabDocumentSystemReadOnly),
    ),
    (
        "colored/tab-document-unsaved",
        TestIconAsset::Colored(ColoredIconAsset::TabDocumentUnsaved),
    ),
    (
        "colored/text-x-generic-template",
        TestIconAsset::Colored(ColoredIconAsset::TextXGenericTemplate),
    ),
    (
        "colored/text-x-generic",
        TestIconAsset::Colored(ColoredIconAsset::TextXGeneric),
    ),
    (
        "colored/text-x-script",
        TestIconAsset::Colored(ColoredIconAsset::TextXScript),
    ),
    (
        "colored/zoom-in",
        TestIconAsset::Colored(ColoredIconAsset::ZoomIn),
    ),
    (
        "colored/zoom-out",
        TestIconAsset::Colored(ColoredIconAsset::ZoomOut),
    ),
];

fn draw_icon(renderer: &mut iced::Renderer, rgba: &[u8]) {
    let bounds = Rectangle {
        x: 5.0,
        y: 5.0,
        width: ICON_SIZE as f32,
        height: ICON_SIZE as f32,
    };
    let clip_bounds = Rectangle::with_size(Size::new(SURFACE_SIZE as f32, SURFACE_SIZE as f32));
    let handle = image::Handle::from_rgba(ICON_SIZE, ICON_SIZE, rgba.to_vec());
    let icon = image::Image::new(handle).filter_method(image::FilterMethod::Linear);

    renderer::Renderer::reset(renderer, clip_bounds);
    image::Renderer::draw_image(renderer, icon, bounds, clip_bounds);
}

fn render_icon(backend: &str, scale_factor: f32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some(backend),
    ))?;

    draw_icon(&mut renderer, rgba);

    let first = renderer.screenshot(
        Size::new(
            (SURFACE_SIZE as f32 * scale_factor).round() as u32,
            (SURFACE_SIZE as f32 * scale_factor).round() as u32,
        ),
        scale_factor,
        Color::TRANSPARENT,
    );
    let cached = renderer.screenshot(
        Size::new(
            (SURFACE_SIZE as f32 * scale_factor).round() as u32,
            (SURFACE_SIZE as f32 * scale_factor).round() as u32,
        ),
        scale_factor,
        Color::TRANSPARENT,
    );
    assert_eq!(
        first, cached,
        "cached {backend} rendering changed pixels at scale {scale_factor}"
    );
    Some(first)
}

fn diff_stats(a: &[u8], b: &[u8]) -> (u8, f32, usize) {
    let mut max_delta = 0;
    let mut sum_delta = 0usize;
    let mut changed_channels = 0usize;

    for (&a, &b) in a.iter().zip(b) {
        let delta = a.abs_diff(b);

        max_delta = max_delta.max(delta);
        sum_delta += usize::from(delta);

        if delta > 0 {
            changed_channels += 1;
        }
    }

    (
        max_delta,
        sum_delta as f32 / a.len() as f32,
        changed_channels,
    )
}

#[derive(Debug)]
struct TopEdgeOvershoot {
    amount: usize,
    detail: Option<(usize, usize, usize, u8, u8)>,
}

fn top_alpha_overshoot(
    cpu: &[u8],
    gpu: &[u8],
    width: usize,
    alpha_threshold: u8,
) -> TopEdgeOvershoot {
    let Some(cpu_top) = top_alpha_row(cpu, width, alpha_threshold) else {
        return TopEdgeOvershoot {
            amount: 0,
            detail: None,
        };
    };
    let Some(gpu_top) = top_alpha_row(gpu, width, alpha_threshold) else {
        return TopEdgeOvershoot {
            amount: 0,
            detail: None,
        };
    };

    // CPU/GPU quantization can put the same edge at alpha 33 and 32. Check
    // coverage at the same pixel before treating a threshold crossing as an
    // extra row. Scan every candidate so rounding noise cannot hide a real edge.
    const ALPHA_ROUNDING_TOLERANCE: u8 = 1;
    for y in cpu_top..gpu_top {
        for x in 0..width {
            let cpu_alpha = alpha_at(cpu, width, x, y);
            let gpu_alpha = alpha_at(gpu, width, x, y);
            if cpu_alpha > alpha_threshold
                && cpu_alpha.saturating_sub(gpu_alpha) > ALPHA_ROUNDING_TOLERANCE
            {
                return TopEdgeOvershoot {
                    amount: gpu_top - y,
                    detail: Some((x, y, gpu_top, cpu_alpha, gpu_alpha)),
                };
            }
        }
    }

    TopEdgeOvershoot {
        amount: 0,
        detail: None,
    }
}

fn top_alpha_row(image: &[u8], width: usize, alpha_threshold: u8) -> Option<usize> {
    let height = image.len() / width / 4;

    (0..height).find(|&y| (0..width).any(|x| alpha_at(image, width, x, y) > alpha_threshold))
}

fn alpha_at(image: &[u8], width: usize, x: usize, y: usize) -> u8 {
    image[(y * width + x) * 4 + 3]
}

#[test]
fn top_edge_ignores_rounding_without_hiding_real_overshoot() {
    let cpu = alpha_image(&[&[0, 33], &[0, 255]]);
    let gpu = alpha_image(&[&[0, 32], &[0, 255]]);
    let overshoot = top_alpha_overshoot(&cpu, &gpu, 2, 32);
    assert_eq!(overshoot.amount, 0);
    assert_eq!(overshoot.detail, None);

    for (cpu_alpha, gpu_alpha) in [(33, 0), (34, 32), (255, 0)] {
        let cpu = alpha_image(&[&[cpu_alpha], &[255]]);
        let gpu = alpha_image(&[&[gpu_alpha], &[255]]);
        let overshoot = top_alpha_overshoot(&cpu, &gpu, 1, 32);
        assert_eq!(overshoot.amount, 1);
        assert_eq!(overshoot.detail, Some((0, 0, 1, cpu_alpha, gpu_alpha)));
    }

    let cpu = alpha_image(&[&[33, 80], &[255, 255]]);
    let gpu = alpha_image(&[&[32, 0], &[255, 255]]);
    let overshoot = top_alpha_overshoot(&cpu, &gpu, 2, 32);
    assert_eq!(overshoot.amount, 1);
    assert_eq!(overshoot.detail, Some((1, 0, 1, 80, 0)));

    let cpu = alpha_image(&[&[33, 0], &[0, 96], &[0, 255]]);
    let gpu = alpha_image(&[&[32, 0], &[0, 0], &[0, 255]]);
    let overshoot = top_alpha_overshoot(&cpu, &gpu, 2, 32);
    assert_eq!(overshoot.amount, 1);
    assert_eq!(overshoot.detail, Some((1, 1, 2, 96, 0)));
}

fn alpha_image(rows: &[&[u8]]) -> Vec<u8> {
    rows.iter()
        .flat_map(|row| row.iter().flat_map(|&alpha| [0, 0, 0, alpha]))
        .collect()
}

#[test]
fn tiny_skia_and_wgpu_render_real_icon_consistently() {
    for &(name, asset) in ICONS {
        let rgba = asset.rgba_bytes();

        for scale_factor in [1.0, 1.5, 2.0] {
            let Some(cpu) = render_icon("tiny-skia", scale_factor, rgba) else {
                panic!("tiny-skia headless renderer should be available");
            };
            let Some(gpu) = render_icon("wgpu", scale_factor, rgba) else {
                if std::env::var_os("CI").is_some()
                    && std::env::var_os("FRAGILE_ALLOW_WGPU_PARITY_SKIP").is_none()
                {
                    panic!(
                        "wgpu headless renderer is unavailable in CI; set FRAGILE_ALLOW_WGPU_PARITY_SKIP=1 to opt out"
                    );
                }
                eprintln!(
                    "skipping icon render parity test: wgpu headless renderer is unavailable"
                );
                return;
            };

            assert_eq!(cpu.len(), gpu.len());

            let (max_delta, mean_delta, changed_channels) = diff_stats(&cpu, &gpu);
            let surface_width = (SURFACE_SIZE as f32 * scale_factor).round() as usize;
            const VISIBLE_EDGE_ALPHA: u8 = 32;

            let top_overshoot = top_alpha_overshoot(&cpu, &gpu, surface_width, VISIBLE_EDGE_ALPHA);
            let (max_allowed, mean_allowed) = if scale_factor == 1.0 {
                (12, 0.25)
            } else {
                (255, 3.0)
            };

            assert!(
                max_delta <= max_allowed && mean_delta <= mean_allowed,
                "CPU/GPU icon output diverged for {name} at scale_factor={scale_factor}: max_delta={max_delta}, mean_delta={mean_delta:.3}, changed_channels={changed_channels}"
            );

            assert_eq!(
                top_overshoot.amount,
                0,
                "CPU icon top edge extends above GPU for {name} at scale_factor={scale_factor}: top_overshoot={}, cpu_top={:?}, gpu_top={:?}, detail={:?}",
                top_overshoot.amount,
                top_alpha_row(&cpu, surface_width, VISIBLE_EDGE_ALPHA),
                top_alpha_row(&gpu, surface_width, VISIBLE_EDGE_ALPHA),
                top_overshoot.detail
            );
        }
    }
}
