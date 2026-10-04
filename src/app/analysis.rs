//! Document analysis owns worker state independently of session persistence.

use super::App;
use crate::core::document::DocumentAnalysis;
use crate::core::{Document, DocumentId, Workspace};
use crate::message::Message;
use iced::Task;

#[derive(Debug, Default)]
pub(super) struct DocumentAnalysisState {
    in_flight: Option<(DocumentId, u64, String, usize)>,
}

impl DocumentAnalysisState {
    pub(super) fn schedule(
        &mut self,
        document: Option<&Document>,
    ) -> Option<Task<DocumentAnalysis>> {
        if self.in_flight.is_some() {
            return None;
        }
        let (buffer, request) = document.and_then(Document::analysis_request)?;
        self.in_flight = Some((
            request.document_id,
            request.revision,
            request.syntax_token.clone(),
            request.indent_width,
        ));
        Some(Task::perform(
            async move {
                let fallback = request.clone();
                tokio::task::spawn_blocking(move || {
                    crate::core::document::analyze_document(buffer, request)
                })
                .await
                .unwrap_or(fallback)
            },
            std::convert::identity,
        ))
    }

    fn complete(
        &mut self,
        workspace: &mut Workspace,
        result: DocumentAnalysis,
    ) -> Option<DocumentId> {
        self.in_flight = None;
        let document = workspace.document_mut(result.document_id)?;
        document.apply_analysis(result).then_some(document.id)
    }
}

impl App {
    pub(super) fn complete_document_analysis(&mut self, result: DocumentAnalysis) -> Task<Message> {
        if let Some(id) = self.analysis.complete(&mut self.workspace, result) {
            self.events
                .publish(super::events::Event::AnalysisCompleted(id));
        }
        self.events.publish(super::events::Event::AnalysisAvailable);
        Task::none()
    }
}

impl DocumentAnalysisState {
    pub(super) fn observe(
        &self,
        event: super::events::Event,
        active: DocumentId,
        work: &mut super::events::PendingWork,
    ) {
        use super::events::{Event, Work};
        use crate::core::workspace::changes::WorkspaceEvent as W;
        let needed = match event {
            Event::Started | Event::SettingsChanged | Event::AnalysisAvailable => true,
            Event::Workspace(W::ActiveDocumentChanged(_) | W::DocumentOpened(_)) => true,
            Event::Workspace(
                W::ContentChanged(id) | W::LoadStateChanged(id) | W::AnalysisInvalidated(id),
            ) => id == active,
            _ => false,
        };
        if needed {
            work.request(Work::Analysis);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_state_is_independent_of_app_and_stale_results_allow_fresh_work() {
        let mut workspace = Workspace::new();
        workspace.active_document_mut().unwrap().analysis_pending = true;
        let request = workspace
            .active_document()
            .unwrap()
            .analysis_request()
            .unwrap()
            .1;
        let mut analysis = DocumentAnalysisState::default();
        assert!(analysis.schedule(workspace.active_document()).is_some());
        assert!(analysis.schedule(workspace.active_document()).is_none());

        let document = workspace.active_document_mut().unwrap();
        document.defer_analysis = true;
        document.buffer = crate::editor::EditorBuffer::from_text("changed");
        document.refresh_after_text_change();
        assert!(analysis.complete(&mut workspace, request).is_none());
        assert!(analysis.schedule(workspace.active_document()).is_some());
    }

    #[test]
    fn completion_during_shutdown_releases_worker_and_failed_exit_resumes_work() {
        let (mut app, _) = App::new();
        app.workspace
            .active_document_mut()
            .unwrap()
            .analysis_pending = true;
        let request = app
            .workspace
            .active_document()
            .unwrap()
            .analysis_request()
            .unwrap()
            .1;
        let _ = app.update(Message::None);
        assert!(app.analysis.in_flight.is_some());

        app.lifecycle.begin_shutdown();
        let _ = app.update(Message::DocumentAnalyzed(request));
        assert!(app.analysis.in_flight.is_some());
        let _ = app.update(Message::ShutdownPersisted(Err("disk unavailable".into())));
        assert!(app.analysis.in_flight.is_none());
        assert!(!app.workspace.active_document().unwrap().analysis_pending);

        app.workspace
            .active_document_mut()
            .unwrap()
            .analysis_pending = true;
        let _ = app.update(Message::None);
        assert!(!app.lifecycle.is_exiting());
        assert!(app.analysis.in_flight.is_some());
    }
}
