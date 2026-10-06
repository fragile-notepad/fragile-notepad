//! Explicit bus wiring. Subscribers never receive App or call each other.

use super::{
    App,
    events::{Event, Work},
    scheduler::UpdateScheduler,
    search,
};
use crate::message::Message;
use iced::Task;
use std::sync::OnceLock;

fn order() -> &'static [Work] {
    static ORDER: OnceLock<Vec<Work>> = OnceLock::new();
    ORDER.get_or_init(|| {
        use Work::*;
        let mut scheduler = UpdateScheduler::new();
        scheduler.register(
            Session,
            &[Search, Find, Outline, Analysis, Syntax, LoadingFind],
        );
        scheduler.register(Syntax, &[Analysis]);
        scheduler.register(Analysis, &[Search]);
        scheduler.register(Outline, &[Search]);
        scheduler.register(LoadingFind, &[Find]);
        scheduler.register(Find, &[Search]);
        scheduler.register(Search, &[Files]);
        scheduler.register(Files, &[]);
        scheduler
            .finish()
            .expect("subscriber dependencies must be complete and acyclic")
    })
}

pub(super) fn run(app: &mut App, message: Message) -> Task<Message> {
    let Some(message) = app.lifecycle.admit(message) else {
        return Task::none();
    };
    let span = crate::perf_trace::span("app_update", format_args!("{message:?}"));
    let task = app.dispatch(message);
    let reactions = drain(app);
    if let Some(span) = span {
        span.end_with("");
    }
    Task::batch([task, reactions])
}

/// Drain facts before running a reaction, then collect any mutations it produced.
/// Work is coalesced within an update and retained during exit. Each replayed
/// background message gets its own update boundary.
pub(super) fn drain(app: &mut App) -> Task<Message> {
    let mut tasks = Vec::new();
    loop {
        app.workspace
            .publish_changes(|event| app.events.publish(Event::Workspace(event)));
        if app.settings_persistence.take_changed() {
            app.events.publish(Event::SettingsChanged);
        }
        while let Some(event) = app.events.pop() {
            let active = app.workspace.active_document_id();
            super::settings::apply_to_workspace(event, &app.settings, &mut app.workspace);
            app.files.observe(event, &mut app.pending_work);
            app.session
                .observe(event, &mut app.workspace, &mut app.pending_work);
            search::observe(event, active, &mut app.pending_work);
            app.outline_parsing
                .observe(event, active, &mut app.pending_work);
            app.analysis.observe(event, active, &mut app.pending_work);
            app.syntax_parsing
                .observe(event, active, &mut app.pending_work);
        }
        app.workspace
            .publish_changes(|event| app.events.publish(Event::Workspace(event)));
        if !app.events.is_empty() {
            continue;
        }
        if app.lifecycle.is_exiting() {
            break;
        }
        let Some(&work) = order().iter().find(|&&work| app.pending_work.take(work)) else {
            break;
        };
        match work {
            Work::Files => app.files.refresh_loading_state(&app.workspace),
            Work::Search => tasks.push(
                search::SearchSubscriber {
                    workspace: &mut app.workspace,
                    pending: &mut app.pending_search,
                    find: &mut app.find,
                    dialog: &mut app.search_dialog,
                }
                .resume(),
            ),
            Work::Find => search::refresh_matches(&mut app.find, &app.workspace),
            Work::LoadingFind => tasks.push(search::schedule_loading_find(
                &app.find,
                &mut app.loading_find_scheduled,
            )),
            Work::Outline => {
                if let Some(document) = app.workspace.active_document() {
                    tasks.push(
                        app.outline_parsing
                            .schedule(document)
                            .map(Message::OutlineParseCompleted),
                    );
                }
            }
            Work::Analysis => {
                if let Some(task) = app.analysis.schedule(app.workspace.active_document()) {
                    tasks.push(task.map(Message::DocumentAnalyzed));
                }
            }
            Work::Syntax => {
                if let Some(task) = app.syntax_parsing.schedule(
                    app.workspace.active_document(),
                    app.settings.resolved_syntax_theme(app.system_dark),
                ) {
                    tasks.push(task);
                }
            }
            Work::Session => tasks.push(app.session.request_save(false)),
        }
    }
    Task::batch(tasks)
}
