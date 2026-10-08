//! Dispatches file commands to the loading, saving, and closing workflows.

use crate::message::FileMessage;

use crate::app::App;
use crate::message::Message;
use crate::services;
use iced::{Task, window};
use std::path::PathBuf;

use crate::core::{Document, DocumentId};
use crate::message::SaveRequest;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum CloseGoal {
    #[default]
    KeepOpen,
    ExitApp,
}

#[derive(Debug, Default)]
pub(super) struct FileOperations {
    is_loading: bool,
    pending_save: Option<SaveRequest>,
    pending_auto_saves: VecDeque<DocumentId>,
    pending_reloads: HashMap<DocumentId, Document>,
    pending_save_all: VecDeque<DocumentId>,
    pending_close_after_save: Option<DocumentId>,
    pending_close_documents: VecDeque<DocumentId>,
    close_goal: CloseGoal,
    load_handles: HashMap<DocumentId, iced::task::Handle>,
}

impl FileOperations {
    fn forget_document(&mut self, id: DocumentId) {
        if let Some(handle) = self.load_handles.remove(&id) {
            handle.abort();
        }
        self.pending_reloads.remove(&id);
        self.pending_auto_saves.retain(|pending| *pending != id);
    }
}

// Test inspection does not expose mutable workflow internals to other features.
#[cfg(test)]
impl FileOperations {
    pub(super) fn is_loading(&self) -> bool {
        self.is_loading
    }
    pub(super) fn pending_save(&self) -> Option<&SaveRequest> {
        self.pending_save.as_ref()
    }
    pub(super) fn pending_save_all(&self) -> &VecDeque<DocumentId> {
        &self.pending_save_all
    }
    pub(super) fn pending_auto_saves(&self) -> &VecDeque<DocumentId> {
        &self.pending_auto_saves
    }
    pub(super) fn pending_close_after_save(&self) -> Option<DocumentId> {
        self.pending_close_after_save
    }
    pub(super) fn pending_close_documents(&self) -> &VecDeque<DocumentId> {
        &self.pending_close_documents
    }
    pub(super) fn close_goal(&self) -> CloseGoal {
        self.close_goal
    }
    pub(super) fn pending_reloads(&self) -> &HashMap<DocumentId, Document> {
        &self.pending_reloads
    }
    pub(super) fn load_handles(&self) -> &HashMap<DocumentId, iced::task::Handle> {
        &self.load_handles
    }
    pub(super) fn set_loading(&mut self, loading: bool) {
        self.is_loading = loading;
    }
    pub(super) fn set_pending_save(&mut self, request: SaveRequest) {
        self.pending_save = Some(request);
    }
}

mod closing;
mod loading;
mod saving;

impl App {
    pub(super) fn update_file(&mut self, message: FileMessage) -> Task<Message> {
        match message {
            FileMessage::TabSelected(document_id) => {
                self.menu.close();
                if self.workspace.document(document_id).is_some() {
                    let auto_save = self.auto_save_before_switch(Some(document_id));
                    if self.workspace.select(document_id) {
                        let load = self.activate_document(document_id);

                        return Task::batch([auto_save, load]);
                    }
                }

                Task::none()
            }
            FileMessage::TabClosed(document_id) => {
                self.menu.close();
                self.dragged_tab = None;
                self.hovered_drop_tab = None;
                self.close_request(document_id)
            }
            FileMessage::TabPinToggled(document_id) => {
                self.menu.close();
                self.dragged_tab = None;
                self.hovered_drop_tab = None;
                self.workspace.toggle_pin(document_id);
                Task::none()
            }
            FileMessage::TabDragStarted(document_id) => {
                self.menu.close();
                self.dragged_tab = Some(document_id);
                self.hovered_drop_tab = Some(document_id);
                if self.workspace.document(document_id).is_some() {
                    let auto_save = self.auto_save_before_switch(Some(document_id));
                    if self.workspace.select(document_id) {
                        let load = self.activate_document(document_id);

                        return Task::batch([auto_save, load]);
                    }
                }
                Task::none()
            }
            FileMessage::TabDragHovered(document_id) => {
                if self.dragged_tab.is_some() {
                    self.hovered_drop_tab = Some(document_id);
                }

                Task::none()
            }
            FileMessage::TabDragLeft(document_id) => {
                if self.hovered_drop_tab == Some(document_id) {
                    self.hovered_drop_tab = None;
                }

                Task::none()
            }
            FileMessage::TabDragReleased(document_id) => {
                self.menu.close();
                if let Some(moved_id) = self.dragged_tab.take() {
                    self.workspace.reorder(moved_id, document_id);
                }

                self.hovered_drop_tab = None;
                Task::none()
            }
            FileMessage::NewFile => {
                self.menu.close();
                let auto_save = self.auto_save_before_switch(None);
                self.workspace.create_untitled();

                auto_save
            }
            FileMessage::OpenFile => {
                self.menu.close();
                if self.files.is_loading {
                    Task::none()
                } else {
                    self.files.is_loading = true;
                    self.file_status = None;

                    window::oldest()
                        .and_then(|id| window::run(id, services::file_dialogs::pick_file))
                        .then(Task::future)
                        .map(Message::FilePicked)
                }
            }
            FileMessage::FileDropped(window_id, path) => self.open_dropped_file(window_id, path),
            FileMessage::FilePicked(result) => self.file_picked(result),
            FileMessage::FileOpened(result) => self.open_done(result),
            FileMessage::FileLoadProgress(progress) => self.load_progress(progress),
            FileMessage::FileLoadChunk(chunk) => self.load_chunk(chunk),
            FileMessage::FileLoadFinished(result) => self.load_finished(result),
            FileMessage::SaveFile => {
                self.menu.close();
                self.file_status = None;
                self.save_active(false)
            }
            FileMessage::SaveAllFiles => {
                self.menu.close();
                self.file_status = None;
                self.save_all_documents()
            }
            FileMessage::SaveFileAs => {
                self.menu.close();
                self.file_status = None;
                self.save_active(true)
            }
            FileMessage::SaveCopyAs => {
                self.menu.close();
                self.file_status = None;
                self.save_copy_active()
            }
            FileMessage::FileSaved(request, result) => self.save_done(request, result),
            FileMessage::FileCopySaved(request, result) => self.save_copy_done(request, result),
            FileMessage::ReloadFromDisk => {
                self.menu.close();
                self.reload_active_from_disk()
            }
            FileMessage::EncodingSelected(encoding) => {
                self.menu.close();
                self.reopen_active_with_encoding(encoding)
            }
            FileMessage::EncodingConverted(encoding) => {
                self.menu.close();
                if let Some(document) = self.workspace.active_document_mut() {
                    if !document.has_complete_text_index() {
                        self.file_status =
                            Some(String::from("Finish loading before changing encoding."));
                        return Task::none();
                    }
                    document.set_encoding(encoding);
                }
                Task::none()
            }
            FileMessage::CloseFile => {
                self.menu.close();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.close_request(self.workspace.active_document_id())
            }
            FileMessage::CloseAllFiles => {
                self.menu.close();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.close_documents(self.workspace.document_ids())
            }
            FileMessage::CloseAllButActiveFile => {
                self.menu.close();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.close_documents(
                    self.workspace
                        .document_ids_except(self.workspace.active_document_id()),
                )
            }
            FileMessage::CloseAllButPinnedFiles => {
                self.menu.close();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.close_documents(self.workspace.document_ids_unpinned())
            }
            FileMessage::CloseAllToLeft => {
                self.menu.close();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.close_documents(
                    self.workspace
                        .document_ids_to_left_of(self.workspace.active_document_id()),
                )
            }
            FileMessage::CloseAllToRight => {
                self.menu.close();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.close_documents(
                    self.workspace
                        .document_ids_to_right_of(self.workspace.active_document_id()),
                )
            }
            FileMessage::CloseAllUnchanged => {
                self.menu.close();
                self.files.close_goal = CloseGoal::KeepOpen;
                self.close_documents(self.workspace.document_ids_clean())
            }
            FileMessage::DirtyCloseResolved(document_id, decision) => {
                match self.close_prompt.resolve(document_id, decision) {
                    Some(decision) => self.resolve_close(document_id, decision),
                    None => Task::none(),
                }
            }
            FileMessage::DirtyCloseFadeFinished(document_id) => {
                match self.close_prompt.finish(document_id) {
                    Some(decision) => self.resolve_close(document_id, decision),
                    None => Task::none(),
                }
            }
        }
    }

    fn record_open_history(&mut self, path: PathBuf) -> Task<Message> {
        if self.settings.record_open_history_path(path) {
            self.settings_dialog.draft.open_history = self.settings.open_history.clone();
            self.persist_settings()
        } else {
            Task::none()
        }
    }
}

impl App {
    pub(super) fn open_paths(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        if !self.settings_persistence.is_loaded() || (self.session.waiting_for_startup()) {
            self.session.queue_paths(paths);
            return Task::none();
        }
        let placeholder = self
            .workspace
            .active_document()
            .filter(|doc| {
                self.workspace.documents().len() == 1
                    && doc.path.is_none()
                    && !doc.is_dirty
                    && doc.buffer.len_bytes() == 0
            })
            .map(|doc| doc.id);
        let tasks = paths
            .into_iter()
            .map(|path| self.start_loading_file(path))
            .collect::<Vec<_>>();
        if !tasks.is_empty()
            && let Some(id) = placeholder
        {
            self.workspace.close(id);
        }
        // Poll the active (last requested) file first while retaining tab order.
        Task::batch(tasks.into_iter().rev())
    }
}

impl FileOperations {
    pub(super) fn refresh_loading_state(&mut self, workspace: &crate::core::Workspace) {
        self.is_loading = workspace.documents().iter().any(Document::is_loading);
    }

    pub(super) fn observe(
        &mut self,
        event: super::events::Event,
        work: &mut super::events::PendingWork,
    ) {
        use super::events::Event;
        use crate::core::workspace::changes::WorkspaceEvent as W;
        if let Event::Workspace(W::DocumentClosed(id)) = event {
            self.forget_document(id);
        }
        if matches!(
            event,
            Event::Workspace(W::DocumentClosed(_) | W::DocumentOpened(_) | W::LoadStateChanged(_))
        ) {
            work.request(super::events::Work::Files);
        }
    }
}
