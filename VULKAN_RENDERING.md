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
