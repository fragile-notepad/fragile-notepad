use crate::message::GoToLineMessage;
use iced::{Task, widget::operation};

use super::{App, animation::RevealAnimation};
use crate::core::DocumentId;
use crate::editor::{EditorPosition, EditorSelection};
use crate::message::Message;
use crate::ui::go_to_line_prompt::INPUT_ID;
use std::time::Instant;

#[derive(Debug)]
pub(super) struct GoToLinePrompt {
    document_id: DocumentId,
    pub input: String,
    pub error: Option<String>,
    pub animation: RevealAnimation,
}

impl App {
    pub(super) fn update_go_to_line_frame(&mut self, at: Instant) -> Task<Message> {
        if let Some(prompt) = &mut self.go_to_line_prompt {
            prompt.animation.update_frame(at);
            if !prompt.animation.rendered_visible() {
                self.go_to_line_prompt = None;
                return operation::focus(crate::ui::editor::EDITOR_ID);
            }
        }
        Task::none()
    }

    pub(super) fn update_go_to_line(&mut self, message: GoToLineMessage) -> Task<Message> {
        match message {
            GoToLineMessage::GoToLineOpened => {
                if self.go_to_line_prompt.is_some()
                    || self.close_prompt.document().is_some()
                    || self.is_about_visible
                    || self.is_window_list_visible
                    || self.is_tab_list_visible
                {
                    return Task::none();
                }
                let Some(document) = self.workspace.active_document_mut() else {
                    return Task::none();
                };
                document.sync_selection_mirror();
                let mut animation = RevealAnimation::hidden();
                animation.set_visible(true);
                self.go_to_line_prompt = Some(GoToLinePrompt {
                    document_id: document.id,
                    input: (document.main_selection().cursor.line + 1).to_string(),
                    error: None,
                    animation,
                });
                self.menu.close();

                self.main_window_id
                    .map(iced::window::gain_focus)
                    .unwrap_or_else(Task::none)
                    .chain(operation::focus(INPUT_ID))
                    .chain(operation::select_all(INPUT_ID))
            }
            GoToLineMessage::GoToLineChanged(input) => {
                if let Some(prompt) = &mut self.go_to_line_prompt
                    && prompt.animation.target_visible()
                {
                    prompt.input = input;
                    prompt.error = None;
                }
                Task::none()
            }
            GoToLineMessage::GoToLineSubmitted => {
                let Some(prompt) = &mut self.go_to_line_prompt else {
                    return Task::none();
                };
                if !prompt.animation.target_visible() {
                    return Task::none();
                }
                let Ok(line_number) = prompt.input.trim().parse::<usize>() else {
                    prompt.error = Some("Enter a valid line number.".into());
                    return operation::focus(INPUT_ID);
                };
                let Some(document) = self.workspace.active_document_mut() else {
                    return self.update_go_to_line(GoToLineMessage::GoToLineClosed);
                };
                if document.id != prompt.document_id {
                    return self.update_go_to_line(GoToLineMessage::GoToLineClosed);
                }
                if !document.has_complete_text_index() {
                    prompt.error = Some("Wait for the document to finish loading.".into());
                    return operation::focus(INPUT_ID);
                }
                let position = document
                    .buffer
                    .clamp_position(EditorPosition::new(line_number.saturating_sub(1), 0));
                document.set_main_selection(EditorSelection::new(position, position));
                document.reveal_position(position);
                self.update_go_to_line(GoToLineMessage::GoToLineClosed)
            }
            GoToLineMessage::GoToLineClosed => {
                let Some(prompt) = &mut self.go_to_line_prompt else {
                    return Task::none();
                };
                if !prompt.animation.target_visible() {
                    return Task::none();
                }
                prompt.animation.set_visible(false);
                if prompt.animation.rendered_visible() {
                    return Task::none();
                }
                self.go_to_line_prompt = None;
                operation::focus(crate::ui::editor::EDITOR_ID)
            }
        }
    }
}
