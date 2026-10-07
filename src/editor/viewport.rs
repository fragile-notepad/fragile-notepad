use std::{borrow::Cow, collections::HashMap, ops::Range};

use super::buffer::EditorBuffer;
use super::cjk::CjkContext;
use super::fold::{FoldDelimiter, FoldModel, FoldRange};
pub use super::fold_projection::{FoldProjection, ProjectionFragment};
use super::layout::{visual_column_for, visual_width_with_tab_width};
use super::position::EditorPosition;
use super::wrap_measurement::{MeasuredWrapLine, WrapMeasurement};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VisibleRow {
    pub row: usize,
    pub document_line: usize,
}

/// The portion of a displayed line occupying one screen row.
///
/// Columns address the displayed text; fold projections map them back to source
/// positions. Visual columns retain line offsets so tabs keep their stops when
/// wrapping. Neither wrapping nor reflow changes buffer text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowSegment {
    pub start_column: usize,
    pub end_column: usize,
    pub start_visual_column: usize,
    pub end_visual_column: usize,
    pub is_last: bool,
}

impl RowSegment {
    pub const fn is_continuation(self) -> bool {
        self.start_column > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportModel {
    line_count: usize,
    visible_lines: Vec<usize>,
    document_to_visible: Vec<Option<usize>>,
    wrap_columns: Option<usize>,
    tab_width: usize,
    wrapped_segments: Option<Vec<RowSegment>>,
    collapsed_ranges: Vec<FoldRange>,
    fold_indicator_columns: usize,
    wrap_measurement: Option<WrapMeasurement>,
    projections: HashMap<usize, FoldProjection>,
    projection_owners: HashMap<usize, usize>,
}

impl ViewportModel {
    pub fn new(line_count: usize, folds: &FoldModel) -> Self {
        Self::new_with_tab_width(line_count, folds, 4)
    }

    pub fn new_with_tab_width(line_count: usize, folds: &FoldModel, tab_width: usize) -> Self {
        let mut visible_lines = Vec::new();
        let mut document_to_visible = vec![None; line_count];
        let mut collapsed = folds.collapsed_ranges().copied().collect::<Vec<_>>();
        collapsed.sort_by_key(|range| (range.start_line, range.end_line));
        let mut collapsed_index = 0;
        let mut line = 0;

        while line < line_count {
            let row = visible_lines.len();
            visible_lines.push(line);
            document_to_visible[line] = Some(row);

            let mut next_line = line + 1;
            while let Some(range) = collapsed.get(collapsed_index)
                && range.start_line <= line
            {
                if range.start_line == line {
                    next_line = next_line.max(range.end_line.saturating_add(1));
                }
                collapsed_index += 1;
            }
            line = next_line;
        }

        Self {
            line_count,
            visible_lines,
            document_to_visible,
            wrap_columns: None,
            tab_width: tab_width.max(1),
            wrapped_segments: None,
            collapsed_ranges: collapsed,
            fold_indicator_columns: 5,
            wrap_measurement: None,
            projections: HashMap::new(),
            projection_owners: HashMap::new(),
        }
    }

    pub fn new_with_buffer(buffer: &EditorBuffer, folds: &FoldModel, tab_width: usize) -> Self {
        let mut viewport = Self::new_with_tab_width(buffer.line_count(), folds, tab_width);
        if viewport.collapsed_ranges.is_empty() {
            return viewport;
        }
        viewport.visible_lines.clear();
        viewport.document_to_visible.fill(None);
        let mut line = 0;
        while line < viewport.line_count {
            let row = viewport.visible_lines.len();
            viewport.visible_lines.push(line);
            viewport.document_to_visible[line] = Some(row);
            let end = viewport
                .collapsed_ranges
                .partition_point(|range| range.start_line <= line);
            let range = end
                .checked_sub(1)
                .and_then(|index| viewport.collapsed_ranges.get(index))
                .copied()
                .filter(|range| range.start_line == line);
            let mut next_line = range.map_or(line + 1, |range| range.end_line + 1);
            if let Some(projection) =
                range.and_then(|range| FoldProjection::build(buffer, folds, range))
            {
                next_line = projection.final_source_line() + 1;
                for fragment in &projection.fragments {
                    if let ProjectionFragment::Source { source_start, .. } = fragment {
                        viewport.projection_owners.insert(source_start.line, line);
                        viewport.document_to_visible[source_start.line] = Some(row);
                    }
                }
                viewport.projection_owners.insert(line, line);
                viewport.projections.insert(line, projection);
            }
            line = next_line;
        }
        viewport
    }

    pub fn has_projections(&self) -> bool {
        !self.projections.is_empty()
    }

    pub fn projection(&self, line: usize) -> Option<&FoldProjection> {
        self.projections.get(&line)
    }

    pub fn source_lines(&self, line: usize) -> Vec<usize> {
        let Some(projection) = self.projection(line) else {
            return vec![line];
        };
        let mut lines = Vec::new();
        for fragment in &projection.fragments {
            if let ProjectionFragment::Source { source_start, .. } = fragment
                && lines.last() != Some(&source_start.line)
            {
                lines.push(source_start.line);
            }
        }
        lines
    }

    pub fn display_text<'a>(&'a self, line: usize, buffer: &EditorBuffer) -> Cow<'a, str> {
        self.projection(line).map_or_else(
            || Cow::Owned(buffer.line(line).unwrap_or_default()),
            |projection| Cow::Borrowed(projection.text.as_str()),
        )
    }

    pub fn display_position(&self, source: EditorPosition) -> Option<EditorPosition> {
        if let Some(&owner) = self.projection_owners.get(&source.line) {
            self.document_line_to_visible_row(owner)?;
            return self
                .projections
                .get(&owner)?
                .source_to_display(source)
                .map(|column| EditorPosition::new(owner, column));
        }
        self.document_line_to_visible_row(source.line)?;
        Some(source)
    }

    pub fn source_position(&self, display: EditorPosition) -> EditorPosition {
        self.projection(display.line).map_or(display, |projection| {
            projection.display_to_source(display.column)
        })
    }

    /// Builds a soft-wrapped viewport without changing logical line numbers.
    ///
    /// Breaks prefer Unicode word boundaries. Long tokens are split only
    /// between complete graphemes, even when one grapheme or tab is wider than
    /// the viewport. All whitespace remains part of exactly one segment.
    pub fn new_wrapped(
        buffer: &EditorBuffer,
        folds: &FoldModel,
        columns: usize,
        tab_width: usize,
    ) -> Self {
        Self::new_wrapped_with_fold_indicator_columns(buffer, folds, columns, tab_width, 5)
    }

    /// Uses the rendered badge's measured column reservation, including its
    /// minimum pixel width when the editor is zoomed out.
    pub fn new_wrapped_with_fold_indicator_columns(
        buffer: &EditorBuffer,
        folds: &FoldModel,
        columns: usize,
        tab_width: usize,
        fold_indicator_columns: usize,
    ) -> Self {
        Self::new_wrapped_with_measurement(
            buffer,
            folds,
            columns,
            tab_width,
            fold_indicator_columns,
            None,
            None,
        )
    }

    pub(crate) fn new_wrapped_with_measurement(
        buffer: &EditorBuffer,
        folds: &FoldModel,
        columns: usize,
        tab_width: usize,
        fold_indicator_columns: usize,
        measurement: Option<WrapMeasurement>,
        context: Option<&CjkContext>,
    ) -> Self {
        let mut viewport = Self::new_with_buffer(buffer, folds, tab_width);
        let visible_lines = std::mem::take(&mut viewport.visible_lines);
        viewport.wrap_columns = Some(columns.max(1));
        viewport.fold_indicator_columns = fold_indicator_columns;
        viewport.wrap_measurement = measurement;
        viewport.wrapped_segments = Some(Vec::with_capacity(visible_lines.len()));

        for line in visible_lines {
            if let Some(projection) = viewport.projection(line).cloned() {
                let segments = viewport
                    .wrapped_segments
                    .as_mut()
                    .expect("wrapped segments");
                viewport.document_to_visible[line] = Some(viewport.visible_lines.len());
                let atomic_ranges = projection
                    .fragments
                    .iter()
                    .filter_map(|fragment| match fragment {
                        ProjectionFragment::Placeholder { display_range, .. } => {
                            Some(display_range.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                append_wrapped_segments_with_measurement(
                    segments,
                    &projection.text,
                    columns,
                    tab_width,
                    line,
                    measurement,
                    context,
                    &atomic_ranges,
                );
                viewport.visible_lines.resize(segments.len(), line);
            } else if let Some(text) = buffer.line(line) {
                let line_columns = viewport.wrapped_line_columns(line, columns);
                let delimiter = viewport.collapsed_delimiter(line, folds);
                viewport.append_wrapped_line(line, &text, line_columns, delimiter, context);
            }
        }
        for (&source, &owner) in &viewport.projection_owners {
            viewport.document_to_visible[source] = viewport.document_to_visible[owner];
        }

        viewport
    }

    pub fn wrap_columns(&self) -> Option<usize> {
        self.wrap_columns
    }

    pub fn fold_indicator_columns(&self) -> usize {
        self.fold_indicator_columns
    }

    pub(crate) fn wrap_measurement(&self) -> Option<WrapMeasurement> {
        self.wrap_measurement
    }

    pub fn line_count(&self) -> usize {
        self.line_count
    }

    pub fn visible_row_count(&self) -> usize {
        self.visible_lines.len()
    }

    pub fn document_line_to_visible_row(&self, document_line: usize) -> Option<usize> {
        self.document_to_visible
            .get(document_line)
            .copied()
            .flatten()
    }

    pub fn visible_row_to_document_line(&self, visible_row: usize) -> Option<usize> {
        self.visible_lines.get(visible_row).copied()
    }

    /// Returns cached boundaries for wrapped rows without copying line text.
    pub fn row_segment(&self, visible_row: usize, buffer: &EditorBuffer) -> Option<RowSegment> {
        if let Some(segments) = &self.wrapped_segments {
            return segments.get(visible_row).copied();
        }

        let line = self.visible_row_to_document_line(visible_row)?;
        let text = self.display_text(line, buffer);
        Some(RowSegment {
            start_column: 0,
            end_column: text.len(),
            start_visual_column: 0,
            end_visual_column: visual_column_for(&text, text.len(), self.tab_width),
            is_last: true,
        })
    }

    /// Maps a caret to its screen row. At a wrap boundary, the caret belongs
    /// to the following row; the logical line end belongs to its final row.
    pub fn position_to_visible_row(&self, position: EditorPosition) -> Option<usize> {
        let position = self.display_position(position)?;
        self.display_position_to_visible_row(position)
    }

    fn display_position_to_visible_row(&self, position: EditorPosition) -> Option<usize> {
        let first_row = self.document_line_to_visible_row(position.line)?;
        let Some(segments) = &self.wrapped_segments else {
            return Some(first_row);
        };
        let row_count =
            self.visible_lines[first_row..].partition_point(|line| *line == position.line);
        let offset = segments[first_row..first_row + row_count]
            .partition_point(|segment| segment.start_column <= position.column)
            .saturating_sub(1);

        Some(first_row + offset)
    }

    pub fn visible_rows(&self) -> impl Iterator<Item = VisibleRow> + '_ {
        self.visible_lines
            .iter()
            .enumerate()
            .map(|(row, document_line)| VisibleRow {
                row,
                document_line: *document_line,
            })
    }

    /// Reflows an inclusive range of edited logical lines without measuring
    /// unchanged text. The caller must include every line whose text changed.
    ///
    /// Returns `false`, leaving this viewport unchanged, if it is unwrapped,
    /// the logical line count or collapsed folds changed, or the view contains
    /// fold projections. Those cases require rebuilding the viewport. Otherwise
    /// edited visible lines are measured; cached suffix rows and their indices
    /// are shifted when the replacement has a different number of rows.
    pub fn reflow_wrapped_lines(
        &mut self,
        buffer: &EditorBuffer,
        folds: &FoldModel,
        first_line: usize,
        last_line: usize,
    ) -> bool {
        self.reflow_wrapped_lines_with_context(buffer, folds, first_line, last_line, None)
    }

    pub(crate) fn reflow_wrapped_lines_with_context(
        &mut self,
        buffer: &EditorBuffer,
        folds: &FoldModel,
        first_line: usize,
        last_line: usize,
        context: Option<&CjkContext>,
    ) -> bool {
        let Some(columns) = self.wrap_columns else {
            return false;
        };
        if self.line_count != buffer.line_count() {
            return false;
        }
        if !self.projections.is_empty()
            || self.collapsed_ranges.iter().any(|range| {
                (range.start_line >= first_line && range.start_line <= last_line
                    || range.end_line >= first_line && range.end_line <= last_line)
                    && folds.delimiter(*range).is_some()
                    && FoldProjection::build(buffer, folds, *range).is_some()
            })
        {
            return false;
        }
        let mut collapsed = folds.collapsed_ranges().copied().collect::<Vec<_>>();
        collapsed.sort_by_key(|range| (range.start_line, range.end_line));
        if collapsed != self.collapsed_ranges {
            return false;
        }
        if first_line > last_line || first_line >= self.line_count {
            return true;
        }

        let last_line = last_line.min(self.line_count - 1);
        let first_row = self
            .visible_lines
            .partition_point(|line| *line < first_line);
        let end_row = self
            .visible_lines
            .partition_point(|line| *line <= last_line);
        if first_row == end_row {
            return true;
        }

        let mut replacement_lines = Vec::new();
        let mut replacement_segments = Vec::new();
        let mut replacement_starts = Vec::new();
        for line in first_line..=last_line {
            if self.document_line_to_visible_row(line).is_none() {
                continue;
            }
            let Some(text) = buffer.line(line) else {
                return false;
            };
            replacement_starts.push((line, replacement_segments.len()));
            append_wrapped_segments_with_fold(
                &mut replacement_segments,
                &text,
                self.wrapped_line_columns(line, columns),
                self.tab_width,
                line,
                self.wrap_measurement,
                context,
                self.collapsed_delimiter(line, folds),
            );
            replacement_lines.resize(replacement_segments.len(), line);
        }

        let removed_rows = end_row - first_row;
        let inserted_rows = replacement_segments.len();
        self.wrapped_segments
            .as_mut()
            .expect("wrapped viewport segments")
            .splice(first_row..end_row, replacement_segments);
        self.visible_lines
            .splice(first_row..end_row, replacement_lines);
        for (line, offset) in replacement_starts {
            self.document_to_visible[line] = Some(first_row + offset);
        }
        if inserted_rows != removed_rows {
            for row in self.document_to_visible[last_line + 1..]
                .iter_mut()
                .flatten()
            {
                *row = *row - removed_rows + inserted_rows;
            }
        }

        true
    }

    /// Synchronizes an unfolded viewport after text has only been appended.
    ///
    /// Loading documents do not expose folds until their text index is
    /// complete. Extending the identity mapping avoids rebuilding every
    /// previously loaded line for each streamed chunk.
    ///
    /// This method explicitly removes wrapping, because a line count alone
    /// cannot reflow changed text. Wrapped append-only loads should use
    /// [`Self::sync_unfolded_wrapped_buffer`] instead.
    pub fn sync_unfolded_line_count(&mut self, line_count: usize) {
        if self.wrap_columns.is_some()
            || !self.collapsed_ranges.is_empty()
            || line_count < self.line_count
            || self.visible_lines.len() != self.line_count
            || self.document_to_visible.len() != self.line_count
        {
            self.visible_lines = (0..line_count).collect();
            self.document_to_visible = (0..line_count).map(Some).collect();
            self.line_count = line_count;
            self.wrap_columns = None;
            self.wrapped_segments = None;
            self.collapsed_ranges.clear();
            self.fold_indicator_columns = 5;
            self.wrap_measurement = None;
            self.projections.clear();
            self.projection_owners.clear();
            return;
        }

        for line in self.line_count..line_count {
            self.visible_lines.push(line);
            self.document_to_visible.push(Some(line));
        }
        self.line_count = line_count;
    }

    /// Updates an unfolded viewport after text has only been appended.
    ///
    /// Only the previously final logical line and newly appended lines are
    /// measured. Completed logical lines are not measured again for each
    /// streamed chunk.
    pub fn sync_unfolded_wrapped_buffer(&mut self, buffer: &EditorBuffer) {
        self.sync_unfolded_wrapped_buffer_with_context(buffer, None);
    }

    pub(crate) fn sync_unfolded_wrapped_buffer_with_context(
        &mut self,
        buffer: &EditorBuffer,
        context: Option<&CjkContext>,
    ) {
        let Some(columns) = self.wrap_columns else {
            self.sync_unfolded_line_count(buffer.line_count());
            return;
        };

        if buffer.line_count() < self.line_count || !self.collapsed_ranges.is_empty() {
            *self = Self::new_wrapped_with_measurement(
                buffer,
                &FoldModel::default(),
                columns,
                self.tab_width,
                self.fold_indicator_columns,
                self.wrap_measurement,
                context,
            );
            return;
        }

        let first_line = self.line_count.saturating_sub(1);
        let first_row = self.document_line_to_visible_row(first_line).unwrap_or(0);
        self.visible_lines.truncate(first_row);
        if let Some(segments) = &mut self.wrapped_segments {
            segments.truncate(first_row);
        }
        self.line_count = buffer.line_count();
        self.document_to_visible.resize(self.line_count, None);

        for line in first_line..self.line_count {
            if let Some(text) = buffer.line(line) {
                self.append_wrapped_line(line, &text, columns, None, context);
            }
        }
    }

    fn append_wrapped_line(
        &mut self,
        line: usize,
        text: &str,
        columns: usize,
        delimiter: Option<FoldDelimiter>,
        context: Option<&CjkContext>,
    ) {
        let Some(segments) = &mut self.wrapped_segments else {
            return;
        };

        self.document_to_visible[line] = Some(self.visible_lines.len());
        append_wrapped_segments_with_fold(
            segments,
            text,
            columns,
            self.tab_width,
            line,
            self.wrap_measurement,
            context,
            delimiter,
        );
        self.visible_lines.resize(segments.len(), line);
    }

    fn collapsed_delimiter(&self, line: usize, folds: &FoldModel) -> Option<FoldDelimiter> {
        let end = self
            .collapsed_ranges
            .partition_point(|range| range.start_line <= line);
        self.collapsed_ranges
            .get(end.checked_sub(1)?)
            .filter(|range| range.start_line == line)
            .and_then(|range| folds.delimiter(*range))
    }

    fn wrapped_line_columns(&self, line: usize, columns: usize) -> usize {
        // A collapsed header ends with an interactive fold placeholder. Reserve
        // its width during reflow so it stays inside the text viewport.
        if !self.projections.contains_key(&line)
            && self
                .collapsed_ranges
                .binary_search_by_key(&line, |range| range.start_line)
                .is_ok()
        {
            columns.saturating_sub(self.fold_indicator_columns).max(1)
        } else {
            columns.max(1)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn append_wrapped_segments_with_fold(
    segments: &mut Vec<RowSegment>,
    text: &str,
    columns: usize,
    tab_width: usize,
    line: usize,
    measurement: Option<WrapMeasurement>,
    context: Option<&CjkContext>,
    delimiter: Option<FoldDelimiter>,
) {
    let wrapped_text = delimiter
        .filter(|delimiter| {
            text.get(delimiter.opening_column..)
                .and_then(|tail| tail.strip_prefix(delimiter.opening))
                .is_some_and(|tail| tail.trim().is_empty())
        })
        .map_or(text, |delimiter| {
            &text[..delimiter.opening_column + delimiter.opening.len_utf8()]
        });
    append_wrapped_segments_with_measurement(
        segments,
        wrapped_text,
        columns,
        tab_width,
        line,
        measurement,
        context,
        &[],
    );
    if wrapped_text.len() < text.len() {
        let last = segments.last_mut().expect("wrapped terminal opener");
        last.end_column = text.len();
        last.end_visual_column = visual_column_for(text, text.len(), tab_width);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_wrapped_segments_with_measurement(
    segments: &mut Vec<RowSegment>,
    text: &str,
    columns: usize,
    tab_width: usize,
    line: usize,
    measurement: Option<WrapMeasurement>,
    context: Option<&CjkContext>,
    atomic_ranges: &[Range<usize>],
) {
    let Some(measurement) = measurement.filter(|_| !text.is_ascii()) else {
        append_wrapped_segments_atomic(segments, text, columns, tab_width, atomic_ranges);
        return;
    };
    let measured = MeasuredWrapLine::new(text, tab_width, line, context, measurement);
    let measured_widths = measured.grapheme_widths();
    let mut widths = Vec::with_capacity(measured_widths.len());
    let mut unit_width = 0.0;
    let mut word_boundaries = text
        .split_word_bound_indices()
        .map(|(offset, word)| offset + word.len())
        .peekable();
    // Keep source byte positions and logical tab columns independent of pixel
    // advances. Every consumer continues to use this same fragment map.
    let mut boundaries = vec![(0usize, 0usize, false)];
    let mut visual_column = 0usize;
    let mut atomic_index = 0;
    for (index, (column, grapheme)) in text.grapheme_indices(true).enumerate() {
        for ch in grapheme.chars() {
            visual_column = visual_column.saturating_add(visual_width_with_tab_width(
                ch,
                visual_column,
                tab_width,
            ));
        }
        let end = column + grapheme.len();
        unit_width += measured_widths[index].1;
        let mut can_break = false;
        while word_boundaries
            .peek()
            .is_some_and(|boundary| *boundary <= end)
        {
            can_break |= word_boundaries.next() == Some(end);
        }
        can_break |= grapheme
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_punctuation() && ch != '_');
        while atomic_ranges
            .get(atomic_index)
            .is_some_and(|range| end > range.end)
        {
            atomic_index += 1;
        }
        if atomic_ranges
            .get(atomic_index)
            .is_some_and(|range| range.start < end && end < range.end)
        {
            continue;
        }
        widths.push(unit_width);
        unit_width = 0.0;
        boundaries.push((end, visual_column, can_break));
    }
    debug_assert_eq!(widths.len() + 1, boundaries.len());
    let limit = columns.max(1) as f32 * measurement.character_width();
    let final_boundary = boundaries.len() - 1;
    let mut start = 0;
    while start < final_boundary {
        let mut fit = start;
        let mut estimated_width = 0.0;
        while fit < final_boundary {
            let next_width = estimated_width + widths[fit];
            if fit > start && next_width > limit + 0.01 {
                break;
            }
            estimated_width = next_width;
            fit += 1;
        }
        let width = |end: usize| {
            measured.width(boundaries[start].0, boundaries[end].0, boundaries[start].1)
        };
        // Verify the actual row fragment: shaping can change at a soft break.
        // One oversized grapheme is allowed so even a tiny viewport progresses.
        while fit > start + 1 && width(fit) > limit + 0.01 {
            fit -= 1;
        }
        while fit < final_boundary && width(fit + 1) <= limit + 0.01 {
            fit += 1;
        }
        let preferred_break = |fit: usize| {
            if fit == final_boundary {
                fit
            } else {
                (start + 1..=fit)
                    .rev()
                    .find(|&index| boundaries[index].2)
                    .unwrap_or(fit)
            }
        };
        let mut end = preferred_break(fit);
        while end > start + 1 && width(end) > limit + 0.01 {
            end = preferred_break(end - 1);
        }
        segments.push(RowSegment {
            start_column: boundaries[start].0,
            end_column: boundaries[end].0,
            start_visual_column: boundaries[start].1,
            end_visual_column: boundaries[end].1,
            is_last: end == final_boundary,
        });
        start = end;
    }
}

fn append_wrapped_segments_atomic(
    segments: &mut Vec<RowSegment>,
    text: &str,
    columns: usize,
    tab_width: usize,
    atomic_ranges: &[Range<usize>],
) {
    let mut word_boundaries = text
        .split_word_bound_indices()
        .map(|(offset, word)| offset + word.len())
        .peekable();
    let mut start_column = 0;
    let mut start_visual_column = 0;
    let mut visual_column: usize = 0;
    let mut last_boundary = None;

    // Each grapheme and each Unicode word boundary is visited once. A wrap
    // can reuse its latest boundary without rescanning the remainder of a
    // long word, which is especially important for minified source files.
    let mut atomic_index = 0;
    for (column, grapheme) in text.grapheme_indices(true) {
        while atomic_ranges
            .get(atomic_index)
            .is_some_and(|range| column >= range.end)
        {
            atomic_index += 1;
        }
        if atomic_ranges
            .get(atomic_index)
            .is_some_and(|range| range.start < column && column < range.end)
        {
            continue;
        }
        let grapheme = atomic_ranges
            .get(atomic_index)
            .filter(|range| range.start == column)
            .map_or(grapheme, |range| &text[range.clone()]);
        let mut end_visual_column = visual_column;
        for ch in grapheme.chars() {
            end_visual_column = end_visual_column.saturating_add(visual_width_with_tab_width(
                ch,
                end_visual_column,
                tab_width,
            ));
        }

        while column > start_column
            && end_visual_column.saturating_sub(start_visual_column) > columns
        {
            let (end_column, end_visual_column) =
                last_boundary.take().unwrap_or((column, visual_column));
            segments.push(RowSegment {
                start_column,
                end_column,
                start_visual_column,
                end_visual_column,
                is_last: false,
            });
            start_column = end_column;
            start_visual_column = end_visual_column;
        }

        visual_column = end_visual_column;
        let end_column = column + grapheme.len();
        while word_boundaries
            .peek()
            .is_some_and(|boundary| *boundary <= end_column)
        {
            if word_boundaries.next() == Some(end_column) {
                last_boundary = Some((end_column, visual_column));
            }
        }
        // Unicode word segmentation deliberately keeps punctuation such as
        // the dot in a qualified identifier inside a word. For source text,
        // these separators are also useful wrap opportunities.
        if grapheme
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_punctuation() && ch != '_')
        {
            last_boundary = Some((end_column, visual_column));
        }
    }

    segments.push(RowSegment {
        start_column,
        end_column: text.len(),
        start_visual_column,
        end_visual_column: visual_column,
        is_last: true,
    });
}
