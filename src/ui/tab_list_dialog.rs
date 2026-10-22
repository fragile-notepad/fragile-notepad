use iced::Element;

use crate::core::{Document, EditorWindowId, Workspace};
use crate::message::Message;
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
    let active = workspace
        .active_document_in(editor_window)
        .map(|document| document.id);
    let total = workspace.window_tab_ids(editor_window).len();
    let rows = workspace
        .window_documents(editor_window)
        .filter(|document| matches_query(document, query))
        .map(|document| {
            let mut description = document.path.as_ref().map_or_else(
                || "Unsaved document".into(),
                |path| path.display().to_string(),
            );
            if document.is_pinned {
                description.insert_str(0, "Pinned · ");
            }
            selector_dialog::entry(Entry {
                title: document.title(),
                description,
                symbol: "Aa",
                active: active == Some(document.id),
                on_select: Message::TabListSelected(document.id),
            })
        })
        .collect();
    selector_dialog::view(Kind::Tabs, query, total, rows)
}
