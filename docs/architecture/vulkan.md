[Documentation](../README.md)

# Vulkan rendering

The hardware renderer uses wgpu's Vulkan backend with portability support and
caches unchanged widget surfaces and completed frames. The
[hybrid handoff](rendering.md#handoff-lifecycle) owns backend preparation and
rollback.

## Resource ownership

Pipelines, caches, workers, and buffers initialize on demand. Engine clones share
pipelines. Shader parameters use immediate data when the device supports the
required feature and parameter size; otherwise they use uniform buffers.

Widgets and recorded frames jointly retain custom resources. Temporary uploads
and replaced buffers remain alive while GPU work is pending. Glyphs used in the
current frame are protected from eviction. Image atlas allocations respect
device limits and roll back failed reservations. Raster images can use dedicated
textures when shared image atlases run out of layers. Failed image allocations
report `OutOfMemory`; exhausted glyph atlases report `AtlasFull`.

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
painting. The renderer compares cached mesh IDs, versions, and positions before
reusing pixels. Changed meshes repaint their previous and current clipped bounds.
Meshes without a cache and custom primitives repaint within their bounds. Calls
that load an external target use ordinary rendering. Dirty tiles and GPU scroll
copying remain future work.

Icons reuse cached paths and tessellated meshes when they move. Each backend has
a limited-size geometry cache. Vulkan keeps separate uploads when one cached mesh
appears at several positions. Its 4× MSAA passes cover only visible geometry
inside the damaged region. Sequential passes share scratch textures and keep
separate shader parameters.

### When retention is bypassed

Sparse decorations draw directly because copying a whole window can cost more
than painting them. Damage covering more than half the window also draws
directly, avoiding an extra full-frame copy during rapid scrolling or large fades.

Repeated viewport or background changes bypass retention while resizing or
transitioning. Once these changes settle, a complete refresh restores valid
retained pixels. The planner also skips scene snapshots when live text, uncached
meshes, or custom primitives would dirty more than half the window.

Renderer counters distinguish direct frames, full repaints, partial repaints,
and reuse. Offscreen pixel tests can disable this cost policy to exercise
retained paths at small dimensions.

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
or closing frame. The `motion::popup`, `motion::dropdown`, and `motion::reveal`
wrappers reuse child layout as they animate positions or disclosure height.
Caret changes, scrolling, fold fades, and editor fast-text settling remain live
while their paint state changes.

## Editor and decorative work

Caret and IME positioning reuse shaped line geometry. Cache keys include content,
metrics, font generation, CJK selection, tab width, and scale. Growing the viewport
cache rehashes retained rows instead of dropping them. Its capacity follows the
peak viewport until the editor is dropped. Visible indentation and fold
decorations are indexed once per render, avoiding a full metadata scan for each
visible row.

For a 280 by 96 logical area, the About header's animated quill trail renders
into a 140 by 48 pixel texture, then composites at the requested size. At scale
1.5, this reduces shader evaluation from 420 by 144 to 140 by 48 pixels. Opacity
changes reuse the trail texture. The separate artwork images render at the
window's native DPI.

## Design references

The retained-paint design follows
[Chromium's RenderingNG architecture](https://developer.chrome.com/docs/chromium/renderingng-architecture)
and its
[reuse of undamaged render passes](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/viz/service/display/direct_renderer.cc).
Compatible quad and text batching preserves ordering and clip boundaries, as
described in [Qt's scene graph renderer](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html).
Preserving completed pictures also follows
[WebRender's caching approach](https://firefox-source-docs.mozilla.org/gfx/RenderingOverview.html#caching).
