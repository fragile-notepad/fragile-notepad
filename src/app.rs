//! Application state and Iced entry points.
//!
//! Features own their workflow state. `update` wires typed event subscribers,
//! `routing` dispatches typed messages, and `lifecycle` controls admission.

use iced::{Subscription, Task, keyboard, window};

use crate::core::{EditorSettings, FindState, Workspace};
use crate::message::{AboutTab, Message};
use crate::search_dialog::SearchDialogState;
use crate::settings_dialog::SettingsDialogState;
use crate::ui;

use std::collections::HashMap;

use animation::ChromeAnimation;
use close_prompt::ClosePrompt;
use files::CloseGoal;

use windowing::managed::{AdvancedSearchWindow, SettingsWindow};

mod analysis;
mod animation;
mod bootstrap;
mod bus;
mod chrome;
mod close_prompt;
mod editor;
mod editor_ops;
mod events;
mod files;
mod focus;
mod go_to_line;
mod lifecycle;
#[cfg(target_os = "macos")]
mod macos;
mod menu;
mod outline;
mod presentation;
mod rendering;
mod routing;
mod scheduler;
mod search;
mod session;
mod settings;
mod shortcuts;
mod subscriptions;
mod syntax;
mod update;
mod windowing;

pub use subscriptions::register_single_instance;

/// Apply the application identity before AppKit creates the menu bar.
pub fn initialize_platform() {
    #[cfg(target_os = "macos")]
    macos::initialize_name();
}

#[derive(Debug)]
pub struct App {
    editor_windows: HashMap<crate::core::EditorWindowId, windowing::editors::EditorWindow>,
    last_active_editor: Option<crate::core::EditorWindowId>,
    advanced_search_owner: Option<crate::core::EditorWindowId>,
    tab_drag: Option<windowing::tabs::TabDragSession>,
    next_drag_session: u64,
    pending_drag_gpu_boost: bool,
    tab_geometry: HashMap<window::Id, crate::ui::tabs::StripGeometry>,
    workspace: Workspace,
    events: bus::MessageBus<events::Event>,
    pending_work: events::PendingWork,
    files: files::FileOperations,
    find: FindState,
    settings: EditorSettings,
    system_dark: bool,
    system_theme_loaded: bool,
    outline_parsing: outline::OutlineParsing,
    syntax_parsing: syntax::SyntaxParsing,
    close_prompt: ClosePrompt,
    file_status: Option<String>,
    is_find_visible: bool,
    is_inline_replace_visible: bool,
    is_function_list_visible: bool,
    function_list_query: String,
    main_window_id: Option<window::Id>,
    settings_window: Option<SettingsWindow>,
    advanced_search_window: Option<AdvancedSearchWindow>,
    menu: menu::MenuState,
    is_about_visible: bool,
    about_tab: AboutTab,
    is_window_list_visible: bool,
    window_list_query: String,
    is_tab_list_visible: bool,
    tab_list_query: String,
    selector_close_action: Option<chrome::SelectorCloseAction>,
    settings_dialog: SettingsDialogState,
    search_dialog: SearchDialogState,
    go_to_line_prompt: Option<go_to_line::GoToLinePrompt>,
    dragged_tab: Option<crate::core::DocumentId>,
    hovered_drop_tab: Option<crate::core::DocumentId>,
    keyboard_modifiers: keyboard::Modifiers,
    focused_window_id: Option<window::Id>,
    maximized_windows: HashMap<window::Id, bool>,
    title_bar_style: ui::title_bar::ControlStyle,
    rendering: rendering::RenderingState,
    chrome_animation: ChromeAnimation,
    session: session::SessionState,
    lifecycle: lifecycle::Lifecycle,
    settings_persistence: settings::SettingsPersistence,
    analysis: analysis::DocumentAnalysisState,
    loading_find_scheduled: bool,
    pending_search: Option<search::PendingSearch>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        Self::new_with_options(crate::startup::StartupOptions {
            files: Vec::new(),
            restore_session: false,
        })
    }

    pub fn new_with_options(options: crate::startup::StartupOptions) -> (Self, Task<Message>) {
        Self::bootstrap(options)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let Some(mut message) = self.lifecycle.admit(message) else {
            return Task::none();
        };
        let mut origin_native = None;
        while let Message::ForWindow(id, inner) = message {
            origin_native = Some(id);
            message = *inner;
        }
        let origin = origin_native.and_then(|id| {
            self.editor_id(id).or_else(|| {
                use windowing::managed::ManagedWindow;
                self.advanced_search_window
                    .filter(|window| window.id() == id)
                    .and(self.advanced_search_owner)
            })
        });
        let command_editor = self.message_editor(&message).or(origin);
        if origin_native.is_some_and(|native| !self.owns_window(native))
            && command_editor.is_none()
            && (message.shutdown_delivery() == crate::message::ShutdownDelivery::Reject
                || matches!(
                    &message,
                    Message::RefreshLoadingFind | Message::FilePicked(_)
                ))
        {
            return Task::none();
        }
        if let Some(id) = command_editor {
            self.switch_editor(id);
        }
        let cancel = if self.tab_drag.is_some()
            && matches!(
                &message,
                Message::NewFile
                    | Message::NewWindow
                    | Message::TabSelected(_)
                    | Message::TabListSelected(_)
                    | Message::TabClosed(_)
                    | Message::TabPinToggled(_)
                    | Message::TabContextAction(_, _)
                    | Message::CloseFile
                    | Message::CloseAllFiles
                    | Message::CloseAllButActiveFile
                    | Message::CloseAllButPinnedFiles
                    | Message::CloseAllToLeft
                    | Message::CloseAllToRight
                    | Message::CloseAllUnchanged
                    | Message::CloseScoped(_)
                    | Message::OpenPaths(_)
                    | Message::ForwardedFiles(_, _, _)
                    | Message::FilePicked(_)
                    | Message::FileOpened(_)
                    | Message::WindowCloseRequested(_)
                    | Message::WindowClosed(_)
                    | Message::CloseWindow
                    | Message::Quit
            ) {
            self.cancel_tab_drag()
        } else {
            Task::none()
        };
        if let Some(id) = command_editor {
            self.switch_editor(id);
        }
        let native = origin_native
            .filter(|id| self.editor_id(*id).is_none())
            .or(self.main_window_id);
        let task = update::run(self, message);
        #[cfg(target_os = "macos")]
        macos::sync(self);
        let task = if let Some(id) = native {
            task.scope_window(id)
                .map(move |message| Message::ForWindow(id, Box::new(message)))
        } else {
            task
        };
        Task::batch([cancel, task, self.schedule_visible_editors()])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        self.subscriptions()
    }
}

#[cfg(test)]
mod tests;
