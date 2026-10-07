<img src="assets/illustrations/bunny/app.svg" align="right" width="96" alt="Bunny holding a notebook">

# Fragile Notepad

A desktop text editor for notes and source files. Written in Rust with
[Iced](https://iced.rs), for Windows and Linux.

[Releases](https://github.com/fragile-notepad/fragile-notepad/releases)
&nbsp;·&nbsp; [Packaging](PACKAGING.md)

> **Generative AI Notice:** Generative AI was used throughout the development of this project.

## Build from source

Requires Git, stable [Rust](https://rustup.rs), Python 3.10+, and native build tools;
Linux packages are listed in [CI](.github/workflows/ci.yml). Patched dependencies are included.

```sh
git clone https://github.com/fragile-notepad/fragile-notepad.git
cd fragile-notepad
python -m pip install -r scripts/requirements-assets.txt
```

Generate assets with `.\scripts\generate_icon_assets.ps1` on Windows or
`bash scripts/generate_icon_assets.sh` on Unix, then:

```sh
cargo run --release --locked
```

Rerun asset generation after SVG changes; outputs are ignored by Git.
Cargo prepares CJK font profiles and may download fonts and Python dependencies.
`FRAGILE_FONT_PYTHON` selects Python, `FRAGILE_FONT_CACHE` changes the cache
(default `target/font-profiles`), and `FRAGILE_FONT_OFFLINE=1` requires cached inputs.

Rendering starts in software and can switch to [Vulkan](SEAMLESS_HYBRID_RENDERING.md).
Add `--no-default-features` to Cargo commands for software-only builds.

## Checks

Run `.\scripts\ci.ps1` on Windows or `bash scripts/ci.sh` on Unix for asset generation,
formatting, tests, and the software-only build check. See [Vulkan setup](VULKAN_RENDERING.md#runtime-and-maintenance)
and [additional renderer checks](SEAMLESS_HYBRID_RENDERING.md#checks-and-tracing).
Application ownership and routing are documented [beside the code](src/app/README.md).

## Previews

Run `cargo run --locked --example NAME`:

| NAME | Output |
| --- | --- |
| `preview_dialogs` | `target/dialog-review/` |
| `preview_branding` | `target/bunny-review/` |
| `preview_cjk` | `target/cjk-*` |

Branding and CJK previews accept `-- --vulkan`; CJK also accepts `-- --weights`
or `-- --hangul-weights`. Debug builds expose **About → Debug → Window controls**;
`FRAGILE_NOTEPAD_TITLE_BAR=macos|windows` selects a title-bar style.

## License

Code uses [BSD-3-Clause](LICENSE); [vendored dependencies](vendor/README.md) retain
their upstream licenses. Original [colored icons](assets/icons/colored/LICENSE)
and [illustrations](assets/illustrations/LICENSE), including rasters and reproductions,
are **all rights reserved** and excluded from the code license.
Other bundled icons retain their MIT licenses; see [artwork notices](assets/icons/NOTICE.txt).
