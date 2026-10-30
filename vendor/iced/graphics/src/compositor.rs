//! A compositor is responsible for initializing a renderer and managing window
//! surfaces.
use crate::core::Color;
use crate::core::backend;
use crate::core::font;
use crate::core::renderer;
use crate::futures::{MaybeSend, MaybeSync};
use crate::{Shell, Viewport};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use thiserror::Error;

use std::borrow::Cow;
use std::fmt::Debug;

/// A graphics compositor that can draw to windows.
pub trait Compositor: Sized {
    /// The iced renderer of the backend.
    type Renderer;

    /// The surface of the backend.
    type Surface;

    /// Creates a new [`Compositor`] with the given [`backend::Settings`].
    fn new(
        settings: backend::Settings,
        display: impl Display + Clone,
        compatible_window: impl Window + Clone,
        shell: Shell,
    ) -> impl Future<Output = Result<Self, backend::Error>> + MaybeSend;

    /// Creates a [`Self::Renderer`] for the [`Compositor`].
    fn create_renderer(&self, settings: renderer::Settings) -> Self::Renderer;

    /// Crates a new [`Surface`] for the given window.
    ///
    /// [`Surface`]: Self::Surface
    fn create_surface(
        &mut self,
        window: impl Window + Clone,
        width: u32,
        height: u32,
    ) -> Self::Surface;

    /// Configures a new [`Surface`] with the given dimensions.
    ///
    /// [`Surface`]: Self::Surface
    fn configure_surface(&mut self, surface: &mut Self::Surface, width: u32, height: u32);

    /// Changes synchronization for subsequently configured window surfaces.
    /// Returns whether existing surfaces need to be configured again.
    /// Backends without a synchronization setting keep their native behavior.
    fn set_vsync(&mut self, _vsync: bool) -> bool {
        false
    }

    /// Returns [`Information`] used by this [`Compositor`].
    fn information(&self) -> Information;

    /// Loads a font from its bytes.
    fn load_font(&mut self, font: Cow<'static, [u8]>) -> Result<(), font::Error> {
        crate::text::font_system()
            .write()
            .expect("Write to font system")
            .load_font(font);

        // TODO: Error handling
        Ok(())
    }

    /// Lists all the available font families.
    fn list_fonts(&mut self) -> Result<Vec<font::Family>, font::Error> {
        use std::collections::BTreeSet;

        let font_system = crate::text::font_system()
            .read()
            .expect("Read from font system");

        let families = BTreeSet::from_iter(font_system.families());

        Ok(families.into_iter().map(font::Family::name).collect())
    }

    /// Presents the [`Renderer`] primitives to the next frame of the given [`Surface`].
    ///
    /// [`Renderer`]: Self::Renderer
    /// [`Surface`]: Self::Surface
    fn present(
        &mut self,
        renderer: &mut Self::Renderer,
        surface: &mut Self::Surface,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), SurfaceError>;

    /// Screenshots the current [`Renderer`] primitives to an offscreen texture, and returns the bytes of
    /// the texture ordered as `RGBA` in the `sRGB` color space.
    ///
    /// [`Renderer`]: Self::Renderer
    fn screenshot(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Vec<u8>;

    /// Warms up the current [`Renderer`] primitives using offscreen work and
    /// returns structured evidence that the submission completed.
    ///
    /// This method must not create or configure any visible window surface.
    ///
    /// [`Renderer`]: Self::Renderer
    fn warm_up_offscreen(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Result<OffscreenWarmUpEvidence, OffscreenWarmUpError>;

    /// Starts offscreen work without waiting for GPU completion.
    /// The renderer must retain its submission until it is polled or dropped.
    fn begin_warm_up_offscreen(
        &mut self,
        _renderer: &mut Self::Renderer,
        _viewport: &Viewport,
        _background_color: Color,
    ) -> Result<(), OffscreenWarmUpError> {
        Err(OffscreenWarmUpError::Unsupported)
    }

    /// Checks completion without blocking the event loop. `None` means pending.
    fn poll_warm_up_offscreen(
        &mut self,
        _renderer: &mut Self::Renderer,
    ) -> Result<Option<OffscreenWarmUpEvidence>, OffscreenWarmUpError> {
        Err(OffscreenWarmUpError::Unsupported)
    }
}

/// A window that can be used in a [`Compositor`].
///
/// This is just a convenient super trait of the `raw-window-handle`
/// traits.
pub trait Window: HasWindowHandle + Debug + MaybeSend + MaybeSync + 'static {}

impl<T> Window for T where T: HasWindowHandle + Debug + MaybeSend + MaybeSync + 'static {}

/// An owned display handle that can be used in a [`Compositor`].
///
/// This is just a convenient super trait of the `raw-window-handle`
/// trait.
pub trait Display: HasDisplayHandle + Debug + Send + Sync + 'static {}

impl<T> Display for T where T: HasDisplayHandle + Debug + Send + Sync + 'static {}

/// Defines the default compositor of a renderer.
pub trait Default {
    /// The compositor of the renderer.
    type Compositor: Compositor<Renderer = Self>;
}

/// Result of an unsuccessful call to [`Compositor::present`].
#[derive(Clone, PartialEq, Eq, Debug, Error)]
pub enum SurfaceError {
    /// A timeout was encountered while trying to acquire the next frame.
    #[error("A timeout was encountered while trying to acquire the next frame")]
    Timeout,
    /// The underlying surface has changed, and therefore the surface must be updated.
    #[error("The underlying surface has changed, and therefore the surface must be updated.")]
    Outdated,
    /// The swap chain has been lost and needs to be recreated.
    #[error("The surface has been lost and needs to be recreated")]
    Lost,
    /// There is no more memory left to allocate a new frame.
    #[error("There is no more memory left to allocate a new frame")]
    OutOfMemory,
    /// The surface is occluded and must not be drawn to.
    #[error("The surface is occluded and must not be drawn to")]
    Occluded,
    /// Acquiring a texture failed with a generic error.
    #[error("Acquiring a texture failed with a generic error")]
    Other,
}

/// Contains information about the graphics (e.g. graphics adapter, graphics backend).
#[derive(Debug)]
pub struct Information {
    /// Contains the graphics adapter.
    pub adapter: String,
    /// Contains the graphics backend.
    pub backend: String,
}

/// Evidence produced by a successful compositor offscreen warm-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffscreenWarmUpEvidence {
    /// The renderer family that produced the evidence.
    pub renderer_family: backend::RendererFamily,
    /// The graphics adapter associated with the warm-up, when available.
    pub adapter: Option<String>,
    /// The graphics backend associated with the warm-up, when available.
    pub backend: Option<String>,
    /// The physical width of the offscreen target.
    pub width: u32,
    /// The physical height of the offscreen target.
    pub height: u32,
    /// The number of render passes implied by the submitted offscreen work.
    pub passes: u32,
    /// The elapsed warm-up time in microseconds.
    pub elapsed_us: u128,
    /// Whether submitted work completion was observed before returning.
    pub submission_completed: bool,
}

/// Error produced by an unsuccessful compositor offscreen warm-up.
#[derive(Clone, PartialEq, Eq, Debug, Error)]
pub enum OffscreenWarmUpError {
    /// Offscreen warm-up is not supported by this compositor.
    #[error("offscreen warm-up is not supported by this compositor")]
    Unsupported,
    /// The viewport dimensions are invalid for offscreen warm-up.
    #[error("offscreen warm-up viewport dimensions are invalid")]
    InvalidDimensions,
    /// Offscreen warm-up failed.
    #[error("offscreen warm-up failed: {0}")]
    Failed(String),
    /// Offscreen warm-up timed out.
    #[error("offscreen warm-up timed out")]
    Timeout,
}

#[cfg(debug_assertions)]
impl Compositor for () {
    type Renderer = ();
    type Surface = ();

    async fn new(
        _settings: backend::Settings,
        _display: impl Display,
        _compatible_window: impl Window + Clone,
        _shell: Shell,
    ) -> Result<Self, backend::Error> {
        Ok(())
    }

    fn create_renderer(&self, _settings: renderer::Settings) -> Self::Renderer {}

    fn create_surface(
        &mut self,
        _window: impl Window + Clone,
        _width: u32,
        _height: u32,
    ) -> Self::Surface {
    }

    fn configure_surface(&mut self, _surface: &mut Self::Surface, _width: u32, _height: u32) {}

    fn load_font(&mut self, _font: Cow<'static, [u8]>) -> Result<(), font::Error> {
        Ok(())
    }

    fn list_fonts(&mut self) -> Result<Vec<font::Family>, font::Error> {
        Ok(Vec::new())
    }

    fn information(&self) -> Information {
        Information {
            adapter: String::from("Null Renderer"),
            backend: String::from("Null"),
        }
    }

    fn present(
        &mut self,
        _renderer: &mut Self::Renderer,
        _surface: &mut Self::Surface,
        _viewport: &Viewport,
        _background_color: Color,
        _on_pre_present: impl FnOnce(),
    ) -> Result<(), SurfaceError> {
        Ok(())
    }

    fn screenshot(
        &mut self,
        _renderer: &mut Self::Renderer,
        _viewport: &Viewport,
        _background_color: Color,
    ) -> Vec<u8> {
        vec![]
    }

    fn warm_up_offscreen(
        &mut self,
        _renderer: &mut Self::Renderer,
        _viewport: &Viewport,
        _background_color: Color,
    ) -> Result<OffscreenWarmUpEvidence, OffscreenWarmUpError> {
        Err(OffscreenWarmUpError::Unsupported)
    }
}

#[cfg(debug_assertions)]
impl Default for () {
    type Compositor = ();
}
