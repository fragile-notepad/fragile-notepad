#[cfg(feature = "hybrid-rendering")]
#[path = "support/gpu_validation.rs"]
mod gpu_validation;

use fragile_notepad::ui::icons::colored::{self, ColoredIcon};
use fragile_notepad::ui::icons::hero::{self, HeroIcon, IconTone};
use fragile_notepad::ui::icons::shortcut::{self, ShortcutIcon};
use fragile_notepad::ui::styles;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, layout, mouse};
use iced::{Color, Element, Point, Radians, Rectangle, Renderer, Size, Theme};

const CANVAS: Size<f32> = Size::new(176.0, 64.0);

struct Capture {
    pixels: Vec<u8>,
    size: Size<u32>,
    scale: f32,
}

impl Capture {
    fn region(&self, bounds: Rectangle) -> impl Iterator<Item = (u32, u32, [u8; 4])> + '_ {
        self.pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(move |(index, pixel)| {
                let x = index as u32 % self.size.width;
                let y = index as u32 / self.size.width;
                bounds
                    .contains(Point::new(
                        (x as f32 + 0.5) / self.scale,
                        (y as f32 + 0.5) / self.scale,
                    ))
                    .then_some((x, y, [pixel[0], pixel[1], pixel[2], pixel[3]]))
            })
    }

    fn ink(&self, bounds: Rectangle) -> Vec<(u32, u32)> {
        self.region(bounds)
            .filter(|(_, _, pixel)| pixel[..3].iter().any(|channel| *channel < 235))
            .map(|(x, y, _)| (x, y))
            .collect()
    }
}

fn begin(renderer: &mut Renderer, scale: f32) {
    renderer.reset(Rectangle::with_size(CANVAS));
    renderer.hint(scale);
}

fn capture(renderer: &mut Renderer, scale: f32, background: Color) -> Capture {
    let size = Size::new(
        (CANVAS.width * scale).round() as u32,
        (CANVAS.height * scale).round() as u32,
    );
    let pixels = renderer.screenshot(size, scale, background);
    assert_eq!(pixels.len(), size.width as usize * size.height as usize * 4);
    Capture {
        pixels,
        size,
        scale,
    }
}

fn paint(
    renderer: &mut Renderer,
    mut icon: Element<'static, ()>,
    size: u32,
    position: Point,
    theme: &Theme,
) {
    let mut tree = Tree::new(icon.as_widget());
    let node = icon
        .as_widget_mut()
        .layout(
            &mut tree,
            renderer,
            &layout::Limits::new(Size::ZERO, CANVAS),
        )
        .move_to(position);
    assert_eq!(node.size(), Size::new(size as f32, size as f32));
    icon.as_widget().draw(
        &tree,
        renderer,
        theme,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &Rectangle::with_size(CANVAS),
    );
}

fn extent(points: &[(u32, u32)]) -> (u32, u32) {
    let left = points.iter().map(|&(x, _)| x).min().expect("visible ink");
    let right = points.iter().map(|&(x, _)| x).max().unwrap();
    let top = points.iter().map(|&(_, y)| y).min().unwrap();
    let bottom = points.iter().map(|&(_, y)| y).max().unwrap();
    (right - left + 1, bottom - top + 1)
}

fn check_icons_at_display_sizes(renderer: &mut Renderer) {
    for size in [14, 22, 44] {
        for scale in [1.0, 1.25, 1.5] {
            begin(renderer, scale);
            for (x, icon) in [
                (8.0, colored::icon(ColoredIcon::Find, size)),
                (
                    64.0,
                    shortcut::icon_with_color(
                        ShortcutIcon::Shift,
                        size,
                        styles::shortcut_text_color,
                    ),
                ),
                (
                    120.0,
                    hero::icon(HeroIcon::QuestionMarkCircle, size, IconTone::Text),
                ),
            ] {
                paint(renderer, icon, size, Point::new(x, 8.0), &Theme::Light);
            }
            let frame = capture(renderer, scale, Color::WHITE);
            for x in [8.0, 64.0, 120.0] {
                let slot = Rectangle::new(Point::new(x - 2.0, 6.0), Size::new(48.0, 48.0));
                let ink = frame.ink(slot);
                let (width, height) = extent(&ink);
                let physical_size = size as f32 * scale;
                assert!(
                    width as f32 > physical_size * 0.4,
                    "size={size}, scale={scale}"
                );
                assert!(
                    height as f32 > physical_size * 0.5,
                    "size={size}, scale={scale}"
                );
                assert!(ink.len() as f32 > physical_size.powi(2) * 0.035);
                assert!(
                    ink.iter().all(|&(px, py)| {
                        px as f32 >= (x * scale).floor()
                            && (px as f32) < ((x + size as f32) * scale).ceil()
                            && py as f32 >= (8.0 * scale).floor()
                            && (py as f32) < ((8.0 + size as f32) * scale).ceil()
                    }),
                    "icon outside layout bounds: size={size}, scale={scale}"
                );
            }
        }
    }
}

fn check_theme_colors(renderer: &mut Renderer) {
    let scale = 1.5;
    for theme in [Theme::Light, Theme::Dark] {
        begin(renderer, scale);
        paint(
            renderer,
            hero::icon(HeroIcon::XMark, 44, IconTone::Text),
            44,
            Point::new(8.0, 8.0),
            &theme,
        );
        paint(
            renderer,
            shortcut::icon_with_color(ShortcutIcon::PinAngleFill, 44, styles::shortcut_text_color),
            44,
            Point::new(64.0, 8.0),
            &theme,
        );
        let frame = capture(renderer, scale, Color::from_rgb8(128, 128, 128));
        let expected = styles::shortcut_text_color(&theme).into_rgba8();
        for x in [8.0, 64.0] {
            let matching = frame
                .region(Rectangle::new(Point::new(x, 8.0), Size::new(44.0, 44.0)))
                .filter(|(_, _, pixel)| {
                    pixel[..3]
                        .iter()
                        .zip(expected[..3].iter())
                        .all(|(actual, expected)| actual.abs_diff(*expected) <= 12)
                })
                .count();
            assert!(matching > 10, "icon color differs from theme: {theme:?}");
        }
    }
}

fn check_disabled_icons(renderer: &mut Renderer) {
    let scale = 1.5;
    for theme in [Theme::Light, Theme::Dark] {
        let background = styles::tool_bar_background(&theme);
        let background_bytes = background.into_rgba8();
        for size in [18, 44] {
            let mut frames = Vec::new();
            for icon in [
                colored::icon(ColoredIcon::Print, size),
                colored::disabled_icon(ColoredIcon::Print, size),
            ] {
                begin(renderer, scale);
                paint(renderer, icon, size, Point::new(8.0, 8.0), &theme);
                frames.push(capture(renderer, scale, background));
            }
            let mut max_difference = 0;
            for (enabled, disabled) in frames[0]
                .pixels
                .chunks_exact(4)
                .zip(frames[1].pixels.chunks_exact(4))
            {
                for channel in 0..3 {
                    // Fading the completed icon must preserve occlusion between paths.
                    let expected = (f32::from(background_bytes[channel])
                        + (f32::from(enabled[channel]) - f32::from(background_bytes[channel]))
                            * 0.38)
                        .round() as u8;
                    max_difference = max_difference.max(disabled[channel].abs_diff(expected));
                }
                assert_eq!(disabled[3], 255);
            }
            assert!(
                max_difference <= 3,
                "disabled print icon differs from the completed image: {theme:?}, size={size}, difference={max_difference}"
            );
        }
    }
}

fn check_magnifier_center(renderer: &mut Renderer) {
    let scale = 1.5;
    begin(renderer, scale);
    paint(
        renderer,
        colored::icon(ColoredIcon::Find, 44),
        44,
        Point::new(8.0, 8.0),
        &Theme::Light,
    );
    let magnifier = capture(renderer, scale, Color::WHITE);
    // The magnifier has a pale center and a blue outline.
    assert!(
        magnifier
            .region(Rectangle::new(Point::new(23.0, 23.0), Size::new(6.0, 6.0)))
            .all(|(_, _, pixel)| pixel[0] >= 230 && pixel[1] >= 230 && pixel[2] >= 230)
    );
    assert!(
        magnifier
            .region(Rectangle::new(Point::new(8.0, 8.0), Size::new(44.0, 44.0)))
            .filter(|(_, _, pixel)| pixel[2] > pixel[0].saturating_add(40))
            .count()
            > 40
    );
}

fn check_rotation(renderer: &mut Renderer) {
    let scale = 1.5;
    let bounds = Rectangle::new(Point::new(12.0, 10.0), Size::new(44.0, 44.0));
    for rotation in [0.0, std::f32::consts::FRAC_PI_2] {
        begin(renderer, scale);
        hero::draw(
            renderer,
            HeroIcon::ChevronRight,
            bounds,
            bounds,
            Color::BLACK,
            Radians(rotation),
        );
        let frame = capture(renderer, scale, Color::WHITE);
        let (width, height) = extent(&frame.ink(Rectangle::with_size(CANVAS)));
        if rotation == 0.0 {
            assert!(height as f32 > width as f32 * 1.25);
        } else {
            assert!(width as f32 > height as f32 * 1.25);
        }
    }
}

fn check_straight_strokes(renderer: &mut Renderer) {
    for size in [10.0, 14.0] {
        for scale in [1.0, 1.25, 1.5] {
            for rotation in [0.0, std::f32::consts::FRAC_PI_2] {
                begin(renderer, scale);
                let bounds = Rectangle::new(Point::new(12.0, 10.0), Size::new(size, size));
                hero::draw(
                    renderer,
                    HeroIcon::Minus,
                    bounds,
                    bounds,
                    Color::BLACK,
                    Radians(rotation),
                );
                let frame = capture(renderer, scale, Color::WHITE);
                let ink = frame.ink(bounds);
                assert!(
                    !ink.is_empty(),
                    "minus icon disappeared: size={size}, scale={scale}, rotation={rotation}"
                );
                let (width, height) = extent(&ink);
                let length = if rotation == 0.0 { width } else { height };
                assert!(length as f32 >= size * scale * 0.4);
            }
        }
    }
}

fn check_clipping(renderer: &mut Renderer) {
    let scale = 1.5;
    let bounds = Rectangle::new(Point::new(12.0, 10.0), Size::new(44.0, 44.0));
    begin(renderer, scale);
    let clip = Rectangle {
        x: bounds.center_x(),
        width: bounds.width / 2.0,
        ..bounds
    };
    hero::draw(
        renderer,
        HeroIcon::ChevronRight,
        bounds,
        clip,
        Color::BLACK,
        Radians(0.0),
    );
    let clipped = capture(renderer, scale, Color::WHITE);
    let ink = clipped.ink(Rectangle::with_size(CANVAS));
    assert!(!ink.is_empty(), "clipping removed the entire chevron");
    assert!(
        ink.iter().all(|&(x, y)| clip.contains(Point::new(
            (x as f32 + 0.5) / scale,
            (y as f32 + 0.5) / scale,
        ))),
        "icon outside clip rectangle"
    );
}

fn check_vector_icons(renderer: &mut Renderer) {
    check_icons_at_display_sizes(renderer);
    check_theme_colors(renderer);
    check_disabled_icons(renderer);
    check_magnifier_center(renderer);
    check_rotation(renderer);
    check_straight_strokes(renderer);
    check_clipping(renderer);
}

#[test]
fn vector_icons_render_on_software() {
    let mut renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("headless software renderer");
    check_vector_icons(&mut renderer);
}

#[cfg(feature = "hybrid-rendering")]
#[test]
fn vector_icons_render_on_vulkan() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Some(adapter) =
        futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
    else {
        gpu_validation::unavailable("vector icons: no Vulkan adapter");
        return;
    };
    let (device, queue) = futures::executor::block_on(adapter.request_device(&Default::default()))
        .expect("create Vulkan vector icon test device");
    let engine = iced_wgpu::Engine::new(
        &adapter,
        device,
        queue,
        if iced_wgpu::graphics::color::GAMMA_CORRECTION {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        Some(iced_wgpu::graphics::Antialiasing::MSAAx4),
        iced_wgpu::graphics::Shell::headless(),
    );
    let mut renderer = Renderer::Primary(iced_wgpu::Renderer::new(
        engine,
        renderer::Settings::default(),
    ));
    check_vector_icons(&mut renderer);
}
