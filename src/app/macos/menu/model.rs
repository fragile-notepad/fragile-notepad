//! Application state projected into native menu policy without Cocoa objects.

use iced::window;

use crate::app::App;
use crate::core::{Document, EditorSettings, TextEncoding};
use crate::message::{Message, NativeEditCommand};
use crate::ui::menu::MenuTree;
use crate::ui::toolbar::{self, WindowMenuState};

pub(in crate::app::macos) struct MenuModel<'a> {
    pub(super) state: MenuState,
    document: Option<&'a Document>,
}

#[derive(Clone, PartialEq)]
pub(super) struct MenuState {
    owner: Option<window::Id>,
    focused_window: Option<window::Id>,
    editor_blocked: bool,
    input_possible: bool,
    pub capturing_shortcut: bool,
    windows: WindowMenuState,
    settings: EditorSettings,
    function_list_visible: bool,
    document: Option<DocumentState>,
    availability: Vec<bool>,
}

#[derive(Clone, PartialEq, Eq)]
struct DocumentState {
    encoding: TextEncoding,
    syntax_token: String,
}

#[derive(Clone)]
pub(super) struct MenuCommand {
    pub message: Message,
    pub enabled: bool,
    pub edit: Option<NativeEditCommand>,
}

impl<'a> MenuModel<'a> {
    pub(in crate::app::macos) fn from_app(app: &'a App) -> Self {
        let editor = app
            .last_active_editor
            .filter(|id| app.editor_windows.contains_key(id))
            .unwrap_or_else(|| app.workspace.current_window_id());
        let owner = app.editor_windows.get(&editor).map(|window| window.native);
        let document = app.workspace.active_document_in(editor);
        let windows =
            owner.map_or_else(WindowMenuState::default, |id| app.window_menu_state_for(id));
        let ui = app.window_ui(editor);
        let auxiliary_focused = app
            .focused_window_id
            .is_some_and(|id| app.editor_id(id).is_none());
        let editor_blocked = app.lifecycle.is_exiting()
            || auxiliary_focused
            || ui.as_ref().is_some_and(|ui| {
                ui.close_prompt.document().is_some()
                    || ui.go_to_line_prompt.is_some()
                    || ui.chrome_animation.about.target_visible()
                    || *ui.is_window_list_visible
                    || *ui.is_tab_list_visible
            });
        let input_possible = auxiliary_focused
            || ui.as_ref().is_some_and(|ui| {
                *ui.is_find_visible
                    || *ui.is_function_list_visible
                    || ui.go_to_line_prompt.is_some()
                    || *ui.is_window_list_visible
                    || *ui.is_tab_list_visible
            });
        Self {
            state: MenuState {
                owner,
                focused_window: app.focused_window_id,
                editor_blocked,
                input_possible,
                capturing_shortcut: app.settings_dialog.capturing_shortcut.is_some(),
                windows,
                settings: app.settings.clone(),
                function_list_visible: ui.is_some_and(|ui| *ui.is_function_list_visible),
                document: document.map(|doc| DocumentState {
                    encoding: doc.encoding,
                    syntax_token: doc.syntax_token.clone(),
                }),
                availability: toolbar::native_menu_availability(document),
            },
            document,
        }
    }

    pub fn trees(&self) -> Vec<(&'static str, MenuTree)> {
        toolbar::native_menu_trees(
            self.state.windows.clone(),
            &self.state.settings,
            self.document,
        )
    }
}

impl MenuState {
    pub fn command(&self, message: &Message, available: bool) -> MenuCommand {
        let edit = NativeEditCommand::from_message(message);
        let enabled = (available && (!self.editor_blocked || allowed_during_dialog(message)))
            || (self.input_possible && edit.is_some_and(NativeEditCommand::supports_text_input));
        let message = edit.map_or_else(|| message.clone(), Message::NativeMenuEdit);
        let owner = if edit.is_some() {
            self.focused_window.or(self.owner)
        } else {
            self.owner
        };
        MenuCommand {
            message: match owner {
                Some(id) => Message::ForWindow(id, Box::new(message)),
                None => message,
            },
            enabled,
            edit,
        }
    }

    pub fn checked(&self, message: &Message) -> bool {
        let decorations = &self.settings.decorations;
        match message {
            Message::ToggleWordWrap => self.settings.word_wrap,
            Message::ToggleLineNumbers => decorations.show_line_numbers,
            Message::ToggleSpaceAndTab => decorations.show_spaces && decorations.show_tabs,
            Message::ToggleEolMarkers => decorations.show_end_of_line_markers,
            Message::ToggleAllCharacters => {
                decorations.show_spaces
                    && decorations.show_tabs
                    && decorations.show_end_of_line_markers
            }
            Message::ToggleIndentationGuides => decorations.show_indentation_guides,
            Message::ToggleFoldingControls => decorations.show_folding_controls,
            Message::ToggleFunctionList => self.function_list_visible,
            Message::EncodingSelected(encoding) => self
                .document
                .as_ref()
                .is_some_and(|doc| doc.encoding == *encoding),
            Message::LanguageSelected(token) => self
                .document
                .as_ref()
                .is_some_and(|doc| doc.syntax_token == *token),
            _ => false,
        }
    }
}

fn allowed_during_dialog(message: &Message) -> bool {
    matches!(
        message,
        Message::NewWindow
            | Message::Quit
            | Message::AboutOpened
            | Message::ToggleSettingsPanel
            | Message::WindowListOpened
            | Message::TabListOpened
            | Message::WindowFocusNext
            | Message::WindowFocusPrevious
    )
}
