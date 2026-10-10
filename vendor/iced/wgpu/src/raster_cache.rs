//! Bounded, explicitly invalidated surfaces. Swapchain contents are never retained.
use crate::core::{Color, Rectangle, Size, Transformation, renderer::Cache};
use crate::graphics::Viewport;
use crate::{Renderer, layer, raster};
use std::collections::HashMap;
use std::sync::{Arc, Weak};

const DEFAULT_BUDGET: u64 = 32 * 1024 * 1024;

/// Cumulative retained-surface activity and current texture occupancy.
#[derive(Debug, Default, Clone, Copy)]
pub struct Statistics {
    pub hits: u64,
    pub misses: u64,
    pub live_fallbacks: u64,
    pub rasterizations: u64,
    pub bytes: u64,
    pub entries: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct Stamp {
    key: u64,
    transformation: Transformation,
    bounds: Rectangle,
    scale: f32,
    geometry: Geometry,
    fonts: crate::graphics::text::Version,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    bounds: Rectangle,
    size: Size<u32>,
}

impl Geometry {
    fn new(bounds: Rectangle, scale: f32, limit: u32) -> Option<Self> {
        if !scale.is_finite() || scale <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let x = (bounds.x * scale).floor();
        let y = (bounds.y * scale).floor();
        let width = ((bounds.x + bounds.width) * scale).ceil() - x;
        let height = ((bounds.y + bounds.height) * scale).ceil() - y;
        if ![x, y, width, height].into_iter().all(f32::is_finite)
            || width <= 0.0
            || height <= 0.0
            || width > limit as f32
            || height > limit as f32
        {
            return None;
        }
        Some(Self {
            bounds: Rectangle {
                x: x / scale,
                y: y / scale,
                width: width / scale,
                height: height / scale,
            },
            size: Size::new(width as u32, height as u32),
        })
    }

    fn bytes(self) -> u64 {
        u64::from(self.size.width) * u64::from(self.size.height) * 4
    }
}

pub(super) struct Entry {
    owner: Weak<()>,
    stamp: Stamp,
    pub(super) target: Arc<raster::Target>,
    pub(super) child: Box<Renderer>,
    pub(super) dirty: bool,
    valid: bool,
    revision: u64,
    used: u64,
}

enum Capture {
    Live,
    Hit,
    Miss {
        id: u64,
        owner: Cache,
        entry: Box<Entry>,
        parent: layer::Stack,
    },
}

pub(super) struct State {
    pub(super) entries: HashMap<u64, Entry>,
    captures: Vec<Capture>,
    frame: u64,
    budget: u64,
    statistics: Statistics,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            captures: Vec::new(),
            frame: 0,
            budget: DEFAULT_BUDGET,
            statistics: Statistics::default(),
        }
    }
}

impl State {
    pub(super) fn statistics(&self) -> Statistics {
        Statistics {
            bytes: self.bytes(),
            entries: self.entries.len(),
            ..self.statistics
        }
    }

    fn bytes(&self) -> u64 {
        self.entries.values().map(|entry| entry.target.bytes).sum()
    }

    pub(super) fn set_budget(&mut self, bytes: u64) {
        self.budget = bytes;
        // Drop old frames only: current instances may already reference their surfaces.
        let _ = self.make_room(0);
    }

    pub(super) fn reset(&mut self) {
        assert!(self.captures.is_empty(), "unbalanced cached layer");
        self.frame = self.frame.wrapping_add(1);
        self.entries
            .retain(|_, entry| entry.owner.strong_count() > 0);
        let _ = self.make_room(0);
    }

    fn make_room(&mut self, additional: u64) -> bool {
        // A request that cannot fit must not evict useful smaller surfaces.
        if additional > self.budget {
            return false;
        }
        while self.bytes().saturating_add(additional) > self.budget {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.used != self.frame)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(&id, _)| id);
            let Some(id) = oldest else {
                return false;
            };
            let _ = self.entries.remove(&id);
        }
        true
    }

    pub(super) fn begin(
        &mut self,
        renderer: &mut Renderer,
        owner: &Cache,
        key: u64,
        bounds: Rectangle,
    ) -> bool {
        let transformation = renderer.layers.transformation();
        let parent_bounds = renderer.layers.current_mut().0.bounds;
        let world = (bounds * transformation).intersection(&parent_bounds);
        renderer.layers.push_clip(bounds);
        let geometry = renderer.scale_factor.and_then(|scale| {
            Geometry::new(
                world?,
                scale,
                renderer.engine.device.limits().max_texture_dimension_2d,
            )
            .map(|geometry| (scale, geometry))
        });
        let supported = matches!(
            renderer.engine.format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let Some((scale, geometry)) = geometry.filter(|_| supported && self.captures.is_empty())
        else {
            return self.live();
        };
        let stamp = Stamp {
            key,
            transformation,
            bounds,
            scale,
            geometry,
            fonts: crate::graphics::text::font_system()
                .read()
                .expect("Read font system")
                .version(),
        };
        if let Some(entry) = self.entries.get_mut(&owner.id()) {
            if entry.stamp == stamp && entry.valid {
                entry.used = self.frame;
                renderer
                    .layers
                    .current_mut()
                    .0
                    .rasters
                    .push(raster::Instance {
                        target: entry.target.clone(),
                        bounds: geometry.bounds,
                        _owner: owner.clone(),
                        revision: entry.revision,
                    });
                self.statistics.hits += 1;
                self.captures.push(Capture::Hit);
                return false;
            }
            if entry.used == self.frame {
                // A single texture cannot hold two different paints in one submission.
                return self.live();
            }
        }
        let old = self.entries.remove(&owner.id());
        let mut entry = match old {
            Some(mut entry) if entry.stamp.geometry.size == geometry.size => {
                entry.stamp = stamp;
                entry
            }
            _ => {
                if !self.make_room(geometry.bytes()) {
                    return self.live();
                }
                Entry {
                    owner: owner.downgrade(),
                    stamp,
                    target: Arc::new(
                        renderer
                            .engine
                            .raster_pipeline()
                            .create_target(&renderer.engine.device, geometry.size),
                    ),
                    child: Box::new(Renderer::new(renderer.engine.clone(), renderer.settings)),
                    dirty: false,
                    valid: false,
                    revision: 0,
                    used: self.frame,
                }
            }
        };
        entry.used = self.frame;
        // Distinguish replacement textures as well as repaints of one texture.
        entry.revision = self.statistics.misses.wrapping_add(1);
        #[cfg(any(feature = "image", feature = "svg"))]
        {
            entry.child.image_cache = renderer.image_cache.clone();
        }
        entry.child.scale_factor = Some(scale);
        entry.child.layers.reset(Rectangle::with_size(Size::new(
            geometry.bounds.width,
            geometry.bounds.height,
        )));
        entry.child.layers.push_transformation(
            Transformation::translate(-geometry.bounds.x, -geometry.bounds.y) * transformation,
        );
        entry.child.layers.push_clip(bounds);
        let parent = std::mem::replace(
            &mut renderer.layers,
            std::mem::replace(&mut entry.child.layers, layer::Stack::new()),
        );
        self.statistics.misses += 1;
        self.captures.push(Capture::Miss {
            id: owner.id(),
            owner: owner.clone(),
            entry: Box::new(entry),
            parent,
        });
        true
    }

    fn live(&mut self) -> bool {
        self.statistics.live_fallbacks += 1;
        self.captures.push(Capture::Live);
        true
    }

    pub(super) fn end(&mut self, renderer: &mut Renderer) {
        match self.captures.pop().expect("cached layer must be started") {
            Capture::Miss {
                id,
                owner,
                mut entry,
                parent,
            } => {
                renderer.layers.pop_clip();
                renderer.layers.pop_transformation();
                entry.child.layers = std::mem::replace(&mut renderer.layers, parent);
                entry.dirty = true;
                entry.valid = true;
                renderer
                    .layers
                    .current_mut()
                    .0
                    .rasters
                    .push(raster::Instance {
                        target: entry.target.clone(),
                        bounds: entry.stamp.geometry.bounds,
                        _owner: owner,
                        revision: entry.revision,
                    });
                let _ = self.entries.insert(id, *entry);
            }
            Capture::Live | Capture::Hit => {}
        }
        renderer.layers.pop_clip();
    }

    pub(super) fn prepare(&mut self, encoder: &mut wgpu::CommandEncoder) -> bool {
        for entry in self.entries.values_mut().filter(|entry| entry.dirty) {
            let viewport =
                Viewport::with_physical_size(entry.stamp.geometry.size, entry.stamp.scale);
            entry.valid = entry.child.encode(
                encoder,
                Some(Color::TRANSPARENT),
                &entry.target.view,
                &viewport,
            );
            entry.dirty = false;
            self.statistics.rasterizations += 1;
        }
        self.entries
            .values()
            .filter(|entry| entry.used == self.frame)
            .all(|entry| entry.valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_bounds_cover_physical_pixels_without_shifting_content() {
        let bounds = Rectangle {
            x: 10.2,
            y: 3.1,
            width: 20.0,
            height: 9.0,
        };
        let geometry = Geometry::new(bounds, 1.5, 2048).unwrap();
        assert_eq!(geometry.size, Size::new(31, 15));
        assert_eq!(geometry.bounds.x, 10.0);
        assert_eq!(geometry.bounds.y, 4.0 / 1.5);
        assert_eq!(geometry.bytes(), 31 * 15 * 4);
    }

    #[test]
    fn invalid_or_oversized_surfaces_decline_allocation() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                Geometry::new(Rectangle::with_size(Size::new(10.0, 10.0)), scale, 2048).is_none()
            );
        }
        assert!(Geometry::new(Rectangle::with_size(Size::new(2049.0, 10.0)), 1.0, 2048).is_none());
        assert!(Geometry::new(Rectangle::with_size(Size::ZERO), 1.0, 2048).is_none());
    }
}
