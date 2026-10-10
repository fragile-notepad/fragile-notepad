# Vulkan rendering

Hardware uses wgpu Vulkan with portability support. Software startup allocates
no Vulkan resources; the [hybrid handoff](SEAMLESS_HYBRID_RENDERING.md#handoff)
owns preparation and rollback.

## Resources

- Pipelines, caches, workers, and buffers initialize on demand; engine clones
  share pipelines. Parameters use supported immediates, otherwise uniforms.
- Widgets and recorded frames jointly retain custom resources. Temporary uploads
  and replaced buffers survive pending GPU work; in-use glyphs remain protected.
- Atlas allocations respect device limits and roll back failed reservations.
  Full pools spill to additional pools or textures; impossible uploads report errors.

## Retained painting

`renderer::Cache` owns a clipped surface; `with_cached_layer` records its closure
on a miss and composites the previous pixels on a hit. The caller must change
the paint key for content, styling, and animation changes. The GPU renderer also
tracks bounds, transforms, DPI, and loaded-font generations. Software performs
ordinary clipped drawing through the same API.

The default surface budget is 32 MiB per window. Surfaces use the main target's
8-bit color format and premultiplied alpha, with physical-pixel-aligned bounds
and nearest sampling. Other formats, nested captures, and oversized surfaces
draw live. Idle surfaces are evicted to make room; dropped ownership tokens are
pruned on reset. Cached children share the window's image atlas and upload
worker. Pending asynchronous images prevent reuse until a complete paint.
Cold paints add offscreen passes to the existing encoder and submission;
warm paints need only a textured triangle. Glyph and image cache generations
advance once after all child surfaces and the main scene are prepared.

This follows [Chromium's retained rendering model](https://developer.chrome.com/docs/chromium/renderingng-architecture)
and its [reuse of undamaged render passes](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/viz/service/display/direct_renderer.cc).
Compatible quad/text batching follows the ordering and clip boundaries described
by [Qt's scene graph renderer](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html).
Each window also retains its final composed frame in a separate, bounded 32 MiB
surface. Scene changes produce a conservative damage rectangle; only that region
is cleared and repainted. Unchanged frames reuse the completed pixels. Presentation
copies the whole owned frame into the acquired target, using a texture copy when
the surface supports it and replacement drawing otherwise. Swapchain history is
never assumed. DPI, dimensions, background, fonts, and pending image uploads force
fresh painting. Custom primitives and meshes remain conservatively dirty. Calls
that load an external target keep ordinary rendering. Dirty tiles and GPU scroll
copying remain future work.

## Runtime and maintenance

Windows uses the graphics driver's Vulkan runtime. Linux needs a Vulkan loader
and driver. macOS uses the loader and MoltenVK packaged by
[scripts/package-macos.sh](scripts/package-macos.sh).

Renderer tests require a Vulkan adapter. CI selects Lavapipe on Linux and
SwiftShader on Windows using `scripts/setup-ci-vulkan.*`.
For macOS checks, install `molten-vk vulkan-loader vulkan-tools` with Homebrew,
then source `scripts/setup-macos-vulkan.sh` and `scripts/ci.sh` in the same shell;
a new system shell may lose `DYLD_LIBRARY_PATH` through SIP.

See [application checks](README.md#checks),
[renderer checks and tracing](SEAMLESS_HYBRID_RENDERING.md#checks-and-tracing),
[packaging](PACKAGING.md), and [Iced patches](vendor/iced/LOCAL_CHANGES.md).
