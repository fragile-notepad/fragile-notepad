# Packaging

Complete [build setup](README.md#build-from-source) and [checks](README.md#checks), then:

```sh
cargo build --release --locked
```

Output: `target/release/fragile-notepad.exe` on Windows or
`target/release/fragile-notepad` on Unix. Add `--no-default-features` for software-only builds.

Distribute the binary, `LICENSE`, `assets/icons/NOTICE.txt` as `ICON-NOTICES.txt`,
and the generated `target/release/FONT-NOTICES.txt` when nonempty.
Preserve dependency/runtime notices. Icons and syntax resources are embedded.

[The nightly workflow](.github/workflows/nightly.yml) defines targets and archives.

## macOS runtime

Install `molten-vk vulkan-loader vulkan-tools` with Homebrew, then:

```sh
mkdir -p dist
bash scripts/package-macos.sh dist/package
tar -C dist/package -czf dist/fragile-notepad-macos.tar.gz .
```

Use a new package directory and distribute it whole: launcher, binary,
Vulkan/MoltenVK libraries, ICD manifest, and licenses. This is a launcher package.

The script collects runtime notices online and ad-hoc signs libraries;
Developer ID signing and notarization are separate.
