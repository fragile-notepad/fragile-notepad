//! Document close queues and application exit decisions.

use crate::app::{App, CloseGoal};
use crate::core::{DirtyCloseDecision, Document, DocumentId, DocumentLoadState};
use crate::message::Message;
use iced::Task;
use std::collections::VecDeque;

impl App {
    /// File queues are process-wide, even while the owning prompt is staged in
    /// an inactive editor. Admit one captured close operation at a time.
    pub(in crate::app) fn close_operation_pending(&self) -> bool {
        self.close_prompt.document().is_some()
            || self
                .editor_windows
                .values()
                .filter_map(|window| window.ui.as_ref())
                .any(|ui| ui.close_prompt.document().is_some())
            || self.files.pending_close_after_save.is_some()
            || !self.files.pending_close_documents.is_empty()
    }

    pub(super) fn close_request(&mut self, document_id: DocumentId) -> Task<Message> {
        let resuming_load = self.files.pending_close_documents.front() == Some(&document_id)
            && self.files.pending_close_after_save.is_none()
            && self.close_prompt.document().is_none()
            && !self
                .editor_windows
                .values()
                .filter_map(|window| window.ui.as_ref())
                .any(|ui| ui.close_prompt.document().is_some());
        if self.close_operation_pending() && !resuming_load {
            return Task::none();
        }
        if resuming_load {
            if self
                .workspace
                .document(document_id)
                .is_some_and(Document::is_loading_or_indexing)
            {
                return Task::none();
            }
            self.files.pending_close_documents.pop_front();
        }
        self.close_request_in_operation(document_id)
    }

    fn close_request_in_operation(&mut self, document_id: DocumentId) -> Task<Message> {
        if let Some(owner) = self.workspace.owner_of(document_id) {
            self.switch_editor(owner);
        }
        if self.close_prompt.is_closing() {
            return Task::none();
        }
        if self.workspace.document(document_id).is_some_and(|doc| {
            matches!(doc.load_state, DocumentLoadState::Deferred { .. }) && doc.is_dirty
        }) {
            let load = self.activate_document(document_id);
            if self
                .workspace
                .document(document_id)
                .is_some_and(Document::has_complete_text_index)
            {
                let close = self.close_request_in_operation(document_id);
                return Task::batch([load, close]);
            }
            self.files.pending_close_documents.push_front(document_id);
            return load.chain(Task::done(Message::ResumeDocumentClose(document_id)));
        }
        let Some(document) = self.workspace.document(document_id) else {
            return Task::none();
        };

        if !document.is_dirty {
            return self.close_now(document_id);
        }

        self.go_to_line_prompt = None;
        self.close_prompt.show(document_id);
        Task::none()
    }

    pub(super) fn resolve_close(
        &mut self,
        document_id: DocumentId,
        decision: DirtyCloseDecision,
    ) -> Task<Message> {
        self.close_prompt.dismiss(document_id);

        match decision {
            DirtyCloseDecision::Save => {
                if self.files.pending_save.is_some() {
                    self.files.pending_close_after_save = None;
                    self.clear_close();
                    self.files.pending_save_all.clear();
                    self.files.close_goal = CloseGoal::KeepOpen;
                    return Task::none();
                }

                self.files.pending_save_all.clear();
                self.files.pending_close_after_save = Some(document_id);
                self.save_one(document_id, false)
            }
            DirtyCloseDecision::Discard => {
                let close_task = self.close_now(document_id);

                let task = if !self.files.pending_close_documents.is_empty() {
                    Task::batch([close_task, self.continue_close()])
                } else if self.should_exit() {
                    self.files.close_goal = CloseGoal::KeepOpen;
                    Task::batch([close_task, self.exit_after_settings()])
                } else {
                    Task::batch([close_task, self.finish_close_goal()])
                };
                Task::batch([task, self.focus_editor_after_close_prompt()])
            }
            DirtyCloseDecision::Cancel => {
                self.files.pending_close_after_save = None;
                self.clear_close();
                self.files.pending_save_all.clear();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.focus_editor_after_close_prompt()
            }
        }
    }

    pub(super) fn close_now(&mut self, document_id: DocumentId) -> Task<Message> {
        if self
            .files
            .pending_save
            .as_ref()
            .is_some_and(|request| request.document_id == document_id)
        {
            self.file_status = Some(String::from("Finish the current save before closing."));
            return Task::none();
        }
        self.close_prompt.dismiss(document_id);

        let owner = self.workspace.owner_of(document_id);
        self.workspace.close_without_replacement(document_id);
        self.refresh_file_loading_state();

        if let Some(owner) = owner
            && self.workspace.window_tab_ids(owner).is_empty()
            && self.files.close_goal == CloseGoal::KeepOpen
        {
            return self.close_empty_editor(owner);
        }

        let active_document_id = self.workspace.active_document_id();
        self.activate_document(active_document_id)
    }

    pub(super) fn close_documents(&mut self, document_ids: Vec<DocumentId>) -> Task<Message> {
        if self.close_operation_pending() {
            return Task::none();
        }
        self.files.pending_close_documents = VecDeque::from(document_ids);
        self.continue_close()
    }

    pub(super) fn continue_close(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();

        while let Some(document_id) = self.files.pending_close_documents.pop_front() {
            let Some(document) = self.workspace.document(document_id) else {
                continue;
            };

            if document.is_dirty {
                tasks.push(self.close_request_in_operation(document_id));
                return Task::batch(tasks);
            }

            tasks.push(self.close_now(document_id));
        }

        self.clear_close();
        tasks.push(self.finish_close_goal());
        Task::batch(tasks)
    }

    pub(in crate::app) fn exit_request(&mut self) -> Task<Message> {
        let cancel = self.cancel_tab_drag();
        if self.close_operation_pending() {
            return cancel;
        }
        self.menu.close();

        if self.files.pending_save.is_some() {
            self.file_status = Some(String::from("Finish the current save before closing."));
            return Task::none();
        }

        if self.session.is_enabled() {
            return Task::batch([cancel, self.persist_before_exit()]);
        }

        let dirty_documents = self
            .workspace
            .documents()
            .iter()
            .filter(|document| document.is_dirty)
            .map(|document| document.id)
            .collect::<Vec<_>>();

        if dirty_documents.is_empty() {
            return self.exit_after_settings();
        }

        self.files.close_goal = CloseGoal::ExitApp;
        self.close_documents(dirty_documents)
    }

    pub(super) fn clear_close(&mut self) {
        self.files.pending_close_documents.clear();
    }

    pub(in crate::app) fn should_exit(&self) -> bool {
        self.files.close_goal == CloseGoal::ExitApp
    }

    pub(in crate::app) fn close_editor_request(
        &mut self,
        id: crate::core::EditorWindowId,
    ) -> Task<Message> {
        let cancel = self.cancel_tab_drag();
        if self.close_operation_pending() {
            return cancel;
        }
        if !self.switch_editor(id) {
            return cancel;
        }
        if self.editor_windows.len() == 1 && self.settings.exit_on_last_window_close {
            return Task::batch([cancel, self.exit_request()]);
        }
        if self.close_prompt.is_closing() || self.files.pending_save.is_some() {
            return cancel;
        }
        self.files.close_goal = CloseGoal::CloseWindow(id);
        let ids = self.workspace.window_tab_ids(id).to_vec();
        Task::batch([cancel, self.close_documents(ids)])
    }

    pub(in crate::app) fn finish_close_goal(&mut self) -> Task<Message> {
        if self.close_operation_pending() {
            return Task::none();
        }
        match std::mem::take(&mut self.files.close_goal) {
            CloseGoal::CloseWindow(id) => self.close_empty_editor(id),
            CloseGoal::ExitApp => self.exit_after_settings(),
            CloseGoal::KeepOpen => Task::none(),
        }
    }
}
