# Hybrid rendering

Default `hybrid-rendering` builds start in tiny-skia with Vulkan handoff available.
`--no-default-features` excludes Vulkan. Startup disables antialiasing and vsync.

## Policy

[rendering.rs](src/app/rendering.rs) resolves saved policy and
`FRAGILE_NOTEPAD_RENDER_BACKEND`:

| Value | Behavior |
| --- | --- |
| `software` | Suppress hardware requests |
| `lazy-gpu` | Prefer low-power Vulkan |
| `hardware-diagnostic` | Prefer high-performance Vulkan |

Valid overrides win; invalid values are ignored and shown in About debug info.
Saved hardware policies request a handoff when the main window opens; otherwise
opening About can trigger it. Keep `WGPU_BACKEND` unset or `vulkan`;
`WGPU_POWER_PREF=low|high|none` overrides adapter preference.
All windows share the renderer. Failed handoffs disable retries and retain
software. Software policy cannot reverse an active GPU handoff.

## Handoff

1. Prepare asynchronously and warm each live window offscreen while software draws.
2. After software presents successfully, install warmed renderers and configure
   surfaces, retaining software for rollback.
3. Require each window's first GPU presentation before its deadline. Release
   software on success; restore it on failure.

Warm-up has a deadline and retains resources through submission completion.
Duplicate requests are suppressed; window closure and resizing update participation
and dimensions. See [Vulkan resource ownership](VULKAN_RENDERING.md#resources).

## Checks and tracing

Alongside the [application checks](README.md#checks), renderer changes need:

```sh
cargo test --locked -p iced_graphics --lib
cargo test --locked -p iced_tiny_skia --lib --features iced_tiny_skia/image
cargo test --locked -p iced_winit -p iced_wgpu --lib
```

Set `FRAGILE_PERF_TRACE=1` and optionally `FRAGILE_PERF_TRACE_DIR` for shared CSV
traces. Use a fresh directory per capture; tracing adds formatting and I/O cost.
