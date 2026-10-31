//! Locates declaration bodies, skipping signature groups such as object return
//! types and constructor member initializers according to language rules.

use super::super::callable_statements::{
    CallableStatement, CallableStatementTerminator, signature_brace_is_group,
};
use super::super::scan::*;
use super::super::source::{OutlineSource, SyntaxSymbol};
use super::super::{OutlineBodyKind, OutlineRulePlan};
use super::RulePatterns;

/// Extends a callable name backward to the declaration's first token.
pub(super) fn callable_signature_start(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    statement: &CallableStatement,
    name_start: usize,
) -> usize {
    let mut start = name_start;
    let mut cursor = name_start;

    while let Some(token) = previous_code_token(text, source, cursor) {
        let token_text = token.text(text);
        if rule
            .callable
            .start_boundaries
            .iter()
            .any(|boundary| boundary == token_text)
            || token.start < statement.range.start
        {
            break;
        }
        start = token.start;
        cursor = token.start;
    }

    line_start_offset(text, start).max(start)
}

pub(super) fn callable_signature_terminator(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    statement: &CallableStatement,
    after_parameters: usize,
) -> Option<RuleTerminator> {
    let mut cursor = after_parameters;
    let mut angle_depth = 0usize;
    if next_code_token(text, source, cursor)
        .is_some_and(|token| source.symbol_text(token.text(text)) == SyntaxSymbol::ParametersOpen)
    {
        return None;
    }

    while cursor < statement.range.end {
        if !source.is_code(cursor) {
            cursor += next_char_len(text, cursor);
            continue;
        }
        let ch = text[cursor..].chars().next()?;
        match source.symbol(ch) {
            SyntaxSymbol::ParametersOpen | SyntaxSymbol::BracketsOpen => {
                let close = source.matching_delimiter(cursor)?;
                cursor = source.next_token(close)?.end;
                continue;
            }
            SyntaxSymbol::GenericsOpen => angle_depth += 1,
            SyntaxSymbol::GenericsClose => angle_depth = angle_depth.saturating_sub(1),
            SyntaxSymbol::StatementEnd
                if angle_depth == 0
                    && rule
                        .declaration_terminator
                        .as_deref()
                        .is_some_and(|end| end != "line" && text[cursor..].starts_with(end)) =>
            {
                return (matches!(statement.terminator, CallableStatementTerminator::Semicolon)
                    && cursor < statement.range.end)
                    .then_some(RuleTerminator::Declaration {
                        end: cursor + ch.len_utf8(),
                    });
            }
            _ if source.is_body_open(text, cursor) && angle_depth == 0 => {
                if signature_brace_is_group(
                    text,
                    source,
                    rule,
                    patterns.brace_prefix.as_ref(),
                    statement.range.start,
                    cursor,
                ) {
                    let close = source.matching_delimiter(cursor)?;
                    cursor = source.next_token(close)?.end;
                    continue;
                }
                if !matches!(statement.terminator, CallableStatementTerminator::Body) {
                    return None;
                }
                let close = matching_code_brace(text, source, cursor)?;
                return Some(RuleTerminator::Body {
                    open: cursor,
                    end: source.next_token(close)?.end,
                });
            }
            _ if matches!(ch, '\r' | '\n') && angle_depth == 0 => {
                if rule.declaration_terminator.as_deref() == Some("line") {
                    return Some(RuleTerminator::Declaration { end: cursor });
                }
                let line_end = line_end_offset(text, cursor);
                let tail = text.get(cursor..line_end).unwrap_or("");
                if !tail.trim().is_empty() {
                    return None;
                }
            }
            SyntaxSymbol::Assignment if angle_depth == 0 => {
                if rule
                    .callable
                    .assignment_arrow
                    .as_deref()
                    .is_some_and(|arrow| text[cursor..].starts_with(arrow))
                {
                    return None;
                }
                if let Some(next) = next_code_token(text, source, cursor + ch.len_utf8())
                    && rule
                        .callable
                        .assignment_continuations
                        .iter()
                        .any(|continuation| continuation == next.text(text))
                {
                    cursor = next.end;
                    continue;
                }
                return None;
            }
            _ => {}
        }
        cursor += ch.len_utf8();
    }

    None
}

/// Finds the signature ending, omitting incomplete declarations.
pub(super) fn signature_terminator(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    after_name: usize,
) -> Option<RuleTerminator> {
    match rule.body {
        OutlineBodyKind::Brace => {
            delimited_signature_terminator(text, source, rule, patterns, after_name)
        }
        OutlineBodyKind::Indent => indent_signature_terminator(text, source, after_name),
        OutlineBodyKind::EndKeyword => {
            end_keyword_signature_terminator(text, source, rule, after_name)
        }
        OutlineBodyKind::None => Some(RuleTerminator::Line {
            end: line_end_offset(text, after_name),
        }),
    }
}

fn delimited_signature_terminator(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    after_name: usize,
) -> Option<RuleTerminator> {
    let mut angle_depth = 0usize;
    let mut cursor = after_name;

    while cursor < text.len() {
        if !source.is_code(cursor) {
            cursor += next_char_len(text, cursor);
            continue;
        }

        let ch = text[cursor..].chars().next()?;
        match source.symbol(ch) {
            SyntaxSymbol::ParametersOpen | SyntaxSymbol::BracketsOpen => {
                let close = source.matching_delimiter(cursor)?;
                cursor = source.next_token(close)?.end;
                continue;
            }
            SyntaxSymbol::GenericsOpen => angle_depth += 1,
            SyntaxSymbol::GenericsClose => angle_depth = angle_depth.saturating_sub(1),
            SyntaxSymbol::StatementEnd
                if angle_depth == 0
                    && rule
                        .declaration_terminator
                        .as_deref()
                        .is_some_and(|end| end != "line" && text[cursor..].starts_with(end)) =>
            {
                return Some(RuleTerminator::Declaration {
                    end: cursor + ch.len_utf8(),
                });
            }
            _ if matches!(ch, '\r' | '\n')
                && angle_depth == 0
                && rule.declaration_terminator.as_deref() == Some("line") =>
            {
                if rule.nextline_body
                    && next_code_token(text, source, cursor)
                        .is_some_and(|token| source.is_body_open(text, token.start))
                {
                    cursor += ch.len_utf8();
                    continue;
                }
                return Some(RuleTerminator::Declaration { end: cursor });
            }
            _ if source.is_body_open(text, cursor) && angle_depth == 0 => {
                if signature_brace_is_group(
                    text,
                    source,
                    rule,
                    patterns.brace_prefix.as_ref(),
                    after_name,
                    cursor,
                ) {
                    let close = source.matching_delimiter(cursor)?;
                    cursor = source.next_token(close)?.end;
                    continue;
                }
                let close = matching_code_brace(text, source, cursor)?;
                return Some(RuleTerminator::Body {
                    open: cursor,
                    end: source.next_token(close)?.end,
                });
            }
            _ => {}
        }

        cursor += ch.len_utf8();
    }

    None
}

fn indent_signature_terminator(
    text: &str,
    source: &OutlineSource,
    after_name: usize,
) -> Option<RuleTerminator> {
    let body = source.body(OutlineBodyKind::Indent)?;
    let mut cursor = after_name;
    while cursor < text.len() {
        let ch = text[cursor..].chars().next()?;
        if source.is_code(cursor) {
            match source.symbol(ch) {
                _ if source.is_delimiter_open(text, cursor) => {
                    let close = source.matching_delimiter(cursor)?;
                    cursor = source.next_token(close)?.end;
                    continue;
                }
                _ if body
                    .header_end
                    .as_deref()
                    .is_some_and(|end| text[cursor..].starts_with(end)) =>
                {
                    let header_end = cursor + body.header_end.as_ref()?.len();
                    let line_end = line_end_offset(text, cursor);
                    if text[header_end..line_end]
                        .char_indices()
                        .any(|(offset, ch)| {
                            !ch.is_whitespace()
                                && (source.is_code(header_end + offset)
                                    || source.is_literal_start(header_end + offset))
                        })
                    {
                        return Some(RuleTerminator::Body {
                            open: header_end,
                            end: line_end,
                        });
                    }
                    return Some(RuleTerminator::Line { end: line_end });
                }
                _ if matches!(ch, '\r' | '\n') => return None,
                _ if body
                    .line_continuation
                    .as_deref()
                    .is_some_and(|token| text[cursor..].starts_with(token)) =>
                {
                    cursor = next_line_start_offset(text, cursor);
                    continue;
                }
                _ => {}
            }
        }
        cursor += ch.len_utf8();
    }
    None
}

/// Finds a body's first dedented line, excluding its preceding line ending.
/// Open delimiters postpone dedent checks until their multiline groups close.
pub(super) fn indent_body_end(
    text: &str,
    source: &OutlineSource,
    header_offset: usize,
    after_header: usize,
) -> usize {
    let header_indent = indentation_before(text, header_offset);
    let mut cursor = next_line_start_offset(text, after_header);
    let mut continuation_end = cursor;
    while cursor < text.len() {
        let line_end = line_end_offset(text, cursor);
        // Comments and the interior of multiline literals cannot dedent a body.
        let first = text[cursor..line_end]
            .char_indices()
            .find(|(relative, ch)| {
                !ch.is_whitespace()
                    && (source.is_code(cursor + relative)
                        || source.is_literal_start(cursor + relative))
            })
            .map(|(relative, _)| cursor + relative);
        if cursor >= continuation_end
            && let Some(first) = first
            && indentation_before(text, first) <= header_indent
        {
            return cursor.saturating_sub(line_ending_len_before(text, cursor));
        }
        for token in source
            .tokens_from(cursor)
            .iter()
            .take_while(|token| token.start < line_end)
        {
            if source.is_delimiter_open(text, token.start)
                && let Some(close) = source.matching_delimiter(token.start)
            {
                continuation_end =
                    continuation_end.max(source.next_token(close).map_or(close, |token| token.end));
            }
        }
        cursor = next_line_start_offset(text, line_end);
    }
    text.len()
}

fn end_keyword_signature_terminator(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    after_name: usize,
) -> Option<RuleTerminator> {
    if let Some(marker) = rule.expression_body.as_deref() {
        let mut cursor = after_name;
        let line_end = line_end_offset(text, after_name);
        if let Some(token) =
            next_code_token(text, source, cursor).filter(|token| token.start < line_end)
            && source.is_delimiter_open(text, token.start)
        {
            let close = source.matching_delimiter(token.start)?;
            cursor = source.next_token(close)?.end;
        }
        if let Some(token) =
            next_code_token(text, source, cursor).filter(|token| token.start < line_end)
            && token.text(text) == marker
        {
            // A setter's '=' is part of its name and excluded by after_name.
            return Some(RuleTerminator::Body {
                open: token.end,
                end: line_end,
            });
        }
    }
    let end = matching_end_keyword(text, source, after_name).unwrap_or(text.len());

    Some(RuleTerminator::Body {
        open: after_name,
        end,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RuleTerminator {
    /// The complete body, including its delimiters when the syntax has them.
    Body { open: usize, end: usize },
    /// The header line ends here; an indented body may follow it.
    Line { end: usize },
    /// A declaration without a body ends here, such as a method prototype.
    Declaration { end: usize },
}
