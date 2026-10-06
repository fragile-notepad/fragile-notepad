# Development

See [README](README.md#build-from-source) for setup,
[Architecture](ARCHITECTURE.md) for ownership, and [Packaging](PACKAGING.md)
for distribution.

## Checks and previews

Run `scripts/ci.ps1` on Windows or `bash scripts/ci.sh` on Unix. These format-check
the application, generate assets, run Python and application tests, test
`iced_wgpu` and `cryoglyph`, and check `--no-default-features`.
Linux uses Xvfb when available; a Vulkan adapter is also required.
CI selects Lavapipe on Linux and SwiftShader on Windows using
`scripts/setup-ci-vulkan.*`.

On macOS, install `molten-vk vulkan-loader vulkan-tools` with Homebrew, then
source `scripts/setup-macos-vulkan.sh` and `scripts/ci.sh` in the same shell.
A new system shell may lose `DYLD_LIBRARY_PATH` through SIP.

For renderer changes, also run:

```sh
cargo test --locked -p iced_graphics --lib
cargo test --locked -p iced_tiny_skia --lib --features iced_tiny_skia/image
cargo test --locked -p iced_winit -p iced_wgpu --lib
```

`cargo run --locked --example preview_dialogs` writes widget previews to
`target/dialog-review/`. `cargo run --locked --example preview_branding` writes
About/title-bar previews to `target/bunny-review/`; add `-- --vulkan` for Vulkan.

Debug builds expose **About → Debug → Window controls** and accept
`FRAGILE_NOTEPAD_TITLE_BAR=macos|windows` to preview either title-bar style.
The macOS green control maximizes/restores; AppKit handles native resizing.

## Editor

`AdvancedEditor` translates input into `EditorAction`; menus and shortcuts share
handlers for history, clipboard, and selection.

- Double-click the gutter or column zero to select a logical line with its ending;
  double-click elsewhere selects a word.
- Selection drags beyond the editor edge autoscroll. Dragging highlighted text
  moves it on release in one undo step; Escape, focus loss, or an outside drop cancels.
- Right-click inside a selection preserves it. Context Menu or Shift+F10 opens
  the keyboard-navigable context menu.
- Cut Line and Delete Line remove all unique touched lines in one undo transaction.

Wrapping shares one fragment map across drawing, hit testing, navigation, IME,
and scrollbars. It preserves logical text, tab stops, and Unicode graphemes.
Caret affinity chooses a side of soft breaks. Resizing, zoom, and decoration
changes reflow; edits with unchanged line counts reuse unaffected measurements.

CJK routing uses cues from each logical line: kana, Hangul, and Chinese variant
forms identify regional fonts; neutral Han defaults to Simplified Chinese.
Mixed passages and paired Chinese variants retain their local regional forms.
Latin stays regular monospace. Cached logical runs survive wrapping and clipping,
and drawing, caret, selection, and IME geometry use the same fonts. Context
sampling is bounded to 1 MiB and 16,384 lines; visible text beyond the sample
still detects local kana/Hangul. Lines over 4 KiB use bounded column-based geometry.

Complete Noto/Source Han regional collections use Regular for consistent strokes.
Other families use generated optical weight profiles. Calibration compares ink
density and stroke estimates across several sizes; uncertain matches keep Regular.
The renderer verifies the chosen face and caches its coverage by font database
version. Unsupported symbols, marks, or variation selectors keep the whole
grapheme in that family's Regular face. Korean Hanja and historical Hangul
clusters retain Regular with its full shaping support.

Fresh Cargo builds and CI prepare profiles using `scripts/prepare_font_profiles.py`
and the Windows/Linux/macOS catalog in `scripts/font_families.py`. Missing regional
coverage or a usable reference triggers a pinned, checksum-verified
[Noto Sans CJK](https://github.com/notofonts/noto-cjk) download. Regional fallbacks
are embedded; reference-only downloads stay in the cache. Preparation reuses
matching inputs and writes generated profiles outside Git. Font notices accompany
the binary and distribution packages.

Build preparation requires Python 3 and installs missing calibration dependencies
in an isolated cache environment. `FRAGILE_FONT_PYTHON` selects Python,
`FRAGILE_FONT_CACHE` moves the cache, and `FRAGILE_FONT_OFFLINE=1` requires cached
dependencies and fonts. The application uses compiled profiles at runtime.

```sh
cargo run --locked --example preview_cjk
cargo run --locked --example preview_cjk -- --weights
cargo run --locked --example preview_cjk -- --hangul-weights
python -m unittest discover -s scripts -p 'test_*font_profiles.py'
```

Previews verify actual glyph/font IDs and write screenshots under `target/cjk-*`.
Add `--vulkan` for Vulkan. Weight previews compare optical balance, Hangul/Hanja,
and coverage-safe fallback at 16, 24, and 32 pixels. Fixtures live in
`tests/fixtures/cjk/`; CI also runs the generator, preparation, and catalog tests.
For independent calibration suggestions and measurement reports, install
`scripts/requirements-font-profiles.txt` and run
`python scripts/generate_font_profiles.py --help` to select fonts, references,
and ignored output paths.

## Files and sessions

```sh
fragile-notepad notes.txt src/main.rs
fragile-notepad -- -draft.txt
fragile-notepad --no-session notes.txt
```

Startup restores the session, then opens supplied paths. Relative paths resolve
against the caller's directory; already-open paths select their tab.
A second invocation forwards files and activates the existing window, succeeding
only after admission. During shutdown it returns a retry error.
`--help` and `--version` exit immediately. `--no-session` disables restoration
and writes for that launch; forwarded requests retain the running instance's policy.

Sessions retain tabs, pins, language, encoding, selections, scroll positions,
folds, and unsaved text. Clean tabs reopen lazily from disk. Dirty-tab closes
prompt Save/Discard/Cancel. Checkpoints debounce for two seconds and flush before
quit; a failed exit write keeps the app open. Invalid sessions and missing files
remain available for recovery. Limits are 256 MiB serialized data and 10,000 tabs.

Recent Files retains 16 paths separately from sessions. Settings writes debounce
for 250 ms and serialize the latest pending state. File reads allow four concurrent
workers; outline parsing allows two. Closing or superseding work cancels tasks.
Full syntax/fold/outline analysis is limited to 1 MiB decoded text.

Syntax prioritizes visible lines, then refines provisional colors with context
from the document start. Batches yield after 128 lines or about 4 ms. Displayed
colors survive edits until replacement; language/theme changes clear them.
Provisional and retained storage are each bounded to 1,024 lines.

| Platform | Configuration: `settings.xml`, `session.json` | Cache |
| --- | --- | --- |
| Windows | `%APPDATA%/FragileNotepad` | `%LOCALAPPDATA%/FragileNotepad/Cache`, falling back to `%APPDATA%/FragileNotepad/Cache` |
| Linux/macOS | `$XDG_CONFIG_HOME/fragile-notepad` or `$HOME/.config/fragile-notepad` | `$XDG_CACHE_HOME/fragile-notepad` or `$HOME/.cache/fragile-notepad` |

Paths are resolved in [paths.rs](src/platform/paths.rs). Unix sessions use
owner-only permissions.

## Vendored dependencies

[Provenance](vendor/README.md) lists revisions, licenses, and patches. Compare
upstream revisions, port selected changes into `vendor/`, update provenance,
and run application plus affected vendor tests with the application's lockfile.

## Renderer diagnostics

`FRAGILE_NOTEPAD_RENDER_BACKEND` accepts `software`, `lazy-gpu`, or
`hardware-diagnostic` and overrides saved policy. See
[Hybrid rendering](SEAMLESS_HYBRID_RENDERING.md) and [Vulkan](VULKAN_RENDERING.md).

Set `FRAGILE_PERF_TRACE=1` and optionally `FRAGILE_PERF_TRACE_DIR` for shared CSV
traces. Use a fresh directory per capture; tracing adds formatting and I/O cost.
