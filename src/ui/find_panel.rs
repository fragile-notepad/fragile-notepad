use std::time::Duration;

use iced::widget::{button, column, container, row, space, text, text_input, tooltip};
use iced::{Center, Element, Fill, Font, Length};

use crate::core::{FindState, SearchError, SearchMode};
use crate::message::{AdvancedSearchTab, Message};
use crate::ui::controls::centered_button_content;
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::icons::search::{self, SearchIcon};
use crate::ui::{motion, styles};

pub const PANEL_ID: &str = "fragile-notepad-find-panel";
pub const FIND_INPUT_ID: &str = "fragile-notepad-find-input";
pub const REPLACE_INPUT_ID: &str = "fragile-notepad-replace-input";
const BODY_TEXT_SIZE: u32 = 14;
const SECONDARY_TEXT_SIZE: u32 = 12;
const ROW_HEIGHT: f32 = 34.0;
const ACTIONS_WIDTH: f32 = 236.0;

pub fn view(
    find: &FindState,
    is_replace_visible: bool,
    is_replace_rendered_visible: bool,
    replace_progress: f32,
) -> Element<'_, Message> {
    let has_matches = !find.matches.is_empty() && find.error.is_none();
    let count = if find.matches_limited {
        format!("{}+", find.matches.len())
    } else {
        find.matches.len().to_string()
    };
    let status = match (find.error.as_ref(), find.current_match, find.matches.len()) {
        (Some(_), _, _) => String::from("Invalid regex"),
        (None, _, 0) if find.query.is_empty() => String::from("0 results"),
        (None, _, 0) => String::from("No results"),
        (None, Some(index), _) => format!("{} / {count}", index + 1),
        (None, None, _) => format!("{count} results"),
    };
    let status_tip = match find.error.as_ref() {
        Some(SearchError::InvalidRegex(error)) => format!("Invalid regular expression\n{error}"),
        None if find.query.is_empty() => String::from("Type to search the current file"),
        None if !has_matches => String::from("No matches in the current file"),
        None if find.matches_limited => format!(
            "Showing the first {} matches in the current file.\nUse Count all in advanced search to count every match.",
            find.matches.len(),
        ),
        None => format!("{} matches in the current file", find.matches.len()),
    };
    let replace_icon = if is_replace_visible {
        HeroIcon::ChevronDown
    } else {
        HeroIcon::ChevronRight
    };

    let query = container(
        row![
            search::icon(SearchIcon::MagnifyingGlass, 18, IconTone::Muted),
            text_input("Find in current file…", &find.query)
                .id(FIND_INPUT_ID)
                .on_input(Message::FindQueryChanged)
                .padding([4, 6])
                .size(BODY_TEXT_SIZE)
                .width(Fill)
                .style(styles::search_input),
            option(
                "Aa",
                "Match case",
                find.case_sensitive,
                Message::FindCaseSensitiveToggled(!find.case_sensitive),
            ),
            option(
                "ab",
                "Match whole words",
                find.whole_word,
                Message::FindWholeWordToggled(!find.whole_word),
            ),
            option(
                ".*",
                "Use regular expressions",
                find.mode == SearchMode::Regex,
                Message::FindModeSelected(if find.mode == SearchMode::Regex {
                    SearchMode::Normal
                } else {
                    SearchMode::Regex
                }),
            ),
        ]
        .spacing(3)
        .align_y(Center),
    )
    .padding([2, 7])
    .height(ROW_HEIGHT)
    .width(Fill)
    .style(styles::search_field);

    let actions = row![
        hint(
            container(
                text(status)
                    .size(SECONDARY_TEXT_SIZE)
                    .wrapping(text::Wrapping::None)
                    .ellipsis(text::Ellipsis::End),
            )
            .width(96)
            .height(28)
            .center_x(96)
            .center_y(28)
            .style(styles::search_status(find.error.is_some())),
            status_tip,
        ),
        icon_action(
            SearchIcon::ArrowUp,
            "Previous match (Shift+Enter / Shift+F3)",
            Message::FindPrevious,
            has_matches,
        ),
        icon_action(
            SearchIcon::ArrowDown,
            "Next match (Enter / F3)",
            Message::FindNext,
            has_matches,
        ),
        icon_action(
            SearchIcon::Adjustments,
            "Advanced find & replace",
            Message::ToggleAdvancedSearch(if is_replace_visible {
                AdvancedSearchTab::Replace
            } else {
                AdvancedSearchTab::Find
            }),
            true,
        ),
        hint(
            button(centered_button_content(hero::icon(
                HeroIcon::XMark,
                17,
                IconTone::Muted,
            )))
            .width(28)
            .height(28)
            .padding(0)
            .style(styles::icon_button)
            .on_press(Message::HideFind),
            "Close search (Esc)",
        ),
    ]
    .spacing(7)
    .align_y(Center)
    .width(ACTIONS_WIDTH);

    let find_row = row![
        hint(
            button(centered_button_content(hero::icon(
                replace_icon,
                17,
                IconTone::Muted,
            )))
            .width(28)
            .height(28)
            .padding(0)
            .style(styles::icon_button)
            .on_press(Message::ToggleInlineReplace),
            if is_replace_visible {
                "Hide replacement"
            } else {
                "Show replacement"
            },
        ),
        query,
        actions,
    ]
    .spacing(7)
    .align_y(Center)
    .height(ROW_HEIGHT)
    .width(Fill);

    let mut rows = column![find_row].padding([6, 8]).width(Fill);

    if is_replace_rendered_visible {
        let replacement = container(
            row![
                search::icon(SearchIcon::Return, 18, IconTone::Muted),
                text_input("Replace with…", &find.replacement)
                    .id(REPLACE_INPUT_ID)
                    .on_input(Message::FindReplacementChanged)
                    .padding([4, 6])
                    .size(BODY_TEXT_SIZE)
                    .width(Fill)
                    .style(styles::search_input),
            ]
            .spacing(3)
            .align_y(Center),
        )
        .padding([2, 7])
        .height(ROW_HEIGHT)
        .width(Fill)
        .style(styles::search_field);

        let replacement_actions = row![
            replace_action("Replace", Message::ReplaceCurrent, has_matches, false),
            replace_action("Replace all", Message::ReplaceAll, has_matches, true),
            space::horizontal(),
        ]
        .width(ACTIONS_WIDTH)
        .spacing(7)
        .align_y(Center);

        rows = rows.push(
            container(motion::fade(
                column![
                    space::vertical().height(6),
                    row![
                        space::horizontal().width(28),
                        replacement,
                        replacement_actions,
                    ]
                    .spacing(7)
                    .align_y(Center)
                    .height(ROW_HEIGHT)
                    .width(Fill),
                ],
                replace_progress,
                styles::utility_bar_background,
                is_replace_visible,
            ))
            .height(Length::Fixed(
                (ROW_HEIGHT + 6.0) * replace_progress.clamp(0.0, 1.0),
            ))
            .width(Fill)
            .clip(true),
        );
    }

    container(rows)
        .id(PANEL_ID)
        .width(Fill)
        .style(styles::search_bar)
        .into()
}

fn option(
    label: &'static str,
    description: &'static str,
    active: bool,
    message: Message,
) -> Element<'static, Message> {
    hint(
        button(centered_button_content(
            text(label).size(12).font(Font::MONOSPACE),
        ))
        .width(28)
        .height(26)
        .padding(0)
        .style(styles::search_option(active))
        .on_press(message),
        description,
    )
}

fn icon_action(
    icon: SearchIcon,
    description: &'static str,
    message: Message,
    enabled: bool,
) -> Element<'static, Message> {
    hint(
        button(centered_button_content(search::icon(
            icon,
            17,
            IconTone::Text,
        )))
        .width(28)
        .height(28)
        .padding(0)
        .style(styles::icon_button)
        .on_press_maybe(enabled.then_some(message)),
        description,
    )
}

fn replace_action(
    label: &'static str,
    message: Message,
    enabled: bool,
    primary: bool,
) -> Element<'static, Message> {
    button(centered_button_content(text(label).size(13)))
        .padding([4, 10])
        .height(28)
        .width(if primary { 96 } else { 80 })
        .style(if primary {
            styles::primary_command_button
        } else {
            styles::command_button
        })
        .on_press_maybe(enabled.then_some(message))
        .into()
}

fn hint<'a>(
    content: impl Into<Element<'a, Message>>,
    description: impl Into<std::borrow::Cow<'a, str>>,
) -> Element<'a, Message> {
    let description = description.into();
    tooltip(
        content,
        container(text(description).size(12))
            .padding([6, 8])
            .max_width(420)
            .style(styles::tooltip),
        tooltip::Position::Bottom,
    )
    .delay(Duration::from_millis(450))
    .gap(6)
    .into()
}
