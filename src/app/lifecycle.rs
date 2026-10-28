//! Application startup and coordinated shutdown.

use super::App;
use crate::message::{Message, ShutdownDelivery};
use crate::services::settings_store;
use iced::{Task, window};
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Default)]
pub(super) struct Lifecycle {
    exiting: bool,
    native_quit_pending: bool,
    deferred: VecDeque<Message>,
    pub(super) main_window_opened: bool,
    pub(super) pending_startup_gpu_boost: bool,
}

impl Lifecycle {
    pub(super) fn is_exiting(&self) -> bool {
        self.exiting
    }

    pub(super) fn begin_shutdown(&mut self) {
        self.exiting = true;
    }

    /// Record the native request and report whether a close sequence is needed.
    fn begin_native_quit(&mut self) -> bool {
        self.native_quit_pending = true;
        !self.exiting
    }

    fn cancel_native_quit_if_idle(&mut self, waiting_to_close: bool) -> bool {
        if self.exiting || waiting_to_close {
            return false;
        }
        std::mem::take(&mut self.native_quit_pending)
    }

    pub(super) fn resume(&mut self) -> VecDeque<Message> {
        self.exiting = false;
        std::mem::take(&mut self.deferred)
    }

    pub(super) fn admit(&mut self, message: Message) -> Option<Message> {
        if !self.exiting {
            return Some(message);
        }
        match message.shutdown_delivery() {
            ShutdownDelivery::Defer => self.deferred.push_back(message),
            ShutdownDelivery::Reject => {
                if let Message::ForwardedFiles(_, _, receipt) = message {
                    receipt.resolve(false);
                }
            }
            ShutdownDelivery::Resolve => return Some(message),
        }
        None
    }
}

impl App {
    pub(super) fn native_quit_requested(&mut self) -> Task<Message> {
        if self.lifecycle.begin_native_quit() {
            self.exit_request()
        } else {
            Task::none()
        }
    }

    pub(super) fn finish_native_quit(&mut self) -> Task<Message> {
        let waiting_to_close = self.should_exit() && self.close_operation_pending();
        if self.lifecycle.cancel_native_quit_if_idle(waiting_to_close) {
            iced::system::cancel_quit()
        } else {
            Task::none()
        }
    }

    pub(super) fn startup_ready(&mut self) -> Task<Message> {
        self.session.mark_startup_ready();
        self.restore_startup()
    }

    pub(super) fn forward_files(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        request: crate::ipc::ActivationRequest,
        receipt: crate::ipc::AdmissionReceipt,
    ) -> Task<Message> {
        if !receipt.try_accept() {
            return Task::none();
        }
        let open = if self.editor_windows.is_empty() {
            self.new_editor_window()
        } else {
            Task::none()
        };
        if let Some(id) = self.last_active_editor {
            self.switch_editor(id);
        }
        Task::batch([open, self.open_paths(paths), self.show_main_window(request)])
    }

    pub(super) fn persist_before_exit(&mut self) -> Task<Message> {
        if !self.settings_persistence.is_loaded() || !self.session.is_initialized() {
            self.file_status = Some("Finish starting before closing.".into());
            return Task::none();
        }
        if self.session.read_failed() {
            if self.workspace.documents().iter().any(|doc| doc.is_dirty) {
                self.file_status = Some("Save or close unsaved tabs before exiting; the unreadable previous session will be preserved.".into());
                return Task::none();
            }
            // Recovery cannot be written, so saved tabs still need a disk
            // check before their only in-memory copy can be discarded.
            return self.close_documents_for_exit();
        }
        self.lifecycle.begin_shutdown();
        let settings = (!self.settings_persistence.read_failed())
            .then(|| settings_store::save_settings(self.settings.clone()));
        let session = crate::services::session_store::save_session(self.capture_session());
        let preserve_settings = self.settings_persistence.read_failed();
        Task::perform(
            async move {
                if let Some(settings) = settings {
                    settings
                        .await
                        .map_err(|e| format!("settings: {}", e.summary()))?;
                }
                session.await?;
                if preserve_settings {
                    Ok(())
                } else {
                    settings_store::flush_settings()
                        .await
                        .map_err(|e| e.summary().to_owned())
                }
            },
            Message::ShutdownPersisted,
        )
    }

    pub(super) fn shutdown_persisted(&mut self, result: Result<(), String>) -> Task<Message> {
        match result {
            Ok(()) => {
                self.lifecycle.deferred.clear();
                iced::exit()
            }
            Err(error) => {
                // Preserve event order, including streamed chunks before their
                // completion and timers before subsequent save acknowledgments.
                let deferred = self.lifecycle.resume();
                let tasks = deferred
                    .into_iter()
                    .map(|message| self.update(message))
                    .collect::<Vec<_>>();
                self.file_status = Some(format!("Could not save session: {error}"));
                Task::batch(tasks)
            }
        }
    }

    pub(super) fn exit_after_settings(&mut self) -> Task<Message> {
        self.lifecycle.begin_shutdown();
        if !self.settings_persistence.is_loaded() || self.settings_persistence.read_failed() {
            return iced::exit();
        }
        let save = settings_store::save_settings(self.settings.clone());
        Task::perform(
            async move { save.await.map_err(|error| error.summary().to_owned()) },
            Message::ShutdownPersisted,
        )
    }
}

impl App {
    pub(super) fn window_opened(&mut self, id: window::Id) -> Task<Message> {
        if !self.owns_window(id) {
            // A cancelled gesture can finish native creation after its
            // registry entry was removed. Retire that delayed surface too.
            return window::close(id);
        }
        if self.main_window_id == Some(id) {
            self.lifecycle.main_window_opened = true;
        }

        let startup_task = if !self.session.is_initialized()
            && self.main_window_id == Some(id)
            && self.session.has_startup_work()
        {
            Task::perform(
                async {
                    tokio::time::sleep(Duration::from_millis(16)).await;
                },
                |_| Message::StartupReady,
            )
        } else {
            Task::none()
        };
        let probe_task =
            if self.main_window_id == Some(id) && crate::startup::startup_probe_enabled() {
                window::screenshot(id).map(|_| Message::StartupFrameReady)
            } else {
                Task::none()
            };
        let visible = if self.tab_drag.is_none() {
            window::ensure_visible(id)
        } else {
            Task::none()
        };
        // The editor's widget tree exists only after native window creation.
        // Target this window explicitly: opening a second editor can originate
        // in a different window, whose update task has its own scope.
        let editor_focus = if self.editor_id(id).is_some() && self.tab_drag.is_none() {
            iced::widget::operation::focus(crate::ui::editor::EDITOR_ID).scope_window(id)
        } else {
            Task::none()
        };
        let window_task = Task::batch([
            self.register_opened_window(id),
            startup_task,
            probe_task,
            visible,
            editor_focus,
            self.capture_window_geometry(id),
        ]);

        if self.main_window_id == Some(id) && self.lifecycle.pending_startup_gpu_boost {
            self.lifecycle.pending_startup_gpu_boost = false;
            Task::batch([window_task, self.request_gpu_boost()])
        } else {
            window_task
        }
    }
}
