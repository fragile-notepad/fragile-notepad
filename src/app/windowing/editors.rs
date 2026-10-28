//! Window-local presentation is exchanged at an update boundary. This lets the
//! existing feature handlers borrow disjoint App fields while every inactive
//! editor retains its own state. Native focus never determines command routing.
use crate::app::{App, animation::ChromeAnimation, close_prompt::ClosePrompt, menu::MenuState};
use crate::core::{EditorWindowId, FindState};
use crate::message::{AboutTab, Message};
use iced::{Task, window};
use std::collections::HashMap;

#[derive(Debug)]
pub(in crate::app) struct EditorWindow {
    pub native: window::Id,
    pub ui: Option<WindowUi>,
    pub bounds: Option<[f32; 4]>,
    pub fullscreen: bool,
}

macro_rules! local_state {
    (view { $($field:ident: $ty:ty = $initial:expr,)* }
     runtime { $($internal:ident: $internal_ty:ty = $internal_initial:expr,)* }) => {
        #[derive(Debug)]
        pub(in crate::app) struct WindowUi {
            $(pub $field: $ty,)*
            $(pub $internal: $internal_ty,)*
        }
        pub(in crate::app) struct WindowUiRef<'a> { $(pub $field: &'a $ty,)* }
        impl WindowUi {
            pub fn new() -> Self { Self { $($field: $initial,)* $($internal: $internal_initial,)* } }
            fn exchange(self, app: &mut App) -> Self {
                Self {
                    $($field: std::mem::replace(&mut app.$field, self.$field),)*
                    $($internal: std::mem::replace(&mut app.$internal, self.$internal),)*
                }
            }
            fn as_ref(&self) -> WindowUiRef<'_> { WindowUiRef { $($field: &self.$field,)* } }
        }
        impl App {
            pub(in crate::app) fn window_ui(&self, id: EditorWindowId) -> Option<WindowUiRef<'_>> {
                if id == self.workspace.current_window_id() {
                    Some(WindowUiRef { $($field: &self.$field,)* })
                } else { self.editor_windows.get(&id)?.ui.as_ref().map(WindowUi::as_ref) }
            }
        }
    }
}

local_state! {
    view {
        find: FindState = FindState::new(),
        close_prompt: ClosePrompt = ClosePrompt::new(),
        file_status: Option<String> = None,
        is_find_visible: bool = false,
        is_inline_replace_visible: bool = false,
        is_function_list_visible: bool = false,
        function_list_query: String = String::new(),
        menu: MenuState = MenuState::default(),
        about_tab: AboutTab = AboutTab::About,
        is_window_list_visible: bool = false,
        window_list_query: String = String::new(),
        is_tab_list_visible: bool = false,
        tab_list_query: String = String::new(),
        go_to_line_prompt: Option<crate::app::go_to_line::GoToLinePrompt> = None,
        hovered_drop_tab: Option<crate::core::DocumentId> = None,
        chrome_animation: ChromeAnimation = ChromeAnimation::new(),
    }
    runtime {
        selector_close_action: Option<crate::app::chrome::SelectorCloseAction> = None,
        is_about_visible: bool = false,
        dragged_tab: Option<crate::core::DocumentId> = None,
        keyboard_modifiers: iced::keyboard::Modifiers = iced::keyboard::Modifiers::default(),
        loading_find_scheduled: bool = false,
        pending_work: crate::app::events::PendingWork = crate::app::events::PendingWork::default(),
    }
}

impl App {
    pub(in crate::app) fn editor_id(&self, native: window::Id) -> Option<EditorWindowId> {
        self.editor_windows
            .iter()
            .find_map(|(id, w)| (w.native == native).then_some(*id))
    }

    pub(in crate::app) fn switch_editor(&mut self, id: EditorWindowId) -> bool {
        let previous = self.workspace.current_window_id();
        if previous == id {
            return self.editor_windows.contains_key(&id);
        }
        let Some(next) = self.editor_windows.get_mut(&id).and_then(|w| w.ui.take()) else {
            return false;
        };
        let previous_ui = next.exchange(self);
        if let Some(window) = self.editor_windows.get_mut(&previous) {
            window.ui = Some(previous_ui);
        }
        self.workspace.set_current_window(id);
        self.main_window_id = self.editor_windows.get(&id).map(|w| w.native);
        true
    }

    pub(in crate::app) fn create_editor(
        &mut self,
        empty: bool,
        settings: Option<window::Settings>,
    ) -> (EditorWindowId, Task<Message>) {
        let was_empty = self.editor_windows.is_empty();
        let previous = self.workspace.current_window_id();
        let id = if empty {
            self.workspace.create_empty_window()
        } else {
            self.workspace.create_window()
        };
        self.workspace.set_current_window(previous);
        let settings = settings.unwrap_or_else(|| window::Settings {
            min_size: Some(iced::Size::new(640.0, 400.0)),
            exit_on_close_request: false,
            ..window::Settings::default()
        });
        let (native, open) = window::open(super::operations::custom_chrome(settings));
        self.editor_windows.insert(
            id,
            EditorWindow {
                native,
                ui: Some(WindowUi::new()),
                bounds: None,
                fullscreen: false,
            },
        );
        if was_empty {
            let ui = self.editor_windows.get_mut(&id).unwrap().ui.take().unwrap();
            let _ = ui.exchange(self);
            self.workspace.set_current_window(id);
            self.main_window_id = Some(native);
        }
        (id, open.map(Message::WindowOpened))
    }

    pub(in crate::app) fn new_editor_window(&mut self) -> Task<Message> {
        // Keyboard and IPC handlers can call this without App::update's guard.
        let cancel = self.cancel_tab_drag();
        let (id, open) = self.create_editor(false, None);
        self.switch_editor(id);
        self.last_active_editor = Some(id);
        self.apply_initial_new_file_defaults();
        Task::batch([cancel, open])
    }

    pub(in crate::app) fn message_editor(&self, message: &Message) -> Option<EditorWindowId> {
        let native = match message {
            Message::ForWindow(native, _)
            | Message::RuntimeEvent(_, _, native)
            | Message::WindowOpened(native)
            | Message::WindowClosed(native)
            | Message::WindowCloseRequested(native)
            | Message::WindowMaximized(native, _)
            | Message::WindowGeometry(native, _, _)
            | Message::WindowChrome(native, _) => Some(*native),
            Message::WindowPlacementChanged(native) => Some(*native),
            Message::WindowOpenFailed(native, _) => Some(*native),
            _ => None,
        };
        let document = match message {
            Message::EditorAction(id, _)
            | Message::TabSelected(id)
            | Message::TabClosed(id)
            | Message::ResumeDocumentClose(id)
            | Message::TabPinToggled(id)
            | Message::TabContextAction(id, _)
            | Message::DirtyCloseResolved(id, _)
            | Message::DirtyCloseFadeFinished(id)
            | Message::AdvancedSearchResultSelected(id, _) => Some(*id),
            Message::FileLoadProgress(value) => Some(value.document_id),
            Message::FileLoadChunk(value) => Some(value.document_id),
            Message::FileLoadFinished(result) => Some(match result {
                Ok(value) => value.document_id,
                Err(value) => value.document_id,
            }),
            Message::ClipboardRead(request, _) => Some(request.document_id),
            Message::FileSaved(value, _) | Message::FileCopySaved(value, _) => {
                Some(value.document_id)
            }
            Message::DocumentAnalyzed(value) => Some(value.document_id),
            Message::OutlineParseCompleted(value) => Some(value.document_id),
            Message::FileCheckedForClose(value) => Some(value.document_id),
            _ => None,
        };
        document
            .and_then(|id| self.workspace.owner_of(id))
            .or_else(|| {
                native.and_then(|id| {
                    self.editor_id(id).or_else(|| {
                        self.advanced_search_window
                            .filter(|w| {
                                use super::managed::ManagedWindow;
                                w.id() == id
                            })
                            .and(self.advanced_search_owner)
                    })
                })
            })
    }

    pub(in crate::app) fn schedule_visible_editors(&mut self) -> Task<Message> {
        if self.lifecycle.is_exiting() {
            return Task::none();
        }
        let original = self.workspace.current_window_id();
        let mut tasks = Vec::new();
        for id in self.workspace.window_ids() {
            if !self.editor_windows.contains_key(&id) {
                continue;
            }
            self.switch_editor(id);
            let mut work = vec![crate::app::update::drain(self)];
            if let Some(document) = self.workspace.active_document() {
                if let Some(task) = self.analysis.schedule(Some(document)) {
                    work.push(task.map(Message::DocumentAnalyzed));
                }
                if let Some(task) = self.syntax_parsing.schedule(
                    Some(document),
                    self.settings.resolved_syntax_theme(self.system_dark),
                ) {
                    work.push(task);
                }
                work.push(
                    self.outline_parsing
                        .schedule(document)
                        .map(Message::OutlineParseCompleted),
                );
            }
            if let Some(native) = self.main_window_id {
                tasks.push(
                    Task::batch(work)
                        .scope_window(native)
                        .map(move |message| Message::ForWindow(native, Box::new(message))),
                );
            }
        }
        self.switch_editor(original);
        Task::batch(tasks)
    }

    pub(in crate::app) fn invalidate_other_editors(&mut self, event: crate::app::events::Event) {
        for (id, window) in &mut self.editor_windows {
            let Some(ui) = &mut window.ui else {
                continue;
            };
            let Some(active) = self.workspace.active_document_in(*id).map(|d| d.id) else {
                continue;
            };
            crate::app::search::observe(event, active, &mut ui.pending_work);
            self.analysis.observe(event, active, &mut ui.pending_work);
            self.syntax_parsing
                .observe(event, active, &mut ui.pending_work);
            self.outline_parsing
                .observe(event, active, &mut ui.pending_work);
        }
    }

    pub(in crate::app) fn scoped_document_ids(
        &self,
        scope: crate::message::BulkScope,
    ) -> Vec<crate::core::DocumentId> {
        use crate::message::BulkScope;
        match scope {
            BulkScope::SelectedTabs => self
                .workspace
                .window(self.workspace.current_window_id())
                .map(|w| w.selected_tabs.clone())
                .unwrap_or_default(),
            BulkScope::ThisWindow => self.workspace.document_ids(),
            BulkScope::AllWindows => self.workspace.all_document_ids(),
        }
    }

    pub(in crate::app) fn capture_window_geometry(&self, native: window::Id) -> Task<Message> {
        if self.editor_id(native).is_none() {
            return Task::none();
        }
        window::position(native).then(move |position| {
            window::size(native).map(move |size| Message::WindowGeometry(native, position, size))
        })
    }

    pub(in crate::app) fn remove_editor_window(&mut self, id: EditorWindowId) {
        if !self.workspace.window_tab_ids(id).is_empty() {
            return;
        }
        if self.workspace.current_window_id() == id
            && let Some(other) = self
                .workspace
                .window_ids()
                .into_iter()
                .find(|other| *other != id && self.editor_windows.contains_key(other))
        {
            self.switch_editor(other);
        }
        if let Some(window) = self.editor_windows.remove(&id) {
            self.maximized_windows.remove(&window.native);
            self.tab_geometry.remove(&window.native);
        }
        self.workspace.remove_empty_window(id);
        if self.last_active_editor == Some(id) {
            self.last_active_editor = self.workspace.window_ids().first().copied();
        }
        if self.editor_windows.is_empty() {
            self.main_window_id = None;
        }
    }

    pub(in crate::app) fn close_empty_editor(&mut self, id: EditorWindowId) -> Task<Message> {
        let Some(native) = self.editor_windows.get(&id).map(|w| w.native) else {
            return Task::none();
        };
        if !self.workspace.window_tab_ids(id).is_empty() {
            return Task::none();
        }
        let search = if self.advanced_search_owner == Some(id) {
            self.advanced_search_owner = None;
            self.pending_search = None;
            self.close_advanced_search_window()
        } else {
            Task::none()
        };
        self.remove_editor_window(id);
        let exit = if self.editor_windows.is_empty() && self.settings.exit_on_last_window_close {
            self.exit_request()
        } else {
            Task::none()
        };
        Task::batch([search, window::close(native), exit])
    }

    pub(in crate::app) fn editor_open_failed(
        &mut self,
        native: window::Id,
        error: String,
    ) -> Task<Message> {
        let failed = self.editor_id(native);
        let cancel = self.cancel_tab_drag();
        if let Some(failed) = failed
            && self.editor_windows.contains_key(&failed)
        {
            if let Some(target) = self
                .workspace
                .window_ids()
                .into_iter()
                .find(|id| *id != failed && self.editor_windows.contains_key(id))
            {
                let ids = self.workspace.window_tab_ids(failed).to_vec();
                let end = self.workspace.window_tab_ids(target).len();
                self.workspace.transfer_tabs(failed, &ids, target, end);
                self.switch_editor(target);
                self.remove_editor_window(failed);
            } else {
                // No native surface can display the startup error. Preserve
                // the existing recovery file rather than overwriting it.
                eprintln!("Could not open editor window: {error}");
                return Task::batch([cancel, iced::exit()]);
            }
        }
        self.file_status = Some(format!("Could not open window: {error}"));
        cancel
    }
}

pub(in crate::app) fn initial_registry(
    id: EditorWindowId,
    native: window::Id,
) -> HashMap<EditorWindowId, EditorWindow> {
    HashMap::from([(
        id,
        EditorWindow {
            native,
            ui: None,
            bounds: None,
            fullscreen: false,
        },
    )])
}
