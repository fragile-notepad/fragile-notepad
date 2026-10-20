[Documentation](../README.md)

# Vulkan rendering

The hardware renderer uses wgpu's Vulkan backend with portability support. It
retains settled surfaces and completed frames to reduce repeated painting while
keeping live input and animation responsive. The
[hybrid handoff](rendering.md#handoff-lifecycle) owns backend preparation and
rollback.

The application owns document, file, session, and analysis workflows. Ropey holds
text. `AdvancedEditor` lays out the visible viewport, including wrapping, folding,
decorations, and CJK font selection.

## Resource ownership

Pipelines, caches, workers, and buffers initialize on demand. Engine clones share
pipelines. Shader parameters use supported immediates and fall back to uniforms.

Widgets and recorded frames jointly retain custom resources. Temporary uploads
and replaced buffers remain alive while GPU work is pending; in-use glyphs remain
protected. Atlas allocations respect device limits and roll back failed
reservations. Full pools spill into additional pools or textures. Impossible
uploads return errors.

## Retained surfaces

`renderer::Cache` owns a clipped surface. `with_cached_layer` records its drawing
closure on a cache miss and composites the previous pixels on a hit. Callers must
change the paint key whenever content, styling, or animation changes. The GPU
renderer also tracks bounds, transforms, DPI, and loaded-font generations. The
software renderer uses the same API for ordinary clipped drawing.

| Property | Behavior |
| --- | --- |
| Budget | 32 MiB per window by default. Idle surfaces are evicted to make room. |
| Format | The main target's 8-bit color format, with premultiplied alpha. |
| Placement | Bounds align to physical pixels and use nearest sampling. |
| Ownership | Dropped ownership tokens are pruned on reset. |
| Shared resources | Cached children use the window's image atlas and upload worker. |
| Incomplete assets | Pending asynchronous images prevent reuse until painting completes. |
| Fallback | Unsupported formats, nested captures, and oversized surfaces draw live. |

Cold paints add offscreen passes to the existing encoder and submission. Warm
paints composite a textured triangle. Glyph and image cache generations advance
once after all child surfaces and the main scene have been prepared.

Surfaces invalidated on consecutive frames draw directly until their paint stamp
settles. The settled paint refreshes the surface once before subsequent hits reuse
its pixels. This avoids rebuilding offscreen textures during every scroll or fade
frame.

## Retained window frames

Each window retains its final composed frame in a separate surface bounded to
32 MiB. Scene changes produce a conservative damage rectangle; the renderer
clears and repaints that region. Unchanged frames reuse the completed pixels.

Presentation copies the entire owned frame into the acquired target. It uses a
texture copy when the surface supports one and replacement drawing otherwise.
The renderer never assumes that swapchain contents survive between frames.

DPI, dimensions, background, fonts, and pending image uploads force fresh
painting. Custom primitives and meshes remain conservatively dirty. Calls that
load an external target use ordinary rendering. Dirty tiles and GPU scroll
copying remain future work.

### When retention is bypassed

Retention is opportunistic. Sparse decorations draw directly because copying a
whole window can cost more than painting them. Damage covering more than half
the window also draws directly, avoiding an extra full-frame copy during rapid
scrolling or large fades.

Repeated viewport or background changes bypass retention while resizing or
transitioning. Broad live text and opaque primitives also bypass snapshot
construction. Once changes settle, a complete refresh restores valid retained
pixels. Renderer counters distinguish direct frames, full repaints, partial
repaints, and reuse. Offscreen pixel tests can disable this cost policy to
exercise retained paths at small dimensions.

## Application scheduling

Native movement and unhandled runtime events do not publish application messages
that rebuild every window's view. Identical physical sizes and effective DPI
preserve the native viewport version, avoiding redundant surface configuration
during dragging or duplicate resize notifications.

The workbench retains settled chrome and editor surfaces while forwarding input,
focus operations, IME, mouse interaction, and overlays. Child deadlines remain
independent of unrelated animation redraws. Application surfaces wait for one
unchanged redraw before capture, so continuously rebuilt scrolling or fading
views stay live.

Find, inline replace, function-list, About, and dirty-close transitions draw live
until their progress reaches the requested target, including the first opening
or closing frame. Locally driven entrances and reveals reuse child layout during
motion. Caret changes, scrolling, fold fades, and editor fast-text settling remain
live while their paint state changes.

## Editor and decorative work

Caret and IME positioning reuse shaped line geometry. Cache keys include content,
metrics, font generation, CJK selection, tab width, and scale. Growing the viewport
cache rehashes retained rows instead of dropping them. Its capacity follows the
peak viewport until the editor is dropped. Visible indentation and fold
decorations are indexed once per render, avoiding a full metadata scan for each
visible row.

The decorative field paints into a bounded 140 by 48 pixel texture for its 280 by
96 logical field, then composites at the requested size. At scale 1.5 this reduces
analytic shader evaluation from 420 by 144 to 140 by 48 pixels. Opacity changes
reuse the field. The native artwork image retains its original resolution.

## Design references

The retained-paint design follows
[Chromium's RenderingNG architecture](https://developer.chrome.com/docs/chromium/renderingng-architecture)
and its
[reuse of undamaged render passes](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/viz/service/display/direct_renderer.cc).
Compatible quad and text batching preserves ordering and clip boundaries, as
described in [Qt's scene graph renderer](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html).
Preserving completed pictures also follows
[WebRender's caching approach](https://firefox-source-docs.mozilla.org/gfx/RenderingOverview.html#caching).

## Related documentation

- [Hybrid rendering](rendering.md): policy, backend preparation, and rollback.
- [Test and profile rendering](../development/rendering.md): focused checks and dated measurement evidence.
- [Iced patch notes](../../vendor/iced/LOCAL_CHANGES.md): local framework changes.
