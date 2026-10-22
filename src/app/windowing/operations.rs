//! Application window operations and focus management.

use crate::message::WindowMessage;
use iced::{Task, window};

use super::managed::{AdvancedSearchWindow, ManagedWindow, SettingsWindow};
use super::platform_activation;
use crate::app::App;
use crate::ipc::ActivationRequest;
use crate::message::{Message, WindowTarget};
use crate::ui::toolbar::WindowMenuState;
use crate::ui::window_list_dialog::WindowListEntry;

pub(crate) fn custom_chrome(mut settings: window::Settings) -> window::Settings {
    settings.decorations = !crate::ui::title_bar::SUPPORTED;
    settings.icon = Some(
        window::icon::from_rgba(crate::assets::APP_ICON_RGBA.to_vec(), 256, 256)
            .expect("generated application icon has valid RGBA dimensions"),
    );
    #[cfg(windows)]
    {
        settings.platform_specific.undecorated_shadow = true;
    }
    #[cfg(target_os = "macos")]
    {
        // A borderless NSWindow has square corners. Retain AppKit's native
        // frame and let our caption occupy its full content area, so macOS
        // supplies corner clipping, shadows, resizing, and fullscreen behavior.
        settings.decorations = true;
        settings.platform_specific.title_hidden = true;
        settings.platform_specific.titlebar_buttons_hidden = true;
        settings.platform_specific.titlebar_transparent = true;
        settings.platform_specific.fullsize_content_view = true;
    }
    settings
}

impl App {
    pub(crate) fn update_window(&mut self, message: WindowMessage) -> Task<Message> {
        match message {
            WindowMessage::NewWindow => self.new_editor_window(),
            WindowMessage::CloseWindow => {
                self.close_editor_request(self.workspace.current_window_id())
            }
            WindowMessage::Quit => self.exit_request(),
            WindowMessage::MoveTabsToNewWindow => self.move_selected_to_new_window(),
            WindowMessage::MoveTabsToWindow(id) => self.move_selected_to_window(id),
            WindowMessage::WindowPlacementChanged(id) => self.capture_window_geometry(id),
            WindowMessage::WindowOpenFailed(id, error) => {
                if self.settings_window.is_some_and(|window| window.is(id)) {
                    self.settings_window = None;
                }
                if self
                    .advanced_search_window
                    .is_some_and(|window| window.is(id))
                {
                    self.advanced_search_window = None;
                    self.advanced_search_owner = None;
                    self.pending_search = None;
                }
                self.editor_open_failed(id, error)
            }
            WindowMessage::WindowGeometry(id, point, size) => {
                let mut changed = false;
                if let Some(editor) = self.editor_id(id)
                    && let Some(state) = self.editor_windows.get_mut(&editor)
                    && !self.maximized_windows.get(&id).copied().unwrap_or(false)
                    && !state.fullscreen
                {
                    let bounds = Some([
                        point.map_or(0.0, |p| p.x),
                        point.map_or(0.0, |p| p.y),
                        size.width,
                        size.height,
                    ]);
                    changed = state.bounds != bounds;
                    state.bounds = bounds;
                }
                if changed {
                    self.request_session_save()
                } else {
                    Task::none()
                }
            }
            WindowMessage::WindowOpened(id) => self.window_opened(id),
            WindowMessage::WindowMaximized(id, maximized) => {
                if self.owns_window(id)
                    && self.maximized_windows.insert(id, maximized) != Some(maximized)
                    && self.editor_id(id).is_some()
                {
                    return self.request_session_save();
                }
                Task::none()
            }
            WindowMessage::WindowChrome(id, action) => {
                use crate::ui::title_bar::Action;
                if !self.owns_window(id) {
                    return Task::none();
                }
                match action {
                    Action::Drag => window::drag(id),
                    Action::Resize(direction) => window::drag_resize(id, direction),
                    Action::Minimize => window::minimize(id, true),
                    Action::ToggleMaximize => {
                        window::toggle_maximize(id).chain(self.refresh_window_state(id))
                    }
                    Action::Close => self.update_window(WindowMessage::WindowCloseRequested(id)),
                    Action::SystemMenu => window::show_system_menu(id),
                }
            }
            WindowMessage::WindowCloseRequested(id) => {
                if let Some(settings_window) = &self.settings_window
                    && settings_window.is(id)
                {
                    settings_window.close_requested(&mut self.settings_dialog, &self.settings)
                } else if let Some(search_window) = &self.advanced_search_window
                    && search_window.is(id)
                {
                    search_window.close_requested()
                } else if let Some(editor) = self.editor_id(id) {
                    self.close_editor_request(editor)
                } else {
                    Task::none()
                }
            }
            WindowMessage::WindowClosed(id) => {
                self.maximized_windows.remove(&id);
                if self.focused_window_id == Some(id) {
                    self.focused_window_id = None;
                }
                if self
                    .settings_window
                    .as_ref()
                    .is_some_and(|settings_window| settings_window.is(id))
                {
                    self.settings_window = None;
                    self.settings_dialog.reset_from(&self.settings);
                    Task::none()
                } else if self
                    .advanced_search_window
                    .as_ref()
                    .is_some_and(|search_window| search_window.is(id))
                {
                    self.advanced_search_window = None;
                    Task::none()
                } else if let Some(editor) = self.editor_id(id) {
                    if self.workspace.window_tab_ids(editor).is_empty() {
                        self.remove_editor_window(editor);
                        Task::none()
                    } else {
                        // Unexpected destruction must not strand live buffers
                        // in an editor that can no longer be activated.
                        self.editor_open_failed(id, "The native window closed unexpectedly.".into())
                    }
                } else {
                    Task::none()
                }
            }
        }
    }

    pub(crate) fn register_opened_window(&mut self, id: window::Id) -> Task<Message> {
        if self.owns_window(id) {
            self.maximized_windows.entry(id).or_insert(false);
        }
        Task::none()
    }

    pub(in crate::app) fn owns_window(&self, id: window::Id) -> bool {
        self.editor_id(id).is_some()
            || self.settings_window.is_some_and(|window| window.is(id))
            || self
                .advanced_search_window
                .is_some_and(|window| window.is(id))
    }

    pub(crate) fn refresh_window_state(&self, id: window::Id) -> Task<Message> {
        if self.owns_window(id) {
            window::is_maximized(id).map(move |maximized| Message::WindowMaximized(id, maximized))
        } else {
            Task::none()
        }
    }

    pub(crate) fn show_main_window(&mut self, request: ActivationRequest) -> Task<Message> {
        self.menu.close();

        let open = if self.editor_windows.is_empty() {
            self.new_editor_window()
        } else {
            Task::none()
        };
        if let Some(id) = self.last_active_editor {
            self.switch_editor(id);
        }
        let Some(main_window_id) = self.main_window_id else {
            return open;
        };

        platform_activation::prepare_for_presentation(&request);
        open.chain(window::minimize(main_window_id, false))
            .chain(window::gain_focus(main_window_id))
            .chain(window::request_user_attention(
                main_window_id,
                Some(window::UserAttention::Informational),
            ))
    }

    pub(crate) fn toggle_settings_window(&mut self) -> Task<Message> {
        self.menu.close();
        if let Some(settings_window) = self.settings_window {
            return iced::window::gain_focus(settings_window.id());
        }

        self.settings_dialog.reset_from(&self.settings);
        let (id, open) = iced::window::open(SettingsWindow::settings());
        self.settings_window = Some(SettingsWindow::new(id));

        open.map(Message::WindowOpened)
    }

    pub(crate) fn close_settings_window(&mut self) -> Task<Message> {
        self.settings_window
            .map(|settings_window| iced::window::close(settings_window.id()))
            .unwrap_or_else(Task::none)
    }

    pub(crate) fn open_advanced_search_window(&mut self) -> Task<Message> {
        self.advanced_search_owner = Some(self.workspace.current_window_id());
        if let Some(search_window) = self.advanced_search_window {
            return iced::window::gain_focus(search_window.id());
        }

        let (id, open) = iced::window::open(AdvancedSearchWindow::settings());
        self.advanced_search_window = Some(AdvancedSearchWindow::new(id));

        open.map(Message::WindowOpened)
    }

    pub(crate) fn close_advanced_search_window(&mut self) -> Task<Message> {
        self.advanced_search_window
            .map(|search_window| iced::window::close(search_window.id()))
            .unwrap_or_else(Task::none)
    }

    pub(crate) fn window_menu_state_for(&self, source: window::Id) -> WindowMenuState {
        WindowMenuState {
            open_window_count: self.open_window_targets().len(),
            editor_windows: self
                .workspace
                .window_ids()
                .into_iter()
                .filter_map(|id| {
                    let native = self.editor_windows.get(&id)?.native;
                    (native != source).then(|| (native, self.title(native)))
                })
                .collect(),
        }
    }

    pub(crate) fn window_list_entries(&self) -> Vec<WindowListEntry> {
        let focused = self.focused_window_target();

        self.open_window_targets()
            .into_iter()
            .filter_map(|target| {
                let id = self.window_id(target)?;
                let (title, description, search_titles) = if let Some(editor) = self.editor_id(id) {
                    let active = self.workspace.active_document_in(editor);
                    let titles: Vec<_> = active
                        .into_iter()
                        .chain(self.workspace.window_documents(editor).filter(|document| {
                            Some(document.id) != active.map(|document| document.id)
                        }))
                        .map(|document| document.title())
                        .collect();
                    let mut title = titles
                        .iter()
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    if titles.len() > 2 {
                        title.push_str(&format!(" and {} more…", titles.len() - 2));
                    }
                    if title.is_empty() {
                        title = "Empty editor".into();
                    }
                    let count = titles.len();
                    let mut description = format!(
                        "Editor · {count} {}",
                        if count == 1 { "tab" } else { "tabs" }
                    );
                    if let Some(parent) = active
                        .and_then(|document| document.path.as_deref())
                        .and_then(|path| path.parent())
                    {
                        description.push_str(&format!(" · {}", parent.display()));
                    }
                    (title, description, titles)
                } else {
                    let title = self
                        .title(id)
                        .trim_end_matches(" - Fragile Notepad")
                        .to_owned();
                    let description = match target {
                        WindowTarget::Settings => "Preferences",
                        WindowTarget::AdvancedSearch => "Find & Replace",
                        _ => "Editor",
                    };
                    (title, description.into(), Vec::new())
                };
                Some(WindowListEntry {
                    target,
                    title,
                    description,
                    search_titles,
                    is_focused: focused == Some(target),
                })
            })
            .collect()
    }

    pub(crate) fn focus_window(&mut self, target: WindowTarget) -> Task<Message> {
        self.menu.close();

        let restore_focus = if self.is_tab_list_visible {
            self.set_tab_list_visible(false)
        } else {
            self.set_window_list_visible(false)
        };

        let Some(id) = self.window_id(target) else {
            return restore_focus;
        };

        self.focused_window_id = Some(id);
        Task::batch([
            restore_focus,
            iced::window::minimize(id, false).chain(iced::window::gain_focus(id)),
        ])
    }

    pub(crate) fn focus_adjacent_window(&mut self, direction: isize) -> Task<Message> {
        self.menu.close();

        let targets = self.open_window_targets();
        if targets.len() < 2 {
            return Task::none();
        }

        let current = self
            .focused_window_target()
            .and_then(|target| targets.iter().position(|candidate| *candidate == target))
            .unwrap_or(0);
        let next = if direction.is_negative() {
            current
                .checked_sub(direction.unsigned_abs())
                .unwrap_or_else(|| targets.len() - 1)
        } else {
            (current + direction as usize) % targets.len()
        };

        self.focus_window(targets[next])
    }

    fn open_window_targets(&self) -> Vec<WindowTarget> {
        let mut targets = Vec::new();

        for id in self.workspace.window_ids() {
            if let Some(window) = self.editor_windows.get(&id) {
                targets.push(WindowTarget::Editor(window.native));
            }
        }
        if self.settings_window.is_some() {
            targets.push(WindowTarget::Settings);
        }
        if self.advanced_search_window.is_some() {
            targets.push(WindowTarget::AdvancedSearch);
        }

        targets
    }

    fn focused_window_target(&self) -> Option<WindowTarget> {
        let id = self.focused_window_id?;

        self.open_window_targets()
            .into_iter()
            .find(|target| self.window_id(*target) == Some(id))
    }

    fn window_id(&self, target: WindowTarget) -> Option<iced::window::Id> {
        match target {
            WindowTarget::Main => self.main_window_id,
            WindowTarget::Editor(id) => self.editor_id(id).map(|_| id),
            WindowTarget::Settings => self.settings_window.map(|window| window.id()),
            WindowTarget::AdvancedSearch => self.advanced_search_window.map(|window| window.id()),
        }
    }
}
