# Local encoding_rs changes

- Base revision: `229d34374bde30c8b9603a03654d7c308ade5df1`
- Upstream: https://github.com/hsivonen/encoding_rs
- License: [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), with
  [WHATWG data notices](LICENSE-WHATWG) and [copyright information](COPYRIGHT)

## OEM code pages

`src/lib.rs` exposes `oem`; `src/oem.rs` adds encoding/decoding, labels, and
tables for CP437, CP720, CP737, CP775, CP850, CP852, CP855, CP857, CP858,
CP860, CP861, CP862, CP863, CP865, CP866, and CP869.

## Backports

| Upstream commits | Change |
| --- | --- |
| `a074922` | Reserve complete two-byte/four-byte output before consuming legacy characters |
| `aa6c866`, `98f0e5a`, `36db69e`, `5063e28` | Initialize spare capacity before writing; update String/Vec lengths after conversion and assert capacity limits |
| `9451175` | Zero borrowed string destinations on unwinding; use `scopeguard` without default features to preserve no-std builds |
| `9cfe36a` | Annotate constructor output lifetimes to resolve compiler warnings |

## Compatibility and performance

The backports require Rust 1.60 for `Vec::spare_capacity_mut`. Portable
initialization fully zeros spare output capacity instead of using upstream's page-touching/assembly
optimization, costing O(spare capacity) work per call. Oversized reused buffers
add overhead; the application's loader sizes a fresh String for each chunk.

## Checks

Run the standalone development tests from the repository root:

```sh
cargo test --manifest-path vendor/encoding_rs/Cargo.toml --tests -j 1
```

## Related documentation

- [Documentation index](../../docs/README.md)
- [Testing](../../docs/development/testing.md)
- [Dependency update process](../README.md#updates)
