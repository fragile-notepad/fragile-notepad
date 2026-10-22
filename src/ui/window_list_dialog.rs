use iced::Element;

use crate::message::{Message, WindowTarget};
use crate::ui::selector_dialog::{self, Entry, Kind};

pub const INPUT_ID: &str = "window-list-search";
pub const SCROLL_ID: &str = "window-list-scroll";
pub const PANEL_ID: &str = "window-list-dialog";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowListEntry {
    pub target: WindowTarget,
    pub title: String,
    pub description: String,
    pub search_titles: Vec<String>,
    pub is_focused: bool,
}

impl WindowListEntry {
    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self
                .search_titles
                .iter()
                .any(|title| title.to_lowercase().contains(&query))
    }
}

pub fn view(entries: Vec<WindowListEntry>, query: &str) -> Element<'_, Message> {
    let total = entries.len();
    let rows = entries
        .into_iter()
        .filter(|entry| entry.matches_query(query))
        .map(|entry| {
            selector_dialog::entry(Entry {
                title: entry.title,
                description: entry.description,
                symbol: match entry.target {
                    WindowTarget::Main | WindowTarget::Editor(_) => "Aa",
                    WindowTarget::AdvancedSearch => ".*",
                    WindowTarget::Settings => "≡",
                },
                active: entry.is_focused,
                on_select: Message::WindowFocusRequested(entry.target),
            })
        })
        .collect();
    selector_dialog::view(Kind::Windows, query, total, rows)
}
