//! Active-document outline scheduling and stale-result rejection.

use std::collections::HashMap;

use super::App;
use crate::editor::EditorSelection;
use crate::message::Message;

use iced::Task;

use crate::core::{Document, DocumentId, DocumentLoadState};
use crate::editor::{
    FunctionEntry, OutlineParseResult, OutlineSnapshotMetadata, OutlineState, OutlineStatus,
    outline_registry_hash, outline_request_for_document, parse_outline_request,
};

#[derive(Debug)]
pub(super) struct OutlineParsing {
    states: HashMap<DocumentId, OutlineState>,
    handles: HashMap<DocumentId, iced::task::Handle>,
    registry_hash: u64,
}

impl OutlineParsing {
    pub(super) fn new() -> Self {
        Self {
            states: HashMap::new(),
            handles: HashMap::new(),
            registry_hash: outline_registry_hash(),
        }
    }

    pub(super) fn state_for(&self, document: &Document) -> Option<&OutlineState> {
        let metadata = OutlineSnapshotMetadata::from_document(document, self.registry_hash);
        self.states
            .get(&document.id)
            .filter(|state| state.matches_metadata(&metadata))
    }

    pub(super) fn functions_for(&self, document: &Document) -> Option<&[FunctionEntry]> {
        let metadata = OutlineSnapshotMetadata::from_document(document, self.registry_hash);
        self.states
            .get(&document.id)
            .and_then(|state| state.current_functions(&metadata))
    }

    pub(super) fn schedule(&mut self, document: &Document) -> Task<OutlineParseResult> {
        let document_id = document.id;
        let inactive = self
            .handles
            .keys()
            .copied()
            .filter(|id| *id != document_id)
            .collect::<Vec<_>>();
        for id in inactive {
            self.remove(id);
        }

        let metadata = OutlineSnapshotMetadata::from_document(document, self.registry_hash);
        if !document.can_run_full_document_analysis() {
            if let Some(handle) = self.handles.remove(&document_id) {
                handle.abort();
            }
            let state = if document.has_complete_text_index()
                || matches!(document.load_state, DocumentLoadState::Failed { .. })
            {
                OutlineState::unavailable_metadata(metadata)
            } else {
                OutlineState::pending_metadata(metadata)
            };
            self.states.insert(document_id, state);
            return Task::none();
        }
        if self
            .states
            .get(&document_id)
            .filter(|state| state.matches_metadata(&metadata))
            .is_some_and(|state| {
                state.status == OutlineStatus::Ready
                    || (state.status == OutlineStatus::Pending
                        && self.handles.contains_key(&document_id))
            })
        {
            return Task::none();
        }

        let request = outline_request_for_document(document, self.registry_hash);
        self.states
            .insert(document_id, OutlineState::pending(&request));
        let (task, handle) =
            Task::perform(parse_outline_request(request), std::convert::identity).abortable();
        if let Some(previous) = self.handles.insert(document_id, handle) {
            previous.abort();
        }
        task
    }

    pub(super) fn complete(&mut self, document: Option<&Document>, result: OutlineParseResult) {
        let metadata = OutlineSnapshotMetadata::from_result(&result);
        let Some(document) = document else {
            self.remove(metadata.document_id);
            return;
        };
        if !document.can_run_full_document_analysis()
            || !metadata.matches_document(document, self.registry_hash)
            || !self
                .states
                .get(&metadata.document_id)
                .is_some_and(|state| state.matches_metadata(&metadata))
        {
            return;
        }

        self.states
            .insert(metadata.document_id, OutlineState::ready(result));
        self.handles.remove(&metadata.document_id);
    }

    pub(super) fn remove(&mut self, document: DocumentId) {
        self.states.remove(&document);
        if let Some(handle) = self.handles.remove(&document) {
            handle.abort();
        }
    }
}

impl App {
    pub(super) fn active_outline_state(&self) -> Option<&OutlineState> {
        self.workspace
            .active_document()
            .and_then(|document| self.outline_parsing.state_for(document))
    }

    pub(super) fn complete_outline_parse(&mut self, result: OutlineParseResult) -> Task<Message> {
        self.outline_parsing
            .complete(self.workspace.document(result.document_id), result);
        Task::none()
    }

    pub(super) fn toggle_function_list(&mut self) -> Task<Message> {
        self.menu.close();

        self.is_function_list_visible = !self.is_function_list_visible;
        self.chrome_animation
            .function_list
            .set_visible(self.is_function_list_visible);

        iced::widget::operation::focus(if self.is_function_list_visible {
            crate::ui::function_list_panel::INPUT_ID
        } else {
            crate::ui::editor::EDITOR_ID
        })
    }

    pub(super) fn select_function_list_entry(
        &mut self,
        position: crate::editor::EditorPosition,
    ) -> Task<Message> {
        self.menu.close();

        let Some(document) = self.workspace.active_document_mut() else {
            return Task::none();
        };

        let position = document.buffer.clamp_position(position);
        document.set_main_selection(EditorSelection::new(position, position));
        document.preferred_vertical_column = None;
        document.reveal_position(position);

        iced::widget::operation::focus(crate::ui::editor::EDITOR_ID)
    }
}

impl OutlineParsing {
    pub(super) fn observe(
        &mut self,
        event: super::events::Event,
        active: DocumentId,
        work: &mut super::events::PendingWork,
    ) {
        use super::events::{Event, Work};
        use crate::core::workspace::changes::WorkspaceEvent as W;
        if let Event::Workspace(W::DocumentClosed(id)) = event {
            self.remove(id);
        }
        let needed = match event {
            Event::Started | Event::SettingsChanged => true,
            Event::Workspace(W::ActiveDocumentChanged(_) | W::DocumentOpened(_)) => true,
            Event::Workspace(W::ContentChanged(id) | W::LoadStateChanged(id)) => id == active,
            _ => false,
        };
        if needed {
            work.request(Work::Outline);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DocumentIndexState;
    use crate::core::document::MAX_FULL_DOCUMENT_ANALYSIS_BYTES;

    #[test]
    fn oversized_documents_are_unavailable_without_starting_a_worker() {
        let document = Document::from_path(
            DocumentId::new(1),
            "large.rs",
            &" ".repeat(MAX_FULL_DOCUMENT_ANALYSIS_BYTES + 1),
        );
        let mut parsing = OutlineParsing::new();
        for _ in 0..2 {
            assert_eq!(parsing.schedule(&document).units(), 0);
            assert_eq!(
                parsing.state_for(&document).unwrap().status,
                OutlineStatus::Unavailable
            );
            assert!(parsing.functions_for(&document).is_none());
            assert!(parsing.handles.is_empty());
        }
    }

    #[test]
    fn finishing_a_load_resumes_parsing_even_without_a_revision_change() {
        let mut document = Document::from_path(DocumentId::new(1), "loading.rs", "fn leaf() {}");
        let generation = crate::core::DocumentLoadGeneration::next();
        document.load_state = DocumentLoadState::Loading {
            generation,
            bytes_read: 0,
            total_bytes: None,
        };
        document.index_state = DocumentIndexState::Pending { generation };
        let mut parsing = OutlineParsing::new();
        assert_eq!(parsing.schedule(&document).units(), 0);
        assert_eq!(
            parsing.state_for(&document).unwrap().status,
            OutlineStatus::Pending
        );
        assert!(parsing.handles.is_empty());

        document.load_state = DocumentLoadState::Complete;
        document.index_state = DocumentIndexState::Complete;
        let task = parsing.schedule(&document);
        assert_eq!(task.units(), 1);
        assert!(parsing.handles.contains_key(&document.id));
        assert_eq!(parsing.schedule(&document).units(), 0);
        let result = crate::editor::parse_outline_snapshot(outline_request_for_document(
            &document,
            parsing.registry_hash,
        ));
        parsing.complete(Some(&document), result);
        assert_eq!(
            parsing.state_for(&document).unwrap().status,
            OutlineStatus::Ready
        );
        assert_eq!(parsing.functions_for(&document).unwrap()[0].name, "leaf");
        assert!(parsing.handles.is_empty());
        assert_eq!(parsing.schedule(&document).units(), 0);
    }

    #[test]
    fn blocked_analysis_aborts_existing_workers_and_rejects_their_results() {
        let mut document = Document::from_path(DocumentId::new(1), "loading.rs", "fn leaf() {}");
        let mut parsing = OutlineParsing::new();
        let task = parsing.schedule(&document);
        assert_eq!(task.units(), 1);
        let handle = parsing.handles[&document.id].clone();
        assert!(!handle.is_aborted());
        let result = crate::editor::parse_outline_snapshot(outline_request_for_document(
            &document,
            parsing.registry_hash,
        ));
        document.load_state = DocumentLoadState::Failed {
            generation: crate::core::DocumentLoadGeneration::next(),
        };
        assert_eq!(parsing.schedule(&document).units(), 0);
        assert!(parsing.handles.is_empty());
        assert!(handle.is_aborted());
        parsing.complete(Some(&document), result);
        assert_eq!(
            parsing.state_for(&document).unwrap().status,
            OutlineStatus::Unavailable
        );
    }
}
