use crate::message::SearchMessage;
use iced::Task;
use iced::widget::operation;

use crate::core::{Document, PreparedSearch};
use crate::editor::{
    EditorRange, EditorSelection, position_for_byte_offset, word_range_at_position,
};
use crate::message::{AdvancedSearchTab, Message};
use crate::ui::advanced_search_panel::QUERY_INPUT_ID;
use crate::ui::find_panel::FIND_INPUT_ID;

use super::App;

#[derive(Debug)]
pub(super) struct PendingSearch {
    dialog: crate::search_dialog::SearchDialogState,
    search: PreparedSearch,
    documents: Vec<crate::core::DocumentId>,
    operation: SearchOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchOperation {
    Find,
    Count,
    Replace,
}

impl App {
    pub(super) fn refresh_loading_find(&mut self) -> Task<Message> {
        self.loading_find_scheduled = false;
        self.refresh_find_matches();
        Task::none()
    }

    pub(super) fn update_search(&mut self, message: SearchMessage) -> Task<Message> {
        // Editing the request or starting a new search cancels its queued work.
        // Loading already requested documents may finish, but cannot mutate text.
        if !matches!(
            message,
            SearchMessage::AdvancedSearchResultSelected(_, _)
                | SearchMessage::AdvancedResultOptionsToggled
                | SearchMessage::AdvancedResultLimitChanged(_)
                | SearchMessage::AdvancedPreviewCharsChanged(_)
                | SearchMessage::AdvancedPreviewContextChanged(_)
                | SearchMessage::AdvancedResultOptionsReset
        ) {
            if self.pending_search.is_some() {
                self.search_dialog.status = String::from("Search canceled.");
            }
            self.pending_search = None;
        }
        if let Some(document) = self.workspace.active_document_mut() {
            document.sync_selection_mirror();
        }

        match message {
            SearchMessage::AdvancedResultOptionsToggled => {
                self.search_dialog.result_options_visible =
                    !self.search_dialog.result_options_visible;
                Task::none()
            }
            SearchMessage::AdvancedResultLimitChanged(value) => {
                self.search_dialog.result_limit_input = value;
                self.persist_search_result_options()
            }
            SearchMessage::AdvancedPreviewCharsChanged(value) => {
                self.search_dialog.preview_chars_input = value;
                self.persist_search_result_options()
            }
            SearchMessage::AdvancedPreviewContextChanged(value) => {
                self.search_dialog.context_before_input = value;
                self.persist_search_result_options()
            }
            SearchMessage::AdvancedResultOptionsReset => {
                self.search_dialog
                    .set_result_settings(crate::core::SearchResultSettings::default());
                self.persist_search_result_options()
            }
            SearchMessage::FindQueryChanged(query) => {
                self.find.set_query(query);
                self.refresh_find_matches();
                Task::none()
            }
            SearchMessage::FindReplacementChanged(replacement) => {
                self.find.set_replacement(replacement);
                Task::none()
            }
            SearchMessage::FindCaseSensitiveToggled(case_sensitive) => {
                self.find.set_case_sensitive(case_sensitive);
                self.refresh_find_matches();
                Task::none()
            }
            SearchMessage::FindWholeWordToggled(whole_word) => {
                self.find.set_whole_word(whole_word);
                self.refresh_find_matches();
                Task::none()
            }
            SearchMessage::ToggleInlineReplace => {
                self.is_inline_replace_visible = !self.is_inline_replace_visible;
                self.chrome_animation
                    .inline_replace
                    .set_visible(self.is_inline_replace_visible);
                Task::none()
            }
            SearchMessage::ShowInlineReplace => {
                self.is_find_visible = true;
                self.is_inline_replace_visible = true;
                self.chrome_animation.find.set_visible(true);
                self.chrome_animation.inline_replace.set_visible(true);
                operation::focus(FIND_INPUT_ID)
            }
            SearchMessage::ToggleFind => self.toggle_find_panel(),
            SearchMessage::HideFind => {
                self.is_find_visible = false;
                self.chrome_animation.find.set_visible(false);
                operation::focus(crate::ui::editor::EDITOR_ID)
            }
            SearchMessage::FindNext => {
                self.menu.close();
                let text_match = self.find.next();
                self.select_active_match(text_match);
                Task::none()
            }
            SearchMessage::FindPrevious => {
                self.menu.close();
                let text_match = self.find.previous();
                self.select_active_match(text_match);
                Task::none()
            }
            SearchMessage::SelectAndFindNext => {
                self.select_text_for_find(true, true);
                Task::none()
            }
            SearchMessage::SelectAndFindPrevious => {
                self.select_text_for_find(true, false);
                Task::none()
            }
            SearchMessage::VolatileFindNext => {
                self.select_text_for_find(false, true);
                Task::none()
            }
            SearchMessage::VolatileFindPrevious => {
                self.select_text_for_find(false, false);
                Task::none()
            }
            SearchMessage::ReplaceCurrent => self.replace_current(),
            SearchMessage::ReplaceAll => self.replace_all(),
            SearchMessage::ToggleAdvancedSearch(tab) => self.toggle_advanced_search_window(tab),
            SearchMessage::AdvancedSearchTabSelected(tab) => {
                self.search_dialog.set_active_tab(tab);
                self.refresh_search_results();
                operation::focus(QUERY_INPUT_ID)
            }
            SearchMessage::AdvancedSearchQueryChanged(query) => {
                self.search_dialog.set_query(query);
                Task::none()
            }
            SearchMessage::AdvancedSearchReplacementChanged(replacement) => {
                self.search_dialog.set_replacement(replacement);
                Task::none()
            }
            SearchMessage::AdvancedSearchCaseSensitiveToggled(case_sensitive) => {
                self.search_dialog.set_case_sensitive(case_sensitive);
                Task::none()
            }
            SearchMessage::AdvancedSearchWholeWordToggled(whole_word) => {
                self.search_dialog.set_whole_word(whole_word);
                Task::none()
            }
            SearchMessage::AdvancedSearchWrapAroundToggled(wrap_around) => {
                self.search_dialog.set_wrap_around(wrap_around);
                Task::none()
            }
            SearchMessage::AdvancedSearchModeSelected(mode) => {
                self.search_dialog.set_mode(mode);
                self.refresh_search_results();
                Task::none()
            }
            SearchMessage::AdvancedSearchIncludeChanged(include_pattern) => {
                self.search_dialog.set_include_pattern(include_pattern);
                Task::none()
            }
            SearchMessage::AdvancedSearchRun => {
                self.begin_pending_search(self.dialog_scope(), SearchOperation::Find)
            }
            SearchMessage::AdvancedCountRun => {
                self.begin_pending_search(self.dialog_scope(), SearchOperation::Count)
            }
            SearchMessage::AdvancedFindNextRun => {
                self.advanced_find_next();
                Task::none()
            }
            SearchMessage::AdvancedFindAllCurrentRun => {
                self.begin_pending_search(SearchScope::Current, SearchOperation::Find)
            }
            SearchMessage::AdvancedFindAllOpenRun => {
                self.begin_pending_search(SearchScope::OpenDocuments, SearchOperation::Find)
            }
            SearchMessage::AdvancedReplaceRun => self.advanced_replace_current(),
            SearchMessage::AdvancedReplaceAllRun => self.advanced_replace_all(),
            SearchMessage::AdvancedReplaceAllCurrentRun => {
                self.replace_all_in(SearchScope::Current)
            }
            SearchMessage::AdvancedReplaceAllOpenRun => {
                self.replace_all_in(SearchScope::OpenDocuments)
            }
            SearchMessage::AdvancedSearchResultSelected(document_id, selection) => {
                if self.workspace.document(document_id).is_some() {
                    let auto_save = self.auto_save_before_switch(Some(document_id));
                    if !self.workspace.select(document_id) {
                        return Task::none();
                    }
                    self.refresh_find_matches();
                    let editor_task = self.update_editor(
                        document_id,
                        crate::editor::EditorAction::SelectRegion(selection),
                    );
                    self.reveal_document_position(
                        document_id,
                        selection.range().normalized().start,
                    );
                    return Task::batch([auto_save, editor_task]);
                }
                Task::none()
            }
            SearchMessage::AdvancedSearchClosed => self.close_advanced_search_window(),
        }
    }

    fn toggle_find_panel(&mut self) -> Task<Message> {
        self.menu.close();
        self.is_find_visible = !self.is_find_visible;
        self.chrome_animation.find.set_visible(self.is_find_visible);

        if self.is_find_visible {
            self.chrome_animation
                .inline_replace
                .set_visible(self.is_inline_replace_visible);
            operation::focus(FIND_INPUT_ID)
        } else {
            operation::focus(crate::ui::editor::EDITOR_ID)
        }
    }

    fn toggle_advanced_search_window(&mut self, tab: AdvancedSearchTab) -> Task<Message> {
        self.menu.close();
        self.search_dialog.set_active_tab(tab);
        self.search_dialog.query = self.find.query.clone();
        self.search_dialog.replacement = self.find.replacement.clone();
        self.search_dialog.case_sensitive = self.find.case_sensitive;
        self.search_dialog.whole_word = self.find.whole_word;
        self.search_dialog.mode = crate::core::SearchMode::Normal;
        self.refresh_search_results();
        self.open_advanced_search_window()
            .chain(operation::focus(QUERY_INPUT_ID))
    }

    fn replace_current(&mut self) -> Task<Message> {
        let Some(document) = self.workspace.active_document() else {
            return Task::none();
        };
        if !document.has_complete_text_index() {
            return Task::none();
        }
        let text = document.text();

        let Some(text_match) = self.find.current() else {
            return Task::none();
        };

        if self.replace_active_match(&text, text_match.start, text_match.end) {
            self.refresh_find_matches();
        }

        Task::none()
    }

    fn select_text_for_find(&mut self, persist_query: bool, forward: bool) {
        self.menu.close();
        let Some(query) = self.active_find_text() else {
            return;
        };

        let previous_find = (!persist_query).then(|| {
            (
                std::mem::take(&mut self.find.query),
                self.find.case_sensitive,
                self.find.whole_word,
            )
        });

        self.find.set_query(query);
        if !persist_query {
            self.find.set_case_sensitive(false);
            self.find.set_whole_word(false);
        }
        self.refresh_find_matches();

        let text_match = if forward {
            self.find.next()
        } else {
            self.find.previous()
        };
        self.select_active_match(text_match);

        if let Some((query, case_sensitive, whole_word)) = previous_find {
            self.find.query = query;
            self.find.case_sensitive = case_sensitive;
            self.find.whole_word = whole_word;
            self.refresh_find_matches();
        }
    }

    fn active_find_text(&self) -> Option<String> {
        let document = self.workspace.active_document()?;
        let range = document
            .buffer
            .clamp_range(document.main_selection().range());

        if !range.is_empty() {
            return Some(document.buffer.slice_text(range));
        }

        let range = word_range_at_position(&document.buffer, range.start, &document.syntax_token)?;

        Some(document.buffer.slice_text(range))
    }

    fn select_active_match(&mut self, text_match: Option<crate::core::TextMatch>) {
        let Some(text_match) = text_match else {
            return;
        };
        let Some(document) = self.workspace.active_document() else {
            return;
        };
        let Some(start_position) = document.buffer.position_for_byte_offset(text_match.start)
        else {
            return;
        };
        let Some(end_position) = document.buffer.position_for_byte_offset(text_match.end) else {
            return;
        };
        let document_id = self.workspace.active_document_id();

        let _ = self.update_editor(
            document_id,
            crate::editor::EditorAction::SelectRegion(EditorSelection::new(
                start_position,
                end_position,
            )),
        );
        self.reveal_document_position(document_id, start_position);
    }

    fn replace_all(&mut self) -> Task<Message> {
        let Some(document) = self.workspace.active_document() else {
            return Task::none();
        };
        if !document.has_complete_text_index() {
            return Task::none();
        }
        let text = document.text();

        let matches = crate::core::search::compute_matches_with_options(
            &text,
            &self.find.query,
            crate::core::SearchOptions::normal(self.find.case_sensitive, self.find.whole_word),
        );

        if matches.is_empty() {
            self.find.refresh_matches(&text);
            return Task::none();
        }

        let replacements = matches
            .into_iter()
            .filter_map(|found| {
                Some((
                    EditorRange::new(
                        document.buffer.position_for_byte_offset(found.start)?,
                        document.buffer.position_for_byte_offset(found.end)?,
                    ),
                    self.find.replacement.clone(),
                ))
            })
            .collect();
        let document = self
            .workspace
            .active_document_mut()
            .expect("active document");
        let changed = super::editor_ops::replace_ranges_for_search(document, replacements);
        if changed {
            document.ensure_caret_visible();
        }

        self.refresh_find_matches();
        Task::none()
    }

    fn replace_active_match(&mut self, text: &str, start: usize, end: usize) -> bool {
        let Some(start_position) = position_for_byte_offset(text, start) else {
            return false;
        };
        let Some(end_position) = position_for_byte_offset(text, end) else {
            return false;
        };

        self.replace_active_document_range(
            start_position,
            end_position,
            self.find.replacement.clone(),
        )
    }

    fn advanced_replace_current(&mut self) -> Task<Message> {
        let Some(document) = self.workspace.active_document() else {
            return Task::none();
        };
        if !document.has_complete_text_index() {
            return Task::none();
        }
        let text = document.text();
        let Some(search) = self.prepare_advanced_search() else {
            return Task::none();
        };
        let matches = search.matches(&text);
        let Some(text_match) = current_selection_match(self.workspace.active_document(), &matches)
            .or_else(|| matches.first().copied())
        else {
            self.refresh_search_results();
            return Task::none();
        };

        let replacement =
            search.replacement_for_match(&text, text_match, &self.search_dialog.replacement);
        if self.replace_active_range_with(&text, text_match.start, text_match.end, replacement) {
            self.refresh_search_results();
        }

        Task::none()
    }

    fn advanced_replace_all(&mut self) -> Task<Message> {
        let scope = if matches!(
            self.search_dialog.active_tab,
            AdvancedSearchTab::FindInFiles | AdvancedSearchTab::ReplaceInFiles
        ) {
            SearchScope::OpenDocuments
        } else {
            SearchScope::Current
        };

        self.replace_all_in(scope)
    }

    fn replace_all_in(&mut self, scope: SearchScope) -> Task<Message> {
        self.begin_pending_search(scope, SearchOperation::Replace)
    }

    fn dialog_scope(&self) -> SearchScope {
        if matches!(
            self.search_dialog.active_tab,
            AdvancedSearchTab::FindInFiles | AdvancedSearchTab::ReplaceInFiles
        ) {
            SearchScope::OpenDocuments
        } else {
            SearchScope::Current
        }
    }

    fn begin_pending_search(
        &mut self,
        scope: SearchScope,
        operation: SearchOperation,
    ) -> Task<Message> {
        self.pending_search = None;
        if operation == SearchOperation::Find
            && let Err(error) = self.search_dialog.parsed_result_settings()
        {
            self.search_dialog.result_options_visible = true;
            self.search_dialog.status = error.into();
            return Task::none();
        }
        let Some(search) = self.prepare_advanced_search() else {
            return Task::none();
        };
        let documents = self.document_ids_for_scope(scope);
        let deferred = documents
            .iter()
            .copied()
            .filter(|id| {
                self.workspace.document(*id).is_some_and(|document| {
                    matches!(
                        document.load_state,
                        crate::core::document::DocumentLoadState::Deferred { .. }
                    )
                })
            })
            .collect::<Vec<_>>();
        self.pending_search = Some(PendingSearch {
            dialog: self.search_dialog.request_snapshot(),
            search,
            documents,
            operation,
        });
        let tasks = deferred
            .into_iter()
            .map(|id| self.activate_document(id))
            .collect::<Vec<_>>();
        self.events.publish(super::events::Event::SearchRequested);
        Task::batch(tasks)
    }

    fn refresh_search_results(&mut self) {
        if matches!(
            self.search_dialog.active_tab,
            AdvancedSearchTab::FindInFiles | AdvancedSearchTab::ReplaceInFiles
        ) {
            self.search_dialog.refresh_from_workspace(&self.workspace);
            return;
        }

        let Some(document) = self.workspace.active_document() else {
            self.search_dialog.clear_results();
            self.search_dialog.status = String::from("No document");
            return;
        };

        self.search_dialog.refresh_from_documents([document]);
    }

    fn advanced_find_next(&mut self) {
        let Some(search) = self.prepare_advanced_search() else {
            return;
        };
        if self.find.query != self.search_dialog.query {
            self.find.set_query(self.search_dialog.query.clone());
        }
        self.find
            .set_case_sensitive(self.search_dialog.case_sensitive);
        self.find.set_whole_word(self.search_dialog.whole_word);
        let Some(document) = self.workspace.active_document() else {
            self.search_dialog.clear_results();
            self.search_dialog.status = String::from("No document");
            return;
        };
        if !document.has_complete_text_index() {
            self.search_dialog.refresh_from_documents([document]);
            return;
        }
        let text = document.text();
        let matches = search.matches(&text);

        let text_match = next_match_after_selection(self.workspace.active_document(), &matches)
            .or_else(|| {
                self.search_dialog
                    .wrap_around
                    .then(|| matches.first().copied())
                    .flatten()
            });
        self.select_active_match(text_match);
    }

    fn prepare_advanced_search(&mut self) -> Option<PreparedSearch> {
        match PreparedSearch::new(&self.search_dialog.query, self.search_dialog.options()) {
            Ok(Some(search)) => Some(search),
            Ok(None) => {
                self.search_dialog.clear_results();
                self.search_dialog.status = String::from("No query");
                None
            }
            Err(error) => {
                self.search_dialog.clear_results();
                self.search_dialog.status = crate::search_dialog::search_error_status(error);
                None
            }
        }
    }

    fn document_ids_for_scope(&self, scope: SearchScope) -> Vec<crate::core::DocumentId> {
        match scope {
            SearchScope::Current => vec![self.workspace.active_document_id()],
            SearchScope::OpenDocuments => self
                .workspace
                .documents()
                .iter()
                .filter(|document| {
                    crate::search_dialog::include_filter_matches(
                        document,
                        &self.search_dialog.include_pattern,
                    )
                })
                .map(|document| document.id)
                .collect(),
        }
    }

    fn replace_active_range_with(
        &mut self,
        text: &str,
        start: usize,
        end: usize,
        replacement: String,
    ) -> bool {
        self.replace_document_range_with(
            self.workspace.active_document_id(),
            text,
            start,
            end,
            replacement,
        )
    }

    fn replace_document_range_with(
        &mut self,
        document_id: crate::core::DocumentId,
        text: &str,
        start: usize,
        end: usize,
        replacement: String,
    ) -> bool {
        let Some(start_position) = position_for_byte_offset(text, start) else {
            return false;
        };
        let Some(end_position) = position_for_byte_offset(text, end) else {
            return false;
        };

        let Some(document) = self.workspace.document_mut(document_id) else {
            return false;
        };

        document.set_main_selection(EditorSelection::new(start_position, end_position));
        let changed = super::editor_ops::replace_selection_for_search(document, &replacement);

        if changed && document_id == self.workspace.active_document_id() {
            self.refresh_find_matches();
        }

        changed
    }

    fn reveal_document_position(
        &mut self,
        document_id: crate::core::DocumentId,
        position: crate::editor::EditorPosition,
    ) {
        let Some(document) = self.workspace.document_mut(document_id) else {
            return;
        };
        document.reveal_position(position);
    }
}

#[derive(Debug, Clone, Copy)]
enum SearchScope {
    Current,
    OpenDocuments,
}

fn current_selection_match(
    document: Option<&Document>,
    matches: &[crate::core::TextMatch],
) -> Option<crate::core::TextMatch> {
    let document = document?;
    let range = document.main_selection().range();
    let start = document.buffer.byte_offset(range.start);
    let end = document.buffer.byte_offset(range.end);

    matches
        .iter()
        .copied()
        .find(|text_match| start == text_match.start && end == text_match.end)
}

fn next_match_after_selection(
    document: Option<&Document>,
    matches: &[crate::core::TextMatch],
) -> Option<crate::core::TextMatch> {
    let document = document?;
    let cursor = document
        .buffer
        .byte_offset(document.main_selection().range().normalized().end);

    matches
        .iter()
        .copied()
        .find(|text_match| text_match.start >= cursor)
}

impl App {
    pub(super) fn refresh_find_matches(&mut self) {
        refresh_matches(&mut self.find, &self.workspace);
    }

    fn persist_search_result_options(&mut self) -> Task<Message> {
        let Ok(settings) = self.search_dialog.parsed_result_settings() else {
            return Task::none();
        };
        self.search_dialog.result_settings = settings;
        self.set_search_result_settings(settings)
    }
}

/// Search reactions cannot access file, session, settings, or window state.
pub(super) struct SearchSubscriber<'a> {
    pub(super) workspace: &'a mut crate::core::Workspace,
    pub(super) pending: &'a mut Option<PendingSearch>,
    pub(super) find: &'a mut crate::core::FindState,
    pub(super) dialog: &'a mut crate::search_dialog::SearchDialogState,
}
impl SearchSubscriber<'_> {
    pub(super) fn resume(&mut self) -> Task<Message> {
        let Some(pending) = self.pending.as_ref() else {
            return Task::none();
        };
        if pending.documents.iter().any(|id| {
            self.workspace.document(*id).is_none_or(|document| {
                matches!(
                    document.load_state,
                    crate::core::document::DocumentLoadState::Failed { .. }
                )
            })
        }) {
            *self.pending = None;
            self.dialog.clear_results();
            self.dialog.status = String::from(
                "Search canceled: a target document was closed or could not be loaded. No replacements were made.",
            );
            return Task::none();
        }
        let waiting = pending
            .documents
            .iter()
            .filter(|id| {
                self.workspace
                    .document(**id)
                    .is_some_and(|document| !document.has_complete_text_index())
            })
            .count();
        if waiting > 0 {
            self.dialog.clear_results();
            self.dialog.status = format!("Loading {waiting} documents for search...");
            return Task::none();
        }
        let pending = self.pending.take().expect("ready search");
        let active_id = self.workspace.active_document_id();
        if pending.operation == SearchOperation::Replace {
            for document_id in &pending.documents {
                let Some(document) = self.workspace.document_mut(*document_id) else {
                    continue;
                };
                if !document.has_complete_text_index() {
                    continue;
                }
                let text = document.text();
                let matches = pending.search.matches(&text);
                let replacements = matches
                    .into_iter()
                    .filter_map(|found| {
                        Some((
                            EditorRange::new(
                                document.buffer.position_for_byte_offset(found.start)?,
                                document.buffer.position_for_byte_offset(found.end)?,
                            ),
                            pending.search.replacement_for_match(
                                &text,
                                found,
                                &pending.dialog.replacement,
                            ),
                        ))
                    })
                    .collect();
                let document_changed =
                    super::editor_ops::replace_ranges_for_search(document, replacements);

                if document_changed && *document_id == active_id {
                    document.ensure_caret_visible();
                }
            }
        }
        if pending.operation == SearchOperation::Replace {
            refresh_matches(self.find, self.workspace);
        }
        let mut completed = pending.dialog;
        let documents = pending
            .documents
            .iter()
            .filter_map(|id| self.workspace.document(*id));
        if pending.operation == SearchOperation::Count {
            completed.count_from_documents(documents);
        } else {
            completed.refresh_from_documents(documents);
        }
        self.dialog.results = completed.results;
        self.dialog.match_count = completed.match_count;
        self.dialog.status = completed.status;

        Task::none()
    }
}

pub(super) fn refresh_matches(
    find: &mut crate::core::FindState,
    workspace: &crate::core::Workspace,
) {
    if find.query.is_empty() {
        find.refresh_matches("");
        return;
    }
    if let Some(document) = workspace.active_document() {
        find.refresh_matches_in_chunks(document.buffer.chunks());
    } else {
        find.refresh_matches("");
    }
}

pub(super) fn observe(
    event: super::events::Event,
    active: crate::core::DocumentId,
    work: &mut super::events::PendingWork,
) {
    use super::events::{Event, Work};
    use crate::core::workspace::changes::WorkspaceEvent as W;
    match event {
        Event::SearchRequested => work.request(Work::Search),
        Event::Started | Event::Workspace(W::ActiveDocumentChanged(_) | W::DocumentOpened(_)) => {
            work.request(Work::Find)
        }
        Event::Workspace(W::ContentChanged(id)) if id == active => work.request(Work::Find),
        Event::Workspace(W::PreviewChanged(id)) if id == active => work.request(Work::LoadingFind),
        Event::Workspace(W::LoadStateChanged(id)) => {
            work.request(Work::Search);
            if id == active {
                work.request(Work::Find);
            }
        }
        Event::Workspace(W::DocumentClosed(_)) => work.request(Work::Search),
        _ => {}
    }
}

pub(super) fn schedule_loading_find(
    find: &crate::core::FindState,
    scheduled: &mut bool,
) -> Task<Message> {
    if find.query.is_empty() || *scheduled {
        return Task::none();
    }
    *scheduled = true;
    Task::perform(
        async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        },
        |_| Message::RefreshLoadingFind,
    )
}
