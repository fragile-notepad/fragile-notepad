[Documentation](../README.md)

# Package the editor

Complete [build setup](building.md) and the relevant [checks](testing.md).
Run commands from the repository root.

## Installer formats

Install CMake 3.24 or later with CPack and the tools below. The
[configuration](../../scripts/packaging/CMakeLists.txt) packages a Cargo release build.

| Platform | Format | Additional packaging tools |
| --- | --- | --- |
| Windows x64 | NSIS `.exe` | NSIS 3.03 or later |
| Debian and Ubuntu | `.deb` | `dpkg-dev` |
| Fedora and openSUSE | `.rpm` | `rpm-build` |
| macOS, Apple Silicon or Intel | `.dmg` | Xcode Command Line Tools, Homebrew Vulkan runtime |

## Build a release executable

Windows Developer PowerShell, including the MSVC C runtime:

```powershell
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --locked -j 1
Remove-Item Env:RUSTFLAGS
```

Linux or macOS:

```sh
cargo build --release --locked -j 1
```

The executable is `target/release/fragile-notepad` (`.exe` on Windows).
Add `--no-default-features` for software-only builds. Assets are embedded.

## Build an installer

On macOS, [prepare the application](#create-a-macos-package) before running CPack.

```sh
cmake -S scripts/packaging -B target/installer
cpack --config target/installer/CPackConfig.cmake -B dist
```

Outputs go to `dist/`, named by Cargo version, platform, and architecture.
Linux creates DEB and RPM; add `-G DEB` or `-G RPM` to select one.
Windows setup requires administrator access. The macOS image contains the app
and an **Applications** shortcut.

Build Linux on the oldest distribution you intend to support; packages retain
its glibc requirements. File dialogs need the desktop's portal backend.

### Default applications and document icons

Windows and macOS register the [supported extensions](../../assets/icons/file-types/catalog.json)
with their document icons. Choose Fragile Notepad as the default application in
the OS settings; installation keeps existing defaults.

Linux registers text and source MIME types for **Open With**. Document icons
follow the desktop's MIME theme, independently of the default app.

## Assemble a Windows or Linux archive

Place these files in a fresh package directory, then archive the directory:

| File | Archive path |
| --- | --- |
| Release executable | `fragile-notepad.exe` on Windows; `fragile-notepad` on Linux |
| [Code license](../../LICENSE) | `LICENSE` |
| [Artwork notices](../../assets/icons/NOTICE.txt) | `ICON-NOTICES.txt` |
| [File-type artwork notices](../../assets/icons/file-types/LICENSE) | `FILE-TYPE-ICON-NOTICES.txt` |
| `target/release/FONT-NOTICES.txt`, when nonempty | `FONT-NOTICES.txt` |
| `target/font-profiles/prepared/licenses/`, when present with font notices | `font-licenses/` |

Preserve applicable dependency and runtime notices. The generated
`FONT-NOTICES.txt` belongs to that build and includes fallback font license text.
See [Vendored dependencies](../../vendor/README.md) for dependency provenance and
[Icons](../../assets/icons/README.md) for artwork terms.

The [nightly workflow](../../.github/workflows/nightly.yml) defines the release
targets, archive names, and exact copy and archive commands.

## Test a package

Use Python 3.12 or later and a graphical desktop session (`python3` on Unix).
Check an archive, macOS disk image, or installed executable:

```sh
python scripts/check_package.py dist/fragile-notepad-windows-x64.zip
python3 scripts/check_package.py dist/fragile-notepad-0.1.0-macos-arm64.dmg
python scripts/check_package.py --executable /path/to/fragile-notepad
```

Use `fragile-notepad.exe` on Windows. The checker uses isolated settings, checks
the CLI, and requires a frame within 60 seconds. Headless Linux needs `xvfb-run -a`.

## Create a macOS package

The script packages the app, CLI launcher, and Vulkan/MoltenVK runtime.
It needs a release build, generated icons, Homebrew, Xcode Command Line Tools,
and network access for runtime notices.

1. Install the Vulkan tools and runtime:

   ```sh
   brew install molten-vk vulkan-loader vulkan-tools
   ```

2. Package the build into a new directory:

   ```sh
   mkdir -p dist
   bash scripts/package-macos.sh dist/package
   ```

   The script refuses an existing directory and includes the runtime and licenses.

3. Create a disk image using the [installer commands](#build-an-installer), or
   create a portable archive:

   ```sh
   tar -C dist/package -czf dist/fragile-notepad-macos-arm64.tar.gz .
   ```

Use `x64` in the archive filename for Intel. Distribute the complete archive or
the DMG. Launch `Fragile Notepad.app` or the archive's `fragile-notepad` launcher.
See [Runtime environment](../reference/runtime-options.md#runtime-environment) for scaling.

Windows installers and macOS disk images are unsigned. macOS apps use ad-hoc
signatures and are not notarized, so downloaded apps can trigger OS warnings.

## Optional file-manager integrations

Build and install the [native file-manager integrations](../../components/README.md)
separately from the editor. Application archives exclude these components.
