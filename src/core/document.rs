//! A live document's text, edit history, and display state.

mod analysis;
mod loading;
mod view;

pub use analysis::{DocumentAnalysis, analyze_document};
pub use loading::{DocumentIndexState, DocumentLoadGeneration, DocumentLoadState};

use analysis::{fold_provider, indent_guides};

use iced::highlighter;
use iced::widget::text_editor::LineEnding;

use crate::core::encoding::{DecodedText, TextEncoding, encode_text, encode_utf8_chunks_for_save};
use crate::editor::cjk::{CjkContext, CjkContextCache};
use crate::editor::wrap_measurement::WrapMeasurement;
use crate::editor::{
    DecorationModel, DecorationSettings, EditorBuffer, EditorHistory, EditorPosition,
    EditorSelection, FoldModel, IndentBraceFoldProvider, ScrollOffset, SelectionSet,
    SyntaxLineCache, ViewportModel,
};
use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DEFAULT_SYNTAX_TOKEN: &str = "txt";
pub const MAX_FULL_DOCUMENT_ANALYSIS_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyntaxTokenSource {
    Auto,
    Manual,
}

/// Identifies a live document independently of its path or owning window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(u64);

impl DocumentId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Last observed state of the file relative to its last loaded or saved bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    #[default]
    Current,
    Missing,
    Changed,
    Unreadable(std::io::ErrorKind),
}

/// After editing `buffer`, refresh the affected lines to update wrapping, syntax,
/// and dirty state.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: DocumentId,
    pub path: Option<PathBuf>,
    /// Original disk bytes, retained even when recovery text or edits differ.
    pub disk_revision: Option<super::FileRevision>,
    pub disk_state: DiskState,
    pub buffer: EditorBuffer,
    pub selection: EditorSelection,
    selection_set: SelectionSet,
    pub preferred_vertical_column: Option<usize>,
    pub history: EditorHistory,
    pub folds: FoldModel,
    pub viewport: ViewportModel,
    pub decorations: DecorationModel,
    pub scroll: ScrollOffset,
    pub viewport_visible_rows: usize,
    pub viewport_text_width: f32,
    pub viewport_character_width: f32,
    viewport_geometry_initialized: bool,
    pending_session_top: Option<EditorPosition>,
    session_folds_pending: bool,
    word_wrap: bool,
    wrap_column_limit: Option<usize>,
    wrap_measurement: Option<WrapMeasurement>,
    caret_row_affinities: Vec<(EditorPosition, usize)>,
    pub is_dirty: bool,
    pub is_pinned: bool,
    pub syntax_token: String,
    pub syntax_cache: RefCell<SyntaxLineCache>,
    cjk_context: RefCell<CjkContextCache>,
    pub line_ending: Option<LineEnding>,
    pub encoding: TextEncoding,
    /// Decoding replaced source bytes. Protect the source until a lossless reload
    /// or a successful Save As to a different file.
    had_decoding_errors: bool,
    pub load_state: DocumentLoadState,
    pub index_state: DocumentIndexState,
    pub defer_analysis: bool,
    pub analysis_pending: bool,
    revision: u64,
    metadata_dirty: bool,
    metadata_revision: u64,
    syntax_token_source: SyntaxTokenSource,
}

impl Document {
    pub fn untitled(id: DocumentId) -> Self {
        Self::from_parts(
            id,
            None,
            String::new(),
            DEFAULT_SYNTAX_TOKEN.to_owned(),
            Some(LineEnding::Lf),
            SyntaxTokenSource::Auto,
        )
    }

    pub fn from_path(id: DocumentId, path: impl Into<PathBuf>, text: &str) -> Self {
        let path = path.into();
        let line_ending = detect_line_ending(text);

        Self::from_parts(
            id,
            Some(path.clone()),
            text.to_owned(),
            syntax_token_for_path(&path),
            line_ending,
            SyntaxTokenSource::Auto,
        )
    }

    pub fn from_decoded(id: DocumentId, path: impl Into<PathBuf>, decoded: DecodedText) -> Self {
        let path = path.into();
        let text = &decoded.text;
        let line_ending = detect_line_ending(text);
        let mut document = Self::from_parts(
            id,
            Some(path.clone()),
            text.to_owned(),
            syntax_token_for_path(&path),
            line_ending,
            SyntaxTokenSource::Auto,
        );
        document.encoding = decoded.encoding;
        document.had_decoding_errors = decoded.had_errors;
        document
    }

    fn from_parts(
        id: DocumentId,
        path: Option<PathBuf>,
        text: String,
        syntax_token: String,
        line_ending: Option<LineEnding>,
        syntax_token_source: SyntaxTokenSource,
    ) -> Self {
        let buffer = EditorBuffer::from_text(text);
        let selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0));
        let selection_set = SelectionSet::single(selection);
        let history = EditorHistory::new("");
        let decoration_settings = DecorationSettings::default();
        let can_run_full_document_analysis = buffer.len_bytes() <= MAX_FULL_DOCUMENT_ANALYSIS_BYTES;
        let folds = if can_run_full_document_analysis {
            fold_provider(decoration_settings, &syntax_token).compute_fold_model(&buffer)
        } else {
            FoldModel::default()
        };
        let viewport =
            ViewportModel::new_with_buffer(&buffer, &folds, decoration_settings.indent_width);
        let indent_guides = if can_run_full_document_analysis {
            indent_guides(&buffer, decoration_settings.indent_width)
        } else {
            Vec::new()
        };
        let decorations = DecorationModel::from_folds(
            decoration_settings,
            buffer.line_count(),
            &folds,
            indent_guides,
        );

        Self {
            id,
            path,
            disk_revision: None,
            disk_state: DiskState::Current,
            buffer,
            selection,
            selection_set,
            preferred_vertical_column: None,
            history,
            folds,
            viewport,
            decorations,
            scroll: ScrollOffset::ZERO,
            viewport_visible_rows: 20,
            viewport_text_width: 640.0,
            viewport_character_width: 8.0,
            viewport_geometry_initialized: false,
            pending_session_top: None,
            session_folds_pending: false,
            word_wrap: false,
            wrap_column_limit: None,
            wrap_measurement: None,
            caret_row_affinities: Vec::new(),
            is_dirty: false,
            is_pinned: false,
            syntax_token,
            syntax_cache: RefCell::new(SyntaxLineCache::default()),
            cjk_context: RefCell::new(CjkContextCache::default()),
            line_ending,
            encoding: TextEncoding::Utf8,
            had_decoding_errors: false,
            load_state: DocumentLoadState::Complete,
            index_state: DocumentIndexState::Complete,
            defer_analysis: false,
            analysis_pending: false,
            revision: 0,
            metadata_dirty: false,
            metadata_revision: 0,
            syntax_token_source,
        }
    }

    pub fn title(&self) -> String {
        let title = self
            .path
            .as_deref()
            .and_then(title_for_path)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Untitled {}", self.id));

        if self.is_dirty {
            format!("*{title}")
        } else {
            title
        }
    }

    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        if self.syntax_token_source == SyntaxTokenSource::Auto {
            let syntax_token = syntax_token_for_path(&path);
            if self.syntax_token != syntax_token {
                self.syntax_token = syntax_token;
                self.refresh_after_syntax_change();
            }
        }
        if self.path.as_ref() != Some(&path) {
            self.disk_revision = None;
            self.disk_state = DiskState::Current;
            self.metadata_revision = self.metadata_revision.wrapping_add(1);
        }
        self.path = Some(path);
    }

    pub fn mark_dirty(&mut self) {
        self.metadata_dirty = true;
        self.is_dirty = true;
    }

    pub fn invalidate_clean_checkpoint(&mut self) {
        self.history.invalidate_clean_checkpoint();
        self.is_dirty = true;
    }

    pub fn mark_clean(&mut self) {
        self.history.mark_clean("");
        self.metadata_dirty = false;
        self.is_dirty = false;
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub(crate) fn metadata_revision(&self) -> u64 {
        self.metadata_revision
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Reuses detected CJK languages across redraws, wrapping, and renderer handoffs.
    pub fn cjk_context(&self) -> Arc<CjkContext> {
        self.cjk_context
            .borrow_mut()
            .get_or_update(&self.buffer, self.id.get(), self.revision)
    }

    pub fn text_for_save(&self) -> String {
        let mut text = self.buffer.text();
        let Some(line_ending) = self.line_ending else {
            return text;
        };

        let ending = line_ending.as_str();

        if !text.is_empty() && !text.ends_with(ending) {
            text.push_str(ending);
        }

        text
    }

    pub fn save_appended_line_ending(&self) -> Option<&'static str> {
        let line_ending = self.line_ending?;
        let ending = line_ending.as_str();
        let mut last = None;
        for chunk in self.buffer.chunks() {
            last = Some(chunk);
        }
        let last = last?;

        (!last.is_empty() && !last.ends_with(ending)).then_some(ending)
    }

    pub fn bytes_for_save(&self) -> Result<Vec<u8>, crate::core::encoding::EncodingError> {
        if matches!(self.encoding, TextEncoding::Utf8 | TextEncoding::Utf8Bom) {
            return Ok(encode_utf8_chunks_for_save(
                self.buffer.chunks(),
                self.save_appended_line_ending(),
                self.encoding == TextEncoding::Utf8Bom,
            ));
        }

        // Non-UTF encoders operate on scalar values and may report unmappable
        // characters, so they still materialize the complete text.
        encode_text(&self.text_for_save(), self.encoding)
    }

    pub fn set_encoding(&mut self, encoding: TextEncoding) {
        if self.encoding != encoding {
            self.encoding = encoding;
            self.mark_dirty();
        }
    }

    pub fn had_decoding_errors(&self) -> bool {
        self.had_decoding_errors
    }

    pub fn set_had_decoding_errors(&mut self, had_errors: bool) {
        if self.had_decoding_errors != had_errors {
            self.had_decoding_errors = had_errors;
            self.metadata_revision = self.metadata_revision.wrapping_add(1);
        }
    }

    pub fn refresh_syntax_from_path(&mut self) {
        self.syntax_token_source = SyntaxTokenSource::Auto;
        let syntax_token = self
            .path
            .as_deref()
            .map(syntax_token_for_path)
            .unwrap_or_else(|| DEFAULT_SYNTAX_TOKEN.to_owned());
        if self.syntax_token != syntax_token {
            self.syntax_token = syntax_token;
            self.refresh_after_syntax_change();
        }
    }

    pub fn syntax_is_automatic(&self) -> bool {
        self.syntax_token_source == SyntaxTokenSource::Auto
    }

    pub fn restore_syntax(&mut self, token: Option<String>, automatic: Option<bool>) {
        // Old sessions cannot distinguish a manual choice matching the extension
        // from automatic detection. Prefer detection when they agree.
        let detected = self
            .path
            .as_deref()
            .map(syntax_token_for_path)
            .unwrap_or_else(|| DEFAULT_SYNTAX_TOKEN.to_owned());
        if automatic.unwrap_or_else(|| token.as_ref().is_none_or(|token| token == &detected)) {
            self.refresh_syntax_from_path();
        } else if let Some(token) = token {
            self.set_syntax_token(token);
        }
    }

    pub fn set_syntax_token(&mut self, syntax_token: impl Into<String>) {
        let syntax_token = syntax_token.into();

        let syntax_token = if syntax_token.is_empty() {
            DEFAULT_SYNTAX_TOKEN.to_owned()
        } else {
            syntax_token
        };
        let syntax_token_source = if syntax_token == DEFAULT_SYNTAX_TOKEN {
            SyntaxTokenSource::Auto
        } else {
            SyntaxTokenSource::Manual
        };

        if self.syntax_token != syntax_token || self.syntax_token_source != syntax_token_source {
            self.syntax_token = syntax_token;
            self.syntax_token_source = syntax_token_source;
            self.refresh_after_syntax_change();
        }
    }

    pub fn uses_syntax_highlighting(&self) -> bool {
        self.syntax_token != DEFAULT_SYNTAX_TOKEN
    }

    pub fn render_syntax_token(&self) -> &str {
        if self.can_run_full_document_analysis() {
            &self.syntax_token
        } else {
            DEFAULT_SYNTAX_TOKEN
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let Some(selection_set) = self.history.undo_selection_set(&mut self.buffer) else {
            return false;
        };

        self.set_selection_set(selection_set);
        self.clamp_selection_set();
        self.refresh_after_text_change();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(selection_set) = self.history.redo_selection_set(&mut self.buffer) else {
            return false;
        };

        self.set_selection_set(selection_set);
        self.clamp_selection_set();
        self.refresh_after_text_change();
        true
    }

    pub fn refresh_after_text_change(&mut self) {
        self.refresh_text_from(0);
    }

    pub fn refresh_text_from(&mut self, first_changed_line: usize) {
        self.refresh_text_lines(
            first_changed_line,
            self.buffer.line_count().saturating_sub(1),
        );
    }

    /// Refreshes an inclusive span of changed logical lines. Supplying the
    /// complete edit span lets unchanged lines retain their wrap measurements.
    pub fn refresh_text_lines(&mut self, first_changed_line: usize, last_changed_line: usize) {
        self.revision = self.revision.saturating_add(1);
        self.caret_row_affinities.clear();
        if self.defer_analysis {
            self.analysis_pending = self.has_complete_text_index();
            if self.folds.ranges().is_empty() {
                if self.word_wrap {
                    self.refresh_wrapped_lines(first_changed_line, last_changed_line);
                } else {
                    self.viewport
                        .sync_unfolded_line_count(self.buffer.line_count());
                }
                self.decorations
                    .sync_loading_line_count(self.buffer.line_count());
            } else if self.word_wrap
                || self.viewport.has_projections()
                || self.viewport.line_count() != self.buffer.line_count()
            {
                self.refresh_wrapped_lines(first_changed_line, last_changed_line);
            }
        } else if self.can_run_full_document_analysis() {
            self.folds
                .recompute_from_model(self.fold_provider().compute_fold_model(&self.buffer));
            self.refresh_view_models();
        } else if !self.folds.ranges().is_empty() || !self.decorations.indent_guides.is_empty() {
            self.folds.recompute(Vec::new());
            self.refresh_view_models();
        } else {
            // Unwrapped large files retain their inexpensive identity mapping.
            // Wrapped rows also depend on the content of the edited line.
            if self.word_wrap {
                self.refresh_wrapped_lines(first_changed_line, last_changed_line);
            } else {
                self.viewport
                    .sync_unfolded_line_count(self.buffer.line_count());
            }
            self.decorations
                .sync_loading_line_count(self.buffer.line_count());
        }
        self.clamp_scroll();
        self.refresh_dirty_state();
        self.syntax_cache.borrow_mut().invalidate_edit(
            first_changed_line,
            last_changed_line,
            self.buffer.line_count(),
        );
    }

    pub fn refresh_view_models(&mut self) {
        self.rebuild_viewport();
        let indent_guides = if self.can_run_full_document_analysis() {
            indent_guides(&self.buffer, self.decorations.settings.indent_width)
        } else {
            Vec::new()
        };
        self.decorations = DecorationModel::from_folds(
            self.decorations.settings,
            self.buffer.line_count(),
            &self.folds,
            indent_guides,
        );
        self.clamp_scroll();
    }

    pub fn set_decoration_settings(&mut self, settings: DecorationSettings) {
        let previous = self.decorations.settings;
        if previous == settings {
            return;
        }
        self.decorations.settings = settings;
        if previous.indent_width == settings.indent_width {
            for line in &mut self.decorations.line_decorations {
                line.line_number = settings.show_line_numbers.then_some(line.line + 1);
                line.has_fold_control = settings.show_folding_controls && line.fold_range.is_some();
            }
            if previous.show_end_of_line_markers != settings.show_end_of_line_markers {
                self.update_viewport_geometry(
                    self.viewport_visible_rows,
                    self.viewport_text_width,
                    self.viewport_character_width,
                );
            }
            return;
        }
        if self.defer_analysis {
            self.analysis_pending = self.has_complete_text_index();
            self.rebuild_viewport();
            return;
        }
        if self.can_run_full_document_analysis() {
            self.folds.recompute_from_model(
                fold_provider(settings, &self.syntax_token).compute_fold_model(&self.buffer),
            );
        } else {
            self.folds.recompute(Vec::new());
        }
        self.refresh_view_models();
    }

    pub fn ensure_syntax_cache(&self, theme: highlighter::Theme) {
        let settings = highlighter::Settings {
            token: self.syntax_token.clone(),
            theme,
        };

        if !self
            .syntax_cache
            .borrow()
            .is_current(&settings, self.buffer.line_count())
        {
            *self.syntax_cache.borrow_mut() = SyntaxLineCache::rebuild(&self.buffer, &settings);
        } else {
            self.syntax_cache.borrow_mut().configure(&settings);
        }
    }

    pub fn ensure_visible_syntax_cache(
        &self,
        theme: highlighter::Theme,
        first_line: usize,
        last_line: usize,
    ) {
        let settings = highlighter::Settings {
            token: self.syntax_token.clone(),
            theme,
        };

        self.syntax_cache.borrow_mut().ensure_visible(
            &self.buffer,
            &settings,
            first_line,
            last_line,
        );
    }

    pub fn clamp_selection(&self, selection: EditorSelection) -> EditorSelection {
        EditorSelection::new(
            self.buffer.clamp_position(selection.anchor),
            self.buffer.clamp_position(selection.cursor),
        )
    }

    pub fn main_selection(&self) -> EditorSelection {
        self.selection_set.main()
    }

    pub fn set_main_selection(&mut self, selection: EditorSelection) {
        self.selection = self.clamp_selection(selection);
        self.selection_set = SelectionSet::single(self.selection);
        self.caret_row_affinities
            .retain(|(position, _)| *position == self.selection.cursor);
    }

    pub fn selection_set(&self) -> &SelectionSet {
        &self.selection_set
    }

    pub fn sync_selection_mirror(&mut self) {
        let selection = self.clamp_selection(self.selection);
        if selection != self.selection_set.main() {
            self.selection = selection;
            self.selection_set = SelectionSet::single(selection);
            self.caret_row_affinities.clear();
        }
    }

    pub fn set_selection_set(&mut self, selection_set: SelectionSet) {
        self.selection_set = selection_set.clamped(&self.buffer);
        self.selection = self.selection_set.main();
        self.caret_row_affinities.retain(|(position, _)| {
            self.selection_set
                .ranges()
                .iter()
                .any(|selection| selection.cursor == *position)
        });
    }

    pub fn clamp_selection_set(&mut self) {
        self.selection_set = self.selection_set.clamped(&self.buffer);
        self.selection = self.selection_set.main();
    }

    fn refresh_dirty_state(&mut self) {
        self.is_dirty = self.metadata_dirty || self.history.is_dirty("");
    }

    fn refresh_after_syntax_change(&mut self) {
        self.syntax_cache.borrow_mut().clear();
        if self.defer_analysis {
            self.analysis_pending = self.has_complete_text_index();
            self.revision = self.revision.saturating_add(1);
            return;
        }
        if self.can_run_full_document_analysis() {
            self.folds
                .recompute_from_model(self.fold_provider().compute_fold_model(&self.buffer));
        } else {
            self.folds.recompute(Vec::new());
        }
        self.refresh_view_models();
        self.revision = self.revision.saturating_add(1);
    }

    fn fold_provider(&self) -> IndentBraceFoldProvider {
        fold_provider(self.decorations.settings, &self.syntax_token)
    }
}

pub fn title_for_path(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

pub fn syntax_token_for_path(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| DEFAULT_SYNTAX_TOKEN.to_owned())
}

pub fn detect_line_ending(text: &str) -> Option<LineEnding> {
    let bytes = text.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                return Some(if bytes.get(index + 1) == Some(&b'\n') {
                    LineEnding::CrLf
                } else {
                    LineEnding::Cr
                });
            }
            b'\n' => {
                return Some(if bytes.get(index + 1) == Some(&b'\r') {
                    LineEnding::LfCr
                } else {
                    LineEnding::Lf
                });
            }
            _ => index += 1,
        }
    }

    None
}
