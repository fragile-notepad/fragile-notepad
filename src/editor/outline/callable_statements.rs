use super::OutlineRulePlan;
use super::fsm::ByteRange;
use super::scan::{next_code_token, previous_code_token};
use super::source::{OutlineSource, SyntaxSymbol};
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CallableStatement {
    pub range: ByteRange,
    pub terminator: CallableStatementTerminator,
    pub is_expression_context: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CallableStatementTerminator {
    Body,
    Semicolon,
    Line,
}

pub(super) fn callable_statements(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    brace_prefix: Option<&Regex>,
) -> Vec<CallableStatement> {
    CallableStatementScanner::new(text, source, rule, brace_prefix).scan()
}

struct CallableStatementScanner<'a> {
    text: &'a str,
    source: &'a OutlineSource<'a>,
    rule: &'a OutlineRulePlan,
    brace_prefix: Option<&'a Regex>,
    statements: Vec<CallableStatement>,
    start: usize,
    cursor: usize,
    paren_depth: usize,
    bracket_depth: usize,
    expression_context: bool,
}

impl<'a> CallableStatementScanner<'a> {
    fn new(
        text: &'a str,
        source: &'a OutlineSource<'a>,
        rule: &'a OutlineRulePlan,
        brace_prefix: Option<&'a Regex>,
    ) -> Self {
        Self {
            text,
            source,
            rule,
            brace_prefix,
            statements: Vec::new(),
            start: 0,
            cursor: 0,
            paren_depth: 0,
            bracket_depth: 0,
            expression_context: false,
        }
    }

    fn scan(mut self) -> Vec<CallableStatement> {
        while self.cursor < self.text.len() {
            let Some(ch) = self.current_char() else {
                break;
            };
            // Body delimiters can contain several characters, including characters
            // which also have punctuation roles. Consume the entire indexed token.
            let len = if self.source.is_body_open(self.text, self.cursor)
                || self.source.is_body_close(self.text, self.cursor)
            {
                self.source.next_token(self.cursor).unwrap().end - self.cursor
            } else {
                ch.len_utf8()
            };

            if self.source.is_code(self.cursor) {
                self.visit_code_char(ch, len);
            }

            self.cursor += len;
        }

        self.finish_line_statement(self.text.len());
        self.statements
    }

    fn current_char(&self) -> Option<char> {
        self.text[self.cursor..].chars().next()
    }

    fn visit_code_char(&mut self, ch: char, len: usize) {
        match self.source.symbol(ch) {
            SyntaxSymbol::ParametersOpen => self.paren_depth += 1,
            SyntaxSymbol::ParametersClose => self.paren_depth = self.paren_depth.saturating_sub(1),
            SyntaxSymbol::BracketsOpen => self.bracket_depth += 1,
            SyntaxSymbol::BracketsClose => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1)
            }
            _ if self.source.is_body_open(self.text, self.cursor)
                && self.at_statement_boundary() =>
            {
                if signature_brace_is_group(
                    self.text,
                    self.source,
                    self.rule,
                    self.brace_prefix,
                    self.start,
                    self.cursor,
                ) && let Some(close) = self.source.matching_delimiter(self.cursor)
                {
                    self.cursor = self.source.next_token(close).unwrap().end - len;
                    return;
                }
                self.finish_statement(self.cursor + len, CallableStatementTerminator::Body);
            }
            _ if self.source.is_body_close(self.text, self.cursor)
                && self.at_statement_boundary() =>
            {
                self.finish_line_statement(self.cursor);
                self.reset_after(self.cursor + len);
            }
            SyntaxSymbol::StatementEnd if self.at_statement_boundary() => {
                self.finish_statement(self.cursor + len, CallableStatementTerminator::Semicolon);
            }
            SyntaxSymbol::Assignment if self.is_assignment_marker() => {
                self.expression_context = true;
            }
            _ => {}
        }
    }

    fn at_statement_boundary(&self) -> bool {
        self.paren_depth == 0 && self.bracket_depth == 0
    }

    fn is_assignment_marker(&self) -> bool {
        self.at_statement_boundary()
            && !code_at_starts_with_any(self.text, self.cursor, &self.rule.callable.operator_tokens)
    }

    fn finish_statement(&mut self, end: usize, terminator: CallableStatementTerminator) {
        let start = statement_start(self.text, self.source, self.start);
        if start < end {
            self.statements.push(CallableStatement {
                range: ByteRange::new(start, end),
                terminator,
                is_expression_context: self.expression_context
                    || statement_starts_with_control_header(
                        self.text,
                        self.source,
                        start,
                        &self.rule.callable.control_headers,
                    ),
            });
        }
        self.reset_after(end);
    }

    fn finish_line_statement(&mut self, end: usize) {
        self.finish_statement(end, CallableStatementTerminator::Line);
    }

    fn reset_after(&mut self, end: usize) {
        self.start = end;
        self.expression_context = false;
    }
}

/// A brace can delimit a type or initializer inside a signature. Keep that
/// group in the current statement so both declaration discovery and its range
/// resolve to the subsequent executable body.
pub(super) fn signature_brace_is_group(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    brace_prefix: Option<&Regex>,
    signature_start: usize,
    open: usize,
) -> bool {
    if let Some(pattern) = brace_prefix {
        let prefix = &text[signature_start..open];
        if pattern.is_match(prefix) {
            return true;
        }
        if prefix
            .char_indices()
            .any(|(offset, _)| !source.is_code(signature_start + offset))
        {
            let code_prefix: String = prefix
                .char_indices()
                .map(|(offset, ch)| {
                    if source.is_code(signature_start + offset) {
                        ch
                    } else {
                        ' '
                    }
                })
                .collect();
            if pattern.is_match(&code_prefix) {
                return true;
            }
        }
    }
    if !rule.signature_type_braces {
        return false;
    }
    let Some(previous) = previous_code_token(text, source, open) else {
        return false;
    };
    let last = previous.text(text).chars().next_back();
    if last.is_some_and(|ch| source.has_token_role(ch, "type-prefix")) {
        return true;
    }
    // A generic type can contain an object type without a colon immediately
    // before the brace, e.g. Promise<{ value: number }>.
    if source.symbol_text(previous.text(text)) == SyntaxSymbol::GenericsOpen {
        return true;
    }
    false
}

fn statement_start(text: &str, source: &OutlineSource, mut start: usize) -> usize {
    while start < text.len() {
        let Some(ch) = text[start..].chars().next() else {
            break;
        };
        if source.is_literal_start(start) || (source.is_code(start) && !ch.is_whitespace()) {
            break;
        }
        start += ch.len_utf8();
    }

    start
}

fn statement_starts_with_control_header(
    text: &str,
    source: &OutlineSource,
    start: usize,
    control_headers: &[String],
) -> bool {
    control_headers
        .iter()
        .any(|header| statement_starts_with_token_sequence(text, source, start, header))
}

fn statement_starts_with_token_sequence(
    text: &str,
    source: &OutlineSource,
    start: usize,
    sequence: &str,
) -> bool {
    let mut cursor = start;

    for expected in sequence.split_whitespace() {
        let Some(token) = next_code_token(text, source, cursor) else {
            return false;
        };
        if token.text(text) != expected {
            return false;
        }
        cursor = token.end;
    }

    true
}

fn code_at_starts_with_any(text: &str, offset: usize, candidates: &[String]) -> bool {
    let Some(rest) = text.get(offset..) else {
        return false;
    };

    candidates
        .iter()
        .any(|candidate| rest.starts_with(candidate.as_str()))
}
