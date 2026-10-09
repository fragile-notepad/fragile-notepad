use crate::core::{
    Document, DocumentId, PreparedSearch, SearchMode, SearchOptions, SearchResultSettings,
    TextMatch, Workspace,
};
use crate::editor::EditorSelection;
use crate::message::AdvancedSearchTab;

pub(crate) fn parse_result_settings(
    result_limit_input: &str,
    preview_chars_input: &str,
    context_before_input: &str,
) -> Result<SearchResultSettings, &'static str> {
    let result_limit = result_limit_input
        .parse::<usize>()
        .ok()
        .filter(|value| {
            (SearchResultSettings::MIN_RESULT_LIMIT..=SearchResultSettings::MAX_RESULT_LIMIT)
                .contains(value)
        })
        .ok_or("Maximum results must be between 1 and 10,000.")?;
    let preview_chars = preview_chars_input
        .parse::<usize>()
        .ok()
        .filter(|value| {
            (SearchResultSettings::MIN_PREVIEW_CHARS..=SearchResultSettings::MAX_PREVIEW_CHARS)
                .contains(value)
        })
        .ok_or("Preview length must be between 1 and 2,000 characters.")?;
    let context_before = context_before_input
        .parse::<usize>()
        .ok()
        .filter(|value| *value < preview_chars)
        .ok_or("Context before match must be 0 or more and shorter than the preview.")?;
    Ok(SearchResultSettings {
        result_limit,
        preview_chars,
        context_before,
    })
}

#[derive(Debug, Clone)]
pub struct SearchDialogState {
    pub active_tab: AdvancedSearchTab,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub wrap_around: bool,
    pub mode: SearchMode,
    pub include_pattern: String,
    pub results: Vec<SearchResult>,
    pub selected_result: Option<usize>,
    pub match_count: usize,
    pub count_summary: Option<SearchCountSummary>,
    pub matches_limited: bool,
    pub preview_generation: u64,
    pub(crate) result_documents: Vec<SearchDocumentSnapshot>,
    pub result_settings: SearchResultSettings,
    pub result_limit_input: String,
    pub preview_chars_input: String,
    pub context_before_input: String,
    pub result_options_visible: bool,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub document_id: DocumentId,
    pub document_title: String,
    pub selection: EditorSelection,
    pub preview: String,
    pub preview_match: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCountSummary {
    pub total_matches: usize,
    pub matched_documents: usize,
    pub per_document: Vec<SearchDocumentCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocumentCount {
    pub document_id: DocumentId,
    pub title: String,
    pub match_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchDocumentSnapshot {
    pub id: DocumentId,
    pub revision: u64,
    pub title: String,
    pub complete: bool,
}

impl SearchDocumentSnapshot {
    pub(crate) fn new(document: &Document) -> Self {
        Self {
            id: document.id,
            revision: document.revision(),
            title: document.title(),
            complete: document.has_complete_text_index(),
        }
    }
}

impl SearchDialogState {
    pub fn new() -> Self {
        Self {
            active_tab: AdvancedSearchTab::Find,
            query: String::new(),
            replacement: String::new(),
            case_sensitive: false,
            whole_word: false,
            wrap_around: true,
            mode: SearchMode::Normal,
            include_pattern: String::new(),
            results: Vec::new(),
            selected_result: None,
            match_count: 0,
            count_summary: None,
            matches_limited: false,
            preview_generation: 0,
            result_documents: Vec::new(),
            result_settings: SearchResultSettings::default(),
            result_limit_input: SearchResultSettings::DEFAULT_RESULT_LIMIT.to_string(),
            preview_chars_input: SearchResultSettings::DEFAULT_PREVIEW_CHARS.to_string(),
            context_before_input: SearchResultSettings::DEFAULT_CONTEXT_BEFORE.to_string(),
            result_options_visible: false,
            status: String::from("No query"),
        }
    }

    pub fn options(&self) -> SearchOptions {
        SearchOptions {
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            mode: self.mode,
        }
    }

    pub fn request_snapshot(&self) -> Self {
        Self {
            active_tab: self.active_tab,
            query: self.query.clone(),
            replacement: self.replacement.clone(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            wrap_around: self.wrap_around,
            mode: self.mode,
            include_pattern: self.include_pattern.clone(),
            result_settings: self.result_settings,
            result_limit_input: self.result_limit_input.clone(),
            preview_chars_input: self.preview_chars_input.clone(),
            context_before_input: self.context_before_input.clone(),
            result_options_visible: self.result_options_visible,
            results: Vec::new(),
            selected_result: None,
            match_count: 0,
            count_summary: None,
            matches_limited: false,
            preview_generation: self.preview_generation,
            result_documents: Vec::new(),
            status: String::new(),
        }
    }

    pub fn set_active_tab(&mut self, tab: AdvancedSearchTab) {
        if self.active_tab != tab {
            self.count_summary = None;
        }
        self.active_tab = tab;
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        let query = query.into();
        self.query = query;
        self.clear_results();
        self.update_ready_status();
    }

    pub fn set_replacement(&mut self, replacement: impl Into<String>) {
        self.replacement = replacement.into();
    }

    pub fn set_case_sensitive(&mut self, case_sensitive: bool) {
        self.case_sensitive = case_sensitive;
        self.clear_results();
        self.update_ready_status();
    }

    pub fn set_whole_word(&mut self, whole_word: bool) {
        self.whole_word = whole_word;
        self.clear_results();
        self.update_ready_status();
    }

    pub fn set_wrap_around(&mut self, wrap_around: bool) {
        self.wrap_around = wrap_around;
    }

    pub fn set_mode(&mut self, mode: SearchMode) {
        self.mode = mode;
        self.clear_results();
        self.update_ready_status();
    }

    pub fn set_include_pattern(&mut self, include_pattern: impl Into<String>) {
        self.include_pattern = include_pattern.into();
        self.clear_results();
        self.update_ready_status();
    }

    pub fn refresh_from_workspace(&mut self, workspace: &Workspace) {
        let include_pattern = self.include_pattern.clone();

        self.refresh_from_documents(
            workspace
                .documents()
                .iter()
                .filter(|document| include_filter_matches(document, &include_pattern)),
        );
    }

    pub fn refresh_from_documents<'a>(
        &mut self,
        documents: impl IntoIterator<Item = &'a Document>,
    ) {
        self.search_documents(documents, self.result_settings.normalized().result_limit);
    }

    pub fn count_from_documents<'a>(&mut self, documents: impl IntoIterator<Item = &'a Document>) {
        self.search_documents(documents, 0);
    }

    pub fn clear_results(&mut self) {
        self.results.clear();
        self.selected_result = None;
        self.match_count = 0;
        self.count_summary = None;
        self.matches_limited = false;
        self.result_documents.clear();
    }

    fn update_ready_status(&mut self) {
        self.status = match PreparedSearch::new(&self.query, self.options()) {
            Ok(Some(_)) => String::from("Ready"),
            Ok(None) => String::from("No query"),
            Err(error) => search_error_status(error),
        };
    }

    pub fn set_result_settings(&mut self, settings: SearchResultSettings) {
        self.result_settings = settings.normalized();
        self.result_limit_input = self.result_settings.result_limit.to_string();
        self.preview_chars_input = self.result_settings.preview_chars.to_string();
        self.context_before_input = self.result_settings.context_before.to_string();
    }

    pub fn parsed_result_settings(&self) -> Result<SearchResultSettings, &'static str> {
        parse_result_settings(
            &self.result_limit_input,
            &self.preview_chars_input,
            &self.context_before_input,
        )
    }

    fn search_documents<'a>(
        &mut self,
        documents: impl IntoIterator<Item = &'a Document>,
        result_limit: usize,
    ) {
        if self.query.is_empty() {
            self.clear_results();
            self.status = String::from("No query");
            return;
        }

        match document_results(
            documents,
            &self.query,
            self.options(),
            self.result_settings.normalized(),
            result_limit,
        ) {
            Ok(SearchResults {
                results,
                match_count,
                incomplete_documents,
                documents,
                count_summary,
                matches_limited,
            }) => {
                self.results = results;
                self.selected_result = None;
                self.result_documents = documents;
                self.match_count = match_count;
                self.count_summary = count_summary;
                self.matches_limited = matches_limited;
                let mut base_status = match (match_count, matches_limited) {
                    (count, true) => format!("{count}+ matches"),
                    (0, false) => String::from("No matches"),
                    (1, false) => String::from("1 match"),
                    (count, false) => format!("{count} matches"),
                };
                if result_limit > 0 && match_count > self.results.len() {
                    base_status.push_str(&format!(" (showing first {})", self.results.len()));
                }
                self.status = if incomplete_documents == 0 {
                    base_status
                } else if incomplete_documents == 1 {
                    format!("{base_status} (partial; 1 document still indexing)")
                } else {
                    format!(
                        "{base_status} (partial; {incomplete_documents} documents still indexing)"
                    )
                };
            }
            Err(error) => {
                self.clear_results();
                self.status = search_error_status(error);
            }
        };
    }
}

impl Default for SearchDialogState {
    fn default() -> Self {
        Self::new()
    }
}

struct SearchResults {
    results: Vec<SearchResult>,
    match_count: usize,
    incomplete_documents: usize,
    documents: Vec<SearchDocumentSnapshot>,
    count_summary: Option<SearchCountSummary>,
    matches_limited: bool,
}

fn document_results<'a>(
    documents: impl IntoIterator<Item = &'a Document>,
    query: &str,
    options: SearchOptions,
    settings: SearchResultSettings,
    result_limit: usize,
) -> Result<SearchResults, crate::core::SearchError> {
    let Some(search) = PreparedSearch::new(query, options)? else {
        return Ok(SearchResults {
            results: Vec::new(),
            match_count: 0,
            incomplete_documents: 0,
            documents: Vec::new(),
            count_summary: None,
            matches_limited: false,
        });
    };
    let mut results = Vec::new();
    let mut match_count = 0;
    let documents = documents.into_iter().collect::<Vec<_>>();
    // Include every scoped document even when scanning stops in an earlier one.
    // Revision changes in unvisited documents must still invalidate the result.
    let searched_documents = documents
        .iter()
        .map(|document| SearchDocumentSnapshot::new(document))
        .collect();
    let incomplete_documents = documents
        .iter()
        .filter(|document| !document.has_complete_text_index())
        .count();
    let mut per_document = Vec::new();
    let mut matches_limited = false;

    for document in documents {
        let mut document_match_count = 0;
        let scanned = search.try_for_each_match_in_chunks(document.buffer.chunks(), |text_match| {
            if result_limit > 0 && match_count >= result_limit {
                matches_limited = true;
                return std::ops::ControlFlow::Break(());
            }
            match_count += 1;
            document_match_count += 1;
            if results.len() < result_limit
                && let Some(result) = result_for_match(document, text_match, settings)
            {
                results.push(result);
            }
            std::ops::ControlFlow::Continue(())
        });
        if result_limit == 0 {
            per_document.push(SearchDocumentCount {
                document_id: document.id,
                title: document.title(),
                match_count: document_match_count,
            });
        }
        if scanned.is_break() {
            break;
        }
    }

    Ok(SearchResults {
        results,
        match_count,
        incomplete_documents,
        documents: searched_documents,
        count_summary: (result_limit == 0).then(|| SearchCountSummary {
            total_matches: match_count,
            matched_documents: per_document
                .iter()
                .filter(|document| document.match_count > 0)
                .count(),
            per_document,
        }),
        matches_limited,
    })
}

pub fn search_error_status(error: crate::core::SearchError) -> String {
    match error {
        crate::core::SearchError::InvalidRegex(error) => format!("Invalid regex: {error}"),
    }
}

pub fn include_filter_matches(document: &Document, include_pattern: &str) -> bool {
    let pattern = include_pattern.trim();

    if pattern.is_empty() || pattern == "*" || pattern == "*.*" {
        return true;
    }

    pattern
        .split([';', ',', ' '])
        .filter(|part| !part.trim().is_empty())
        .any(|part| matches_one_pattern(document, part.trim()))
}

fn matches_one_pattern(document: &Document, pattern: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    if wildcard_match(&document.title().to_ascii_lowercase(), &pattern) {
        return true;
    }

    document.path.as_ref().is_some_and(|path| {
        wildcard_match(&path.display().to_string().to_ascii_lowercase(), &pattern)
    })
}

fn wildcard_match(value: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }

    let value = value.chars().collect::<Vec<_>>();
    let pattern = pattern.chars().collect::<Vec<_>>();
    let (mut value_index, mut pattern_index) = (0usize, 0usize);
    let mut star_pattern = None;
    let mut star_value = 0usize;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
        {
            value_index += 1;
            pattern_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_pattern = Some(pattern_index);
            pattern_index += 1;
            star_value = value_index;
        } else if let Some(star) = star_pattern {
            pattern_index = star + 1;
            star_value += 1;
            value_index = star_value;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

fn result_for_match(
    document: &Document,
    text_match: TextMatch,
    settings: SearchResultSettings,
) -> Option<SearchResult> {
    let start = document.buffer.position_for_byte_offset(text_match.start)?;
    let end = document.buffer.position_for_byte_offset(text_match.end)?;
    let selection = EditorSelection::new(start, end);
    let (preview, preview_match) = document.buffer.line_excerpt_with_match(
        selection.range(),
        settings.preview_chars,
        settings.context_before,
    )?;

    Some(SearchResult {
        document_id: document.id,
        document_title: document.title(),
        selection,
        preview,
        preview_match,
    })
}

#[cfg(test)]
mod tests {
    use super::{SearchDialogState, include_filter_matches};
    use crate::core::SearchResultSettings;
    const MAX_SEARCH_RESULTS: usize = SearchResultSettings::DEFAULT_RESULT_LIMIT;
    const MAX_PREVIEW_CHARS: usize = SearchResultSettings::DEFAULT_PREVIEW_CHARS;
    use crate::core::{Document, DocumentId, DocumentIndexState, SearchMode};
    use crate::editor::{EditorPosition, EditorSelection};

    #[test]
    fn include_filter_accepts_empty_and_star_patterns() {
        let document = Document::from_path(DocumentId::new(1), "main.rs", "");

        assert!(include_filter_matches(&document, ""));
        assert!(include_filter_matches(&document, "*"));
        assert!(include_filter_matches(&document, "*.*"));
    }

    #[test]
    fn include_filter_matches_document_title_and_path_with_wildcards() {
        let document = Document::from_path(DocumentId::new(2), "src/app/search.rs", "");

        assert!(include_filter_matches(&document, "*.rs"));
        assert!(include_filter_matches(&document, "src*search.rs"));
        assert!(include_filter_matches(&document, "*.txt;*.rs"));
        assert!(!include_filter_matches(&document, "*.md;*.txt"));
    }

    #[test]
    fn search_results_preserve_full_match_selection() {
        let document = Document::from_path(DocumentId::new(3), "notes.txt", "alpha beta gamma");
        let mut dialog = SearchDialogState::new();

        dialog.set_query("beta");
        dialog.refresh_from_documents([&document]);

        assert_eq!(dialog.results.len(), 1);
        assert_eq!(
            dialog.results[0].selection,
            EditorSelection::new(EditorPosition::new(0, 6), EditorPosition::new(0, 10))
        );
    }

    #[test]
    fn extended_search_mode_matches_escape_sequences() {
        let document = Document::from_path(DocumentId::new(4), "notes.txt", "alpha\nbeta");
        let mut dialog = SearchDialogState::new();

        dialog.set_query(r"alpha\nbeta");
        dialog.set_mode(SearchMode::Extended);
        dialog.refresh_from_documents([&document]);

        assert_eq!(dialog.results.len(), 1);
    }

    #[test]
    fn regex_search_mode_reports_invalid_patterns() {
        let document = Document::from_path(DocumentId::new(5), "notes.txt", "alpha");
        let mut dialog = SearchDialogState::new();

        dialog.set_query("[");
        dialog.set_mode(SearchMode::Regex);
        dialog.refresh_from_documents([&document]);

        assert!(dialog.results.is_empty());
        assert!(dialog.status.starts_with("Invalid regex:"));
    }

    #[test]
    fn search_status_reports_partial_results_for_indexing_documents() {
        let mut document = Document::from_path(DocumentId::new(6), "notes.txt", "alpha beta");
        document.index_state = DocumentIndexState::Pending {
            generation: crate::core::DocumentLoadGeneration::next(),
        };
        let mut dialog = SearchDialogState::new();

        dialog.set_query("alpha");
        dialog.refresh_from_documents([&document]);

        assert_eq!(dialog.results.len(), 1);
        assert!(dialog.status.contains("partial"));
        assert!(dialog.status.contains("still indexing"));
    }

    #[test]
    fn frequent_matches_keep_preview_storage_and_displayed_results_bounded() {
        let text = "a".repeat(8 * 1024);
        let document = Document::from_path(DocumentId::new(7), "minified.txt", &text);
        for mode in [SearchMode::Normal, SearchMode::Regex] {
            let mut dialog = SearchDialogState::new();
            dialog.set_query("a");
            dialog.set_mode(mode);
            dialog.refresh_from_documents([&document]);

            assert_eq!(dialog.match_count, MAX_SEARCH_RESULTS);
            assert!(dialog.matches_limited);
            assert_eq!(dialog.results.len(), MAX_SEARCH_RESULTS);
            assert_eq!(dialog.status, "500+ matches");
            let preview_bytes: usize = dialog
                .results
                .iter()
                .map(|result| result.preview.len())
                .sum();
            assert!(preview_bytes <= MAX_SEARCH_RESULTS * (MAX_PREVIEW_CHARS * 4 + 6));
            assert!(
                dialog
                    .results
                    .iter()
                    .all(|result| result.preview.chars().count() <= MAX_PREVIEW_CHARS + 2)
            );
            assert_eq!(dialog.results[0].selection.range().start.column, 0);
            assert_eq!(
                dialog.results[MAX_SEARCH_RESULTS - 1]
                    .selection
                    .range()
                    .start
                    .column,
                MAX_SEARCH_RESULTS - 1
            );
        }
    }

    #[test]
    fn long_unicode_preview_keeps_match_context_and_full_selection() {
        let text = format!("{}needle{}", "🙂".repeat(400), "界".repeat(400));
        let document = Document::from_path(DocumentId::new(8), "unicode.txt", &text);
        let mut dialog = SearchDialogState::new();
        dialog.set_query("needle");
        dialog.refresh_from_documents([&document]);

        let result = &dialog.results[0];
        assert!(result.preview.starts_with('…'));
        assert!(result.preview.ends_with('…'));
        assert!(result.preview.contains("needle"));
        assert!(result.preview.chars().count() <= MAX_PREVIEW_CHARS + 2);
        assert_eq!(result.selection.range().start.column, 400 * 4);
        assert_eq!(result.selection.range().end.column, 400 * 4 + 6);
    }

    #[test]
    fn count_reports_all_matches_across_documents_without_result_rows() {
        let first = Document::from_path(DocumentId::new(9), "first.txt", &"a".repeat(8192));
        let second = Document::from_path(DocumentId::new(10), "second.txt", &"a".repeat(4096));
        for mode in [SearchMode::Normal, SearchMode::Regex] {
            let mut dialog = SearchDialogState::new();
            dialog.set_query("a");
            dialog.set_mode(mode);
            dialog.refresh_from_documents([&first]);
            assert!(!dialog.results.is_empty());
            dialog.count_from_documents([&first, &second]);

            assert!(dialog.results.is_empty());
            assert_eq!(dialog.match_count, 12288);
            assert_eq!(dialog.status, "12288 matches");
            assert!(!dialog.matches_limited);
            let summary = dialog.count_summary.as_ref().unwrap();
            assert_eq!(summary.total_matches, 12288);
            assert_eq!(summary.matched_documents, 2);
            assert_eq!(summary.per_document[0].match_count, 8192);
            assert_eq!(summary.per_document[1].match_count, 4096);
            dialog.set_query("missing");
            assert_eq!(dialog.match_count, 0);
            assert!(dialog.count_summary.is_none());
            dialog.count_from_documents([&first]);
            assert_eq!(dialog.status, "No matches");
            let summary = dialog.count_summary.as_ref().unwrap();
            assert_eq!(summary.total_matches, 0);
            assert_eq!(summary.matched_documents, 0);
            assert_eq!(summary.per_document[0].match_count, 0);
        }
    }

    #[test]
    fn configured_limits_bound_ordinary_search_and_leave_count_unlimited() {
        let document = Document::from_path(DocumentId::new(11), "dense.txt", &"a".repeat(8192));
        let mut dialog = SearchDialogState::new();
        dialog.set_query("a");
        dialog.set_result_settings(SearchResultSettings {
            result_limit: 3,
            preview_chars: 12,
            context_before: 2,
        });
        dialog.refresh_from_documents([&document]);
        assert_eq!(dialog.match_count, 3);
        assert!(dialog.matches_limited);
        assert_eq!(dialog.results.len(), 3);
        assert!(
            dialog
                .results
                .iter()
                .all(|result| result.preview.chars().count() <= 14)
        );
        assert_eq!(dialog.status, "3+ matches");
        dialog.count_from_documents([&document]);
        assert_eq!(dialog.match_count, 8192);
        assert!(dialog.results.is_empty());
    }

    #[test]
    fn bounded_search_probes_one_extra_match_and_snapshots_the_entire_scope() {
        let first = Document::from_path(DocumentId::new(16), "first.txt", &"a".repeat(500));
        let empty = Document::from_path(DocumentId::new(17), "empty.txt", "none");
        let overflow = Document::from_path(DocumentId::new(18), "overflow.txt", "a");
        let mut dialog = SearchDialogState::new();
        dialog.set_query("a");
        dialog.refresh_from_documents([&first, &empty]);
        assert_eq!(dialog.match_count, 500);
        assert!(!dialog.matches_limited);
        assert_eq!(dialog.status, "500 matches");

        dialog.refresh_from_documents([&first, &overflow, &empty]);
        assert_eq!(dialog.match_count, 500);
        assert!(dialog.matches_limited);
        assert_eq!(dialog.results.len(), 500);
        assert_eq!(dialog.result_documents.len(), 3);
        assert!(dialog.count_summary.is_none());

        dialog.count_from_documents([&first, &overflow, &empty]);
        let summary = dialog.count_summary.as_ref().unwrap();
        assert_eq!(summary.total_matches, 501);
        assert_eq!(summary.matched_documents, 2);
        assert_eq!(summary.per_document.len(), 3);
        assert_eq!(summary.per_document[2].document_id, empty.id);
        assert_eq!(summary.per_document[2].match_count, 0);
        dialog.refresh_from_documents([&first]);
        assert!(dialog.count_summary.is_none());
    }

    #[test]
    fn zero_before_context_does_not_include_earlier_text_near_line_end() {
        let text = format!("{}needle", "x".repeat(300));
        let document = Document::from_path(DocumentId::new(12), "end.txt", &text);
        let mut dialog = SearchDialogState::new();
        dialog.set_query("needle");
        dialog.set_result_settings(SearchResultSettings {
            context_before: 0,
            ..SearchResultSettings::default()
        });
        dialog.refresh_from_documents([&document]);
        assert_eq!(dialog.results[0].preview, "…needle");
    }

    #[test]
    fn preview_highlight_identifies_the_exact_match_in_unicode_context() {
        let text = format!("{}café café {}", "🙂".repeat(80), "界".repeat(80));
        let document = Document::from_path(DocumentId::new(13), "unicode.txt", &text);
        let mut dialog = SearchDialogState::new();
        dialog.set_query("café");
        dialog.set_result_settings(SearchResultSettings {
            preview_chars: 16,
            context_before: 8,
            ..SearchResultSettings::default()
        });
        dialog.refresh_from_documents([&document]);
        assert_eq!(dialog.results.len(), 2);
        for result in &dialog.results {
            assert_eq!(&result.preview[result.preview_match.clone()], "café");
            assert!(result.preview.starts_with('…'));
            assert!(result.preview.ends_with('…'));
        }
        assert_ne!(
            dialog.results[0].preview_match,
            dialog.results[1].preview_match
        );
    }

    #[test]
    fn preview_highlights_clip_multiline_matches_and_preserve_indentation() {
        let document = Document::from_path(DocumentId::new(14), "notes.txt", "    one\ntwo");
        let mut dialog = SearchDialogState::new();
        dialog.set_query("one\ntwo");
        dialog.refresh_from_documents([&document]);
        let result = &dialog.results[0];
        assert_eq!(result.preview, "    one");
        assert_eq!(&result.preview[result.preview_match.clone()], "one");
        assert_eq!(result.selection.range().end, EditorPosition::new(1, 3));
    }

    #[test]
    fn regex_validation_updates_before_running_a_search() {
        let mut dialog = SearchDialogState::new();
        dialog.set_mode(SearchMode::Regex);
        dialog.set_query("[");
        assert!(dialog.status.starts_with("Invalid regex:"));
        dialog.set_query("a+");
        assert_eq!(dialog.status, "Ready");
    }

    #[test]
    fn single_character_file_masks_accept_unicode_characters() {
        let document = Document::from_path(DocumentId::new(15), "é.txt", "");
        assert!(include_filter_matches(&document, "?.txt"));
        assert!(!include_filter_matches(&document, "??.txt"));
    }
}
