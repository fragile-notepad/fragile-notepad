use iced::Element;

use crate::core::{Document, EditorWindowId, Workspace};
use crate::message::Message;
use crate::ui::icons::file_types;
use crate::ui::selector_dialog::{self, Entry, Kind};

pub const INPUT_ID: &str = "tab-list-search";
pub const SCROLL_ID: &str = "tab-list-scroll";
pub const PANEL_ID: &str = "tab-list-dialog";

pub(crate) fn matches_query(document: &Document, query: &str) -> bool {
    document
        .title()
        .to_lowercase()
        .contains(&query.trim().to_lowercase())
}

pub fn view<'a>(
    workspace: &Workspace,
    editor_window: EditorWindowId,
    query: &'a str,
) -> Element<'a, Message> {
    view_animated(workspace, editor_window, query, 1.0, true)
}

pub fn view_animated<'a>(
    workspace: &Workspace,
    editor_window: EditorWindowId,
    query: &'a str,
    progress: f32,
    interactive: bool,
) -> Element<'a, Message> {
    let progress = progress.clamp(0.0, 1.0);
    let active = workspace
        .active_document_in(editor_window)
        .map(|document| document.id);
    let total = workspace.window_tab_ids(editor_window).len();
    let rows = workspace
        .window_documents(editor_window)
        .filter(|document| matches_query(document, query))
        .map(|document| {
            let mut description = document
                .path
                .as_ref()
                .map_or_else(|| "Unsaved".into(), |path| path.display().to_string());
            if document.is_pinned {
                description.insert_str(0, "Pinned · ");
            }
            selector_dialog::entry(
                Entry {
                    title: document.title(),
                    description,
                    icon: file_types::icon_for_path_with_opacity(
                        document.path.as_deref(),
                        32,
                        progress,
                    ),
                    active: active == Some(document.id),
                    on_select: Message::TabListSelected(document.id),
                },
                progress,
            )
        })
        .collect();
    selector_dialog::view(Kind::Tabs, query, total, rows, progress, interactive)
}
