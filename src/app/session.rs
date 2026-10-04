use super::App;
use crate::core::session::{Session, SessionDocument};
use crate::core::{Document, DocumentId, DocumentLoadState};
use crate::editor::{EditorBuffer, EditorPosition, EditorSelection};
use crate::message::Message;
use crate::services::types::FileLoadRequest;
use crate::startup::StartupOptions;
use iced::Task;
use std::{collections::HashMap, path::PathBuf, time::Duration};

#[derive(Debug)]
pub(super) struct SessionState {
    enabled: bool,
    ready: bool,
    loaded: bool,
    initialized: bool,
    flush_scheduled: bool,
    dirty: bool,
    saving: bool,
    read_failed: bool,
    saved: Option<Session>,
    paths: Vec<PathBuf>,
    pending: HashMap<DocumentId, SessionDocument>,
    folds: HashMap<DocumentId, Vec<(usize, usize)>>,
}

impl SessionState {
    fn apply_metadata(&mut self, workspace: &mut crate::core::Workspace, id: DocumentId) {
        let Some(entry) = self.pending.remove(&id) else {
            return;
        };
        let Some(document) = workspace.document_mut(id) else {
            return;
        };
        if entry.text.is_some() {
            document.encoding = entry.encoding;
            document.line_ending = entry
                .line_ending
                .as_deref()
                .and_then(crate::core::document::detect_line_ending);
            if entry.is_dirty {
                document.mark_dirty();
            }
        }
        document.restore_syntax(entry.syntax_token, entry.syntax_automatic);
        document.set_main_selection(EditorSelection::new(
            EditorPosition::new(entry.anchor_line, entry.anchor_column),
            EditorPosition::new(entry.cursor_line, entry.cursor_column),
        ));
        document.restore_session_scroll(
            entry
                .first_visible_position
                .map(|(line, column)| EditorPosition::new(line, column)),
            entry.first_visible_row,
            entry.horizontal_offset,
            !entry.collapsed_folds.is_empty() && document.can_run_full_document_analysis(),
        );
        self.folds.insert(id, entry.collapsed_folds);
    }

    pub(super) fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub(super) fn is_initialized(&self) -> bool {
        self.initialized
    }
    pub(super) fn read_failed(&self) -> bool {
        self.read_failed
    }
    pub(super) fn waiting_for_startup(&self) -> bool {
        self.enabled && !self.initialized
    }
    pub(super) fn has_startup_work(&self) -> bool {
        self.enabled || !self.paths.is_empty()
    }
    pub(super) fn mark_startup_ready(&mut self) {
        self.ready = true;
    }
    pub(super) fn queue_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.paths.extend(paths);
    }
    #[cfg(test)]
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    #[cfg(test)]
    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }
    #[cfg(test)]
    pub(super) fn flush_scheduled(&self) -> bool {
        self.flush_scheduled
    }
    #[cfg(test)]
    pub(super) fn is_saving(&self) -> bool {
        self.saving
    }

    fn forget_document(&mut self, id: DocumentId) {
        self.pending.remove(&id);
        self.folds.remove(&id);
    }

    #[cfg(test)]
    pub(super) fn defer_document(&mut self, id: DocumentId, entry: SessionDocument) {
        self.pending.insert(id, entry);
    }

    pub fn new(options: StartupOptions) -> Self {
        let ready = !options.restore_session && options.files.is_empty();
        Self {
            enabled: options.restore_session,
            ready,
            loaded: !options.restore_session,
            initialized: false,
            flush_scheduled: false,
            dirty: false,
            saving: false,
            read_failed: false,
            saved: None,
            paths: options.files,
            pending: HashMap::new(),
            folds: HashMap::new(),
        }
    }
}

impl App {
    pub(super) fn session_loaded(
        &mut self,
        result: Result<Option<Session>, String>,
    ) -> Task<Message> {
        self.session.loaded = true;
        match result {
            Ok(session) => self.session.saved = session,
            Err(error) => {
                self.session.read_failed = true;
                self.file_status = Some(format!("Session could not be restored: {error}"));
            }
        }
        self.restore_startup()
    }

    pub(super) fn restore_startup(&mut self) -> Task<Message> {
        if self.session.initialized
            || !self.session.ready
            || !self.session.loaded
            || !self.settings_persistence.is_loaded()
        {
            return Task::none();
        }
        self.session.initialized = true;
        let mut tasks = Vec::new();
        if let Some(saved) = self.session.saved.take() {
            let untouched = self.workspace.documents().len() == 1
                && self
                    .workspace
                    .active_document()
                    .is_some_and(|d| d.path.is_none() && !d.is_dirty && d.buffer.len_bytes() == 0);
            if !saved.documents.is_empty() {
                if untouched {
                    self.workspace.clear_documents();
                }
                let mut ids = Vec::new();
                for entry in saved.documents {
                    let id = self.workspace.generate_document_id();
                    let generation = crate::core::DocumentLoadGeneration::next();
                    let mut document =
                        Document::loading(id, entry.path.clone().unwrap_or_default(), generation);
                    document.path = entry.path.clone();
                    document.load_state = DocumentLoadState::Deferred { generation };
                    document.defer_analysis = true;
                    document.is_pinned = entry.is_pinned;
                    document.is_dirty = entry.is_dirty;
                    self.workspace.push_document(document);
                    self.session.pending.insert(id, entry);
                    ids.push(id);
                }
                let id = ids[saved.active_index.min(ids.len() - 1)];
                self.workspace.select(id);
                tasks.push(self.activate_document(id));
            }
        }
        let paths = std::mem::take(&mut self.session.paths);
        tasks.push(self.open_paths(paths));
        self.events.publish(super::events::Event::SessionReady);
        Task::batch(tasks)
    }

    pub(super) fn activate_document(&mut self, id: DocumentId) -> Task<Message> {
        let Some(document) = self.workspace.document_mut(id) else {
            return Task::none();
        };
        let DocumentLoadState::Deferred { generation } = document.load_state else {
            return Task::none();
        };
        document.load_state = DocumentLoadState::Loading {
            generation,
            bytes_read: 0,
            total_bytes: None,
        };
        if let Some(entry) = self.session.pending.get(&id)
            && entry.text.is_some()
        {
            let text = entry.text.as_ref().unwrap();
            document.buffer = EditorBuffer::from_text(text.clone());
            document.complete_streaming_load(generation, entry.encoding);

            return Task::none();
        }
        let Some(path) = document.path.clone() else {
            document.complete_streaming_load(generation, crate::core::TextEncoding::Utf8);
            return Task::none();
        };
        self.start_load_request(FileLoadRequest {
            document_id: id,
            generation,
            path,
            chunk_size: crate::services::chunked_file::DEFAULT_CHUNK_SIZE,
        })
    }

    pub(super) fn snapshot_session(&self) -> Session {
        let mut session = Session::default();
        session.active_index = self
            .workspace
            .documents()
            .iter()
            .position(|d| d.id == self.workspace.active_document_id())
            .unwrap_or(0);
        session.documents = self
            .workspace
            .documents()
            .iter()
            .map(|document| {
                if let Some(pending) = self.session.pending.get(&document.id)
                    && (matches!(document.load_state, DocumentLoadState::Deferred { .. })
                        || !document.is_dirty)
                {
                    let mut entry = pending.clone();
                    entry.is_pinned = document.is_pinned;
                    return entry;
                }
                let selection = document.main_selection();
                SessionDocument {
                    path: document.path.clone(),
                    text: ((document.has_complete_text_index()
                        || matches!(document.load_state, DocumentLoadState::Failed { .. }))
                        && (document.path.is_none() || document.is_dirty))
                        .then(|| document.text()),
                    encoding: document.encoding,
                    line_ending: document
                        .line_ending
                        .map(|ending| ending.as_str().to_owned()),
                    is_pinned: document.is_pinned,
                    is_dirty: document.is_dirty,
                    anchor_line: selection.anchor.line,
                    anchor_column: selection.anchor.column,
                    cursor_line: selection.cursor.line,
                    cursor_column: selection.cursor.column,
                    first_visible_row: document.scroll.first_visible_row,
                    first_visible_position: document
                        .session_top_position()
                        .map(|position| (position.line, position.column)),
                    horizontal_offset: document.scroll.horizontal_px,
                    syntax_token: Some(document.syntax_token.clone()),
                    syntax_automatic: Some(document.syntax_is_automatic()),
                    collapsed_folds: self
                        .session
                        .folds
                        .get(&document.id)
                        .cloned()
                        .unwrap_or_else(|| {
                            document
                                .folds
                                .collapsed_ranges()
                                .map(|r| (r.start_line, r.end_line))
                                .collect()
                        }),
                }
            })
            .collect();
        session
    }

    pub(super) fn request_session_save(&mut self) -> Task<Message> {
        self.session.request_save(self.lifecycle.is_exiting())
    }

    pub(super) fn flush_session(&mut self) -> Task<Message> {
        self.session.flush_scheduled = false;
        if !self.session.dirty
            || self.session.saving
            || !self.session.initialized
            || self.session.read_failed
        {
            return Task::none();
        }
        self.session.dirty = false;
        self.session.saving = true;
        Task::perform(
            crate::services::session_store::save_session(self.snapshot_session()),
            Message::SessionPersisted,
        )
    }

    pub(super) fn session_persisted(&mut self, result: Result<(), String>) -> Task<Message> {
        self.session.saving = false;
        if let Err(error) = result {
            self.session.dirty = true;
            self.file_status = Some(format!("Session save failed: {error}"));
            return Task::none();
        }
        if self.session.dirty {
            self.request_session_save()
        } else {
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests;

impl SessionState {
    pub(super) fn observe(
        &mut self,
        event: super::events::Event,
        workspace: &mut crate::core::Workspace,
        work: &mut super::events::PendingWork,
    ) {
        use super::events::{Event, Work};
        use crate::core::workspace::changes::WorkspaceEvent as W;
        match event {
            Event::SessionReady if self.dirty => work.request(Work::Session),
            Event::Workspace(W::DocumentClosed(id)) => {
                self.forget_document(id);
                work.request(Work::Session);
            }
            Event::Workspace(
                W::DocumentOpened(_)
                | W::ActiveDocumentChanged(_)
                | W::OrderChanged
                | W::ContentChanged(_)
                | W::MetadataChanged(_)
                | W::ViewChanged(_),
            ) => work.request(Work::Session),
            Event::Workspace(W::LoadStateChanged(id)) => {
                if workspace
                    .document(id)
                    .is_some_and(Document::has_complete_text_index)
                {
                    self.apply_metadata(workspace, id);
                }
                work.request(Work::Session);
            }
            Event::AnalysisCompleted(id) => {
                if let Some(ranges) = self.folds.remove(&id)
                    && let Some(document) = workspace.document_mut(id)
                {
                    document.restore_collapsed_folds(&ranges);
                }
            }
            _ => {}
        }
    }

    pub(super) fn request_save(&mut self, exiting: bool) -> Task<Message> {
        if !self.enabled {
            return Task::none();
        }
        self.dirty = true;
        if !self.initialized || self.read_failed || self.flush_scheduled || self.saving || exiting {
            return Task::none();
        }
        self.flush_scheduled = true;
        Task::perform(
            async {
                tokio::time::sleep(Duration::from_secs(2)).await;
            },
            |_| Message::SessionFlush,
        )
    }
}
