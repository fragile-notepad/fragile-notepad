# Vulkan rendering

Hardware uses wgpu Vulkan with portability support. Software startup allocates
no Vulkan resources; the [hybrid handoff](SEAMLESS_HYBRID_RENDERING.md#handoff)
owns preparation and rollback.

The application owns document, file, session, and analysis workflows. Ropey holds
text; `AdvancedEditor` lays out the visible viewport with wrapping, folding,
decorations, and CJK font selection. The initial GPU investigation found that
warm glyph and instance uploads were already retained, but every requested redraw
still repainted the whole scene. A captured static editor drew 1,854 glyph
instances across 15 draws; About drew 2,124 across 24, including the unchanged
editor underneath it. This made paint reuse and bounded damage the main targets.

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
Surfaces invalidated on consecutive frames paint directly until their stamp
settles. This avoids rebuilding offscreen textures on every scroll or fade frame;
the settled paint refreshes once before later hits reuse its pixels.

This follows [Chromium's retained rendering model](https://developer.chrome.com/docs/chromium/renderingng-architecture)
and its [reuse of undamaged render passes](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/viz/service/display/direct_renderer.cc).
Compatible quad/text batching follows the ordering and clip boundaries described
by [Qt's scene graph renderer](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html).
Preserving completed pictures also follows
[WebRender's caching approach](https://firefox-source-docs.mozilla.org/gfx/RenderingOverview.html#caching).
Each window also retains its final composed frame in a separate, bounded 32 MiB
surface. Scene changes produce a conservative damage rectangle; only that region
is cleared and repainted. Unchanged frames reuse the completed pixels. Presentation
copies the whole owned frame into the acquired target, using a texture copy when
the surface supports it and replacement drawing otherwise. Swapchain history is
never assumed. DPI, dimensions, background, fonts, and pending image uploads force
fresh painting. Custom primitives and meshes remain conservatively dirty. Calls
that load an external target keep ordinary rendering. Dirty tiles and GPU scroll
copying remain future work.

Retention is opportunistic. Sparse decorations render directly; copying a whole
window would cost more than their paint. Damage covering more than half the window
also renders directly, avoiding an extra full-frame copy during rapid scrolling or
large fades. Repeated viewport/background changes also bypass retention while
resizing or transitioning. Broad live text and opaque primitives bypass snapshot
construction too. Once changes settle, a complete refresh restores valid retained
pixels. Renderer counters expose direct frames alongside full/partial repaints
and reuse. Offscreen pixel tests can disable this cost policy to exercise every
retained path at small dimensions.

## Application scheduling and editor work

Native movement and unhandled runtime events no longer publish application
messages that rebuild every window's view. Identical physical sizes and effective
DPI also preserve the native viewport version, avoiding redundant surface
configuration while dragging or receiving duplicate resize notifications.

The workbench retains settled chrome and editor surfaces while forwarding input,
focus operations, IME, mouse interaction, and overlays. Child deadlines stay
independent of unrelated animation redraws. App surfaces wait for one unchanged
redraw before capture, keeping continuously rebuilt scrolling/fading views live.
Find, inline replace, function-list,
About, and dirty-close transitions draw live until their progress reaches the
requested target, including their first opening/closing frame. Locally driven
entrances and reveals reuse child layout during motion; caret, scrolling, fold
fades, and editor fast-text settling remain live when their paint state changes.

Caret and IME positioning reuse shaped line geometry, keyed by content, metrics,
font generation, CJK selection, tab width, and scale. Growing the viewport cache
rehashes retained rows instead of dropping them; its capacity follows the peak
viewport until the editor is dropped. Visible indentation and fold decorations
are indexed once per render, avoiding a full metadata scan for each visible row.

The decorative field paints into a bounded texture (140 x 48 pixels for the
280 x 96 logical field), then composites at the requested size. At 1.5 scale this
cuts analytic shader evaluation from 420 x 144 to 140 x 48 pixels. Opacity-only
changes reuse that field; the native artwork image keeps its original resolution.

## Focused profiling

The headless probe retains the real runtime widget tree and layout between
redraws, rebuilding only for messages or widget invalidation:

```sh
cargo run --offline -j 1 --example profile_composition -- --compare --frames 48
```

It reports CPU update/record/submit and Vulkan timestamp median/p95, paint-cache
counters, actual editor wheel messages, and fade toggles. `--case` selects a
workload; `--fixture` accepts a UTF-8 editor file; `--width`, `--height`, and
`--scale` vary the viewport. The default fixture is embedded from `src/app.rs`.
`--compare` runs each uncached/cached pair consecutively on one device;
`--uncached` disables both paint-cache budgets while retaining batching, the
bounded decorative field, and editor optimizations.

This probe forces redraws at at most 60 Hz, waits for timestamp readback outside
CPU timing, and excludes queued atlas uploads, swapchain presentation, desktop
composition, and background task execution. It does not measure OS GPU utilization
or native window-drag latency. Fade clocks use real elapsed time, so opacity
progress and first-time shaping can vary with machine load. Run it serially with
other resource-intensive checks on the user's development machine.

The 2026-10-10 check used an AMD Radeon 610M, Vulkan driver 25.10.36.11, a dev
build, 1536 x 1152 physical pixels at scale 1.5, and 48 measured frames after
8 warm-up frames. Its UTF-8 Japanese CSV fixture contained 600 lines / 47,689
bytes, BLAKE3 `740dda4cfdebba80e01dc2f0eeba8d2b22086206f6dea5b89a360be37147d5c7`.
Each cell below is **uncached -> cached**, in milliseconds; these are paint-cache
comparisons on the optimized code, not total before/after application timings.

| Workload | GPU median | CPU median | GPU p95 | CPU p95 |
| --- | ---: | ---: | ---: | ---: |
| Repeated editor paint | 1.082 -> 0.911 | 2.864 -> 0.456 | 1.494 -> 1.329 | 3.387 -> 0.860 |
| Settled About with live effects | 1.522 -> 1.244 | 3.465 -> 0.937 | 2.013 -> 1.570 | 4.497 -> 1.419 |
| Isolated decorative field | 0.489 -> 0.480 | 0.610 -> 0.667 | 0.776 -> 0.807 | 1.039 -> 1.030 |
| Rapid editor scroll reversals | 0.850 -> 0.909 | 4.217 -> 4.303 | 1.624 -> 1.556 | 41.447 -> 24.040 |
| Repeated dialog fade reversals | 1.573 -> 1.347 | 3.859 -> 4.305 | 2.173 -> 2.545 | 52.734 -> 57.814 |

The repeated editor reused all 48 completed frames without repainting. The live
effects case repainted 6,074,918 pixels instead of 84,934,656 full-frame pixels
(about 7.2%), with 240 child-surface hits and no offscreen recaptures. Rapid
scrolling sent 48 real editor wheel actions, reversing every six frames; all
48 root frames rendered directly and no child surfaces were recaptured. The fade
case performed five close/open toggles and also avoided child recaptures.

Clock/load variation was visible across runs, including the isolated field
control where both modes take the same drawing path. The scroll median and fade
CPU/p95 results do not establish a speedup. First-time shaping and view rebuilding
still contribute long editor/transition frames; GPU scroll copying and damage
tiles remain unimplemented. The reproducible probe and pixel/event tests provide
the next profiling points without claiming native presentation or drag results.

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
