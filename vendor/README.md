# Vendored dependencies

Fragile Notepad builds against the local dependency sources listed below. This
index records their provenance, licenses, and application-specific patches.

| Dependency | Upstream | Base revision or version | License |
| --- | --- | --- | --- |
| Iced | https://github.com/iced-rs/iced | `ddd7c42a9ba625b219e5e8062ff9be83eea467c5` | [MIT](iced/LICENSE) |
| winit | https://github.com/iced-rs/winit | `48116469f15d3bc52b5e56bb516694be8882870d` | [Apache-2.0](winit/LICENSE) |
| Cryoglyph | https://github.com/iced-rs/cryoglyph | `53ba3e879539d19ed8162942126a977ec896cc3b` | [MIT](cryoglyph/LICENSE-MIT), [Apache-2.0](cryoglyph/LICENSE-APACHE), or [Zlib](cryoglyph/LICENSE-ZLIB) |
| encoding_rs | https://github.com/hsivonen/encoding_rs | `229d34374bde30c8b9603a03654d7c308ade5df1` | [MIT](encoding_rs/LICENSE-MIT) or [Apache-2.0](encoding_rs/LICENSE-APACHE), with [WHATWG data notices](encoding_rs/LICENSE-WHATWG) |
| cosmic-text | https://github.com/pop-os/cosmic-text | Package version `0.19.0`; no pinned base revision recorded | [MIT](cosmic-text/LICENSE-MIT) or [Apache-2.0](cosmic-text/LICENSE-APACHE) |

Additional notices: [Druid layout code](iced/core/src/layout/DRUID_LICENSE)
and [encoding_rs copyright](encoding_rs/COPYRIGHT).

## Local patches

- [Iced](iced/LOCAL_CHANGES.md): rendering, widgets, and continuous tab gestures.
- [winit](winit/LOCAL_CHANGES.md): native tab gestures and window lifecycle.
- [Cryoglyph](cryoglyph/LOCAL_CHANGES.md): glyph atlas lifetime and uploads.
- [encoding_rs](encoding_rs/LOCAL_CHANGES.md): OEM code pages and conversion safety.
- [cosmic-text](cosmic-text/PATCHES.md): font optical-size overrides.

Iced and encoding_rs are direct path dependencies. Iced selects the local winit
and Cryoglyph sources. The root [`Cargo.toml`](../Cargo.toml) selects cosmic-text
through `[patch.crates-io]`.

## Updates

1. Compare candidate upstream changes with the recorded base and local patches.
2. Port selected fixes and update the affected patch notes and provenance.
3. Run the relevant [application checks](../docs/development/testing.md) and
   affected Iced or Cryoglyph tests with `--locked` and `-j 1`. Use
   `--test-threads=1` for graphics tests. Run [encoding_rs checks](encoding_rs/LOCAL_CHANGES.md#checks)
   with its standalone manifest.

Run one build or test command at a time. See [rendering development](../docs/development/rendering.md)
for graphics checks.

## Related documentation

- [Documentation index](../docs/README.md)
- [Rendering architecture](../docs/architecture/rendering.md)
- [Vulkan renderer](../docs/architecture/vulkan.md)
