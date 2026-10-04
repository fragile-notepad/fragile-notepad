<img src="assets/illustrations/bunny/app.svg" align="right" width="96" alt="Bunny holding a notebook">

# Fragile Notepad

A desktop text editor for notes and source files. Written in Rust with
[Iced](https://iced.rs), for Windows and Linux.

[Releases](https://github.com/fragile-notepad/fragile-notepad/releases)
&nbsp;·&nbsp; [Development](DEVELOPMENT.md)
&nbsp;·&nbsp; [Architecture](ARCHITECTURE.md)

> **Generative AI Notice:** Generative AI was used throughout the development of this project.

## Build from source

Requires Git, stable [Rust](https://rustup.rs), Python 3.10+, and native build tools.
Patched dependencies are included.

```sh
git clone https://github.com/fragile-notepad/fragile-notepad.git
cd fragile-notepad
python -m pip install -r scripts/requirements-assets.txt
```

Generate assets on fresh checkouts and after SVG changes; outputs are ignored by Git.

**Windows — PowerShell**

```powershell
.\scripts\generate_icon_assets.ps1
cargo run --release --locked
```

**Linux**

```sh
bash scripts/generate_icon_assets.sh
cargo run --release --locked
```

The default build starts in software and can switch to Vulkan.

Append `--no-default-features` to Cargo commands for software-only builds.
Linux dependencies are listed in [CI](.github/workflows/ci.yml).
See [checks](DEVELOPMENT.md#checks-and-previews) and [packaging](PACKAGING.md).

## License

The project code is licensed under [BSD-3-Clause](LICENSE). Vendored dependencies
retain their upstream licenses; see [vendor provenance](vendor/README.md).
The original artwork in [`assets/icons/colored/`](assets/icons/colored/LICENSE)
and [`assets/illustrations/`](assets/illustrations/LICENSE),
including generated rasters and reproductions, is **all rights reserved** and
excluded from that license. Other bundled icons retain their MIT licenses;
see [Artwork notices](assets/icons/NOTICE.txt).
