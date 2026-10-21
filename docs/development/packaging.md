[Documentation](../README.md)

# Package the editor

Create a portable release archive or a macOS application bundle. Complete
[build setup](building.md) and the relevant [validation](testing.md) first.
Run commands from the repository root.

## Build a release executable

```sh
cargo build --release --locked -j 1
```

| Platform | Output |
| --- | --- |
| Windows | `target/release/fragile-notepad.exe` |
| Linux or macOS | `target/release/fragile-notepad` |

Add `--no-default-features` for software-only builds. Icons, illustrations, syntax
resources, and prepared font data are embedded in the executable.

## Assemble a Windows or Linux archive

Place these files in a fresh package directory, then archive the directory:

| File | Archive path |
| --- | --- |
| Release executable | `fragile-notepad.exe` on Windows; `fragile-notepad` on Linux |
| [Code license](../../LICENSE) | `LICENSE` |
| [Artwork notices](../../assets/icons/NOTICE.txt) | `ICON-NOTICES.txt` |
| `target/release/FONT-NOTICES.txt`, when nonempty | `FONT-NOTICES.txt` |
| `target/font-profiles/prepared/licenses/`, when present with font notices | `font-licenses/` |

Preserve applicable dependency and runtime notices. The generated
`FONT-NOTICES.txt` belongs to that build and includes fallback font license text.
See [Vendored dependencies](../../vendor/README.md) for dependency provenance and
[Icons](../../assets/icons/README.md) for artwork terms.

The [nightly workflow](../../.github/workflows/nightly.yml) defines the release
targets, archive names, and exact copy and archive commands.

## Test an archive

With Python 3.12 or later and a graphical desktop session, run:

```sh
python scripts/check_package.py dist/fragile-notepad-windows-x64.zip
```

Use the archive for your platform. The script extracts it into temporary storage,
checks the CLI, and requires the editor to render a frame and exit within 60
seconds. Linux CI uses `xvfb-run -a`; macOS tests both launchers.

## Create a macOS package

The macOS packaging script creates a command-line launcher, a Finder application
bundle, and a relocatable Vulkan/MoltenVK runtime. It requires Homebrew, Xcode
Command Line Tools, a release executable, generated application icons, and network
access to collect runtime notices.

1. Install the Vulkan tools and runtime:

   ```sh
   brew install molten-vk vulkan-loader vulkan-tools
   ```

2. Package the build into a new directory:

   ```sh
   mkdir -p dist
   bash scripts/package-macos.sh dist/package
   ```

   The script refuses an existing package directory. The result includes
   `Fragile Notepad.app`, the `fragile-notepad` launcher, the editor binary,
   Vulkan and MoltenVK libraries, the ICD manifest, and licenses.

3. Create the archive:

   ```sh
   tar -C dist/package -czf dist/fragile-notepad-macos.tar.gz .
   ```

Distribute the complete package. Launch `Fragile Notepad.app` from Finder or use
the adjacent `fragile-notepad` launcher. Optional interface scaling is documented
under [Runtime environment](../reference/runtime-options.md#runtime-environment).

The script applies ad-hoc signatures to the bundled libraries and application.
Developer ID signing and notarization require separate release steps.

## Optional file-manager integrations

Build and install the [native file-manager integrations](../../components/README.md)
separately from the editor. Application archives exclude these components.
