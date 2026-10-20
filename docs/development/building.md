[Documentation](../README.md)

# Build from source

Build and run Fragile Notepad on Windows, Linux, or macOS. Run commands from the
repository root unless a step says otherwise.

## Prerequisites

Install Git, stable [Rust](https://rustup.rs), Python 3.10 or later with pip, and
the native tools for your platform.

| Platform | Native tools and libraries |
| --- | --- |
| Windows | Visual Studio Build Tools with the C++ build tools and Windows SDK for the MSVC Rust toolchain. |
| Linux | A C/C++ toolchain, pkg-config, Fontconfig, and X11/Wayland development libraries. See the Debian/Ubuntu example below. |
| macOS | Xcode Command Line Tools. Vulkan rendering also requires the loader and MoltenVK; see [Prepare a Vulkan runtime](rendering.md#prepare-a-vulkan-runtime). |

On Debian or Ubuntu, the windowing and Vulkan dependencies used by
[CI](../../.github/workflows/ci.yml) are:

```sh
sudo apt-get install --no-install-recommends \
  build-essential pkg-config fontconfig \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-dev libx11-xcb-dev libxcb1-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev \
  libvulkan1 mesa-vulkan-drivers vulkan-tools
```

Headless tests also use `xvfb` and `xauth`; see [Test and validate](testing.md).

## Prepare the checkout

1. Clone the repository and enter its directory:

   ```sh
   git clone https://github.com/fragile-notepad/fragile-notepad.git
   cd fragile-notepad
   ```

2. Install the asset and font preparation dependencies.

   Windows PowerShell:

   ```powershell
   python -m pip install -r scripts/requirements-assets.txt -r scripts/requirements-font-profiles.txt
   ```

   Linux or macOS:

   ```sh
   python3 -m pip install -r scripts/requirements-assets.txt -r scripts/requirements-font-profiles.txt
   ```

3. Generate the embedded artwork for your platform:

   | Platform | Command |
   | --- | --- |
   | Windows PowerShell | `.\scripts\generate_icon_assets.ps1` |
   | Linux or macOS | `bash scripts/generate_icon_assets.sh` |

   The scripts create raster assets under `assets/` and application icons under
   `target/app-icons/`.
   See [Generate assets](assets.md) for the source inventory and regeneration
   commands.

## Run the editor

```sh
cargo run --release --locked -j 1
```

Cargo prepares platform font profiles during the build. The first build may
download fallback fonts; subsequent builds reuse `target/font-profiles`.
Python is a build dependency and is not required to run a packaged editor.

The build produces `target/release/fragile-notepad.exe` on Windows or
`target/release/fragile-notepad` on Linux and macOS. To pass editor options through
Cargo, place them after `--`:

```sh
cargo run --release --locked -j 1 -- --no-session notes.txt
```

Use one Cargo build job and run one resource-intensive build or test command at a
time. Keep the existing `target/` cache when switching between focused checks.

## Build without Vulkan

Disable the default `hybrid-rendering` feature for a software-only executable:

```sh
cargo run --release --locked --no-default-features -j 1
```

Default builds start with software rendering and can switch to Vulkan according
to the saved rendering policy. See [Rendering architecture](../architecture/rendering.md)
for the handoff and [Runtime and build options](../reference/runtime-options.md)
for overrides and offline font preparation.

## Related topics

- [Test and validate](testing.md)
- [Package the editor](packaging.md)
- [Generate assets](assets.md)
