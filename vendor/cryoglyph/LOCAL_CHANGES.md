# Local Cryoglyph changes

Base: `53ba3e879539d19ed8162942126a977ec896cc3b`.
Upstream: https://github.com/iced-rs/cryoglyph.

`f4e7e4eb84` protects cached and new glyphs with generation stamps until atlas trim.

Glyph preparations reuse CPU vertices and upload changed spans. Empty draws and
atlas exhaustion invalidate draw state; lookups still mark atlas entries in use.
Buffer growth releases handles without destroying buffers retained by pending draws.

Run `cargo test --locked -p cryoglyph --lib` from the application root.
