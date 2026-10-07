#![cfg(feature = "alloc")]

use encoding_rs::{CoderResult, DecoderResult, UTF_8, WINDOWS_1252};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[test]
fn finished_decoder_preserves_string_on_panic() {
    for encoding in [UTF_8, WINDOWS_1252] {
        for without_replacement in [false, true] {
            let mut decoder = encoding.new_decoder_without_bom_handling();
            let mut output = String::with_capacity(64);
            output.push_str("prefix");
            if without_replacement {
                assert_eq!(
                    decoder.decode_to_string_without_replacement(b"text", &mut output, true),
                    (DecoderResult::InputEmpty, 4)
                );
            } else {
                assert_eq!(
                    decoder.decode_to_string(b"text", &mut output, true),
                    (CoderResult::InputEmpty, 4, false)
                );
            }
            let old_len = output.len();
            let old_capacity = output.capacity();
            let result = catch_unwind(AssertUnwindSafe(|| {
                if without_replacement {
                    let _ =
                        decoder.decode_to_string_without_replacement(b"extra", &mut output, true);
                } else {
                    let _ = decoder.decode_to_string(b"extra", &mut output, true);
                }
            }));
            assert!(result.is_err());
            assert_eq!(output.len(), old_len);
            assert_eq!(output.capacity(), old_capacity);
            assert_eq!(output, "prefixtext");
        }
    }
}
