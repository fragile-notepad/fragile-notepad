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
mod go_to_line;
mod lifecycle;
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

#[derive(Debug)]
pub struct App {
    workspace: Workspace,
    events: bus::MessageBus<events::Event>,
    pending_work: events::PendingWork,
    files: files::FileOperations,
    find: FindState,
    settings: EditorSettings,
    system_dark: bool,
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
        update::run(self, message)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        self.subscriptions()
    }
}

#[cfg(test)]
mod tests;
