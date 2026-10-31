//! Mutation journal. Only documents borrowed mutably are inspected at a boundary.
//! Stamps contain scalar metadata; document text and fold ranges are never copied.

use super::EditorWindowId;
use crate::core::{Document, DocumentId, DocumentIndexState, DocumentLoadState};
use crate::editor::EditorSelection;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceEvent {
    DocumentOpened(DocumentId),
    DocumentClosed(DocumentId),
    ActiveDocumentChanged(DocumentId),
    OrderChanged,
    WindowOpened(EditorWindowId),
    WindowClosed(EditorWindowId),
    WindowTabsChanged(EditorWindowId),
    ContentChanged(DocumentId),
    PreviewChanged(DocumentId),
    ViewChanged(DocumentId),
    MetadataChanged(DocumentId),
    LoadStateChanged(DocumentId),
    AnalysisInvalidated(DocumentId),
}

#[derive(Debug, Clone, PartialEq)]
struct Stamp {
    revision: u64,
    metadata_revision: u64,
    automatic_syntax: bool,
    encoding: crate::core::TextEncoding,
    line_ending: Option<iced::widget::text_editor::LineEnding>,
    dirty: bool,
    pinned: bool,
    selection: EditorSelection,
    scroll: (usize, f32),
    geometry: (usize, f32, f32),
    folds: u64,
    load: DocumentLoadState,
    index: DocumentIndexState,
    analysis_pending: bool,
}

impl Stamp {
    fn of(document: &Document) -> Self {
        // Progress alone must not restart work for a load-state change.
        let load = match document.load_state {
            DocumentLoadState::Loading { generation, .. } => DocumentLoadState::Loading {
                generation,
                bytes_read: 0,
                total_bytes: None,
            },
            other => other,
        };
        Self {
            revision: document.revision(),
            metadata_revision: document.metadata_revision(),
            automatic_syntax: document.syntax_is_automatic(),
            encoding: document.encoding,
            line_ending: document.line_ending,
            dirty: document.is_dirty,
            pinned: document.is_pinned,
            selection: document.main_selection(),
            scroll: (
                document.scroll.first_visible_row,
                document.scroll.horizontal_px,
            ),
            geometry: (
                document.viewport_visible_rows,
                document.viewport_text_width,
                document.viewport_character_width,
            ),
            folds: document.folds.visibility_revision(),
            load,
            index: document.index_state,
            analysis_pending: document.analysis_pending,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ChangeJournal {
    before: HashMap<DocumentId, Stamp>,
    touched: Vec<DocumentId>,
    events: VecDeque<WorkspaceEvent>,
}

impl ChangeJournal {
    pub(super) fn touch(&mut self, document: &Document) {
        self.before.entry(document.id).or_insert_with(|| {
            self.touched.push(document.id);
            Stamp::of(document)
        });
    }

    pub(super) fn push(&mut self, event: WorkspaceEvent) {
        self.events.push_back(event);
    }

    // Closed documents already emitted their structural event; skip later comparisons.
    pub(super) fn publish(
        &mut self,
        documents: &[Document],
        indices: &HashMap<DocumentId, usize>,
        mut emit: impl FnMut(WorkspaceEvent),
    ) {
        for event in self.events.drain(..) {
            emit(event);
        }
        for id in self.touched.drain(..) {
            let before = self
                .before
                .remove(&id)
                .expect("touched document has a stamp");
            let Some(document) = indices.get(&id).and_then(|index| documents.get(*index)) else {
                continue;
            };
            let after = Stamp::of(document);
            if before.revision != after.revision {
                emit(if document.has_complete_text_index() {
                    WorkspaceEvent::ContentChanged(id)
                } else {
                    WorkspaceEvent::PreviewChanged(id)
                });
            }
            if before.load != after.load || before.index != after.index {
                emit(WorkspaceEvent::LoadStateChanged(id));
            }
            if before.selection != after.selection
                || before.scroll != after.scroll
                || before.geometry != after.geometry
                || before.folds != after.folds
            {
                emit(WorkspaceEvent::ViewChanged(id));
            }
            if before.metadata_revision != after.metadata_revision
                || before.automatic_syntax != after.automatic_syntax
                || before.encoding != after.encoding
                || before.line_ending != after.line_ending
                || before.dirty != after.dirty
                || before.pinned != after.pinned
            {
                emit(WorkspaceEvent::MetadataChanged(id));
            }
            if !before.analysis_pending && after.analysis_pending {
                emit(WorkspaceEvent::AnalysisInvalidated(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Workspace;
    use crate::editor::{EditorBuffer, EditorPosition};

    fn events(workspace: &mut Workspace) -> Vec<WorkspaceEvent> {
        let mut events = Vec::new();
        workspace.publish_changes(|event| events.push(event));
        events
    }

    #[test]
    fn borrowing_without_a_change_emits_nothing_and_batches_repeated_edits() {
        let mut workspace = Workspace::new();
        let id = workspace.active_document_id();
        let _ = workspace.document_mut(id).unwrap();
        assert!(events(&mut workspace).is_empty());
        for text in ["one", "two"] {
            let document = workspace.document_mut(id).unwrap();
            document.buffer = EditorBuffer::from_text(text);
            document.refresh_after_text_change();
        }
        let observed = events(&mut workspace);
        assert_eq!(
            observed
                .iter()
                .filter(|event| **event == WorkspaceEvent::ContentChanged(id))
                .count(),
            1
        );
        assert!(events(&mut workspace).is_empty());
    }

    #[test]
    fn structural_events_preserve_order_and_closed_documents_have_no_late_changes() {
        let mut workspace = Workspace::new();
        let original = workspace.active_document_id();
        let added = workspace.create_untitled();
        workspace.document_mut(added).unwrap().mark_dirty();
        workspace.close(added);
        assert_eq!(
            events(&mut workspace),
            vec![
                WorkspaceEvent::DocumentOpened(added),
                WorkspaceEvent::ActiveDocumentChanged(added),
                WorkspaceEvent::DocumentClosed(added),
                WorkspaceEvent::ActiveDocumentChanged(original),
            ]
        );
    }

    #[test]
    fn bulk_changes_remain_attached_to_their_documents_after_reordering() {
        let mut workspace = Workspace::new();
        let first = workspace.active_document_id();
        let second = workspace.create_untitled();
        events(&mut workspace);
        workspace.edit_documents(Document::mark_dirty);
        workspace.reorder(first, second);
        let observed = events(&mut workspace);
        assert!(observed.contains(&WorkspaceEvent::MetadataChanged(first)));
        assert!(observed.contains(&WorkspaceEvent::MetadataChanged(second)));
    }

    #[test]
    fn selection_and_path_changes_are_observed_without_text_changes() {
        let mut workspace = Workspace::new();
        let id = workspace.insert_loaded_file("before.txt", "text");
        events(&mut workspace);
        let document = workspace.document_mut(id).unwrap();
        let cursor = EditorPosition::new(0, 2);
        document.set_main_selection(EditorSelection::new(cursor, cursor));
        document.set_path("after.txt");
        let observed = events(&mut workspace);
        assert!(observed.contains(&WorkspaceEvent::ViewChanged(id)));
        assert!(observed.contains(&WorkspaceEvent::MetadataChanged(id)));
        assert!(!observed.contains(&WorkspaceEvent::ContentChanged(id)));
    }

    #[test]
    fn progress_is_silent_and_preview_is_distinct_from_committed_content() {
        let mut workspace = Workspace::new();
        let (id, generation) = workspace.insert_loading_file("stream.txt");
        events(&mut workspace);
        workspace
            .document_mut(id)
            .unwrap()
            .update_load_progress(generation, 10, Some(20));
        assert!(events(&mut workspace).is_empty());
        workspace.document_mut(id).unwrap().replace_loading_preview(
            generation,
            "text",
            false,
            15,
            Some(20),
        );
        let observed = events(&mut workspace);
        assert!(observed.contains(&WorkspaceEvent::PreviewChanged(id)));
        assert!(!observed.contains(&WorkspaceEvent::ContentChanged(id)));
        workspace
            .document_mut(id)
            .unwrap()
            .complete_streaming_load(generation, crate::core::TextEncoding::Utf8);
        assert!(events(&mut workspace).contains(&WorkspaceEvent::LoadStateChanged(id)));
    }
}
