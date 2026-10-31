//! Load progress and rejection of results from older load attempts.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::line_endings::detect_line_ending_in_chunks;
use super::{
    Document, DocumentId, MAX_FULL_DOCUMENT_ANALYSIS_BYTES, SyntaxTokenSource, detect_line_ending,
    syntax_token_for_path,
};
use crate::core::encoding::{DecodedText, TextEncoding};
use crate::editor::{
    DecorationModel, EditorBuffer, EditorHistory, EditorPosition, EditorSelection, SelectionSet,
};

static NEXT_LOAD_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Distinguishes a load attempt from earlier attempts for the same document.
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

impl Document {
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

    /// Accepts only the active load attempt; resets selections and syntax measurements.
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
            self.buffer = EditorBuffer::from_text(text.to_owned());
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

        let text = &decoded.text;
        self.buffer = EditorBuffer::from_text(text.to_owned());
        self.selection = EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0));
        self.selection_set = SelectionSet::single(self.selection);
        self.history = EditorHistory::new("");
        self.metadata_dirty = false;
        self.is_dirty = false;
        self.line_ending = detect_line_ending(text);
        self.encoding = decoded.encoding;
        self.set_had_decoding_errors(decoded.had_errors);
        self.load_state = DocumentLoadState::Complete;
        self.index_state = DocumentIndexState::Complete;
        self.refresh_after_text_change();
        true
    }

    /// Finishes loading text already appended through `replace_loading_preview`.
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
        self.line_ending = detect_line_ending_in_chunks(self.buffer.chunks());
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
}
