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
