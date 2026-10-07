use std::ops::Range;

use super::{EditorBuffer, EditorPosition, FoldDelimiter, FoldModel, FoldRange};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldProjection {
    pub text: String,
    pub fragments: Vec<ProjectionFragment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionFragment {
    Source {
        display_range: Range<usize>,
        source_start: EditorPosition,
    },
    Placeholder {
        display_range: Range<usize>,
        range: FoldRange,
        delimiter: FoldDelimiter,
    },
}

impl FoldProjection {
    pub fn source_to_display(&self, position: EditorPosition) -> Option<usize> {
        for fragment in &self.fragments {
            if let ProjectionFragment::Source {
                display_range,
                source_start,
            } = fragment
                && position.line == source_start.line
                && position.column >= source_start.column
                && position.column <= source_start.column + display_range.len()
            {
                return Some(display_range.start + position.column - source_start.column);
            }
        }
        for fragment in &self.fragments {
            if let ProjectionFragment::Placeholder {
                display_range,
                range,
                delimiter,
            } = fragment
            {
                if position == EditorPosition::new(range.start_line, delimiter.opening_column) {
                    return Some(display_range.start);
                }
                // A terminal opener's trailing whitespace still belongs to its
                // header. Keep End and virtual-space carets at the folded edge.
                if position.line == range.start_line && position.column > delimiter.opening_column {
                    return Some(display_range.end);
                }
                if position == EditorPosition::new(range.end_line, delimiter.closing_column) {
                    return Some(display_range.end);
                }
            }
        }
        if let Some(ProjectionFragment::Source {
            display_range,
            source_start,
        }) = self.fragments.last()
            && position.line == source_start.line
            && position.column > source_start.column + display_range.len()
        {
            return Some(
                display_range.end + position.column - source_start.column - display_range.len(),
            );
        }
        None
    }

    pub fn display_to_source(&self, column: usize) -> EditorPosition {
        for fragment in &self.fragments {
            if let ProjectionFragment::Placeholder {
                display_range,
                range,
                delimiter,
            } = fragment
            {
                if display_range.contains(&column) {
                    return EditorPosition::new(range.start_line, delimiter.opening_column);
                }
                if column == display_range.end {
                    return EditorPosition::new(range.end_line, delimiter.closing_column);
                }
            }
        }
        for fragment in &self.fragments {
            if let ProjectionFragment::Source {
                display_range,
                source_start,
            } = fragment
                && column >= display_range.start
                && column <= display_range.end
            {
                return EditorPosition::new(
                    source_start.line,
                    source_start.column + column - display_range.start,
                );
            }
        }
        match self.fragments.last() {
            Some(ProjectionFragment::Source {
                display_range,
                source_start,
            }) => EditorPosition::new(
                source_start.line,
                source_start.column + column.saturating_sub(display_range.start),
            ),
            Some(ProjectionFragment::Placeholder {
                range, delimiter, ..
            }) => EditorPosition::new(range.end_line, delimiter.closing_column),
            None => EditorPosition::new(0, 0),
        }
    }

    pub(crate) fn build(
        buffer: &EditorBuffer,
        folds: &FoldModel,
        range: FoldRange,
    ) -> Option<Self> {
        let delimiter = valid_delimiter(buffer, folds, range)?;
        let closing_text = buffer.line(range.end_line)?;
        if closing_text[delimiter.closing_column..].trim().is_empty() {
            return None;
        }

        let mut projection = Self {
            text: String::new(),
            fragments: Vec::new(),
        };
        let mut source = EditorPosition::new(range.start_line, 0);
        let mut next = Some((range, delimiter));
        while let Some((range, delimiter)) = next {
            let header = buffer.line(range.start_line)?;
            projection.append_source(source, header.get(source.column..delimiter.opening_column)?);
            let start = projection.text.len();
            projection.text.push_str(match delimiter.opening {
                '{' => "{...}",
                '[' => "[...]",
                '(' => "(...)",
                _ => return None,
            });
            projection.fragments.push(ProjectionFragment::Placeholder {
                display_range: start..projection.text.len(),
                range,
                delimiter,
            });
            source = EditorPosition::new(range.end_line, delimiter.closing_column);
            let first = folds
                .ranges()
                .partition_point(|range| range.start_line < source.line);
            next = folds.ranges()[first..]
                .iter()
                .copied()
                .take_while(|candidate| candidate.start_line == source.line)
                .filter(|candidate| folds.is_collapsed(*candidate))
                .filter_map(|candidate| {
                    valid_delimiter(buffer, folds, candidate)
                        .filter(|delimiter| delimiter.opening_column >= source.column)
                        .map(|delimiter| (candidate, delimiter))
                })
                .max_by_key(|(candidate, _)| candidate.end_line);
        }
        let closing_text = buffer.line(source.line)?;
        projection.append_source(source, closing_text.get(source.column..)?);
        Some(projection)
    }

    pub(crate) fn final_source_line(&self) -> usize {
        match self.fragments.last() {
            Some(ProjectionFragment::Source { source_start, .. }) => source_start.line,
            Some(ProjectionFragment::Placeholder { range, .. }) => range.end_line,
            None => 0,
        }
    }

    fn append_source(&mut self, source_start: EditorPosition, text: &str) {
        let start = self.text.len();
        self.text.push_str(text);
        self.fragments.push(ProjectionFragment::Source {
            display_range: start..self.text.len(),
            source_start,
        });
    }
}

fn valid_delimiter(
    buffer: &EditorBuffer,
    folds: &FoldModel,
    range: FoldRange,
) -> Option<FoldDelimiter> {
    let delimiter = folds.delimiter(range)?;
    let header = buffer.line(range.start_line)?;
    let tail = header.get(delimiter.opening_column..)?;
    if !tail.strip_prefix(delimiter.opening)?.trim().is_empty() {
        return None;
    }
    let closing = buffer.line(range.end_line)?;
    let close = match delimiter.opening {
        '{' => '}',
        '[' => ']',
        '(' => ')',
        _ => return None,
    };
    (closing
        .as_bytes()
        .get(delimiter.closing_column.checked_sub(1)?)
        == Some(&(close as u8)))
    .then_some(delimiter)
}
