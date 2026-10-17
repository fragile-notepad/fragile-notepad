//! Inputs required to render the workbench; independent of application state ownership.

use super::{about_dialog, toolbar::WindowMenuState, window_list_dialog::WindowListEntry};
use crate::core::{Document, DocumentId, EditorSettings, EditorWindowId, FindState, Workspace};
use crate::editor::OutlineState;
use crate::message::{AboutTab, Menu};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChromeAnimationInfo {
    pub find_rendered_visible: bool,
    pub find_progress: f32,
    pub inline_replace_rendered_visible: bool,
    pub inline_replace_progress: f32,
    pub function_list_rendered_visible: bool,
    pub function_list_progress: f32,
    pub about_rendered_visible: bool,
    pub about_progress: f32,
    pub about_interactive: bool,
    pub dirty_close_progress: f32,
    pub dirty_close_interactive: bool,
}

pub(crate) struct WorkbenchView<'a> {
    pub workspace: &'a Workspace,
    pub editor_window: EditorWindowId,
    pub find: &'a FindState,
    pub settings: &'a EditorSettings,
    pub is_find_visible: bool,
    pub is_inline_replace_visible: bool,
    pub is_function_list_visible: bool,
    pub function_list_query: &'a str,
    pub chrome_animation: ChromeAnimationInfo,
    pub active_menu: Option<Menu>,
    pub active_menu_path: &'a [String],
    pub window_menu_state: WindowMenuState,
    pub dragged_tab: Option<DocumentId>,
    pub hovered_drop_tab: Option<DocumentId>,
    pub dirty_close_document: Option<&'a Document>,
    pub about_tab: Option<AboutTab>,
    pub rendering_debug_info: about_dialog::RenderingDebugInfo,
    pub window_list_entries: Option<Vec<WindowListEntry>>,
    pub file_status: Option<&'a str>,
    pub active_outline_state: Option<&'a OutlineState>,
}
