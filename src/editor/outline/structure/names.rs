//! Reads callable names and preceding type tokens using language-specific rules.

use super::super::OutlineRulePlan;
use super::super::fsm::ByteRange;
use super::super::scan::*;
use super::super::source::{CodeToken, OutlineSource, SyntaxSymbol};

/// Includes configured prefixes and operators in the name before a parameter list.
pub(super) fn callable_name_before_parameters(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    open_paren: usize,
) -> Option<ByteRange> {
    if let Some(name) = callable_conversion_name_before_parameters(text, source, rule, open_paren) {
        return Some(name);
    }
    if let Some(name) = callable_compound_name_before_parameters(text, source, rule, open_paren) {
        return Some(name);
    }

    let mut token = previous_contiguous_code_token(text, source, open_paren)?;
    if token
        .text(text)
        .chars()
        .next_back()
        .is_some_and(|ch| source.symbol(ch) == SyntaxSymbol::GenericsClose)
    {
        let close = token.end - token.text(text).chars().next_back()?.len_utf8();
        let open = matching_code_angle_before(text, source, close)?;
        token = previous_contiguous_code_token(text, source, open)?;
    }
    let token_text = token.text(text);
    if rule
        .callable
        .compound_prefixes
        .iter()
        .any(|prefix| token_text == prefix)
    {
        return None;
    }
    if let Some(prefix) = rule
        .callable
        .compound_prefixes
        .iter()
        .find(|prefix| token_text.ends_with(prefix.as_str()))
    {
        let prefix = ByteRange::new(token.end - prefix.len(), token.end);
        return callable_operator_name(text, source, rule, prefix, open_paren);
    }

    if rule
        .callable
        .container_name_previous
        .iter()
        .any(|previous| previous == token_text)
    {
        return Some(ByteRange::new(token.start, token.end));
    }

    let name = callable_suffix_name_range(text, source, rule, token)?;
    if name.start == name.end {
        return None;
    }
    Some(name)
}

fn callable_conversion_name_before_parameters(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    open_paren: usize,
) -> Option<ByteRange> {
    if rule.callable.compound_prefixes.is_empty() {
        return None;
    }
    let mut cursor = open_paren;
    let end = previous_code_token(text, source, open_paren)?.end;
    let mut has_type = false;
    while let Some(token) = previous_code_token(text, source, cursor) {
        let value = token.text(text);
        if rule
            .callable
            .compound_prefixes
            .iter()
            .any(|prefix| prefix == value)
        {
            return has_type.then_some(ByteRange::new(token.start, end));
        }
        if source.is_body_open(text, token.start)
            || source.is_body_close(text, token.start)
            || matches!(
                source.symbol_text(value),
                SyntaxSymbol::StatementEnd
                    | SyntaxSymbol::ParametersOpen
                    | SyntaxSymbol::ParametersClose
            )
        {
            return None;
        }
        if value.chars().all(|ch| source.is_word_char(ch)) {
            has_type = true;
        } else if !value.chars().all(|ch| {
            source.has_token_role(ch, "type-suffix")
                || matches!(
                    source.symbol(ch),
                    SyntaxSymbol::GenericsOpen
                        | SyntaxSymbol::GenericsClose
                        | SyntaxSymbol::Separator
                )
                || rule
                    .callable
                    .qualified_separators
                    .iter()
                    .any(|separator| separator.contains(ch))
        }) {
            return None;
        }
        cursor = token.start;
    }
    None
}

fn callable_compound_name_before_parameters(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    open_paren: usize,
) -> Option<ByteRange> {
    if rule.callable.compound_prefixes.is_empty() || rule.callable.operator_tokens.is_empty() {
        return None;
    }

    let mut cursor = open_paren;
    loop {
        cursor = skip_code_whitespace_before(text, source, cursor);
        if cursor == 0 {
            return None;
        }
        if let Some(prefix) = compound_prefix_before(text, source, rule, cursor) {
            // The cursor only moves backward from the parameter list.
            return callable_operator_name(text, source, rule, prefix, open_paren);
        }
        cursor = operator_token_before(text, source, rule, cursor)?;
    }
}

/// Returns the unqualified name, retaining configured prefixes such as destructors.
pub(super) fn callable_suffix_name_range(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    token: CodeToken,
) -> Option<ByteRange> {
    let token_text = token.text(text);
    let end = token.end;
    let mut cursor = end;
    let mut prefix_start = None;

    if let Some(separator) = rule
        .callable
        .qualified_separators
        .iter()
        .filter_map(|separator| {
            token_text
                .rfind(separator)
                .map(|index| (separator.as_str(), index))
        })
        .max_by_key(|(_, index)| *index)
    {
        if !qualified_separator_has_prefix(token_text, separator.1, source) {
            return None;
        }
        cursor = token.start + separator.1 + separator.0.len();
    } else {
        for (relative, ch) in token_text.char_indices().rev() {
            let offset = token.start + relative;
            if !source.is_word_char(ch) {
                break;
            }
            cursor = offset;
        }
    }
    for prefix in &rule.callable.name_prefixes {
        if cursor >= token.start + prefix.len()
            && text.get(cursor - prefix.len()..cursor) == Some(prefix.as_str())
        {
            prefix_start = Some(cursor - prefix.len());
            break;
        }
        if code_before_ends_with(text, source, token.start, prefix) {
            prefix_start = Some(previous_code_sequence_start(
                text,
                source,
                token.start,
                prefix,
            ));
            break;
        }
    }

    (cursor < end).then_some(ByteRange::new(prefix_start.unwrap_or(cursor), end))
}

fn compound_prefix_before(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    before: usize,
) -> Option<ByteRange> {
    let token = previous_code_token(text, source, before)?;
    let token_text = token.text(text);
    rule.callable
        .compound_prefixes
        .iter()
        .find(|prefix| token_text.ends_with(prefix.as_str()))
        .map(|prefix| ByteRange::new(token.end - prefix.len(), token.end))
}

fn operator_token_before(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    before: usize,
) -> Option<usize> {
    rule.callable.operator_tokens.iter().find_map(|token| {
        code_before_ends_with(text, source, before, token)
            .then(|| previous_code_sequence_start(text, source, before, token))
    })
}

fn callable_operator_name(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    operator: ByteRange,
    open_paren: usize,
) -> Option<ByteRange> {
    let mut cursor = operator.end;
    while cursor < open_paren {
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        if !source.is_code(cursor) {
            cursor += ch.len_utf8();
            continue;
        }
        if ch.is_whitespace() {
            cursor += ch.len_utf8();
            continue;
        }
        if source.symbol(ch) == SyntaxSymbol::ParametersOpen
            && matching_code_paren_after(text, source, cursor) == Some(open_paren)
        {
            return Some(ByteRange::new(operator.start, open_paren));
        }
        if let Some(token) = rule
            .callable
            .operator_tokens
            .iter()
            .find(|token| text[cursor..].starts_with(token.as_str()))
        {
            cursor += token.len();
            if cursor > open_paren {
                return None;
            }
            continue;
        }
        return None;
    }

    Some(ByteRange::new(operator.start, open_paren))
}

/// Matches a suffix backward through code, ignoring whitespace and shielded text.
pub(super) fn code_before_ends_with(
    text: &str,
    source: &OutlineSource,
    before: usize,
    suffix: &str,
) -> bool {
    let mut cursor = before;

    for expected in suffix.chars().rev() {
        let Some(offset) = previous_code_char(text, source, cursor) else {
            return false;
        };
        if !text[offset..].starts_with(expected) {
            return false;
        }
        cursor = offset;
    }

    true
}

fn previous_code_sequence_start(
    text: &str,
    source: &OutlineSource,
    before: usize,
    sequence: &str,
) -> usize {
    let mut cursor = before;

    for _ in sequence.chars().rev() {
        let Some(offset) = previous_code_char(text, source, cursor) else {
            break;
        };
        cursor = offset;
    }

    cursor
}

pub(super) fn token_matches_required_kind(
    text: &str,
    source: &OutlineSource,
    token: CodeToken,
    required: &str,
) -> bool {
    let token_text = token.text(text);
    match required {
        "identifier" => token_text.chars().all(|ch| source.is_word_char(ch)),
        "qualified-identifier" => is_qualified_identifier(token_text, source),
        "template-type-tail" => {
            source
                .symbol_char(SyntaxSymbol::GenericsClose)
                .is_some_and(|close| token_text.ends_with(close))
                && template_type_prefix_before_tail(text, source, token).is_some_and(|previous| {
                    let previous_text = previous.text(text);
                    is_qualified_identifier(previous_text, source)
                        || token_matches_required_kind(text, source, previous, "template-type-tail")
                })
        }
        "array-type-tail" => {
            let Some(close) = previous_code_token(text, source, token.end).filter(|close| {
                source.symbol_text(close.text(text)) == SyntaxSymbol::BracketsClose
            }) else {
                return false;
            };
            let Some(open) = source.matching_delimiter(close.start) else {
                return false;
            };
            // Array type suffixes have empty bracket pairs, possibly with spacing.
            if source
                .next_token(open)
                .and_then(|open| source.next_token(open.end))
                != Some(close)
            {
                return false;
            }
            previous_contiguous_code_token(text, source, open).is_some_and(|prefix| {
                is_qualified_identifier(prefix.text(text), source)
                    || token_matches_required_kind(text, source, prefix, "template-type-tail")
                    || token_matches_required_kind(text, source, prefix, "array-type-tail")
            })
        }
        "pointer-type-tail" => {
            let mut cursor = token.end;
            let mut has_suffix = false;
            while let Some(previous) = previous_code_token(text, source, cursor) {
                if previous
                    .text(text)
                    .chars()
                    .all(|ch| source.has_token_role(ch, "type-suffix"))
                {
                    has_suffix = true;
                    cursor = previous.start;
                    continue;
                }
                return has_suffix
                    && (is_qualified_identifier(previous.text(text), source)
                        || token_matches_required_kind(
                            text,
                            source,
                            previous,
                            "template-type-tail",
                        ));
            }
            false
        }
        _ => false,
    }
}

fn template_type_prefix_before_tail(
    text: &str,
    source: &OutlineSource,
    token: CodeToken,
) -> Option<CodeToken> {
    let token_text = token.text(text);
    if source.symbol_text(token_text) == SyntaxSymbol::GenericsClose {
        return matching_code_angle_before(text, source, token.start)
            .and_then(|open| previous_contiguous_code_token(text, source, open));
    }

    let open = matching_angle_in_token(token_text, source)?;
    let prefix_end = token.start + open;
    (prefix_end > token.start).then_some(CodeToken {
        start: token.start,
        end: prefix_end,
    })
}

fn matching_angle_in_token(token: &str, source: &OutlineSource) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in token.char_indices().rev() {
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

pub(super) fn is_qualified_identifier(value: &str, source: &OutlineSource) -> bool {
    let is_identifier = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .next()
                .is_some_and(|ch| source.is_identifier_start(ch))
            && part.chars().all(|ch| source.is_word_char(ch))
    };
    is_identifier(value)
        || source
            .plan
            .declarations
            .iter()
            .flat_map(|rule| &rule.callable.qualified_separators)
            .any(|separator| !separator.is_empty() && value.split(separator).all(is_identifier))
}

pub(super) fn callable_name_has_qualified_separator(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    name_start: usize,
) -> bool {
    rule.callable.qualified_separators.iter().any(|separator| {
        code_before_ends_with(text, source, name_start, separator)
            && qualified_prefix_before_separator(text, source, name_start, separator)
    })
}

fn qualified_prefix_before_separator(
    text: &str,
    source: &OutlineSource,
    name_start: usize,
    separator: &str,
) -> bool {
    let separator_start = previous_code_sequence_start(text, source, name_start, separator);
    let Some(prefix) = previous_contiguous_code_token(text, source, separator_start) else {
        return false;
    };

    let prefix_text = prefix.text(text);
    prefix_text
        .chars()
        .next_back()
        .is_some_and(|ch| source.is_word_char(ch))
}

fn qualified_separator_has_prefix(
    token: &str,
    separator_index: usize,
    source: &OutlineSource,
) -> bool {
    separator_index > 0
        && token[..separator_index]
            .chars()
            .next_back()
            .is_some_and(|ch| source.is_word_char(ch))
}
