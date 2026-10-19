<p align="center">
  <picture>
    <source media="(max-width: 600px)" srcset="assets/illustrations/readme/banner-compact.svg">
    <img src="assets/illustrations/readme/banner.svg" width="100%" alt="Fragile Notepad — A little room for notes and code.">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/fragile-notepad/fragile-notepad/releases"><strong>Download</strong></a>
  &nbsp; · &nbsp; <a href="#build-from-source">Build</a>
  &nbsp; · &nbsp; <a href="#documentation">Documentation</a>
</p>

<p align="center">
  A desktop text editor for notes and source files, built with Rust and <a href="https://iced.rs">Iced</a>.<br>
  Windows · Linux · experimental macOS
</p>

**Windows** · Dark appearance

![Fragile Notepad running on Windows with Rust code and multiple tabs.](assets/screenshots/windows.png)

**macOS** · Light appearance

![Fragile Notepad running on macOS with Rust code and native window controls.](assets/screenshots/macos.png)

## Features

- Multiple windows with pinned, reorderable, and detachable tabs.
- Multiple carets, rectangular selections, word wrap, and undo/redo.
- Syntax highlighting, code folding, and function navigation.
- Find and replace with regular expressions across tabs and windows.
- Unicode, CJK text, legacy encodings, and configurable line endings.
- Session recovery for unsaved text, customizable shortcuts, and themes.

## Build from source

<details>
<summary>Requirements and commands</summary>

Requires Git, stable [Rust](https://rustup.rs), Python 3.10+, and native build tools;
Linux packages are listed in [CI](.github/workflows/ci.yml). Use `python3` on Unix.

```sh
git clone https://github.com/fragile-notepad/fragile-notepad.git
cd fragile-notepad
python -m pip install -r scripts/requirements-assets.txt -r scripts/requirements-font-profiles.txt
```

Generate assets for your platform:

| Platform | Command |
| --- | --- |
| Windows (PowerShell) | `.\scripts\generate_icon_assets.ps1` |
| Linux / macOS | `bash scripts/generate_icon_assets.sh` |

Then launch the editor:

```sh
cargo run --release --locked
```

The first build may download fonts. Regenerate assets after SVG changes.
Add `--no-default-features` for software-only rendering; see
[Vulkan setup](VULKAN_RENDERING.md#runtime-and-maintenance) for GPU requirements,
including macOS runtime setup.

</details>

## Checks

Run `.\scripts\ci.ps1` on Windows or `bash scripts/ci.sh` on Unix to generate assets,
check formatting, run tests, and check the software-only build.

## Documentation

- [Packaging and macOS app bundles](PACKAGING.md)
- [Optional file-manager integrations](components/README.md)
- [Rendering design and diagnostics](SEAMLESS_HYBRID_RENDERING.md)
- [Application architecture](src/app/README.md)

## License

Code uses [BSD-3-Clause](LICENSE); [vendored dependencies](vendor/README.md) retain
their upstream licenses. Original [colored icons](assets/icons/colored/LICENSE)
and [illustrations](assets/illustrations/LICENSE), including rasters and reproductions,
are **all rights reserved** and excluded from the code license.
Other bundled icons retain their MIT licenses; see [artwork notices](assets/icons/NOTICE.txt).

Generative AI was used throughout development.
