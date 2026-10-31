//! Discovers containers and declarations as UTF-8 byte ranges.

mod arrows;
mod callables;
mod names;
mod signatures;

use super::callable_statements::callable_statements;
use super::fsm::{ByteRange, DeclarationEvent, StructuralEvent, StructuralEventKind};
use super::scan::*;
use super::source::{OutlineSource, SyntaxSymbol};
use super::{OutlineBodyKind, OutlineNameCapture, OutlinePlan, OutlineRulePlan, OutlineScanMode};
use arrows::arrow_function_declaration_events_for_rule;
use callables::{ContainerNames, callable_declaration_events_for_rule};
use names::code_before_ends_with;
use regex::Regex;
use signatures::{RuleTerminator, indent_body_end, signature_terminator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructurePassOutput {
    pub containers: Vec<StructuralEvent>,
    pub declarations: Vec<DeclarationEvent>,
}

struct RulePatterns {
    name: Option<Regex>,
    callable_name: Option<Regex>,
    brace_prefix: Option<Regex>,
}

impl RulePatterns {
    fn new(rule: &OutlineRulePlan) -> Self {
        Self {
            name: rule
                .name_pattern
                .as_ref()
                .and_then(|pattern| Regex::new(&format!("\\A(?:{pattern})")).ok()),
            callable_name: rule
                .name_pattern
                .as_ref()
                .and_then(|pattern| Regex::new(&format!("(?:{pattern})\\z")).ok()),
            brace_prefix: rule
                .signature_brace_prefix_pattern
                .as_ref()
                .and_then(|pattern| Regex::new(&format!("(?:{pattern})\\z")).ok()),
        }
    }
}

/// Discovers members before callables so constructor checks can use container names.
pub(super) fn discover_structure(
    text: &str,
    source: &OutlineSource,
    plan: &OutlinePlan,
) -> StructurePassOutput {
    let mut containers = Vec::new();
    let mut declarations = Vec::new();

    for rule in &plan.containers {
        containers.extend(container_events_for_rule(text, source, rule));
    }
    // Overlapping XML keywords can describe the same body. Prefer the earliest
    // header and, at the same start, the longest prefix before the name.
    containers.sort_by_key(|event| {
        (
            event.signature_range.start,
            std::cmp::Reverse(event.name_range.start),
        )
    });
    let mut container_bodies = std::collections::HashSet::new();
    containers.retain(|event| {
        let extent = event.body_range.unwrap_or(ByteRange::new(
            event.signature_range.end,
            event.signature_range.end,
        ));
        container_bodies.insert((extent.start, extent.end))
    });
    let members = super::members::discover_members(text, source, &containers);
    let member_names = members
        .iter()
        .map(|member| member.name_range.start)
        .collect::<std::collections::HashSet<_>>();
    containers.extend(members);
    let container_names = ContainerNames::new(text, &containers);

    for rule in &plan.declarations {
        declarations.extend(declaration_events_for_rule(
            text,
            source,
            rule,
            &container_names,
        ));
    }

    containers.sort_by_key(|event| (event.signature_range.start, event.signature_range.end));
    declarations.sort_by_key(|event| (event.signature_range.start, event.signature_range.end));
    // Overlapping keyword rules (e.g. a modifier plus declaration keyword) describe
    // one declaration. Remove duplicates before they contribute nesting intervals.
    let mut seen = std::collections::HashSet::new();
    declarations.retain(|event| {
        !member_names.contains(&event.name_range.start)
            && seen.insert((event.name_range.start, event.signature_range.end))
    });

    StructurePassOutput {
        containers,
        declarations,
    }
}

fn container_events_for_rule(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
) -> Vec<StructuralEvent> {
    let mut events = Vec::new();
    let mut cursor = 0;
    let patterns = RulePatterns::new(rule);

    while let Some(keyword_offset) = find_keyword_sequence(text, source, cursor, &rule.keyword) {
        if let Some(event) = container_at(text, source, rule, &patterns, keyword_offset) {
            events.push(event);
        }
        cursor = keyword_offset + rule.keyword[0].len();
    }

    events
}

fn declaration_events_for_rule(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    containers: &ContainerNames,
) -> Vec<DeclarationEvent> {
    let patterns = RulePatterns::new(rule);
    if rule.scan == OutlineScanMode::Callable {
        let statements = callable_statements(text, source, rule, patterns.brace_prefix.as_ref());
        let mut events = callable_declaration_events_for_rule(
            text,
            source,
            rule,
            &patterns,
            containers,
            &statements,
        );
        if rule.callable.assignment_arrow.is_some() {
            events.extend(arrow_function_declaration_events_for_rule(
                text,
                source,
                rule,
                containers,
                &statements,
            ));
            events.sort_by_key(|event| (event.signature_range.start, event.signature_range.end));
        }
        return events;
    }

    let mut events = Vec::new();
    let mut cursor = 0;

    while let Some(keyword_offset) = find_keyword_sequence(text, source, cursor, &rule.keyword) {
        if let Some(event) = declaration_at(text, source, rule, &patterns, keyword_offset) {
            events.push(event);
        }
        cursor = keyword_offset + rule.keyword[0].len();
    }

    events
}

fn container_at(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    keyword_offset: usize,
) -> Option<StructuralEvent> {
    let keyword_end = keyword_sequence_end(text, source, keyword_offset, &rule.keyword)?;
    if !keyword_has_declaration_context(text, source, rule, keyword_offset, keyword_end) {
        return None;
    }
    let name_range = name_range_after(text, source, rule, patterns, keyword_end)
        .unwrap_or(ByteRange::new(keyword_end, keyword_end));
    let terminator = signature_terminator(text, source, rule, patterns, name_range.end)?;
    let (body_range, signature_end) = match terminator {
        RuleTerminator::Body { open, end } => (Some(ByteRange::new(open, end)), end),
        RuleTerminator::Line { end } if rule.body == OutlineBodyKind::Indent => {
            let body_end = indent_body_end(text, source, keyword_offset, end);
            (Some(ByteRange::new(end, body_end)), body_end)
        }
        RuleTerminator::Line { end } => (None, end),
        RuleTerminator::Declaration { end } => (None, end),
    };

    Some(StructuralEvent {
        kind: StructuralEventKind::Body {
            owner_kind: rule.node_kind,
            body_kind: rule.body,
        },
        keyword_range: ByteRange::new(keyword_offset, keyword_end),
        name_range,
        signature_range: ByteRange::new(keyword_offset, signature_end),
        body_range,
    })
}

fn declaration_at(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    keyword_offset: usize,
) -> Option<DeclarationEvent> {
    let keyword_end = keyword_sequence_end(text, source, keyword_offset, &rule.keyword)?;
    if !keyword_has_declaration_context(text, source, rule, keyword_offset, keyword_end) {
        return None;
    }
    let name_range = name_range_after(text, source, rule, patterns, keyword_end)?;
    let signature_start = signature_start(text, source, keyword_offset);
    let terminator = signature_terminator(text, source, rule, patterns, name_range.end)?;
    let (body_range, signature_end, terminated) = match terminator {
        RuleTerminator::Body { open, end } => (Some(ByteRange::new(open, end)), end, false),
        RuleTerminator::Line { end } if rule.body == OutlineBodyKind::Indent => {
            let body_end = indent_body_end(text, source, keyword_offset, end);
            (Some(ByteRange::new(end, body_end)), body_end, false)
        }
        RuleTerminator::Line { end } => (None, end, false),
        RuleTerminator::Declaration { end } => (None, end, true),
    };

    Some(DeclarationEvent {
        rule: rule.clone(),
        name: text[name_range.start..name_range.end].to_owned(),
        name_range,
        signature_range: ByteRange::new(signature_start, signature_end),
        body_range,
        terminated,
    })
}

fn name_range_after(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    patterns: &RulePatterns,
    after_keyword: usize,
) -> Option<ByteRange> {
    if let Some(pattern) = &patterns.name {
        // Unlike ordinary identifiers, an XML pattern can capture an escaped
        // name whose spelling is also shielded as a quoted literal.
        let mut pattern_start = name_pattern_start(text, source, after_keyword);
        let first = skip_non_code_whitespace(text, source, pattern_start);
        if text[first..]
            .chars()
            .next()
            .is_some_and(|ch| source.symbol(ch) == SyntaxSymbol::GenericsOpen)
        {
            let close = matching_code_angle_after(text, source, first)?;
            pattern_start = name_pattern_start(
                text,
                source,
                close + text[close..].chars().next()?.len_utf8(),
            );
        }
        let captures = pattern.captures(text.get(pattern_start..)?)?;
        let found = pattern
            .capture_names()
            .flatten()
            .filter(|name| *name == "name" || name.starts_with("name_"))
            .find_map(|name| captures.name(name))?;
        let captured = found.as_str();
        let leading = captured.len() - captured.trim_start().len();
        let trailing = captured.trim_end().len();
        return Some(ByteRange::new(
            pattern_start + found.start() + leading,
            pattern_start + found.start() + trailing,
        ));
    }
    match rule.name {
        OutlineNameCapture::AfterKeyword => {
            let start = skip_non_code_whitespace(text, source, after_keyword);
            parse_identifier_range(text, source, start)
        }
        OutlineNameCapture::BeforeParameters => None,
    }
}

fn name_pattern_start(text: &str, source: &OutlineSource, mut cursor: usize) -> usize {
    while let Some(ch) = text.get(cursor..).and_then(|tail| tail.chars().next()) {
        if source.is_literal_start(cursor) || (source.is_code(cursor) && !ch.is_whitespace()) {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn keyword_has_declaration_context(
    text: &str,
    source: &OutlineSource,
    rule: &OutlineRulePlan,
    keyword_start: usize,
    keyword_end: usize,
) -> bool {
    if rule.reject_previous.iter().any(|prefix| {
        if let Some(role) = prefix.strip_prefix('@') {
            previous_code_char(text, source, keyword_start)
                .and_then(|offset| text[offset..].chars().next())
                .is_some_and(|ch| source.has_token_role(ch, role))
        } else {
            code_before_ends_with(text, source, keyword_start, prefix)
        }
    }) || next_code_token(text, source, keyword_end).is_some_and(|next| {
        rule.reject_next
            .iter()
            .any(|reject| reject == next.text(text))
    }) {
        return false;
    }
    if !rule.require_statement_start {
        return true;
    }
    let mut cursor = keyword_start;
    while let Some(previous) = previous_code_token(text, source, cursor) {
        if source
            .plan
            .signature_modifiers
            .iter()
            .any(|modifier| modifier == previous.text(text))
        {
            cursor = previous.start;
            continue;
        }
        if source.symbol_text(previous.text(text)) == SyntaxSymbol::BracketsClose
            && let Some(open) = source.matching_delimiter(previous.start)
            && let Some(attribute) = previous_code_token(text, source, open).filter(|token| {
                token
                    .text(text)
                    .chars()
                    .all(|ch| source.has_token_role(ch, "attribute-prefix"))
            })
        {
            cursor = attribute.start;
            continue;
        }
        return source.is_body_open(text, previous.start)
            || source.is_body_close(text, previous.start)
            || source.symbol_text(previous.text(text)) == SyntaxSymbol::StatementEnd;
    }
    true
}

#[cfg(test)]
#[path = "structure_tests.rs"]
mod tests;
