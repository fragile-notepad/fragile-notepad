use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, stack, text, text_input,
};
use iced::{Center, Element, Fill, Font};

use crate::message::{Message, WindowTarget};
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::{motion, styles};

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
    stack![
        opaque(
            mouse_area(
                container(space::vertical())
                    .width(Fill)
                    .height(Fill)
                    .style(styles::modal_scrim)
            )
            .on_press(Message::WindowListClosed)
        ),
        container(motion::popup(opaque(dialog(entries, query))))
            .padding(24)
            .center(Fill),
    ]
    .into()
}

fn dialog(entries: Vec<WindowListEntry>, query: &str) -> Element<'_, Message> {
    let total = entries.len();
    let entries: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.matches_query(query))
        .collect();
    let count = entries.len();
    let rows = entries
        .into_iter()
        .fold(column![].spacing(8), |rows, entry| {
            rows.push(window_row(entry))
        });

    container(
        column![
            row![
                text("Windows")
                    .size(24)
                    .font(Font {
                        weight: iced::font::Weight::Semibold,
                        ..Font::DEFAULT
                    })
                    .width(Fill),
                container(
                    text(if query.trim().is_empty() {
                        format!("{total} open")
                    } else {
                        format!("{count} / {total}")
                    })
                    .size(12)
                )
                .padding([5, 9])
                .style(styles::info_badge),
            ]
            .spacing(16)
            .align_y(Center),
            text_input("Search window or tab titles…", query)
                .id(INPUT_ID)
                .on_input(Message::WindowListQueryChanged)
                .on_submit_maybe((count > 0).then_some(Message::WindowListSubmitted))
                .padding([10, 12])
                .size(14)
                .style(styles::input),
            container(
                scrollable(if count == 0 {
                    rows.push(
                        container(text("No matching windows").size(14))
                            .padding(16)
                            .center_x(Fill),
                    )
                } else {
                    rows
                })
                .id(SCROLL_ID)
                .style(styles::scrollable)
                .smooth_scroll(true)
                .spacing(8)
                .height(Fill)
            )
            .max_height(250)
            .width(Fill),
            row![
                space::horizontal(),
                button(text("Done").size(13))
                    .padding([9, 22])
                    .style(styles::command_button)
                    .on_press(Message::WindowListClosed),
            ]
            .spacing(16)
            .align_y(Center),
        ]
        .spacing(20),
    )
    .id(PANEL_ID)
    .width(Fill)
    .max_width(640)
    .height(Fill)
    .max_height((total.max(1) as f32 * 72.0 + 210.0).min(460.0))
    .padding(24)
    .style(styles::utility_dialog)
    .into()
}

fn window_row(entry: WindowListEntry) -> Element<'static, Message> {
    let symbol = match entry.target {
        WindowTarget::Main | WindowTarget::Editor(_) => "Aa",
        WindowTarget::AdvancedSearch => ".*",
        WindowTarget::Settings => "≡",
    };
    let label = column![
        text(entry.title)
            .size(14)
            .font(Font {
                weight: iced::font::Weight::Semibold,
                ..Font::DEFAULT
            })
            .wrapping(text::Wrapping::None)
            .ellipsis(text::Ellipsis::End)
            .width(Fill),
        container(
            text(entry.description)
                .size(12)
                .wrapping(text::Wrapping::None)
                .ellipsis(text::Ellipsis::Middle)
                .width(Fill)
        )
        .style(styles::info_muted),
    ]
    .spacing(4)
    .width(Fill)
    .clip(true);
    let mut trailing = row![].spacing(12).align_y(Center);
    if entry.is_focused {
        trailing = trailing.push(
            container(text("Active").size(11))
                .padding([4, 8])
                .style(styles::info_badge),
        );
    }
    trailing = trailing.push(hero::icon(HeroIcon::ChevronRight, 16, IconTone::Muted));

    button(
        row![
            container(text(symbol).size(18).font(Font::MONOSPACE))
                .center(40)
                .style(styles::info_card),
            label,
            trailing,
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(12)
    .width(Fill)
    .style(styles::utility_selection(entry.is_focused))
    .on_press(Message::WindowFocusRequested(entry.target))
    .into()
}
