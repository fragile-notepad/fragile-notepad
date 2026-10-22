use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, stack, text, text_input,
};
use iced::{Center, Element, Fill, Font};

use crate::message::Message;
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::{motion, styles, tab_list_dialog, window_list_dialog};

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Windows,
    Tabs,
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Tabs => "Tabs",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Windows => "Search window or tab titles…",
            Self::Tabs => "Search tab titles…",
        }
    }

    fn empty_message(self) -> &'static str {
        match self {
            Self::Windows => "No matching windows",
            Self::Tabs => "No matching tabs",
        }
    }

    fn input_id(self) -> &'static str {
        match self {
            Self::Windows => window_list_dialog::INPUT_ID,
            Self::Tabs => tab_list_dialog::INPUT_ID,
        }
    }

    fn scroll_id(self) -> &'static str {
        match self {
            Self::Windows => window_list_dialog::SCROLL_ID,
            Self::Tabs => tab_list_dialog::SCROLL_ID,
        }
    }

    fn panel_id(self) -> &'static str {
        match self {
            Self::Windows => window_list_dialog::PANEL_ID,
            Self::Tabs => tab_list_dialog::PANEL_ID,
        }
    }

    fn query_changed(self, query: String) -> Message {
        match self {
            Self::Windows => Message::WindowListQueryChanged(query),
            Self::Tabs => Message::TabListQueryChanged(query),
        }
    }

    fn submitted(self) -> Message {
        match self {
            Self::Windows => Message::WindowListSubmitted,
            Self::Tabs => Message::TabListSubmitted,
        }
    }

    fn closed(self) -> Message {
        match self {
            Self::Windows => Message::WindowListClosed,
            Self::Tabs => Message::TabListClosed,
        }
    }
}

pub(super) fn view<'a>(
    kind: Kind,
    query: &'a str,
    total: usize,
    rows: Vec<Element<'static, Message>>,
) -> Element<'a, Message> {
    stack![
        opaque(
            mouse_area(
                container(space::vertical())
                    .width(Fill)
                    .height(Fill)
                    .style(styles::modal_scrim)
            )
            .on_press(kind.closed())
        ),
        container(motion::popup(opaque(dialog(kind, query, total, rows))))
            .padding(24)
            .center(Fill),
    ]
    .into()
}

fn dialog<'a>(
    kind: Kind,
    query: &'a str,
    total: usize,
    rows: Vec<Element<'static, Message>>,
) -> Element<'a, Message> {
    let count = rows.len();
    let rows = column(rows).spacing(8);

    container(
        column![
            row![
                text(kind.title())
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
            text_input(kind.placeholder(), query)
                .id(kind.input_id())
                .on_input(move |query| kind.query_changed(query))
                .on_submit_maybe((count > 0).then(|| kind.submitted()))
                .padding([10, 12])
                .size(14)
                .style(styles::input),
            container(
                scrollable(if count == 0 {
                    rows.push(
                        container(text(kind.empty_message()).size(14))
                            .padding(16)
                            .center_x(Fill),
                    )
                } else {
                    rows
                })
                .id(kind.scroll_id())
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
                    .on_press(kind.closed()),
            ]
            .spacing(16)
            .align_y(Center),
        ]
        .spacing(20),
    )
    .id(kind.panel_id())
    .width(Fill)
    .max_width(640)
    .height(Fill)
    .max_height((total.max(1) as f32 * 72.0 + 210.0).min(460.0))
    .padding(24)
    .style(styles::utility_dialog)
    .into()
}

pub(super) struct Entry {
    pub title: String,
    pub description: String,
    pub symbol: &'static str,
    pub active: bool,
    pub on_select: Message,
}

pub(super) fn entry(entry: Entry) -> Element<'static, Message> {
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
    if entry.active {
        trailing = trailing.push(
            container(text("Active").size(11))
                .padding([4, 8])
                .style(styles::info_badge),
        );
    }
    trailing = trailing.push(hero::icon(HeroIcon::ChevronRight, 16, IconTone::Muted));

    button(
        row![
            container(text(entry.symbol).size(18).font(Font::MONOSPACE))
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
    .style(styles::utility_selection(entry.active))
    .on_press(entry.on_select)
    .into()
}
