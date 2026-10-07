use encoding_rs::{EncoderResult, GB18030, SHIFT_JIS};

#[test]
fn legacy_encoder_reserves_complete_character() {
    for (encoding, text, capacity) in [(SHIFT_JIS, "abc一", 4), (GB18030, "abc\u{FF00}", 6)] {
        let mut output = vec![0; capacity];
        let mut encoder = encoding.new_encoder();
        assert_eq!(
            encoder.encode_from_utf8_without_replacement(text, &mut output, false),
            (EncoderResult::OutputFull, 3, 3)
        );
        let prefix = output[..3].to_vec();
        let mut remainder = [0; 16];
        let (result, read, written) =
            encoder.encode_from_utf8_without_replacement(&text[3..], &mut remainder, true);
        assert_eq!(result, EncoderResult::InputEmpty);
        assert_eq!(read, text.len() - 3);
        let mut complete = prefix;
        complete.extend_from_slice(&remainder[..written]);
        let mut reference = [0; 16];
        let (result, _, reference_written) = encoding
            .new_encoder()
            .encode_from_utf8_without_replacement(text, &mut reference, true);
        assert_eq!(result, EncoderResult::InputEmpty);
        assert_eq!(complete, &reference[..reference_written]);
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut encoder = encoding.new_encoder();
        assert_eq!(
            encoder.encode_from_utf16_without_replacement(&utf16, &mut output, false),
            (EncoderResult::OutputFull, 3, 3)
        );
        let (result, read, _) =
            encoder.encode_from_utf16_without_replacement(&utf16[3..], &mut remainder, true);
        assert_eq!(result, EncoderResult::InputEmpty);
        assert_eq!(read, utf16.len() - 3);
    }
}
