# Local winit changes

These patches support continuous tab gestures and native window lifecycle events.

- Base revision: `48116469f15d3bc52b5e56bb516694be8882870d`
- Upstream: https://github.com/iced-rs/winit
- License: [Apache-2.0](LICENSE)

## Tab gestures

- Add a cancellable tab-gesture pointer API. Windows retains native pointer
  capture and samples the physical button without a blocking move loop or
  synthetic mouse release. X11 grabs an unconfined pointer using its existing
  connection. macOS tracks AppKit pointer/button state on the GUI event loop.
- Native hit testing respects window stacking and can exclude a followed
  window to permit attachment beneath it. Desktop conversion handles each
  target's scale factor and Cocoa's screen-coordinate convention.
- Wayland binds the data-device manager and optionally
  `xdg_toplevel_drag_manager_v1` on the existing queue/connection. Source press
  serials and seats originate from winit pointer input. Session-tagged native
  events report surface entry, motion, leave, drop, and cancellation.
- Toplevel attachment can be requested before first presentation. Compositors
  without the extension retain data-device transfer between existing windows;
  the application offers an explicit Move to New Window fallback.
- Delay the compositor data-device request until the existing press moves ten
  logical pixels. A tiny shared-memory tab preview remains visible on fallback
  compositors. Native source terminal events govern release and cancellation.
- Implement Wayland hide/unmap and configure-gated show/remap, suppressing hidden
  redraws so docking releases the protocol attachment before another tear-off.

## Window lifecycle

- Forward macOS Dock reopening through the application handler and Iced event
  subscription, allowing an editor daemon to reopen after the last window closes.

## Source preservation

No upstream source or license files were omitted; `.git` metadata is excluded.

## Related documentation

- [Documentation index](../../docs/README.md)
- [Local Iced changes](../iced/LOCAL_CHANGES.md)
- [Dependency update process](../README.md#updates)
