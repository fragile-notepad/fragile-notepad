use super::diagnostics;
use super::schema::{
    RawBody, RawFamily, RawLanguage, RawLexical, RawOutlineSchema, RawRule, RawString,
};
use super::types::{OutlineDiagnostic, OutlineNodeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledOutlineRegistry {
    pub plans: Vec<OutlinePlan>,
    pub diagnostics: Vec<OutlineDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlinePlan {
    pub language_name: String,
    pub syntax_tokens: Vec<String>,
    pub family_id: String,
    pub adapter_name: String,
    pub signature_modifiers: Vec<String>,
    pub containers: Vec<OutlineRulePlan>,
    pub declarations: Vec<OutlineRulePlan>,
    pub members: Vec<OutlineMemberPlan>,
    pub lexical: OutlineLexicalPlan,
    pub structure: OutlineStructurePlan,
    pub diagnostics: Vec<OutlineDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineRulePlan {
    pub node_kind: OutlineNodeKind,
    pub keyword: Vec<String>,
    pub name: OutlineNameCapture,
    pub scan: OutlineScanMode,
    pub callable: OutlineCallablePlan,
    pub body: OutlineBodyKind,
    pub method_containers: Vec<OutlineNodeKind>,
    pub declaration_terminator: Option<String>,
    pub name_pattern: Option<String>,
    pub reject_previous: Vec<String>,
    pub reject_next: Vec<String>,
    pub require_statement_start: bool,
    pub signature_type_braces: bool,
    pub signature_brace_prefix_pattern: Option<String>,
    pub nextline_body: bool,
    pub expression_body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineMemberPlan {
    pub node_kind: OutlineNodeKind,
    pub within: OutlineNodeKind,
    pub separator: String,
    pub terminator: Option<String>,
    pub prefix_pattern: Option<String>,
    pub name_pattern: Option<String>,
    pub line_skip_pattern: Option<String>,
    pub generic_open_pattern: Option<String>,
    pub generic_suffix_pattern: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutlineBodyKind {
    Brace,
    Indent,
    EndKeyword,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineNameCapture {
    AfterKeyword,
    BeforeParameters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineScanMode {
    Keyword,
    Callable,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutlineCallablePlan {
    pub reject_names: Vec<String>,
    pub reject_previous: Vec<String>,
    pub reject_prefixes: Vec<String>,
    pub require_previous: Vec<String>,
    pub require_non_container_previous: Vec<String>,
    pub require_non_container_previous_kind: Vec<String>,
    pub start_boundaries: Vec<String>,
    pub operator_tokens: Vec<String>,
    pub name_prefixes: Vec<String>,
    pub qualified_separators: Vec<String>,
    pub compound_prefixes: Vec<String>,
    pub container_name_previous: Vec<String>,
    pub compact_constructor_containers: Vec<String>,
    pub assignment_continuations: Vec<String>,
    pub control_headers: Vec<String>,
    pub assignment_arrow: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutlineLexicalPlan {
    pub line_comments: Vec<String>,
    pub line_skip_patterns: Vec<String>,
    pub block_comments: Vec<OutlineBlockCommentPlan>,
    pub strings: Vec<OutlineStringPlan>,
    pub raw_strings: Vec<super::schema::RawRawString>,
    pub regex_literals: Vec<super::schema::RawRegexLiteral>,
    pub heredocs: Vec<super::schema::RawHeredoc>,
    pub opaque_blocks: Vec<super::schema::RawOpaqueBlock>,
    pub identifier_prefix: Option<String>,
    pub word_character_extra: String,
    pub unicode_word_characters: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineBlockCommentPlan {
    pub open: String,
    pub close: String,
    pub nested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineStringPlan {
    pub open: String,
    pub close: String,
    pub escape: Option<String>,
    pub requires_closing_on_line: bool,
    pub single_quote_literals: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineStructurePlan {
    pub bodies: Vec<OutlineBodyPlan>,
    pub delimiters: Vec<super::schema::RawDelimiter>,
    pub syntax_tokens: Vec<super::schema::RawSyntaxToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineBodyPlan {
    pub kind: OutlineBodyKind,
    pub open: Option<String>,
    pub close: Option<String>,
    pub end_keyword: Option<String>,
    pub block_openers: Vec<String>,
    pub conditional_openers: Vec<String>,
    pub loop_openers: Vec<String>,
    pub loop_body_keyword: Option<String>,
    pub statement_boundaries: Vec<String>,
    pub member_prefixes: Vec<String>,
    pub header_end: Option<String>,
    pub line_continuation: Option<String>,
}

pub fn compile_outline_schema(
    schema: RawOutlineSchema,
    parse_diagnostics: Vec<OutlineDiagnostic>,
) -> CompiledOutlineRegistry {
    let mut diagnostics = parse_diagnostics;
    let mut plans = Vec::new();

    if schema.schema_version.as_deref() != Some("1") {
        diagnostics.push(diagnostics::warning(
            "outline parser schema version is not supported; attempting best-effort compilation",
        ));
    }

    for language in &schema.languages {
        match compile_language(language, &schema.families) {
            Ok(plan) => plans.push(plan),
            Err(mut language_diagnostics) => diagnostics.append(&mut language_diagnostics),
        }
    }

    if plans.is_empty() {
        diagnostics.push(diagnostics::info(
            "outline parser registry compiled without any language plans",
        ));
    }

    CompiledOutlineRegistry { plans, diagnostics }
}

fn compile_language(
    language: &RawLanguage,
    families: &[RawFamily],
) -> Result<OutlinePlan, Vec<OutlineDiagnostic>> {
    let language_name = language
        .name
        .clone()
        .unwrap_or_else(|| "<unnamed language>".to_owned());
    let mut diagnostics = Vec::new();

    if language.tokens.is_empty() {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} must define at least one syntax token"
        )));
    }

    let Some(use_family) = &language.family else {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} must reference a family"
        )));
        return Err(diagnostics);
    };
    let Some(family_id) = use_family.id.as_deref() else {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} has a family reference without an id"
        )));
        return Err(diagnostics);
    };
    let Some(adapter_name) = use_family.adapter.as_deref() else {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} has a family reference without an adapter"
        )));
        return Err(diagnostics);
    };
    let Some(family) = families
        .iter()
        .find(|family| family.id.as_deref() == Some(family_id))
    else {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} references unknown family {family_id}"
        )));
        return Err(diagnostics);
    };

    if !is_supported_adapter(adapter_name)
        || !family.adapters.iter().any(|name| name == adapter_name)
    {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} uses unsupported adapter {adapter_name}"
        )));
    }

    let structure = compile_structure(family, family_id, &mut diagnostics);
    let allowed_bodies = structure
        .bodies
        .iter()
        .map(|body| body.kind)
        .collect::<Vec<_>>();

    let containers = compile_rules(
        &language.containers,
        &allowed_bodies,
        RuleRole::Container,
        &language_name,
        &mut diagnostics,
    );
    let declarations = compile_rules(
        &language.declarations,
        &allowed_bodies,
        RuleRole::Declaration,
        &language_name,
        &mut diagnostics,
    );

    validate_lexical(&language.lexical, &language_name, &mut diagnostics);
    let members = language
        .members
        .iter()
        .filter_map(|rule| {
            let kinds = parse_node_kind(&rule.kind).zip(parse_node_kind(&rule.within));
            let valid_patterns = [
                &rule.prefix_pattern,
                &rule.name_pattern,
                &rule.line_skip_pattern,
                &rule.generic_open_pattern,
                &rule.generic_suffix_pattern,
            ]
            .into_iter()
            .all(|pattern| {
                pattern.as_ref().is_none_or(|pattern| {
                    regex::Regex::new(&format!("\\A(?:{pattern})"))
                        .is_ok_and(|regex| !regex.is_match(""))
                })
            });
            let valid_name = rule.name_pattern.as_ref().is_none_or(|pattern| {
                regex::Regex::new(pattern).is_ok_and(|regex| {
                    regex
                        .capture_names()
                        .flatten()
                        .any(|name| name == "name" || name.starts_with("name_"))
                })
            });
            let valid_generics = rule.generic_open_pattern.is_some()
                == rule.generic_suffix_pattern.is_some()
                && (rule.generic_open_pattern.is_none()
                    || ["generics-open", "generics-close"]
                        .iter()
                        .all(|role| family.syntax_tokens.iter().any(|token| token.role == *role)));
            if kinds.is_none()
                || rule.separator.is_empty()
                || rule.terminator.as_deref() == Some("")
                || rule.terminator.as_deref() == Some(rule.separator.as_str())
                || !valid_patterns
                || !valid_name
                || !valid_generics
            {
                diagnostics.push(diagnostics::error(format!(
                    "outline language {language_name} has an invalid members rule"
                )));
                return None;
            }
            let (node_kind, within) = kinds.unwrap();
            Some(OutlineMemberPlan {
                node_kind,
                within,
                separator: rule.separator.clone(),
                terminator: rule.terminator.clone(),
                prefix_pattern: rule.prefix_pattern.clone(),
                name_pattern: rule.name_pattern.clone(),
                line_skip_pattern: rule.line_skip_pattern.clone(),
                generic_open_pattern: rule.generic_open_pattern.clone(),
                generic_suffix_pattern: rule.generic_suffix_pattern.clone(),
            })
        })
        .collect();
    for token in &family.syntax_tokens {
        if token.value.chars().count() != 1
            || !matches!(
                token.role.as_str(),
                "parameters-open"
                    | "parameters-close"
                    | "brackets-open"
                    | "brackets-close"
                    | "generics-open"
                    | "generics-close"
                    | "assignment"
                    | "separator"
                    | "statement-end"
                    | "assignment-reject-before"
                    | "assignment-reject-after"
                    | "type-prefix"
                    | "type-suffix"
                    | "attribute-prefix"
            )
        {
            diagnostics.push(diagnostics::error(format!(
                "outline family {family_id} has an invalid syntax-token role or value"
            )));
        }
    }
    for delimiter in &family.delimiters {
        if delimiter.open.is_empty()
            || delimiter.close.is_empty()
            || delimiter.open == delimiter.close
        {
            diagnostics.push(diagnostics::error(format!(
                "outline family {family_id} has an empty or ambiguous delimiter"
            )));
        }
    }

    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == super::types::OutlineDiagnosticSeverity::Error)
    {
        return Err(diagnostics);
    }

    Ok(OutlinePlan {
        language_name,
        syntax_tokens: language.tokens.clone(),
        family_id: family_id.to_owned(),
        adapter_name: adapter_name.to_owned(),
        signature_modifiers: language.signature_modifiers.clone(),
        containers,
        declarations,
        members,
        lexical: compile_lexical(&language.lexical),
        structure,
        diagnostics,
    })
}

fn compile_structure(
    family: &RawFamily,
    family_id: &str,
    diagnostics: &mut Vec<OutlineDiagnostic>,
) -> OutlineStructurePlan {
    let mut bodies = Vec::new();

    if family.bodies.is_empty() {
        diagnostics.push(diagnostics::error(format!(
            "outline family {family_id} must define at least one body kind"
        )));
    }

    for body in &family.bodies {
        match compile_body(body) {
            Ok(body) => bodies.push(body),
            Err(message) => diagnostics.push(diagnostics::error(format!(
                "outline family {family_id} has invalid body: {message}"
            ))),
        }
    }

    OutlineStructurePlan {
        bodies,
        delimiters: family.delimiters.clone(),
        syntax_tokens: family.syntax_tokens.clone(),
    }
}

fn compile_body(body: &RawBody) -> Result<OutlineBodyPlan, String> {
    let kind = parse_body_kind(
        body.kind
            .as_deref()
            .ok_or_else(|| "body is missing kind".to_owned())?,
    )
    .ok_or_else(|| {
        format!(
            "unsupported body kind {:?}",
            body.kind.as_deref().unwrap_or("")
        )
    })?;

    match kind {
        OutlineBodyKind::Brace => {
            if body.open.as_deref().unwrap_or("").is_empty()
                || body.close.as_deref().unwrap_or("").is_empty()
                || body.open == body.close
            {
                return Err(
                    "brace body must define distinct non-empty open and close delimiters"
                        .to_owned(),
                );
            }
        }
        OutlineBodyKind::EndKeyword => {
            if body.end_keyword.as_deref().unwrap_or("").is_empty() {
                return Err("end-keyword body must define an end-keyword".to_owned());
            }
        }
        OutlineBodyKind::Indent => {
            if body.header_end.as_deref() == Some("")
                || body.line_continuation.as_deref() == Some("")
            {
                return Err("indent body has an empty header or continuation token".to_owned());
            }
        }
        OutlineBodyKind::None => {}
    }

    Ok(OutlineBodyPlan {
        kind,
        open: body.open.clone(),
        close: body.close.clone(),
        end_keyword: body.end_keyword.clone(),
        block_openers: body.block_openers.clone(),
        conditional_openers: body.conditional_openers.clone(),
        loop_openers: body.loop_openers.clone(),
        loop_body_keyword: body.loop_body_keyword.clone(),
        statement_boundaries: body.statement_boundaries.clone(),
        member_prefixes: body.member_prefixes.clone(),
        header_end: body.header_end.clone(),
        line_continuation: body.line_continuation.clone(),
    })
}

fn compile_rules(
    rules: &[RawRule],
    allowed_bodies: &[OutlineBodyKind],
    role: RuleRole,
    language_name: &str,
    diagnostics: &mut Vec<OutlineDiagnostic>,
) -> Vec<OutlineRulePlan> {
    rules
        .iter()
        .filter_map(|rule| compile_rule(rule, allowed_bodies, role, language_name, diagnostics))
        .collect()
}

fn compile_rule(
    rule: &RawRule,
    allowed_bodies: &[OutlineBodyKind],
    role: RuleRole,
    language_name: &str,
    diagnostics: &mut Vec<OutlineDiagnostic>,
) -> Option<OutlineRulePlan> {
    let rule_label = role.label();
    if let Some(pattern) = &rule.name_pattern {
        match regex::Regex::new(pattern) {
            Ok(pattern)
                if !pattern.is_match("")
                    && pattern
                        .capture_names()
                        .flatten()
                        .any(|name| name == "name" || name.starts_with("name_")) => {}
            Ok(_) => {
                diagnostics.push(diagnostics::error(format!(
                    "outline language {language_name} has empty-matching {rule_label} name-pattern or no named name capture"
                )));
                return None;
            }
            Err(error) => {
                diagnostics.push(diagnostics::error(format!(
                    "outline language {language_name} has invalid {rule_label} name-pattern: {error}"
                )));
                return None;
            }
        }
    }
    if let Some(pattern) = &rule.signature_brace_prefix_pattern
        && let Err(error) = regex::Regex::new(pattern)
    {
        diagnostics.push(diagnostics::error(format!("outline language {language_name} has invalid {rule_label} signature-brace-prefix-pattern: {error}")));
        return None;
    }
    let Some(node_kind) = rule.kind.as_deref().and_then(parse_node_kind) else {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} has {rule_label} with unsupported node kind {:?}",
            rule.kind.as_deref().unwrap_or("")
        )));
        return None;
    };
    let scan = match rule.scan.as_deref().unwrap_or("keyword") {
        "keyword" => OutlineScanMode::Keyword,
        "callable" if role == RuleRole::Declaration => OutlineScanMode::Callable,
        "callable" => {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language_name} has {rule_label} with callable scan; only declarations support callable scan"
            )));
            return None;
        }
        value => {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language_name} has {rule_label} with unsupported scan mode {value}"
            )));
            return None;
        }
    };
    let keyword = rule
        .keyword
        .as_deref()
        .map(parse_keyword_sequence)
        .unwrap_or_default();
    if scan == OutlineScanMode::Keyword && keyword.is_empty() {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} has {rule_label} with an empty keyword"
        )));
        return None;
    }
    let name = match rule.name.as_deref().unwrap_or("after-keyword") {
        "after-keyword" => OutlineNameCapture::AfterKeyword,
        "before-parameters" if scan == OutlineScanMode::Callable => {
            OutlineNameCapture::BeforeParameters
        }
        "before-parameters" => {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language_name} has {rule_label} with before-parameters name capture outside callable scan"
            )));
            return None;
        }
        value => {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language_name} has {rule_label} with unsupported name capture {value}"
            )));
            return None;
        }
    };
    let Some(body) = parse_body_kind(rule.body.as_deref().unwrap_or("none")) else {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} has {rule_label} with unsupported body {:?}",
            rule.body.as_deref().unwrap_or("")
        )));
        return None;
    };
    if body != OutlineBodyKind::None && !allowed_bodies.contains(&body) {
        diagnostics.push(diagnostics::error(format!(
            "outline language {language_name} has {rule_label} body {:?} not provided by its family",
            rule.body.as_deref().unwrap_or("")
        )));
        return None;
    }

    let method_containers = rule
        .method_containers
        .iter()
        .filter_map(|container| {
            parse_node_kind(container).or_else(|| {
                diagnostics.push(diagnostics::warning(format!(
                    "outline language {language_name} has {rule_label} with unsupported method container {container}"
                )));
                None
            })
        })
        .collect();
    let callable = if scan == OutlineScanMode::Callable {
        OutlineCallablePlan {
            reject_names: rule.reject_names.clone(),
            reject_previous: rule.reject_previous.clone(),
            reject_prefixes: rule.reject_prefixes.clone(),
            require_previous: rule.require_previous.clone(),
            require_non_container_previous: rule.require_non_container_previous.clone(),
            require_non_container_previous_kind: rule.require_non_container_previous_kind.clone(),
            start_boundaries: rule.start_boundaries.clone(),
            operator_tokens: rule.operator_tokens.clone(),
            name_prefixes: rule.name_prefixes.clone(),
            qualified_separators: rule.qualified_separators.clone(),
            compound_prefixes: rule.compound_prefixes.clone(),
            container_name_previous: rule.container_name_previous.clone(),
            compact_constructor_containers: rule.compact_constructor_containers.clone(),
            assignment_continuations: rule.assignment_continuations.clone(),
            control_headers: rule.control_headers.clone(),
            assignment_arrow: rule.assignment_arrow.clone(),
        }
    } else {
        OutlineCallablePlan::default()
    };

    Some(OutlineRulePlan {
        node_kind,
        keyword,
        name,
        scan,
        callable,
        body,
        method_containers,
        declaration_terminator: rule.declaration_terminator.clone(),
        name_pattern: rule.name_pattern.clone(),
        reject_previous: rule.keyword_reject_previous.clone(),
        reject_next: rule.keyword_reject_next.clone(),
        require_statement_start: rule.require_statement_start,
        signature_type_braces: rule.signature_type_braces,
        signature_brace_prefix_pattern: rule.signature_brace_prefix_pattern.clone(),
        nextline_body: rule.nextline_body,
        expression_body: rule.expression_body.clone(),
    })
}

fn validate_lexical(
    lexical: &RawLexical,
    language: &str,
    diagnostics: &mut Vec<OutlineDiagnostic>,
) {
    let invalid = lexical.line_comments.iter().any(String::is_empty)
        || lexical
            .block_comments
            .iter()
            .any(|comment| comment.open.is_empty() || comment.close.is_empty())
        || lexical.strings.iter().any(|string| {
            string.open.is_empty()
                || string.close.is_empty()
                || string.escape.as_deref() == Some("")
        })
        || lexical.raw_strings.iter().any(|raw| {
            raw.prefixes.is_empty()
                || raw.prefixes.iter().any(String::is_empty)
                || raw.open.is_empty()
                || raw.close.is_empty()
                || raw.repeat.as_deref() == Some("")
        })
        || lexical.regex_literals.iter().any(|literal| {
            literal.open.is_empty()
                || literal.close.is_empty()
                || literal.escape.as_deref() == Some("")
                || literal.character_class_open.as_deref() == Some("")
                || literal.character_class_close.as_deref() == Some("")
                || literal.character_class_open.is_some() != literal.character_class_close.is_some()
        });
    if invalid {
        diagnostics.push(diagnostics::error(format!("outline language {language} has an empty lexical delimiter or incomplete raw-string rule")));
    }
    for pattern in lexical
        .regex_literals
        .iter()
        .filter_map(|literal| literal.prefix_pattern.as_ref())
    {
        if let Err(error) = regex::Regex::new(pattern) {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language} has invalid regex-literal prefix-pattern: {error}"
            )));
        }
    }
    for pattern in &lexical.line_skip_patterns {
        if pattern.is_empty() {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language} has empty line-skip-pattern"
            )));
        } else if let Err(error) = regex::Regex::new(pattern) {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language} has invalid line-skip-pattern: {error}"
            )));
        }
    }
    for heredoc in &lexical.heredocs {
        match regex::Regex::new(&heredoc.prefix_pattern) {
            Ok(pattern) if !heredoc.prefix_pattern.is_empty() && !pattern.is_match("") && pattern.capture_names().flatten().any(|name| name == "delimiter" || name.starts_with("delimiter_")) => {}
            Ok(_) => diagnostics.push(diagnostics::error(format!("outline language {language} has heredoc prefix-pattern without a named delimiter capture"))),
            Err(error) => diagnostics.push(diagnostics::error(format!("outline language {language} has invalid heredoc prefix-pattern: {error}"))),
        }
    }
    for block in &lexical.opaque_blocks {
        if block.open.is_empty() || block.close.is_empty() || block.open == block.close {
            diagnostics.push(diagnostics::error(format!(
                "outline language {language} has empty or ambiguous opaque-block delimiters"
            )));
        }
        match regex::Regex::new(&block.prefix_pattern) {
            Ok(pattern) if !block.prefix_pattern.is_empty() && !pattern.is_match("") => {}
            Ok(_) => diagnostics.push(diagnostics::error(format!(
                "outline language {language} has empty-matching opaque-block prefix-pattern"
            ))),
            Err(error) => diagnostics.push(diagnostics::error(format!(
                "outline language {language} has invalid opaque-block prefix-pattern: {error}"
            ))),
        }
    }
}

fn compile_lexical(lexical: &RawLexical) -> OutlineLexicalPlan {
    OutlineLexicalPlan {
        line_comments: lexical.line_comments.clone(),
        line_skip_patterns: lexical.line_skip_patterns.clone(),
        block_comments: lexical
            .block_comments
            .iter()
            .map(|comment| OutlineBlockCommentPlan {
                open: comment.open.clone(),
                close: comment.close.clone(),
                nested: comment.nested,
            })
            .collect(),
        strings: lexical.strings.iter().map(compile_string).collect(),
        raw_strings: lexical.raw_strings.clone(),
        regex_literals: lexical.regex_literals.clone(),
        heredocs: lexical.heredocs.clone(),
        opaque_blocks: lexical.opaque_blocks.clone(),
        identifier_prefix: lexical.identifier_prefix.clone(),
        word_character_extra: lexical.word_character_extra.clone().unwrap_or_default(),
        unicode_word_characters: lexical.unicode_word_characters,
    }
}

fn compile_string(string: &RawString) -> OutlineStringPlan {
    OutlineStringPlan {
        open: string.open.clone(),
        close: string.close.clone(),
        escape: string.escape.clone(),
        requires_closing_on_line: string.requires_closing_on_line,
        single_quote_literals: string.single_quote_literals,
    }
}

fn parse_keyword_sequence(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_node_kind(value: &str) -> Option<OutlineNodeKind> {
    match value {
        "module" => Some(OutlineNodeKind::Module),
        "namespace" => Some(OutlineNodeKind::Namespace),
        "class" => Some(OutlineNodeKind::Class),
        "enum" => Some(OutlineNodeKind::Enum),
        "enum-member" => Some(OutlineNodeKind::EnumMember),
        "interface" => Some(OutlineNodeKind::Interface),
        "trait" => Some(OutlineNodeKind::Trait),
        "impl" => Some(OutlineNodeKind::Impl),
        "method" => Some(OutlineNodeKind::Method),
        "constructor" => Some(OutlineNodeKind::Constructor),
        "function" => Some(OutlineNodeKind::Function),
        "declaration" => Some(OutlineNodeKind::Declaration),
        "tag" => Some(OutlineNodeKind::Tag),
        "section" => Some(OutlineNodeKind::Section),
        "unknown" => Some(OutlineNodeKind::Unknown),
        _ => None,
    }
}

fn parse_body_kind(value: &str) -> Option<OutlineBodyKind> {
    match value {
        "brace" => Some(OutlineBodyKind::Brace),
        "indent" => Some(OutlineBodyKind::Indent),
        "end-keyword" => Some(OutlineBodyKind::EndKeyword),
        "none" => Some(OutlineBodyKind::None),
        _ => None,
    }
}

fn is_supported_adapter(value: &str) -> bool {
    matches!(
        value,
        "rust"
            | "python"
            | "javascript"
            | "ruby"
            | "generic-brace"
            | "generic-indent"
            | "generic-end-keyword"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleRole {
    Container,
    Declaration,
}

impl RuleRole {
    fn label(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Declaration => "declaration",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::outline::schema::parse_outline_schema;

    #[test]
    fn invalid_schema_entries_report_diagnostics_without_panicking() {
        let xml = r#"
            <outline-parsers schema-version="1">
              <family id="brace">
                <adapter name="rust" />
                <body kind="brace" open="{" close="}" />
              </family>
              <language name="Broken">
                <token value="broken" />
                <use-family id="missing" adapter="unknown" />
                <declaration kind="nope" keyword="fn" body="brace" />
              </language>
            </outline-parsers>
        "#;
        let (schema, parse_diagnostics) = parse_outline_schema(xml);
        let compiled = compile_outline_schema(schema, parse_diagnostics);

        assert!(compiled.plans.is_empty());
        assert!(!compiled.diagnostics.is_empty());
    }
}
