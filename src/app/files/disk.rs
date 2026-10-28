//! Checks saved files without replacing the editor's text.

use crate::app::App;
use crate::core::{DiskState, Document};
use crate::message::Message;
use crate::services::file_system::{FileCheck, check_file};
use iced::Task;

impl App {
    pub(in crate::app) fn check_disk_files(&mut self) -> Task<Message> {
        self.start_disk_checks(true)
    }

    pub(super) fn poll_disk_files(&mut self) -> Task<Message> {
        self.start_disk_checks(false)
    }

    fn start_disk_checks(&mut self, compare_bytes: bool) -> Task<Message> {
        if self.lifecycle.is_exiting() {
            return Task::none();
        }
        if self.files.disk_check_running {
            self.files.disk_check_again |= compare_bytes;
            return Task::none();
        }
        let mut checks: Vec<_> = self
            .workspace
            .documents()
            .iter()
            .filter(|document| {
                document.has_complete_text_index()
                    && self.files.pending_disk_close != Some(document.id)
                    && self
                        .files
                        .pending_save
                        .as_ref()
                        .is_none_or(|save| save.document_id != document.id)
            })
            .filter_map(|document| {
                let mut check = file_check(document)?;
                if !compare_bytes
                    && let Some(previous) = self.files.disk_stamps.get(&document.id)
                    && previous.path == check.path
                    && previous.expected == check.expected
                {
                    check.stamp = previous.stamp;
                }
                Some(check)
            })
            .collect();
        if checks.is_empty() {
            return Task::none();
        }
        for check in &mut checks {
            self.assign_disk_check_id(check);
        }
        self.files.disk_check_running = true;
        Task::perform(
            async move {
                let mut results = Vec::with_capacity(checks.len());
                // Read files sequentially so many open tabs do not compete for I/O.
                for check in checks {
                    results.push(check_file(check).await);
                }
                results
            },
            Message::DiskFilesChecked,
        )
    }

    pub(super) fn disk_files_checked(&mut self, checks: Vec<FileCheck>) -> Task<Message> {
        self.files.disk_check_running = false;
        for check in checks {
            self.apply_file_check(check);
        }
        if std::mem::take(&mut self.files.disk_check_again) {
            self.check_disk_files()
        } else {
            Task::none()
        }
    }

    fn apply_file_check(&mut self, check: FileCheck) -> bool {
        let Some(document) = self.workspace.document_mut(check.document_id) else {
            return false;
        };
        if self.files.disk_check_ids.get(&check.document_id) != Some(&check.check_id)
            || document.path.as_ref() != Some(&check.path)
            || document.disk_revision != Some(check.expected)
            || !document.has_complete_text_index()
            || self
                .files
                .pending_save
                .as_ref()
                .is_some_and(|save| save.document_id == document.id)
        {
            return false;
        }
        if let Some(state) = check.state {
            document.disk_state = state;
            if state != DiskState::Current {
                // Even a previously clean buffer may now be the only copy.
                document.mark_dirty();
            }
        }
        self.files.disk_stamps.insert(document.id, check);
        true
    }

    fn assign_disk_check_id(&mut self, check: &mut FileCheck) {
        self.files.next_disk_check += 1;
        check.check_id = self.files.next_disk_check;
        self.files
            .disk_check_ids
            .insert(check.document_id, check.check_id);
    }

    pub(super) fn check_disk_before_close(&mut self, mut check: FileCheck) -> Task<Message> {
        self.assign_disk_check_id(&mut check);
        self.files.pending_disk_close = Some(check.document_id);
        Task::perform(check_file(check), Message::FileCheckedForClose)
    }

    pub(super) fn file_checked_for_close(&mut self, check: FileCheck) -> Task<Message> {
        let id = check.document_id;
        if self.files.pending_disk_close != Some(id) {
            return Task::none();
        }
        self.files.pending_disk_close = None;
        if !self.apply_file_check(check) {
            // A save or reload started while the check was running. Keep the tab.
            self.clear_close();
            self.files.close_goal = super::CloseGoal::KeepOpen;
            self.file_status = Some("The file changed while closing. Please close it again after the current operation finishes.".into());
            return Task::none();
        }
        let close = self.close_checked_document(id);
        if self.close_prompt.document().is_some() {
            close
        } else {
            Task::batch([close, self.continue_close()])
        }
    }
}

pub(super) fn file_check(document: &Document) -> Option<FileCheck> {
    Some(FileCheck {
        check_id: 0,
        document_id: document.id,
        path: document.path.clone()?,
        expected: document.disk_revision?,
        stamp: None,
        state: None,
    })
}

#[cfg(test)]
mod tests;
