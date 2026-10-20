[Documentation](../README.md)

# Runtime and build options

Look up command-line arguments, persistent file locations, and environment
variables used by the editor and its build tooling.

## Command line

```text
fragile-notepad [OPTIONS] [--] [FILE ...]
```

| Argument | Behavior |
| --- | --- |
| `FILE ...` | Open one or more paths. Relative paths resolve from the launching process's working directory. |
| `--no-session` | Start without restoring the previous session. Applies when starting the primary instance. |
| `-h`, `--help` | Print usage and exit without opening a window. |
| `-V`, `--version` | Print the version and exit without opening a window. |
| `--` | Treat all remaining arguments as paths, including names that begin with `-`. |

For example, with the executable on `PATH`:

```sh
fragile-notepad notes.txt src/main.rs
fragile-notepad --no-session
fragile-notepad -- -draft.txt
```

Unknown options and empty path arguments produce errors. A second process
forwards its file paths to the running instance and exits; it does not start an
independent session. Parsing is implemented in [startup.rs](../../src/startup.rs),
and process forwarding is wired in [main.rs](../../src/main.rs).

## Settings and session files

Change preferences through **Settings > Preferences...**. Settings use
`settings.xml`; session recovery uses `session.json` in the same configuration
directory.

| Platform | Configuration directory |
| --- | --- |
| Windows | `%APPDATA%\FragileNotepad\` |
| Linux and macOS | `$XDG_CONFIG_HOME/fragile-notepad/` when set; otherwise `$HOME/.config/fragile-notepad/` |

The compiled outline cache is `outline-registry.xml` in the cache directory:

| Platform | Cache directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\FragileNotepad\Cache\`; falls back to `%APPDATA%\FragileNotepad\Cache\` |
| Linux and macOS | `$XDG_CACHE_HOME/fragile-notepad/` when set; otherwise `$HOME/.cache/fragile-notepad/` |

macOS uses the Unix paths above. See [platform paths](../../src/platform/paths.rs),
[settings persistence](../../src/services/settings_store.rs), and
[session persistence](../../src/services/session_store.rs) for the file formats
and path resolution.

## Runtime environment

Set variables before launching the primary instance. Values passed to a secondary
process do not reconfigure the running editor.

| Variable | Values and behavior |
| --- | --- |
| `FRAGILE_NOTEPAD_RENDER_BACKEND` | `software`, `lazy-gpu`, or `hardware-diagnostic`. A valid value overrides the saved rendering policy; an invalid value is ignored and reported in About debug information. |
| `FRAGILE_NOTEPAD_UI_SCALE` | Additional interface scale from `0.75` through `3.0`, inclusive. Defaults to `1.0` for unset, invalid, nonfinite, or out-of-range values. The operating system's display scale still applies. |
| `FRAGILE_NOTEPAD_OUTLINE_CACHE_PATH` | Override the full path to the compiled outline cache. |
| `WGPU_BACKEND` | Leave unset or set to `vulkan` for this application's hardware renderer. |
| `WGPU_POWER_PREF` | `low`, `high`, or `none`; overrides the adapter power preference. |

Rendering policy values map to these requests:

| Value | Request |
| --- | --- |
| `software` | Suppress hardware handoff requests. |
| `lazy-gpu` | Prefer a low-power Vulkan adapter. |
| `hardware-diagnostic` | Prefer a high-performance Vulkan adapter. |

The default saved setting is **Hybrid rendering**. The editor starts in software
and requests a hardware handoff when a hardware policy is active. A software-only
build excludes Vulkan regardless of environment overrides. See
[Rendering architecture](../architecture/rendering.md) for failure handling and
the handoff lifecycle.

For example, force software rendering for a new instance in PowerShell:

```powershell
$env:FRAGILE_NOTEPAD_RENDER_BACKEND = 'software'
.\fragile-notepad.exe
```

On Linux or macOS:

```sh
FRAGILE_NOTEPAD_RENDER_BACKEND=software ./fragile-notepad
```

## Diagnostic environment

These variables support development and profiling.

| Variable | Behavior |
| --- | --- |
| `FRAGILE_PERF_TRACE` | Presence enables CSV tracing when the output file can be opened; use `1` by convention. |
| `FRAGILE_PERF_TRACE_DIR` | Trace directory; defaults to `target/perf`. Use a fresh directory per capture. |
| `FRAGILE_NOTEPAD_STARTUP_PROBE` | Presence enables startup timing output, bypasses session restoration, and uses a process-specific instance identity. |
| `FRAGILE_NOTEPAD_TITLE_BAR` | Debug builds only: `windows` or `macos` selects the window control style. |

See [Test and profile rendering](../development/rendering.md) for trace capture,
adapter selection, and profiling commands. The profiling examples also describe
their own workload-specific options in
[profile_composition.rs](../../examples/profile_composition.rs) and
[profile_scroll.rs](../../examples/profile_scroll.rs).

## Build environment

| Variable | Behavior |
| --- | --- |
| `FRAGILE_FONT_PYTHON` | Python executable used by `build.rs`; defaults to `python` on Windows and `python3` elsewhere. |
| `FRAGILE_FONT_CACHE` | Font preparation cache path; defaults to `target/font-profiles` under the repository root. |
| `FRAGILE_FONT_OFFLINE` | Set to `1` to disable network access and dependency installation during font preparation. Required fonts and Python tooling must be installed or available in verified caches. |
| `CARGO_BUILD_JOBS` | Use `1` on a shared development machine to limit concurrent compilation. |
| `RUST_TEST_THREADS` | Use `1` for graphics and font-intensive test runs, including full validation scripts. |

Font preparation is invoked by [build.rs](../../build.rs); the preparation
script is [prepare_font_profiles.py](../../scripts/prepare_font_profiles.py).

## Related topics

- [Get started](../getting-started.md)
- [Build from source](../development/building.md)
- [Generate assets](../development/assets.md)
