[Documentation](../README.md)

# Hybrid rendering

Fragile Notepad starts with the tiny-skia software renderer. Builds with the
default `hybrid-rendering` feature can prepare Vulkan in the background and switch
all windows to the hardware renderer after a validated handoff. Software startup
allocates no Vulkan resources. Building with `--no-default-features` excludes
Vulkan support.

## Rendering policy

The application resolves the saved hardware acceleration setting and
[runtime overrides](../reference/runtime-options.md#runtime-environment) before
requesting a handoff.

Hardware policies request a handoff when the main window opens. Opening About
also requests a handoff if a hardware policy is active and the renderer is still
in the software state. Requests wait until an active tab drag finishes. Duplicate
requests are suppressed. A failed handoff retains software and disables further
attempts for that application session. Selecting software does not reverse an
already active GPU handoff.

Startup disables antialiasing. Vulkan enables 4× MSAA for vector icons and uses
the saved VSync setting. All windows share the selected renderer backend.

## Theme transitions

The initial theme applies immediately as startup settings load. Later changes
between light and dark mode blend two live drawings of each window over 240 ms.
The root widget keeps one input tree and draws the complete scene,
including nested menus, in both modes. The dark drawing is composited over the
light drawing with group opacity. Reversing a transition starts from the current
color mix; a settled window draws once.

Software uses reusable offscreen pixels; Vulkan uses a reusable texture without
reading pixels back to the CPU. Syntax parsing supplies both color variants
together, and each mode keeps its own shaped-text cache. Widget redraw requests
advance the transition without rebuilding application state every frame.

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
