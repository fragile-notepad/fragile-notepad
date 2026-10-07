use encoding_rs::{
    mem::convert_utf16_to_str_partial, CoderResult, DecoderResult, UTF_8, WINDOWS_1252,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[test]
fn decoder_str_buffer_is_valid_after_unwind() {
    for encoding in [UTF_8, WINDOWS_1252] {
        for without_replacement in [false, true] {
            let mut decoder = encoding.new_decoder_without_bom_handling();
            let mut buffer = String::from("é🙂 buffer");
            if without_replacement {
                let (result, read, written) =
                    decoder.decode_to_str_without_replacement(b"done", &mut buffer, true);
                assert_eq!((result, read, written), (DecoderResult::InputEmpty, 4, 4));
            } else {
                let (result, read, written, had_errors) =
                    decoder.decode_to_str(b"done", &mut buffer, true);
                assert_eq!(
                    (result, read, written, had_errors),
                    (CoderResult::InputEmpty, 4, 4, false)
                );
            }
            assert!(std::str::from_utf8(buffer.as_bytes()).is_ok());
            let result = catch_unwind(AssertUnwindSafe(|| {
                if without_replacement {
                    let _ = decoder.decode_to_str_without_replacement(b"again", &mut buffer, true);
                } else {
                    let _ = decoder.decode_to_str(b"again", &mut buffer, true);
                }
            }));
            assert!(result.is_err());
            assert!(buffer.as_bytes().iter().all(|byte| *byte == 0));
            assert!(std::str::from_utf8(buffer.as_bytes()).is_ok());
        }
    }
}

#[test]
fn utf16_str_conversion_preserves_valid_utf8_at_partial_boundary() {
    let mut output = String::from("🙂🙂");
    let utf16: Vec<u16> = "a🙂b🙂".encode_utf16().collect();
    assert_eq!(convert_utf16_to_str_partial(&utf16, &mut output), (4, 6));
    assert_eq!(&output[..6], "a🙂b");
    assert!(std::str::from_utf8(output.as_bytes()).is_ok());
}
