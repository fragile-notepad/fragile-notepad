//! Whole-document folding and indentation scans, including worker results.

use super::{Document, DocumentId, MAX_FULL_DOCUMENT_ANALYSIS_BYTES};
use crate::editor::{
    DecorationModel, DecorationSettings, EditorBuffer, FoldModel, IndentBraceFoldProvider,
    IndentGuide,
};

/// Folding and indent guides computed from one document revision and its settings.
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
    /// Captures pending analysis once the text index is complete.
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

    /// Rejects results whose document, text revision, syntax, or indentation changed.
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

/// Computes folds and guides below the full-analysis size limit.
pub fn analyze_document(buffer: EditorBuffer, mut result: DocumentAnalysis) -> DocumentAnalysis {
    if buffer.len_bytes() <= MAX_FULL_DOCUMENT_ANALYSIS_BYTES {
        result.folds =
            IndentBraceFoldProvider::for_syntax(result.indent_width, &result.syntax_token)
                .compute_fold_model(&buffer);
        result.guides = indent_guides(&buffer, result.indent_width);
    }
    result
}

pub(super) fn indent_guides(buffer: &EditorBuffer, indent_width: usize) -> Vec<IndentGuide> {
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
pub(super) fn fold_provider(
    settings: DecorationSettings,
    syntax_token: &str,
) -> IndentBraceFoldProvider {
    IndentBraceFoldProvider::for_syntax(settings.indent_width, syntax_token)
}
