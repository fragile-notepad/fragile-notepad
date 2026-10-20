[Documentation](../README.md)

# Generate application assets

Generate artwork before the first application build and after changing icon or
illustration SVGs. Prepare font profiles when inspecting calibration or priming
the build cache. The application embeds generated rasters and font data; the
platform icon files are also used for packaging. Run all commands from the
repository root.

## Requirements

- Python 3.10 or later. Use `python` on Windows and `python3` on Linux or macOS.
- Pillow and resvg-py, installed using the versions in
  [requirements-assets.txt](../../scripts/requirements-assets.txt).
- For the Rust preview example, the [application build prerequisites](building.md).

The [font preparation task](#prepare-font-profiles) has separate Python
dependencies and caching. Cargo also runs that preparation automatically.

## Generate all icons and illustrations

1. Install the asset dependencies:

   ```powershell
   python -m pip install -r scripts/requirements-assets.txt
   ```

   On Linux or macOS:

   ```sh
   python3 -m pip install -r scripts/requirements-assets.txt
   ```

2. Run the script for your platform:

   | Platform | Command |
   | --- | --- |
   | Windows PowerShell | `.\scripts\generate_icon_assets.ps1` |
   | Linux or macOS | `bash scripts/generate_icon_assets.sh` |

   The scripts generate colored icons, monochrome controls and shortcuts, the
   quill, bunny animation layers, and platform application icons. They do not
   process the file-type SVG collection or the README banners.

3. Check that the expected outputs exist:

   | Location | Contents |
   | --- | --- |
   | `assets/icons/{colored,heroicons,bootstrap}/rgba/` | 22 × 22 straight RGBA icons |
   | `assets/illustrations/macaw-quill.rgba` | 400 × 440 straight RGBA illustration |
   | `assets/illustrations/bunny/*.rgba` | Application, title-bar, and About animation rasters; [sizes and filenames](../../assets/illustrations/bunny/README.md#generated-assets) |
   | `target/app-icons/` | `app.ico`, `app.icns`, and `app.png` |
   | `target/animation-review/macaw-art-preview.png` | Quill review image |

   Generated files are ignored by Git. Keep the editable SVGs as the source of
   truth and regenerate assets in each build checkout.

## Regenerate a single family

Use `python3` instead of `python` on Linux or macOS.

| Family | Command |
| --- | --- |
| Colored action icons | `python scripts/generate_colored_rgba.py` |
| Heroicons controls | `python scripts/rasterize_svg_icons.py --svg-dir assets/icons/heroicons/svg --out-dir assets/icons/heroicons/rgba --size 22` |
| Bootstrap shortcuts and pins | `python scripts/rasterize_svg_icons.py --svg-dir assets/icons/bootstrap/svg --out-dir assets/icons/bootstrap/rgba --size 22` |
| Quill | `python scripts/rasterize_illustrations.py` |
| Bunny layers and platform application icons | `python scripts/generate_app_icons.py` |

The small-icon rasterizer defaults to 8× supersampling. Its monochrome output
uses `#64748b` unless overridden with `--color`; the colored-icon wrapper
preserves the SVG's fill and stroke colors. Follow the
[icon SVG contract](../../assets/icons/README.md#svg-contract) when editing
those sources. Bunny artwork uses resvg and supports gradients and transforms.

## Prepare font profiles

Cargo invokes [prepare_font_profiles.py](../../scripts/prepare_font_profiles.py)
automatically and reuses verified entries in `target/font-profiles/`. Manual
preparation is useful for inspecting generated data or populating that cache
before a build.

1. Install the font measurement dependencies:

   ```powershell
   python -m pip install -r scripts/requirements-font-profiles.txt
   ```

   Use `python3` on Linux or macOS. The requirements pin Pillow and fontTools.

2. Prepare profiles for the target platform. This example targets Windows:

   ```powershell
   python scripts/prepare_font_profiles.py --out-dir target/font-profiles/prepared --target-os windows --cache-dir target/font-profiles
   ```

   Set `--target-os` to `windows`, `linux`, or `macos`. Use `python3` on Linux
   or macOS. The script inventories installed fonts, measures the selected
   families, and downloads a pinned, checksum-verified Noto CJK fallback and
   license when needed. Downloads, measurement reports, and dependency caches
   stay under the cache directory.

3. Inspect the prepared outputs under `target/font-profiles/prepared/`:

   | Output | Purpose |
   | --- | --- |
   | `font_profiles.rs`, `font_assets.rs`, `font_families.rs`, `font_collections.rs` | Rust expressions for calibration, embedded font data, preferred families, and coherent collections |
   | `font_profiles_manifest.json` | Selected families, inputs, verified files, and cache provenance |
   | `rerun-paths.txt` | Font and source inputs that Cargo tracks for rebuilds |
   | `FONT-NOTICES.txt` | Bundled fallback-font notice and license text; empty when no fallback font is embedded |
   | `licenses/NotoSansCJK/OFL.txt` | Fallback-font license, when embedded |

During a build, Cargo writes the prepared expressions to its own `OUT_DIR` and
copies that build's `FONT-NOTICES.txt` beside the compiled artifact. Distribute
the nonempty notice with the corresponding executable; see
[Package the application](packaging.md).

Set `FRAGILE_FONT_OFFLINE=1` to prohibit downloads and dependency installation.
The required fonts and Python dependencies must already be installed or cached.
See the [build environment reference](../reference/runtime-options.md#build-environment)
for the Python executable and cache overrides.

## Review the branding animation

After generation, render the real About and title-bar widgets with one Cargo
build job:

```sh
cargo run --locked -j 1 --example preview_branding
```

Review outputs appear under `target/bunny-review/`. To use the Vulkan renderer,
run:

```sh
cargo run --locked -j 1 --example preview_branding -- --vulkan
```

Vulkan outputs appear under `target/bunny-review-vulkan/`. This mode requires
the default `hybrid-rendering` feature and a working Vulkan adapter. See
[Rendering setup and diagnostics](rendering.md) for runtime setup.

For focused automated checks of the sources and generated rasters, run:

```powershell
python -m unittest discover -s scripts -p test_icon_assets.py
```

Use `python3` on Linux or macOS. Generate the assets first, and run resource-heavy
previews or builds one at a time. See [Testing](testing.md) for the other checks.

## Related documentation

- [Icon inventory and notices](../../assets/icons/README.md)
- [Illustration inventory and rights](../../assets/illustrations/README.md)
- [Bunny sources and generated formats](../../assets/illustrations/bunny/README.md)
- [Package the application](packaging.md)
