use iced::Element;

use crate::message::{Message, WindowTarget};
use crate::ui::icons::{
    file_types,
    hero::IconTone,
    search::{self, SearchIcon},
};
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
    view_animated(entries, query, 1.0, true)
}

pub fn view_animated(
    entries: Vec<WindowListEntry>,
    query: &str,
    progress: f32,
    interactive: bool,
) -> Element<'_, Message> {
    let progress = progress.clamp(0.0, 1.0);
    let total = entries.len();
    let rows = entries
        .into_iter()
        .filter(|entry| entry.matches_query(query))
        .map(|entry| {
            let icon = match entry.target {
                WindowTarget::Main | WindowTarget::Editor(_) => {
                    file_types::icon_for_path_with_opacity(
                        entry
                            .search_titles
                            .first()
                            .map(|title| std::path::Path::new(title.trim_start_matches('*'))),
                        32,
                        progress,
                    )
                }
                WindowTarget::AdvancedSearch => {
                    search::icon(SearchIcon::MagnifyingGlass, 24, IconTone::Muted)
                }
                WindowTarget::Settings => {
                    search::icon(SearchIcon::Adjustments, 24, IconTone::Muted)
                }
            };
            selector_dialog::entry(
                Entry {
                    title: entry.title,
                    description: entry.description,
                    icon,
                    active: entry.is_focused,
                    on_select: Message::WindowFocusRequested(entry.target),
                },
                progress,
            )
        })
        .collect();
    selector_dialog::view(Kind::Windows, query, total, rows, progress, interactive)
}
