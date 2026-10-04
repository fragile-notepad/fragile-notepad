//! Shared lexical and delimiter index for one immutable parsing snapshot.
use super::lexical::OutlineCodeMask;
use super::{OutlineBodyKind, OutlineBodyPlan, OutlinePlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodeToken {
    pub start: usize,
    pub end: usize,
}

impl CodeToken {
    pub(super) fn text(self, text: &str) -> &str {
        &text[self.start..self.end]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SyntaxSymbol {
    ParametersOpen,
    ParametersClose,
    BracketsOpen,
    BracketsClose,
    GenericsOpen,
    GenericsClose,
    Assignment,
    Separator,
    StatementEnd,
    BodyOpen,
    BodyClose,
    Other,
}

impl SyntaxSymbol {
    fn from_role(role: &str) -> Self {
        match role {
            "parameters-open" => Self::ParametersOpen,
            "parameters-close" => Self::ParametersClose,
            "brackets-open" => Self::BracketsOpen,
            "brackets-close" => Self::BracketsClose,
            "generics-open" => Self::GenericsOpen,
            "generics-close" => Self::GenericsClose,
            "assignment" => Self::Assignment,
            "separator" => Self::Separator,
            "statement-end" => Self::StatementEnd,
            _ => Self::Other,
        }
    }
}

pub(super) struct OutlineSource<'a> {
    pub plan: &'a OutlinePlan,
    symbols: Vec<(SyntaxSymbol, char)>,
    mask: OutlineCodeMask,
    tokens: Vec<CodeToken>,
    // Token indices, so memory scales with tokens, not source bytes.
    pairs: Vec<Option<usize>>,
    delimiters: Vec<super::schema::RawDelimiter>,
    pub expression_rules: Vec<OutlineExpressionRule<'a>>,
}

pub(super) struct OutlineExpressionRule<'a> {
    pub keyword: &'a str,
    pub marker: &'a str,
    pub name_pattern: Option<regex::Regex>,
}

impl<'a> OutlineSource<'a> {
    pub(super) fn new(text: &str, plan: &'a OutlinePlan) -> Self {
        let mask = OutlineCodeMask::new(text, &plan.lexical);
        let is_word = |ch: char| {
            plan.lexical.word_character_extra.contains(ch)
                || if plan.lexical.unicode_word_characters {
                    unicode_ident::is_xid_continue(ch)
                } else {
                    ch.is_ascii_alphanumeric()
                }
        };
        let mut delimiters = plan.structure.delimiters.clone();
        for body in &plan.structure.bodies {
            if body.kind == OutlineBodyKind::Brace
                && let (Some(open), Some(close)) = (&body.open, &body.close)
            {
                delimiters.push(super::schema::RawDelimiter {
                    open: open.clone(),
                    close: close.clone(),
                });
            }
        }
        let mut tokens = Vec::new();
        let mut chars = text.char_indices().peekable();
        while let Some((start, ch)) = chars.next() {
            if !mask.is_code(start) || ch.is_whitespace() {
                continue;
            }
            let mut end = start + ch.len_utf8();
            let delimiter = delimiters
                .iter()
                .flat_map(|pair| [&pair.open, &pair.close])
                .filter(|value| {
                    !value.is_empty()
                        && !is_word(ch)
                        && text[start..].starts_with(value.as_str())
                        && mask.is_code_range(start, start + value.len())
                })
                .max_by_key(|value| value.len());
            let identifier_prefix = plan.lexical.identifier_prefix.as_deref().filter(|prefix| {
                !prefix.is_empty()
                    && text[start..].starts_with(prefix)
                    && mask.is_code_range(start, start + prefix.len())
                    && text[start + prefix.len()..]
                        .chars()
                        .next()
                        .is_some_and(|ch| {
                            plan.lexical.word_character_extra.contains(ch)
                                || if plan.lexical.unicode_word_characters {
                                    unicode_ident::is_xid_start(ch)
                                } else {
                                    ch.is_ascii_alphabetic()
                                }
                        })
            });
            if let Some(prefix) = identifier_prefix {
                end = start + prefix.len();
                while chars.peek().is_some_and(|(offset, _)| *offset < end) {
                    chars.next();
                }
                while let Some(&(offset, ch)) = chars.peek() {
                    if !mask.is_code(offset) || !is_word(ch) {
                        break;
                    }
                    end = offset + ch.len_utf8();
                    chars.next();
                }
            } else if let Some(delimiter) = delimiter {
                end = start + delimiter.len();
                while chars.peek().is_some_and(|(offset, _)| *offset < end) {
                    chars.next();
                }
            } else if is_word(ch) {
                while let Some(&(offset, ch)) = chars.peek() {
                    if !mask.is_code(offset) || !is_word(ch) {
                        break;
                    }
                    end = offset + ch.len_utf8();
                    chars.next();
                }
            }
            tokens.push(CodeToken { start, end });
        }
        let mut pairs = vec![None; tokens.len()];
        let mut stacks = vec![Vec::new(); delimiters.len()];
        for (index, token) in tokens.iter().enumerate() {
            for (group, delimiter) in delimiters.iter().enumerate() {
                if token.text(text) == delimiter.open {
                    stacks[group].push(index);
                } else if token.text(text) == delimiter.close
                    && let Some(open) = stacks[group].pop()
                {
                    pairs[open] = Some(index);
                    pairs[index] = Some(open);
                }
            }
        }
        let mut symbols = plan
            .structure
            .syntax_tokens
            .iter()
            .filter_map(|token| {
                let role = SyntaxSymbol::from_role(&token.role);
                (role != SyntaxSymbol::Other)
                    .then(|| Some((role, token.value.chars().next()?)))
                    .flatten()
            })
            .collect::<Vec<_>>();
        for body in &plan.structure.bodies {
            if body.kind == OutlineBodyKind::Brace {
                for (role, value) in [
                    (SyntaxSymbol::BodyOpen, &body.open),
                    (SyntaxSymbol::BodyClose, &body.close),
                ] {
                    if let Some(value) = value.as_deref().filter(|value| value.chars().count() == 1)
                    {
                        symbols.push((role, value.chars().next().unwrap()));
                    }
                }
            }
        }
        Self {
            plan,
            symbols,
            mask,
            tokens,
            pairs,
            delimiters,
            expression_rules: plan
                .declarations
                .iter()
                .filter_map(|rule| {
                    Some(OutlineExpressionRule {
                        keyword: rule.keyword.last()?.as_str(),
                        marker: rule.expression_body.as_deref()?,
                        name_pattern: rule
                            .name_pattern
                            .as_ref()
                            .and_then(|pattern| regex::Regex::new(pattern).ok()),
                    })
                })
                .collect(),
        }
    }

    pub(super) fn symbol(&self, ch: char) -> SyntaxSymbol {
        self.symbols
            .iter()
            .find(|(_, value)| *value == ch)
            .map_or(SyntaxSymbol::Other, |(role, _)| *role)
    }

    pub(super) fn symbol_char(&self, role: SyntaxSymbol) -> Option<char> {
        self.symbols
            .iter()
            .find(|(candidate, _)| *candidate == role)
            .map(|(_, ch)| *ch)
    }

    pub(super) fn symbol_text(&self, text: &str) -> SyntaxSymbol {
        let mut chars = text.chars();
        let Some(ch) = chars.next() else {
            return SyntaxSymbol::Other;
        };
        if chars.next().is_some() {
            return SyntaxSymbol::Other;
        }
        self.symbol(ch)
    }

    pub(super) fn has_token_role(&self, ch: char, role: &str) -> bool {
        self.plan
            .structure
            .syntax_tokens
            .iter()
            .any(|token| token.role == role && token.value.starts_with(ch))
    }

    pub(super) fn is_word_char(&self, ch: char) -> bool {
        self.plan.lexical.word_character_extra.contains(ch)
            || if self.plan.lexical.unicode_word_characters {
                unicode_ident::is_xid_continue(ch)
            } else {
                ch.is_ascii_alphanumeric()
            }
    }

    pub(super) fn is_identifier_start(&self, ch: char) -> bool {
        self.plan.lexical.word_character_extra.contains(ch)
            || if self.plan.lexical.unicode_word_characters {
                unicode_ident::is_xid_start(ch)
            } else {
                ch.is_ascii_alphabetic()
            }
    }

    pub(super) fn is_body_open(&self, text: &str, offset: usize) -> bool {
        self.body(OutlineBodyKind::Brace)
            .and_then(|body| body.open.as_deref())
            .is_some_and(|open| {
                !open.is_empty()
                    && text[offset..].starts_with(open)
                    && self
                        .tokens_from(offset)
                        .first()
                        .is_some_and(|token| token.start == offset && token.text(text) == open)
            })
    }

    pub(super) fn is_body_close(&self, text: &str, offset: usize) -> bool {
        self.body(OutlineBodyKind::Brace)
            .and_then(|body| body.close.as_deref())
            .is_some_and(|close| {
                !close.is_empty()
                    && text[offset..].starts_with(close)
                    && self
                        .tokens_from(offset)
                        .first()
                        .is_some_and(|token| token.start == offset && token.text(text) == close)
            })
    }

    pub(super) fn is_delimiter_open(&self, text: &str, offset: usize) -> bool {
        self.delimiters
            .iter()
            .any(|pair| !pair.open.is_empty() && text[offset..].starts_with(&pair.open))
    }

    pub(super) fn body(&self, kind: OutlineBodyKind) -> Option<&OutlineBodyPlan> {
        self.plan
            .structure
            .bodies
            .iter()
            .find(|body| body.kind == kind)
    }

    pub(super) fn is_code(&self, offset: usize) -> bool {
        self.mask.is_code(offset)
    }
    pub(super) fn is_code_range(&self, start: usize, end: usize) -> bool {
        self.mask.is_code_range(start, end)
    }
    pub(super) fn is_literal_start(&self, offset: usize) -> bool {
        self.mask.is_literal_start(offset)
    }

    pub(super) fn tokens_from(&self, offset: usize) -> &[CodeToken] {
        &self.tokens[self.tokens.partition_point(|token| token.end <= offset)..]
    }

    pub(super) fn next_token(&self, offset: usize) -> Option<CodeToken> {
        self.tokens_from(offset)
            .first()
            .copied()
            .map(|token| CodeToken {
                start: token.start.max(offset),
                ..token
            })
    }

    pub(super) fn previous_token(&self, offset: usize) -> Option<CodeToken> {
        let index = self
            .tokens
            .partition_point(|token| token.start < offset)
            .checked_sub(1)?;
        let token = self.tokens[index];
        Some(CodeToken {
            end: token.end.min(offset),
            ..token
        })
    }

    pub(super) fn matching_delimiter(&self, offset: usize) -> Option<usize> {
        let index = self
            .tokens
            .binary_search_by_key(&offset, |token| token.start)
            .ok()?;
        Some(self.tokens[self.pairs[index]?].start)
    }
}
