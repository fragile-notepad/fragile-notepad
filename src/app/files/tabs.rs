//! Context commands retain the clicked document ID without selecting its tab.

use super::CloseGoal;
use crate::app::App;
use crate::core::DocumentId;
use crate::message::{FileMessage, Message, TabContextAction};
use iced::{Task, clipboard};

impl App {
    pub(super) fn update_tab_context_action(
        &mut self,
        document: DocumentId,
        action: TabContextAction,
    ) -> Task<Message> {
        let Some(owner) = self.workspace.owner_of(document) else {
            return Task::none();
        };
        if self.close_operation_pending() && action != TabContextAction::CopyPath {
            return Task::none();
        }
        if !self.switch_editor(owner) {
            return Task::none();
        }
        self.menu.close();

        let ids = match action {
            TabContextAction::TogglePin => {
                return self.update_file(FileMessage::TabPinToggled(document));
            }
            TabContextAction::CopyPath => {
                return self
                    .workspace
                    .document(document)
                    .and_then(|document| document.path.as_ref())
                    .map(|path| {
                        let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
                        clipboard::write(path.to_string_lossy().into_owned())
                            .map(Message::ClipboardWritten)
                    })
                    .unwrap_or_else(Task::none);
            }
            TabContextAction::MoveToNewWindow => return self.move_tab_to_new_window(document),
            TabContextAction::MoveToWindow(native) => {
                return self.move_tab_to_window(document, native);
            }
            TabContextAction::Close => vec![document],
            TabContextAction::CloseOthers => self.workspace.document_ids_except(document),
            TabContextAction::CloseAll => self.workspace.document_ids(),
            TabContextAction::CloseUnmodified => self.workspace.document_ids_clean(),
            TabContextAction::CloseToLeft => self.workspace.document_ids_to_left_of(document),
            TabContextAction::CloseToRight => self.workspace.document_ids_to_right_of(document),
            TabContextAction::CloseUnpinned => self.workspace.document_ids_unpinned(),
        };
        self.files.close_goal = CloseGoal::KeepOpen;
        self.close_documents(ids)
    }
}

#[cfg(test)]
mod tests;
