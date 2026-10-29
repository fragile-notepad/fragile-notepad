//! Render a complete group to a reusable texture before applying its opacity.

use crate::core::{Color, Rectangle, Size, Transformation, renderer};
use crate::graphics::Viewport;
use crate::{Renderer, raster, raster_cache::Geometry};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct State {
    entries: Vec<Option<Entry>>,
    next: usize,
}

struct Entry {
    child: Box<Renderer>,
    paint: Paint,
}

struct Paint {
    target: Option<Arc<raster::Target>>,
    geometry: Option<Geometry>,
    scale: f32,
    owner: renderer::Cache,
    revision: u64,
    opacity: raster::Opacity,
    dirty: bool,
    ready: bool,
}

pub(super) struct Capture {
    parent: Box<Renderer>,
    paint: Paint,
    slot: usize,
    bounds: Rectangle,
}

impl State {
    pub(super) fn reset(&mut self) {
        self.next = 0;
    }

    pub(super) fn begin(renderer: &mut Renderer, bounds: Rectangle, opacity: f32) {
        let slot = renderer.opacity.next;
        renderer.opacity.next += 1;
        let transformation = renderer.layers.transformation();
        let parent_bounds = renderer.layers.current_mut().0.bounds;
        let scale = renderer.scale_factor.unwrap_or(1.0);
        let geometry = (bounds * transformation)
            .intersection(&parent_bounds)
            .and_then(|bounds| {
                Geometry::new(
                    bounds,
                    scale,
                    renderer.engine.device.limits().max_texture_dimension_2d,
                )
            });
        if slot == renderer.opacity.entries.len() {
            renderer.opacity.entries.push(None);
        }
        let entry = renderer.opacity.entries[slot].take();
        let (mut child, old_paint) = match entry {
            Some(entry) => (entry.child, Some(entry.paint)),
            None => (
                Box::new(Renderer::new(renderer.engine.clone(), renderer.settings)),
                None,
            ),
        };
        let target = geometry.map(|geometry| {
            old_paint
                .as_ref()
                .and_then(|paint| {
                    (paint.geometry.is_some_and(|old| old.size == geometry.size))
                        .then(|| paint.target.clone())
                        .flatten()
                })
                .unwrap_or_else(|| {
                    Arc::new(
                        renderer
                            .engine
                            .raster_pipeline()
                            .create_target(&renderer.engine.device, geometry.size),
                    )
                })
        });
        let paint = Paint {
            target,
            geometry,
            scale,
            owner: old_paint
                .as_ref()
                .map_or_else(renderer::Cache::new, |paint| paint.owner.clone()),
            revision: old_paint
                .as_ref()
                .map_or(1, |paint| paint.revision.wrapping_add(1)),
            opacity: renderer
                .engine
                .raster_pipeline()
                .create_opacity(&renderer.engine.device, opacity.clamp(0.0, 1.0)),
            dirty: true,
            ready: false,
        };
        let origin = geometry.map_or(Rectangle::with_size(Size::ZERO), |geometry| geometry.bounds);
        crate::core::Renderer::reset(&mut *child, Rectangle::with_size(origin.size()));
        child.scale_factor = Some(scale);
        #[cfg(any(feature = "image", feature = "svg"))]
        {
            child.image_cache = renderer.image_cache.clone();
        }
        child
            .layers
            .push_transformation(Transformation::translate(-origin.x, -origin.y) * transformation);
        child.layers.push_clip(bounds);
        std::mem::swap(renderer, &mut *child);
        renderer.opacity_parent = Some(Box::new(Capture {
            parent: child,
            paint,
            slot,
            bounds,
        }));
    }

    pub(super) fn end(renderer: &mut Renderer) {
        let mut capture = *renderer
            .opacity_parent
            .take()
            .expect("opacity layer must be started");
        renderer.layers.pop_clip();
        renderer.layers.pop_transformation();
        std::mem::swap(renderer, &mut *capture.parent);
        let child = capture.parent;
        // Append the composite after the parent's previous clipped children.
        renderer.layers.next_layer();
        if let (Some(target), Some(geometry)) = (&capture.paint.target, capture.paint.geometry) {
            // Give the result its own layer after every child of the group.
            renderer.layers.push_clip(capture.bounds);
            renderer
                .layers
                .current_mut()
                .0
                .rasters
                .push(raster::Instance {
                    target: target.clone(),
                    bounds: geometry.bounds,
                    _owner: capture.paint.owner.clone(),
                    revision: capture.paint.revision,
                    opacity: Some(capture.paint.opacity.clone()),
                });
            renderer.layers.pop_clip();
        }
        // Later parent primitives must also stay above the finished group.
        renderer.layers.next_layer();
        renderer.opacity.entries[capture.slot] = Some(Entry {
            child,
            paint: capture.paint,
        });
    }

    pub(super) fn prepare(&mut self, encoder: &mut wgpu::CommandEncoder) -> bool {
        // Drop groups as soon as this frame stops using them.
        self.entries.truncate(self.next);
        let mut ready = true;
        for entry in self.entries.iter_mut().flatten() {
            if entry.paint.dirty {
                if let (Some(target), Some(geometry)) = (&entry.paint.target, entry.paint.geometry)
                {
                    let viewport = Viewport::with_physical_size(geometry.size, entry.paint.scale);
                    entry.paint.ready = entry.child.encode(
                        encoder,
                        Some(Color::TRANSPARENT),
                        &target.view,
                        &viewport,
                    );
                } else {
                    entry.paint.ready = true;
                }
                entry.paint.dirty = false;
            }
            ready &= entry.paint.ready;
        }
        ready
    }

    pub(super) fn finish(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.child.finish();
        }
    }

    pub(super) fn recall(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.child.recall();
        }
    }
}
