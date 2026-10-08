use iced::highlighter;
use iced::widget::text_editor::LineEnding;

use crate::core::encoding::{
    DecodedText, TextEncoding, encode_text, encode_utf8_chunks_for_save, strip_text_bom,
};
use crate::editor::cjk::{CjkContext, CjkContextCache};
use crate::editor::wrap_measurement::WrapMeasurement;
use crate::editor::{
    DecorationModel, DecorationSettings, EditorBuffer, EditorHistory, EditorPosition,
    EditorSelection, FoldModel, IndentBraceFoldProvider, IndentGuide, ScrollOffset, SelectionSet,
    SyntaxLineCache, ViewportModel,
};
use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const DEFAULT_SYNTAX_TOKEN: &str = "txt";
pub const MAX_FULL_DOCUMENT_ANALYSIS_BYTES: usize = 1024 * 1024;
const DEFAULT_REVEAL_CONTEXT_ROWS: usize = 3;
static NEXT_LOAD_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyntaxTokenSource {
    Auto,
    Manual,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentLoadGeneration(u64);

impl DocumentLoadGeneration {
    pub fn next() -> Self {
        Self(NEXT_LOAD_GENERATION.fetch_add(1, Ordering::Relaxed))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DocumentLoadGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentLoadState {
    Complete,
    Deferred {
        generation: DocumentLoadGeneration,
    },
    Loading {
        generation: DocumentLoadGeneration,
        bytes_read: u64,
        total_bytes: Option<u64>,
    },
    Failed {
        generation: DocumentLoadGeneration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentIndexState {
    Complete,
    Pending { generation: DocumentLoadGeneration },
}

#[derive(Debug, Clone)]
pub struct Document {
    pub id: DocumentId,
    pub path: Option<PathBuf>,
    /// Original disk bytes, retained even when recovery text or edits differ.
    pub disk_revision: Option<super::FileRevision>,
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
        let text = strip_text_bom(text);
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
        let text = strip_text_bom(&decoded.text);
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
        document
    }

    pub fn loading(
        id: DocumentId,
        path: impl Into<PathBuf>,
        generation: DocumentLoadGeneration,
    ) -> Self {
        let path = path.into();
        let mut document = Self::from_parts(
            id,
            Some(path.clone()),
            String::new(),
            syntax_token_for_path(&path),
            None,
            SyntaxTokenSource::Auto,
        );
        document.load_state = DocumentLoadState::Loading {
            generation,
            bytes_read: 0,
            total_bytes: None,
        };
        document.index_state = DocumentIndexState::Pending { generation };
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

    pub fn is_loading(&self) -> bool {
        matches!(self.load_state, DocumentLoadState::Loading { .. })
    }

    pub fn is_indexing(&self) -> bool {
        matches!(self.index_state, DocumentIndexState::Pending { .. })
    }

    pub fn is_loading_or_indexing(&self) -> bool {
        self.is_loading() || self.is_indexing()
    }

    pub fn has_complete_text_index(&self) -> bool {
        matches!(self.load_state, DocumentLoadState::Complete)
            && matches!(self.index_state, DocumentIndexState::Complete)
    }

    pub fn can_run_full_document_analysis(&self) -> bool {
        self.has_complete_text_index()
            && self.buffer.len_bytes() <= MAX_FULL_DOCUMENT_ANALYSIS_BYTES
    }

    pub fn load_generation(&self) -> Option<DocumentLoadGeneration> {
        match self.load_state {
            DocumentLoadState::Deferred { generation }
            | DocumentLoadState::Loading { generation, .. }
            | DocumentLoadState::Failed { generation } => Some(generation),
            DocumentLoadState::Complete => match self.index_state {
                DocumentIndexState::Pending { generation } => Some(generation),
                DocumentIndexState::Complete => None,
            },
        }
    }

    pub fn accepts_load_generation(&self, generation: DocumentLoadGeneration) -> bool {
        self.load_generation() == Some(generation)
    }

    pub fn update_load_progress(
        &mut self,
        generation: DocumentLoadGeneration,
        bytes_read: u64,
        total_bytes: Option<u64>,
    ) -> bool {
        let DocumentLoadState::Loading {
            generation: current,
            bytes_read: current_bytes,
            total_bytes: current_total,
        } = &mut self.load_state
        else {
            return false;
        };

        if *current != generation {
            return false;
        }

        *current_bytes = bytes_read;
        *current_total = total_bytes;
        true
    }

    pub fn has_active_load(&self, generation: DocumentLoadGeneration) -> bool {
        matches!(
            self.load_state,
            DocumentLoadState::Loading {
                generation: current,
                ..
            } if current == generation
        )
    }

    pub fn replace_loading_preview(
        &mut self,
        generation: DocumentLoadGeneration,
        text: &str,
        reset: bool,
        bytes_read: u64,
        total_bytes: Option<u64>,
    ) -> bool {
        if !self.update_load_progress(generation, bytes_read, total_bytes) {
            return false;
        }

        // Invalidate font routing before the new text is measured for wrapping.
        self.revision = self.revision.saturating_add(1);
        if reset {
            self.buffer = EditorBuffer::from_text(strip_text_bom(text).to_owned());
            self.folds.recompute(Vec::new());
            self.rebuild_viewport();
            self.decorations = DecorationModel::from_folds(
                self.decorations.settings,
                self.buffer.line_count(),
                &self.folds,
                Vec::new(),
            );
        } else {
            self.buffer.append_text(text);
            let line_count = self.buffer.line_count();
            if self.word_wrap {
                let context = self.wrap_measurement.map(|_| self.cjk_context());
                self.viewport
                    .sync_unfolded_wrapped_buffer_with_context(&self.buffer, context.as_deref());
            } else {
                self.viewport.sync_unfolded_line_count(line_count);
            }
            self.decorations.sync_loading_line_count(line_count);
        }
        self.selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0));
        self.selection_set = SelectionSet::single(self.selection);
        self.caret_row_affinities.clear();
        self.syntax_cache.borrow_mut().clear();
        true
    }

    pub fn complete_loading(
        &mut self,
        generation: DocumentLoadGeneration,
        decoded: DecodedText,
    ) -> bool {
        if !self.has_active_load(generation) {
            return false;
        }

        let text = strip_text_bom(&decoded.text);
        self.buffer = EditorBuffer::from_text(text.to_owned());
        self.selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0));
        self.selection_set = SelectionSet::single(self.selection);
        self.history = EditorHistory::new("");
        self.metadata_dirty = false;
        self.is_dirty = false;
        self.line_ending = detect_line_ending(text);
        self.encoding = decoded.encoding;
        self.load_state = DocumentLoadState::Complete;
        self.index_state = DocumentIndexState::Complete;
        self.refresh_after_text_change();
        true
    }

    pub fn complete_streaming_load(
        &mut self,
        generation: DocumentLoadGeneration,
        encoding: TextEncoding,
    ) -> bool {
        if !self.has_active_load(generation) {
            return false;
        }

        self.history = EditorHistory::new("");
        self.metadata_dirty = false;
        self.is_dirty = false;
        self.line_ending = self.detect_current_line_ending();
        self.encoding = encoding;
        self.load_state = DocumentLoadState::Complete;
        self.index_state = DocumentIndexState::Complete;
        self.refresh_after_text_change();
        true
    }

    pub fn fail_loading(&mut self, generation: DocumentLoadGeneration) -> bool {
        if !self.has_active_load(generation) {
            return false;
        }

        self.load_state = DocumentLoadState::Failed { generation };
        self.index_state = DocumentIndexState::Complete;
        true
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

    /// Reuses language evidence across redraws, wrapping, and renderer handoffs.
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

    fn clamp_scroll(&mut self) {
        self.scroll.first_visible_row = self
            .scroll
            .first_visible_row
            .min(self.viewport.visible_row_count().saturating_sub(1));
        if self.word_wrap {
            self.scroll.horizontal_px = 0.0;
        }
    }

    pub fn word_wrap(&self) -> bool {
        self.word_wrap
    }

    pub fn set_word_wrap(&mut self, enabled: bool) {
        if self.word_wrap == enabled {
            return;
        }
        let caret_was_visible = self.caret_is_in_view();
        self.word_wrap = enabled;
        self.preferred_vertical_column = None;
        self.rebuild_viewport();
        if caret_was_visible {
            self.ensure_caret_visible();
        }
    }

    pub fn set_wrap_column_limit(&mut self, columns: Option<usize>) {
        let columns = columns.map(|value| {
            value.clamp(
                crate::core::EditorSettings::MIN_WRAP_COLUMN,
                crate::core::EditorSettings::MAX_WRAP_COLUMN,
            )
        });
        if self.wrap_column_limit == columns {
            return;
        }
        let caret_was_visible = self.caret_is_in_view();
        self.wrap_column_limit = columns;
        if self.word_wrap {
            self.preferred_vertical_column = None;
            self.rebuild_viewport();
            if caret_was_visible {
                self.ensure_caret_visible();
            }
        }
    }

    fn caret_is_in_view(&self) -> bool {
        if !self.caret_visible_row().is_some_and(|row| {
            row >= self.scroll.first_visible_row
                && row
                    < self
                        .scroll
                        .first_visible_row
                        .saturating_add(self.viewport_visible_rows)
        }) {
            return false;
        }
        if self.word_wrap {
            return true;
        }
        let cursor = self.buffer.clamp_position(self.main_selection().cursor);
        let display = self.viewport.display_position(cursor).unwrap_or(cursor);
        let line = self.viewport.display_text(display.line, &self.buffer);
        let column = crate::editor::layout::visual_column_for(
            &line,
            display.column,
            self.decorations.settings.indent_width,
        );
        let x = column as f32 * self.viewport_character_width.max(1.0);
        x >= self.scroll.horizontal_px
            && x < self.scroll.horizontal_px + self.viewport_text_width.max(1.0)
    }

    pub fn update_viewport_geometry(
        &mut self,
        visible_rows: usize,
        text_width: f32,
        character_width: f32,
    ) {
        let caret_was_visible = self.caret_is_in_view();
        self.viewport_visible_rows = visible_rows.max(1);
        self.viewport_text_width = text_width.max(1.0);
        self.viewport_character_width = character_width.max(1.0);
        if self.word_wrap
            && (self.viewport.wrap_columns() != Some(self.wrap_columns())
                || self.viewport.fold_indicator_columns() != self.wrap_fold_indicator_columns()
                || self.viewport.wrap_measurement() != self.wrap_measurement)
        {
            self.preferred_vertical_column = None;
            self.rebuild_viewport();
        }
        if self.word_wrap && caret_was_visible {
            self.ensure_caret_visible();
        }
    }

    /// Uses the editor's typography for the shared Unicode wrap map.
    pub fn update_viewport_geometry_with_typography(
        &mut self,
        visible_rows: usize,
        text_width: f32,
        character_width: f32,
        font_size: f32,
        hint_factor: Option<f32>,
    ) {
        self.wrap_measurement = Some(WrapMeasurement::new(
            character_width,
            font_size,
            hint_factor,
        ));
        self.update_viewport_geometry(visible_rows, text_width, character_width);
    }

    fn wrap_columns(&self) -> usize {
        // Keep the insertion caret inside the text area at the end of a full row.
        let character_width = self.viewport_character_width.max(1.0);
        let marker_width = if self.decorations.settings.show_end_of_line_markers {
            crate::editor::render::end_of_line_marker_reservation(character_width)
        } else {
            0.0
        };
        let window_columns = ((self.viewport_text_width - marker_width - 2.0).max(1.0)
            / character_width)
            .floor()
            .max(1.0) as usize;
        self.wrap_column_limit
            .map_or(window_columns, |limit| limit.min(window_columns))
    }

    fn wrap_fold_indicator_columns(&self) -> usize {
        let character_width = self.viewport_character_width.max(1.0);
        (crate::editor::render::collapsed_fold_indicator_reservation(character_width)
            / character_width)
            .ceil() as usize
    }

    /// Returns the exact recovery anchor until the first measured viewport is
    /// available, so an early snapshot cannot replace it with a rounded row.
    pub fn session_top_position(&self) -> Option<EditorPosition> {
        self.pending_session_top
            .or_else(|| self.viewport_top_position())
    }

    /// Restores saved scrolling without relying on a provisional wrap width.
    pub fn restore_session_scroll(
        &mut self,
        position: Option<EditorPosition>,
        fallback_row: usize,
        horizontal_offset: f32,
        wait_for_folds: bool,
    ) {
        self.pending_session_top = None;
        self.session_folds_pending = wait_for_folds;
        if let Some(position) = position {
            let position = self.buffer.clamp_position(position);
            self.restore_viewport_top(Some(position));
            if !self.viewport_geometry_initialized || self.session_folds_pending {
                self.pending_session_top = Some(position);
            }
        } else {
            self.scroll.first_visible_row =
                fallback_row.min(self.viewport.visible_row_count().saturating_sub(1));
        }
        self.scroll.horizontal_px = if self.word_wrap {
            0.0
        } else {
            horizontal_offset.max(0.0)
        };
    }

    /// Called after the widget reports its measured viewport geometry. This
    /// also records geometry received before a streamed document finishes.
    pub fn finish_session_scroll_restore(&mut self) {
        self.viewport_geometry_initialized = true;
        if let Some(position) = self.pending_session_top {
            self.restore_viewport_top(Some(position));
            if !self.session_folds_pending {
                self.pending_session_top = None;
            }
        }
    }

    fn viewport_top_position(&self) -> Option<EditorPosition> {
        self.viewport
            .visible_row_to_document_line(self.scroll.first_visible_row)
            .map(|line| {
                let column = if self.viewport.wrap_columns().is_some() {
                    self.viewport
                        .row_segment(self.scroll.first_visible_row, &self.buffer)
                        .map_or(0, |segment| segment.start_column)
                } else {
                    0
                };
                self.buffer.clamp_position(
                    self.viewport
                        .source_position(EditorPosition::new(line, column)),
                )
            })
    }

    fn refresh_wrapped_lines(&mut self, first: usize, last: usize) {
        let top_position = self.viewport_top_position();
        let context = self.wrap_measurement.map(|_| self.cjk_context());
        if self.viewport.reflow_wrapped_lines_with_context(
            &self.buffer,
            &self.folds,
            first,
            last,
            context.as_deref(),
        ) {
            self.restore_viewport_top(top_position);
        } else {
            self.rebuild_viewport();
        }
    }

    fn rebuild_viewport(&mut self) {
        let top_position = self.viewport_top_position();
        self.viewport = if self.word_wrap {
            let context = self.wrap_measurement.map(|_| self.cjk_context());
            ViewportModel::new_wrapped_with_measurement(
                &self.buffer,
                &self.folds,
                self.wrap_columns(),
                self.decorations.settings.indent_width,
                self.wrap_fold_indicator_columns(),
                self.wrap_measurement,
                context.as_deref(),
            )
        } else {
            ViewportModel::new_with_buffer(
                &self.buffer,
                &self.folds,
                self.decorations.settings.indent_width,
            )
        };
        self.restore_viewport_top(top_position);
    }

    fn restore_viewport_top(&mut self, top_position: Option<EditorPosition>) {
        if let Some(mut position) = top_position {
            while self.viewport.display_position(position).is_none()
                && let Some(range) = self
                    .folds
                    .collapsed_covering_position(position)
                    .or_else(|| self.folds.collapsed_covering(position.line))
            {
                position = EditorPosition::new(range.start_line, 0);
            }
            if let Some(row) = self.viewport.position_to_visible_row(position) {
                self.scroll.first_visible_row = row;
            }
        }
        self.caret_row_affinities.clear();
        self.clamp_scroll();
    }

    /// Carets can sit at either side of a soft line break. Keyboard End,
    /// vertical navigation, and pointer placement retain the chosen display row.
    pub fn caret_visible_row(&self) -> Option<usize> {
        self.position_visible_row(self.main_selection().cursor)
    }

    pub fn position_visible_row(&self, position: EditorPosition) -> Option<usize> {
        let position = self.buffer.clamp_position(position);
        let display = self.viewport.display_position(position)?;
        if let Some(&(_, row)) = self
            .caret_row_affinities
            .iter()
            .find(|(caret, _)| *caret == position)
            && self.viewport.visible_row_to_document_line(row) == Some(display.line)
            && self
                .viewport
                .row_segment(row, &self.buffer)
                .is_some_and(|segment| {
                    display.column >= segment.start_column && display.column <= segment.end_column
                })
        {
            return Some(row);
        }
        self.viewport.position_to_visible_row(position)
    }

    pub fn set_caret_row_affinity(&mut self, position: EditorPosition, row: usize) {
        let position = self.buffer.clamp_position(position);
        if let Some((_, previous_row)) = self
            .caret_row_affinities
            .iter_mut()
            .find(|(caret, _)| *caret == position)
        {
            *previous_row = row;
        } else {
            self.caret_row_affinities.push((position, row));
        }
    }

    pub fn caret_row_affinities(&self) -> &[(EditorPosition, usize)] {
        &self.caret_row_affinities
    }

    pub fn clear_caret_row_affinity(&mut self) {
        self.caret_row_affinities.clear();
    }

    pub fn ensure_caret_visible(&mut self) {
        let cursor = self.buffer.clamp_position(self.main_selection().cursor);
        let mut unfolded = false;
        while self.viewport.display_position(cursor).is_none()
            && let Some(range) = self
                .folds
                .collapsed_covering_position(cursor)
                .or_else(|| self.folds.collapsed_covering(cursor.line))
        {
            self.folds.set_collapsed(range, false);
            unfolded = true;
        }
        if unfolded {
            self.refresh_view_models();
        }
        if let Some(row) = self.caret_visible_row() {
            let capacity = self.viewport_visible_rows.max(1);
            if row < self.scroll.first_visible_row {
                self.scroll.first_visible_row = row;
            } else if row >= self.scroll.first_visible_row.saturating_add(capacity) {
                self.scroll.first_visible_row = row.saturating_sub(capacity - 1);
            }
        }
        if self.word_wrap {
            self.scroll.horizontal_px = 0.0;
            return;
        }
        let display = self.viewport.display_position(cursor).unwrap_or(cursor);
        let line = self.viewport.display_text(display.line, &self.buffer);
        let column = crate::editor::layout::visual_column_for(
            &line,
            display.column,
            self.decorations.settings.indent_width,
        );
        let char_width = self.viewport_character_width.max(1.0);
        let x = column as f32 * char_width;
        let width = self.viewport_text_width.max(char_width);
        if x < self.scroll.horizontal_px {
            self.scroll.horizontal_px = x;
        } else if x + char_width > self.scroll.horizontal_px + width {
            self.scroll.horizontal_px = (x + char_width - width).max(0.0);
        }
    }

    pub fn reveal_line(&mut self, line: usize) {
        self.reveal_line_with_context(line, DEFAULT_REVEAL_CONTEXT_ROWS);
    }

    pub fn reveal_position(&mut self, position: EditorPosition) {
        let position = self.buffer.clamp_position(position);
        let mut unfolded = false;
        while self.viewport.display_position(position).is_none()
            && let Some(range) = self
                .folds
                .collapsed_covering_position(position)
                .or_else(|| self.folds.collapsed_covering(position.line))
        {
            self.folds.set_collapsed(range, false);
            unfolded = true;
        }
        if unfolded {
            self.refresh_view_models();
        }
        let row = if position == self.main_selection().cursor {
            self.caret_visible_row()
        } else {
            self.viewport.position_to_visible_row(position)
        };
        if let Some(row) = row {
            let context =
                DEFAULT_REVEAL_CONTEXT_ROWS.min(self.viewport_visible_rows.saturating_sub(1));
            self.scroll.first_visible_row = row.saturating_sub(context);
        }
    }

    pub fn reveal_line_with_context(&mut self, line: usize, context_rows: usize) {
        let Some(visible_row) = self.viewport.document_line_to_visible_row(line) else {
            return;
        };

        let max = self.viewport.visible_row_count().saturating_sub(1);
        self.scroll.first_visible_row = visible_row.saturating_sub(context_rows).min(max);
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

    fn detect_current_line_ending(&self) -> Option<LineEnding> {
        let mut previous_was_cr = false;

        for chunk in self.buffer.chunks() {
            if chunk.is_empty() {
                continue;
            }

            if previous_was_cr {
                return Some(if chunk.as_bytes().first() == Some(&b'\n') {
                    LineEnding::CrLf
                } else {
                    LineEnding::Cr
                });
            }

            let bytes = chunk.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                match bytes[index] {
                    b'\r' => {
                        if index + 1 == bytes.len() {
                            previous_was_cr = true;
                            break;
                        }
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
        }

        if previous_was_cr {
            return Some(LineEnding::Cr);
        }

        None
    }

    fn fold_provider(&self) -> IndentBraceFoldProvider {
        fold_provider(self.decorations.settings, &self.syntax_token)
    }
}

#[derive(Debug, Clone)]
pub struct DocumentAnalysis {
    pub document_id: DocumentId,
    pub revision: u64,
    pub syntax_token: String,
    pub indent_width: usize,
    pub folds: FoldModel,
    pub guides: Vec<IndentGuide>,
}

impl Document {
    pub fn analysis_request(&self) -> Option<(EditorBuffer, DocumentAnalysis)> {
        (self.analysis_pending && self.has_complete_text_index()).then(|| {
            (
                self.buffer.clone(),
                DocumentAnalysis {
                    document_id: self.id,
                    revision: self.revision,
                    syntax_token: self.syntax_token.clone(),
                    indent_width: self.decorations.settings.indent_width,
                    folds: FoldModel::default(),
                    guides: Vec::new(),
                },
            )
        })
    }

    pub fn apply_analysis(&mut self, result: DocumentAnalysis) -> bool {
        if self.id != result.document_id
            || self.revision != result.revision
            || self.syntax_token != result.syntax_token
            || self.decorations.settings.indent_width != result.indent_width
        {
            return false;
        }
        let delimiter_changed = self
            .folds
            .collapsed_ranges()
            .any(|&range| self.folds.delimiter(range) != result.folds.delimiter(range));
        let visibility_before = self.folds.visibility_revision();
        self.folds.recompute_from_model(result.folds);
        let visibility_changed = self.folds.visibility_revision() != visibility_before;
        if visibility_changed
            || delimiter_changed
            || self.viewport.line_count() != self.buffer.line_count()
        {
            self.rebuild_viewport();
        }
        self.decorations = DecorationModel::from_folds(
            self.decorations.settings,
            self.buffer.line_count(),
            &self.folds,
            result.guides,
        );
        self.analysis_pending = false;
        self.clamp_scroll();
        true
    }

    pub fn restore_collapsed_folds(&mut self, ranges: &[(usize, usize)]) {
        for &(start, end) in ranges {
            self.folds
                .set_collapsed(crate::editor::FoldRange::new(start, end), true);
        }
        self.rebuild_viewport();
        self.decorations = DecorationModel::from_folds(
            self.decorations.settings,
            self.buffer.line_count(),
            &self.folds,
            std::mem::take(&mut self.decorations.indent_guides),
        );
        self.clamp_scroll();
        self.session_folds_pending = false;
        if self.viewport_geometry_initialized {
            self.finish_session_scroll_restore();
        }
    }
}

pub fn analyze_document(buffer: EditorBuffer, mut result: DocumentAnalysis) -> DocumentAnalysis {
    if buffer.len_bytes() <= MAX_FULL_DOCUMENT_ANALYSIS_BYTES {
        result.folds =
            IndentBraceFoldProvider::for_syntax(result.indent_width, &result.syntax_token)
                .compute_fold_model(&buffer);
        result.guides = indent_guides(&buffer, result.indent_width);
    }
    result
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

fn indent_guides(buffer: &EditorBuffer, indent_width: usize) -> Vec<IndentGuide> {
    let mut guides = Vec::new();
    let indent_width = indent_width.max(1);

    for line in 0..buffer.line_count() {
        let Some(text) = buffer.line(line) else {
            continue;
        };

        let columns = text
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .map(|ch| if ch == '\t' { indent_width } else { 1 })
            .sum::<usize>();

        for depth in 1..columns / indent_width {
            guides.push(IndentGuide { line, depth });
        }
    }

    guides
}

fn fold_provider(settings: DecorationSettings, syntax_token: &str) -> IndentBraceFoldProvider {
    IndentBraceFoldProvider::for_syntax(settings.indent_width, syntax_token)
}
