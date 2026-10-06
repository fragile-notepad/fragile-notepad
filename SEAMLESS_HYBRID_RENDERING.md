# Hybrid rendering

The default `hybrid-rendering` feature adds Vulkan to tiny-skia.
`--no-default-features` builds only software rendering. Startup always uses
`Backend::Software` with antialiasing and vsync disabled.

## Policy

[rendering.rs](src/app/rendering.rs) resolves saved policy and
`FRAGILE_NOTEPAD_RENDER_BACKEND`:

| Value | Behavior |
| --- | --- |
| `software` | Suppress hardware requests |
| `lazy-gpu` | Allow hardware handoff, preferring a low-power GPU |
| `hardware-diagnostic` | Allow diagnostic handoff, preferring a high-performance GPU |

Recognized overrides win; invalid values are ignored and shown in About debug
information. Loading saved lazy/diagnostic settings requests a boost when the main
window opens; without saved settings, opening About can trigger it.
Hardware requests select Vulkan. Keep `WGPU_BACKEND` unset or set to `vulkan`.
`WGPU_POWER_PREF=low|high|none` overrides the adapter preference for diagnostics.
The renderer is shared across windows and remains active after About closes.

States are Software → PreparingHardware → Hardware, or Failed.
Duplicate requests are suppressed. Failure retains software and suppresses retries
for the process. Changing policy to software does not switch an active GPU back.

## Handoff

`backend::prepare_warm_and_commit` reports `StrictHandoffOutcome` through
`Message::BackendBoostConfigured`. The implementation spans vendored Iced's
winit runtime, wgpu compositor, and fallback renderer.

1. Prepare a pending GPU compositor asynchronously while software stays active.
2. Draw each live window into a pending renderer and warm it offscreen.
   Completion polls use redraw deadlines; warm-up has a three-second timeout.
3. Retain warmed renderers until a successful software presentation.
4. Install them and configure visible surfaces, retaining software state for rollback.
5. Require each live window's first GPU presentation within three seconds.
   Success releases software resources; failure restores them.

Software continues drawing during preparation. Warm-up owns recorded primitives
and resources until submission completes. Closing or resizing windows updates
handoff participation and dimensions. Prepare, warm, commit, presentation,
cancellation, unsupported operations, and rollback failures have distinct outcomes.

## Software resources

Text scroll matching is bounded to 65,536 comparisons; exhausted searches redraw.
Damage merging bounds cases above 256 regions with a union. Zero-damage frames
share layer snapshots; opaque bounded scroll regions copy in place.

Linear-image resampling caches up to 128 entries and 4 Mi pixels.
Text clipping borrows full-width strips or reuses crop storage; clip masks reuse
identical bounds. Animated images use fractional bounds with `Image::snap(false)`.

About uses a shared pausable 60 Hz clock. Vulkan draws its trail procedurally;
software generates the field on the CPU. Closing, clipping, and focus loss stop
animation scheduling.

See [Vulkan resource ownership](VULKAN_RENDERING.md) and
[diagnostics and checks](DEVELOPMENT.md#renderer-diagnostics).
