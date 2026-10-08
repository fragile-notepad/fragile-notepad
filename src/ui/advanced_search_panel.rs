//! A compact search workspace with live, grouped results.
use iced::widget::{
    button, checkbox, column, container, row, rule, scrollable, space, text, text_input, tooltip,
};
use iced::{Center, Color, Element, Fill, Font};

use crate::core::SearchMode;
use crate::message::{AdvancedSearchTab, Message};
use crate::search_dialog::{SearchCountSummary, SearchDialogState, SearchResult};
use crate::ui::icons::hero::IconTone;
use crate::ui::icons::search::{self, SearchIcon};
use crate::ui::{motion, styles, utility};

pub const QUERY_INPUT_ID: &str = "advanced-search-query";
pub const REPLACEMENT_INPUT_ID: &str = "advanced-search-replacement";
pub const PANEL_ID: &str = "fragile-notepad-advanced-search-panel";

pub fn view(dialog: &SearchDialogState) -> Element<'_, Message> {
    let body = column![search_form(dialog), commands(dialog), results(dialog)]
        .spacing(12)
        .height(Fill);
    container(
        column![
            container(navigation(dialog.active_tab))
                .padding([12, 16])
                .width(Fill),
            rule::horizontal(1).style(styles::utility_rule),
            container(body).padding(16).width(Fill).height(Fill),
            rule::horizontal(1).style(styles::utility_rule),
            row![
                status_feedback(dialog),
                utility::description("Esc to close"),
                action("Close", Message::AdvancedSearchClosed, false, true),
            ]
            .spacing(14)
            .align_y(Center)
            .padding([8, 16]),
        ]
        .height(Fill),
    )
    .id(PANEL_ID)
    .width(Fill)
    .height(Fill)
    .style(styles::settings_panel)
    .into()
}

fn navigation(active: AdvancedSearchTab) -> Element<'static, Message> {
    let open = open_scope(active);
    container(
        row![
            button(text("Find").size(13).font(utility::semibold()))
                .padding([7, 18])
                .style(styles::dialog_tab_button(!replace_mode(active)))
                .on_press(Message::AdvancedSearchTabSelected(search_tab(false, open))),
            button(text("Replace").size(13).font(utility::semibold()))
                .padding([7, 18])
                .style(styles::dialog_tab_button(replace_mode(active)))
                .on_press(Message::AdvancedSearchTabSelected(search_tab(true, open))),
        ]
        .spacing(2),
    )
    .padding(3)
    .style(styles::dialog_tab_group)
    .into()
}

fn search_form(dialog: &SearchDialogState) -> Element<'_, Message> {
    let replace = replace_mode(dialog.active_tab);
    let open = open_scope(dialog.active_tab);
    let enabled = !dialog.query.is_empty() && !has_query_error(dialog);
    let query = container(
        row![
            search::icon(SearchIcon::MagnifyingGlass, 18, IconTone::Muted),
            text_input("Find text or a pattern…", &dialog.query)
                .id(QUERY_INPUT_ID)
                .on_input(Message::AdvancedSearchQueryChanged)
                .padding([8, 8])
                .size(14)
                .width(Fill)
                .style(styles::search_input),
            option(
                "Aa",
                "Match case",
                dialog.case_sensitive,
                Message::AdvancedSearchCaseSensitiveToggled(!dialog.case_sensitive)
            ),
            option(
                "ab",
                "Match whole words",
                dialog.whole_word,
                Message::AdvancedSearchWholeWordToggled(!dialog.whole_word)
            ),
            option(
                r"\n",
                r"Interpret escapes: \n, \t, \r",
                dialog.mode == SearchMode::Extended,
                Message::AdvancedSearchModeSelected(if dialog.mode == SearchMode::Extended {
                    SearchMode::Normal
                } else {
                    SearchMode::Extended
                })
            ),
            option(
                ".*",
                "Use regular expressions",
                dialog.mode == SearchMode::Regex,
                Message::AdvancedSearchModeSelected(if dialog.mode == SearchMode::Regex {
                    SearchMode::Normal
                } else {
                    SearchMode::Regex
                })
            ),
            container(rule::vertical(1).style(styles::utility_rule)).height(20),
            icon_action(
                SearchIcon::ArrowUp,
                "Previous match · Shift+Enter",
                Message::AdvancedFindPreviousRun,
                enabled
            ),
            icon_action(
                SearchIcon::ArrowDown,
                "Next match · Enter",
                Message::AdvancedFindNextRun,
                enabled
            ),
        ]
        .spacing(3)
        .align_y(Center),
    )
    .padding([3, 9])
    .width(Fill)
    .style(styles::search_field);

    // Keeping these widgets mounted preserves the caret during reversible reveals.
    let replacement = motion::reveal(
        container(
            container(
                row![
                    search::icon(SearchIcon::Return, 18, IconTone::Muted),
                    text_input("Replace with…", &dialog.replacement)
                        .id(REPLACEMENT_INPUT_ID)
                        .on_input(Message::AdvancedSearchReplacementChanged)
                        .padding([8, 8])
                        .size(14)
                        .width(Fill)
                        .style(styles::search_input),
                ]
                .spacing(3)
                .align_y(Center),
            )
            .padding([3, 9])
            .width(Fill)
            .style(styles::search_field),
        )
        .padding(iced::Padding {
            top: 8.0,
            ..Default::default()
        }),
        replace,
        styles::settings_panel_background,
    );

    let scope = row![
        scope_button("Current document", !open, search_tab(replace, false)),
        scope_button("Open documents", open, search_tab(replace, true)),
        space::horizontal(),
        checkbox(dialog.wrap_around)
            .style(styles::checkbox)
            .label("Wrap around")
            .text_size(12)
            .size(14)
            .on_toggle(Message::AdvancedSearchWrapAroundToggled),
    ]
    .spacing(6)
    .align_y(Center);
    let filter = motion::reveal(
        container(
            row![
                utility::description("File names"),
                text_input("All files · e.g. *.rs; *.txt", &dialog.include_pattern)
                    .on_input(Message::AdvancedSearchIncludeChanged)
                    .padding([6, 9])
                    .size(12)
                    .width(Fill)
                    .style(styles::input),
            ]
            .spacing(12)
            .align_y(Center),
        )
        .padding(iced::Padding {
            top: 8.0,
            ..Default::default()
        }),
        open,
        styles::settings_panel_background,
    );
    let hint = match dialog.mode {
        SearchMode::Extended => r"Escapes enabled · \n new line, \t tab, \r carriage return",
        SearchMode::Regex if replace => {
            "Regular expressions enabled · Use $1, $2, … for captured groups"
        }
        SearchMode::Regex => "Regular expressions enabled",
        SearchMode::Normal => "",
    };
    column![
        query,
        replacement,
        container(column![scope, filter]).padding(iced::Padding {
            top: 10.0,
            ..Default::default()
        }),
        motion::reveal(
            container(utility::description(hint)).padding(iced::Padding {
                top: 6.0,
                ..Default::default()
            }),
            !hint.is_empty(),
            styles::settings_panel_background
        ),
    ]
    .into()
}

fn scope_button(
    label: &'static str,
    selected: bool,
    tab: AdvancedSearchTab,
) -> Element<'static, Message> {
    button(text(label).size(12))
        .padding([6, 10])
        .style(styles::search_option(selected))
        .on_press(Message::AdvancedSearchTabSelected(tab))
        .into()
}

fn option(
    label: &'static str,
    hint: &'static str,
    active: bool,
    message: Message,
) -> Element<'static, Message> {
    tooltip(
        button(container(text(label).size(12).font(utility::semibold())).center(28))
            .width(28)
            .height(28)
            .padding(0)
            .style(styles::search_option(active))
            .on_press(message),
        container(text(hint).size(12))
            .padding([6, 9])
            .style(styles::utility_card),
        tooltip::Position::Bottom,
    )
    .delay(std::time::Duration::from_millis(450))
    .into()
}

fn icon_action(
    icon: SearchIcon,
    hint: &'static str,
    message: Message,
    enabled: bool,
) -> Element<'static, Message> {
    tooltip(
        button(container(search::icon(icon, 16, IconTone::Text)).center(28))
            .width(28)
            .height(28)
            .padding(0)
            .style(styles::icon_button)
            .on_press_maybe(enabled.then_some(message)),
        container(text(hint).size(12))
            .padding([6, 9])
            .style(styles::utility_card),
        tooltip::Position::Bottom,
    )
    .delay(std::time::Duration::from_millis(450))
    .into()
}

fn commands(dialog: &SearchDialogState) -> Element<'_, Message> {
    let enabled = !dialog.query.is_empty() && !has_query_error(dialog);
    let open = open_scope(dialog.active_tab);
    let find_all = if open {
        Message::AdvancedFindAllOpenRun
    } else {
        Message::AdvancedFindAllCurrentRun
    };
    let replace_all = if open {
        Message::AdvancedReplaceAllOpenRun
    } else {
        Message::AdvancedReplaceAllCurrentRun
    };
    let mut actions = row![
        action(
            "Find all",
            find_all,
            true,
            enabled && dialog.parsed_result_settings().is_ok()
        ),
        action("Count", Message::AdvancedCountRun, false, enabled),
    ]
    .spacing(8)
    .align_y(Center);
    if replace_mode(dialog.active_tab) {
        actions = actions.push(space::horizontal());
        if !open {
            actions = actions.push(action(
                "Replace",
                Message::AdvancedReplaceRun,
                false,
                enabled,
            ));
        }
        actions = actions.push(action("Replace all", replace_all, false, enabled));
    } else {
        actions = actions
            .push(space::horizontal())
            .push(utility::description("Enter next · Ctrl+Enter find all"));
    }
    actions.into()
}

fn results(dialog: &SearchDialogState) -> Element<'_, Message> {
    let count_summary = dialog
        .count_summary
        .as_ref()
        .filter(|_| !is_searching(dialog));
    let document_count = dialog
        .results
        .iter()
        .map(|result| result.document_id)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let mut header = row![
        text(if count_summary.is_some() {
            "Full count"
        } else {
            "Results"
        })
        .size(13)
        .font(utility::semibold())
    ]
    .spacing(8)
    .align_y(Center);
    if count_summary.is_none() {
        header = header.push(utility::badge(format!(
            "{}{}",
            dialog.match_count,
            if dialog.matches_limited { "+" } else { "" }
        )));
    }
    if count_summary.is_none() && document_count > 0 {
        header = header.push(
            container(
                text(format!(
                    "in {document_count} {}",
                    if document_count == 1 {
                        "document"
                    } else {
                        "documents"
                    }
                ))
                .size(12),
            )
            .style(styles::info_muted),
        );
    }
    if dialog.matches_limited && count_summary.is_none() {
        header = header.push(utility::description("Limit reached"));
    }
    header = header.push(space::horizontal()).push(
        tooltip(
            button(
                row![
                    search::icon(SearchIcon::Adjustments, 15, IconTone::Text),
                    text("Display").size(12)
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([5, 8])
            .style(styles::search_option(dialog.result_options_visible))
            .on_press(Message::AdvancedResultOptionsToggled),
            container(text("Search limit and preview length").size(12))
                .padding([6, 9])
                .style(styles::utility_card),
            tooltip::Position::Bottom,
        )
        .delay(std::time::Duration::from_millis(450)),
    );

    let body: Element<'_, Message> = if dialog.results.is_empty() && is_searching(dialog) {
        space::horizontal().width(Fill).height(Fill).into()
    } else if let Some(summary) = count_summary {
        count_results(dialog, summary)
    } else if dialog.results.is_empty() {
        let (title, hint) = if has_query_error(dialog) {
            (
                "Check your pattern",
                "Fix the regular expression to continue searching.",
            )
        } else if dialog.query.is_empty() {
            (
                "Search your documents",
                "Start typing to preview matching lines.",
            )
        } else if dialog.status.starts_with("No matches") || dialog.status.starts_with("0 matches")
        {
            (
                "No matches found",
                "Try another query or adjust the search options.",
            )
        } else {
            ("Ready to search", "Matching lines will appear here.")
        };
        container(
            column![
                container(search::icon(
                    SearchIcon::MagnifyingGlass,
                    25,
                    IconTone::Text
                ))
                .center(52)
                .style(styles::search_empty_icon),
                space::vertical().height(3),
                text(title).size(15).font(utility::semibold()),
                utility::description(hint),
            ]
            .spacing(8)
            .align_x(Center),
        )
        .padding(16)
        .center(Fill)
        .into()
    } else {
        let mut rows = column![].spacing(2);
        let mut index = 0;
        while index < dialog.results.len() {
            let first = &dialog.results[index];
            let end = dialog.results[index..]
                .iter()
                .position(|result| result.document_id != first.document_id)
                .map_or(dialog.results.len(), |offset| index + offset);
            rows = rows.push(
                container(
                    row![
                        text(&first.document_title)
                            .size(12)
                            .font(utility::semibold())
                            .wrapping(text::Wrapping::None)
                            .width(Fill),
                        text(format!(
                            "{} {}",
                            end - index,
                            if end - index == 1 { "match" } else { "matches" }
                        ))
                        .size(11),
                    ]
                    .spacing(10)
                    .align_y(Center),
                )
                .padding([7, 10])
                .width(Fill)
                .clip(true)
                .style(styles::search_result_group),
            );
            for result_index in index..end {
                rows = rows.push(result_row(
                    &dialog.results[result_index],
                    dialog.selected_result == Some(result_index),
                ));
            }
            index = end;
        }
        scrollable(rows)
            .id("advanced-search-results")
            .style(styles::scrollable)
            .spacing(6)
            .smooth_scroll(true)
            .height(Fill)
            .into()
    };
    container(
        column![
            header,
            motion::reveal(
                container(result_options(dialog)).padding(iced::Padding {
                    top: 8.0,
                    bottom: 8.0,
                    ..Default::default()
                }),
                dialog.result_options_visible,
                styles::editor_background
            ),
            rule::horizontal(1).style(styles::utility_rule),
            container(body)
                .padding(iced::Padding {
                    top: 6.0,
                    ..Default::default()
                })
                .width(Fill)
                .height(Fill),
        ]
        .height(Fill),
    )
    .padding(12)
    .width(Fill)
    .height(Fill)
    .style(styles::utility_card)
    .into()
}

fn count_results<'a>(
    dialog: &'a SearchDialogState,
    summary: &'a SearchCountSummary,
) -> Element<'a, Message> {
    let total = text(summary.total_matches.to_string())
        .size(40)
        .font(utility::semibold())
        .style(|theme| text::Style {
            color: Some(styles::accent_color(theme)),
        });
    let documents = summary.per_document.len();
    let scope = format!(
        "{documents} {} searched",
        if documents == 1 {
            "document"
        } else {
            "documents"
        }
    );
    let mut overview = row![
        column![
            row![
                total,
                text(if summary.total_matches == 1 {
                    "match"
                } else {
                    "matches"
                })
                .size(16),
            ]
            .spacing(10)
            .align_y(Center),
            container(text(scope).size(12)).style(styles::info_muted),
        ]
        .spacing(2),
        space::horizontal(),
    ]
    .spacing(12)
    .align_y(Center);
    if summary.total_matches > 0 {
        overview = overview.push(action(
            "Show matching lines",
            if open_scope(dialog.active_tab) {
                Message::AdvancedFindAllOpenRun
            } else {
                Message::AdvancedFindAllCurrentRun
            },
            true,
            dialog.parsed_result_settings().is_ok(),
        ));
    }
    let mut rows = column![container(overview).padding([12, 10])].spacing(4);
    for document in &summary.per_document {
        rows = rows.push(
            container(
                row![
                    text(&document.title)
                        .size(12)
                        .wrapping(text::Wrapping::None)
                        .width(Fill),
                    text(document.match_count.to_string())
                        .size(13)
                        .font(utility::semibold()),
                ]
                .spacing(12)
                .align_y(Center),
            )
            .padding([9, 10])
            .width(Fill)
            .clip(true)
            .style(styles::search_result_group),
        );
    }
    scrollable(rows)
        .id("advanced-search-count")
        .style(styles::scrollable)
        .spacing(6)
        .smooth_scroll(true)
        .height(Fill)
        .into()
}

fn result_options(dialog: &SearchDialogState) -> Element<'_, Message> {
    let fields = row![
        result_option_field(
            "Search limit",
            &dialog.result_limit_input,
            Message::AdvancedResultLimitChanged
        ),
        result_option_field(
            "Preview length",
            &dialog.preview_chars_input,
            Message::AdvancedPreviewCharsChanged
        ),
        result_option_field(
            "Before match",
            &dialog.context_before_input,
            Message::AdvancedPreviewContextChanged
        ),
        button(text("Reset").size(12))
            .padding([6, 12])
            .style(styles::command_button)
            .on_press(Message::AdvancedResultOptionsReset),
    ]
    .spacing(16)
    .align_y(iced::Bottom);
    let mut options = column![fields].spacing(6);
    if let Err(error) = dialog.parsed_result_settings() {
        options = options.push(container(text(error).size(12)).style(styles::search_status(true)));
    }
    options.into()
}

fn result_option_field<'a>(
    label: &'static str,
    value: &'a str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    column![
        utility::description(label),
        text_input("", value)
            .on_input(on_input)
            .padding([5, 7])
            .size(12)
            .width(Fill)
            .style(styles::input)
    ]
    .spacing(4)
    .width(Fill)
    .into()
}

fn result_row(result: &SearchResult, selected: bool) -> Element<'_, Message> {
    let start = result.selection.range().start;
    let range = &result.preview_match;
    let preview: Element<'_, Message> = if let (Some(before), Some(found), Some(after)) = (
        result.preview.get(..range.start),
        result.preview.get(range.clone()),
        result.preview.get(range.end..),
    ) {
        text::Rich::<(), Message>::with_spans([
            text::Span::new(before),
            text::Span::new(if found.is_empty() { "│" } else { found })
                .background(Color::from_rgba8(55, 145, 245, 0.19)),
            text::Span::new(after),
        ])
        .size(12)
        .font(Font::MONOSPACE)
        .wrapping(text::Wrapping::None)
        .into()
    } else {
        text(&result.preview)
            .size(12)
            .font(Font::MONOSPACE)
            .wrapping(text::Wrapping::None)
            .into()
    };
    button(
        row![
            container(
                text(format!("{}:{}", start.line + 1, start.column + 1))
                    .size(11)
                    .font(Font::MONOSPACE)
            )
            .width(72)
            .style(styles::info_muted),
            container(preview).width(Fill).clip(true),
        ]
        .spacing(10)
        .align_y(Center),
    )
    .padding([7, 10])
    .width(Fill)
    .style(styles::search_result(selected))
    .on_press(Message::AdvancedSearchResultSelected(
        result.document_id,
        result.selection,
    ))
    .into()
}

fn action<'a>(
    label: &'static str,
    message: Message,
    primary: bool,
    enabled: bool,
) -> Element<'a, Message> {
    button(text(label).size(12))
        .padding([7, 12])
        .style(if primary {
            styles::primary_command_button
        } else {
            styles::command_button
        })
        .on_press_maybe(enabled.then_some(message))
        .into()
}

fn has_query_error(dialog: &SearchDialogState) -> bool {
    dialog.status.starts_with("Invalid regex:")
}
fn is_searching(dialog: &SearchDialogState) -> bool {
    dialog.status.starts_with("Searching")
        || dialog.status.starts_with("Loading")
        || dialog.status.starts_with("Updating results")
}
fn has_error(dialog: &SearchDialogState) -> bool {
    has_query_error(dialog)
        || dialog
            .parsed_result_settings()
            .err()
            .is_some_and(|error| dialog.status == error)
        || dialog.status.starts_with("Search canceled")
        || dialog.status == "No document"
}
fn status_feedback(dialog: &SearchDialogState) -> Element<'_, Message> {
    let label = status_label(dialog);
    let error = has_error(dialog);
    let searching = is_searching(dialog);
    let light = if error {
        motion::StatusLightState::Error
    } else if searching {
        motion::StatusLightState::Searching
    } else if dialog.match_count > 0 || dialog.status.starts_with("Replaced") {
        motion::StatusLightState::Success
    } else {
        motion::StatusLightState::Idle
    };
    let compact = if has_query_error(dialog) {
        label.lines().last().unwrap_or(label)
    } else {
        label
    };
    tooltip(
        container(
            row![
                motion::status_light(light),
                text(compact).size(12).wrapping(text::Wrapping::None),
            ]
            .spacing(8)
            .align_y(Center),
        )
        .style(styles::search_status(error))
        .width(Fill)
        .clip(true),
        container(text(label).size(12))
            .padding([8, 10])
            .max_width(560)
            .style(styles::utility_card),
        tooltip::Position::Top,
    )
    .delay(std::time::Duration::from_millis(450))
    .into()
}
fn status_label(dialog: &SearchDialogState) -> &str {
    if matches!(dialog.status.as_str(), "No query" | "Ready" | "") {
        "Idle"
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
