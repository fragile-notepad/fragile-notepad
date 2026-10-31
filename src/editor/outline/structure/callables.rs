//! Distinguishes callable declarations from calls and checks constructor names.

use super::super::callable_statements::{CallableStatement, CallableStatementTerminator};
use super::super::fsm::{ByteRange, DeclarationEvent, StructuralEvent};
use super::super::scan::*;
use super::super::source::{OutlineSource, SyntaxSymbol};
use super::super::{OutlineNodeKind, OutlineRulePlan};
use super::RulePatterns;
use super::names::{
    callable_name_before_parameters, callable_name_has_qualified_separator, code_before_ends_with,
    is_qualified_identifier, token_matches_required_kind,
};
use super::signatures::{RuleTerminator, callable_signature_start, callable_signature_terminator};

pub(super) fn callable_declaration_events_for_rule(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    containers: &ContainerNames,
    statements: &[CallableStatement],
) -> Vec<DeclarationEvent> {
    let mut events = Vec::new();
    let mut statement_index = 0;
    let mut cursor = 0;

    let Some(parameters_open) = source.symbol_char(SyntaxSymbol::ParametersOpen) else {
        return events;
    };
    while let Some(open_paren) = find_next_code_char(text, source, cursor, parameters_open) {
        while statement_index < statements.len()
            && statements[statement_index].range.end <= open_paren
        {
            statement_index += 1;
        }
        let Some(statement) = statements.get(statement_index).filter(|statement| {
            statement.range.start <= open_paren && open_paren < statement.range.end
        }) else {
            cursor = open_paren + parameters_open.len_utf8();
            continue;
        };

        if statement.is_expression_context {
            cursor = open_paren + parameters_open.len_utf8();
            continue;
        }

        if let Some(event) = callable_declaration_at(
            text, source, rule, patterns, containers, statement, open_paren,
        ) {
            events.push(event);
        }
        cursor = open_paren + parameters_open.len_utf8();
    }

    if !rule.callable.compact_constructor_containers.is_empty() {
        events.extend(compact_constructor_events(
            text, source, rule, containers, statements,
        ));
    }

    events
}

fn compact_constructor_events(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    containers: &ContainerNames,
    statements: &[CallableStatement],
) -> Vec<DeclarationEvent> {
    let mut events = Vec::new();
    for statement in statements.iter().filter(|statement| {
        !statement.is_expression_context
            && statement.terminator == CallableStatementTerminator::Body
    }) {
        let Some(open) = previous_code_token(text, source, statement.range.end)
            .filter(|token| source.is_body_open(text, token.start))
        else {
            continue;
        };
        let Some(name) = previous_code_token(text, source, open.start).filter(|name| {
            name.start >= statement.range.start && is_qualified_identifier(name.text(text), source)
        }) else {
            continue;
        };
        if !containers.compact.iter().any(|container| {
            container.name == name.text(text)
                && container.start < name.start
                && name.start < container.end
                && rule
                    .callable
                    .compact_constructor_containers
                    .iter()
                    .any(|keyword| keyword == &container.keyword)
        }) || source
            .tokens_from(statement.range.start)
            .iter()
            .take_while(|token| token.start < name.start)
            .any(|token| {
                matches!(
                    source.symbol_text(token.text(text)),
                    SyntaxSymbol::ParametersOpen | SyntaxSymbol::ParametersClose
                )
            })
        {
            continue;
        }
        let Some(close) = source
            .matching_delimiter(open.start)
            .and_then(|close| source.next_token(close))
        else {
            continue;
        };
        let mut constructor_rule = rule.clone();
        constructor_rule.node_kind = OutlineNodeKind::Constructor;
        events.push(DeclarationEvent {
            rule: constructor_rule,
            name: name.text(text).to_owned(),
            name_range: ByteRange::new(name.start, name.end),
            signature_range: ByteRange::new(statement.range.start, close.end),
            body_range: Some(ByteRange::new(open.start, close.end)),
            terminated: false,
        });
    }
    events
}

fn callable_declaration_at(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    containers: &ContainerNames,
    statement: &CallableStatement,
    open_paren: usize,
) -> Option<DeclarationEvent> {
    let name_range = patterns
        .callable_name
        .as_ref()
        .and_then(|pattern| {
            let captures = pattern.captures(text[statement.range.start..open_paren].trim_end())?;
            let found = pattern
                .capture_names()
                .flatten()
                .filter(|name| *name == "name" || name.starts_with("name_"))
                .find_map(|name| captures.name(name))?;
            Some(ByteRange::new(
                statement.range.start + found.start(),
                statement.range.start + found.end(),
            ))
        })
        .or_else(|| callable_name_before_parameters(text, source, rule, open_paren))?;
    let name = text.get(name_range.start..name_range.end)?;
    if rule
        .callable
        .reject_names
        .iter()
        .any(|reject| reject == name)
    {
        return None;
    }
    if callable_is_rejected_by_previous_token(text, source, rule, name_range.start) {
        return None;
    }
    if callable_is_rejected_by_prefix(text, source, rule, name_range.start) {
        return None;
    }
    if !callable_has_required_previous_token(text, source, rule, name_range.start) {
        return None;
    }
    if !callable_has_required_non_container_previous_token(
        text, source, rule, containers, name_range,
    ) {
        return None;
    }
    let close_paren = matching_code_paren_after(text, source, open_paren)?;
    let signature_start = callable_signature_start(text, source, rule, statement, name_range.start);
    let terminator = callable_signature_terminator(
        text,
        source,
        rule,
        patterns,
        statement,
        source.next_token(close_paren)?.end,
    )?;
    let (body_range, signature_end, terminated) = match terminator {
        RuleTerminator::Body { open, end } => (Some(ByteRange::new(open, end)), end, false),
        RuleTerminator::Declaration { end } => (None, end, true),
        RuleTerminator::Line { end } => (None, end, false),
    };

    Some(DeclarationEvent {
        rule: rule.clone(),
        name: name.to_owned(),
        name_range,
        signature_range: ByteRange::new(signature_start, signature_end),
        body_range,
        terminated,
    })
}

fn callable_is_rejected_by_previous_token(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    name_start: usize,
) -> bool {
    let Some(previous) = previous_contiguous_code_token(text, source, name_start) else {
        return false;
    };
    let previous_text = previous.text(text);
    rule.callable
        .reject_previous
        .iter()
        .any(|reject| reject == previous_text)
        || previous_code_token(text, source, name_start).is_some_and(|previous| {
            let previous_text = previous.text(text);
            rule.callable
                .reject_previous
                .iter()
                .any(|reject| reject == previous_text)
        })
}

pub(super) fn callable_is_rejected_by_prefix(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    name_start: usize,
) -> bool {
    rule.callable
        .reject_prefixes
        .iter()
        .any(|prefix| code_before_ends_with(text, source, name_start, prefix))
}

fn callable_has_required_previous_token(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    name_start: usize,
) -> bool {
    if rule.callable.require_previous.is_empty() {
        return true;
    }
    let Some(previous) = previous_contiguous_code_token(text, source, name_start) else {
        return false;
    };
    let previous_text = previous.text(text);
    rule.callable
        .require_previous
        .iter()
        .any(|required| required == previous_text)
}

pub(super) fn callable_has_required_non_container_previous_token(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    containers: &ContainerNames,
    name_range: ByteRange,
) -> bool {
    if rule.callable.require_non_container_previous.is_empty()
        && rule.callable.require_non_container_previous_kind.is_empty()
    {
        return true;
    }
    // Constructors and operators need no return type. Qualified names also
    // carry declaration context through the owning type before the separator.
    if callable_has_container_name_previous_token(text, source, rule, name_range.start)
        || callable_name_matches_containing_container(text, containers, rule, name_range)
        || callable_name_has_qualified_separator(text, source, rule, name_range.start)
        || rule.callable.compound_prefixes.iter().any(|prefix| {
            text[name_range.start..name_range.end]
                .strip_prefix(prefix)
                .is_some_and(|tail| {
                    tail.chars()
                        .next()
                        .is_none_or(|ch| !source.is_word_char(ch))
                })
        })
    {
        return true;
    }
    let Some(previous) = previous_contiguous_code_token(text, source, name_range.start) else {
        return false;
    };
    let previous_text = previous.text(text);
    rule.callable
        .require_non_container_previous
        .iter()
        .any(|required| required == previous_text)
        || rule
            .callable
            .require_non_container_previous_kind
            .iter()
            .any(|required| token_matches_required_kind(text, source, previous, required))
}

fn callable_name_matches_containing_container(
    text: &str,
    containers: &ContainerNames,
    rule: &OutlineRulePlan,
    name_range: ByteRange,
) -> bool {
    if rule.callable.container_name_previous.is_empty() {
        return false;
    }
    let Some(name) = text.get(name_range.start..name_range.end) else {
        return false;
    };
    let unprefixed_name = rule
        .callable
        .name_prefixes
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(name);

    containers.contains(unprefixed_name, name_range.start)
}

/// Tests containment with independently sorted starts and ends for each name.
/// More starts than ends at an offset means a body with that name is still open,
/// including nested bodies with the same name. Ranges exclude their end byte.
pub(super) struct ContainerNames {
    names: std::collections::HashMap<String, (Vec<usize>, Vec<usize>)>,
    compact: Vec<CompactContainer>,
}

struct CompactContainer {
    name: String,
    keyword: String,
    start: usize,
    end: usize,
}

impl ContainerNames {
    pub(super) fn new(text: &str, containers: &[StructuralEvent]) -> Self {
        let mut names: std::collections::HashMap<String, (Vec<usize>, Vec<usize>)> =
            std::collections::HashMap::new();
        let mut compact = Vec::new();
        for container in containers {
            let Some(body) = container.body_range.filter(|range| range.start < range.end) else {
                continue;
            };
            let Some(name) = text.get(container.name_range.start..container.name_range.end) else {
                continue;
            };
            let (starts, ends) = names.entry(name.to_owned()).or_default();
            starts.push(body.start);
            ends.push(body.end);
            compact.push(CompactContainer {
                name: name.to_owned(),
                keyword: text[container.keyword_range.start..container.keyword_range.end]
                    .to_owned(),
                start: body.start,
                end: body.end,
            });
        }
        for (starts, ends) in names.values_mut() {
            starts.sort_unstable();
            ends.sort_unstable();
        }
        Self { names, compact }
    }

    fn contains(&self, name: &str, offset: usize) -> bool {
        self.names.get(name).is_some_and(|(starts, ends)| {
            let started = starts.partition_point(|start| *start <= offset);
            let ended = ends.partition_point(|end| *end <= offset);
            started > ended
        })
    }
}

fn callable_has_container_name_previous_token(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    name_start: usize,
) -> bool {
    let Some(previous) = previous_contiguous_code_token(text, source, name_start) else {
        return false;
    };
    let previous_text = previous.text(text);
    rule.callable
        .container_name_previous
        .iter()
        .any(|container| container == previous_text)
}
