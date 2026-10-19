//! Focus restoration after a document close prompt leaves the widget tree.

use super::App;
use crate::message::Message;
use iced::Task;
use iced::advanced::widget::{self, operation};

impl App {
    pub(super) fn focus_editor_after_close_prompt(&self) -> Task<Message> {
        // Removing the modal remounts the editor tree. Inspect focus when the
        // operation runs, so input focused during an async save keeps ownership.
        // An already visible inline Find must not prevent editor restoration.
        if self.lifecycle.is_exiting()
            || self.close_prompt.document().is_some()
            || self.go_to_line_prompt.is_some()
            || self.chrome_animation.about.rendered_visible()
            || self.is_window_list_visible
            || self.workspace.active_document().is_none()
        {
            return Task::none();
        }
        self.main_window_id
            .map(|id| widget::operate(RestoreEditorFocus::default()).scope_window(id))
            .unwrap_or_else(Task::none)
    }
}

#[derive(Default)]
struct RestoreEditorFocus {
    has_focus: bool,
}

impl widget::Operation<Message> for RestoreEditorFocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation<Message>)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        _id: Option<&widget::Id>,
        _bounds: iced::Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        self.has_focus |= state.is_focused();
    }

    fn finish(&self) -> operation::Outcome<Message> {
        if self.has_focus {
            operation::Outcome::None
        } else {
            operation::Outcome::Chain(Box::new(operation::focusable::focus(widget::Id::new(
                crate::ui::editor::EDITOR_ID,
            ))))
        }
    }
}
