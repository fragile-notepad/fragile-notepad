use crate::message::HistoryMessage;
use iced::{Task, clipboard};

use super::App;
use crate::core::DocumentId;
use crate::editor::{
    CaretMotion, EditorAction, EditorPosition, EditorSelection, FunctionEntry, document_end,
};
use crate::message::{ClipboardReadResult, Message, PasteRequest};

use super::editor_ops::folds::{
    set_all_folds_collapsed, set_current_fold_collapsed, toggle_current_fold, toggle_fold,
};
use super::editor_ops::navigation::{
    go_to_matching_delimiter, go_to_next_function, go_to_previous_function, move_document_position,
    select_current_function, select_current_function_body, select_delimiter_in_place,
    select_matching_delimiter, select_word_at,
};
use super::editor_ops::{
    add_adjacent_caret, backspace, convert_selection_to_rectangle, delete, delete_line,
    duplicate_line, indent, join_lines, line_span_text, lowercase_selection, paste_clipboard_mode,
    paste_selection, replace_selection, selected_text, selection_set_is_all_carets,
    split_selection_into_lines, trim_trailing_spaces, unindent, uppercase_selection,
};

impl App {
    pub(super) fn update_editor(
        &mut self,
        document_id: DocumentId,
        action: EditorAction,
    ) -> Task<Message> {
        if action == EditorAction::Focus {
            return if document_id == self.workspace.active_document_id() {
                iced::widget::operation::focus(crate::ui::editor::EDITOR_ID)
            } else {
                Task::none()
            };
        }
        if self.editor_action_blocked_while_indexing(document_id, &action) {
            self.file_status = Some(String::from("Finish loading before editing."));
            return Task::none();
        }

        let clipboard_task = self.clipboard_task(document_id, &action);
        let entries = matches!(
            action,
            EditorAction::NextFunction
                | EditorAction::PreviousFunction
                | EditorAction::SelectCurrentFunction
                | EditorAction::SelectCurrentFunctionBody
        )
        .then(|| self.outline_entries_for_editor_action(document_id))
        .flatten();
        let reveal_caret = action.mutates_document()
            || matches!(action, EditorAction::MoveCaret(_) | EditorAction::Select(_));
        self.apply_editor_action(document_id, action, entries.as_deref());
        if reveal_caret && let Some(document) = self.workspace.document_mut(document_id) {
            document.ensure_caret_visible();
        }

        clipboard_task
    }

    pub(super) fn update_clipboard_read(
        &mut self,
        request: PasteRequest,
        result: ClipboardReadResult,
    ) -> Task<Message> {
        let Ok(text) = result else {
            return Task::none();
        };
        let document_id = request.document_id;
        if self
            .workspace
            .document(document_id)
            .is_some_and(|document| !document.has_complete_text_index())
        {
            self.file_status = Some(String::from("Finish loading before editing."));
            return Task::none();
        }

        let changed = self.apply_paste_request(&request, text.as_ref());
        if changed && let Some(document) = self.workspace.document_mut(document_id) {
            document.ensure_caret_visible();
        }

        Task::none()
    }

    pub(super) fn update_language(&mut self, syntax_token: String) -> Task<Message> {
        self.menu.close();
        if let Some(document) = self.workspace.active_document_mut() {
            document.set_syntax_token(syntax_token);
        }
        Task::none()
    }

    pub(super) fn update_editor_command(&mut self, message: HistoryMessage) -> Task<Message> {
        self.menu.close();

        if self
            .workspace
            .active_document()
            .is_some_and(|document| !document.has_complete_text_index())
        {
            self.file_status = Some(String::from("Finish loading before editing."));
            return Task::none();
        }

        let changed = match message {
            HistoryMessage::Undo => self
                .workspace
                .active_document_mut()
                .is_some_and(|document| document.undo()),
            HistoryMessage::Redo => self
                .workspace
                .active_document_mut()
                .is_some_and(|document| document.redo()),
        };

        if changed && let Some(document) = self.workspace.active_document_mut() {
            document.ensure_caret_visible();
        }

        iced::widget::operation::focus(crate::ui::editor::EDITOR_ID)
    }

    pub(super) fn replace_active_document_range(
        &mut self,
        start: EditorPosition,
        end: EditorPosition,
        replacement: String,
    ) -> bool {
        let Some(document) = self.workspace.active_document_mut() else {
            return false;
        };
        if !document.has_complete_text_index() {
            self.file_status = Some(String::from("Finish loading before editing."));
            return false;
        }

        document.set_main_selection(EditorSelection::new(start, end));
        replace_selection(
            document,
            &replacement,
            false,
            self.settings.indentation.width() as usize,
        )
    }

    fn cached_outline_entries(&self, document_id: DocumentId) -> Option<&[FunctionEntry]> {
        let document = self.workspace.document(document_id)?;
        self.outline_parsing.functions_for(document)
    }

    fn outline_entries_for_editor_action(
        &self,
        document_id: DocumentId,
    ) -> Option<Vec<FunctionEntry>> {
        let document = self.workspace.document(document_id)?;
        if !document.can_run_full_document_analysis() {
            return Some(Vec::new());
        }

        self.cached_outline_entries(document_id).map(Vec::from)
    }

    fn apply_editor_action(
        &mut self,
        document_id: DocumentId,
        action: EditorAction,
        outline_entries: Option<&[FunctionEntry]>,
    ) -> bool {
        let Some(document) = self.workspace.document_mut(document_id) else {
            return false;
        };
        document.sync_selection_mirror();
        let tab_width = self.settings.indentation.width() as usize;

        match action {
            EditorAction::InsertText(text) => replace_selection(document, &text, true, tab_width),
            EditorAction::InsertNewline => {
                let newline = document
                    .line_ending
                    .map(|ending| ending.as_str())
                    .unwrap_or("\n")
                    .to_owned();

                replace_selection(document, &newline, false, tab_width)
            }
            EditorAction::Backspace => backspace(document, tab_width),
            EditorAction::Delete => delete(document, tab_width),
            EditorAction::MoveCaret(motion) => {
                let selection = document.main_selection();
                let cursor = move_document_position(document, selection.cursor, motion);
                let cursor = document.buffer.clamp_position(cursor);
                document.set_main_selection(EditorSelection::new(cursor, cursor));
                false
            }
            EditorAction::Select(motion) => {
                let selection = document.main_selection();
                let cursor = move_document_position(document, selection.cursor, motion);
                document.set_main_selection(EditorSelection::new(selection.anchor, cursor));
                false
            }
            EditorAction::SelectAll => {
                let start = EditorPosition::new(0, 0);
                let end = document_end(&document.buffer);
                document.set_main_selection(EditorSelection::new(start, end));
                false
            }
            EditorAction::ReplaceSelection(text) => {
                replace_selection(document, &text, false, tab_width)
            }
            EditorAction::Indent => {
                let text = match self.settings.indentation {
                    crate::core::IndentationMode::Tabs => "\t".to_owned(),
                    crate::core::IndentationMode::Spaces(width) => " ".repeat(width as usize),
                };

                indent(document, tab_width, &text)
            }
            EditorAction::Unindent => {
                unindent(document, self.settings.indentation.width() as usize)
            }
            EditorAction::DuplicateLine => duplicate_line(document),
            EditorAction::DeleteLine => delete_line(document),
            EditorAction::CopyLine => false,
            EditorAction::CutLine => delete_line(document),
            EditorAction::Uppercase => uppercase_selection(document, tab_width),
            EditorAction::Lowercase => lowercase_selection(document, tab_width),
            EditorAction::TrimTrailingSpaces => trim_trailing_spaces(document, tab_width),
            EditorAction::JoinLines => join_lines(document),
            EditorAction::ScrollLines(lines) => {
                let max = document.viewport.visible_row_count().saturating_sub(1);
                let next = if lines.is_negative() {
                    document
                        .scroll
                        .first_visible_row
                        .saturating_sub(lines.unsigned_abs() as usize)
                } else {
                    document
                        .scroll
                        .first_visible_row
                        .saturating_add(lines as usize)
                };
                document.scroll.first_visible_row = next.min(max);
                false
            }
            EditorAction::ScrollToRow(row) => {
                let max = document.viewport.visible_row_count().saturating_sub(1);
                document.scroll.first_visible_row = row.min(max);
                false
            }
            EditorAction::ViewportChanged {
                visible_rows,
                text_width,
                character_width_milli,
                font_size_milli,
                hint_factor_milli,
            } => {
                document.update_viewport_geometry_with_typography(
                    visible_rows,
                    text_width as f32,
                    character_width_milli as f32 / 1000.0,
                    font_size_milli as f32 / 1000.0,
                    hint_factor_milli.map(|scale| scale as f32 / 1000.0),
                );
                document.set_word_wrap(self.settings.word_wrap);
                document.finish_session_scroll_restore();
                false
            }
            EditorAction::ToggleFold(range) => {
                toggle_fold(document, range);
                false
            }
            EditorAction::FoldCurrent => {
                set_current_fold_collapsed(document, true);
                false
            }
            EditorAction::UnfoldCurrent => {
                set_current_fold_collapsed(document, false);
                false
            }
            EditorAction::ToggleCurrentFold => {
                toggle_current_fold(document);
                false
            }
            EditorAction::FoldAll => {
                set_all_folds_collapsed(document, true);
                false
            }
            EditorAction::UnfoldAll => {
                set_all_folds_collapsed(document, false);
                false
            }
            EditorAction::GoToMatchingDelimiter => {
                go_to_matching_delimiter(document);
                false
            }
            EditorAction::SelectMatchingDelimiter => {
                select_matching_delimiter(document);
                false
            }
            EditorAction::SelectMatchingDelimiterInPlace => {
                select_delimiter_in_place(document);
                false
            }
            EditorAction::NextFunction => {
                go_to_next_function(document, outline_entries);
                false
            }
            EditorAction::PreviousFunction => {
                go_to_previous_function(document, outline_entries);
                false
            }
            EditorAction::SelectCurrentFunction => {
                select_current_function(document, outline_entries);
                false
            }
            EditorAction::SelectCurrentFunctionBody => {
                select_current_function_body(document, outline_entries);
                false
            }
            EditorAction::Undo => document.undo(),
            EditorAction::Redo => document.redo(),
            EditorAction::Copy | EditorAction::Paste => false,
            EditorAction::Cut => {
                if selection_set_is_all_carets(
                    document.selection_set(),
                    &document.buffer,
                    tab_width,
                ) {
                    delete_line(document)
                } else {
                    replace_selection(document, "", false, tab_width)
                }
            }
            EditorAction::Focus => false,
            EditorAction::MoveSelection { source, target } => {
                super::editor_ops::move_selection(document, &source, target, tab_width)
            }
            EditorAction::PlaceCaret(position) => {
                document.preferred_vertical_column = None;
                document.clear_caret_row_affinity();
                let position = document.buffer.clamp_position(position);
                document.set_main_selection(EditorSelection::new(position, position));
                false
            }
            EditorAction::PlaceCaretOnRow { position, row } => {
                document.preferred_vertical_column = None;
                let position = document.buffer.clamp_position(position);
                document.set_main_selection(EditorSelection::new(position, position));
                document.set_caret_row_affinity(position, row);
                false
            }
            EditorAction::SelectWordAt(position) => {
                document.clear_caret_row_affinity();
                select_word_at(document, position);
                false
            }
            EditorAction::SelectRegion(selection) => {
                document.preferred_vertical_column = None;
                document.clear_caret_row_affinity();
                document.set_main_selection(selection);
                false
            }
            EditorAction::SelectRegionOnRow { selection, row } => {
                document.preferred_vertical_column = None;
                document.set_main_selection(selection);
                document.set_caret_row_affinity(document.main_selection().cursor, row);
                false
            }
            EditorAction::AddCaretAbove => {
                add_adjacent_caret(document, CaretMotion::Up);
                false
            }
            EditorAction::AddCaretBelow => {
                add_adjacent_caret(document, CaretMotion::Down);
                false
            }
            EditorAction::SplitSelectionIntoLines => {
                split_selection_into_lines(document, tab_width);
                false
            }
            EditorAction::ConvertSelectionToRectangle => {
                convert_selection_to_rectangle(document, tab_width);
                false
            }
        }
    }

    fn editor_action_blocked_while_indexing(
        &self,
        document_id: DocumentId,
        action: &EditorAction,
    ) -> bool {
        let Some(document) = self.workspace.document(document_id) else {
            return false;
        };
        if document.has_complete_text_index() {
            return false;
        }

        action.mutates_document()
    }

    fn clipboard_task(&self, document_id: DocumentId, action: &EditorAction) -> Task<Message> {
        match action {
            EditorAction::Copy | EditorAction::Cut => self
                .workspace
                .document(document_id)
                .and_then(|document| {
                    let tab_width = self.settings.indentation.width() as usize;
                    if selection_set_is_all_carets(
                        document.selection_set(),
                        &document.buffer,
                        tab_width,
                    ) {
                        line_span_text(document, tab_width)
                    } else {
                        selected_text(document, tab_width)
                    }
                })
                .map(|text| clipboard::write(text).map(Message::ClipboardWritten))
                .unwrap_or_else(Task::none),
            EditorAction::CopyLine | EditorAction::CutLine => self
                .workspace
                .document(document_id)
                .and_then(|document| {
                    line_span_text(document, self.settings.indentation.width() as usize)
                })
                .map(|text| clipboard::write(text).map(Message::ClipboardWritten))
                .unwrap_or_else(Task::none),
            EditorAction::Paste => self
                .workspace
                .document(document_id)
                .map(|document| PasteRequest {
                    document_id,
                    revision: document.revision(),
                    selection: document.main_selection(),
                    selection_set: document.selection_set().clone(),
                    clipboard_mode: paste_clipboard_mode(
                        document,
                        self.settings.indentation.width() as usize,
                    ),
                })
                .map(|request| {
                    clipboard::read_text()
                        .map(move |result| Message::ClipboardRead(request.clone(), result))
                })
                .unwrap_or_else(Task::none),
            _ => Task::none(),
        }
    }

    fn apply_paste_request(&mut self, request: &PasteRequest, text: &str) -> bool {
        let Some(document) = self.workspace.document_mut(request.document_id) else {
            return false;
        };
        if document.revision() != request.revision {
            return false;
        }

        let tab_width = self.settings.indentation.width() as usize;
        document.set_selection_set(request.selection_set.clone());
        document.preferred_vertical_column = None;
        paste_selection(document, request.clipboard_mode, text, tab_width)
    }
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;

impl App {
    pub(super) fn update_active_editor_command(&mut self, action: EditorAction) -> Task<Message> {
        self.menu.close();

        Task::batch([
            self.update_editor(self.workspace.active_document_id(), action),
            iced::widget::operation::focus(crate::ui::editor::EDITOR_ID),
        ])
    }
}
