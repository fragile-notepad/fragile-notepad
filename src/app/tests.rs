pub(in crate::app) mod test_support {
    pub(super) use crate::app::windowing::managed::ManagedWindow;
    pub(super) use crate::app::{App, CloseGoal};
    pub(super) use crate::core::DirtyCloseDecision;
    pub(super) use crate::core::{HardwareAccelerationMode, IndentationMode};
    pub(super) use crate::editor::{EditorAction, EditorBuffer, EditorPosition, EditorSelection};
    pub(super) use crate::message::{
        AboutTab, ClipboardMode, Menu, Message, PasteRequest, SaveRequest,
    };
    pub(super) use crate::services::types::{
        FileLoadChunk, FileLoadFailure, FileLoadFinished, OpenedFile,
    };
    pub(super) use std::path::PathBuf;
    pub(super) use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

    pub(in crate::app) struct TestFile(pub(in crate::app) PathBuf);

    impl TestFile {
        pub(in crate::app) fn new(bytes: &[u8]) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "fragile-app-file-{}-{}.txt",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    pub(in crate::app) fn run_task(app: &mut App, task: iced::Task<Message>) {
        use futures::StreamExt;
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                    while let Some(action) = stream.next().await {
                        if let iced_runtime::Action::Output(message) = action {
                            let _ = app.update(message);
                        }
                    }
                }
            });
    }

    pub(super) fn pending_save_all_ids(app: &App) -> Vec<crate::core::DocumentId> {
        app.files.pending_save_all().iter().copied().collect()
    }

    pub(super) fn pending_auto_save_ids(app: &App) -> Vec<crate::core::DocumentId> {
        app.files.pending_auto_saves().iter().copied().collect()
    }

    pub(super) fn set_active_document_text(app: &mut App, text: &str, selection: EditorSelection) {
        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        document.buffer = EditorBuffer::from_text(text);
        document.selection = selection;
        document.refresh_after_text_change();
        document.mark_clean();
    }
}

#[path = "app_tests/dirty_close.rs"]
mod dirty_close;
#[path = "app_tests/documents.rs"]
mod documents;
#[path = "app_tests/editor_actions.rs"]
mod editor_actions;
#[path = "app_tests/lifecycle.rs"]
mod lifecycle;
#[path = "app_tests/search.rs"]
mod search;

#[path = "app_tests/go_to_line.rs"]
mod go_to_line;

#[path = "app_tests/recovery.rs"]
mod recovery;

#[path = "app_tests/events.rs"]
mod events;
