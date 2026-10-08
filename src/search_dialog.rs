use crate::core::{
    Document, DocumentId, PreparedSearch, SearchMode, SearchOptions, SearchResultSettings,
    TextMatch, Workspace,
};
use crate::editor::EditorSelection;
use crate::message::AdvancedSearchTab;

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
    pub match_count: usize,
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
            match_count: 0,
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
            match_count: 0,
            status: String::new(),
        }
    }

    pub fn set_active_tab(&mut self, tab: AdvancedSearchTab) {
        self.active_tab = tab;
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        let query = query.into();
        self.query = query;
        self.clear_results();
        self.status = if self.query.is_empty() {
            String::from("No query")
        } else {
            String::from("Ready")
        };
    }

    pub fn set_replacement(&mut self, replacement: impl Into<String>) {
        self.replacement = replacement.into();
    }

    pub fn set_case_sensitive(&mut self, case_sensitive: bool) {
        self.case_sensitive = case_sensitive;
        self.clear_results();
        self.status = search_ready_status(&self.query);
    }

    pub fn set_whole_word(&mut self, whole_word: bool) {
        self.whole_word = whole_word;
        self.clear_results();
        self.status = search_ready_status(&self.query);
    }

    pub fn set_wrap_around(&mut self, wrap_around: bool) {
        self.wrap_around = wrap_around;
    }

    pub fn set_mode(&mut self, mode: SearchMode) {
        self.mode = mode;
        self.clear_results();
    }

    pub fn set_include_pattern(&mut self, include_pattern: impl Into<String>) {
        self.include_pattern = include_pattern.into();
        self.clear_results();
        self.status = search_ready_status(&self.query);
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
        self.match_count = 0;
    }

    pub fn set_result_settings(&mut self, settings: SearchResultSettings) {
        self.result_settings = settings.normalized();
        self.result_limit_input = self.result_settings.result_limit.to_string();
        self.preview_chars_input = self.result_settings.preview_chars.to_string();
        self.context_before_input = self.result_settings.context_before.to_string();
    }

    pub fn parsed_result_settings(&self) -> Result<SearchResultSettings, &'static str> {
        let result_limit = self
            .result_limit_input
            .parse::<usize>()
            .ok()
            .filter(|value| (1..=SearchResultSettings::MAX_RESULT_LIMIT).contains(value))
            .ok_or("Displayed results must be between 1 and 10,000.")?;
        let preview_chars = self
            .preview_chars_input
            .parse::<usize>()
            .ok()
            .filter(|value| (1..=SearchResultSettings::MAX_PREVIEW_CHARS).contains(value))
            .ok_or("Preview length must be between 1 and 2,000 characters.")?;
        let context_before = self
            .context_before_input
            .parse::<usize>()
            .ok()
            .filter(|value| *value < preview_chars)
            .ok_or("Characters before a match must be between 0 and preview length minus 1.")?;
        Ok(SearchResultSettings {
            result_limit,
            preview_chars,
            context_before,
        })
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
            }) => {
                self.results = results;
                self.match_count = match_count;
                let mut base_status = match match_count {
                    0 => String::from("No matches"),
                    1 => String::from("1 match"),
                    count => format!("{count} matches"),
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

fn search_ready_status(query: &str) -> String {
    if query.is_empty() {
        String::from("No query")
    } else {
        String::from("Ready")
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
        });
    };
    let mut results = Vec::new();
    let mut match_count = 0;
    let mut incomplete_documents = 0;

    for document in documents {
        if !document.has_complete_text_index() {
            incomplete_documents += 1;
        }
        search.for_each_match_in_chunks(document.buffer.chunks(), |text_match| {
            match_count += 1;
            if results.len() < result_limit
                && let Some(result) = result_for_match(document, text_match, settings)
            {
                results.push(result);
            }
        });
    }

    Ok(SearchResults {
        results,
        match_count,
        incomplete_documents,
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

    let value = value.as_bytes();
    let pattern = pattern.as_bytes();
    let (mut value_index, mut pattern_index) = (0usize, 0usize);
    let mut star_pattern = None;
    let mut star_value = 0usize;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            value_index += 1;
            pattern_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
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

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
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
    let preview = document
        .buffer
        .line_excerpt(start, settings.preview_chars, settings.context_before)?
        .trim()
        .to_owned();

    Some(SearchResult {
        document_id: document.id,
        document_title: document.title(),
        selection: EditorSelection::new(start, end),
        preview,
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

            assert_eq!(dialog.match_count, text.len());
            assert_eq!(dialog.results.len(), MAX_SEARCH_RESULTS);
            assert!(dialog.status.contains("8192 matches"));
            assert!(dialog.status.contains("showing first 500"));
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
            dialog.set_query("missing");
            assert_eq!(dialog.match_count, 0);
            dialog.count_from_documents([&first]);
            assert_eq!(dialog.status, "No matches");
        }
    }

    #[test]
    fn configured_limits_bound_previews_without_limiting_the_count() {
        let document = Document::from_path(DocumentId::new(11), "dense.txt", &"a".repeat(8192));
        let mut dialog = SearchDialogState::new();
        dialog.set_query("a");
        dialog.set_result_settings(SearchResultSettings {
            result_limit: 3,
            preview_chars: 12,
            context_before: 2,
        });
        dialog.refresh_from_documents([&document]);
        assert_eq!(dialog.match_count, 8192);
        assert_eq!(dialog.results.len(), 3);
        assert!(
            dialog
                .results
                .iter()
                .all(|result| result.preview.chars().count() <= 14)
        );
        assert!(dialog.status.contains("showing first 3"));
        dialog.count_from_documents([&document]);
        assert_eq!(dialog.match_count, 8192);
        assert!(dialog.results.is_empty());
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
}
