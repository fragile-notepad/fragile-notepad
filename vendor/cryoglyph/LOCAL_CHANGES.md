# Local Cryoglyph changes

Base: `53ba3e879539d19ed8162942126a977ec896cc3b`.
Upstream: https://github.com/iced-rs/cryoglyph.

## Backports

`f4e7e4eb84` replaces the per-frame glyph-use hash set with generation stamps.
Cache hits and newly rasterized glyphs stay protected until the next atlas trim;
the local changed-span uploads and pending-draw buffer lifetimes are preserved.

## Application patches

Glyph preparations reuse CPU vertices and upload changed spans. Empty draws and
atlas exhaustion invalidate draw state; lookups still mark atlas entries in use.
Buffer growth releases handles without destroying buffers retained by pending draws.

Run `cargo test --locked -p cryoglyph --lib` from the application root.
