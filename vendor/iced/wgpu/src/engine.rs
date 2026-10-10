use crate::graphics::{Antialiasing, Shell};
use crate::primitive;
use crate::quad;
use crate::text;
use crate::triangle;

use std::sync::{Arc, OnceLock, RwLock};

/// Enable a bounded amount of native immediate data when the adapter supports
/// it. Custom primitives can then encode small parameters directly in a pass.
pub(crate) fn immediate_size(adapter: &wgpu::Adapter) -> u32 {
    if adapter.features().contains(wgpu::Features::IMMEDIATES) {
        adapter.limits().max_immediate_size.min(128)
    } else {
        0
    }
}

#[derive(Clone)]
pub struct Engine {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) format: wgpu::TextureFormat,

    pub(crate) quad_pipeline: quad::Pipeline,
    pub(crate) text_pipeline: text::Pipeline,
    triangle_pipeline: Arc<OnceLock<triangle::Pipeline>>,
    raster_pipeline: Arc<OnceLock<crate::raster::Pipeline>>,
    antialiasing: Option<Antialiasing>,
    #[cfg(any(feature = "image", feature = "svg"))]
    image_pipeline: Arc<OnceLock<crate::image::Pipeline>>,
    #[cfg(any(feature = "image", feature = "svg"))]
    image_backend: wgpu::Backend,
    pub(crate) primitive_storage: Arc<RwLock<primitive::Storage>>,
    _shell: Shell,
}

impl Engine {
    pub fn new(
        _adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        antialiasing: Option<Antialiasing>,
        shell: Shell,
    ) -> Self {
        Self {
            format,

            quad_pipeline: quad::Pipeline::new(&device, format),
            text_pipeline: text::Pipeline::new(&device, &queue, format),
            triangle_pipeline: Arc::new(OnceLock::new()),
            raster_pipeline: Arc::new(OnceLock::new()),
            antialiasing,

            #[cfg(any(feature = "image", feature = "svg"))]
            image_pipeline: Arc::new(OnceLock::new()),
            #[cfg(any(feature = "image", feature = "svg"))]
            image_backend: _adapter.get_info().backend,

            primitive_storage: Arc::new(RwLock::new(primitive::Storage::default())),

            device,
            queue,
            _shell: shell,
        }
    }

    /// Mesh pipelines are shared across renderer clones and compiled only when
    /// a visible mesh is first prepared, including any optional MSAA pipeline.
    pub(crate) fn triangle_pipeline(&self) -> &triangle::Pipeline {
        self.triangle_pipeline
            .get_or_init(|| triangle::Pipeline::new(&self.device, self.format, self.antialiasing))
    }

    pub(crate) fn raster_pipeline(&self) -> &crate::raster::Pipeline {
        self.raster_pipeline
            .get_or_init(|| crate::raster::Pipeline::new(&self.device, self.format))
    }

    #[cfg(any(feature = "image", feature = "svg"))]
    pub(crate) fn image_pipeline(&self) -> &crate::image::Pipeline {
        self.image_pipeline.get_or_init(|| {
            crate::image::Pipeline::new(&self.device, self.format, self.image_backend)
        })
    }

    #[cfg(any(feature = "image", feature = "svg"))]
    pub fn create_image_cache(&self) -> crate::image::Cache {
        self.image_pipeline()
            .create_cache(&self.device, &self.queue, &self._shell)
    }

    pub fn trim(&mut self) {
        self.text_pipeline.trim();

        self.primitive_storage
            .write()
            .expect("primitive storage should be writable")
            .trim();
    }
}
