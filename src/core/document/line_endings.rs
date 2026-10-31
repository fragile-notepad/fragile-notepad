//! Detects the first line-ending style in contiguous or rope-backed text.

use iced::widget::text_editor::LineEnding;

/// Returns the first CR, LF, CRLF, or LFCR ending, or `None` if absent.
pub fn detect_line_ending(text: &str) -> Option<LineEnding> {
    detect_line_ending_in_chunks([text])
}

pub(super) fn detect_line_ending_in_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a str>,
) -> Option<LineEnding> {
    // Look ahead across chunks so a split CRLF or LFCR stays one ending.
    let mut bytes = chunks.into_iter().flat_map(str::bytes);
    let first = bytes.find(|&byte| byte == b'\r' || byte == b'\n')?;
    Some(match (first, bytes.next()) {
        (b'\r', Some(b'\n')) => LineEnding::CrLf,
        (b'\n', Some(b'\r')) => LineEnding::LfCr,
        (b'\r', _) => LineEnding::Cr,
        _ => LineEnding::Lf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_ending_is_preserved_at_every_chunk_split() {
        for (text, expected) in [
            ("", None),
            ("plain text", None),
            ("\u{4e2d}\u{6587}\r", Some(LineEnding::Cr)),
            ("\u{4e2d}\u{6587}\n", Some(LineEnding::Lf)),
            ("\u{4e2d}\u{6587}\r\n", Some(LineEnding::CrLf)),
            ("\u{4e2d}\u{6587}\n\r", Some(LineEnding::LfCr)),
            ("first\rsecond\n", Some(LineEnding::Cr)),
            ("first\nsecond\r", Some(LineEnding::Lf)),
            ("first\r\nsecond\n", Some(LineEnding::CrLf)),
            ("first\n\rsecond\r\n", Some(LineEnding::LfCr)),
        ] {
            assert_eq!(detect_line_ending(text), expected, "{text:?}");
            for split in (0..=text.len()).filter(|&split| text.is_char_boundary(split)) {
                let chunks = ["", &text[..split], "", &text[split..], ""];
                assert_eq!(
                    detect_line_ending_in_chunks(chunks),
                    expected,
                    "{text:?} split at byte {split}",
                );
            }
        }
        assert_eq!(detect_line_ending_in_chunks([]), None);
    }
}
