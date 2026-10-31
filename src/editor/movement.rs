//! Grapheme-based caret movement; word motions use ASCII letters, digits, and `_`.

use unicode_segmentation::UnicodeSegmentation;

use super::action::CaretMotion;
use super::buffer::EditorBuffer;
use super::position::EditorPosition;
use super::word::is_default_word_char;

pub fn is_vertical_motion(motion: CaretMotion) -> bool {
    matches!(
        motion,
        CaretMotion::Up | CaretMotion::Down | CaretMotion::PageUp | CaretMotion::PageDown
    )
}

/// Page motions move 20 logical lines; paragraphs are separated by blank lines.
pub fn move_position(
    buffer: &EditorBuffer,
    position: EditorPosition,
    motion: CaretMotion,
) -> EditorPosition {
    let position = buffer.clamp_position(position);

    match motion {
        CaretMotion::Left => previous_position(buffer, position).unwrap_or(position),
        CaretMotion::Right => next_position(buffer, position).unwrap_or(position),
        CaretMotion::WordLeft => previous_word_position(buffer, position).unwrap_or(position),
        CaretMotion::WordRight => next_word_position(buffer, position).unwrap_or(position),
        CaretMotion::Up | CaretMotion::Down | CaretMotion::PageUp | CaretMotion::PageDown => {
            move_position_with_column(buffer, position, motion, position.column)
        }
        CaretMotion::ParagraphUp => previous_paragraph_position(buffer, position),
        CaretMotion::ParagraphDown => next_paragraph_position(buffer, position),
        CaretMotion::LineStart => EditorPosition::new(position.line, 0),
        CaretMotion::LineEnd => line_end(buffer, position.line),
        CaretMotion::DocumentStart => EditorPosition::new(0, 0),
        CaretMotion::DocumentEnd => document_end(buffer),
    }
}

/// `column` is a UTF-8 byte column, not a display column, and is clamped on short lines.
pub fn move_position_with_column(
    buffer: &EditorBuffer,
    position: EditorPosition,
    motion: CaretMotion,
    column: usize,
) -> EditorPosition {
    let line = match motion {
        CaretMotion::Up => position.line.saturating_sub(1),
        CaretMotion::Down => position
            .line
            .saturating_add(1)
            .min(buffer.line_count().saturating_sub(1)),
        CaretMotion::PageUp => position.line.saturating_sub(20),
        CaretMotion::PageDown => position
            .line
            .saturating_add(20)
            .min(buffer.line_count().saturating_sub(1)),
        _ => position.line,
    };

    buffer.clamp_position(EditorPosition::new(line, column))
}

/// Crosses CRLF/LFCR pairs in one step.
pub fn previous_grapheme_position(
    buffer: &EditorBuffer,
    position: EditorPosition,
) -> Option<EditorPosition> {
    let position = buffer.clamp_position(position);
    if position.column == 0 {
        return position
            .line
            .checked_sub(1)
            .map(|line| line_end(buffer, line));
    }
    let text = buffer.line(position.line)?;
    previous_grapheme_offset(&text, position.column)
        .map(|column| EditorPosition::new(position.line, column))
}

pub fn next_grapheme_position(
    buffer: &EditorBuffer,
    position: EditorPosition,
) -> Option<EditorPosition> {
    let position = buffer.clamp_position(position);
    let text = buffer.line(position.line)?;
    if position.column == text.len() {
        return (position.line + 1 < buffer.line_count())
            .then_some(EditorPosition::new(position.line + 1, 0));
    }
    next_grapheme_offset(&text, position.column)
        .map(|column| EditorPosition::new(position.line, column))
}

/// The byte offset must already lie on a grapheme boundary.
pub fn previous_grapheme_offset(text: &str, offset: usize) -> Option<usize> {
    text.get(..offset)?
        .grapheme_indices(true)
        .next_back()
        .map(|(index, _)| index)
}

/// The byte offset must already lie on a grapheme boundary.
pub fn next_grapheme_offset(text: &str, offset: usize) -> Option<usize> {
    let next = text
        .get(offset..)?
        .graphemes(true)
        .next()
        .filter(|grapheme| !grapheme.is_empty())?;

    Some(offset + next.len())
}

pub fn line_end(buffer: &EditorBuffer, line: usize) -> EditorPosition {
    buffer.clamp_position(EditorPosition::new(line, usize::MAX))
}

pub fn document_end(buffer: &EditorBuffer) -> EditorPosition {
    let line = buffer.line_count().saturating_sub(1);

    line_end(buffer, line)
}

fn previous_position(buffer: &EditorBuffer, position: EditorPosition) -> Option<EditorPosition> {
    previous_grapheme_position(buffer, position)
}

fn next_position(buffer: &EditorBuffer, position: EditorPosition) -> Option<EditorPosition> {
    next_grapheme_position(buffer, position)
}

fn previous_word_position(
    buffer: &EditorBuffer,
    position: EditorPosition,
) -> Option<EditorPosition> {
    let mut position = buffer.clamp_position(position);
    let mut in_word = false;
    loop {
        let text = buffer.line(position.line)?;
        for (column, grapheme) in text[..position.column].grapheme_indices(true).rev() {
            let word = grapheme.chars().next().is_some_and(is_default_word_char);
            if in_word && !word {
                return Some(position);
            }
            in_word |= word;
            position.column = column;
        }
        if in_word || position.line == 0 {
            return Some(position);
        }
        position = line_end(buffer, position.line - 1);
    }
}

fn next_word_position(buffer: &EditorBuffer, position: EditorPosition) -> Option<EditorPosition> {
    let mut position = buffer.clamp_position(position);
    // A line break separates words even without adjacent punctuation or spaces.
    let mut skipping_word = true;
    loop {
        let text = buffer.line(position.line)?;
        let start = position.column;
        for (offset, ch) in text[start..].char_indices() {
            let word = is_default_word_char(ch);
            if !word {
                skipping_word = false;
            } else if !skipping_word {
                return Some(EditorPosition::new(position.line, start + offset));
            }
        }
        if position.line + 1 == buffer.line_count() {
            return Some(EditorPosition::new(position.line, text.len()));
        }
        skipping_word = false;
        position = EditorPosition::new(position.line + 1, 0);
    }
}

fn previous_paragraph_position(buffer: &EditorBuffer, position: EditorPosition) -> EditorPosition {
    let current_line = buffer.clamp_position(position).line;
    let Some(mut line) = current_line.checked_sub(1) else {
        return EditorPosition::new(0, 0);
    };

    while line > 0 && is_blank_line(buffer, line) {
        line -= 1;
    }

    while line > 0 && !is_blank_line(buffer, line - 1) {
        line -= 1;
    }

    EditorPosition::new(line, 0)
}

fn next_paragraph_position(buffer: &EditorBuffer, position: EditorPosition) -> EditorPosition {
    let current_line = buffer.clamp_position(position).line;
    let last_line = buffer.line_count().saturating_sub(1);

    if current_line >= last_line {
        return document_end(buffer);
    }

    let mut line = current_line + 1;

    if !is_blank_line(buffer, current_line) {
        while line <= last_line && !is_blank_line(buffer, line) {
            line += 1;
        }
    }

    while line <= last_line && is_blank_line(buffer, line) {
        line += 1;
    }

    if line > last_line {
        return document_end(buffer);
    }

    EditorPosition::new(line, 0)
}

fn is_blank_line(buffer: &EditorBuffer, line: usize) -> bool {
    buffer.line(line).unwrap_or_default().trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grapheme_navigation_crosses_combining_clusters_and_paired_endings() {
        let buffer = EditorBuffer::from_text("a\u{301}😀\r\nb\n\rc");
        let positions = [
            EditorPosition::new(0, 0),
            EditorPosition::new(0, 3),
            EditorPosition::new(0, 7),
            EditorPosition::new(1, 0),
            EditorPosition::new(1, 1),
            EditorPosition::new(2, 0),
            EditorPosition::new(2, 1),
        ];
        for pair in positions.windows(2) {
            assert_eq!(next_grapheme_position(&buffer, pair[0]), Some(pair[1]));
            assert_eq!(previous_grapheme_position(&buffer, pair[1]), Some(pair[0]));
        }
        assert_eq!(previous_grapheme_position(&buffer, positions[0]), None);
        assert_eq!(next_grapheme_position(&buffer, positions[6]), None);
    }

    #[test]
    fn word_navigation_crosses_empty_lines_without_copying_the_document() {
        let buffer = EditorBuffer::from_text("one\r\n\r\n  two! three");
        assert_eq!(
            move_position(&buffer, EditorPosition::new(0, 0), CaretMotion::WordRight),
            EditorPosition::new(2, 2)
        );
        assert_eq!(
            move_position(&buffer, EditorPosition::new(2, 2), CaretMotion::WordLeft),
            EditorPosition::new(0, 0)
        );
        assert_eq!(
            move_position(&buffer, EditorPosition::new(2, 2), CaretMotion::WordRight),
            EditorPosition::new(2, 7)
        );
        assert_eq!(
            move_position(&buffer, EditorPosition::new(2, 7), CaretMotion::WordLeft),
            EditorPosition::new(2, 2)
        );
    }
}
