# Local encoding_rs changes

[Upstream](https://github.com/hsivonen/encoding_rs) revision:
`229d34374bde30c8b9603a03654d7c308ade5df1`.

`src/lib.rs` exposes `oem`; `src/oem.rs` adds encoding/decoding, labels, and
tables for CP437, CP720, CP737, CP775, CP850, CP852, CP855, CP857, CP858,
CP860, CP861, CP862, CP863, CP865, CP866, and CP869.

## Upstream backports

Audited the changes from the original revision through upstream
`a155adc7271c9e507556d053c95b459186671d2a` (0.8.42) on 2026-10-08.
The original revision remains the base for this selective backport.

- `a074922023d74acbc56ee53aa931978315419916`: reserve the complete
  two-byte or four-byte destination before consuming a non-ASCII character
  in legacy encoders. Regression coverage exercises UTF-8 and UTF-16 inputs
  with a destination that is one byte short, including resuming conversion.

- `aa6c866206162ee8dc22521a0adaa40948ed03e8`,
  `98f0e5a623219e61877da1a4c64252baebd9ce16`, and
  `36db69e88d2c5a6e39e209005cf89a4e6dc1f4ce`: write into spare capacity
  before increasing `String`/`Vec` lengths, and assert the length stays within
  capacity in release builds. The initialization helper uses upstream's
  original full-zeroing path for every target to preserve portability without
  importing the later assembly-based optimization. The pointer is obtained
  after initialization, as in `5063e286befaa4a1cf36034b3efac4c4d6e2e168`.
  This requires Rust 1.60 for `Vec::spare_capacity_mut`; existing APIs and the
  local OEM module are preserved. Regression coverage catches reuse of a
  finished decoder and verifies the destination retains its contents, length,
  and capacity, with and without replacement.
