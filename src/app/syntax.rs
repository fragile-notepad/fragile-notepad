use super::App;
use crate::core::{Document, DocumentId, Workspace};
use crate::editor::EditorBuffer;
use crate::editor::render::SyntaxParseResult;
use crate::editor::render::syntax::SyntaxParseRequest;
use crate::message::Message;
use iced::{Task, highlighter};
use std::sync::Arc;

#[derive(Debug, Default)]
pub(super) struct SyntaxParsing {
    next_id: u64,
    in_flight: Option<PendingParse>,
    snapshot: Option<SyntaxSnapshot>,
}

#[derive(Debug)]
struct SyntaxSnapshot {
    document: DocumentId,
    revision: u64,
    generation: Arc<()>,
    buffer: Arc<EditorBuffer>,
}

#[derive(Debug)]
struct PendingParse {
    id: u64,
    document: DocumentId,
    revision: u64,
    settings: highlighter::Settings,
    generation: Arc<()>,
}

impl App {
    #[cfg(test)]
    fn next_syntax_request(&mut self) -> Option<(u64, SyntaxParseRequest)> {
        if self.lifecycle.is_exiting() {
            return None;
        }
        self.syntax_parsing.next_request(
            self.workspace.active_document(),
            self.settings.resolved_syntax_theme(self.system_dark),
        )
    }

    pub(super) fn complete_syntax_parse(
        &mut self,
        id: u64,
        result: Result<SyntaxParseResult, String>,
    ) -> Task<Message> {
        self.syntax_parsing.complete(
            &self.workspace,
            self.settings.resolved_syntax_theme(self.system_dark),
            id,
            result,
        );
        self.events.publish(super::events::Event::SyntaxAvailable);
        Task::none()
    }
}

impl SyntaxParsing {
    pub(super) fn schedule(
        &mut self,
        document: Option<&Document>,
        theme: highlighter::Theme,
    ) -> Option<Task<Message>> {
        let (id, request) = self.next_request(document, theme)?;
        Some(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || request.parse())
                    .await
                    .map_err(|error| error.to_string())
            },
            move |result| Message::SyntaxParsed(id, result),
        ))
    }

    fn next_request(
        &mut self,
        document: Option<&Document>,
        theme: highlighter::Theme,
    ) -> Option<(u64, SyntaxParseRequest)> {
        let document = document?;
        let settings = highlighter::Settings {
            token: document.render_syntax_token().to_owned(),
            theme,
        };
        let mut cache = document.syntax_cache.borrow_mut();
        cache.configure(&settings);
        // At most one bounded worker batch is outstanding, even across tab
        // switches/edits. The next batch always uses the latest active viewport.
        if self.in_flight.is_some() {
            return None;
        }
        if !document.has_complete_text_index() || settings.token == "txt" {
            self.snapshot = None;
            return None;
        }
        let last_line = document.buffer.line_count().saturating_sub(1);
        if !cache.needs_parse(last_line) {
            return None;
        }
        let revision = document.revision();
        let snapshot = &mut self.snapshot;
        if !snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.document == document.id
                && snapshot.revision == revision
                && Arc::ptr_eq(&snapshot.generation, cache.generation())
        }) {
            // Clone the rope and its line index once per document revision,
            // not on each batch or frame.
            *snapshot = Some(SyntaxSnapshot {
                document: document.id,
                revision,
                generation: cache.generation().clone(),
                buffer: Arc::new(document.buffer.clone()),
            });
        }
        let first_row = document.scroll.first_visible_row;
        let end_row = first_row.saturating_add(document.viewport_visible_rows + 1);
        let mut seen = std::collections::HashSet::new();
        // Visible rows precede lookahead/lookbehind. Mapping each row also
        // skips folded blocks and deduplicates wrapped fragments of a line.
        let priority_lines: Vec<_> = (first_row..end_row.saturating_add(32))
            .chain(first_row.saturating_sub(32)..first_row)
            .filter_map(|row| document.viewport.visible_row_to_document_line(row))
            .filter(|line| seen.insert(*line))
            .collect();
        let request =
            cache.parse_request(snapshot.as_ref().unwrap().buffer.clone(), &priority_lines);
        self.next_id += 1;
        let id = self.next_id;
        self.in_flight = Some(PendingParse {
            id,
            document: document.id,
            revision,
            settings,
            generation: cache.generation().clone(),
        });
        Some((id, request))
    }

    fn complete(
        &mut self,
        workspace: &Workspace,
        theme: highlighter::Theme,
        id: u64,
        result: Result<SyntaxParseResult, String>,
    ) {
        if self
            .in_flight
            .as_ref()
            .is_none_or(|pending| pending.id != id)
        {
            return;
        }
        let pending = self.in_flight.take().unwrap();
        let Some(document) = workspace.document(pending.document) else {
            self.snapshot = None;
            return;
        };
        if document.revision() != pending.revision
            || document.render_syntax_token() != pending.settings.token
            || theme != pending.settings.theme
        {
            return;
        }
        let mut cache = document.syntax_cache.borrow_mut();
        if !Arc::ptr_eq(cache.generation(), &pending.generation) {
            return;
        }
        match result {
            Ok(result) => {
                cache.apply_parsed(result);
            }
            Err(_) => {
                // Do not spin on a worker failure. Editing or changing syntax
                // settings invalidates this suppression along with the cache.
                cache.stop_parsing();
            }
        }
        // This message causes a redraw, and App::update schedules the next
        // batch, first filling the latest viewport and then refining context.
    }
}

impl SyntaxParsing {
    pub(super) fn observe(
        &self,
        event: super::events::Event,
        active: DocumentId,
        work: &mut super::events::PendingWork,
    ) {
        use super::events::{Event, Work};
        use crate::core::workspace::changes::WorkspaceEvent as W;
        let needed = match event {
            Event::Started | Event::SettingsChanged | Event::SyntaxAvailable => true,
            Event::Workspace(W::ActiveDocumentChanged(_) | W::DocumentOpened(_)) => true,
            Event::AnalysisCompleted(id)
            | Event::Workspace(
                W::ContentChanged(id) | W::ViewChanged(id) | W::LoadStateChanged(id),
            ) => id == active,
            _ => false,
        };
        if needed {
            work.request(Work::Syntax);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered_rows(app: &App) -> Vec<crate::editor::render::RowRenderPlan> {
        let document = app.workspace.active_document().unwrap();
        crate::editor::build_render_plan_with_cache(
            &document.buffer,
            &document.viewport,
            &document.decorations,
            document.main_selection(),
            crate::editor::EditorLayout::new(
                crate::editor::EditorMetrics::default(),
                document.scroll,
                800.0,
                400.0,
            ),
            &document.syntax_cache.borrow(),
        )
        .rows
    }

    #[test]
    fn typing_keeps_displayed_highlights_until_exact_replacement_arrives() {
        let (mut app, _) = App::new();
        let source = format!("<!--\n{}-->\n", "<p>comment text</p>\n".repeat(200));
        app.workspace.insert_loaded_file("example.html", &source);
        let document = app.workspace.active_document_mut().unwrap();
        document.scroll.first_visible_row = 75;
        let position = crate::editor::EditorPosition::new(80, 10);
        document.set_main_selection(crate::editor::EditorSelection::new(position, position));
        document.ensure_syntax_cache(app.settings.resolved_syntax_theme(app.system_dark));
        let before = rendered_rows(&app);
        let id = app.workspace.active_document_id();

        for text in ["a", "é", "🦀"] {
            let _ = app.update_editor(id, crate::editor::EditorAction::InsertText(text.into()));
            // This is the frame immediately after an edit, before any worker
            // can return. Previously every visible row lost its colors here.
            let after = rendered_rows(&app);
            for (old, new) in before.iter().zip(&after) {
                assert!(
                    !new.syntax_spans.is_empty(),
                    "line {} flashed unstyled",
                    new.line
                );
                if old.line != 80 {
                    assert_eq!(old.syntax_spans, new.syntax_spans);
                }
            }
            while let Some((batch, request)) = app.next_syntax_request() {
                let _ = app.complete_syntax_parse(batch, Ok(request.parse()));
                for row in rendered_rows(&app) {
                    assert!(!row.syntax_spans.is_empty());
                    assert!(row.syntax_spans.iter().all(|span| {
                        row.text.is_char_boundary(span.range.start)
                            && row.text.is_char_boundary(span.range.end)
                    }));
                    if row.line != 80 {
                        assert_eq!(
                            row.syntax_spans,
                            before
                                .iter()
                                .find(|old| old.line == row.line)
                                .unwrap()
                                .syntax_spans
                        );
                    }
                }
            }
            let document = app.workspace.active_document().unwrap();
            assert_eq!(
                *document.syntax_cache.borrow(),
                crate::editor::SyntaxLineCache::rebuild(
                    &document.buffer,
                    &highlighter::Settings {
                        token: "html".into(),
                        theme: app.settings.resolved_syntax_theme(app.system_dark),
                    }
                )
            );
        }
    }

    fn app() -> App {
        let (mut app, _) = App::new();
        app.workspace
            .insert_loaded_file("example.html", &"<p>hello</p>\n".repeat(500));
        app.workspace
            .active_document_mut()
            .unwrap()
            .scroll
            .first_visible_row = 300;
        app
    }

    #[test]
    fn rapid_typing_and_line_breaks_keep_colors_while_obsolete_work_finishes() {
        let (mut app, _) = App::new();
        app.workspace.insert_loaded_file(
            "example.rs",
            "fn main() {\n    let café = 42;\n    // comment\n    let other = \"🦀\";\n}\n",
        );
        let document = app.workspace.active_document_mut().unwrap();
        document.ensure_syntax_cache(app.settings.resolved_syntax_theme(app.system_dark));
        let position = crate::editor::EditorPosition::new(1, 8);
        document.set_main_selection(crate::editor::EditorSelection::new(position, position));
        let before = rendered_rows(&app);
        let document_id = app.workspace.active_document_id();
        let _ = app.update_editor(
            document_id,
            crate::editor::EditorAction::InsertText("x".into()),
        );
        rendered_rows(&app);
        let (old_id, old_request) = app.next_syntax_request().unwrap();
        let _ = app.update_editor(
            document_id,
            crate::editor::EditorAction::InsertText("é".into()),
        );
        let _ = app.update_editor(
            document_id,
            crate::editor::EditorAction::InsertText("\n".into()),
        );
        let split = rendered_rows(&app);
        assert!(!split[1].syntax_spans.is_empty());
        assert!(
            !split[2].syntax_spans.is_empty(),
            "split suffix keeps its colors"
        );
        for original in before.iter().filter(|row| row.line > 1) {
            assert_eq!(split[original.line + 1].syntax_spans, original.syntax_spans);
        }
        let _ = app.complete_syntax_parse(old_id, Ok(old_request.parse()));
        assert_eq!(
            rendered_rows(&app),
            split,
            "obsolete work cannot change the frame"
        );
        let _ = app.update_editor(document_id, crate::editor::EditorAction::Backspace);
        let joined = rendered_rows(&app);
        for original in before.iter().filter(|row| row.line > 1) {
            assert_eq!(joined[original.line].syntax_spans, original.syntax_spans);
        }
        while let Some((id, request)) = app.next_syntax_request() {
            let _ = app.complete_syntax_parse(id, Ok(request.parse()));
        }
        let document = app.workspace.active_document().unwrap();
        assert_eq!(
            *document.syntax_cache.borrow(),
            crate::editor::SyntaxLineCache::rebuild(
                &document.buffer,
                &highlighter::Settings {
                    token: "rs".into(),
                    theme: app.settings.resolved_syntax_theme(app.system_dark)
                }
            )
        );
    }

    #[test]
    fn scrolling_coalesces_work_and_ignores_duplicate_completions() {
        let mut app = app();
        let (id, request) = app.next_syntax_request().unwrap();
        app.workspace
            .active_document_mut()
            .unwrap()
            .scroll
            .first_visible_row = 450;
        assert!(
            app.next_syntax_request().is_none(),
            "only one batch can run"
        );
        let result = request.parse();
        let _ = app.complete_syntax_parse(id, Ok(result.clone()));
        let (next_id, next) = app.next_syntax_request().unwrap();
        let _ = app.complete_syntax_parse(id, Ok(result));
        assert_eq!(app.syntax_parsing.in_flight.as_ref().unwrap().id, next_id);
        let _ = app.complete_syntax_parse(next_id, Ok(next.parse()));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .syntax_cache
                .borrow()
                .cached_line_count(),
            0,
            "both batches should prioritize their new visible ranges before context"
        );
    }

    #[test]
    fn edits_and_theme_changes_reject_pending_work() {
        let mut app = app();
        let (id, request) = app.next_syntax_request().unwrap();
        let document = app.workspace.active_document_mut().unwrap();
        document.buffer = EditorBuffer::from_text("<script>let x = 1;</script>");
        document.refresh_after_text_change();
        let _ = app.complete_syntax_parse(id, Ok(request.parse()));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .syntax_cache
                .borrow()
                .cached_line_count(),
            0
        );
        let (id, request) = app.next_syntax_request().unwrap();
        app.settings.syntax_theme = highlighter::Theme::SolarizedDark;
        let _ = app.complete_syntax_parse(id, Ok(request.parse()));
        assert!(app.next_syntax_request().is_some());
    }

    #[test]
    fn system_color_mode_rejects_old_colors_without_changing_preset() {
        let mut app = app();
        app.settings.appearance = crate::core::AppearanceMode::System;
        app.system_dark = false;
        let family = app.settings.syntax_theme;
        let (id, request) = app.next_syntax_request().unwrap();
        let _ = app.update_settings(crate::message::SettingsMessage::SystemColorModeChanged(
            iced::theme::Mode::Dark,
        ));
        assert_eq!(app.settings.syntax_theme, family);
        assert!(app.settings_dialog.system_dark);
        let _ = app.complete_syntax_parse(id, Ok(request.parse()));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .syntax_cache
                .borrow()
                .cached_line_count(),
            0
        );
        assert!(app.next_syntax_request().is_some());
        assert!(
            app.syntax_parsing
                .in_flight
                .as_ref()
                .unwrap()
                .settings
                .theme
                .is_dark()
        );
    }

    #[test]
    fn closing_or_switching_tabs_does_not_continue_obsolete_work() {
        let mut app = app();
        let (id, request) = app.next_syntax_request().unwrap();
        let document = app.workspace.active_document_id();
        app.workspace.close(document);
        let _ = app.complete_syntax_parse(id, Ok(request.parse()));
        assert!(app.next_syntax_request().is_none());
        assert!(app.syntax_parsing.snapshot.is_none());
    }

    #[test]
    fn worker_failure_does_not_start_a_retry_loop() {
        let mut app = app();
        let (id, _) = app.next_syntax_request().unwrap();
        let _ = app.complete_syntax_parse(id, Err("worker stopped".into()));
        assert!(app.next_syntax_request().is_none());
        app.workspace
            .active_document_mut()
            .unwrap()
            .syntax_cache
            .borrow_mut()
            .invalidate_from(0);
        assert!(app.next_syntax_request().is_some());
    }

    #[test]
    fn reload_with_reused_document_revision_gets_a_fresh_snapshot() {
        let mut app = app();
        let (id, request) = app.next_syntax_request().unwrap();
        let document_id = app.workspace.active_document_id();
        let old_revision = app.workspace.active_document().unwrap().revision();
        *app.workspace.document_mut(document_id).unwrap() = crate::core::Document::from_path(
            document_id,
            "example.html",
            "<script>const replacement = 42;</script>",
        );
        assert_eq!(
            app.workspace.active_document().unwrap().revision(),
            old_revision
        );
        let _ = app.complete_syntax_parse(id, Ok(request.parse()));
        while let Some((id, request)) = app.next_syntax_request() {
            let _ = app.complete_syntax_parse(id, Ok(request.parse()));
        }
        let document = app.workspace.active_document().unwrap();
        let expected = crate::editor::SyntaxLineCache::rebuild(
            &document.buffer,
            &highlighter::Settings {
                token: "html".into(),
                theme: app.settings.resolved_syntax_theme(app.system_dark),
            },
        );
        assert_eq!(*document.syntax_cache.borrow(), expected);
    }

    #[test]
    fn completion_during_shutdown_does_not_leave_a_stuck_worker_after_failed_exit() {
        let mut app = app();
        let (id, request) = app.next_syntax_request().unwrap();
        app.lifecycle.begin_shutdown();
        let _ = app.update(Message::SyntaxParsed(id, Ok(request.parse())));
        assert_eq!(app.syntax_parsing.in_flight.as_ref().unwrap().id, id);
        let _ = app.update(Message::ShutdownPersisted(Err("disk unavailable".into())));
        assert!(!app.lifecycle.is_exiting());
        assert_ne!(app.syntax_parsing.in_flight.as_ref().unwrap().id, id);
    }

    #[test]
    fn syntax_results_do_not_dirty_session_state() {
        let mut app = app();
        let (id, request) = app.next_syntax_request().unwrap();
        let _ = app.update(Message::None); // Commit fixture mutations before observing session dirtiness.
        app.session.set_enabled(true);
        let _ = app.update(Message::SyntaxParsed(id, Ok(request.parse())));
        assert!(!app.session.is_dirty());
    }
}
