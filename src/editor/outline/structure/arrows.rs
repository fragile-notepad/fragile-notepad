//! Finds named assignment arrows with bodies; expression-only arrows are omitted.

use super::super::OutlineRulePlan;
use super::super::callable_statements::CallableStatement;
use super::super::fsm::{ByteRange, DeclarationEvent};
use super::super::scan::*;
use super::super::source::{OutlineSource, SyntaxSymbol};
use super::callables::{
    ContainerNames, callable_has_required_non_container_previous_token,
    callable_is_rejected_by_prefix,
};
use super::names::callable_suffix_name_range;

pub(super) fn arrow_function_declaration_events_for_rule(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    containers: &ContainerNames,
    statements: &[CallableStatement],
) -> Vec<DeclarationEvent> {
    let mut events = Vec::new();

    for statement in statements
        .iter()
        .filter(|statement| statement.is_expression_context)
    {
        for arrow in top_level_arrow_offsets(
            text,
            source,
            statement.range,
            rule.callable.assignment_arrow.as_deref().unwrap_or(""),
        ) {
            if let Some(event) =
                arrow_function_declaration_at(text, source, rule, containers, statement, arrow)
            {
                events.push(event);
            }
        }
    }

    events
}

fn arrow_function_declaration_at(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    containers: &ContainerNames,
    statement: &CallableStatement,
    arrow: usize,
) -> Option<DeclarationEvent> {
    let assignment = arrow_assignment_before(text, source, statement.range.start, arrow)?;
    let token = previous_contiguous_code_token(text, source, assignment)?;
    let name_range = callable_suffix_name_range(text, source, rule, token)?;
    let name = text.get(name_range.start..name_range.end)?;
    if name.is_empty()
        || rule
            .callable
            .reject_names
            .iter()
            .any(|reject| reject == name)
    {
        return None;
    }
    if callable_is_rejected_by_prefix(text, source, rule, name_range.start) {
        return None;
    }
    if !callable_has_required_non_container_previous_token(
        text, source, rule, containers, name_range,
    ) {
        return None;
    }

    let body_open = next_code_token(
        text,
        source,
        arrow + rule.callable.assignment_arrow.as_ref()?.len(),
    )
    .filter(|token| source.is_body_open(text, token.start))
    .map(|token| token.start)?;
    if body_open >= statement.range.end {
        return None;
    }
    let body_close = matching_code_brace(text, source, body_open)?;
    let body_end = source.next_token(body_close)?.end;

    Some(DeclarationEvent {
        rule: rule.clone(),
        name: name.to_owned(),
        name_range,
        signature_range: ByteRange::new(statement.range.start, body_end),
        body_range: Some(ByteRange::new(body_open, body_end)),
        terminated: false,
    })
}

fn arrow_assignment_before(
    text: &str,
    source: &OutlineSource,
    start: usize,
    arrow: usize,
) -> Option<usize> {
    let mut assignment = None;
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    while cursor < arrow {
        let ch = text[cursor..].chars().next()?;
        let len = ch.len_utf8();
        if !source.is_code(cursor) {
            cursor += len;
            continue;
        }

        match source.symbol(ch) {
            SyntaxSymbol::ParametersOpen => paren_depth += 1,
            SyntaxSymbol::ParametersClose => paren_depth = paren_depth.saturating_sub(1),
            SyntaxSymbol::BracketsOpen => bracket_depth += 1,
            SyntaxSymbol::BracketsClose => bracket_depth = bracket_depth.saturating_sub(1),
            SyntaxSymbol::BodyOpen => brace_depth += 1,
            SyntaxSymbol::BodyClose => brace_depth = brace_depth.saturating_sub(1),
            SyntaxSymbol::Assignment
                if paren_depth == 0
                    && bracket_depth == 0
                    && brace_depth == 0
                    && standalone_assignment_at(text, source, cursor) =>
            {
                assignment = Some(cursor);
            }
            _ => {}
        }

        cursor += len;
    }

    assignment
}

fn standalone_assignment_at(text: &str, source: &OutlineSource, offset: usize) -> bool {
    let Some(assignment) = source.symbol_char(SyntaxSymbol::Assignment) else {
        return false;
    };
    if !text[offset..].starts_with(assignment) {
        return false;
    }
    if text[offset + assignment.len_utf8()..]
        .chars()
        .next()
        .is_some_and(|ch| source.has_token_role(ch, "assignment-reject-after"))
    {
        return false;
    }
    previous_code_char(text, source, offset)
        .and_then(|previous| text[previous..].chars().next())
        .is_none_or(|ch| !source.has_token_role(ch, "assignment-reject-before"))
}

fn top_level_arrow_offsets(
    text: &str,
    source: &OutlineSource,
    range: ByteRange,
    arrow_token: &str,
) -> Vec<usize> {
    let mut arrows = Vec::new();
    if arrow_token.is_empty() {
        return arrows;
    }
    let mut cursor = range.start;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    while cursor < range.end {
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        let len = ch.len_utf8();
        if !source.is_code(cursor) {
            cursor += len;
            continue;
        }

        match source.symbol(ch) {
            SyntaxSymbol::ParametersOpen => paren_depth += 1,
            SyntaxSymbol::ParametersClose => paren_depth = paren_depth.saturating_sub(1),
            SyntaxSymbol::BracketsOpen => bracket_depth += 1,
            SyntaxSymbol::BracketsClose => bracket_depth = bracket_depth.saturating_sub(1),
            SyntaxSymbol::BodyOpen => brace_depth += 1,
            SyntaxSymbol::BodyClose => brace_depth = brace_depth.saturating_sub(1),
            _ if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                let arrow_end = cursor + arrow_token.len();
                if arrow_end <= range.end
                    && text.get(cursor..arrow_end) == Some(arrow_token)
                    && source.is_code_range(cursor, arrow_end)
                {
                    arrows.push(cursor);
                    cursor = arrow_end;
                    continue;
                }
            }
            _ => {}
        }

        cursor += len;
    }

    arrows
}
