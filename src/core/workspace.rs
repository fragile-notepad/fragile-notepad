pub mod changes;
use crate::core::document::{Document, DocumentId, DocumentLoadGeneration};
use crate::core::encoding::DecodedText;
use changes::{ChangeJournal, WorkspaceEvent};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

/// Stable process-local identity of an editor window, independent of native handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditorWindowId(u64);

impl EditorWindowId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorWindow {
    pub tabs: Vec<DocumentId>,
    pub active_document_id: Option<DocumentId>,
    pub selected_tabs: Vec<DocumentId>,
    pub selection_anchor: Option<DocumentId>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    documents: Vec<Document>,
    changes: ChangeJournal,
    indices: HashMap<DocumentId, usize>,
    windows: BTreeMap<EditorWindowId, EditorWindow>,
    current_window: EditorWindowId,
    next_document_id: u64,
    next_window_id: u64,
}

impl Workspace {
    pub fn new() -> Self {
        let first_id = DocumentId::new(1);
        let primary = EditorWindowId::new(1);
        Self {
            documents: vec![Document::untitled(first_id)],
            next_document_id: 2,
            next_window_id: 2,
            changes: ChangeJournal::default(),
            indices: HashMap::from([(first_id, 0)]),
            windows: BTreeMap::from([(
                primary,
                EditorWindow {
                    tabs: vec![first_id],
                    active_document_id: Some(first_id),
                    selected_tabs: vec![first_id],
                    selection_anchor: Some(first_id),
                },
            )]),
            current_window: primary,
        }
    }

    pub const fn primary_window_id() -> EditorWindowId {
        EditorWindowId::new(1)
    }
    pub fn current_window_id(&self) -> EditorWindowId {
        self.current_window
    }
    pub fn set_current_window(&mut self, id: EditorWindowId) -> bool {
        if !self.windows.contains_key(&id) {
            return false;
        }
        self.current_window = id;
        true
    }
    pub fn window_ids(&self) -> Vec<EditorWindowId> {
        self.windows.keys().copied().collect()
    }
    pub fn window(&self, id: EditorWindowId) -> Option<&EditorWindow> {
        self.windows.get(&id)
    }
    pub fn window_tab_ids(&self, id: EditorWindowId) -> &[DocumentId] {
        self.windows
            .get(&id)
            .map_or(&[], |window| window.tabs.as_slice())
    }
    pub fn window_documents(&self, id: EditorWindowId) -> impl Iterator<Item = &Document> {
        self.window_tab_ids(id)
            .iter()
            .filter_map(|id| self.document(*id))
    }
    pub fn active_document_in(&self, id: EditorWindowId) -> Option<&Document> {
        self.windows
            .get(&id)?
            .active_document_id
            .and_then(|id| self.document(id))
    }
    pub fn owner_of(&self, id: DocumentId) -> Option<EditorWindowId> {
        self.windows
            .iter()
            .find_map(|(window, state)| state.tabs.contains(&id).then_some(*window))
    }
    pub fn create_empty_window(&mut self) -> EditorWindowId {
        let id = EditorWindowId::new(self.next_window_id);
        self.next_window_id += 1;
        self.windows.insert(id, EditorWindow::default());
        self.changes.push(WorkspaceEvent::WindowOpened(id));
        id
    }
    pub fn create_window(&mut self) -> EditorWindowId {
        let id = self.create_empty_window();
        self.current_window = id;
        self.create_untitled();
        id
    }
    pub fn remove_empty_window(&mut self, id: EditorWindowId) -> bool {
        if !self
            .windows
            .get(&id)
            .is_some_and(|window| window.tabs.is_empty())
        {
            return false;
        }
        self.windows.remove(&id);
        self.changes.push(WorkspaceEvent::WindowClosed(id));
        if self.current_window == id
            && let Some(next) = self.windows.keys().next()
        {
            self.current_window = *next;
        }
        true
    }

    /// A nonempty editor always has an active document. Empty transition windows
    /// return the reserved zero identity; use `active_document` for optional access.
    pub fn active_document_id(&self) -> DocumentId {
        self.windows
            .get(&self.current_window)
            .and_then(|window| window.active_document_id)
            .unwrap_or(DocumentId::new(0))
    }
    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    /// Apply a bulk edit while preserving tab order and recording touched documents.
    /// As with `document_mut`, edits must preserve each document's ID.
    pub fn edit_documents(&mut self, mut edit: impl FnMut(&mut Document)) {
        for document in &mut self.documents {
            self.changes.touch(document);
            edit(document);
        }
    }
    pub fn push_document(&mut self, document: Document) {
        assert!(
            !self.indices.contains_key(&document.id),
            "document IDs must be unique"
        );
        assert!(
            self.windows.contains_key(&self.current_window),
            "a document needs an editor window"
        );
        self.next_document_id = self.next_document_id.max(document.id.get() + 1);
        self.indices.insert(document.id, self.documents.len());
        self.changes
            .push(WorkspaceEvent::DocumentOpened(document.id));
        let window = self.windows.get_mut(&self.current_window).unwrap();
        window.tabs.push(document.id);
        if window.active_document_id.is_none() {
            window.active_document_id = Some(document.id);
            window.selected_tabs = vec![document.id];
            window.selection_anchor = Some(document.id);
        }
        self.documents.push(document);
        self.normalize_pins(self.current_window);
        self.reindex_in_tab_order();
    }
    pub fn clear_documents(&mut self) {
        self.indices.clear();
        for document in self.documents.drain(..) {
            self.changes
                .push(WorkspaceEvent::DocumentClosed(document.id));
        }
        for window in self.windows.values_mut() {
            *window = EditorWindow::default();
        }
    }
    pub fn publish_changes(&mut self, emit: impl FnMut(WorkspaceEvent)) {
        self.changes.publish(&self.documents, &self.indices, emit);
    }
    pub fn next_document_id(&self) -> DocumentId {
        DocumentId::new(self.next_document_id)
    }
    pub fn generate_document_id(&mut self) -> DocumentId {
        let id = self.next_document_id();
        self.next_document_id += 1;
        id
    }
    pub fn create_untitled(&mut self) -> DocumentId {
        let id = self.generate_document_id();
        self.push_document(Document::untitled(id));
        self.select(id);
        id
    }
    pub fn insert_loaded_file(&mut self, path: impl Into<PathBuf>, text: &str) -> DocumentId {
        let id = self.generate_document_id();
        self.push_document(Document::from_path(id, path, text));
        self.select(id);
        id
    }
    pub fn insert_decoded_file(
        &mut self,
        path: impl Into<PathBuf>,
        decoded: DecodedText,
    ) -> DocumentId {
        let id = self.generate_document_id();
        self.push_document(Document::from_decoded(id, path, decoded));
        self.select(id);
        id
    }
    pub fn insert_loading_file(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> (DocumentId, DocumentLoadGeneration) {
        let id = self.generate_document_id();
        let generation = DocumentLoadGeneration::next();
        self.push_document(Document::loading(id, path, generation));
        self.select(id);
        (id, generation)
    }
    pub fn active_document(&self) -> Option<&Document> {
        self.active_document_in(self.current_window)
    }
    pub fn active_document_mut(&mut self) -> Option<&mut Document> {
        self.document_mut(self.active_document_id())
    }
    pub fn document(&self, id: DocumentId) -> Option<&Document> {
        self.indices
            .get(&id)
            .and_then(|index| self.documents.get(*index))
    }
    pub fn document_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        let index = self.index_of(id)?;
        let document = &mut self.documents[index];
        self.changes.touch(document);
        Some(document)
    }
    pub fn select(&mut self, id: DocumentId) -> bool {
        let Some(owner) = self.owner_of(id) else {
            return false;
        };
        self.current_window = owner;
        self.select_in_window(owner, id)
    }
    pub fn select_in_window(&mut self, window_id: EditorWindowId, id: DocumentId) -> bool {
        let Some(window) = self.windows.get_mut(&window_id) else {
            return false;
        };
        if !window.tabs.contains(&id) {
            return false;
        }
        if window.active_document_id != Some(id) {
            window.active_document_id = Some(id);
            self.changes.push(WorkspaceEvent::ActiveDocumentChanged(id));
        }
        window.selected_tabs = vec![id];
        window.selection_anchor = Some(id);
        true
    }
    pub fn select_tabs(
        &mut self,
        window_id: EditorWindowId,
        ids: &[DocumentId],
        active: DocumentId,
    ) -> bool {
        let Some(window) = self.windows.get_mut(&window_id) else {
            return false;
        };
        if !ids.contains(&active) || ids.iter().any(|id| !window.tabs.contains(id)) {
            return false;
        }
        window.selected_tabs = window
            .tabs
            .iter()
            .filter(|id| ids.contains(id))
            .copied()
            .collect();
        window.selection_anchor = Some(active);
        if window.active_document_id != Some(active) {
            window.active_document_id = Some(active);
            self.changes
                .push(WorkspaceEvent::ActiveDocumentChanged(active));
        }
        self.changes
            .push(WorkspaceEvent::WindowTabsChanged(window_id));
        true
    }
    pub fn toggle_tab_selection(&mut self, window_id: EditorWindowId, id: DocumentId) -> bool {
        let Some(window) = self.windows.get_mut(&window_id) else {
            return false;
        };
        if !window.tabs.contains(&id) {
            return false;
        }
        if window.selected_tabs.contains(&id) {
            if window.selected_tabs.len() == 1 {
                return true;
            }
            window.selected_tabs.retain(|candidate| *candidate != id);
            if window.active_document_id == Some(id) {
                let active = window.selected_tabs[0];
                window.active_document_id = Some(active);
                self.changes
                    .push(WorkspaceEvent::ActiveDocumentChanged(active));
            }
        } else {
            window.selected_tabs.push(id);
            window
                .selected_tabs
                .sort_by_key(|id| window.tabs.iter().position(|tab| tab == id));
            window.active_document_id = Some(id);
            self.changes.push(WorkspaceEvent::ActiveDocumentChanged(id));
        }
        window.selection_anchor = Some(id);
        self.changes
            .push(WorkspaceEvent::WindowTabsChanged(window_id));
        true
    }
    pub fn select_tab_range(&mut self, window_id: EditorWindowId, id: DocumentId) -> bool {
        let Some(window) = self.windows.get_mut(&window_id) else {
            return false;
        };
        let Some(end) = window.tabs.iter().position(|candidate| *candidate == id) else {
            return false;
        };
        let start = window
            .selection_anchor
            .and_then(|anchor| window.tabs.iter().position(|tab| *tab == anchor))
            .unwrap_or(end);
        window.selected_tabs = window.tabs[start.min(end)..=start.max(end)].to_vec();
        window.active_document_id = Some(id);
        window.selection_anchor.get_or_insert(id);
        self.changes.push(WorkspaceEvent::ActiveDocumentChanged(id));
        self.changes
            .push(WorkspaceEvent::WindowTabsChanged(window_id));
        true
    }

    pub fn close(&mut self, id: DocumentId) -> Option<Document> {
        let owner = self.owner_of(id)?;
        let removed = self.close_without_replacement(id)?;
        if self.window_tab_ids(owner).is_empty() {
            let previous = self.current_window;
            self.current_window = owner;
            self.create_untitled();
            if previous != owner {
                self.current_window = previous;
            }
        }
        Some(removed)
    }
    pub fn close_without_replacement(&mut self, id: DocumentId) -> Option<Document> {
        let index = self.index_of(id)?;
        let owner = self.owner_of(id)?;
        let removed = self.documents.remove(index);
        self.reindex();
        self.changes.push(WorkspaceEvent::DocumentClosed(id));
        let window = self.windows.get_mut(&owner).unwrap();
        let tab_index = window
            .tabs
            .iter()
            .position(|candidate| *candidate == id)
            .unwrap();
        window.tabs.remove(tab_index);
        window.selected_tabs.retain(|candidate| *candidate != id);
        if window.active_document_id == Some(id) {
            window.active_document_id = window.tabs.get(tab_index.saturating_sub(1)).copied();
            if let Some(active) = window.active_document_id {
                if !window.selected_tabs.contains(&active) {
                    window.selected_tabs.push(active);
                }
                self.changes
                    .push(WorkspaceEvent::ActiveDocumentChanged(active));
            }
        }
        if window.selection_anchor == Some(id) {
            window.selection_anchor = window.active_document_id;
        }
        Some(removed)
    }

    /// Atomically move live documents, preserving IDs, buffers, history and load generations.
    /// `insertion` is a slot in the destination before removing the moving group.
    pub fn transfer_tabs(
        &mut self,
        source: EditorWindowId,
        ids: &[DocumentId],
        target: EditorWindowId,
        insertion: usize,
    ) -> bool {
        let (Some(source_window), Some(target_window)) =
            (self.windows.get(&source), self.windows.get(&target))
        else {
            return false;
        };
        let unique: HashSet<_> = ids.iter().copied().collect();
        if unique.is_empty()
            || unique.len() != ids.len()
            || ids.iter().any(|id| !source_window.tabs.contains(id))
        {
            return false;
        }
        let moving: Vec<_> = source_window
            .tabs
            .iter()
            .filter(|id| unique.contains(id))
            .copied()
            .collect();
        let source_active = source_window.active_document_id;
        let old_tabs = source_window.tabs.clone();
        let mut slot = insertion.min(target_window.tabs.len());
        if source == target {
            slot -= source_window.tabs[..slot]
                .iter()
                .filter(|id| unique.contains(id))
                .count();
        }
        let source_state = self.windows.get_mut(&source).unwrap();
        let first_removed = source_state
            .tabs
            .iter()
            .position(|id| unique.contains(id))
            .unwrap();
        source_state.tabs.retain(|id| !unique.contains(id));
        source_state.selected_tabs.retain(|id| !unique.contains(id));
        if source_state
            .active_document_id
            .is_some_and(|id| unique.contains(&id))
        {
            source_state.active_document_id = source_state
                .tabs
                .get(
                    first_removed
                        .saturating_sub(1)
                        .min(source_state.tabs.len().saturating_sub(1)),
                )
                .copied();
            source_state.selected_tabs = source_state.active_document_id.into_iter().collect();
        }
        if source_state
            .selection_anchor
            .is_some_and(|id| unique.contains(&id))
        {
            source_state.selection_anchor = source_state.active_document_id;
        }
        let target_state = self.windows.get_mut(&target).unwrap();
        let slot = slot.min(target_state.tabs.len());
        target_state.tabs.splice(slot..slot, moving.iter().copied());
        target_state.active_document_id = source_active
            .filter(|id| unique.contains(id))
            .or_else(|| moving.first().copied());
        target_state.selected_tabs = moving;
        target_state.selection_anchor = target_state.active_document_id;
        self.normalize_pins(target);
        if source != target {
            self.normalize_pins(source);
        }
        let target_state = self.windows.get_mut(&target).unwrap();
        target_state.selected_tabs = target_state
            .tabs
            .iter()
            .filter(|id| unique.contains(id))
            .copied()
            .collect();
        let changed = source != target || self.window_tab_ids(source) != old_tabs;
        if changed {
            self.reindex_in_tab_order();
            self.changes.push(WorkspaceEvent::OrderChanged);
            self.changes.push(WorkspaceEvent::WindowTabsChanged(source));
            if source != target {
                self.changes.push(WorkspaceEvent::WindowTabsChanged(target));
            }
            if let Some(active) = self.windows[&target].active_document_id {
                self.changes
                    .push(WorkspaceEvent::ActiveDocumentChanged(active));
            }
        }
        changed
    }

    /// Restore a drag transaction's membership and selection only. Document
    /// buffers and in-flight load/save generations remain untouched. Windows
    /// created during the gesture are left empty for native teardown.
    pub fn restore_window_layout(&mut self, layout: &[(EditorWindowId, EditorWindow)]) -> bool {
        let mut windows = HashSet::new();
        let mut assigned = HashSet::new();
        for (id, state) in layout {
            if !self.windows.contains_key(id) || !windows.insert(*id) {
                return false;
            }
            if state.tabs.is_empty() != state.active_document_id.is_none()
                || state
                    .active_document_id
                    .is_some_and(|id| !state.tabs.contains(&id))
                || state
                    .selection_anchor
                    .is_some_and(|id| !state.tabs.contains(&id))
                || state
                    .selected_tabs
                    .iter()
                    .any(|id| !state.tabs.contains(id))
                || state.selected_tabs.iter().collect::<HashSet<_>>().len()
                    != state.selected_tabs.len()
            {
                return false;
            }
            for id in &state.tabs {
                if self.document(*id).is_none() || !assigned.insert(*id) {
                    return false;
                }
            }
        }
        if assigned.len() != self.documents.len() {
            return false;
        }
        let desired: HashMap<_, _> = layout.iter().map(|(id, state)| (*id, state)).collect();
        let mut changed = false;
        for (id, state) in &mut self.windows {
            let replacement = desired
                .get(id)
                .map_or_else(EditorWindow::default, |state| (**state).clone());
            if *state != replacement {
                if state.active_document_id != replacement.active_document_id
                    && let Some(active) = replacement.active_document_id
                {
                    self.changes
                        .push(WorkspaceEvent::ActiveDocumentChanged(active));
                }
                *state = replacement;
                self.changes.push(WorkspaceEvent::WindowTabsChanged(*id));
                changed = true;
            }
        }
        if changed {
            self.reindex_in_tab_order();
            self.changes.push(WorkspaceEvent::OrderChanged);
        }
        true
    }

    pub fn document_ids(&self) -> Vec<DocumentId> {
        self.window_tab_ids(self.current_window).to_vec()
    }
    pub fn all_document_ids(&self) -> Vec<DocumentId> {
        self.documents.iter().map(|document| document.id).collect()
    }
    pub fn document_ids_except(&self, excluded_id: DocumentId) -> Vec<DocumentId> {
        self.document_ids_matching(|document| document.id != excluded_id)
    }
    pub fn document_ids_unpinned(&self) -> Vec<DocumentId> {
        self.document_ids_matching(|document| !document.is_pinned)
    }
    pub fn document_ids_clean(&self) -> Vec<DocumentId> {
        self.document_ids_matching(|document| !document.is_dirty)
    }
    pub fn document_ids_to_left_of(&self, id: DocumentId) -> Vec<DocumentId> {
        let Some(owner) = self.owner_of(id) else {
            return Vec::new();
        };
        let tabs = self.window_tab_ids(owner);
        let index = tabs.iter().position(|tab| *tab == id).unwrap();
        tabs[..index].to_vec()
    }
    pub fn document_ids_to_right_of(&self, id: DocumentId) -> Vec<DocumentId> {
        let Some(owner) = self.owner_of(id) else {
            return Vec::new();
        };
        let tabs = self.window_tab_ids(owner);
        let index = tabs.iter().position(|tab| *tab == id).unwrap();
        tabs[index + 1..].to_vec()
    }
    pub fn toggle_pin(&mut self, id: DocumentId) -> bool {
        let Some(owner) = self.owner_of(id) else {
            return false;
        };
        let document = self.document_mut(id).unwrap();
        document.is_pinned = !document.is_pinned;
        let window = self.windows.get_mut(&owner).unwrap();
        window.tabs.retain(|tab| *tab != id);
        window.tabs.push(id);
        self.normalize_pins(owner);
        self.reindex_in_tab_order();
        self.changes.push(WorkspaceEvent::OrderChanged);
        true
    }
    pub fn reorder(&mut self, moved_id: DocumentId, target_id: DocumentId) -> bool {
        if moved_id == target_id {
            return false;
        }
        let Some(owner) = self.owner_of(moved_id) else {
            return false;
        };
        if self.owner_of(target_id) != Some(owner)
            || self.document(moved_id).unwrap().is_pinned
                != self.document(target_id).unwrap().is_pinned
        {
            return false;
        }
        let window = self.windows.get_mut(&owner).unwrap();
        let from = window.tabs.iter().position(|id| *id == moved_id).unwrap();
        let to = window.tabs.iter().position(|id| *id == target_id).unwrap();
        window.tabs.remove(from);
        window.tabs.insert(to, moved_id);
        self.reindex_in_tab_order();
        self.changes.push(WorkspaceEvent::OrderChanged);
        true
    }
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
    pub fn pinned_count(&self) -> usize {
        self.window_documents(self.current_window)
            .take_while(|document| document.is_pinned)
            .count()
    }
    fn normalize_pins(&mut self, window_id: EditorWindowId) {
        let indices = &self.indices;
        let documents = &self.documents;
        if let Some(window) = self.windows.get_mut(&window_id) {
            window
                .tabs
                .sort_by_key(|id| !documents[indices[id]].is_pinned);
        }
    }
    fn reindex_in_tab_order(&mut self) {
        let positions: HashMap<_, _> = self
            .windows
            .values()
            .flat_map(|window| window.tabs.iter())
            .enumerate()
            .map(|(index, id)| (*id, index))
            .collect();
        self.documents
            .sort_by_key(|document| positions[&document.id]);
        self.reindex();
    }
    fn reindex(&mut self) {
        self.indices.clear();
        self.indices.extend(
            self.documents
                .iter()
                .enumerate()
                .map(|(index, document)| (document.id, index)),
        );
    }
    fn index_of(&self, id: DocumentId) -> Option<usize> {
        self.indices.get(&id).copied()
    }
    fn document_ids_matching(&self, predicate: impl Fn(&Document) -> bool) -> Vec<DocumentId> {
        self.window_documents(self.current_window)
            .filter(|document| predicate(document))
            .map(|document| document.id)
            .collect()
    }
}
impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}
