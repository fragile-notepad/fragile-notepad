use std::collections::VecDeque;
use std::ops::{ControlFlow, Range};

use regex::{Regex, RegexBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMatch {
    pub start: usize,
    pub end: usize,
}

impl TextMatch {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn range(self) -> Range<usize> {
        self.start..self.end
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindState {
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub mode: SearchMode,
    pub wrap_around: bool,
    pub error: Option<SearchError>,
    pub matches: Vec<TextMatch>,
    pub current_match: Option<usize>,
    /// A match selected by navigation, including a zero-width match at the caret.
    pub navigation_match: Option<TextMatch>,
    pub matches_stale: bool,
    /// Identifies the source buffer for navigation, when used by an editor.
    pub match_context: Option<u64>,
    pub match_limit: Option<usize>,
    pub matches_limited: bool,
}

impl Default for FindState {
    fn default() -> Self {
        Self {
            query: String::new(),
            replacement: String::new(),
            case_sensitive: false,
            whole_word: false,
            mode: SearchMode::Normal,
            wrap_around: true,
            error: None,
            matches: Vec::new(),
            current_match: None,
            navigation_match: None,
            matches_stale: false,
            match_context: None,
            match_limit: None,
            matches_limited: false,
        }
    }
}

impl FindState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_query(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            ..Self::default()
        }
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.clear_matches();
    }

    pub fn set_replacement(&mut self, replacement: impl Into<String>) {
        self.replacement = replacement.into();
    }

    pub fn set_case_sensitive(&mut self, case_sensitive: bool) {
        if self.case_sensitive != case_sensitive {
            self.case_sensitive = case_sensitive;
            self.clear_matches();
        }
    }

    pub fn set_whole_word(&mut self, whole_word: bool) {
        if self.whole_word != whole_word {
            self.whole_word = whole_word;
            self.clear_matches();
        }
    }

    pub fn set_mode(&mut self, mode: SearchMode) {
        if self.mode != mode {
            self.mode = mode;
            self.clear_matches();
        }
    }

    pub fn options(&self) -> SearchOptions {
        SearchOptions {
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            mode: self.mode,
        }
    }

    fn clear_matches(&mut self) {
        self.current_match = None;
        self.navigation_match = None;
        self.matches.clear();
        self.error = None;
        self.matches_stale = true;
        self.matches_limited = false;
    }

    pub fn refresh_matches(&mut self, text: &str) {
        self.refresh_matches_in_chunks([text]);
    }

    pub fn refresh_matches_in_chunks<'a>(&mut self, chunks: impl IntoIterator<Item = &'a str>) {
        self.matches_stale = false;
        self.error = None;
        self.matches_limited = false;
        self.matches = match PreparedSearch::new(&self.query, self.options()) {
            Ok(Some(search)) => {
                let mut matches = Vec::new();
                let _ = search.try_for_each_match_in_chunks(chunks, |found| {
                    if self.match_limit.is_some_and(|limit| matches.len() >= limit) {
                        self.matches_limited = true;
                        return ControlFlow::Break(());
                    }
                    matches.push(found);
                    ControlFlow::Continue(())
                });
                matches
            }
            Ok(None) => Vec::new(),
            Err(error) => {
                self.error = Some(error);
                Vec::new()
            }
        };
        self.navigation_match = self
            .navigation_match
            .filter(|found| self.matches.contains(found));
        self.current_match = match (self.current_match, self.matches.is_empty()) {
            (_, true) => None,
            (Some(index), false) => Some(index.min(self.matches.len() - 1)),
            (None, false) => Some(0),
        };
    }

    pub fn current(&self) -> Option<TextMatch> {
        self.current_match
            .and_then(|index| self.matches.get(index).copied())
    }

    pub fn next(&mut self) -> Option<TextMatch> {
        if self.matches.is_empty() {
            self.current_match = None;
            return None;
        }

        let next = match self.current_match {
            Some(index) if index + 1 < self.matches.len() => index + 1,
            Some(_) if !self.wrap_around => return None,
            Some(_) | None => 0,
        };

        self.current_match = Some(next);
        self.current()
    }

    pub fn previous(&mut self) -> Option<TextMatch> {
        if self.matches.is_empty() {
            self.current_match = None;
            return None;
        }

        let previous = match self.current_match {
            Some(index) if index > 0 => index - 1,
            Some(_) if !self.wrap_around => return None,
            Some(_) => self.matches.len() - 1,
            None => 0,
        };

        self.current_match = Some(previous);
        self.current()
    }

    pub fn replace_current(&mut self, text: &str) -> Option<String> {
        let current = self.current()?;
        let replacement = replacement_for_match(
            text,
            current,
            &self.query,
            &self.replacement,
            self.options(),
        )
        .ok()?;
        let replaced = replace_current(text, current, &replacement)?;

        self.refresh_matches(&replaced);

        Some(replaced)
    }

    pub fn replace_all(&mut self, text: &str) -> (String, usize) {
        let Ok(Some(search)) = PreparedSearch::new(&self.query, self.options()) else {
            self.refresh_matches(text);
            return (text.to_owned(), 0);
        };
        let matches = search.matches(text);

        if matches.is_empty() {
            self.matches.clear();
            self.current_match = None;
            self.navigation_match = None;
            self.matches_limited = false;
            self.matches_stale = false;
            return (text.to_owned(), 0);
        }

        let mut replaced = String::with_capacity(text.len());
        let mut cursor = 0;

        for text_match in &matches {
            replaced.push_str(&text[cursor..text_match.start]);
            replaced.push_str(&search.replacement_for_match(text, *text_match, &self.replacement));
            cursor = text_match.end;
        }

        replaced.push_str(&text[cursor..]);

        let count = matches.len();
        self.refresh_matches(&replaced);

        (replaced, count)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Normal,
    Extended,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    InvalidRegex(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub mode: SearchMode,
}

impl SearchOptions {
    pub const fn normal(case_sensitive: bool, whole_word: bool) -> Self {
        Self {
            case_sensitive,
            whole_word,
            mode: SearchMode::Normal,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedSearch {
    matcher: PreparedMatcher,
    options: SearchOptions,
}

#[derive(Debug, Clone)]
enum PreparedMatcher {
    Literal(String),
    Regex(Regex),
}

impl PreparedSearch {
    pub fn new(query: &str, options: SearchOptions) -> Result<Option<Self>, SearchError> {
        if query.is_empty() {
            return Ok(None);
        }

        let matcher = match options.mode {
            SearchMode::Normal => PreparedMatcher::Literal(query.to_owned()),
            SearchMode::Extended => {
                let query = expand_extended_pattern(query);

                if query.is_empty() {
                    return Ok(None);
                }

                PreparedMatcher::Literal(query)
            }
            SearchMode::Regex => PreparedMatcher::Regex(build_regex(query, options)?),
        };

        Ok(Some(Self { matcher, options }))
    }

    pub fn matches(&self, text: &str) -> Vec<TextMatch> {
        match &self.matcher {
            PreparedMatcher::Literal(query) => compute_literal_matches(text, query, self.options),
            PreparedMatcher::Regex(regex) => compute_regex_matches(text, regex, self.options),
        }
    }

    pub fn matches_in_chunks<'a>(
        &self,
        chunks: impl IntoIterator<Item = &'a str>,
    ) -> Vec<TextMatch> {
        let mut matches = Vec::new();
        self.for_each_match_in_chunks(chunks, |found| matches.push(found));
        matches
    }

    /// Visits matches without allocating a collection proportional to their count.
    pub fn for_each_match_in_chunks<'a>(
        &self,
        chunks: impl IntoIterator<Item = &'a str>,
        mut visit: impl FnMut(TextMatch),
    ) {
        let _ = self.try_for_each_match_in_chunks(chunks, |found| {
            visit(found);
            ControlFlow::<()>::Continue(())
        });
    }

    /// Stops scanning as soon as the visitor returns `ControlFlow::Break`.
    pub fn try_for_each_match_in_chunks<'a, B>(
        &self,
        chunks: impl IntoIterator<Item = &'a str>,
        visit: impl FnMut(TextMatch) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        match &self.matcher {
            PreparedMatcher::Literal(query) => {
                visit_literal_matches_in_chunks(chunks, query, self.options, visit)
            }
            PreparedMatcher::Regex(regex) => {
                let text = chunks.into_iter().collect::<String>();
                visit_regex_matches(&text, regex, self.options, visit)
            }
        }
    }

    pub fn replacement_for_match(
        &self,
        text: &str,
        text_match: TextMatch,
        replacement: &str,
    ) -> String {
        match &self.matcher {
            PreparedMatcher::Literal(_) if self.options.mode == SearchMode::Extended => {
                expand_extended_pattern(replacement)
            }
            PreparedMatcher::Literal(_) => replacement.to_owned(),
            PreparedMatcher::Regex(regex) => {
                regex_replacement_for_match(text, text_match, regex, replacement)
                    .unwrap_or_else(|| replacement.to_owned())
            }
        }
    }
}

pub fn compute_matches(text: &str, query: &str, case_sensitive: bool) -> Vec<TextMatch> {
    compute_matches_with_options(text, query, SearchOptions::normal(case_sensitive, false))
}

pub fn compute_matches_with_options(
    text: &str,
    query: &str,
    options: SearchOptions,
) -> Vec<TextMatch> {
    try_matches(text, query, options).unwrap_or_default()
}

pub fn compute_matches_in_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a str>,
    query: &str,
    options: SearchOptions,
) -> Vec<TextMatch> {
    let Ok(Some(search)) = PreparedSearch::new(query, options) else {
        return Vec::new();
    };

    search.matches_in_chunks(chunks)
}

pub fn try_matches(
    text: &str,
    query: &str,
    options: SearchOptions,
) -> Result<Vec<TextMatch>, SearchError> {
    Ok(PreparedSearch::new(query, options)?
        .map(|prepared| prepared.matches(text))
        .unwrap_or_default())
}

fn compute_literal_matches(text: &str, query: &str, options: SearchOptions) -> Vec<TextMatch> {
    if options.case_sensitive {
        return text
            .match_indices(query)
            .filter(|(start, value)| {
                !options.whole_word || is_whole_word_match(text, *start, start + value.len())
            })
            .map(|(start, value)| TextMatch::new(start, start + value.len()))
            .collect();
    }

    compute_literal_matches_in_chunks([text], query, options)
}

fn compute_literal_matches_in_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a str>,
    query: &str,
    options: SearchOptions,
) -> Vec<TextMatch> {
    let mut matches = Vec::new();
    let _ = visit_literal_matches_in_chunks(chunks, query, options, |found| {
        matches.push(found);
        ControlFlow::<()>::Continue(())
    });
    matches
}

fn visit_literal_matches_in_chunks<'a, B>(
    chunks: impl IntoIterator<Item = &'a str>,
    query: &str,
    options: SearchOptions,
    mut visit: impl FnMut(TextMatch) -> ControlFlow<B>,
) -> ControlFlow<B> {
    if query.is_empty() {
        return ControlFlow::Continue(());
    }

    // Keep a single scan cursor across chunks, plus both word-boundary neighbors.
    // Re-scanning overlapping chunk suffixes can change non-overlap semantics.
    let query_chars: Vec<char> = query.chars().collect();
    let query_folded = query.to_lowercase();
    let width = query_chars.len();
    let mut input = chunks.into_iter().flat_map(str::chars);
    let mut window = VecDeque::with_capacity(width + 1);
    let mut absolute_offset = 0usize;
    let mut before = None;
    loop {
        while window.len() <= width {
            let Some(ch) = input.next() else { break };
            window.push_back(ch);
        }
        if window.len() < width {
            break;
        }
        let candidate = window.iter().take(width);
        let equal = if options.case_sensitive {
            candidate.copied().eq(query_chars.iter().copied())
        } else if query.is_ascii() && candidate.clone().all(|ch| ch.is_ascii()) {
            candidate
                .zip(&query_chars)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
        } else {
            candidate.collect::<String>().to_lowercase() == query_folded
        };
        let whole_word = !options.whole_word
            || (!before.is_some_and(is_word_char)
                && !window.get(width).copied().is_some_and(is_word_char));
        let accepted = equal && whole_word;
        let advance = if accepted || (equal && options.case_sensitive) {
            width
        } else {
            1
        };
        let start = absolute_offset;
        for _ in 0..advance {
            let ch = window.pop_front().expect("complete search window");
            absolute_offset += ch.len_utf8();
            before = Some(ch);
        }
        if accepted {
            visit(TextMatch::new(start, absolute_offset))?;
        }
    }
    ControlFlow::Continue(())
}

fn compute_regex_matches(text: &str, regex: &Regex, options: SearchOptions) -> Vec<TextMatch> {
    let mut matches = Vec::new();
    let _ = visit_regex_matches(text, regex, options, |found| {
        matches.push(found);
        ControlFlow::<()>::Continue(())
    });
    matches
}

fn visit_regex_matches<B>(
    text: &str,
    regex: &Regex,
    options: SearchOptions,
    mut visit: impl FnMut(TextMatch) -> ControlFlow<B>,
) -> ControlFlow<B> {
    for found in regex.find_iter(text) {
        if !options.whole_word || is_whole_word_match(text, found.start(), found.end()) {
            visit(TextMatch::new(found.start(), found.end()))?;
        }
    }
    ControlFlow::Continue(())
}

pub fn replacement_for_match(
    text: &str,
    text_match: TextMatch,
    query: &str,
    replacement: &str,
    options: SearchOptions,
) -> Result<String, SearchError> {
    Ok(PreparedSearch::new(query, options)?
        .map(|prepared| prepared.replacement_for_match(text, text_match, replacement))
        .unwrap_or_else(|| replacement.to_owned()))
}

fn build_regex(query: &str, options: SearchOptions) -> Result<Regex, SearchError> {
    RegexBuilder::new(query)
        .case_insensitive(!options.case_sensitive)
        // Native Windows line endings are one boundary in the editor too.
        .crlf(true)
        .build()
        .map_err(|error| SearchError::InvalidRegex(error.to_string()))
}

fn regex_replacement_for_match(
    text: &str,
    text_match: TextMatch,
    regex: &Regex,
    replacement: &str,
) -> Option<String> {
    if text_match.end > text.len()
        || text_match.start > text_match.end
        || !text.is_char_boundary(text_match.start)
        || !text.is_char_boundary(text_match.end)
    {
        return None;
    }

    let captures = regex.captures_at(text, text_match.start)?;
    let full_match = captures.get(0)?;

    if full_match.start() != text_match.start || full_match.end() != text_match.end {
        return None;
    }

    let mut expanded = String::new();
    captures.expand(replacement, &mut expanded);
    Some(expanded)
}

pub fn replace_current(text: &str, text_match: TextMatch, replacement: &str) -> Option<String> {
    if text_match.end > text.len()
        || text_match.start > text_match.end
        || !text.is_char_boundary(text_match.start)
        || !text.is_char_boundary(text_match.end)
    {
        return None;
    }

    let mut replaced = String::with_capacity(text.len() + replacement.len());
    replaced.push_str(&text[..text_match.start]);
    replaced.push_str(replacement);
    replaced.push_str(&text[text_match.end..]);

    Some(replaced)
}

pub fn replace_all(
    text: &str,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
) -> (String, usize) {
    let mut state = FindState {
        query: query.to_owned(),
        replacement: replacement.to_owned(),
        case_sensitive,
        whole_word: false,
        ..FindState::default()
    };

    state.replace_all(text)
}

fn is_whole_word_match(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();

    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn expand_extended_pattern(value: &str) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            expanded.push(ch);
            continue;
        }

        match chars.next() {
            Some('n') => expanded.push('\n'),
            Some('r') => expanded.push('\r'),
            Some('t') => expanded.push('\t'),
            Some('0') => expanded.push('\0'),
            Some('\\') => expanded.push('\\'),
            Some(other) => {
                expanded.push('\\');
                expanded.push(other);
            }
            None => expanded.push('\\'),
        }
    }

    expanded
}
