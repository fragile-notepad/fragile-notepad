//! SVGs compiled into paths, shared by the software and Vulkan geometry backends.

use iced::advanced::graphics::cache::{self, Cached};
use iced::advanced::graphics::color;
use iced::advanced::graphics::geometry::{
    self, Fill, Frame, LineCap, LineJoin, Path, Stroke, fill, path,
};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Layout, layout, mouse, renderer};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Size, Theme, Vector};
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

const GEOMETRY_CACHE_LIMIT: usize = 512;

thread_local! {
    // Renderers and their geometry stay on the UI thread. Keep separate typed
    // stores so software and hybrid builds can use the same drawing code.
    static GEOMETRY_CACHES: RefCell<HashMap<TypeId, Box<dyn Any>>> =
        RefCell::new(HashMap::new());
}

type RendererGeometryCache<Renderer> =
    <<Renderer as geometry::Renderer>::Geometry as Cached>::Cache;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GeometryKey {
    icon: usize,
    backend: usize,
    size: [u32; 2],
    clip: [u32; 4],
    color: [u32; 4],
    opacity: u32,
    background: [u32; 4],
    strength: u32,
    rotation: u32,
}

struct GeometryEntry<Geometry> {
    geometry: Geometry,
    last_used: u64,
}

struct GeometryCache<Geometry> {
    entries: HashMap<GeometryKey, GeometryEntry<Geometry>>,
    group: cache::Group,
    clock: u64,
}

impl<Geometry> GeometryCache<Geometry> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            group: cache::Group::unique(),
            clock: 0,
        }
    }

    fn get_or_insert_with(
        &mut self,
        key: GeometryKey,
        build: impl FnOnce(cache::Group) -> Geometry,
    ) -> &Geometry {
        self.clock += 1;
        if self.entries.len() >= GEOMETRY_CACHE_LIMIT && !self.entries.contains_key(&key) {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key)
                .expect("a full geometry cache has an entry");
            self.entries.remove(&oldest);
        }
        let entry = self.entries.entry(key).or_insert_with(|| GeometryEntry {
            geometry: build(self.group),
            last_used: self.clock,
        });
        entry.last_used = self.clock;
        &entry.geometry
    }
}

#[allow(dead_code)]
pub(super) mod compiled {
    use super::*;
    include!(concat!(env!("OUT_DIR"), "/vector_icons.rs"));
}

pub(super) struct VectorIcon {
    view_box: [f32; 4],
    paths: Vec<PaintedPath>,
}

struct PaintedPath {
    path: Path,
    fill: Paint,
    stroke: Paint,
    fill_rule: fill::Rule,
    stroke_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
}

#[derive(Clone, Copy)]
enum Paint {
    None,
    CurrentColor,
    Solid(Color),
}

impl Paint {
    fn color(self, style: PaintStyle) -> Option<Color> {
        match self {
            Self::None => None,
            Self::CurrentColor => Some(style.color(style.current_color)),
            Self::Solid(color) => Some(style.color(color)),
        }
    }
}

#[derive(Clone, Copy)]
struct PaintStyle {
    current_color: Color,
    opacity: f32,
    disabled: Option<DisabledPalette>,
}

impl PaintStyle {
    fn color(self, color: Color) -> Color {
        self.disabled
            .map_or(color, |palette| palette.color(color))
            .scale_alpha(self.opacity)
    }
}

#[derive(Clone, Copy)]
struct DisabledPalette {
    background: Color,
    strength: f32,
}

impl DisabledPalette {
    fn color(self, color: Color) -> Color {
        // Preserve source alpha so opaque front shapes still hide the paths
        // below them. Blend RGB in the rendering backends' color space.
        let foreground = color::pack(color).components();
        let background = color::pack(self.background).components();
        let channel = |index: usize| {
            background[index] + (foreground[index] - background[index]) * self.strength
        };
        if color::GAMMA_CORRECTION {
            Color::from_linear_rgba(channel(0), channel(1), channel(2), color.a)
        } else {
            Color::from_rgba(channel(0), channel(1), channel(2), color.a)
        }
    }
}

impl VectorIcon {
    pub(super) fn draw(
        &'static self,
        renderer: &mut impl geometry::Renderer,
        bounds: Rectangle,
        clip: Rectangle,
        color: Color,
        opacity: f32,
        rotation: Radians,
    ) {
        self.draw_style(
            renderer,
            bounds,
            clip,
            PaintStyle {
                current_color: color,
                opacity,
                disabled: None,
            },
            rotation,
        );
    }

    fn draw_style(
        &'static self,
        renderer: &mut impl geometry::Renderer,
        bounds: Rectangle,
        clip: Rectangle,
        style: PaintStyle,
        rotation: Radians,
    ) {
        let Some(clip) = bounds.intersection(&clip) else {
            return;
        };
        if bounds.width <= 0.0 || bounds.height <= 0.0 || style.opacity <= 0.0 {
            return;
        }
        let clip = Rectangle {
            x: clip.x - bounds.x,
            y: clip.y - bounds.y,
            ..clip
        };
        let current = style.current_color;
        let palette = style.disabled.unwrap_or(DisabledPalette {
            background: Color::TRANSPARENT,
            strength: 1.0,
        });
        let key = GeometryKey {
            icon: self as *const Self as usize,
            backend: renderer.cache_backend(),
            size: [bounds.width.to_bits(), bounds.height.to_bits()],
            clip: [clip.x, clip.y, clip.width, clip.height].map(f32::to_bits),
            color: [current.r, current.g, current.b, current.a].map(f32::to_bits),
            opacity: style.opacity.to_bits(),
            background: [
                palette.background.r,
                palette.background.g,
                palette.background.b,
                palette.background.a,
            ]
            .map(f32::to_bits),
            strength: palette.strength.to_bits(),
            rotation: rotation.0.to_bits(),
        };
        let geometry = cached_geometry(renderer, key, |frame| {
            self.paint(frame, bounds.size(), style, rotation);
        });
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry);
        });
    }

    fn paint<Renderer: geometry::Renderer>(
        &self,
        frame: &mut Frame<Renderer>,
        size: Size,
        style: PaintStyle,
        rotation: Radians,
    ) {
        let [x, y, width, height] = self.view_box;
        let scale = (size.width / width).min(size.height / height);
        frame.translate(Vector::new(size.width / 2.0, size.height / 2.0));
        frame.rotate(rotation);
        frame.scale(scale);
        frame.translate(Vector::new(-x - width / 2.0, -y - height / 2.0));
        for path in &self.paths {
            if let Some(color) = path.fill.color(style) {
                frame.fill(
                    &path.path,
                    Fill {
                        style: geometry::Style::Solid(color),
                        rule: path.fill_rule,
                    },
                );
            }
            if let Some(color) = path.stroke.color(style)
                && path.stroke_width > 0.0
            {
                // Both geometry backends transform paths before stroking them;
                // their stroke widths therefore need the icon scale explicitly.
                frame.stroke(
                    &path.path,
                    Stroke::default()
                        .with_color(color)
                        .with_width(path.stroke_width * scale)
                        .with_line_cap(path.line_cap)
                        .with_line_join(path.line_join),
                );
            }
        }
    }
}

fn cached_geometry<Renderer: geometry::Renderer>(
    renderer: &Renderer,
    key: GeometryKey,
    draw: impl FnOnce(&mut Frame<Renderer>),
) -> Renderer::Geometry {
    GEOMETRY_CACHES.with(|caches| {
        let mut caches = caches.borrow_mut();
        let cache = caches
            .entry(TypeId::of::<RendererGeometryCache<Renderer>>())
            .or_insert_with(|| Box::new(GeometryCache::<RendererGeometryCache<Renderer>>::new()))
            .downcast_mut::<GeometryCache<RendererGeometryCache<Renderer>>>()
            .expect("geometry caches are indexed by their type");
        let geometry = cache.get_or_insert_with(key, |group| {
            let [x, y, width, height] = key.clip.map(f32::from_bits);
            let mut frame = Frame::with_bounds(
                renderer,
                Rectangle::new(Point::new(x, y), Size::new(width, height)),
            );
            draw(&mut frame);
            frame.into_geometry().cache(group, None)
        });
        Renderer::Geometry::load(geometry)
    })
}

pub(super) fn icon<Message: 'static>(
    icon: &'static VectorIcon,
    size: u32,
    color: fn(&Theme) -> Color,
    opacity: f32,
) -> Element<'static, Message> {
    Element::new(Icon {
        icon,
        size,
        color,
        opacity: opacity.clamp(0.0, 1.0),
        disabled: None,
    })
}

pub(super) fn disabled_icon<Message: 'static>(
    icon: &'static VectorIcon,
    size: u32,
    color: fn(&Theme) -> Color,
    background: fn(&Theme) -> Color,
    strength: f32,
) -> Element<'static, Message> {
    Element::new(Icon {
        icon,
        size,
        color,
        opacity: 1.0,
        disabled: Some(DisabledIcon {
            background,
            strength: strength.clamp(0.0, 1.0),
        }),
    })
}

struct DisabledIcon {
    background: fn(&Theme) -> Color,
    strength: f32,
}

struct Icon {
    icon: &'static VectorIcon,
    size: u32,
    color: fn(&Theme) -> Color,
    opacity: f32,
    disabled: Option<DisabledIcon>,
}

impl<Message, Renderer: geometry::Renderer> Widget<Message, Theme, Renderer> for Icon {
    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(self.size as f32),
            Length::Fixed(self.size as f32),
        )
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = <Self as Widget<Message, Theme, Renderer>>::size(self);
        layout::Node::new(limits.resolve(size.width, size.height, Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.icon.draw_style(
            renderer,
            layout.bounds(),
            *viewport,
            PaintStyle {
                current_color: (self.color)(theme),
                opacity: self.opacity,
                disabled: self.disabled.as_ref().map(|disabled| DisabledPalette {
                    background: (disabled.background)(theme),
                    strength: disabled.strength,
                }),
            },
            Radians(0.0),
        );
    }
}

fn svg_arc(
    builder: &mut path::Builder,
    from: Point,
    radii: Vector,
    rotation: f32,
    large_arc: bool,
    sweep: bool,
    to: Point,
) {
    use path::lyon_path::{geom, math};

    if from == to {
        return;
    }
    if radii.x == 0.0 || radii.y == 0.0 {
        builder.line_to(to);
        return;
    }
    let arc = geom::SvgArc {
        from: math::Point::new(from.x, from.y),
        to: math::Point::new(to.x, to.y),
        radii: math::Vector::new(radii.x.abs(), radii.y.abs()),
        x_rotation: math::Angle::degrees(rotation),
        flags: geom::ArcFlags { large_arc, sweep },
    };
    if arc.is_straight_line() {
        builder.line_to(to);
        return;
    }
    arc.to_arc().for_each_cubic_bezier(&mut |curve| {
        builder.bezier_curve_to(
            Point::new(curve.ctrl1.x, curve.ctrl1.y),
            Point::new(curve.ctrl2.x, curve.ctrl2.y),
            Point::new(curve.to.x, curve.to.y),
        );
    });
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn key() -> GeometryKey {
        GeometryKey {
            icon: 1,
            backend: 0,
            size: [24.0_f32.to_bits(); 2],
            clip: [0.0, 0.0, 24.0, 24.0].map(f32::to_bits),
            color: [0.0, 0.0, 0.0, 1.0].map(f32::to_bits),
            opacity: 1.0_f32.to_bits(),
            background: [0.0; 4].map(f32::to_bits),
            strength: 1.0_f32.to_bits(),
            rotation: 0.0_f32.to_bits(),
        }
    }

    fn clear_geometry_cache() {
        GEOMETRY_CACHES.with(|caches| caches.borrow_mut().clear());
    }

    #[test]
    fn cache_builds_each_icon_once() {
        clear_geometry_cache();
        let builds = Cell::new(0);
        for _ in 0..100 {
            cached_geometry(&(), key(), |_| builds.set(builds.get() + 1));
        }
        assert_eq!(builds.get(), 1);
    }

    #[test]
    fn cache_reuses_moved_icons() {
        clear_geometry_cache();
        let mut renderer = ();
        for position in [Point::new(0.0, 0.0), Point::new(120.0, 80.0)] {
            compiled::HEROICONS_X_MARK.draw(
                &mut renderer,
                Rectangle::new(position, Size::new(24.0, 24.0)),
                Rectangle::with_size(Size::new(640.0, 480.0)),
                Color::BLACK,
                1.0,
                Radians(0.0),
            );
        }
        GEOMETRY_CACHES.with(|caches| {
            let caches = caches.borrow();
            let cache = caches
                .get(&TypeId::of::<()>())
                .unwrap()
                .downcast_ref::<GeometryCache<()>>()
                .unwrap();
            assert_eq!(cache.entries.len(), 1);
        });
    }

    #[test]
    fn cache_keys_include_render_settings() {
        clear_geometry_cache();
        let original = key();
        let mut changed = [original; 10];
        changed[1].icon += 1;
        changed[2].backend += 1;
        changed[3].size[0] = 48.0_f32.to_bits();
        changed[4].clip[0] = 1.0_f32.to_bits();
        changed[5].color[0] = 1.0_f32.to_bits();
        changed[6].opacity = 0.5_f32.to_bits();
        changed[7].rotation = 0.5_f32.to_bits();
        changed[8].background[0] = 1.0_f32.to_bits();
        changed[9].strength = 0.38_f32.to_bits();
        let builds = Cell::new(0);
        for key in changed.into_iter().chain(changed).chain([original]) {
            cached_geometry(&(), key, |_| builds.set(builds.get() + 1));
        }
        assert_eq!(builds.get(), changed.len());
    }

    #[test]
    fn cache_evicts_the_least_recently_used_icon() {
        let mut cache = GeometryCache::new();
        for icon in 0..GEOMETRY_CACHE_LIMIT {
            cache.get_or_insert_with(GeometryKey { icon, ..key() }, |_| icon);
        }
        let recently_used = GeometryKey { icon: 0, ..key() };
        cache.get_or_insert_with(recently_used, |_| panic!("cached geometry was rebuilt"));
        let next = GeometryKey {
            icon: GEOMETRY_CACHE_LIMIT,
            ..key()
        };
        cache.get_or_insert_with(next, |_| GEOMETRY_CACHE_LIMIT);
        assert_eq!(cache.entries.len(), GEOMETRY_CACHE_LIMIT);
        assert!(cache.entries.contains_key(&recently_used));
        assert!(
            !cache
                .entries
                .contains_key(&GeometryKey { icon: 1, ..key() })
        );
    }
}
