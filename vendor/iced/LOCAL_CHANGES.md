# Local Iced changes

These patches adapt Iced to Fragile Notepad's
[rendering](../../docs/architecture/rendering.md) and window workflows. See the
[dependency update process](../README.md#updates) before updating the base.

- Base revision: `ddd7c42a9ba625b219e5e8062ff9be83eea467c5`
- Upstream: https://github.com/iced-rs/iced
- License: [MIT](LICENSE)

## Backports

| Upstream commits | Change |
| --- | --- |
| `b54f2c599`, `8caf9e44f` | Drop expired redraw deadlines |
| `8e05eade2`, `40339edb7` | Throttle surface-error recovery; preserve strict handoff rollback |
| `ca79fdb70` | Preserve events and clear layout when an overlay disappears |
| `7c6ce8789` | Preserve stronger mouse interaction across nested overlays |
| `3c81aac2e` | Retain scrollbar interaction across widget rebuilds |
| `d8dabb4ab` | Suppress content cursor while dragging a scrollbar |
| `79cba74a6`, `63a82addc`, `82acd61ea` | Update winit for native crashes, IME, Wayland input, and Windows `Sync` fixes |
| `bb22add15` | Notify logical window resize when application scale changes |
| `8d1955dc1` | Rebuild invalidated widgets during redraw even without messages |
| `fc2cfe937` | Keep toggler geometry in logical coordinates with crisp disabled |
| `f3ee4cfdc` | Preserve fractional scroll offsets until physical-pixel drawing |
| `9efcdf274` | Intersect nested scrollable update viewports with parent clipping |
| `3e309ef55` | Reset retained scrollable state when its widget ID changes |
| `6c9b87d37` | Probe image file contents when identifying its format |
| `044364c029` | Align CPU clipping with GPU snapping; preserve signed image bounds |
| `3542404b15` | Read gradient quad snap flags from their correct vertex offset |
| `ee455ef9bb` | Render gradient quad shadows with shared solid-quad shadow logic |
| `ba94c520a1` | Render single-stop gradients correctly |

## Continuous tab gestures

- Expose session-tagged `window::tab_drag` actions/events, native desktop hit
  testing, and pointer-following without entering an OS move loop.
- Retain the original native source and hand capture to a visible target through
  tab attachment; check
  physical release before handling delayed window-open tasks.
- Attach Wayland toplevels before first presentation when requested in window
  settings, using the existing winit connection and event queue.
- Recover from native creation failures with an explicit OpenFailed event and
  a session-tagged drag cancellation. Restore reachable window placement through
  `window::ensure_visible`, preserving negative desktop coordinates.
- Scope focus, selection, and scroll widget operations to one window with
  `Task::scope_window`, preserving explicit operation targets.

## Application patches

- Join adjacent compatible quad runs and text groups after layer merging,
  preserving transformation, clip, and cached-text ordering boundaries.
- Geometry caches identify their backend so a handoff cannot mix software and
  hardware geometry. Cached geometry owns its data.
- Compare cached mesh IDs, versions, and positions when finding frame damage.
  Changed meshes repaint their previous and current clipped bounds; meshes
  without a cache repaint within their bounds.
- Limit mesh MSAA passes to visible geometry inside the damaged region. Share
  scratch textures between sequential passes, including oversized textures
  while in use. Each pass keeps separate blit parameters; discard multisample
  contents after resolving them.
- Keep separate GPU mesh uploads when one cache appears at several positions.
  Remove unused transforms after painting and release uploads when their cache
  owner is dropped, including uploads held by other renderers. Advance index
  offsets when a mesh is clipped out.
- Provide explicit cached-layer tokens and paint keys, with a 32 MiB per-window GPU
  surface budget and ordinary clipped drawing on software. Cached surfaces
  share the window's image atlas and the engine's pipelines; nested caches,
  unsupported targets, and oversized allocations draw live.
- Custom primitives can prepare offscreen passes in the existing frame encoder.
- Retain a bounded final frame, repaint conservative scene damage, and copy
  completed pixels to each acquired target without relying on swapchain history.
  Sparse scenes and broad damage draw directly to avoid extra copy bandwidth.
  Broad live text and primitives skip unnecessary scene snapshot construction.
- Repeatedly invalidated surfaces paint live during scrolling and fades, then
  refresh once their paint keys and geometry settle.
- Software-first handoff with offscreen warm-up, presentation checks, and rollback.
- Bounded software damage/scroll matching, retained frames, and reusable image/clip caches.
- Vulkan portability, optional immediates, shared lazy resources, bounded atlases,
  and changed-span uploads.
- Windows opening-fade painting and synchronous resize viewport updates.
- Fractional image bounds through `Image::snap(false)` in both renderers.
- Rich paragraphs honor `Basic`, `Advanced`, and `Auto` shaping like plain text,
  preserving glyph positions when fold placeholders add color spans.
- Software rounded quads honor pixel-grid snapping after scaling and translation,
  keeping fold placeholder backgrounds inside their painted bounds.
- Clip negative text bitmap origins to prevent repeated edge pixels in software rendering.
