use super::OutlineBodyKind;
use super::fsm::ByteRange;
use super::source::CodeToken;
use super::source::{OutlineSource, SyntaxSymbol};

pub(super) fn matching_code_brace(
    text: &str,
    source: &OutlineSource,
    open: usize,
) -> Option<usize> {
    (source.is_body_open(text, open))
        .then(|| source.matching_delimiter(open))
        .flatten()
}

pub(super) fn matching_end_keyword(
    text: &str,
    source: &OutlineSource,
    start: usize,
) -> Option<usize> {
    let body = source.body(OutlineBodyKind::EndKeyword)?;
    let end_keyword = body.end_keyword.as_deref()?;
    let includes =
        |values: &[String], value: &str| values.iter().any(|candidate| candidate == value);
    let mut depth = 1usize;
    let mut previous: Option<CodeToken> = None;
    let mut loop_header = false;
    for token in source.tokens_from(start).iter().copied() {
        let new_line =
            previous.is_none_or(|previous| text[previous.end..token.start].contains(['\r', '\n']));
        if new_line {
            loop_header = false;
        }
        let token_text = token.text(text);
        let member =
            previous.is_some_and(|previous| includes(&body.member_prefixes, previous.text(text)));
        let statement_start = new_line
            || previous
                .is_some_and(|previous| includes(&body.statement_boundaries, previous.text(text)));
        let opens = includes(&body.block_openers, token_text)
            && !member
            && (!includes(&body.conditional_openers, token_text) || statement_start)
            && (body.loop_body_keyword.as_deref() != Some(token_text) || !loop_header);
        let expression_definition = opens
            && source.expression_rules.iter().any(|rule| {
                if rule.keyword != token_text {
                    return false;
                }
                let end = line_end_offset(text, token.end);
                let name_end = if let Some(pattern) = &rule.name_pattern {
                    let Some(captures) = pattern.captures(&text[token.end..end]) else {
                        return false;
                    };
                    let Some(name) = pattern
                        .capture_names()
                        .flatten()
                        .filter(|name| *name == "name" || name.starts_with("name_"))
                        .find_map(|name| captures.name(name))
                    else {
                        return false;
                    };
                    token.end + name.end()
                } else {
                    let name_start = skip_non_code_whitespace(text, source, token.end);
                    let Some(name) = parse_identifier_range(text, source, name_start) else {
                        return false;
                    };
                    name.end
                };
                let mut cursor = name_end;
                while let Some(candidate) = source
                    .next_token(cursor)
                    .filter(|candidate| candidate.start < end)
                {
                    if source.is_delimiter_open(text, candidate.start) {
                        let Some(close) = source.matching_delimiter(candidate.start) else {
                            return false;
                        };
                        let Some(close_token) = source.next_token(close) else {
                            return false;
                        };
                        cursor = close_token.end;
                        continue;
                    }
                    return candidate.text(text) == rule.marker;
                }
                false
            });
        if opens && !expression_definition {
            depth += 1;
            loop_header = includes(&body.loop_openers, token_text);
        } else if token_text == end_keyword && !member {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(token.end);
            }
        }
        if body.loop_body_keyword.as_deref() == Some(token_text)
            || source.symbol_text(token_text) == SyntaxSymbol::StatementEnd
        {
            loop_header = false;
        }
        previous = Some(token);
    }

    None
}

pub(super) fn signature_start(text: &str, source: &OutlineSource, keyword_offset: usize) -> usize {
    if source.plan.signature_modifiers.is_empty() {
        return keyword_offset;
    }

    let mut start = keyword_offset;
    let mut cursor = keyword_offset;

    while let Some(token) = previous_code_token(text, source, cursor) {
        if source
            .plan
            .signature_modifiers
            .iter()
            .any(|modifier| modifier == token.text(text))
        {
            start = token.start;
            cursor = token.start;
            continue;
        }

        if source.symbol_text(token.text(text)) == SyntaxSymbol::ParametersClose {
            let Some(open) = matching_code_paren_before(text, source, token.start) else {
                break;
            };
            let Some(previous) = previous_code_token(text, source, open) else {
                break;
            };
            if source
                .plan
                .signature_modifiers
                .iter()
                .any(|modifier| modifier == previous.text(text))
            {
                start = previous.start;
                cursor = previous.start;
                continue;
            }
        }

        break;
    }

    start
}

pub(super) fn matching_code_paren_before(
    text: &str,
    source: &OutlineSource,
    close: usize,
) -> Option<usize> {
    (text
        .get(close..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(|ch| source.symbol(ch) == SyntaxSymbol::ParametersClose))
    .then(|| source.matching_delimiter(close))
    .flatten()
}

pub(super) fn matching_code_angle_before(
    text: &str,
    source: &OutlineSource,
    close: usize,
) -> Option<usize> {
    let mut depth = 0usize;
    let close_end = close + text.get(close..)?.chars().next()?.len_utf8();
    for (offset, ch) in text.get(..close_end)?.char_indices().rev() {
        if !source.is_code(offset) {
            continue;
        }

        match source.symbol(ch) {
            SyntaxSymbol::GenericsClose => depth += 1,
            SyntaxSymbol::GenericsOpen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }

    None
}

/// Match a configured generic delimiter without treating operators inside paired
/// parameter, array, or const-expression delimiters as generic delimiters.
pub(super) fn matching_code_angle_after(
    text: &str,
    source: &OutlineSource,
    open: usize,
) -> Option<usize> {
    let first = text.get(open..)?.chars().next()?;
    if source.symbol(first) != SyntaxSymbol::GenericsOpen || !source.is_code(open) {
        return None;
    }
    let mut cursor = open;
    let mut depth = 0usize;
    while cursor < text.len() {
        let ch = text[cursor..].chars().next()?;
        if !source.is_code(cursor) {
            cursor += ch.len_utf8();
            continue;
        }
        if source.is_delimiter_open(text, cursor) {
            let close = source.matching_delimiter(cursor)?;
            if close > cursor {
                cursor = source.next_token(close)?.end;
                continue;
            }
        }
        match source.symbol(ch) {
            SyntaxSymbol::GenericsOpen => depth += 1,
            SyntaxSymbol::GenericsClose => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            SyntaxSymbol::StatementEnd | SyntaxSymbol::BodyClose => return None,
            _ => {}
        }
        cursor += ch.len_utf8();
    }
    None
}

pub(super) fn matching_code_paren_after(
    text: &str,
    source: &OutlineSource,
    open: usize,
) -> Option<usize> {
    (text
        .get(open..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(|ch| source.symbol(ch) == SyntaxSymbol::ParametersOpen))
    .then(|| source.matching_delimiter(open))
    .flatten()
}

pub(super) fn find_next_code_char(
    text: &str,
    source: &OutlineSource,
    mut cursor: usize,
    target: char,
) -> Option<usize> {
    while cursor < text.len() {
        let ch = text[cursor..].chars().next()?;
        if source.is_code(cursor) && ch == target {
            return Some(cursor);
        }
        cursor += ch.len_utf8();
    }

    None
}

pub(super) fn find_keyword_sequence(
    text: &str,
    source: &OutlineSource,
    cursor: usize,
    keywords: &[String],
) -> Option<usize> {
    find_keyword_sequence_in_range(text, source, cursor, text.len(), keywords)
}

pub(super) fn find_keyword_sequence_in_range(
    text: &str,
    source: &OutlineSource,
    mut cursor: usize,
    end: usize,
    keywords: &[String],
) -> Option<usize> {
    let first = keywords.first()?;
    let end = end.min(text.len());

    while let Some(offset) = find_keyword_in_range(text, source, cursor, end, first) {
        if keyword_sequence_end(text, source, offset, keywords).is_some_and(|next| next <= end) {
            return Some(offset);
        }

        cursor = offset + first.len();
    }

    None
}

pub(super) fn keyword_sequence_end(
    text: &str,
    source: &OutlineSource,
    offset: usize,
    keywords: &[String],
) -> Option<usize> {
    let mut tokens = source.tokens_from(offset).iter();
    let first = tokens.next()?;
    if first.start != offset || first.text(text) != keywords.first()? {
        return None;
    }
    let mut end = first.end;
    for keyword in &keywords[1..] {
        let token = tokens.next()?;
        if token.text(text) != keyword {
            return None;
        }
        end = token.end;
    }
    Some(end)
}

pub(super) fn find_keyword_in_range(
    text: &str,
    source: &OutlineSource,
    cursor: usize,
    end: usize,
    keyword: &str,
) -> Option<usize> {
    source
        .tokens_from(cursor)
        .iter()
        .take_while(|token| token.end <= end)
        .find(|token| token.start >= cursor && token.text(text) == keyword)
        .map(|token| token.start)
}

pub(super) fn previous_code_token(
    _text: &str,
    source: &OutlineSource,
    before: usize,
) -> Option<CodeToken> {
    source.previous_token(before)
}

pub(super) fn next_code_token(
    _text: &str,
    source: &OutlineSource,
    cursor: usize,
) -> Option<CodeToken> {
    source.next_token(cursor)
}

pub(super) fn previous_contiguous_code_token(
    text: &str,
    source: &OutlineSource,
    before: usize,
) -> Option<CodeToken> {
    let last = previous_code_char(text, source, before)?;
    let end = last + next_char_len(text, last);
    let mut start = last;
    for (offset, ch) in text[..last].char_indices().rev() {
        if !source.is_code(offset) || ch.is_whitespace() {
            break;
        }
        start = offset;
    }
    Some(CodeToken { start, end })
}

pub(super) fn previous_code_char(
    text: &str,
    source: &OutlineSource,
    before: usize,
) -> Option<usize> {
    text.get(..before)?
        .char_indices()
        .rev()
        .find(|(offset, ch)| source.is_code(*offset) && !ch.is_whitespace())
        .map(|(offset, _)| offset)
}

pub(super) fn skip_non_code_whitespace(
    text: &str,
    source: &OutlineSource,
    mut cursor: usize,
) -> usize {
    while cursor < text.len() {
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        if source.is_code(cursor) && !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }

    cursor
}

pub(super) fn skip_code_whitespace_before(
    text: &str,
    source: &OutlineSource,
    before: usize,
) -> usize {
    let mut cursor = before;

    while let Some(offset) = previous_code_char_including_whitespace(text, source, cursor) {
        let Some(ch) = text[offset..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        cursor = offset;
    }

    cursor
}

pub(super) fn previous_code_char_including_whitespace(
    text: &str,
    source: &OutlineSource,
    before: usize,
) -> Option<usize> {
    text.get(..before)?
        .char_indices()
        .rev()
        .find(|(offset, _)| source.is_code(*offset))
        .map(|(offset, _)| offset)
}

pub(super) fn parse_identifier_range(
    text: &str,
    source: &OutlineSource,
    start: usize,
) -> Option<ByteRange> {
    let mut cursor = start;
    let mut name_start = start;

    if let Some(prefix) = source
        .plan
        .lexical
        .identifier_prefix
        .as_deref()
        .filter(|prefix| !prefix.is_empty() && text[start..].starts_with(prefix))
    {
        cursor += prefix.len();
        name_start = cursor;
    }

    let first = text[cursor..].chars().next()?;
    if !source.is_identifier_start(first) {
        return None;
    }
    cursor += first.len_utf8();

    while let Some(ch) = text.get(cursor..).and_then(|tail| tail.chars().next()) {
        if !source.is_word_char(ch) {
            break;
        }
        cursor += ch.len_utf8();
    }

    Some(ByteRange::new(name_start, cursor))
}

pub(super) fn line_start_offset(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .and_then(|prefix| prefix.rfind(['\n', '\r']))
        .map(|index| {
            if text[index..].starts_with("\r\n") || text[index..].starts_with("\n\r") {
                index + 2
            } else {
                index + 1
            }
        })
        .unwrap_or(0)
}

pub(super) fn next_line_start_offset(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }

    let mut cursor = offset;
    while cursor < text.len() {
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        cursor += ch.len_utf8();
        if ch == '\r' || ch == '\n' {
            if cursor < text.len() {
                let next = text[cursor..].chars().next();
                if matches!((ch, next), ('\r', Some('\n')) | ('\n', Some('\r'))) {
                    cursor += 1;
                }
            }
            return cursor;
        }
    }

    text.len()
}

pub(super) fn line_end_offset(text: &str, offset: usize) -> usize {
    text.get(offset..)
        .and_then(|tail| {
            tail.char_indices()
                .find(|(_, ch)| *ch == '\r' || *ch == '\n')
                .map(|(index, _)| offset + index)
        })
        .unwrap_or(text.len())
}

pub(super) fn line_ending_len_before(text: &str, offset: usize) -> usize {
    if offset >= 2 {
        let pair = text.as_bytes().get(offset - 2..offset);
        if pair == Some(b"\r\n") || pair == Some(b"\n\r") {
            return 2;
        }
    }
    if offset >= 1
        && text
            .as_bytes()
            .get(offset - 1)
            .is_some_and(|byte| *byte == b'\r' || *byte == b'\n')
    {
        return 1;
    }
    0
}

pub(super) fn indentation_before(text: &str, offset: usize) -> usize {
    let line_start = line_start_offset(text, offset);
    text.get(line_start..offset)
        .unwrap_or("")
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

pub(super) fn next_char_len(text: &str, index: usize) -> usize {
    text[index..]
        .chars()
        .next()
        .map(char::len_utf8)
        .unwrap_or(1)
}
