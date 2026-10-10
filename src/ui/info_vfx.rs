//! Layered floating artwork and a soft, flowing quill trail for the About header.

use std::cell::RefCell;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, window};

use crate::message::Message;

#[cfg(feature = "hybrid-rendering")]
mod gpu;

const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
pub(super) const HEADER_HEIGHT: f32 = 96.0;
pub(super) const LOGO_SIZE: f32 = 80.0;
const TILE_SIZE: f32 = 64.0;
const BUNNY_SIZE: f32 = 96.0;
const QUIET_WIDTH: f32 = 260.0;
const ART_WIDTH: f32 = 280.0;
const QUILL_WIDTH: f32 = 72.0;
const QUILL_HEIGHT: f32 = 79.2;
const QUILL_RIGHT_INSET: f32 = 12.0;
const QUILL_PIXELS: &[u8] = include_bytes!("../../assets/illustrations/macaw-quill.rgba");
static QUILL: LazyLock<image::Handle> =
    LazyLock::new(|| image::Handle::from_rgba(400, 440, QUILL_PIXELS));

pub fn view(progress: f32, running: bool) -> Element<'static, Message> {
    Element::new(InfoVfx {
        progress: progress.clamp(0.0, 1.0),
        running,
    })
}

struct InfoVfx {
    progress: f32,
    running: bool,
}

struct State {
    #[cfg(feature = "hybrid-rendering")]
    gpu_instance: gpu::Instance,
    enabled: bool,
    focused: bool,
    zero_sized: bool,
    elapsed: f64,
    last_tick: Option<Instant>,
    next_tick: Option<Instant>,
    image: RefCell<Option<CachedField>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FieldKey {
    phase: u64,
    width: u32,
    height: u32,
    dark: bool,
}

struct CachedField {
    key: FieldKey,
    handle: image::Handle,
}

impl State {
    fn pause(&mut self) {
        self.last_tick = None;
        self.next_tick = None;
    }
}

impl Widget<Message, Theme, Renderer> for InfoVfx {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(HEADER_HEIGHT))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, HEADER_HEIGHT)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if self.progress <= 0.0 {
            return;
        }
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let phase = state.elapsed as f32 * std::f32::consts::TAU / 4.0;
        let tile = Rectangle {
            x: bounds.x + (LOGO_SIZE - TILE_SIZE) * 0.5,
            y: bounds.y + (HEADER_HEIGHT - TILE_SIZE) * 0.5 + 0.5 * phase.sin(),
            width: TILE_SIZE,
            height: TILE_SIZE,
        };
        let bunny = Rectangle {
            x: bounds.x + (LOGO_SIZE - BUNNY_SIZE) * 0.5 + 0.6 * phase.cos(),
            y: bounds.y + (HEADER_HEIGHT - BUNNY_SIZE) * 0.5 - 3.0 + 2.5 * phase.sin(),
            width: BUNNY_SIZE,
            height: BUNNY_SIZE,
        };
        let paper_phase = state.elapsed as f32 * std::f32::consts::TAU / 5.5 + 1.1;
        let paper = Rectangle {
            x: bounds.x + (LOGO_SIZE - BUNNY_SIZE) * 0.5 - 7.0 + 1.5 * paper_phase.cos(),
            y: bounds.y + (HEADER_HEIGHT - BUNNY_SIZE) * 0.5 - 6.0 + 3.5 * paper_phase.sin(),
            width: BUNNY_SIZE,
            height: BUNNY_SIZE,
        };
        for (handle, area) in [
            (crate::assets::about_background_handle(), tile),
            (crate::assets::about_paper_handle(), paper),
            (
                crate::assets::about_bunny_handle(blink_frame(state.elapsed)),
                bunny,
            ),
        ] {
            let Some(clip) = area.intersection(viewport) else {
                continue;
            };
            renderer.draw_image(
                image::Image::new(handle)
                    .filter_method(image::FilterMethod::Linear)
                    .snap(false)
                    .opacity(self.progress),
                area,
                clip,
            );
        }
        let Some(clip) = visible_art(bounds, *viewport) else {
            return;
        };
        let art = art_bounds(bounds);
        // This deliberately low-resolution field has no sharp details. Linear
        // sampling stays diffuse at every DPI while keeping generation bounded.
        let key = FieldKey {
            phase: state.elapsed.to_bits(),
            width: (art.width * 0.5).ceil().clamp(8.0, 160.0) as u32,
            height: (art.height * 0.5).ceil().clamp(8.0, 48.0) as u32,
            dark: theme.palette().is_dark,
        };
        #[cfg(feature = "hybrid-rendering")]
        let gpu_drawn = if matches!(renderer, Renderer::Primary(_)) {
            use iced::advanced::Renderer as _;
            use iced_wgpu::primitive::Renderer as _;
            renderer.with_layer(clip, |renderer| {
                renderer.draw_primitive(
                    art,
                    gpu::Trail {
                        instance: state.gpu_instance.clone(),
                        time: state.elapsed as f32,
                        opacity: self.progress,
                        dark: key.dark,
                    },
                );
            });
            state.image.borrow_mut().take();
            true
        } else {
            false
        };
        #[cfg(not(feature = "hybrid-rendering"))]
        let gpu_drawn = false;
        if !gpu_drawn {
            let mut cached = state.image.borrow_mut();
            if cached.as_ref().is_none_or(|field| field.key != key) {
                *cached = Some(CachedField {
                    key,
                    handle: image::Handle::from_rgba(key.width, key.height, render_field(key)),
                });
            }
            if let Some(field) = cached.as_ref() {
                renderer.draw_image(
                    image::Image::new(&field.handle)
                        .filter_method(image::FilterMethod::Linear)
                        .opacity(self.progress),
                    art,
                    clip,
                );
            }
        }
        if bounds.width < QUIET_WIDTH + QUILL_WIDTH + QUILL_RIGHT_INSET {
            return;
        }
        let quill_bounds = Rectangle {
            x: bounds.x + bounds.width - QUILL_WIDTH - QUILL_RIGHT_INSET,
            y: bounds.y + (HEADER_HEIGHT - QUILL_HEIGHT) * 0.5,
            width: QUILL_WIDTH,
            height: QUILL_HEIGHT,
        };
        // Keep the feather secondary to the bunny. The original artwork stays
        // sharp across desktop scales and shares one handle across instances.
        // The width guard keeps it entirely clear of the title.
        if let Some(clip) = quill_bounds.intersection(viewport) {
            renderer.draw_image(
                image::Image::new(&*QUILL)
                    .filter_method(image::FilterMethod::Linear)
                    .opacity(self.progress),
                quill_bounds,
                clip,
            );
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            #[cfg(feature = "hybrid-rendering")]
            gpu_instance: gpu::Instance::default(),
            enabled: self.running && self.progress > 0.0,
            focused: true,
            zero_sized: false,
            elapsed: 0.0,
            last_tick: None,
            next_tick: None,
            image: RefCell::new(None),
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        let enabled = self.running && self.progress > 0.0;
        if enabled != state.enabled {
            state.enabled = enabled;
            state.pause();
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Window(window::Event::Unfocused | window::Event::Closed) => {
                state.focused = false;
                state.pause();
            }
            Event::Window(window::Event::Focused)
            | Event::Mouse(mouse::Event::ButtonPressed(_))
            | Event::Keyboard(iced::keyboard::Event::KeyPressed { .. })
            | Event::Touch(iced::touch::Event::FingerPressed { .. }) => {
                if !state.focused {
                    state.focused = true;
                    state.pause();
                }
            }
            Event::Window(window::Event::Resized(size)) => {
                state.zero_sized = size.width <= 0.0 || size.height <= 0.0;
                state.pause();
            }
            _ => {}
        }

        if !self.running
            || self.progress <= 0.0
            || !state.focused
            || state.zero_sized
            || layout.bounds().intersection(viewport).is_none()
        {
            state.pause();
            return;
        }

        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        if let Event::Window(window::Event::RedrawRequested(_)) = event
            && state.next_tick.is_none_or(|deadline| now >= deadline)
        {
            if let Some(previous) = state.last_tick {
                // If presentation stalls without a focus/resize event,
                // do not jump the light field forward on its return.
                state.elapsed += now
                    .saturating_duration_since(previous)
                    .min(FRAME_INTERVAL * 2)
                    .as_secs_f64();
            }
            state.last_tick = Some(now);
            // Keep the 60 Hz cadence anchored when a display presents late,
            // without catch-up bursts.
            let deadline = state.next_tick.unwrap_or(now);
            let remainder =
                now.saturating_duration_since(deadline).as_nanos() % FRAME_INTERVAL.as_nanos();
            state.next_tick = Some(now + FRAME_INTERVAL - Duration::from_nanos(remainder as u64));
        }
        let deadline = *state.next_tick.get_or_insert(now + FRAME_INTERVAL);
        shell.request_redraw_at(deadline);
    }
}

// A 250ms close/hold/open gesture every four seconds, on the same pausable
// clock as the floating motion. Static application/title icons never blink.
fn blink_frame(elapsed: f64) -> u8 {
    let phase = elapsed.rem_euclid(4.0);
    if (2.75..3.0).contains(&phase) {
        if (2.75 + 1.0 / 12.0..2.75 + 1.0 / 6.0).contains(&phase) {
            2
        } else {
            1
        }
    } else {
        0
    }
}

fn art_bounds(bounds: Rectangle) -> Rectangle {
    let width = (bounds.width - QUIET_WIDTH).clamp(0.0, ART_WIDTH);
    Rectangle {
        x: bounds.x + bounds.width - width,
        width,
        ..bounds
    }
}

fn visible_art(bounds: Rectangle, viewport: Rectangle) -> Option<Rectangle> {
    let art = art_bounds(bounds);
    if art.width < 16.0 || art.height <= 0.0 {
        return None;
    }
    art.intersection(&viewport)
        .filter(|clip| clip.width > 0.0 && clip.height > 0.0)
}

fn smooth(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

/// Curved ribbons with a diffuse halo, encoded as straight RGBA.
/// Only one small image is retained; opacity is applied later by the renderer.
fn render_field(key: FieldKey) -> Vec<u8> {
    let time = f64::from_bits(key.phase) as f32;
    let colors = if key.dark {
        [
            [133.0, 195.0, 241.0],
            [185.0, 163.0, 235.0],
            [238.0, 181.0, 172.0],
        ]
    } else {
        [
            [66.0, 145.0, 203.0],
            [145.0, 116.0, 205.0],
            [219.0, 146.0, 139.0],
        ]
    };
    let mut pixels = vec![0; key.width as usize * key.height as usize * 4];
    for x in 0..key.width {
        let u = x as f32 / (key.width - 1) as f32;
        // Each wisp tapers into the quill's nib. Motion is strongest in the
        // trailing end, leaving a quiet, stable attachment near the feather.
        let t = (u / 0.80).clamp(0.0, 1.0);
        let arch = (t * std::f32::consts::PI).sin();
        let flow = (t * 7.0 - time * 0.65).sin();
        let center = 0.64 - 0.23 * arch + 0.16 * t + 0.035 * flow * arch;
        let taper = smooth(u / 0.30) * smooth((0.94 - u) / 0.18);
        for y in 0..key.height {
            let v = y as f32 / (key.height - 1) as f32;
            let envelope = taper * smooth(v / 0.20) * smooth((1.0 - v) / 0.22);
            let mut weight = 0.0;
            let mut rgb = [0.0; 3];
            for (strand, color) in colors.into_iter().enumerate() {
                let offset = (strand as f32 - 1.0) * 0.105 * arch;
                let drift = 0.025 * (time * 0.5 + t * 5.0 + strand as f32 * 1.8).sin() * arch;
                let distance = v - center - offset - drift;
                let halo = (-0.5 * (distance / 0.13).powi(2)).exp() * 0.20;
                let ribbon = (-0.5 * (distance / 0.055).powi(2)).exp() * 0.30;
                let pulse = 0.88 + 0.12 * (t * 9.0 - time * 0.8 + strand as f32).cos();
                let light = (halo + ribbon) * pulse;
                weight += light;
                for channel in 0..3 {
                    rgb[channel] += color[channel] * light;
                }
            }
            let alpha = (1.0 - (-weight).exp()) * envelope * if key.dark { 0.50 } else { 0.44 };
            let index = ((y * key.width + x) * 4) as usize;
            for channel in 0..3 {
                pixels[index + channel] = (rgb[channel] / weight.max(0.0001))
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            pixels[index + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Color;
    use iced::advanced::Renderer as _;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::Headless;

    const BOUNDS: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 600.0,
        height: HEADER_HEIGHT,
    };

    fn renderer() -> Renderer {
        futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("CPU renderer")
    }

    fn mount(renderer: &Renderer) -> (Element<'static, Message>, Tree, layout::Node) {
        let mut widget = view(1.0, true);
        let mut tree = Tree::empty();
        tree.diff(widget.as_widget_mut());
        let node = widget.as_widget_mut().layout(
            &mut tree,
            renderer,
            &layout::Limits::new(Size::ZERO, BOUNDS.size()),
        );
        (widget, tree, node)
    }

    fn update(
        widget: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        event: Event,
        viewport: Rectangle,
    ) -> window::RedrawRequest {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
        widget.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Unavailable,
            renderer,
            &mut shell,
            &viewport,
        );
        assert!(!shell.are_widgets_invalid());
        assert!(shell.is_layout_invalid().is_none());
        assert!(shell.is_empty());
        shell.redraw_request()
    }

    #[test]
    fn blink_is_brief_returns_to_open_and_repeats() {
        for (at, expected) in [
            (0.0, 0),
            (2.7, 0),
            (2.78, 1),
            (2.87, 2),
            (2.96, 1),
            (3.0, 0),
        ] {
            assert_eq!(blink_frame(at), expected);
            assert_eq!(blink_frame(at + 4.0), expected);
        }
    }

    #[test]
    fn narrow_header_keeps_the_logo_visible_without_the_decorative_field() {
        let mut renderer = renderer();
        let mut widget = view(1.0, true);
        let mut tree = Tree::empty();
        tree.diff(widget.as_widget_mut());
        let bounds = Rectangle::with_size(Size::new(240.0, HEADER_HEIGHT));
        let node = widget.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, bounds.size()),
        );
        assert!(visible_art(bounds, bounds).is_none());
        renderer.reset(bounds);
        widget.as_widget().draw(
            &tree,
            &mut renderer,
            &Theme::Light,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &bounds,
        );
        let pixels = renderer.screenshot(
            Size::new(240, HEADER_HEIGHT as u32),
            1.0,
            iced::Color::TRANSPARENT,
        );
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(matches!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(Instant::now())),
                bounds,
            ),
            window::RedrawRequest::At(_)
        ));
    }

    #[test]
    fn deadline_throttles_external_and_duplicate_redraws_without_relayout() {
        let renderer = renderer();
        let (mut widget, mut tree, node) = mount(&renderer);
        let start = Instant::now();
        let tick = |at| Event::Window(window::Event::RedrawRequested(at));
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start),
                BOUNDS
            ),
            window::RedrawRequest::At(start + FRAME_INTERVAL)
        );
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start + FRAME_INTERVAL / 2),
                BOUNDS
            ),
            window::RedrawRequest::At(start + FRAME_INTERVAL)
        );
        assert_eq!(tree.state.downcast_ref::<State>().elapsed, 0.0);
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            tick(start + FRAME_INTERVAL),
            BOUNDS,
        );
        let elapsed = tree.state.downcast_ref::<State>().elapsed;
        assert!((elapsed - FRAME_INTERVAL.as_secs_f64()).abs() < 1e-9);
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start + FRAME_INTERVAL),
                BOUNDS
            ),
            window::RedrawRequest::At(start + FRAME_INTERVAL * 2)
        );
        assert_eq!(tree.state.downcast_ref::<State>().elapsed, elapsed);
        assert_eq!(node.bounds(), BOUNDS);
    }

    #[test]
    fn floating_motion_advances_on_each_sixty_hz_display_frame() {
        let renderer = renderer();
        let (mut widget, mut tree, node) = mount(&renderer);
        let start = Instant::now();
        let mut changes = 0;
        for frame in 0..=120 {
            let before = tree.state.downcast_ref::<State>().elapsed;
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(
                    start + Duration::from_nanos(16_666_667) * frame,
                )),
                BOUNDS,
            );
            changes += usize::from(tree.state.downcast_ref::<State>().elapsed != before);
        }
        assert_eq!(
            changes, 120,
            "floating artwork must advance on every 60 Hz display frame"
        );
    }

    #[test]
    fn pause_unfocus_clipping_and_zero_size_stop_frames_and_resume_without_jump() {
        let renderer = renderer();
        let (mut widget, mut tree, node) = mount(&renderer);
        let start = Instant::now();
        let tick = |at| Event::Window(window::Event::RedrawRequested(at));
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            tick(start),
            BOUNDS,
        );
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            tick(start + FRAME_INTERVAL),
            BOUNDS,
        );
        let elapsed = tree.state.downcast_ref::<State>().elapsed;
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::Unfocused),
                BOUNDS
            ),
            window::RedrawRequest::Wait
        );
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start + Duration::from_secs(10)),
                BOUNDS
            ),
            window::RedrawRequest::Wait
        );
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::Focused),
            BOUNDS,
        );
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            tick(start + Duration::from_secs(20)),
            BOUNDS,
        );
        assert_eq!(tree.state.downcast_ref::<State>().elapsed, elapsed);

        widget = view(1.0, false);
        tree.diff(widget.as_widget_mut());
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start + Duration::from_secs(30)),
                BOUNDS
            ),
            window::RedrawRequest::Wait
        );
        widget = view(1.0, true);
        tree.diff(widget.as_widget_mut());
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            tick(start + Duration::from_secs(40)),
            BOUNDS,
        );
        assert_eq!(tree.state.downcast_ref::<State>().elapsed, elapsed);

        let hidden = Rectangle {
            x: 0.0,
            y: 100.0,
            ..BOUNDS
        };
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start + Duration::from_secs(50)),
                hidden
            ),
            window::RedrawRequest::Wait
        );
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            tick(start + Duration::from_secs(60)),
            BOUNDS,
        );
        assert_eq!(tree.state.downcast_ref::<State>().elapsed, elapsed);
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::Resized(Size::ZERO)),
                BOUNDS
            ),
            window::RedrawRequest::Wait
        );
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start + Duration::from_secs(70)),
                BOUNDS
            ),
            window::RedrawRequest::Wait
        );

        // Restore the resize gate so zero-opacity scheduling is tested on its own.
        update(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::Resized(BOUNDS.size())),
            BOUNDS,
        );
        assert!(!tree.state.downcast_ref::<State>().zero_sized);
        widget = view(0.0, true);
        tree.diff(widget.as_widget_mut());
        assert_eq!(
            update(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                tick(start + Duration::from_secs(80)),
                BOUNDS
            ),
            window::RedrawRequest::Wait
        );
    }

    fn snapshot(
        renderer: &mut Renderer,
        theme: &Theme,
        progress: f32,
        elapsed: f64,
        scale: f32,
    ) -> Vec<u8> {
        renderer.hint(scale);
        let mut widget = view(progress, false);
        let mut tree = Tree::empty();
        tree.diff(widget.as_widget_mut());
        tree.state.downcast_mut::<State>().elapsed = elapsed;
        let node = widget.as_widget_mut().layout(
            &mut tree,
            renderer,
            &layout::Limits::new(Size::ZERO, BOUNDS.size()),
        );
        renderer.reset(BOUNDS);
        widget.as_widget().draw(
            &tree,
            renderer,
            theme,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &BOUNDS,
        );
        renderer.screenshot(
            Size::new(
                (BOUNDS.width * scale) as u32,
                (BOUNDS.height * scale) as u32,
            ),
            scale,
            Color::TRANSPARENT,
        )
    }

    #[test]
    fn floating_artwork_moves_between_subpixel_software_frames() {
        assert_subpixel_motion(renderer());
    }

    #[test]
    #[cfg(feature = "hybrid-rendering")]
    fn floating_artwork_moves_between_subpixel_vulkan_frames() {
        let Some(renderer) = futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("wgpu"),
        )) else {
            eprintln!("Skipping Vulkan floating artwork validation: renderer unavailable");
            return;
        };
        assert_subpixel_motion(renderer);
    }

    fn assert_subpixel_motion(mut renderer: Renderer) {
        for scale in [1.0, 1.5, 2.0] {
            let mut previous = None;
            for frame in 0..12 {
                let pixels = snapshot(
                    &mut renderer,
                    &Theme::Light,
                    1.0,
                    frame as f64 / 60.0,
                    scale,
                );
                // Isolate the logo from the independently animated quill trail.
                let logo: Vec<u8> = pixels
                    .chunks_exact((BOUNDS.width * scale) as usize * 4)
                    .flat_map(|row| row[..(100.0 * scale) as usize * 4].iter().copied())
                    .collect();
                if let Some(previous) = previous {
                    assert!(
                        logo != previous,
                        "floating artwork stalled at frame {frame}, scale {scale}"
                    );
                }
                previous = Some(logo);
            }
        }
    }

    #[test]
    fn light_field_cache_reuses_handles_and_invalidates_only_for_paint_changes() {
        let mut renderer = renderer();
        let (mut widget, mut tree, node) = mount(&renderer);
        let theme = crate::ui::styles::modern_theme(crate::core::AppearanceMode::Light).unwrap();
        let draw = |widget: &Element<'_, Message>,
                    tree: &Tree,
                    renderer: &mut Renderer,
                    theme: &Theme,
                    node: &layout::Node| {
            renderer.reset(BOUNDS);
            widget.as_widget().draw(
                tree,
                renderer,
                theme,
                &renderer::Style::default(),
                Layout::new(node),
                mouse::Cursor::Unavailable,
                &BOUNDS,
            );
            tree.state
                .downcast_ref::<State>()
                .image
                .borrow()
                .as_ref()
                .unwrap()
                .handle
                .id()
        };
        let first = draw(&widget, &tree, &mut renderer, &theme, &node);
        assert_eq!(first, draw(&widget, &tree, &mut renderer, &theme, &node));
        widget = view(0.4, false);
        tree.diff(widget.as_widget_mut());
        assert_eq!(first, draw(&widget, &tree, &mut renderer, &theme, &node));
        tree.state.downcast_mut::<State>().elapsed = 3.0;
        let moved = draw(&widget, &tree, &mut renderer, &theme, &node);
        assert_ne!(first, moved);
        let dark = crate::ui::styles::modern_theme(crate::core::AppearanceMode::Dark).unwrap();
        assert_ne!(moved, draw(&widget, &tree, &mut renderer, &dark, &node));
        let previous = draw(&widget, &tree, &mut renderer, &dark, &node);
        let narrow = layout::Node::new(Size::new(440.0, HEADER_HEIGHT));
        assert_ne!(
            previous,
            draw(&widget, &tree, &mut renderer, &dark, &narrow)
        );
        let field_before_dpi = draw(&widget, &tree, &mut renderer, &dark, &narrow);
        renderer.hint(1.5);
        assert_eq!(
            field_before_dpi,
            draw(&widget, &tree, &mut renderer, &dark, &narrow)
        );
    }

    #[test]
    fn quill_artwork_has_valid_dimensions_and_transparent_edges() {
        const WIDTH: usize = 400;
        const HEIGHT: usize = 440;
        assert_eq!(QUILL_PIXELS.len(), WIDTH * HEIGHT * 4);
        let alpha = |x: usize, y: usize| QUILL_PIXELS[(y * WIDTH + x) * 4 + 3];
        for x in 0..WIDTH {
            assert_eq!(alpha(x, 0), 0);
            assert_eq!(alpha(x, HEIGHT - 1), 0);
        }
        for y in 0..HEIGHT {
            assert_eq!(alpha(0, y), 0);
            assert_eq!(alpha(WIDTH - 1, y), 0);
        }
        assert!(QUILL_PIXELS.chunks_exact(4).any(|pixel| pixel[3] > 200));
        assert!(
            QUILL_PIXELS
                .chunks_exact(4)
                .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
        );
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = &*QUILL
        else {
            panic!("quill must use its original RGBA artwork");
        };
        assert_eq!((*width as usize, *height as usize), (WIDTH, HEIGHT));
        assert_eq!(pixels.len(), QUILL_PIXELS.len());
    }

    #[test]
    fn diffuse_field_is_straight_rgba_and_feathers_smoothly_at_every_edge() {
        for dark in [false, true] {
            let key = FieldKey {
                phase: 2.0_f64.to_bits(),
                width: 140,
                height: 42,
                dark,
            };
            let pixels = render_field(key);
            let alpha = |x: u32, y: u32| pixels[((y * key.width + x) * 4 + 3) as usize];
            for x in 0..key.width {
                assert_eq!(alpha(x, 0), 0);
                assert_eq!(alpha(x, key.height - 1), 0);
            }
            for y in 0..key.height {
                assert_eq!(alpha(0, y), 0);
                assert_eq!(alpha(key.width - 1, y), 0);
            }
            let mut largest_step = 0;
            for y in 1..key.height {
                for x in 1..key.width {
                    largest_step = largest_step.max(alpha(x, y).abs_diff(alpha(x - 1, y)));
                    largest_step = largest_step.max(alpha(x, y).abs_diff(alpha(x, y - 1)));
                }
            }
            assert!(
                largest_step <= 20,
                "diffuse field must not introduce hard alpha edges: {largest_step}"
            );
            assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 50));
            // Bright RGB under low alpha proves this is straight, not premultiplied.
            assert!(
                pixels
                    .chunks_exact(4)
                    .any(|pixel| pixel[3] > 0 && pixel[3] < 20 && pixel[0] > 100)
            );
        }
    }

    #[test]
    #[cfg(feature = "hybrid-rendering")]
    fn vulkan_handoff_releases_cpu_field_and_rollback_restores_the_same_phase() {
        let mut software = renderer();
        let Some(mut hardware) = futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("wgpu"),
        )) else {
            eprintln!("Skipping Vulkan handoff validation: renderer unavailable");
            return;
        };
        let (widget, mut tree, node) = mount(&software);
        tree.state.downcast_mut::<State>().elapsed = 2.9;
        let draw = |renderer: &mut Renderer| {
            renderer.reset(BOUNDS);
            widget.as_widget().draw(
                &tree,
                renderer,
                &Theme::Light,
                &renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &BOUNDS,
            );
            renderer.screenshot(
                Size::new(BOUNDS.width as u32, BOUNDS.height as u32),
                1.0,
                Color::TRANSPARENT,
            )
        };
        let before = draw(&mut software);
        assert!(tree.state.downcast_ref::<State>().image.borrow().is_some());
        let gpu_pixels = draw(&mut hardware);
        assert!(gpu_pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(tree.state.downcast_ref::<State>().image.borrow().is_none());
        let after = draw(&mut software);
        assert_eq!(before, after);
        assert!(tree.state.downcast_ref::<State>().image.borrow().is_some());
        assert_eq!(tree.state.downcast_ref::<State>().elapsed, 2.9);
    }

    #[test]
    #[cfg(feature = "hybrid-rendering")]
    fn vulkan_trail_matches_fallback_and_keeps_instances_and_clipping_independent() {
        let Some(renderer) = futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("wgpu"),
        )) else {
            eprintln!("Skipping Vulkan shader parity validation: renderer unavailable");
            return;
        };
        assert_trail_parity(renderer);

        // Also render the uniform-buffer path on a device without immediates.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let Some(adapter) =
            futures::executor::block_on(instance.request_adapter(&Default::default())).ok()
        else {
            eprintln!("Skipping Vulkan uniform parity validation: no Vulkan adapter");
            return;
        };
        let Ok((device, queue)) =
            futures::executor::block_on(adapter.request_device(&Default::default()))
        else {
            eprintln!("Skipping Vulkan uniform parity validation: device unavailable");
            return;
        };
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            None,
            iced_wgpu::graphics::Shell::headless(),
        );
        assert_trail_parity(Renderer::Primary(iced_wgpu::Renderer::new(
            engine,
            renderer::Settings::default(),
        )));
    }

    #[cfg(feature = "hybrid-rendering")]
    fn assert_trail_parity(mut renderer: Renderer) {
        use iced_wgpu::primitive::Renderer as _;
        let instances = [gpu::Instance::default(), gpu::Instance::default()];
        let surface = Rectangle::with_size(Size::new(640.0, 128.0));
        for scale in [1.0, 1.5, 2.0] {
            renderer.hint(scale);
            for time in [0.0, 2.9, 7.25] {
                let mut screenshots = Vec::new();
                for shader in [false, true] {
                    renderer.reset(surface);
                    for (id, x, dark, opacity) in [(1, 16.0, false, 1.0), (2, 336.0, true, 0.4)] {
                        let area = Rectangle {
                            x,
                            y: 16.0,
                            width: 280.0,
                            height: 96.0,
                        };
                        let clip = Rectangle {
                            x: x + 20.0,
                            y: 28.0,
                            width: 240.0,
                            height: 72.0,
                        };
                        let phase = time + id as f64;
                        if shader {
                            renderer.with_layer(clip, |renderer| {
                                renderer.draw_primitive(
                                    area,
                                    gpu::Trail {
                                        instance: instances[id as usize - 1].clone(),
                                        time: phase as f32,
                                        opacity,
                                        dark,
                                    },
                                );
                            });
                        } else {
                            let key = FieldKey {
                                phase: phase.to_bits(),
                                width: 140,
                                height: 48,
                                dark,
                            };
                            renderer.draw_image(
                                image::Image::new(image::Handle::from_rgba(
                                    140,
                                    48,
                                    render_field(key),
                                ))
                                .filter_method(image::FilterMethod::Linear)
                                .opacity(opacity),
                                area,
                                clip,
                            );
                        }
                    }
                    screenshots.push(renderer.screenshot(
                        Size::new((640.0 * scale) as u32, (128.0 * scale) as u32),
                        scale,
                        Color::TRANSPARENT,
                    ));
                }
                let reference = &screenshots[0];
                let gpu = &screenshots[1];
                let differences: Vec<_> = reference
                    .iter()
                    .zip(gpu)
                    .map(|(a, b)| a.abs_diff(*b))
                    .collect();
                let max = differences.iter().copied().max().unwrap();
                let mean =
                    differences.iter().map(|d| *d as f64).sum::<f64>() / differences.len() as f64;
                // Both paths interpolate a bounded half-resolution, 8-bit
                // field; GPU shader arithmetic and quantization can differ slightly.
                assert!(
                    max <= 12 && mean < 1.0,
                    "scale={scale} time={time} max={max} mean={mean}"
                );
                let width = (640.0 * scale) as usize;
                for (index, pixel) in gpu.chunks_exact(4).enumerate() {
                    let x = (index % width) as f32 / scale;
                    let y = (index / width) as f32 / scale;
                    if !(28.0..100.0).contains(&y)
                        || !((36.0..276.0).contains(&x) || (356.0..596.0).contains(&x))
                    {
                        assert_eq!(pixel, &[0, 0, 0, 0], "GPU trail escaped its clip");
                    }
                }
            }
        }
    }

    #[test]
    fn cpu_pixels_fade_to_exact_zero_leave_title_clear_and_change_deterministically() {
        let mut renderer = renderer();
        for appearance in [
            crate::core::AppearanceMode::Light,
            crate::core::AppearanceMode::Dark,
        ] {
            let theme = crate::ui::styles::modern_theme(appearance).unwrap();
            for scale in [1.0, 1.5] {
                let invisible = snapshot(&mut renderer, &theme, 0.0, 0.0, scale);
                assert!(invisible.chunks_exact(4).all(|pixel| pixel[3] == 0));
                let first = snapshot(&mut renderer, &theme, 1.0, 0.0, scale);
                let repeat = snapshot(&mut renderer, &theme, 1.0, 0.0, scale);
                let later = snapshot(&mut renderer, &theme, 1.0, 3.0, scale);
                assert_eq!(first, repeat);
                assert_ne!(first, later);
                assert!(first.chunks_exact(4).any(|pixel| pixel[3] > 0));
                let width = (BOUNDS.width * scale) as usize;
                let calm_width = (QUIET_WIDTH * scale) as usize;
                for row in first.chunks_exact(width * 4) {
                    assert!(
                        row[(LOGO_SIZE * scale) as usize * 4..calm_width * 4]
                            .chunks_exact(4)
                            .all(|pixel| pixel[3] == 0)
                    );
                }
                let partial = snapshot(&mut renderer, &theme, 0.4, 0.0, scale);
                let sum = |image: &[u8]| {
                    image
                        .chunks_exact(4)
                        .map(|pixel| pixel[3] as u64)
                        .sum::<u64>()
                };
                assert!(sum(&partial) > 0 && sum(&partial) < sum(&first));
            }
        }
    }
}
