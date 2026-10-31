//! Syntax-specific word selection, distinct from keyboard word movement rules.

use super::buffer::EditorBuffer;
use super::position::{EditorPosition, EditorRange};
use super::syntax_hints::SyntaxHintSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WordCharacters {
    extra: String,
    unicode: bool,
}

impl Default for WordCharacters {
    fn default() -> Self {
        Self {
            extra: "_".to_owned(),
            unicode: false,
        }
    }
}

impl WordCharacters {
    pub(super) fn new(extra: impl Into<String>, unicode: bool) -> Self {
        Self {
            extra: extra.into(),
            unicode,
        }
    }

    pub(super) fn is_word_char(&self, ch: char) -> bool {
        self.extra.contains(ch)
            || if self.unicode {
                ch.is_alphanumeric()
            } else {
                ch.is_ascii_alphanumeric()
            }
    }
}

pub(super) fn is_default_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// At line end, selects the word to the left; other separators select nothing.
pub fn word_range_at_position(
    buffer: &EditorBuffer,
    position: EditorPosition,
    syntax_token: &str,
) -> Option<EditorRange> {
    let hints = SyntaxHintSet::load().hints_for(syntax_token);

    word_range_with_characters(buffer, position, &hints.word_characters)
}

fn word_range_with_characters(
    buffer: &EditorBuffer,
    position: EditorPosition,
    word_characters: &WordCharacters,
) -> Option<EditorRange> {
    let position = buffer.clamp_position(position);
    // Syntax word characters exclude CR/LF, so the word stays on this line.
    let text = buffer.line(position.line)?;
    let offset = word_offset_at_column(&text, position.column, word_characters)?;
    let start = word_start_offset(&text, offset, word_characters);
    let end = word_end_offset(&text, offset, word_characters);

    Some(EditorRange::new(
        EditorPosition::new(position.line, start),
        EditorPosition::new(position.line, end),
    ))
}

fn word_offset_at_column(
    text: &str,
    offset: usize,
    word_characters: &WordCharacters,
) -> Option<usize> {
    if text
        .get(offset..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(|ch| word_characters.is_word_char(ch))
    {
        return Some(offset);
    }

    if offset != text.len() {
        return None;
    }

    let previous = previous_char_offset(text, offset)?;
    text[previous..]
        .chars()
        .next()
        .is_some_and(|ch| word_characters.is_word_char(ch))
        .then_some(previous)
}

fn word_start_offset(text: &str, mut offset: usize, word_characters: &WordCharacters) -> usize {
    while let Some(previous) = previous_char_offset(text, offset) {
        let Some(ch) = text[previous..].chars().next() else {
            break;
        };
        if !word_characters.is_word_char(ch) {
            break;
        }
        offset = previous;
    }

    offset
}

fn previous_char_offset(text: &str, offset: usize) -> Option<usize> {
    text.get(..offset)?
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn word_end_offset(text: &str, mut offset: usize, word_characters: &WordCharacters) -> usize {
    while offset < text.len() {
        let Some(ch) = text[offset..].chars().next() else {
            break;
        };
        if !word_characters.is_word_char(ch) {
            break;
        }
        offset += ch.len_utf8();
    }

    offset
}
