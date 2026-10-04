//! Lexical shielding. Offsets always refer to UTF-8 bytes in the original source.
use super::schema::{RawHeredoc, RawRegexLiteral};
use super::{OutlineLexicalPlan, OutlineStringPlan};
use regex::Regex;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OutlineCodeMask {
    code: Vec<bool>,
    literal_starts: Vec<usize>,
    literal_ranges: Vec<(usize, usize)>,
    group_opens: HashMap<usize, usize>,
}

impl OutlineCodeMask {
    pub(super) fn new(text: &str, plan: &OutlineLexicalPlan) -> Self {
        let mut mask = Self {
            code: vec![true; text.len()],
            literal_starts: Vec::new(),
            literal_ranges: Vec::new(),
            group_opens: HashMap::new(),
        };
        let regex_literals = plan
            .regex_literals
            .iter()
            .map(|literal| {
                (
                    literal,
                    literal
                        .prefix_pattern
                        .as_deref()
                        .and_then(|pattern| Regex::new(pattern).ok()),
                )
            })
            .collect::<Vec<_>>();
        let heredocs = plan
            .heredocs
            .iter()
            .filter_map(|heredoc| {
                Regex::new(&format!("^(?:{})", heredoc.prefix_pattern))
                    .ok()
                    .map(|pattern| (heredoc, pattern))
            })
            .collect::<Vec<_>>();
        let line_skips = plan
            .line_skip_patterns
            .iter()
            .filter_map(|pattern| Regex::new(&format!("^(?:{pattern})")).ok())
            .collect::<Vec<_>>();
        let mut pending_heredocs: VecDeque<PendingHeredoc> = VecDeque::new();
        let heredoc_closers = (!heredocs.is_empty()).then(|| HeredocClosers::new(text));
        let mut cursor = 0;
        let mut parentheses = Vec::new();
        let mut braces = Vec::new();
        while cursor < text.len() {
            if let Some(pending) =
                pending_heredocs.pop_front_if(|pending| cursor >= pending.body_start)
            {
                let end = heredoc_closers
                    .as_ref()
                    .unwrap()
                    .end(text, cursor, &pending)
                    .unwrap_or(text.len());
                mask.shield(cursor, end, true);
                cursor = end;
                continue;
            }
            let tail = &text[cursor..];
            let mut literal = false;
            let end = if let Some(end) = directive_end(text, cursor, &mask, &line_skips) {
                Some(end)
            } else if let Some(open) = plan
                .line_comments
                .iter()
                .filter(|open| !open.is_empty() && tail.starts_with(open.as_str()))
                .max_by_key(|open| open.len())
            {
                Some(line_end(text, cursor + open.len()))
            } else if let Some(comment) = plan
                .block_comments
                .iter()
                .filter(|comment| {
                    !comment.open.is_empty()
                        && !comment.close.is_empty()
                        && tail.starts_with(&comment.open)
                })
                .max_by_key(|comment| comment.open.len())
            {
                Some(block_comment_end(text, cursor, comment))
            } else if let Some(end) = raw_string_end(text, cursor, plan) {
                literal = true;
                Some(end)
            } else if let Some((end, pending)) =
                heredoc_start(text, cursor, &mask, &heredocs, heredoc_closers.as_ref())
            {
                pending_heredocs.push_back(pending);
                literal = true;
                Some(end)
            } else if let Some(end) = regex_literal_end(text, cursor, &mask, &regex_literals) {
                literal = true;
                Some(end)
            } else if let Some((_, end)) = plan
                .strings
                .iter()
                .filter(|string| {
                    !string.open.is_empty()
                        && !string.close.is_empty()
                        && tail.starts_with(&string.open)
                        && (!string.single_quote_literals
                            || single_quoted_literal_starts(text, cursor, string))
                })
                .filter_map(|string| {
                    let end = quoted_string_end(text, cursor, string);
                    (!string.requires_closing_on_line || end.is_some()).then_some((string, end))
                })
                .max_by_key(|(string, _)| string.open.len())
            {
                literal = true;
                Some(end.unwrap_or(text.len()))
            } else {
                None
            };

            if let Some(end) = end {
                mask.shield(cursor, end, literal);
                cursor = end;
            } else {
                let ch = text[cursor..].chars().next().unwrap();
                let stack = match ch {
                    _ if regex_literals.is_empty() && heredocs.is_empty() => None,
                    '(' | ')' => Some(&mut parentheses),
                    '{' | '}' => Some(&mut braces),
                    _ => None,
                };
                if let Some(stack) = stack {
                    if matches!(ch, '(' | '{') {
                        stack.push(cursor);
                    } else if let Some(open) = stack.pop() {
                        mask.group_opens.insert(cursor, open);
                    }
                }
                cursor += ch.len_utf8();
            }
        }
        mask.shield_opaque_blocks(text, plan);
        mask
    }

    fn shield(&mut self, start: usize, end: usize, literal: bool) {
        if literal {
            self.literal_starts.push(start);
            self.literal_ranges.push((start, end));
        }
        self.code[start..end].fill(false);
    }

    fn shield_opaque_blocks(&mut self, text: &str, plan: &OutlineLexicalPlan) {
        for block in &plan.opaque_blocks {
            let Ok(pattern) = Regex::new(&block.prefix_pattern) else {
                continue;
            };
            for header in pattern.find_iter(text) {
                if !self.is_code(header.start())
                    || plan
                        .identifier_prefix
                        .as_deref()
                        .is_some_and(|prefix| text[..header.start()].ends_with(prefix))
                {
                    continue;
                }
                let mut open = header.end();
                while open < text.len() {
                    let ch = text[open..].chars().next().unwrap();
                    if self.is_code(open) && !ch.is_whitespace() {
                        break;
                    }
                    open += ch.len_utf8();
                }
                if block.open.is_empty()
                    || block.close.is_empty()
                    || !text[open..].starts_with(&block.open)
                {
                    continue;
                }
                let mut cursor = open + block.open.len();
                let mut depth = 1usize;
                while cursor < text.len() {
                    if self.is_code(cursor) && text[cursor..].starts_with(&block.open) {
                        depth += 1;
                        cursor += block.open.len();
                    } else if self.is_code(cursor) && text[cursor..].starts_with(&block.close) {
                        depth -= 1;
                        cursor += block.close.len();
                        if depth == 0 {
                            break;
                        }
                    } else {
                        cursor += char_len(text, cursor);
                    }
                }
                // Comments and literals were shielded first, so their apparent
                // delimiters cannot close an opaque declaration template.
                self.code[header.start()..cursor].fill(false);
            }
        }
    }

    fn is_literal(&self, offset: usize) -> bool {
        let index = self
            .literal_ranges
            .partition_point(|(start, _)| *start <= offset);
        index > 0 && offset < self.literal_ranges[index - 1].1
    }

    pub(super) fn is_code(&self, offset: usize) -> bool {
        self.code.get(offset).copied().unwrap_or(false)
    }

    pub(super) fn is_code_range(&self, start: usize, end: usize) -> bool {
        start < end
            && self
                .code
                .get(start..end)
                .is_some_and(|code| code.iter().all(|code| *code))
    }

    pub(super) fn is_literal_start(&self, offset: usize) -> bool {
        self.literal_starts.binary_search(&offset).is_ok()
    }
}

fn directive_end(
    text: &str,
    start: usize,
    mask: &OutlineCodeMask,
    patterns: &[Regex],
) -> Option<usize> {
    if patterns.is_empty() {
        return None;
    }
    let end = patterns
        .iter()
        .filter_map(|pattern| pattern.find(&text[start..]))
        .filter(|matched| matched.start() == 0 && !matched.is_empty())
        .map(|matched| start + matched.end())
        .max()?;
    // Only inspect the prefix once a directive opener matches. Ordinary source
    // characters must not repeatedly walk back through a potentially long line.
    let line_start = text[..start]
        .rfind(['\r', '\n'])
        .map_or(0, |offset| offset + 1);
    if text[line_start..start]
        .char_indices()
        .any(|(relative, ch)| !ch.is_whitespace() && mask.is_code(line_start + relative))
    {
        return None;
    }
    Some(end)
}

fn previous_significant(
    text: &str,
    mask: &OutlineCodeMask,
    mut before: usize,
) -> Option<(usize, char)> {
    while let Some(ch) = text[..before].chars().next_back() {
        before -= ch.len_utf8();
        if ch.is_whitespace() {
            continue;
        }
        if mask.is_literal(before) || mask.is_code(before) {
            return Some((before, ch));
        }
    }
    None
}

// A configured expression-literal rule sees the previous significant token,
// rather than a growing source prefix. Comments are transparent, and literals
// are operands. Group owners distinguish a control header from a function call.
fn lexical_context(
    text: &str,
    mask: &OutlineCodeMask,
    before: usize,
    expression_prefix: Option<&Regex>,
) -> String {
    let Some((start, ch)) = previous_significant(text, mask, before) else {
        return String::new();
    };
    if mask.is_literal(start) {
        return String::from("<literal>");
    }
    if ch.is_alphanumeric() || matches!(ch, '_' | '$') {
        return lexical_token_before(text, mask, before);
    }
    if matches!(ch, ')' | '}') {
        let open = if ch == ')' { '(' } else { '{' };
        if let Some(&offset) = mask.group_opens.get(&start) {
            if ch == '}' && group_header_is_expression(text, mask, offset, expression_prefix) {
                return String::from("<literal>");
            }
            // Resolve only the preceding token here; nested groups must
            // not recursively consume stack on deeply nested source.
            let owner = if ch == '}'
                && previous_significant(text, mask, offset)
                    .is_some_and(|(_, previous)| previous == ')')
            {
                // One preceding parameter/control group is sufficient
                // to identify a completed block, with bounded recursion.
                lexical_context(text, mask, offset, expression_prefix)
            } else {
                lexical_token_before(text, mask, offset)
            };
            return format!("{owner}{open}{ch}");
        }
    }
    let end = start + ch.len_utf8();
    if let Some(previous) = text[..start].chars().next_back() {
        let offset = start - previous.len_utf8();
        if mask.is_code(offset) {
            let pair = &text[offset..end];
            if matches!(pair, "=>" | "&&" | "||" | "??" | "++" | "--") {
                return pair.to_owned();
            }
        }
    }
    ch.to_string()
}

fn group_header_is_expression(
    text: &str,
    mask: &OutlineCodeMask,
    mut before: usize,
    expression_prefix: Option<&Regex>,
) -> bool {
    while let Some((offset, ch)) = previous_significant(text, mask, before) {
        if mask.is_literal(offset) {
            // A string in the header is an operand, not a statement boundary.
            let index = mask
                .literal_ranges
                .partition_point(|(start, _)| *start <= offset);
            before = mask.literal_ranges[index - 1].0;
            continue;
        }
        match ch {
            ';' | '{' | '}' => return false,
            '=' | '(' | '[' | ',' | ':' => return true,
            ')' => {
                if let Some(open) = mask.group_opens.get(&offset) {
                    before = *open;
                    continue;
                }
            }
            _ if ch.is_alphanumeric() || matches!(ch, '_' | '$') => {
                let token = lexical_token_before(text, mask, before);
                if expression_prefix.is_some_and(|prefix| prefix.is_match(&token)) {
                    return true;
                }
                before = offset + ch.len_utf8() - token.trim_start_matches('.').len();
                continue;
            }
            _ => {}
        }
        before = offset;
    }
    false
}

fn lexical_token_before(text: &str, mask: &OutlineCodeMask, before: usize) -> String {
    let Some((start, ch)) = previous_significant(text, mask, before) else {
        return String::new();
    };
    if mask.is_literal(start) {
        return String::from("<literal>");
    }
    let mut begin = start;
    if ch.is_alphanumeric() || matches!(ch, '_' | '$') {
        while let Some(previous) = text[..begin].chars().next_back() {
            let offset = begin - previous.len_utf8();
            if !mask.is_code(offset)
                || !(previous.is_alphanumeric() || matches!(previous, '_' | '$'))
            {
                break;
            }
            begin = offset;
        }
    }
    let token = &text[begin..start + ch.len_utf8()];
    if previous_significant(text, mask, begin).is_some_and(|(_, previous)| previous == '.') {
        format!(".{token}")
    } else {
        token.to_owned()
    }
}

fn regex_literal_end(
    text: &str,
    start: usize,
    mask: &OutlineCodeMask,
    literals: &[(&RawRegexLiteral, Option<Regex>)],
) -> Option<usize> {
    for (literal, prefix) in literals {
        if !text[start..].starts_with(&literal.open)
            || literal.open.is_empty()
            || literal.close.is_empty()
        {
            continue;
        }
        if prefix.as_ref().is_some_and(|prefix| {
            !prefix.is_match(&lexical_context(text, mask, start, Some(prefix)))
        }) {
            continue;
        }
        let mut cursor = start + literal.open.len();
        let end = line_end(text, cursor);
        let mut in_class = false;
        while cursor < end {
            let tail = &text[cursor..end];
            if let Some(escape) = literal
                .escape
                .as_deref()
                .filter(|escape| !escape.is_empty() && tail.starts_with(escape))
            {
                cursor += escape.len();
                if cursor < end {
                    cursor += char_len(text, cursor);
                }
            } else if !in_class && tail.starts_with(&literal.close) {
                cursor += literal.close.len();
                // Flags belong to the literal, not to declaration scanning.
                while cursor < text.len() && text.as_bytes()[cursor].is_ascii_alphabetic() {
                    cursor += 1;
                }
                return Some(cursor);
            } else if literal
                .character_class_open
                .as_deref()
                .is_some_and(|open| !open.is_empty() && tail.starts_with(open))
            {
                in_class = true;
                cursor += literal.character_class_open.as_ref().unwrap().len();
            } else if in_class
                && literal
                    .character_class_close
                    .as_deref()
                    .is_some_and(|close| !close.is_empty() && tail.starts_with(close))
            {
                in_class = false;
                cursor += literal.character_class_close.as_ref().unwrap().len();
            } else {
                cursor += char_len(text, cursor);
            }
        }
        // An unfinished expression literal shields its line while the document
        // is being edited, but must not swallow subsequent declarations.
        return Some(end);
    }
    None
}

struct PendingHeredoc {
    delimiter: String,
    indented: bool,
    body_start: usize,
}

fn heredoc_start(
    text: &str,
    start: usize,
    mask: &OutlineCodeMask,
    heredocs: &[(&RawHeredoc, Regex)],
    closers: Option<&HeredocClosers<'_>>,
) -> Option<(usize, PendingHeredoc)> {
    for (heredoc, pattern) in heredocs {
        let Some(captures) = pattern.captures(&text[start..]) else {
            continue;
        };
        let full = captures.get(0)?;
        if full.start() != 0 || full.is_empty() {
            continue;
        }
        let delimiter = pattern
            .capture_names()
            .flatten()
            .filter(|name| *name == "delimiter" || name.starts_with("delimiter_"))
            .find_map(|name| captures.name(name))?;
        let has_indent_capture = pattern
            .capture_names()
            .flatten()
            .any(|name| name == "indent");
        let indented =
            heredoc.indented && (!has_indent_capture || captures.name("indent").is_some());
        let opener_end = start + full.end();
        let pending = PendingHeredoc {
            delimiter: delimiter.as_str().to_owned(),
            indented,
            body_start: after_line(text, opener_end),
        };
        let context = lexical_context(text, mask, start, None);
        // A plain, unquoted opener after an operand can instead be a shift.
        // A matching terminator makes command-argument heredocs unambiguous.
        let plain = delimiter.start() + start == opener_end - delimiter.len();
        if context == "<literal>" || context.chars().next().is_some_and(|ch| ch.is_numeric()) {
            continue;
        }
        if plain
            && !indented
            && context
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric())
            && closers?.end(text, pending.body_start, &pending).is_none()
        {
            continue;
        }
        return Some((opener_end, pending));
    }
    None
}

struct HeredocClosers<'a> {
    exact: HashMap<&'a str, Vec<usize>>,
    indented: HashMap<&'a str, Vec<usize>>,
}

impl<'a> HeredocClosers<'a> {
    fn new(text: &'a str) -> Self {
        let mut closers = Self {
            exact: HashMap::new(),
            indented: HashMap::new(),
        };
        let mut cursor = 0;
        while cursor < text.len() {
            let end = line_end(text, cursor);
            let line = &text[cursor..end];
            if !line.is_empty() {
                closers.exact.entry(line).or_default().push(cursor);
                closers
                    .indented
                    .entry(line.trim_start_matches([' ', '\t']))
                    .or_default()
                    .push(cursor);
            }
            cursor = after_line(text, end);
        }
        closers
    }

    fn end(&self, text: &str, start: usize, pending: &PendingHeredoc) -> Option<usize> {
        let candidates = if pending.indented {
            &self.indented
        } else {
            &self.exact
        }
        .get(pending.delimiter.as_str())?;
        let index = candidates.partition_point(|offset| *offset < start);
        Some(after_line(text, *candidates.get(index)?))
    }
}

fn after_line(text: &str, start: usize) -> usize {
    let mut end = line_end(text, start);
    if let Some(first) = text.as_bytes().get(end).copied() {
        end += 1;
        if matches!(
            (first, text.as_bytes().get(end)),
            (b'\r', Some(b'\n')) | (b'\n', Some(b'\r'))
        ) {
            end += 1;
        }
    }
    end
}

fn block_comment_end(text: &str, start: usize, comment: &super::OutlineBlockCommentPlan) -> usize {
    let mut cursor = start + comment.open.len();
    let mut depth = 1;
    while cursor < text.len() {
        if text[cursor..].starts_with(&comment.close) {
            cursor += comment.close.len();
            depth -= 1;
            if depth == 0 {
                return cursor;
            }
        } else if comment.nested && text[cursor..].starts_with(&comment.open) {
            depth += 1;
            cursor += comment.open.len();
        } else {
            cursor += char_len(text, cursor);
        }
    }
    text.len()
}

fn quoted_string_end(text: &str, start: usize, string: &OutlineStringPlan) -> Option<usize> {
    let mut cursor = start + string.open.len();
    let end = if string.requires_closing_on_line {
        line_end(text, cursor)
    } else {
        text.len()
    };
    while cursor < end {
        if let Some(escape) = string
            .escape
            .as_deref()
            .filter(|escape| !escape.is_empty() && text[cursor..end].starts_with(escape))
        {
            cursor += escape.len();
            if cursor < end {
                cursor += char_len(text, cursor);
            }
        } else if text[cursor..end].starts_with(&string.close) {
            return Some(cursor + string.close.len());
        } else {
            cursor += char_len(text, cursor);
        }
    }
    None
}

fn raw_string_end(text: &str, start: usize, plan: &OutlineLexicalPlan) -> Option<usize> {
    if text[..start].chars().next_back().is_some_and(|ch| {
        plan.word_character_extra.contains(ch)
            || if plan.unicode_word_characters {
                ch.is_alphanumeric()
            } else {
                ch.is_ascii_alphanumeric()
            }
    }) {
        return None;
    }
    for raw in &plan.raw_strings {
        let Some(prefix) = raw
            .prefixes
            .iter()
            .filter(|prefix| !prefix.is_empty() && text[start..].starts_with(prefix.as_str()))
            .max_by_key(|prefix| prefix.len())
        else {
            continue;
        };
        if raw.open.is_empty() || raw.close.is_empty() {
            continue;
        }
        let delimiter_start = start + prefix.len();
        let mut cursor = delimiter_start;
        if let Some(repeat) = raw.repeat.as_deref().filter(|repeat| !repeat.is_empty()) {
            while text[cursor..].starts_with(repeat) {
                cursor += repeat.len();
            }
        } else {
            while cursor < text.len() && !text[cursor..].starts_with(&raw.open) {
                let ch = text[cursor..].chars().next()?;
                if ch.is_whitespace()
                    || raw.forbidden_delimiter_characters.contains(ch)
                    || raw
                        .max_delimiter_length
                        .is_some_and(|limit| cursor - delimiter_start >= limit)
                {
                    break;
                }
                cursor += ch.len_utf8();
            }
        }
        if !text[cursor..].starts_with(&raw.open) {
            continue;
        }
        let delimiter = &text[delimiter_start..cursor];
        if raw
            .max_delimiter_length
            .is_some_and(|limit| delimiter.len() > limit)
        {
            continue;
        }
        let body = cursor + raw.open.len();
        let close = format!("{}{}{}", raw.close, delimiter, raw.suffix);
        return Some(
            text[body..]
                .find(&close)
                .map_or(text.len(), |offset| body + offset + close.len()),
        );
    }
    None
}

fn single_quoted_literal_starts(text: &str, index: usize, string: &OutlineStringPlan) -> bool {
    let start = index + string.open.len();
    if let Some(escape) = string
        .escape
        .as_deref()
        .filter(|escape| !escape.is_empty() && text[start..].starts_with(escape))
    {
        return text[start + escape.len()..line_end(text, start)].contains(&string.close);
    }
    text.get(start + char_len(text, start)..)
        .is_some_and(|tail| tail.starts_with(&string.close))
}

fn line_end(text: &str, start: usize) -> usize {
    text[start..]
        .find(['\r', '\n'])
        .map_or(text.len(), |offset| start + offset)
}

fn char_len(text: &str, start: usize) -> usize {
    text[start..].chars().next().map_or(1, char::len_utf8)
}
