[Documentation](../README.md)

# Hybrid rendering

Fragile Notepad starts with the tiny-skia software renderer. Builds with the
default `hybrid-rendering` feature can prepare Vulkan in the background and switch
all windows to the hardware renderer after a validated handoff. Software startup
allocates no Vulkan resources. Building with `--no-default-features` excludes
Vulkan support.

## Rendering policy

[The application rendering module](../../src/app/rendering.rs) resolves the saved
hardware acceleration setting and the `FRAGILE_NOTEPAD_RENDER_BACKEND` override.

| Policy value | Behavior |
| --- | --- |
| `software` | Suppress hardware requests. |
| `lazy-gpu` | Request Vulkan with a low-power adapter preference. |
| `hardware-diagnostic` | Request Vulkan with a high-performance adapter preference. |

A valid environment override takes precedence over the saved setting. Invalid
values are ignored and reported in About debug information. See
[runtime options](../reference/runtime-options.md) for environment variables and
adapter overrides.

Hardware policies request a handoff when the main window opens. Opening About
also requests a handoff if a hardware policy is active and the renderer is still
in the software state. Requests wait until an active tab drag finishes. Duplicate
requests are suppressed. A failed handoff retains software and disables further
attempts for that application session. Selecting software does not reverse an
already active GPU handoff.

Startup disables antialiasing and vsync. The Vulkan handoff enables vsync and
4× multisample antialiasing for vector icon meshes. Text, images, and quads use
their existing rendering paths. All windows share the selected renderer backend.

## Handoff lifecycle

The runtime keeps software rendering available until every participating window
has presented successfully with Vulkan.

1. Prepare the hardware backend asynchronously and warm each live window
   offscreen while software continues to draw.
2. After a successful software presentation, install the warmed renderers and
   configure their surfaces. Retain the software backend for rollback.
3. Require each window's first GPU presentation before its deadline. Release
   software after success; restore it if the handoff fails.

Warm-up has a deadline and retains resources until submitted GPU work completes.
Window closure and resizing update the participating windows and their
dimensions. The [Vulkan resource model](vulkan.md#resource-ownership) describes
how the renderer keeps submitted resources alive.

## Implementation map

| Responsibility | Source |
| --- | --- |
| Application policy and handoff outcome | [`src/app/rendering.rs`](../../src/app/rendering.rs) |
| Initial backend settings | [`src/startup.rs`](../../src/startup.rs) |
| Runtime handoff, first presentation, and rollback | [`vendor/iced/winit/src/lib.rs`](../../vendor/iced/winit/src/lib.rs) |
| Local framework changes | [Iced patch notes](../../vendor/iced/LOCAL_CHANGES.md) |
