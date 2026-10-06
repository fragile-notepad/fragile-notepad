use iced::widget::{button, checkbox, column, container, row, rule, scrollable, text, text_input};
use iced::{Center, Element, Fill, Font};

use crate::core::SearchMode;
use crate::message::{AdvancedSearchTab, Message};
use crate::search_dialog::{SearchDialogState, SearchResult};
use crate::ui::dropdown::dropdown;
use crate::ui::{styles, utility};

pub const QUERY_INPUT_ID: &str = "advanced-search-query";

pub fn view(dialog: &SearchDialogState) -> Element<'_, Message> {
    let body = column![search_form(dialog), commands(dialog), results(dialog)]
        .spacing(12)
        .height(Fill);

    container(
        column![
            container(navigation(dialog.active_tab)).padding([4, 16]),
            rule::horizontal(1).style(styles::utility_rule),
            container(body).padding(16).width(Fill).height(Fill),
            rule::horizontal(1).style(styles::utility_rule),
            row![
                container(text(status_label(dialog)).size(12))
                    .style(styles::info_muted)
                    .width(Fill),
                action("Close", Message::AdvancedSearchClosed, false, true),
            ]
            .spacing(16)
            .align_y(Center)
            .padding([8, 16]),
        ]
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .style(styles::settings_panel)
    .into()
}

fn navigation(active: AdvancedSearchTab) -> Element<'static, Message> {
    let open = open_scope(active);
    let tabs = [
        ("Find", search_tab(false, open)),
        ("Replace", search_tab(true, open)),
    ]
    .into_iter()
    .fold(row![].spacing(2), |tabs, (label, tab)| {
        tabs.push(
            button(text(label).size(13).font(utility::semibold()))
                .padding([8, 16])
                .style(styles::dialog_tab_button(active == tab))
                .on_press(Message::AdvancedSearchTabSelected(tab)),
        )
    });

    container(tabs)
        .padding(3)
        .style(styles::dialog_tab_group)
        .into()
}

fn search_form(dialog: &SearchDialogState) -> Element<'_, Message> {
    let open = open_scope(dialog.active_tab);
    let replace = replace_mode(dialog.active_tab);
    let submit = if open {
        Message::AdvancedFindAllOpenRun
    } else {
        Message::AdvancedFindNextRun
    };
    let mut fields = column![field(
        "Find",
        text_input("Enter text or a pattern", &dialog.query)
            .id(QUERY_INPUT_ID)
            .on_input(Message::AdvancedSearchQueryChanged)
            .on_submit(submit)
            .padding([8, 10])
            .size(14)
            .width(Fill)
            .style(styles::input)
            .into(),
    )]
    .spacing(8);
    if replace {
        fields = fields.push(field(
            "Replace with",
            text_input("Replacement text", &dialog.replacement)
                .on_input(Message::AdvancedSearchReplacementChanged)
                .padding([8, 10])
                .size(14)
                .width(Fill)
                .style(styles::input)
                .into(),
        ));
    }
    let scope = field(
        "Search in",
        dropdown(
            Some(open),
            &[false, true],
            |open| {
                if *open {
                    "Open documents"
                } else {
                    "Current document"
                }
                .into()
            },
            move |open| Message::AdvancedSearchTabSelected(search_tab(replace, open)),
        )
        .width(200)
        .into(),
    );
    if open {
        let filter = field(
            "File names",
            text_input("All · e.g. *.rs;*.txt", &dialog.include_pattern)
                .on_input(Message::AdvancedSearchIncludeChanged)
                .on_submit(Message::AdvancedFindAllOpenRun)
                .padding([8, 10])
                .size(13)
                .width(Fill)
                .style(styles::input)
                .into(),
        );
        fields = fields.push(row![scope, filter].spacing(16).align_y(Center));
    } else {
        fields = fields.push(scope);
    }
    column![fields, options(dialog)].spacing(10).into()
}

fn options(dialog: &SearchDialogState) -> Element<'_, Message> {
    let modes = [
        (SearchMode::Normal, "Plain text"),
        (SearchMode::Extended, "Escapes"),
        (SearchMode::Regex, "Regex"),
    ]
    .into_iter()
    .fold(row![].spacing(4), |row, (mode, label)| {
        row.push(
            button(text(label).size(12))
                .padding([6, 12])
                .style(styles::settings_category_button(dialog.mode == mode))
                .on_press(Message::AdvancedSearchModeSelected(mode)),
        )
    });
    let hint = match dialog.mode {
        SearchMode::Extended => Some(r"Escapes: \n (new line), \t (tab), \r (carriage return)."),
        SearchMode::Regex if replace_mode(dialog.active_tab) => {
            Some("Use $1, $2, … for captured groups.")
        }
        _ => None,
    };
    let mut flags = row![
        checkbox(dialog.case_sensitive)
            .style(styles::checkbox)
            .label("Match case")
            .text_size(12)
            .size(16)
            .on_toggle(Message::AdvancedSearchCaseSensitiveToggled),
        checkbox(dialog.whole_word)
            .style(styles::checkbox)
            .label("Whole words")
            .text_size(12)
            .size(16)
            .on_toggle(Message::AdvancedSearchWholeWordToggled),
    ]
    .spacing(18)
    .align_y(Center);
    if !open_scope(dialog.active_tab) {
        flags = flags.push(
            checkbox(dialog.wrap_around)
                .style(styles::checkbox)
                .label("Wrap around")
                .text_size(12)
                .size(16)
                .on_toggle(Message::AdvancedSearchWrapAroundToggled),
        );
    }
    let mut options = column![row![modes, flags].spacing(20).align_y(Center)].spacing(8);
    if let Some(hint) = hint {
        options = options.push(utility::description(hint));
    }
    options.into()
}

fn commands(dialog: &SearchDialogState) -> Element<'_, Message> {
    let enabled = !dialog.query.is_empty();
    let is_open = open_scope(dialog.active_tab);
    let find_all = if is_open {
        Message::AdvancedFindAllOpenRun
    } else {
        Message::AdvancedFindAllCurrentRun
    };
    let replace_all = if is_open {
        Message::AdvancedReplaceAllOpenRun
    } else {
        Message::AdvancedReplaceAllCurrentRun
    };
    let mut actions = row![].spacing(8);
    if !is_open {
        actions = actions.push(action(
            "Find next",
            Message::AdvancedFindNextRun,
            true,
            enabled,
        ));
    }
    actions = actions
        .push(action("Find all", find_all, is_open, enabled))
        .push(action("Count", Message::AdvancedCountRun, false, enabled));
    if replace_mode(dialog.active_tab) {
        if !is_open {
            actions = actions.push(action(
                "Replace",
                Message::AdvancedReplaceRun,
                false,
                enabled,
            ));
        }
        actions = actions.push(action("Replace all", replace_all, false, enabled));
    }
    actions.into()
}

fn results(dialog: &SearchDialogState) -> Element<'_, Message> {
    let count = dialog.results.len();
    let header = row![
        text("Results").size(14).font(utility::semibold()),
        utility::badge(count.to_string()),
    ]
    .spacing(10)
    .align_y(Center);
    let body: Element<'_, Message> = if count == 0 {
        let label =
            if dialog.status.starts_with("No matches") || dialog.status.starts_with("0 matches") {
                "No matches"
            } else {
                "No results yet"
            };
        container(utility::description(label))
            .padding(16)
            .center(Fill)
            .into()
    } else {
        let mut rows = column![].spacing(2);
        let mut previous = None;
        for result in &dialog.results {
            if previous != Some(result.document_id) {
                rows = rows.push(
                    container(
                        text(&result.document_title)
                            .size(12)
                            .font(utility::semibold())
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([6, 10])
                    .width(Fill)
                    .clip(true)
                    .style(styles::find_status),
                );
                previous = Some(result.document_id);
            }
            rows = rows.push(result_row(result));
        }
        scrollable(rows)
            .style(styles::scrollable)
            .spacing(8)
            .smooth_scroll(true)
            .height(Fill)
            .into()
    };
    container(column![header, body].spacing(6).height(Fill))
        .padding(10)
        .height(Fill)
        .width(Fill)
        .style(styles::utility_card)
        .into()
}

fn result_row(result: &SearchResult) -> Element<'_, Message> {
    let start = result.selection.range().start;
    button(
        row![
            container(
                text(format!("{}:{}", start.line + 1, start.column + 1))
                    .size(12)
                    .font(Font::MONOSPACE)
            )
            .width(76)
            .style(styles::info_muted),
            container(
                text(&result.preview)
                    .size(13)
                    .font(Font::MONOSPACE)
                    .wrapping(text::Wrapping::None)
            )
            .width(Fill)
            .clip(true),
        ]
        .spacing(12)
        .align_y(Center),
    )
    .padding([5, 10])
    .width(Fill)
    .style(styles::menu_dropdown_item)
    .on_press(Message::AdvancedSearchResultSelected(
        result.document_id,
        result.selection,
    ))
    .into()
}

fn field<'a>(label: &'static str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(13).width(88), control]
        .spacing(10)
        .align_y(Center)
        .into()
}

fn action<'a>(
    label: &'static str,
    message: Message,
    primary: bool,
    enabled: bool,
) -> Element<'a, Message> {
    button(text(label).size(13))
        .padding([8, 14])
        .style(if primary {
            styles::primary_command_button
        } else {
            styles::command_button
        })
        .on_press_maybe(enabled.then_some(message))
        .into()
}

fn status_label(dialog: &SearchDialogState) -> &str {
    if dialog.status == "No query" {
        "Ready to search"
    } else {
        &dialog.status
    }
}

const fn replace_mode(tab: AdvancedSearchTab) -> bool {
    matches!(
        tab,
        AdvancedSearchTab::Replace | AdvancedSearchTab::ReplaceInFiles
    )
}

const fn open_scope(tab: AdvancedSearchTab) -> bool {
    matches!(
        tab,
        AdvancedSearchTab::FindInFiles | AdvancedSearchTab::ReplaceInFiles
    )
}

const fn search_tab(replace: bool, open: bool) -> AdvancedSearchTab {
    match (replace, open) {
        (false, false) => AdvancedSearchTab::Find,
        (true, false) => AdvancedSearchTab::Replace,
        (false, true) => AdvancedSearchTab::FindInFiles,
        (true, true) => AdvancedSearchTab::ReplaceInFiles,
    }
}
