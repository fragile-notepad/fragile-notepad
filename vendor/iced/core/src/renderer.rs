//! Write your own renderer.
mod cache;

#[cfg(debug_assertions)]
mod null;

pub use cache::Cache;

use crate::image;
use crate::{
    Background, Border, Color, Font, Pixels, Rectangle, Shadow, Size, Transformation, Vector,
};

/// Whether anti-aliasing should be avoided by snapping primitive coordinates to the
/// pixel grid.
pub const CRISP: bool = cfg!(feature = "crisp");

/// A component that can be used by widgets to draw themselves on a screen.
pub trait Renderer {
    /// Starts recording a new layer.
    fn start_layer(&mut self, bounds: Rectangle);

    /// Ends recording a new layer.
    ///
    /// The new layer will clip its contents to the provided `bounds`.
    fn end_layer(&mut self);

    /// Draws the primitives recorded in the given closure in a new layer.
    ///
    /// The layer will clip its contents to the provided `bounds`.
    fn with_layer(&mut self, bounds: Rectangle, f: impl FnOnce(&mut Self)) {
        self.start_layer(bounds);
        f(self);
        self.end_layer();
    }

    /// Starts a clipped layer whose painted pixels may be cached.
    ///
    /// The caller must change `key` whenever anything affecting painting changes,
    /// including content and styling. Backends track layer size, transformation,
    /// and scale factor separately.
    ///
    /// Returns `true` when the caller must draw the layer, or `false` when the
    /// backend reuses retained texture pixels. Caching is opportunistic and
    /// bounded: a backend may decline to cache a layer or evict it at any time,
    /// requiring it to be drawn again even when `key` is unchanged.
    ///
    /// Cached pixels are composited into the current frame; this does not assume
    /// that swapchain contents are preserved or rely on swapchain damage tracking.
    ///
    /// The default implementation performs ordinary clipped drawing, including
    /// on software renderers. Every call must be paired with
    /// [`end_cached_layer`](Self::end_cached_layer), even when it returns `false`.
    fn start_cached_layer(&mut self, _cache: &Cache, _key: u64, bounds: Rectangle) -> bool {
        self.start_layer(bounds);
        true
    }

    /// Ends a layer started with [`start_cached_layer`](Self::start_cached_layer).
    ///
    /// This must be called whether the layer was drawn or reused cached pixels.
    /// The default implementation ends an ordinary clipped layer.
    fn end_cached_layer(&mut self) {
        self.end_layer();
    }

    /// Draws a clipped layer using `f` whenever its cached pixels cannot be reused.
    ///
    /// The caller must change `key` for every painting change, as described in
    /// [`start_cached_layer`](Self::start_cached_layer). The layer is ended both
    /// when `f` is called and when cached pixels are reused.
    fn with_cached_layer(
        &mut self,
        cache: &Cache,
        key: u64,
        bounds: Rectangle,
        f: impl FnOnce(&mut Self),
    ) {
        if self.start_cached_layer(cache, key, bounds) {
            f(self);
        }

        self.end_cached_layer();
    }

    /// Starts recording with a new [`Transformation`].
    fn start_transformation(&mut self, transformation: Transformation);

    /// Ends recording a new layer.
    ///
    /// The new layer will clip its contents to the provided `bounds`.
    fn end_transformation(&mut self);

    /// Applies a [`Transformation`] to the primitives recorded in the given closure.
    fn with_transformation(&mut self, transformation: Transformation, f: impl FnOnce(&mut Self)) {
        self.start_transformation(transformation);
        f(self);
        self.end_transformation();
    }

    /// Applies a translation to the primitives recorded in the given closure.
    fn with_translation(&mut self, translation: Vector, f: impl FnOnce(&mut Self)) {
        self.with_transformation(Transformation::translate(translation.x, translation.y), f);
    }

    /// Fills a [`Quad`] with the provided [`Background`].
    fn fill_quad(&mut self, quad: Quad, background: impl Into<Background>);

    /// Creates an [`image::Allocation`] for the given [`image::Handle`] and calls the given callback with it.
    fn allocate_image(
        &mut self,
        handle: &image::Handle,
        callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
    );

    /// Provides hints to the [`Renderer`] about the rendering target.
    ///
    /// This may be used internally by the [`Renderer`] to perform optimizations
    /// and/or improve rendering quality.
    ///
    /// For instance, providing a `scale_factor` may be used by some renderers to
    /// perform metrics hinting internally in physical coordinates while keeping
    /// layout coordinates logical and, therefore, maintain linearity.
    fn hint(&mut self, scale_factor: f32);

    /// Returns the last scale factor provided as a [`hint`](Self::hint).
    fn scale_factor(&self) -> Option<f32>;

    /// Resets the [`Renderer`] to start drawing in the `new_bounds` from scratch.
    fn reset(&mut self, new_bounds: Rectangle);

    /// Polls any concurrent computations that may be pending in the [`Renderer`].
    ///
    /// By default, it does nothing.
    fn tick(&mut self) {}
}

/// A polygon with four sides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// The bounds of the [`Quad`].
    pub bounds: Rectangle,

    /// The [`Border`] of the [`Quad`]. The border is drawn on the inside of the [`Quad`].
    pub border: Border,

    /// The [`Shadow`] of the [`Quad`].
    pub shadow: Shadow,

    /// Whether the [`Quad`] should be snapped to the pixel grid.
    pub snap: bool,
}

impl Default for Quad {
    fn default() -> Self {
        Self {
            bounds: Rectangle::with_size(Size::ZERO),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: CRISP,
        }
    }
}

/// The styling attributes of a [`Renderer`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The text color
    pub text_color: Color,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            text_color: Color::BLACK,
        }
    }
}

/// A headless renderer is a renderer that can render offscreen without
/// a window nor a compositor.
pub trait Headless {
    /// Creates a new [`Headless`] renderer;
    fn new(settings: Settings, backend: Option<&str>) -> impl Future<Output = Option<Self>>
    where
        Self: Sized;

    /// Returns the unique name of the renderer.
    ///
    /// This name may be used by testing libraries to uniquely identify
    /// snapshots.
    fn name(&self) -> String;

    /// Draws offscreen into a screenshot, returning a collection of
    /// bytes representing the rendered pixels in RGBA order.
    fn screenshot(
        &mut self,
        size: Size<u32>,
        scale_factor: f32,
        background_color: Color,
    ) -> Vec<u8>;
}

/// The settings of a [`Renderer`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// The default [`Font`] to use.
    pub default_font: Font,

    /// The default size of text.
    ///
    /// By default, it will be set to `16.0`.
    pub default_text_size: Pixels,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_font: Font::DEFAULT,
            default_text_size: Pixels(16.0),
        }
    }
}
