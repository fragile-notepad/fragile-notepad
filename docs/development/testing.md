[Documentation](../README.md)

# Test and validate

Select checks for the area you changed. Complete [build setup](building.md)
before running checks that compile the editor or load generated assets.

Run one resource-intensive command at a time, including across Windows and WSL.
Use `-j 1` for Cargo builds and one test thread for graphics or font-intensive
tests. Reuse existing build and asset caches.

## Choose focused checks

Run commands from the repository root. Each command below checks a specific area;
choose the rows that apply to your change.

| Changed area | Check |
| --- | --- |
| Application Rust formatting | `cargo fmt --package fragile-notepad --check` |
| Document model | `cargo test --locked -j 1 --test core_document -- --test-threads=1` |
| Command-line parsing and instance forwarding | `cargo test --locked -j 1 --test cli -- --test-threads=1` |
| Outline configuration | `cargo test --locked -j 1 --test outline_configuration_regressions -- --test-threads=1` |
| Generated icons and illustrations | `python scripts/test_icon_assets.py` |
| Font tooling | `python -m unittest discover -s scripts -p 'test_*font_profiles.py'` |
| Software-only compilation | `cargo check --locked --no-default-features -j 1` |
| Rendering and local GPU patches | [Rendering checks](rendering.md#check-a-rendering-change) |
| Other local dependency patches | [Dependency update process](../../vendor/README.md#updates) |

Use `python3` instead of `python` on Linux and macOS. For another integration
test, use its filename from [tests/](../../tests) without the `.rs` suffix:

```sh
cargo test --locked -j 1 --test editor_history -- --test-threads=1
```

Documentation-only changes need link, anchor, path, and Markdown checks. They do
not require compiling the editor. Follow the [documentation review checklist](documentation.md#review-a-documentation-change).

## Run the full validation scripts

Use full validation when the scope of the change calls for it or before a release.
The scripts check formatting, test font tooling, prepare assets, run Clippy and
application tests, test Iced's GPU renderer and Cryoglyph, and check software-only
compilation. See [ci.ps1](../../scripts/ci.ps1) and [ci.sh](../../scripts/ci.sh)
for the exact commands.

Configure a [Vulkan adapter](rendering.md#prepare-a-vulkan-runtime) before running graphics
tests. On headless Linux, install `xvfb` and `xauth`; the Unix script uses
`xvfb-run` when available.

Windows PowerShell:

```powershell
$env:CARGO_BUILD_JOBS = '1'
$env:RUST_TEST_THREADS = '1'
.\scripts\ci.ps1
```

Linux:

```sh
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 bash scripts/ci.sh
```

On macOS, load the Vulkan environment and validation script in the same shell:

```sh
export CARGO_BUILD_JOBS=1
export RUST_TEST_THREADS=1
source scripts/setup-macos-vulkan.sh
source scripts/ci.sh
```

The [CI workflow](../../.github/workflows/ci.yml) runs validation on Windows,
Linux, and macOS. Its software Vulkan setup is documented in the
[rendering guide](rendering.md).

## Related topics

- [Generate assets](assets.md)
- [Test and profile rendering](rendering.md)
- [Contributing](../../CONTRIBUTING.md)
