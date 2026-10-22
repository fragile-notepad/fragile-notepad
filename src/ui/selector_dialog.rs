use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, stack, text, text_input,
    tooltip,
};
use iced::{Center, Element, Fill, Font, Theme};

use crate::message::Message;
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::{motion, styles, tab_list_dialog, window_list_dialog};

const ROW_HEIGHT: f32 = 46.0;
const VISIBLE_ROWS: usize = 8;

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
    opacity: f32,
    interactive: bool,
) -> Element<'a, Message> {
    let opacity = opacity.clamp(0.0, 1.0);
    let surface =
        stack![
            opaque(
                mouse_area(container(space::vertical()).width(Fill).height(Fill).style(
                    move |theme| motion::fade_container(styles::modal_scrim(theme), opacity)
                ))
                .on_press(kind.closed())
            ),
            container(
                container(motion::popup_with_progress(
                    opaque(dialog(kind, query, total, rows, opacity)),
                    opacity,
                ))
                .width(Fill)
                .max_width(600)
                .height(Fill)
                .max_height(total.clamp(1, VISIBLE_ROWS) as f32 * ROW_HEIGHT + 106.0)
            )
            .padding(24)
            .center(Fill),
        ];
    opaque(motion::fade(
        surface,
        1.0,
        styles::editor_background,
        interactive,
    ))
}

fn dialog<'a>(
    kind: Kind,
    query: &'a str,
    total: usize,
    rows: Vec<Element<'static, Message>>,
    opacity: f32,
) -> Element<'a, Message> {
    let count = rows.len();
    let rows = column(rows).spacing(0);

    container(
        column![
            row![
                text(kind.title()).size(17).font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::DEFAULT
                }),
                container(
                    text(if query.trim().is_empty() {
                        format!("{total} open")
                    } else {
                        format!("{count} / {total}")
                    })
                    .size(11)
                )
                .style(move |theme| motion::fade_container(styles::info_muted(theme), opacity)),
                space::horizontal(),
                button(
                    container(hero::icon_with_opacity(
                        HeroIcon::XMark,
                        14,
                        IconTone::Muted,
                        opacity
                    ))
                    .center(24)
                )
                .padding(0)
                .style(move |theme, status| motion::fade_button(
                    styles::icon_button(theme, status),
                    opacity
                ))
                .on_press(kind.closed()),
            ]
            .spacing(10)
            .align_y(Center),
            text_input(kind.placeholder(), query)
                .id(kind.input_id())
                .on_input(move |query| kind.query_changed(query))
                .on_submit_maybe((count > 0).then(|| kind.submitted()))
                .padding([7, 10])
                .size(13)
                .style(move |theme, status| {
                    let mut style = styles::input(theme, status);
                    style.background = style.background.scale_alpha(opacity);
                    style.border.color = style.border.color.scale_alpha(opacity);
                    style.icon = style.icon.scale_alpha(opacity);
                    style.placeholder = style.placeholder.scale_alpha(opacity);
                    style.value = style.value.scale_alpha(opacity);
                    style.selection = style.selection.scale_alpha(opacity);
                    style
                }),
            container(
                scrollable(if count == 0 {
                    rows.push(
                        container(text(kind.empty_message()).size(13))
                            .center_x(Fill)
                            .center_y(ROW_HEIGHT),
                    )
                } else {
                    rows
                })
                .id(kind.scroll_id())
                .style(move |theme, status| fade_scrollable(theme, status, opacity))
                .smooth_scroll(true)
                .spacing(4)
                .height(Fill)
            )
            .max_height(ROW_HEIGHT * VISIBLE_ROWS as f32)
            .width(Fill),
        ]
        .spacing(10),
    )
    .id(kind.panel_id())
    .width(Fill)
    .max_width(600)
    .height(Fill)
    .max_height(count.clamp(1, VISIBLE_ROWS) as f32 * ROW_HEIGHT + 106.0)
    .padding(14)
    .style(move |theme| motion::fade_container(styles::utility_dialog(theme), opacity))
    .into()
}

pub(super) struct Entry {
    pub title: String,
    pub description: String,
    pub icon: Element<'static, Message>,
    pub active: bool,
    pub on_select: Message,
}

pub(super) fn entry(entry: Entry, opacity: f32) -> Element<'static, Message> {
    let tooltip_content = column![
        text(entry.title.clone()).size(12),
        text(entry.description.clone()).size(11)
    ]
    .spacing(3);
    let label = column![
        row![
            text(entry.title)
                .size(13)
                .font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::DEFAULT
                })
                .wrapping(text::Wrapping::None)
                .ellipsis(text::Ellipsis::End)
                .width(Fill),
            container(text(if entry.active { "Active" } else { "" }).size(10)).style(
                move |theme| {
                    container::Style {
                        text_color: Some(styles::accent_color(theme).scale_alpha(opacity)),
                        ..Default::default()
                    }
                }
            ),
        ]
        .spacing(8)
        .align_y(Center),
        container(
            text(entry.description)
                .size(11)
                .wrapping(text::Wrapping::None)
                .ellipsis(text::Ellipsis::Middle)
                .width(Fill)
        )
        .style(move |theme| motion::fade_container(styles::info_muted(theme), opacity)),
    ]
    .spacing(2)
    .width(Fill);
    let content = row![container(entry.icon).center(32), label]
        .spacing(10)
        .align_y(Center);

    tooltip(
        button(content)
            .padding([6, 10])
            .height(ROW_HEIGHT)
            .width(Fill)
            .style(move |theme, status| {
                motion::fade_button(styles::selector_entry(entry.active)(theme, status), opacity)
            })
            .on_press(entry.on_select),
        container(tooltip_content)
            .padding([6, 9])
            .max_width(560)
            .style(move |theme| motion::fade_container(styles::tooltip(theme), opacity)),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn fade_scrollable(theme: &Theme, status: scrollable::Status, opacity: f32) -> scrollable::Style {
    let mut style = styles::scrollable(theme, status);
    style.container = motion::fade_container(style.container, opacity);
    for rail in [&mut style.horizontal_rail, &mut style.vertical_rail] {
        rail.background = rail.background.map(|color| color.scale_alpha(opacity));
        rail.border.color = rail.border.color.scale_alpha(opacity);
        rail.scroller.background = rail.scroller.background.scale_alpha(opacity);
        rail.scroller.border.color = rail.scroller.border.color.scale_alpha(opacity);
    }
    style.gap = style.gap.map(|color| color.scale_alpha(opacity));
    style.auto_scroll.background = style.auto_scroll.background.scale_alpha(opacity);
    style.auto_scroll.border.color = style.auto_scroll.border.color.scale_alpha(opacity);
    style.auto_scroll.shadow.color = style.auto_scroll.shadow.color.scale_alpha(opacity);
    style.auto_scroll.icon = style.auto_scroll.icon.scale_alpha(opacity);
    style
}
