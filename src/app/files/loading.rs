//! Opening, reloading, and streamed file loading.

use crate::app::App;
use crate::core::{
    DocumentId, DocumentIndexState, DocumentLoadGeneration, DocumentLoadState, TextEncoding,
};
use crate::message::Message;
use crate::services::chunked_file::{self, DEFAULT_CHUNK_SIZE};
use crate::services::types::{
    FileLoadChunk, FileLoadFailure, FileLoadFinished, FileLoadProgress, FileLoadRequest,
    FileOpenResult, FileResult,
};
use iced::{Task, window};
use std::path::PathBuf;

impl App {
    pub(super) fn open_done(&mut self, result: FileOpenResult) -> Task<Message> {
        let task = match result {
            Ok(opened) => {
                self.file_status = None;
                let opened_path = opened.path.clone();
                if let Some(document_id) = self.loading_document_id_for_path(&opened.path) {
                    let auto_save = self.auto_save_before_switch(Some(document_id));
                    if let Some(document) = self.workspace.document_mut(document_id)
                        && let Some(generation) = document.load_generation()
                    {
                        document.complete_loading(generation, opened.contents.as_ref().clone());
                        document.disk_revision = Some(opened.disk_revision);
                    }
                    self.workspace.select(document_id);
                    let history = self.record_open_history(opened_path);
                    Task::batch([auto_save, history])
                } else {
                    let auto_save = self.auto_save_before_switch(None);
                    self.workspace
                        .insert_decoded_file(opened.path, opened.contents.as_ref().clone());
                    if let Some(document) = self.workspace.active_document_mut() {
                        document.disk_revision = Some(opened.disk_revision);
                    }
                    let history = self.record_open_history(opened_path);
                    Task::batch([auto_save, history])
                }
            }
            Err(error) => {
                self.file_status = Some(format!("Open failed: {}", error.summary()));
                Task::none()
            }
        };

        self.refresh_file_loading_state();
        task
    }

    pub(super) fn file_picked(&mut self, result: FileResult<PathBuf>) -> Task<Message> {
        match result {
            Ok(path) => self.start_loading_file(path),
            Err(error) => {
                self.file_status = Some(format!("Open failed: {}", error.summary()));
                self.refresh_file_loading_state();
                Task::none()
            }
        }
    }

    pub(super) fn open_dropped_file(
        &mut self,
        window_id: window::Id,
        path: PathBuf,
    ) -> Task<Message> {
        if self.main_window_id != Some(window_id) {
            return Task::none();
        }

        self.menu.close();
        self.start_loading_file(path)
    }

    pub(in crate::app) fn start_loading_file(&mut self, path: PathBuf) -> Task<Message> {
        if self.session.waiting_for_startup() {
            self.session.queue_paths([path]);
            return Task::none();
        }
        let path = std::path::absolute(&path).unwrap_or(path);
        if let Some(id) = self
            .workspace
            .documents()
            .iter()
            .find(|doc| {
                doc.path
                    .as_ref()
                    .is_some_and(|existing| same_file_path(existing, &path))
            })
            .map(|doc| doc.id)
        {
            let auto_save = self.auto_save_before_switch(Some(id));
            self.workspace.select(id);
            let activate = self.activate_document(id);
            return Task::batch([auto_save, activate]);
        }
        self.files.is_loading = true;
        self.file_status = None;

        let auto_save = self.auto_save_before_switch(None);
        let (document_id, generation) = self.workspace.insert_loading_file(path.clone());
        if let Some(document) = self.workspace.document_mut(document_id) {
            document.defer_analysis = true;
        }

        let load = self.start_load_request(FileLoadRequest {
            document_id,
            generation,
            path,
            chunk_size: DEFAULT_CHUNK_SIZE,
        });
        Task::batch([auto_save, load])
    }

    pub(super) fn reload_active_from_disk(&mut self) -> Task<Message> {
        let encoding = self
            .workspace
            .active_document()
            .map(|document| document.encoding);
        self.reload_active_with_encoding(encoding)
    }

    pub(super) fn reopen_active_with_encoding(&mut self, encoding: TextEncoding) -> Task<Message> {
        let Some(document) = self.workspace.active_document_mut() else {
            return Task::none();
        };
        if !document.has_complete_text_index() {
            self.file_status = Some("Finish loading before changing encoding.".into());
            return Task::none();
        }
        if document.path.is_none() {
            document.set_encoding(encoding);
            return Task::none();
        }
        self.reload_active_with_encoding(Some(encoding))
    }

    fn reload_active_with_encoding(&mut self, encoding: Option<TextEncoding>) -> Task<Message> {
        let document_id = self.workspace.active_document_id();
        let Some(document) = self.workspace.document(document_id) else {
            return Task::none();
        };

        let Some(path) = document.path.clone() else {
            self.file_status = Some(String::from("Reload from disk requires a saved file."));
            return Task::none();
        };

        if document.is_dirty {
            self.file_status = Some(String::from("Save changes before reloading from disk."));
            return Task::none();
        }

        if document.is_loading_or_indexing() {
            self.file_status = Some(String::from("Finish loading before reloading."));
            return Task::none();
        }

        if self.files.pending_save.is_some() {
            self.file_status = Some("Finish the current save before reloading.".into());
            return Task::none();
        }

        let generation = DocumentLoadGeneration::next();
        if let Some(document) = self.workspace.document_mut(document_id) {
            document.load_state = DocumentLoadState::Loading {
                generation,
                bytes_read: 0,
                total_bytes: None,
            };
            document.index_state = DocumentIndexState::Pending { generation };
            let staged = crate::core::Document::loading(document_id, path.clone(), generation);
            self.files.pending_reloads.insert(document_id, staged);
        }

        self.files.is_loading = true;
        self.file_status = None;

        self.start_load_request_with_encoding(
            FileLoadRequest {
                document_id,
                generation,
                path,
                chunk_size: DEFAULT_CHUNK_SIZE,
            },
            encoding,
        )
    }

    pub(super) fn load_progress(&mut self, progress: FileLoadProgress) -> Task<Message> {
        let Some(document) = self.workspace.document_mut(progress.document_id) else {
            return Task::none();
        };

        document.update_load_progress(
            progress.generation,
            progress.bytes_read,
            progress.total_bytes,
        );
        Task::none()
    }

    pub(super) fn load_chunk(&mut self, chunk: FileLoadChunk) -> Task<Message> {
        if let Some(staged) = self.files.pending_reloads.get_mut(&chunk.document_id) {
            staged.replace_loading_preview(
                chunk.generation,
                chunk.text.as_ref(),
                chunk.reset,
                chunk.bytes_read,
                chunk.total_bytes,
            );
            if let Some(document) = self.workspace.document_mut(chunk.document_id) {
                document.update_load_progress(
                    chunk.generation,
                    chunk.bytes_read,
                    chunk.total_bytes,
                );
            }
            return Task::none();
        }
        let Some(document) = self.workspace.document_mut(chunk.document_id) else {
            return Task::none();
        };

        document.replace_loading_preview(
            chunk.generation,
            chunk.text.as_ref(),
            chunk.reset,
            chunk.bytes_read,
            chunk.total_bytes,
        );

        Task::none()
    }

    pub(super) fn load_finished(
        &mut self,
        result: Result<FileLoadFinished, FileLoadFailure>,
    ) -> Task<Message> {
        let (id, generation) = match &result {
            Ok(done) => (done.document_id, done.generation),
            Err(failed) => (failed.document_id, failed.generation),
        };
        if self
            .workspace
            .document(id)
            .is_some_and(|doc| doc.has_active_load(generation))
        {
            self.files.load_handles.remove(&id);
        }
        let task = match result {
            Ok(finished) => {
                let opened_path = finished.path.clone();
                let Some(document) = self.workspace.document_mut(finished.document_id) else {
                    self.files.pending_reloads.remove(&finished.document_id);
                    self.refresh_file_loading_state();
                    return Task::none();
                };

                if !document.has_active_load(finished.generation) {
                    self.refresh_file_loading_state();
                    return Task::none();
                }
                // Only publish a reload after every chunk has arrived successfully.
                if let Some(staged) = self.files.pending_reloads.remove(&finished.document_id) {
                    document.set_selection_set(staged.selection_set().clone());
                    document.buffer = staged.buffer;
                }

                let completed = if let Some(contents) = finished.fallback_contents {
                    document.complete_loading(finished.generation, contents.as_ref().clone())
                } else {
                    document.complete_streaming_load(finished.generation, finished.encoding)
                };

                if !completed {
                    self.refresh_file_loading_state();
                    return Task::none();
                }
                document.disk_revision = Some(finished.disk_revision);

                self.file_status = finished.had_errors.then(|| {
                    String::from("Opened with decoding errors; check the text before saving.")
                });
                self.record_open_history(opened_path)
            }
            Err(failure) => self.load_failed(failure),
        };

        self.refresh_file_loading_state();
        task
    }

    pub(super) fn load_failed(&mut self, failure: FileLoadFailure) -> Task<Message> {
        let Some(document) = self.workspace.document_mut(failure.document_id) else {
            return Task::none();
        };

        if document.fail_loading(failure.generation) {
            if self
                .files
                .pending_reloads
                .remove(&failure.document_id)
                .is_some()
            {
                document.load_state = DocumentLoadState::Complete;
                document.index_state = DocumentIndexState::Complete;
            }
            self.file_status = Some(format!("Open failed: {}", failure.error.summary()));
        }

        Task::none()
    }

    pub(super) fn refresh_file_loading_state(&mut self) {
        self.files.refresh_loading_state(&self.workspace);
    }

    pub(super) fn loading_document_id_for_path(
        &self,
        path: &std::path::Path,
    ) -> Option<DocumentId> {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        self.workspace
            .documents()
            .iter()
            .find(|document| {
                document.is_loading()
                    && document
                        .path
                        .as_ref()
                        .is_some_and(|existing| same_file_path(existing, &path))
            })
            .map(|document| document.id)
    }

    pub(in crate::app) fn start_load_request(&mut self, request: FileLoadRequest) -> Task<Message> {
        self.start_load_request_with_encoding(request, None)
    }

    pub(in crate::app) fn start_load_request_with_encoding(
        &mut self,
        request: FileLoadRequest,
        encoding: Option<TextEncoding>,
    ) -> Task<Message> {
        let id = request.document_id;
        let (task, handle) = Task::run(
            chunked_file::load_file_chunks_with_encoding(request, encoding),
            Message::from,
        )
        .abortable();
        if let Some(previous) = self.files.load_handles.insert(id, handle) {
            previous.abort();
        }
        task
    }
}

fn same_file_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        fn fold(unit: u16) -> u16 {
            if (b'A' as u16..=b'Z' as u16).contains(&unit) {
                unit + 32
            } else {
                unit
            }
        }
        a.as_os_str()
            .encode_wide()
            .map(fold)
            .eq(b.as_os_str().encode_wide().map(fold))
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}
