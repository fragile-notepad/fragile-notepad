//! Saving snapshots and handling completion or failure.

use crate::app::{App, CloseGoal};
use crate::core::{Document, DocumentId, DocumentLoadState};
use crate::message::{Message, SaveRequest};
use crate::services::types::{
    FileError, FileSaveResult, SaveFileDialogFilter, SaveFileDialogOptions,
};
use crate::services::{file_dialogs, file_system};
use iced::{Task, highlighter, window};
use std::sync::Arc;

fn save_dialog_options(document: &Document) -> SaveFileDialogOptions {
    if !document.uses_syntax_highlighting() {
        return SaveFileDialogOptions::default();
    }

    let Some(syntax) = highlighter::syntaxes()
        .iter()
        .find(|syntax| syntax.token.eq_ignore_ascii_case(&document.syntax_token))
    else {
        return SaveFileDialogOptions::default();
    };

    let mut file_name = document
        .path
        .as_deref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Untitled {}", document.id));
    let mut suggested_path = std::path::PathBuf::from(&file_name);
    suggested_path.set_extension(&syntax.token);
    file_name = suggested_path.to_string_lossy().into_owned();

    SaveFileDialogOptions {
        file_name: Some(file_name),
        filter: Some(SaveFileDialogFilter {
            name: syntax.name.clone(),
            extension: syntax.token.clone(),
        }),
    }
}

impl App {
    pub(in crate::app) fn auto_save_before_switch(
        &mut self,
        target: Option<DocumentId>,
    ) -> Task<Message> {
        let active = self.workspace.active_document_id();
        if target == Some(active) {
            return Task::none();
        }

        self.queue_auto_save(active)
    }

    pub(in crate::app) fn queue_auto_save(&mut self, document_id: DocumentId) -> Task<Message> {
        if !self.settings.auto_save
            || self.close_prompt.is_closing()
            || self.files.pending_auto_saves.contains(&document_id)
        {
            return Task::none();
        }

        let eligible = self
            .workspace
            .document(document_id)
            .is_some_and(|document| {
                document.is_dirty && document.path.is_some() && document.has_complete_text_index()
            });
        if !eligible {
            return Task::none();
        }

        self.files.pending_auto_saves.push_back(document_id);
        self.continue_auto_save()
    }

    fn continue_auto_save(&mut self) -> Task<Message> {
        if !self.settings.auto_save || self.should_exit() {
            self.files.pending_auto_saves.clear();
            return Task::none();
        }
        if self.files.pending_save.is_some() || !self.files.pending_save_all.is_empty() {
            return Task::none();
        }

        while let Some(document_id) = self.files.pending_auto_saves.pop_front() {
            let eligible = self
                .workspace
                .document(document_id)
                .is_some_and(|document| {
                    document.is_dirty
                        && document.path.is_some()
                        && document.has_complete_text_index()
                });
            if !eligible {
                continue;
            }

            let task = self.save_one(document_id, false);
            if self.files.pending_save.is_some() {
                return task;
            }
        }

        Task::none()
    }

    pub(super) fn save_active(&mut self, force_save_as: bool) -> Task<Message> {
        self.files.pending_save_all.clear();
        self.save_one(self.workspace.active_document_id(), force_save_as)
    }

    pub(super) fn save_copy_active(&mut self) -> Task<Message> {
        let id = self.workspace.active_document_id();
        if self
            .workspace
            .document(id)
            .is_some_and(|doc| matches!(doc.load_state, DocumentLoadState::Deferred { .. }))
        {
            let load = self.activate_document(id);
            if self
                .workspace
                .document(id)
                .is_some_and(|doc| !doc.has_complete_text_index())
            {
                return load;
            }
        }
        if self.files.pending_save.is_some() {
            return Task::none();
        }

        let Some(document) = self.workspace.active_document() else {
            return Task::none();
        };
        if document.is_loading_or_indexing() {
            self.file_status = Some(String::from("Finish loading before saving."));
            return Task::none();
        }
        if matches!(document.load_state, DocumentLoadState::Failed { .. }) {
            self.file_status = Some(String::from("Reload the file successfully before saving."));
            return Task::none();
        }
        let snapshot = match document.bytes_for_save() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.file_status = Some(format!(
                    "Save copy failed: {}",
                    FileError::Encoding(error).summary()
                ));
                return Task::none();
            }
        };

        let request = SaveRequest {
            document_id: document.id,
            revision: document.revision(),
            snapshot: Arc::new(snapshot),
        };
        let dialog_options = save_dialog_options(document);
        self.files.pending_save = Some(request.clone());
        let contents = request.snapshot.as_ref().clone();

        window::oldest()
            .and_then(move |id| {
                let contents = contents.clone();
                let dialog_options = dialog_options.clone();

                window::run(id, move |window| {
                    file_dialogs::save_file_copy_as_with_options(window, contents, dialog_options)
                })
            })
            .then(Task::future)
            .map(move |result| Message::FileCopySaved(request.clone(), result))
    }

    pub(super) fn save_all_documents(&mut self) -> Task<Message> {
        if self.files.pending_save.is_some() {
            return Task::none();
        }

        self.files.pending_save_all = self
            .workspace
            .documents()
            .iter()
            .filter(|document| document.is_dirty)
            .map(|document| document.id)
            .collect();

        self.continue_save_all()
    }

    pub(super) fn continue_save_all(&mut self) -> Task<Message> {
        while let Some(document_id) = self.files.pending_save_all.front().copied() {
            if self
                .workspace
                .document(document_id)
                .is_some_and(|document| document.is_dirty)
            {
                return self.save_one(document_id, false);
            }

            self.files.pending_save_all.pop_front();
        }

        self.continue_auto_save()
    }

    pub(super) fn save_one(
        &mut self,
        document_id: DocumentId,
        force_save_as: bool,
    ) -> Task<Message> {
        if self
            .workspace
            .document(document_id)
            .is_some_and(|doc| matches!(doc.load_state, DocumentLoadState::Deferred { .. }))
        {
            let load = self.activate_document(document_id);
            if self
                .workspace
                .document(document_id)
                .is_some_and(|doc| !doc.has_complete_text_index())
            {
                return load;
            }
        }
        if self.files.pending_save.is_some() {
            return Task::none();
        }

        let Some(document) = self.workspace.document_mut(document_id) else {
            return Task::none();
        };
        if document.is_loading_or_indexing() {
            self.file_status = Some(String::from("Finish loading before saving."));
            return Task::none();
        }
        if matches!(document.load_state, DocumentLoadState::Failed { .. }) {
            self.file_status = Some(String::from("Reload the file successfully before saving."));
            return Task::none();
        }
        let snapshot = match document.bytes_for_save() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.save_failed(document_id, FileError::Encoding(error));
                return Task::none();
            }
        };

        let request = SaveRequest {
            document_id: document.id,
            revision: document.revision(),
            snapshot: Arc::new(snapshot),
        };
        document.history.break_group();
        self.files.pending_save = Some(request.clone());

        if !force_save_as && let Some(path) = document.path.clone() {
            let contents = request.snapshot.as_ref().clone();

            return Task::perform(file_system::save_file(path, contents), move |result| {
                Message::FileSaved(request, result)
            });
        }

        let dialog_options = save_dialog_options(document);
        let contents = request.snapshot.as_ref().clone();

        window::oldest()
            .and_then(move |id| {
                let contents = contents.clone();
                let dialog_options = dialog_options.clone();

                window::run(id, move |window| {
                    file_dialogs::save_file_as_with_options(window, contents, dialog_options)
                })
            })
            .then(Task::future)
            .map(move |result| Message::FileSaved(request.clone(), result))
    }

    pub(super) fn save_done(
        &mut self,
        request: SaveRequest,
        result: FileSaveResult,
    ) -> Task<Message> {
        self.files.pending_save = None;
        let save_succeeded = result.is_ok();
        let mut tasks = Vec::new();

        match result {
            Ok(path) => {
                self.file_status = None;
                let saved_path = path.clone();
                if let Some(document) = self.workspace.document_mut(request.document_id) {
                    document.set_path(path);
                    let saved_snapshot_is_current = document
                        .bytes_for_save()
                        .is_ok_and(|bytes| bytes == request.snapshot.as_ref().as_slice());

                    if saved_snapshot_is_current {
                        document.mark_clean();
                    } else {
                        document.invalidate_clean_checkpoint();
                    }
                }
                tasks.push(self.record_open_history(saved_path));
            }
            Err(error) => {
                self.file_status = Some(format!("Save failed: {}", error.summary()));
            }
        }

        if self.files.pending_save_all.front() == Some(&request.document_id) {
            if save_succeeded {
                self.files.pending_save_all.pop_front();
                tasks.push(self.continue_save_all());
                return Task::batch(tasks);
            }

            self.files.pending_save_all.clear();
        }

        if self.files.pending_close_after_save == Some(request.document_id) {
            self.files.pending_close_after_save = None;

            if save_succeeded
                && self
                    .workspace
                    .document(request.document_id)
                    .is_some_and(|document| !document.is_dirty)
            {
                tasks.push(self.close_now(request.document_id));

                if !self.files.pending_close_documents.is_empty() {
                    tasks.push(self.continue_close());
                    return Task::batch(tasks);
                }

                if self.should_exit() {
                    self.files.close_goal = CloseGoal::KeepOpen;
                    tasks.push(self.exit_after_settings());
                }
            } else {
                self.clear_close();
                self.files.close_goal = CloseGoal::KeepOpen;
            }
        }

        tasks.push(self.continue_auto_save());
        Task::batch(tasks)
    }

    pub(super) fn save_copy_done(
        &mut self,
        _request: SaveRequest,
        result: FileSaveResult,
    ) -> Task<Message> {
        self.files.pending_save = None;

        match result {
            Ok(path) => {
                self.file_status = Some(format!("Saved copy: {}", path.display()));
            }
            Err(error) => {
                self.file_status = Some(format!("Save copy failed: {}", error.summary()));
            }
        }

        self.continue_auto_save()
    }

    pub(super) fn save_failed(&mut self, document_id: DocumentId, error: FileError) {
        self.files.pending_save = None;
        self.file_status = Some(format!("Save failed: {}", error.summary()));

        if self.files.pending_save_all.front() == Some(&document_id) {
            self.files.pending_save_all.clear();
        }

        if self.files.pending_close_after_save == Some(document_id) {
            self.files.pending_close_after_save = None;
            self.clear_close();
            self.files.close_goal = CloseGoal::KeepOpen;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_save_dialog_suggests_extension_and_filter() {
        let mut document = Document::untitled(DocumentId::new(7));
        document.set_syntax_token("rs");

        let options = save_dialog_options(&document);

        assert_eq!(options.file_name.as_deref(), Some("Untitled 7.rs"));
        assert_eq!(
            options.filter,
            Some(SaveFileDialogFilter {
                name: "Rust".into(),
                extension: "rs".into(),
            })
        );
    }

    #[test]
    fn language_save_dialog_replaces_existing_extension() {
        let mut document = Document::from_path(DocumentId::new(8), "notes.txt", "body");
        document.set_syntax_token("py");

        let options = save_dialog_options(&document);

        assert_eq!(options.file_name.as_deref(), Some("notes.py"));
        assert_eq!(
            options
                .filter
                .as_ref()
                .map(|filter| filter.extension.as_str()),
            Some("py")
        );
    }

    #[test]
    fn plain_text_save_dialog_has_no_suggestion() {
        let document = Document::untitled(DocumentId::new(9));

        assert_eq!(
            save_dialog_options(&document),
            SaveFileDialogOptions::default()
        );
    }

    #[test]
    fn unknown_syntax_does_not_create_a_filter() {
        let mut document = Document::untitled(DocumentId::new(10));
        document.set_syntax_token("not-a-real-language");

        assert_eq!(
            save_dialog_options(&document),
            SaveFileDialogOptions::default()
        );
    }
}
