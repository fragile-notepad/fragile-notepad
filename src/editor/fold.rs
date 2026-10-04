use std::{
    cmp::Reverse,
    collections::{HashSet, hash_set},
};

use super::buffer::EditorBuffer;
use super::syntax_hints::{StringHint, SyntaxHintSet, SyntaxHints};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,
}

impl FoldRange {
    pub const fn new(start_line: usize, end_line: usize) -> Self {
        Self {
            start_line,
            end_line,
        }
    }

    pub fn is_foldable(self) -> bool {
        self.end_line > self.start_line
    }

    pub fn contains_hidden_line(self, line: usize) -> bool {
        line > self.start_line && line <= self.end_line
    }
}

pub trait FoldProvider {
    fn compute_folds(&self, buffer: &EditorBuffer) -> Vec<FoldRange>;
}

#[derive(Debug, Clone, Default)]
pub struct FoldModel {
    ranges: Vec<FoldRange>,
    collapsed: HashSet<FoldRange>,
    visibility_revision: u64,
}

impl FoldModel {
    pub fn new(ranges: Vec<FoldRange>) -> Self {
        Self::with_collapsed(ranges, HashSet::new())
    }

    pub fn with_collapsed(ranges: Vec<FoldRange>, mut collapsed: HashSet<FoldRange>) -> Self {
        let ranges = normalize_ranges(ranges);
        if !collapsed.is_empty() {
            let available = ranges.iter().copied().collect::<HashSet<_>>();
            collapsed.retain(|range| available.contains(range));
        }

        Self {
            ranges,
            collapsed,
            visibility_revision: 0,
        }
    }

    pub fn recompute(&mut self, ranges: Vec<FoldRange>) {
        let revision = self.visibility_revision;
        let previous_count = self.collapsed.len();
        let collapsed = std::mem::take(&mut self.collapsed);
        *self = Self::with_collapsed(ranges, collapsed);
        self.visibility_revision =
            revision.wrapping_add(u64::from(previous_count != self.collapsed.len()));
    }

    pub(crate) fn visibility_revision(&self) -> u64 {
        self.visibility_revision
    }

    pub fn ranges(&self) -> &[FoldRange] {
        &self.ranges
    }

    pub fn collapsed_ranges(&self) -> hash_set::Iter<'_, FoldRange> {
        self.collapsed.iter()
    }

    pub fn range_at_or_parent(&self, line: usize) -> Option<FoldRange> {
        self.ranges
            .iter()
            .copied()
            .filter(|range| range.start_line == line)
            .max_by_key(|range| range.end_line)
            .or_else(|| {
                self.ranges
                    .iter()
                    .copied()
                    .filter(|range| range.contains_hidden_line(line))
                    .max_by_key(|range| (range.start_line, Reverse(range.end_line)))
            })
    }

    pub fn is_collapsed(&self, range: FoldRange) -> bool {
        self.collapsed.contains(&range)
    }

    pub fn set_collapsed(&mut self, range: FoldRange, collapsed: bool) -> bool {
        if self
            .ranges
            .binary_search_by_key(&(range.start_line, range.end_line), |range| {
                (range.start_line, range.end_line)
            })
            .is_err()
        {
            return false;
        }

        let changed = if collapsed {
            self.collapsed.insert(range)
        } else {
            self.collapsed.remove(&range)
        };
        self.visibility_revision = self.visibility_revision.wrapping_add(u64::from(changed));
        changed
    }

    pub fn toggle(&mut self, range: FoldRange) -> bool {
        if self.is_collapsed(range) {
            self.set_collapsed(range, false)
        } else {
            self.set_collapsed(range, true)
        }
    }

    pub fn set_all_collapsed(&mut self, collapsed: bool) -> bool {
        if collapsed {
            // Collapsed ranges are a subset of the unique available ranges.
            let changed = self.collapsed.len() != self.ranges.len();

            if changed {
                self.collapsed = self.ranges.iter().copied().collect();
                self.visibility_revision = self.visibility_revision.wrapping_add(1);
            }

            changed
        } else {
            let changed = !self.collapsed.is_empty();
            self.collapsed.clear();
            self.visibility_revision = self.visibility_revision.wrapping_add(u64::from(changed));
            changed
        }
    }

    pub fn collapsed_covering(&self, line: usize) -> Option<FoldRange> {
        self.collapsed
            .iter()
            .copied()
            .filter(|range| range.contains_hidden_line(line))
            .min_by_key(|range| (range.start_line, Reverse(range.end_line)))
    }

    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.collapsed_covering(line).is_some()
    }
}

#[derive(Debug, Clone)]
pub struct IndentBraceFoldProvider {
    indent_width: usize,
    syntax_token: String,
    hints: SyntaxHints,
}

impl IndentBraceFoldProvider {
    pub const DEFAULT_INDENT_WIDTH: usize = 4;

    pub fn new(indent_width: usize) -> Self {
        Self::for_syntax(indent_width, "txt")
    }

    pub fn for_syntax(indent_width: usize, syntax_token: impl Into<String>) -> Self {
        let syntax_token = syntax_token.into();
        Self {
            indent_width,
            hints: SyntaxHintSet::load().hints_for(&syntax_token),
            syntax_token,
        }
    }

    fn indent_width(&self) -> usize {
        self.indent_width.max(1)
    }

    pub fn syntax_token(&self) -> &str {
        &self.syntax_token
    }
}

impl Default for IndentBraceFoldProvider {
    fn default() -> Self {
        Self::new(Self::DEFAULT_INDENT_WIDTH)
    }
}

impl FoldProvider for IndentBraceFoldProvider {
    fn compute_folds(&self, buffer: &EditorBuffer) -> Vec<FoldRange> {
        let mut ranges = indentation_folds(buffer, self.indent_width());
        ranges.extend(brace_folds(buffer, &self.hints));
        normalize_ranges(ranges)
    }
}

fn indentation_folds(buffer: &EditorBuffer, indent_width: usize) -> Vec<FoldRange> {
    let line_count = buffer.line_count();
    let mut ranges = Vec::new();
    let mut stack: Vec<IndentFoldCandidate> = Vec::new();
    let mut previous_nonblank_line = None;

    for line_index in 0..line_count {
        let Some(line) = buffer.line(line_index) else {
            continue;
        };

        if line.trim().is_empty() {
            continue;
        }

        let indent = indentation_width(&line, indent_width);
        while stack
            .last()
            .is_some_and(|candidate| candidate.indent >= indent)
        {
            finish_indent_candidate(&mut ranges, stack.pop(), previous_nonblank_line);
        }

        if let Some(parent) = stack.last_mut() {
            parent.has_deeper_line = true;
        }

        stack.push(IndentFoldCandidate {
            line: line_index,
            indent,
            has_deeper_line: false,
        });
        previous_nonblank_line = Some(line_index);
    }

    while let Some(candidate) = stack.pop() {
        finish_indent_candidate(&mut ranges, Some(candidate), previous_nonblank_line);
    }

    ranges
}

#[derive(Debug, Clone, Copy)]
struct IndentFoldCandidate {
    line: usize,
    indent: usize,
    has_deeper_line: bool,
}

fn finish_indent_candidate(
    ranges: &mut Vec<FoldRange>,
    candidate: Option<IndentFoldCandidate>,
    end_line: Option<usize>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    let Some(end_line) = end_line else {
        return;
    };

    if candidate.has_deeper_line && end_line > candidate.line {
        ranges.push(FoldRange::new(candidate.line, end_line));
    }
}

fn brace_folds(buffer: &EditorBuffer, hints: &SyntaxHints) -> Vec<FoldRange> {
    let mut ranges = Vec::new();
    let mut stack = Vec::new();
    let mut syntax = BraceSyntax::Code;

    for line_index in 0..buffer.line_count() {
        let Some(line) = buffer.line(line_index) else {
            continue;
        };

        let mut index = 0;
        while index < line.len() {
            match syntax.clone() {
                BraceSyntax::Code => {
                    if hints
                        .line_comments
                        .iter()
                        .any(|comment| line[index..].starts_with(comment))
                    {
                        break;
                    }

                    if let Some(comment) = hints
                        .block_comments
                        .iter()
                        .find(|comment| line[index..].starts_with(&comment.open))
                    {
                        syntax = BraceSyntax::BlockComment {
                            close: comment.close.clone(),
                        };
                        index += comment.open.len();
                        continue;
                    }

                    if let Some((hashes, raw_string_len)) = hints
                        .raw_strings
                        .then(|| raw_string_start(&line, index))
                        .flatten()
                    {
                        syntax = BraceSyntax::RawString { hashes };
                        index += raw_string_len;
                        continue;
                    }

                    if let Some(delimiter) = quoted_string_delimiter(&line, index, hints) {
                        syntax = BraceSyntax::QuotedString {
                            close: delimiter.close.clone(),
                            escape: delimiter.escape,
                            escaped: false,
                        };
                        index += delimiter.open.len();
                        continue;
                    }
                }
                BraceSyntax::BlockComment { close } => {
                    if line[index..].starts_with(&close) {
                        syntax = BraceSyntax::Code;
                        index += close.len();
                    } else {
                        index += next_char_len(&line, index);
                    }
                    continue;
                }
                BraceSyntax::QuotedString {
                    close,
                    escape,
                    escaped,
                } => {
                    if escaped {
                        syntax = BraceSyntax::QuotedString {
                            close,
                            escape,
                            escaped: false,
                        };
                    } else if escape
                        .as_deref()
                        .is_some_and(|escape| line[index..].starts_with(escape))
                    {
                        syntax = BraceSyntax::QuotedString {
                            close,
                            escape,
                            escaped: true,
                        };
                    } else if line[index..].starts_with(&close) {
                        syntax = BraceSyntax::Code;
                    }

                    index += next_char_len(&line, index);
                    continue;
                }
                BraceSyntax::RawString { hashes } => {
                    if raw_string_end(&line, index, hashes).is_some() {
                        syntax = BraceSyntax::Code;
                        index += hashes + 1;
                    } else {
                        index += next_char_len(&line, index);
                    }
                    continue;
                }
            }

            let ch = line[index..]
                .chars()
                .next()
                .expect("index should be on a character boundary");
            match ch {
                '{' | '[' | '(' => stack.push((ch, line_index)),
                '}' | ']' | ')' => {
                    let Some(index) = stack
                        .iter()
                        .rposition(|(open, _)| matching_close(*open) == ch)
                    else {
                        index += ch.len_utf8();
                        continue;
                    };
                    let (_, start_line) = stack.remove(index);

                    if line_index > start_line {
                        ranges.push(FoldRange::new(start_line, line_index));
                    }
                }
                _ => {}
            }

            index += ch.len_utf8();
        }
    }

    ranges
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BraceSyntax {
    Code,
    BlockComment {
        close: String,
    },
    QuotedString {
        close: String,
        escape: Option<String>,
        escaped: bool,
    },
    RawString {
        hashes: usize,
    },
}

fn quoted_string_delimiter(line: &str, index: usize, hints: &SyntaxHints) -> Option<StringHint> {
    hints
        .strings
        .iter()
        .find(|hint| {
            line[index..].starts_with(&hint.open)
                && (!hint.requires_closing_on_line
                    || line
                        .get(index + hint.open.len()..)
                        .is_some_and(|tail| tail.contains(&hint.close)))
        })
        .cloned()
}

fn raw_string_start(line: &str, index: usize) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    if bytes.get(index) != Some(&b'r') {
        return None;
    }

    if index > 0
        && bytes
            .get(index - 1)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }

    let mut cursor = index + 1;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }

    (bytes.get(cursor) == Some(&b'"')).then_some((cursor - index - 1, cursor - index + 1))
}

fn raw_string_end(line: &str, index: usize, hashes: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if bytes.get(index) != Some(&b'"') {
        return None;
    }

    let end = index + hashes + 1;
    (end <= line.len() && bytes.get(index + 1..end)?.iter().all(|byte| *byte == b'#'))
        .then_some(end)
}

fn next_char_len(line: &str, index: usize) -> usize {
    line[index..]
        .chars()
        .next()
        .map(char::len_utf8)
        .unwrap_or(1)
}

fn matching_close(open: char) -> char {
    match open {
        '{' => '}',
        '[' => ']',
        '(' => ')',
        _ => open,
    }
}

fn indentation_width(line: &str, indent_width: usize) -> usize {
    line.chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { indent_width } else { 1 })
        .sum()
}

fn normalize_ranges(mut ranges: Vec<FoldRange>) -> Vec<FoldRange> {
    ranges.retain(|range| range.is_foldable());
    ranges.sort_by_key(|range| (range.start_line, range.end_line));
    ranges.dedup();
    ranges
}
