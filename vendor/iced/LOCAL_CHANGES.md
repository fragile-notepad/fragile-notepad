# Local Iced changes

Base: `ddd7c42a9ba625b219e5e8062ff9be83eea467c5`.
Upstream: https://github.com/iced-rs/iced. License: [MIT](LICENSE).

## Backports

| Upstream commits | Change |
| --- | --- |
| `b54f2c599`, `8caf9e44f` | Drop expired redraw deadlines |
| `8e05eade2`, `40339edb7` | Throttle surface-error recovery; preserve strict handoff rollback |
| `ca79fdb70` | Preserve events and clear layout when an overlay disappears |
| `7c6ce8789` | Preserve stronger mouse interaction across nested overlays |
| `3c81aac2e` | Retain scrollbar interaction across widget rebuilds |
| `d8dabb4ab` | Suppress content cursor while dragging a scrollbar |
| `79cba74a6`, `63a82addc`, `82acd61ea` | Update the winit fork to `48116469f15d3bc52b5e56bb516694be8882870d` and align `smol_str` to 0.3; includes native IME/crash fixes, Wayland wheel and repeated-key handling, and Windows `Sync` restoration |
| `bb22add15` | Notify logical window resize when application scale changes |
| `8d1955dc1` | Rebuild invalidated widgets during redraw even without messages |
| `fc2cfe937` | Keep toggler geometry in logical coordinates with crisp disabled |
| `f3ee4cfdc` | Preserve fractional scroll offsets until physical-pixel drawing |
| `9efcdf274` | Intersect nested scrollable update viewports with parent clipping |
| `3e309ef55` | Reset retained scrollable state when its widget ID changes |
| `6c9b87d37` | Probe image file contents when identifying its format |
| `044364c029` | Align CPU clips and GPU quad edges using the same pixel nudge; preserve signed image bounds and snap(false) |
| `3542404b15` | Read gradient quad snap flags from their correct vertex offset |

## Application patches

- Software-first handoff with offscreen warm-up, presentation checks, and rollback.
- Bounded software damage/scroll matching, retained frames, and reusable image/clip caches.
- Vulkan portability, optional immediates, shared lazy resources, bounded atlases,
  and changed-span uploads.
- Windows opening-fade painting and synchronous resize viewport updates.
- Fractional image bounds through `Image::snap(false)` in both renderers.

Details: [hybrid rendering](../../SEAMLESS_HYBRID_RENDERING.md),
[Vulkan resources](../../VULKAN_RENDERING.md),
[update process](../README.md#updates).
