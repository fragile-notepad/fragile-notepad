//! Logical text positions and linear or rectangular selections.

use super::buffer::EditorBuffer;
use super::layout::{byte_column_for, visual_column_for};

/// A zero-based logical line and UTF-8 byte column, independent of wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EditorPosition {
    pub line: usize,
    pub column: usize,
}

impl EditorPosition {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A half-open range after ordering endpoints with `normalized`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EditorRange {
    pub start: EditorPosition,
    pub end: EditorPosition,
}

impl EditorRange {
    pub const fn new(start: EditorPosition, end: EditorPosition) -> Self {
        Self { start, end }
    }

    pub fn normalized(self) -> Self {
        if self.start <= self.end {
            self
        } else {
            Self {
                start: self.end,
                end: self.start,
            }
        }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EditorSelection {
    pub anchor: EditorPosition,
    pub cursor: EditorPosition,
}

impl EditorSelection {
    pub const fn new(anchor: EditorPosition, cursor: EditorPosition) -> Self {
        Self { anchor, cursor }
    }

    pub fn range(self) -> EditorRange {
        EditorRange::new(self.anchor, self.cursor).normalized()
    }

    pub fn is_caret(self) -> bool {
        self.anchor == self.cursor
    }
}

/// Display-cell columns shared by every line of a rectangular selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RectangularSelection {
    pub anchor_visual_column: usize,
    pub cursor_visual_column: usize,
}

impl RectangularSelection {
    pub const fn new(anchor_visual_column: usize, cursor_visual_column: usize) -> Self {
        Self {
            anchor_visual_column,
            cursor_visual_column,
        }
    }

    pub fn visual_columns(self) -> (usize, usize) {
        if self.anchor_visual_column <= self.cursor_visual_column {
            (self.anchor_visual_column, self.cursor_visual_column)
        } else {
            (self.cursor_visual_column, self.anchor_visual_column)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionShape {
    Linear,
    Rectangular(RectangularSelection),
}

impl SelectionShape {
    pub const fn is_rectangular(self) -> bool {
        matches!(self, Self::Rectangular(_))
    }
}

/// Virtual columns count display cells beyond line ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectionRange {
    pub anchor: EditorPosition,
    pub cursor: EditorPosition,
    pub shape: SelectionShape,
    pub anchor_virtual_column: Option<usize>,
    pub cursor_virtual_column: Option<usize>,
}

impl SelectionRange {
    pub const fn new(anchor: EditorPosition, cursor: EditorPosition) -> Self {
        Self {
            anchor,
            cursor,
            shape: SelectionShape::Linear,
            anchor_virtual_column: None,
            cursor_virtual_column: None,
        }
    }

    pub const fn rectangular(
        anchor: EditorPosition,
        cursor: EditorPosition,
        anchor_visual_column: usize,
        cursor_visual_column: usize,
    ) -> Self {
        Self {
            anchor,
            cursor,
            shape: SelectionShape::Rectangular(RectangularSelection::new(
                anchor_visual_column,
                cursor_visual_column,
            )),
            anchor_virtual_column: None,
            cursor_virtual_column: None,
        }
    }

    pub const fn with_virtual_columns(
        mut self,
        anchor_virtual_column: Option<usize>,
        cursor_virtual_column: Option<usize>,
    ) -> Self {
        self.anchor_virtual_column = anchor_virtual_column;
        self.cursor_virtual_column = cursor_virtual_column;
        self
    }

    pub const fn selection(self) -> EditorSelection {
        EditorSelection::new(self.anchor, self.cursor)
    }

    pub fn range(self) -> EditorRange {
        self.selection().range()
    }

    pub fn is_caret(self) -> bool {
        self.anchor == self.cursor
    }

    pub const fn is_rectangular(self) -> bool {
        self.shape.is_rectangular()
    }

    pub fn clamped(self, buffer: &EditorBuffer) -> Self {
        Self {
            anchor: buffer.clamp_position(self.anchor),
            cursor: buffer.clamp_position(self.cursor),
            shape: self.shape,
            anchor_virtual_column: self.anchor_virtual_column,
            cursor_virtual_column: self.cursor_virtual_column,
        }
    }

    /// Retains rectangular columns beyond short lines. Linear selections include
    /// their final line even when the range ends at column zero.
    pub fn projected_lines(
        self,
        buffer: &EditorBuffer,
        tab_width: usize,
    ) -> Vec<ProjectedSelectionLine> {
        match self.shape {
            SelectionShape::Linear => project_linear_selection(self, buffer, tab_width),
            SelectionShape::Rectangular(rectangular) => {
                project_rectangular_selection(self, rectangular, buffer, tab_width)
            }
        }
    }
}

impl From<EditorSelection> for SelectionRange {
    fn from(selection: EditorSelection) -> Self {
        Self::new(selection.anchor, selection.cursor)
    }
}

impl From<SelectionRange> for EditorSelection {
    fn from(selection: SelectionRange) -> Self {
        selection.selection()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectedSelectionLine {
    pub line: usize,
    pub start: EditorPosition,
    pub end: EditorPosition,
    pub start_visual_column: usize,
    pub end_visual_column: usize,
    pub start_virtual_column: Option<usize>,
    pub end_virtual_column: Option<usize>,
}

impl ProjectedSelectionLine {
    pub const fn range(self) -> EditorRange {
        EditorRange::new(self.start, self.end)
    }

    pub const fn selection(self) -> EditorSelection {
        EditorSelection::new(self.start, self.end)
    }
}

/// Nonempty and source-ordered, with one main selection. Overlaps and duplicate
/// carets are retained for the editing caller to handle.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SelectionSet {
    ranges: Vec<SelectionRange>,
    main: usize,
}

impl SelectionSet {
    pub fn new(selection: EditorSelection) -> Self {
        Self {
            ranges: vec![SelectionRange::from(selection)],
            main: 0,
        }
    }

    pub fn from_ranges(ranges: Vec<EditorSelection>, main: usize) -> Self {
        Self::from_selection_ranges(ranges.into_iter().map(SelectionRange::from).collect(), main)
    }

    /// Empty input becomes a start caret; an out-of-range `main` selects the last input.
    pub fn from_selection_ranges(ranges: Vec<SelectionRange>, main: usize) -> Self {
        if ranges.is_empty() {
            return Self::new(EditorSelection::new(
                EditorPosition::new(0, 0),
                EditorPosition::new(0, 0),
            ));
        }

        let main = main.min(ranges.len().saturating_sub(1));
        let main_range = ranges[main];
        let mut ranges = ranges;
        ranges.sort_by_key(|selection| {
            let range = selection.range();

            (range.start, range.end, selection.cursor, selection.anchor)
        });
        let main = ranges
            .iter()
            .position(|selection| *selection == main_range)
            .unwrap_or(0);

        Self { ranges, main }
    }

    pub fn single(selection: EditorSelection) -> Self {
        Self::new(selection)
    }

    pub fn rectangular(
        anchor: EditorPosition,
        cursor: EditorPosition,
        anchor_visual_column: usize,
        cursor_visual_column: usize,
    ) -> Self {
        Self::from_selection_ranges(
            vec![SelectionRange::rectangular(
                anchor,
                cursor,
                anchor_visual_column,
                cursor_visual_column,
            )],
            0,
        )
    }

    pub fn main(&self) -> EditorSelection {
        self.ranges[self.main].selection()
    }

    pub fn main_range(&self) -> SelectionRange {
        self.ranges[self.main]
    }

    pub fn main_index(&self) -> usize {
        self.main
    }

    pub fn ranges(&self) -> &[SelectionRange] {
        &self.ranges
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn is_single(&self) -> bool {
        self.ranges.len() == 1
    }

    pub fn is_single_caret(&self) -> bool {
        self.is_single() && self.main_range().is_caret()
    }

    pub fn clamped(&self, buffer: &EditorBuffer) -> Self {
        Self::from_selection_ranges(
            self.ranges
                .iter()
                .map(|selection| selection.clamped(buffer))
                .collect(),
            self.main,
        )
    }

    pub fn projected_lines(
        &self,
        buffer: &EditorBuffer,
        tab_width: usize,
    ) -> Vec<ProjectedSelectionLine> {
        self.ranges
            .iter()
            .flat_map(|selection| selection.projected_lines(buffer, tab_width))
            .collect()
    }
}

impl From<EditorSelection> for SelectionSet {
    fn from(selection: EditorSelection) -> Self {
        Self::new(selection)
    }
}

impl From<SelectionRange> for SelectionSet {
    fn from(selection: SelectionRange) -> Self {
        Self::from_selection_ranges(vec![selection], 0)
    }
}

impl From<SelectionSet> for EditorSelection {
    fn from(selections: SelectionSet) -> Self {
        selections.main()
    }
}

impl From<&SelectionSet> for EditorSelection {
    fn from(selections: &SelectionSet) -> Self {
        selections.main()
    }
}

/// Unlike the buffer method, accepts offsets inside CRLF/LFCR pairs and counts
/// both bytes as one ending only when they precede the offset.
pub fn position_for_byte_offset(text: &str, byte_offset: usize) -> Option<EditorPosition> {
    if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
        return None;
    }

    let mut line = 0;
    let mut column = 0;
    let mut index = 0;
    let bytes = text.as_bytes();

    while index < byte_offset {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') && index + 2 <= byte_offset => {
                line += 1;
                column = 0;
                index += 2;
            }
            b'\n' if bytes.get(index + 1) == Some(&b'\r') && index + 2 <= byte_offset => {
                line += 1;
                column = 0;
                index += 2;
            }
            b'\r' | b'\n' => {
                line += 1;
                column = 0;
                index += 1;
            }
            _ => {
                let ch = text[index..].chars().next()?;
                column += ch.len_utf8();
                index += ch.len_utf8();
            }
        }
    }

    Some(EditorPosition::new(line, column))
}

/// Counts CRLF and LFCR pairs as single endings.
pub fn position_after_text(start: EditorPosition, text: &str) -> EditorPosition {
    let mut line = start.line;
    let mut column = start.column;
    let mut index = 0;
    let bytes = text.as_bytes();

    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                line += 1;
                column = 0;
                index += 2;
            }
            b'\n' if bytes.get(index + 1) == Some(&b'\r') => {
                line += 1;
                column = 0;
                index += 2;
            }
            b'\r' | b'\n' => {
                line += 1;
                column = 0;
                index += 1;
            }
            _ => {
                let Some(ch) = text[index..].chars().next() else {
                    break;
                };
                column += ch.len_utf8();
                index += ch.len_utf8();
            }
        }
    }

    EditorPosition::new(line, column)
}

fn project_linear_selection(
    selection: SelectionRange,
    buffer: &EditorBuffer,
    tab_width: usize,
) -> Vec<ProjectedSelectionLine> {
    let range = buffer.clamp_range(selection.range());
    let mut lines = Vec::new();

    for line in range.start.line..=range.end.line {
        let Some(text) = buffer.line(line) else {
            continue;
        };
        let start_column = if line == range.start.line {
            range.start.column
        } else {
            0
        };
        let end_column = if line == range.end.line {
            range.end.column
        } else {
            text.len()
        };
        let start = buffer.clamp_position(EditorPosition::new(line, start_column));
        let end = buffer.clamp_position(EditorPosition::new(line, end_column));

        lines.push(ProjectedSelectionLine {
            line,
            start,
            end,
            start_visual_column: visual_column_for(&text, start.column, tab_width),
            end_visual_column: visual_column_for(&text, end.column, tab_width),
            start_virtual_column: None,
            end_virtual_column: None,
        });
    }

    lines
}

fn project_rectangular_selection(
    selection: SelectionRange,
    rectangular: RectangularSelection,
    buffer: &EditorBuffer,
    tab_width: usize,
) -> Vec<ProjectedSelectionLine> {
    let anchor = buffer.clamp_position(selection.anchor);
    let cursor = buffer.clamp_position(selection.cursor);
    let (first_line, last_line) = if anchor.line <= cursor.line {
        (anchor.line, cursor.line)
    } else {
        (cursor.line, anchor.line)
    };
    let (start_visual_column, end_visual_column) = rectangular.visual_columns();
    let mut lines = Vec::new();

    for line in first_line..=last_line {
        let Some(text) = buffer.line(line) else {
            continue;
        };
        let line_visual_width = visual_column_for(&text, text.len(), tab_width);
        let start_column = byte_column_for(&text, start_visual_column, tab_width);
        let end_column = byte_column_for(&text, end_visual_column, tab_width);

        lines.push(ProjectedSelectionLine {
            line,
            start: EditorPosition::new(line, start_column),
            end: EditorPosition::new(line, end_column),
            start_visual_column,
            end_visual_column,
            start_virtual_column: (start_visual_column > line_visual_width)
                .then_some(start_visual_column),
            end_virtual_column: (end_visual_column > line_visual_width)
                .then_some(end_visual_column),
        });
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::{EditorPosition, position_after_text, position_for_byte_offset};

    #[test]
    fn byte_offsets_handle_line_endings_and_utf8_boundaries() {
        for (text, offset, expected) in [
            ("one\ntwo", 5, Some(EditorPosition::new(1, 1))),
            ("one\r\ntwo", 6, Some(EditorPosition::new(1, 1))),
            ("éx\n好", "éx\n".len(), Some(EditorPosition::new(1, 0))),
            ("éx", "é".len(), Some(EditorPosition::new(0, 2))),
            ("é", 1, None),
        ] {
            assert_eq!(
                position_for_byte_offset(text, offset),
                expected,
                "{text:?} at {offset}"
            );
        }
    }

    #[test]
    fn position_after_text_handles_common_line_endings_and_utf8_byte_columns() {
        assert_eq!(
            position_after_text(EditorPosition::new(2, 3), "a\r\n\u{00e9}"),
            EditorPosition::new(3, 2)
        );
        assert_eq!(
            position_after_text(EditorPosition::new(0, 1), "a\n\rb"),
            EditorPosition::new(1, 1)
        );
    }
}
