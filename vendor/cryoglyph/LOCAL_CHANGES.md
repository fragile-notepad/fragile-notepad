# Local Cryoglyph changes

These patches preserve glyph resources across cached draws and reduce uploads.

- Base revision: `53ba3e879539d19ed8162942126a977ec896cc3b`
- Upstream: https://github.com/iced-rs/cryoglyph
- License: [MIT](LICENSE-MIT), [Apache-2.0](LICENSE-APACHE), or [Zlib](LICENSE-ZLIB)

## Atlas lifetime and uploads

`f4e7e4eb84` protects cached and new glyphs with generation stamps until atlas trim.

Glyph preparations reuse CPU vertices and upload changed spans. Empty draws and
atlas exhaustion invalidate draw state; lookups still mark atlas entries in use.
Buffer growth releases handles without destroying buffers retained by pending draws.

## Checks

Run the library tests from the application root:

```sh
cargo test --locked -p cryoglyph --lib -j 1 -- --test-threads=1
```

## Related documentation

- [Documentation index](../../docs/README.md)
- [Vulkan renderer](../../docs/architecture/vulkan.md)
- [Rendering development](../../docs/development/rendering.md)
- [Dependency update process](../README.md#updates)
