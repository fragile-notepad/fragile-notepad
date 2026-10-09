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
            SearchMessage::AdvancedSearchPreviewDue(_)
                | SearchMessage::AdvancedSearchResultSelected(_, _)
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
        if !matches!(
            message,
            SearchMessage::AdvancedSearchPreviewDue(_)
                | SearchMessage::FindReplacementChanged(_)
                | SearchMessage::AdvancedSearchReplacementChanged(_)
                | SearchMessage::FindWrapAroundToggled(_)
                | SearchMessage::AdvancedSearchWrapAroundToggled(_)
                | SearchMessage::AdvancedResultOptionsToggled
                | SearchMessage::AdvancedResultLimitChanged(_)
                | SearchMessage::AdvancedPreviewCharsChanged(_)
                | SearchMessage::AdvancedPreviewContextChanged(_)
                | SearchMessage::AdvancedResultOptionsReset
                | SearchMessage::FindNext
                | SearchMessage::FindPrevious
                | SearchMessage::ReplaceCurrent
                | SearchMessage::ReplaceAll
                | SearchMessage::ToggleInlineReplace
                | SearchMessage::ShowInlineReplace
                | SearchMessage::ToggleFind
                | SearchMessage::HideFind
        ) {
            // A later edit, command, or close owns the status shown in the dialog.
            self.search_dialog.preview_generation =
                self.search_dialog.preview_generation.wrapping_add(1);
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
                self.sync_dialog_from_find();
                self.schedule_search_preview_if_open()
            }
            SearchMessage::FindReplacementChanged(replacement) => {
                self.find.set_replacement(replacement.clone());
                self.search_dialog.set_replacement(replacement);
                Task::none()
            }
            SearchMessage::FindCaseSensitiveToggled(case_sensitive) => {
                self.find.set_case_sensitive(case_sensitive);
                self.refresh_find_matches();
                self.sync_dialog_from_find();
                self.schedule_search_preview_if_open()
            }
            SearchMessage::FindWholeWordToggled(whole_word) => {
                self.find.set_whole_word(whole_word);
                self.refresh_find_matches();
                self.sync_dialog_from_find();
                self.schedule_search_preview_if_open()
            }
            SearchMessage::FindModeSelected(mode) => {
                self.find.set_mode(mode);
                self.refresh_find_matches();
                self.sync_dialog_from_find();
                self.schedule_search_preview_if_open()
            }
            SearchMessage::FindWrapAroundToggled(wrap_around) => {
                self.find.wrap_around = wrap_around;
                self.search_dialog.set_wrap_around(wrap_around);
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
                self.navigate_find(true);
                Task::none()
            }
            SearchMessage::FindPrevious => {
                self.menu.close();
                self.navigate_find(false);
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
                self.sync_find_request_from_dialog();
                self.schedule_search_preview()
            }
            SearchMessage::AdvancedSearchReplacementChanged(replacement) => {
                self.search_dialog.set_replacement(replacement.clone());
                self.find.set_replacement(replacement);
                Task::none()
            }
            SearchMessage::AdvancedSearchCaseSensitiveToggled(case_sensitive) => {
                self.search_dialog.set_case_sensitive(case_sensitive);
                self.sync_find_request_from_dialog();
                self.schedule_search_preview()
            }
            SearchMessage::AdvancedSearchWholeWordToggled(whole_word) => {
                self.search_dialog.set_whole_word(whole_word);
                self.sync_find_request_from_dialog();
                self.schedule_search_preview()
            }
            SearchMessage::AdvancedSearchWrapAroundToggled(wrap_around) => {
                self.search_dialog.set_wrap_around(wrap_around);
                self.find.wrap_around = wrap_around;
                Task::none()
            }
            SearchMessage::AdvancedSearchModeSelected(mode) => {
                self.search_dialog.set_mode(mode);
                self.sync_find_request_from_dialog();
                self.schedule_search_preview()
            }
            SearchMessage::AdvancedSearchIncludeChanged(include_pattern) => {
                self.search_dialog.set_include_pattern(include_pattern);
                self.schedule_search_preview()
            }
            SearchMessage::AdvancedSearchPreviewDue(generation) => {
                if generation == self.search_dialog.preview_generation
                    && self.pending_search.is_none()
                {
                    self.refresh_find_matches();
                    self.refresh_search_results();
                }
                Task::none()
            }
            SearchMessage::AdvancedSearchRun => {
                self.begin_pending_search(self.dialog_scope(), SearchOperation::Find)
            }
            SearchMessage::AdvancedCountRun => {
                self.begin_pending_search(self.dialog_scope(), SearchOperation::Count)
            }
            SearchMessage::AdvancedFindNextRun => self.advanced_find(true),
            SearchMessage::AdvancedFindPreviousRun => self.advanced_find(false),
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
                    self.sync_find_from_dialog();
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
                    self.search_dialog.selected_result =
                        self.search_dialog.results.iter().position(|result| {
                            result.document_id == document_id && result.selection == selection
                        });
                    self.synchronize_selected_find_match();
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
        self.sync_dialog_from_find();
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
        let Ok(Some(search)) = PreparedSearch::new(&self.find.query, self.find.options()) else {
            return Task::none();
        };
        let matches = search.matches(&text);
        let Some(text_match) = current_selection_match(Some(document), &matches).or_else(|| {
            match_for_navigation(Some(document), &matches, true, self.find.wrap_around, None)
        }) else {
            return Task::none();
        };
        let replacement = search.replacement_for_match(&text, text_match, &self.find.replacement);
        let replacement_end = text_match.start + replacement.len();
        self.replace_active_range_with(&text, text_match.start, text_match.end, replacement);
        self.select_match_after_replacement(replacement_end, text_match.is_empty());

        Task::none()
    }

    fn select_text_for_find(&mut self, persist_query: bool, forward: bool) {
        self.menu.close();
        let Some(query) = self.active_find_text() else {
            return;
        };

        let previous_find = (!persist_query).then(|| self.find.clone());

        self.find.set_query(query);
        if !persist_query {
            self.find.set_case_sensitive(false);
            self.find.set_whole_word(false);
        }
        // Selected editor text is always searched literally.
        self.find.set_mode(crate::core::SearchMode::Normal);
        self.refresh_find_matches();
        self.navigate_find(forward);

        if let Some(previous) = previous_find {
            self.find = previous;
            self.refresh_find_matches();
        } else {
            self.sync_dialog_from_find();
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
        self.find.current_match = self
            .find
            .matches
            .iter()
            .position(|found| *found == text_match);
        self.find.navigation_match = Some(text_match);

        let _ = self.update_editor(
            document_id,
            crate::editor::EditorAction::SelectRegion(EditorSelection::new(
                start_position,
                end_position,
            )),
        );
        self.reveal_document_position(document_id, start_position);
        self.search_dialog.selected_result = self.search_dialog.results.iter().position(|result| {
            result.document_id == document_id
                && result.selection == EditorSelection::new(start_position, end_position)
        });
    }

    fn replace_all(&mut self) -> Task<Message> {
        let Some(document) = self.workspace.active_document() else {
            return Task::none();
        };
        if !document.has_complete_text_index() {
            return Task::none();
        }
        let text = document.text();

        let Ok(Some(search)) = PreparedSearch::new(&self.find.query, self.find.options()) else {
            return Task::none();
        };
        let matches = search.matches(&text);

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
                    search.replacement_for_match(&text, found, &self.find.replacement),
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
            .or_else(|| {
                match_for_navigation(
                    self.workspace.active_document(),
                    &matches,
                    true,
                    self.search_dialog.wrap_around,
                    None,
                )
            })
        else {
            self.refresh_search_results();
            return Task::none();
        };

        let replacement =
            search.replacement_for_match(&text, text_match, &self.search_dialog.replacement);
        let replacement_end = text_match.start + replacement.len();
        if self.replace_active_range_with(&text, text_match.start, text_match.end, replacement) {
            self.refresh_search_results();
        }
        self.select_match_after_replacement(replacement_end, text_match.is_empty());
        self.search_dialog.status = String::from("Replaced 1 match");

        Task::none()
    }

    fn advanced_replace_all(&mut self) -> Task<Message> {
        self.replace_all_in(self.dialog_scope())
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
        self.search_dialog.count_summary = None;
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

    fn advanced_find(&mut self, forward: bool) -> Task<Message> {
        let Some(_search) = self.prepare_advanced_search() else {
            return Task::none();
        };
        if matches!(self.dialog_scope(), SearchScope::OpenDocuments) {
            return self.advanced_find_in_open_documents(forward);
        }
        let Some(document) = self.workspace.active_document() else {
            self.search_dialog.clear_results();
            self.search_dialog.status = String::from("No document");
            return Task::none();
        };
        if !document.has_complete_text_index() {
            self.search_dialog.refresh_from_documents([document]);
            return Task::none();
        }
        let matches = self.find.matches.clone();

        let text_match = match_for_navigation(
            self.workspace.active_document(),
            &matches,
            forward,
            self.search_dialog.wrap_around,
            self.find.navigation_match,
        );
        self.search_dialog.status = if let Some(found) = text_match {
            let index = matches
                .iter()
                .position(|candidate| *candidate == found)
                .unwrap_or(0);
            format!(
                "{} of {}{} matches",
                index + 1,
                matches.len(),
                if self.find.matches_limited { "+" } else { "" }
            )
        } else if matches.is_empty() {
            String::from("No matches")
        } else if forward {
            String::from("Reached the last match")
        } else {
            String::from("Reached the first match")
        };
        self.select_active_match(text_match);
        Task::none()
    }

    fn prepare_advanced_search(&mut self) -> Option<PreparedSearch> {
        self.sync_find_from_dialog();
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

    fn advanced_find_in_open_documents(&mut self, forward: bool) -> Task<Message> {
        if self.search_results_are_stale() || self.search_dialog.results.is_empty() {
            self.refresh_search_results();
        }
        let active = self.workspace.active_document_id();
        let selection = self
            .workspace
            .active_document()
            .map(Document::main_selection);
        let selected = self.search_dialog.results.iter().position(|result| {
            result.document_id == active
                && Some(result.selection) == selection
                && (!result.selection.is_caret()
                    || self.workspace.document(active).is_some_and(|document| {
                        let range = result.selection.range();
                        self.find.navigation_match
                            == Some(crate::core::TextMatch::new(
                                document.buffer.byte_offset(range.start),
                                document.buffer.byte_offset(range.end),
                            ))
                    }))
        });
        let documents = self.workspace.documents();
        let active_order = documents
            .iter()
            .position(|document| document.id == active)
            .unwrap_or(0);
        let eligible = |index: usize, result: &crate::search_dialog::SearchResult| {
            if Some(index) == selected {
                return false;
            }
            if result.document_id == active {
                return selection.is_none_or(|selection| {
                    if forward {
                        result.selection.range().start >= selection.range().end
                    } else {
                        result.selection.range().end <= selection.range().start
                    }
                });
            }
            let order = documents
                .iter()
                .position(|document| document.id == result.document_id);
            order.is_some_and(|order| {
                if forward {
                    order > active_order
                } else {
                    order < active_order
                }
            })
        };
        let results = &self.search_dialog.results;
        let index = if forward {
            results
                .iter()
                .enumerate()
                .find(|(index, result)| eligible(*index, result))
                .map(|(index, _)| index)
                .or_else(|| (self.search_dialog.wrap_around && !results.is_empty()).then_some(0))
        } else {
            results
                .iter()
                .enumerate()
                .rev()
                .find(|(index, result)| eligible(*index, result))
                .map(|(index, _)| index)
                .or_else(|| {
                    self.search_dialog
                        .wrap_around
                        .then(|| results.len().checked_sub(1))
                        .flatten()
                })
        };
        let Some(index) = index else {
            self.search_dialog.status = if results.is_empty() {
                String::from("No matches")
            } else if forward {
                String::from("Reached the last match")
            } else {
                String::from("Reached the first match")
            };
            return Task::none();
        };
        let result = results[index].clone();
        self.search_dialog.status = format!("{} of {} displayed matches", index + 1, results.len());
        self.update_search(SearchMessage::AdvancedSearchResultSelected(
            result.document_id,
            result.selection,
        ))
    }

    fn scoped_search_documents(&self) -> Vec<crate::search_dialog::SearchDocumentSnapshot> {
        self.document_ids_for_scope(self.dialog_scope())
            .into_iter()
            .filter_map(|id| self.workspace.document(id))
            .map(crate::search_dialog::SearchDocumentSnapshot::new)
            .collect()
    }

    fn search_results_are_stale(&self) -> bool {
        self.search_dialog.result_documents != self.scoped_search_documents()
    }

    pub(super) fn refresh_workspace_search_preview(&mut self) -> Task<Message> {
        if self.advanced_search_window.is_none()
            || self.pending_search.is_some()
            || self.search_dialog.query.is_empty()
            || PreparedSearch::new(&self.search_dialog.query, self.search_dialog.options()).is_err()
            || !self.search_results_are_stale()
        {
            return Task::none();
        }
        self.search_dialog.preview_generation =
            self.search_dialog.preview_generation.wrapping_add(1);
        self.search_dialog.clear_results();
        self.schedule_search_preview()
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

    fn sync_find_from_dialog(&mut self) {
        let matching_changed = self.sync_find_request_from_dialog();
        if matching_changed
            || self.find.matches_stale
            || self.find.match_limit
                != Some(self.search_dialog.result_settings.normalized().result_limit)
        {
            self.refresh_find_matches();
        }
    }

    fn sync_find_request_from_dialog(&mut self) -> bool {
        let matching_changed = self.find.query != self.search_dialog.query
            || self.find.options() != self.search_dialog.options();
        if self.find.query != self.search_dialog.query {
            self.find.set_query(self.search_dialog.query.clone());
        }
        self.find
            .set_replacement(self.search_dialog.replacement.clone());
        self.find
            .set_case_sensitive(self.search_dialog.case_sensitive);
        self.find.set_whole_word(self.search_dialog.whole_word);
        self.find.set_mode(self.search_dialog.mode);
        self.find.wrap_around = self.search_dialog.wrap_around;
        matching_changed
    }

    fn sync_dialog_from_find(&mut self) {
        if self.search_dialog.query != self.find.query {
            self.search_dialog.set_query(self.find.query.clone());
        }
        self.search_dialog
            .set_replacement(self.find.replacement.clone());
        if self.search_dialog.case_sensitive != self.find.case_sensitive {
            self.search_dialog
                .set_case_sensitive(self.find.case_sensitive);
        }
        if self.search_dialog.whole_word != self.find.whole_word {
            self.search_dialog.set_whole_word(self.find.whole_word);
        }
        if self.search_dialog.mode != self.find.mode {
            self.search_dialog.set_mode(self.find.mode);
        }
        self.search_dialog.set_wrap_around(self.find.wrap_around);
    }

    fn schedule_search_preview_if_open(&mut self) -> Task<Message> {
        if self.advanced_search_window.is_some() {
            self.schedule_search_preview()
        } else {
            Task::none()
        }
    }

    fn schedule_search_preview(&mut self) -> Task<Message> {
        if !matches!(
            PreparedSearch::new(&self.search_dialog.query, self.search_dialog.options()),
            Ok(Some(_))
        ) {
            return Task::none();
        }
        self.search_dialog.status = String::from("Searching…");
        let generation = self.search_dialog.preview_generation;
        Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(140)).await;
                generation
            },
            Message::AdvancedSearchPreviewDue,
        )
    }

    fn navigate_find(&mut self, forward: bool) {
        if self.find.matches_stale
            || self.find.match_limit
                != Some(self.search_dialog.result_settings.normalized().result_limit)
        {
            self.refresh_find_matches();
        }
        let found = match_for_navigation(
            self.workspace.active_document(),
            &self.find.matches,
            forward,
            self.find.wrap_around,
            self.find.navigation_match,
        );
        self.select_active_match(found);
    }

    fn select_match_after_replacement(&mut self, end: usize, skip_empty: bool) {
        let found = self
            .find
            .matches
            .iter()
            .copied()
            .find(|found| {
                found.start >= end && !(skip_empty && found.is_empty() && found.start == end)
            })
            .or_else(|| {
                self.find
                    .wrap_around
                    .then(|| self.find.matches.first().copied())
                    .flatten()
            });
        self.find.navigation_match = None;
        self.select_active_match(found);
    }

    fn synchronize_selected_find_match(&mut self) {
        if let Some(found) =
            current_selection_match(self.workspace.active_document(), &self.find.matches)
        {
            self.find.current_match = self
                .find
                .matches
                .iter()
                .position(|candidate| *candidate == found);
            self.find.navigation_match = Some(found);
        }
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
        .binary_search_by_key(&(start, end), |found| (found.start, found.end))
        .ok()
        .map(|index| matches[index])
}

fn match_for_navigation(
    document: Option<&Document>,
    matches: &[crate::core::TextMatch],
    forward: bool,
    wrap_around: bool,
    navigation_match: Option<crate::core::TextMatch>,
) -> Option<crate::core::TextMatch> {
    let document = document?;
    let range = document.main_selection().range().normalized();
    let start = document.buffer.byte_offset(range.start);
    let end = document.buffer.byte_offset(range.end);
    // An empty caret is only an already-visited regex match when navigation
    // selected it. This lets the first Enter find an anchor at the caret.
    let selected = current_selection_match(Some(document), matches)
        .filter(|found| !found.is_empty() || navigation_match == Some(*found));
    if forward {
        let mut index = matches.partition_point(|found| found.start < end);
        if matches
            .get(index)
            .copied()
            .is_some_and(|found| Some(found) == selected)
        {
            index += 1;
        }
        matches
            .get(index)
            .copied()
            .or_else(|| wrap_around.then(|| matches.first().copied()).flatten())
    } else {
        let mut limit = matches.partition_point(|found| found.end <= start);
        if limit > 0
            && matches
                .get(limit - 1)
                .copied()
                .is_some_and(|found| Some(found) == selected)
        {
            limit -= 1;
        }
        limit
            .checked_sub(1)
            .and_then(|index| matches.get(index))
            .copied()
            .or_else(|| wrap_around.then(|| matches.last().copied()).flatten())
    }
}

impl App {
    pub(super) fn refresh_find_matches(&mut self) {
        refresh_matches(
            &mut self.find,
            &self.workspace,
            self.search_dialog.result_settings.normalized().result_limit,
        );
    }

    fn persist_search_result_options(&mut self) -> Task<Message> {
        let Ok(settings) = self.search_dialog.parsed_result_settings() else {
            return Task::none();
        };
        self.search_dialog.result_settings = settings;
        let persist = self.set_search_result_settings(settings);
        Task::batch([persist, self.search_result_settings_changed()])
    }

    pub(super) fn search_result_settings_changed(&mut self) -> Task<Message> {
        let settings = self.search_dialog.result_settings;
        if self.find.match_limit != Some(settings.result_limit) {
            self.refresh_find_matches();
        }
        if let Some(pending) = self.pending_search.as_mut() {
            if pending.operation != SearchOperation::Count {
                // Display preferences can change while loading; mutation inputs
                // and the captured scope remain owned by the original request.
                pending.dialog.set_result_settings(settings);
            }
            return Task::none();
        }
        if self.advanced_search_window.is_some() && self.search_dialog.count_summary.is_none() {
            self.search_dialog.preview_generation =
                self.search_dialog.preview_generation.wrapping_add(1);
            self.search_dialog.clear_results();
            self.schedule_search_preview()
        } else {
            Task::none()
        }
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
        let mut replacement_count = 0;
        let mut replaced_documents = 0;
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
                    .collect::<Vec<_>>();
                let count = replacements.len();
                let document_changed =
                    super::editor_ops::replace_ranges_for_search(document, replacements);
                if document_changed {
                    replacement_count += count;
                    replaced_documents += 1;
                }
                if document_changed && *document_id == active_id {
                    document.ensure_caret_visible();
                }
            }
        }
        if pending.operation == SearchOperation::Replace {
            refresh_matches(
                self.find,
                self.workspace,
                self.dialog.result_settings.normalized().result_limit,
            );
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
        if pending.operation == SearchOperation::Replace {
            completed.status = match (replacement_count, replaced_documents) {
                (0, _) => String::from("No matches replaced"),
                (1, 1) => String::from("Replaced 1 match"),
                (count, 1) => format!("Replaced {count} matches"),
                (count, documents) => format!("Replaced {count} matches in {documents} documents"),
            };
        }
        self.dialog.results = completed.results;
        self.dialog.selected_result = completed.selected_result;
        self.dialog.match_count = completed.match_count;
        self.dialog.count_summary = completed.count_summary;
        self.dialog.matches_limited = completed.matches_limited;
        self.dialog.status = completed.status;
        self.dialog.result_documents = completed.result_documents;

        Task::none()
    }
}

pub(super) fn refresh_matches(
    find: &mut crate::core::FindState,
    workspace: &crate::core::Workspace,
    match_limit: usize,
) {
    find.match_limit = Some(match_limit);
    let context = Some(workspace.active_document_id().get());
    if find.match_context != context {
        find.navigation_match = None;
        find.current_match = None;
        find.match_context = context;
    }
    if find.query.is_empty() {
        find.refresh_matches("");
        return;
    }
    if let Some(document) = workspace.active_document() {
        find.refresh_matches_in_chunks(document.buffer.chunks());
        if let Some(found) = current_selection_match(Some(document), &find.matches)
            .filter(|found| !found.is_empty() || find.navigation_match == Some(*found))
        {
            find.current_match = find
                .matches
                .iter()
                .position(|candidate| *candidate == found);
        }
    } else {
        find.refresh_matches("");
    }
}

/// Updates the counter after caret motion without scanning document text again.
pub(super) fn synchronize_selection(
    find: &mut crate::core::FindState,
    workspace: &crate::core::Workspace,
) {
    let selected = current_selection_match(workspace.active_document(), &find.matches);
    find.navigation_match = find
        .navigation_match
        .filter(|found| selected == Some(*found));
    find.current_match = selected
        .filter(|found| !found.is_empty() || find.navigation_match == Some(*found))
        .and_then(|found| {
            find.matches
                .binary_search_by_key(&(found.start, found.end), |candidate| {
                    (candidate.start, candidate.end)
                })
                .ok()
        });
}

pub(super) fn observe(
    event: super::events::Event,
    active: crate::core::DocumentId,
    work: &mut super::events::PendingWork,
) {
    use super::events::{Event, Work};
    use crate::core::workspace::changes::WorkspaceEvent as W;
    if matches!(
        event,
        Event::Workspace(
            W::DocumentOpened(_)
                | W::DocumentClosed(_)
                | W::ActiveDocumentChanged(_)
                | W::ContentChanged(_)
                | W::PreviewChanged(_)
                | W::LoadStateChanged(_)
                | W::MetadataChanged(_)
                | W::OrderChanged
        )
    ) {
        work.request(Work::SearchPreview);
    }
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
        Event::Workspace(W::ViewChanged(id)) if id == active => work.request(Work::FindSelection),
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
