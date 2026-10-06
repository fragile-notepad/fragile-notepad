//! Constructs the application and its initial tasks.

use super::{App, analysis, lifecycle, outline, rendering, session, settings, syntax, windowing};
use super::{animation::ChromeAnimation, close_prompt::ClosePrompt};
use crate::core::{EditorSettings, FindState, Workspace};
use crate::message::{AboutTab, Message};
use crate::search_dialog::SearchDialogState;
use crate::settings_dialog::SettingsDialogState;
use crate::ui;
use iced::{Task, keyboard, window};
use std::collections::HashMap;

impl App {
    pub(super) fn bootstrap(options: crate::startup::StartupOptions) -> (Self, Task<Message>) {
        let (main_window_id, open) =
            window::open(windowing::operations::custom_chrome(window::Settings {
                min_size: Some(iced::Size::new(640.0, 400.0)),
                exit_on_close_request: false,
                ..window::Settings::default()
            }));

        let mut app = Self {
            workspace: Workspace::new(),
            events: super::bus::MessageBus::default(),
            pending_work: super::events::PendingWork::default(),
            files: super::files::FileOperations::default(),
            find: FindState::new(),
            settings: EditorSettings::default(),
            system_dark: false,
            outline_parsing: outline::OutlineParsing::new(),
            syntax_parsing: syntax::SyntaxParsing::default(),
            close_prompt: ClosePrompt::new(),
            file_status: None,
            is_find_visible: false,
            is_inline_replace_visible: false,
            is_function_list_visible: false,
            function_list_query: String::new(),
            main_window_id: Some(main_window_id),
            settings_window: None,
            advanced_search_window: None,
            menu: super::menu::MenuState::default(),
            is_about_visible: false,
            about_tab: AboutTab::About,
            is_window_list_visible: false,
            settings_dialog: SettingsDialogState::new(&EditorSettings::default()),
            search_dialog: SearchDialogState::new(),
            go_to_line_prompt: None,
            dragged_tab: None,
            hovered_drop_tab: None,
            keyboard_modifiers: keyboard::Modifiers::default(),
            focused_window_id: Some(main_window_id),
            maximized_windows: HashMap::new(),
            title_bar_style: ui::title_bar::ControlStyle::startup(),
            rendering: rendering::RenderingState::Software,
            chrome_animation: ChromeAnimation::new(),
            session: session::SessionState::new(options),
            lifecycle: lifecycle::Lifecycle::default(),
            settings_persistence: settings::SettingsPersistence::default(),
            analysis: analysis::DocumentAnalysisState::default(),
            loading_find_scheduled: false,
            pending_search: None,
        };

        app.events.publish(super::events::Event::Started);
        let initial_work = super::update::drain(&mut app);

        let session_task = if app.session.is_enabled() {
            Task::perform(
                crate::services::session_store::load_session(),
                Message::SessionLoaded,
            )
        } else {
            Task::none()
        };
        (
            app,
            Task::batch([
                open.map(Message::WindowOpened),
                iced::system::theme().map(Message::SystemColorModeChanged),
                iced::widget::operation::focus(crate::ui::editor::EDITOR_ID),
                Task::perform(
                    crate::services::settings_store::load_settings(),
                    Message::SettingsLoaded,
                ),
                initial_work,
                session_task,
            ]),
        )
    }
}
