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
