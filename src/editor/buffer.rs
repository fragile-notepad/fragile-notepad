use super::position::{EditorPosition, EditorRange, position_after_text};
use ropey::Rope;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDelta {
    pub before_range: EditorRange,
    pub after_range: EditorRange,
    pub before_text: String,
    pub after_text: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EditorBuffer {
    rope: Rope,
    line_starts: Vec<usize>,
}

impl fmt::Debug for EditorBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorBuffer")
            .field("text", &self.text())
            .finish()
    }
}

impl EditorBuffer {
    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let line_starts = line_starts(&text);

        Self {
            rope: Rope::from_str(&text),
            line_starts,
        }
    }

    /// Materializes the complete buffer text.
    ///
    /// The rope is the authoritative storage. Code that can operate
    /// incrementally should prefer `chunks`, `slice_text`, or `line_text`.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn chunks(&self) -> impl Iterator<Item = &str> {
        self.rope.chunks()
    }

    pub fn slice_text(&self, range: EditorRange) -> String {
        let range = self.clamp_range(range);
        let start = self.char_offset_clamped(range.start);
        let end = self.char_offset_clamped(range.end);

        self.rope.slice(start..end).to_string()
    }

    pub fn position_for_byte_offset(&self, byte_offset: usize) -> Option<EditorPosition> {
        self.byte_to_char_boundary(byte_offset)?;
        if self.is_inside_paired_line_ending(byte_offset) {
            return None;
        }
        let line = self
            .line_starts
            .binary_search(&byte_offset)
            .unwrap_or_else(|next_line| next_line.saturating_sub(1));
        let line_start = *self.line_starts.get(line)?;
        let column = byte_offset.saturating_sub(line_start);

        Some(EditorPosition::new(line, column))
    }

    /// Includes the final line, even when the buffer is empty.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn line(&self, index: usize) -> Option<String> {
        self.line_text(index)
    }

    pub fn line_text(&self, index: usize) -> Option<String> {
        let start = self.byte_to_char_boundary(*self.line_starts.get(index)?)?;
        let end = self.line_content_end_char(index, start);

        Some(self.rope.slice(start..end).to_string())
    }

    /// Copies bounded context around a position without materializing the whole line.
    pub fn line_excerpt(
        &self,
        position: EditorPosition,
        max_chars: usize,
        context_before: usize,
    ) -> Option<String> {
        self.line_excerpt_with_match(
            EditorRange::new(position, position),
            max_chars,
            context_before,
        )
        .map(|(excerpt, _)| excerpt)
    }

    /// Copies bounded context and returns the match's UTF-8 byte range inside it.
    /// Multiline matches are clipped to the first line of the preview.
    pub fn line_excerpt_with_match(
        &self,
        range: EditorRange,
        max_chars: usize,
        context_before: usize,
    ) -> Option<(String, std::ops::Range<usize>)> {
        let range = range.normalized();
        let position = range.start;
        let line_byte_start = *self.line_starts.get(position.line)?;
        let line_start = self.byte_to_char_boundary(line_byte_start)?;
        let line_end = self.line_content_end_char(position.line, line_start);
        let center = self.byte_to_char_boundary(line_byte_start.checked_add(position.column)?)?;
        if center > line_end || max_chars == 0 {
            return None;
        }
        let start = center
            .saturating_sub(context_before.min(max_chars - 1))
            .max(line_start);
        let end = start.saturating_add(max_chars).min(line_end);
        let mut excerpt = String::new();
        if start > line_start {
            excerpt.push('…');
        }
        let match_start = excerpt.len() + self.rope.slice(start..center).len_bytes();
        let match_end_char = self
            .char_offset_clamped(self.clamp_position(range.end))
            .min(end)
            .max(center);
        let match_end = match_start + self.rope.slice(center..match_end_char).len_bytes();
        excerpt.extend(self.rope.slice(start..end).chars());
        if end < line_end {
            excerpt.push('…');
        }
        Some((excerpt, match_start..match_end))
    }

    pub fn replace_range(&mut self, range: EditorRange, replacement: &str) -> EditDelta {
        let before_range = self.clamp_range(range);
        let start_offset = self.char_offset_clamped(before_range.start);
        let end_offset = self.char_offset_clamped(before_range.end);
        let before_text = self.rope.slice(start_offset..end_offset).to_string();

        self.replace_chars(start_offset, end_offset, replacement);

        let after_end = position_after_text(before_range.start, replacement);
        let after_range = EditorRange::new(
            self.clamp_position(before_range.start),
            self.clamp_position(after_end),
        );

        EditDelta {
            before_range,
            after_range,
            before_text,
            after_text: replacement.to_owned(),
        }
    }

    pub fn append_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let byte_len = self.rope.len_bytes();
        let can_extend_line_index = byte_len > 0
            && !matches!(self.rope.byte(byte_len - 1), b'\r' | b'\n')
            && !text
                .as_bytes()
                .first()
                .is_some_and(|byte| matches!(byte, b'\r' | b'\n'));

        if can_extend_line_index {
            self.rope.insert(self.rope.len_chars(), text);
            self.line_starts.extend(
                line_starts(text)
                    .into_iter()
                    .skip(1)
                    .map(|offset| byte_len.saturating_add(offset)),
            );
        } else {
            let end = self.rope.len_chars();
            self.replace_chars(end, end, text);
        }
    }

    pub fn clamp_position(&self, position: EditorPosition) -> EditorPosition {
        let line = position.line.min(self.line_count().saturating_sub(1));
        let line_start = self
            .line_starts
            .get(line)
            .and_then(|offset| self.byte_to_char_boundary(*offset))
            .unwrap_or(0);
        let line_end = self.line_content_end_char(line, line_start);
        let column =
            previous_char_boundary_in_slice(self.rope.slice(line_start..line_end), position.column);

        EditorPosition::new(line, column)
    }

    pub fn clamp_range(&self, range: EditorRange) -> EditorRange {
        let range = range.normalized();

        EditorRange::new(
            self.clamp_position(range.start),
            self.clamp_position(range.end),
        )
    }

    pub fn byte_offset(&self, position: EditorPosition) -> usize {
        let position = self.clamp_position(position);
        self.line_starts
            .get(position.line)
            .copied()
            .unwrap_or(0)
            .saturating_add(position.column)
    }

    /// Convert an endpoint already checked by `clamp_range`.
    fn char_offset_clamped(&self, position: EditorPosition) -> usize {
        self.rope
            .byte_to_char(self.line_starts[position.line] + position.column)
    }

    fn byte_to_char_boundary(&self, byte_offset: usize) -> Option<usize> {
        let char_offset = self.rope.try_byte_to_char(byte_offset).ok()?;

        if self.rope.char_to_byte(char_offset) == byte_offset {
            Some(char_offset)
        } else {
            None
        }
    }

    fn is_inside_paired_line_ending(&self, byte_offset: usize) -> bool {
        if byte_offset == 0 || byte_offset >= self.len_bytes() {
            return false;
        }

        let Ok(before_char) = self.rope.try_byte_to_char(byte_offset - 1) else {
            return false;
        };
        let Ok(after_char) = self.rope.try_byte_to_char(byte_offset) else {
            return false;
        };
        matches!(
            (self.rope.char(before_char), self.rope.char(after_char)),
            ('\r', '\n') | ('\n', '\r')
        )
    }

    fn line_content_end_char(&self, index: usize, start: usize) -> usize {
        let raw_end = if index + 1 < self.line_count() {
            self.line_starts
                .get(index + 1)
                .and_then(|offset| self.byte_to_char_boundary(*offset))
                .unwrap_or_else(|| self.rope.len_chars())
        } else {
            self.rope.len_chars()
        };
        let line = self.rope.slice(start..raw_end);
        let mut end = raw_end;
        let line_char_count = line.len_chars();

        if line_char_count > 0 && line.char(line_char_count - 1) == '\n' {
            end -= 1;

            if line_char_count > 1 && line.char(line_char_count - 2) == '\r' {
                end -= 1;
            }
        } else if line_char_count > 0 && line.char(line_char_count - 1) == '\r' {
            end -= 1;

            if line_char_count > 1 && line.char(line_char_count - 2) == '\n' {
                end -= 1;
            }
        }

        end
    }

    fn replace_chars(&mut self, start: usize, end: usize, replacement: &str) {
        let start_byte = self.rope.char_to_byte(start);
        let end_byte = self.rope.char_to_byte(end);
        let first_line = self
            .line_starts
            .partition_point(|offset| *offset <= start_byte)
            .saturating_sub(2);
        let window_start = self.line_starts[first_line];
        let mut suffix_line = self
            .line_starts
            .partition_point(|offset| *offset <= end_byte);
        // A changed CR/LF pairing can propagate through a run of line endings.
        // Resume the old index only at a boundary followed by ordinary text.
        while suffix_line < self.line_starts.len() {
            let offset = self.line_starts[suffix_line];
            if offset < self.rope.len_bytes() && !matches!(self.rope.byte(offset), b'\r' | b'\n') {
                break;
            }
            suffix_line += 1;
        }
        let old_window_end = self
            .line_starts
            .get(suffix_line)
            .copied()
            .unwrap_or(self.rope.len_bytes());
        let removed_bytes = end_byte - start_byte;
        let new_window_end = old_window_end - removed_bytes + replacement.len();

        self.rope.remove(start..end);
        self.rope.insert(start, replacement);

        let window = self
            .rope
            .slice(self.rope.byte_to_char(window_start)..self.rope.byte_to_char(new_window_end));
        let mut local_starts = line_starts_from_chunks(window.chunks());
        if suffix_line < self.line_starts.len() {
            // The final boundary belongs to the unchanged suffix.
            local_starts.pop();
        }
        for offset in &mut self.line_starts[suffix_line..] {
            *offset = *offset - removed_bytes + replacement.len();
        }
        self.line_starts.splice(
            first_line..suffix_line,
            local_starts.into_iter().map(|offset| window_start + offset),
        );
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    line_starts_from_chunks(std::iter::once(text))
}

fn line_starts_from_chunks<'a>(chunks: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    let mut starts = vec![0];
    let mut index = 0;
    let mut pending_line_ending = None;

    for chunk in chunks {
        for byte in chunk.bytes() {
            if let Some(previous) = pending_line_ending.take() {
                if is_paired_line_ending(previous, byte) {
                    index += 1;
                    starts.push(index);
                    continue;
                }

                starts.push(index);
            }

            index += 1;
            if matches!(byte, b'\r' | b'\n') {
                pending_line_ending = Some(byte);
            }
        }
    }

    if pending_line_ending.is_some() {
        starts.push(index);
    }

    starts
}

fn is_paired_line_ending(first: u8, second: u8) -> bool {
    matches!((first, second), (b'\r', b'\n') | (b'\n', b'\r'))
}

fn previous_char_boundary_in_slice(text: ropey::RopeSlice<'_>, target: usize) -> usize {
    let mut index = target.min(text.len_bytes());

    while index > 0 {
        let Ok(char_index) = text.try_byte_to_char(index) else {
            index -= 1;
            continue;
        };

        if text.char_to_byte(char_index) == index {
            break;
        }

        index -= 1;
    }

    index
}
