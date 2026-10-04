use crate::core::{
    Document, DocumentId, PreparedSearch, SearchMode, SearchOptions, TextMatch, Workspace,
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

    pub fn set_active_tab(&mut self, tab: AdvancedSearchTab) {
        self.active_tab = tab;
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        let query = query.into();
        self.query = query;
        self.results.clear();
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
        self.results.clear();
        self.status = search_ready_status(&self.query);
    }

    pub fn set_whole_word(&mut self, whole_word: bool) {
        self.whole_word = whole_word;
        self.results.clear();
        self.status = search_ready_status(&self.query);
    }

    pub fn set_wrap_around(&mut self, wrap_around: bool) {
        self.wrap_around = wrap_around;
    }

    pub fn set_mode(&mut self, mode: SearchMode) {
        self.mode = mode;
        self.results.clear();
    }

    pub fn set_include_pattern(&mut self, include_pattern: impl Into<String>) {
        self.include_pattern = include_pattern.into();
        self.results.clear();
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
        if self.query.is_empty() {
            self.results.clear();
            self.status = String::from("No query");
            return;
        }

        match document_results(documents, &self.query, self.options()) {
            Ok(SearchResults {
                results,
                incomplete_documents,
            }) => {
                self.results = results;
                let base_status = match self.results.len() {
                    0 => String::from("No matches"),
                    1 => String::from("1 match"),
                    count => format!("{count} matches"),
                };
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
                self.results.clear();
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
    incomplete_documents: usize,
}

fn document_results<'a>(
    documents: impl IntoIterator<Item = &'a Document>,
    query: &str,
    options: SearchOptions,
) -> Result<SearchResults, crate::core::SearchError> {
    let Some(search) = PreparedSearch::new(query, options)? else {
        return Ok(SearchResults {
            results: Vec::new(),
            incomplete_documents: 0,
        });
    };
    let mut results = Vec::new();
    let mut incomplete_documents = 0;

    for document in documents {
        if !document.has_complete_text_index() {
            incomplete_documents += 1;
        }
        for text_match in search.matches_in_chunks(document.buffer.chunks()) {
            if let Some(result) = result_for_match(document, text_match) {
                results.push(result);
            }
        }
    }

    Ok(SearchResults {
        results,
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

fn result_for_match(document: &Document, text_match: TextMatch) -> Option<SearchResult> {
    let start = document.buffer.position_for_byte_offset(text_match.start)?;
    let end = document.buffer.position_for_byte_offset(text_match.end)?;
    let preview = document.buffer.line(start.line)?.trim().to_owned();

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
}
