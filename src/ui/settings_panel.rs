use iced::advanced::text::highlighter::Highlighter as _;
use iced::highlighter;
use iced::widget::text_editor::LineEnding;
use iced::widget::{
    button, column, container, keyed_column, rich_text, row, rule, scrollable, space, span, text,
    text_input, toggler,
};
use iced::{Center, Color, Element, Fill, Font};

use crate::core::{
    AppearanceMode, EditorSettings, HardwareAccelerationMode, IndentationMode, KeyBinding,
    SearchResultSettings, ShortcutCommand, ShortcutDisplayPart, ShortcutGroup,
    ShortcutModifierIcon, TextEncoding,
};
use crate::message::{Message, SettingsCategory};
use crate::settings_dialog::{SettingsDialogState, ShortcutNoticeKind};
use crate::ui::dropdown::dropdown;
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::icons::shortcut::{self, ShortcutIcon};
use crate::ui::{controls, motion, styles, utility};

const INDENTATION_OPTIONS: &[IndentationMode] = &[
    IndentationMode::Tabs,
    IndentationMode::Spaces(2),
    IndentationMode::Spaces(4),
    IndentationMode::Spaces(8),
];
const SHORTCUT_STATUS_HEIGHT: f32 = 42.0;
const CONTROL_WIDTH: f32 = 210.0;
const NEW_FILE_ENCODINGS: &[TextEncoding] = &[
    TextEncoding::Utf8,
    TextEncoding::Utf8Bom,
    TextEncoding::Utf16LeBom,
    TextEncoding::Utf16BeBom,
];

pub const fn category_label(category: SettingsCategory) -> &'static str {
    match category {
        SettingsCategory::General => "Files",
        SettingsCategory::Appearance => "Appearance",
        SettingsCategory::Editor => "Editor",
        SettingsCategory::Display => "Display",
        SettingsCategory::Search => "Search",
        SettingsCategory::Shortcuts => "Shortcuts",
        SettingsCategory::Advanced => "Advanced",
    }
}

const fn category_description(category: SettingsCategory) -> &'static str {
    match category {
        SettingsCategory::General => "New files, saving, and recent files.",
        SettingsCategory::Appearance => "Colors and text size.",
        SettingsCategory::Editor => "Typing, line wrapping, and scrolling.",
        SettingsCategory::Display => "Line numbers, spaces, and visual guides.",
        SettingsCategory::Search => "Search results and previews.",
        SettingsCategory::Shortcuts => "Keyboard shortcut assignments.",
        SettingsCategory::Advanced => "Graphics settings.",
    }
}

pub fn view(dialog: &SettingsDialogState) -> Element<'_, Message> {
    let validation_error = dialog.validation_error();
    let valid = validation_error.is_none();
    let pane = match dialog.category {
        SettingsCategory::General => files_pane(dialog),
        SettingsCategory::Appearance => appearance_pane(&dialog.draft, dialog.system_dark),
        SettingsCategory::Editor => editor_pane(dialog),
        SettingsCategory::Display => display_pane(&dialog.draft),
        SettingsCategory::Search => search_pane(dialog),
        SettingsCategory::Shortcuts => shortcuts_pane(dialog),
        SettingsCategory::Advanced => advanced_pane(&dialog.draft),
    };
    let pane: Element<'_, Message> = if dialog.category == SettingsCategory::Shortcuts {
        pane
    } else {
        scrollable(pane)
            .style(styles::scrollable)
            .smooth_scroll(true)
            .spacing(6)
            .height(Fill)
            .into()
    };
    let categories = SettingsCategory::ALL.iter().fold(
        column![utility::description("SETTINGS")].spacing(4),
        |items, &item| items.push(category(category_label(item), item, dialog.category)),
    );
    let sidebar = container(categories.push(space::vertical()).height(Fill))
        .padding([16, 10])
        .width(156)
        .height(Fill)
        .style(styles::settings_category_list);

    container(
        column![
            row![
                sidebar,
                container(
                    column![
                        column![
                            utility::heading(category_label(dialog.category)),
                            utility::description(category_description(dialog.category)),
                        ]
                        .spacing(4),
                        // A new category starts at the top; redraws within a page retain scrolling.
                        keyed_column![(dialog.category, pane)]
                            .height(Fill)
                            .width(Fill),
                    ]
                    .spacing(14)
                    .height(Fill)
                )
                .padding(18)
                .width(Fill)
                .height(Fill)
            ]
            .height(Fill),
            rule::horizontal(1).style(styles::utility_rule),
            row![
                container(text(validation_error.unwrap_or_default()).size(12))
                    .style(styles::info_muted)
                    .width(Fill),
                footer_button("Cancel", Some(Message::CancelSettings), false),
                footer_button("Apply", valid.then_some(Message::ApplySettings), false),
                footer_button("Save & close", valid.then_some(Message::SaveSettings), true),
            ]
            .spacing(8)
            .align_y(Center)
            .padding([8, 16]),
        ]
        .width(Fill)
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .style(styles::settings_panel)
    .into()
}

fn category(
    label: &'static str,
    category: SettingsCategory,
    active: SettingsCategory,
) -> Element<'static, Message> {
    utility::navigation(
        label,
        category == active,
        Message::SettingsCategorySelected(category),
    )
}

fn files_pane(dialog: &SettingsDialogState) -> Element<'_, Message> {
    let settings = &dialog.draft;
    column![
        section(
            "New files",
            column![
                described_row(
                    "Encoding",
                    "Default text encoding for new files.",
                    dropdown(
                        Some(settings.new_file_encoding),
                        NEW_FILE_ENCODINGS,
                        |encoding| encoding.label().into(),
                        Message::DraftNewFileEncodingSelected,
                    )
                    .width(CONTROL_WIDTH)
                    .into()
                ),
                divider(),
                described_row(
                    "Line endings",
                    "Default line endings for new files.",
                    dropdown(
                        Some(settings.new_file_line_ending),
                        &[LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr],
                        |ending| match ending {
                            LineEnding::Lf => "LF (\\n)",
                            LineEnding::CrLf => "CRLF (\\r\\n)",
                            LineEnding::Cr => "CR (\\r)",
                            LineEnding::LfCr => "LFCR (legacy)",
                            LineEnding::None => "None",
                        }
                        .into(),
                        Message::DraftNewFileLineEndingSelected,
                    )
                    .width(CONTROL_WIDTH)
                    .into()
                ),
            ]
            .spacing(12)
            .into()
        ),
        section(
            "Saving",
            described_toggle(
                "Auto-save",
                "Automatically saves named files when switching tabs or leaving the window.",
                settings.auto_save,
                Message::DraftAutoSaveToggled,
            )
        ),
        section(
            "Recent files",
            described_row(
                "History limit",
                format!(
                    "0 disables recent files. Maximum: {}.",
                    EditorSettings::MAX_RECENT_FILE_LIMIT
                ),
                number_input(
                    &EditorSettings::DEFAULT_RECENT_FILE_LIMIT.to_string(),
                    &dialog.recent_file_limit_input,
                    Message::DraftRecentFileLimitChanged
                ),
            )
        ),
    ]
    .spacing(14)
    .into()
}

fn advanced_pane(settings: &EditorSettings) -> Element<'_, Message> {
    column![section(
        "Rendering",
        column![
            described_row(
                "Renderer",
                "Hybrid rendering uses graphics hardware when available.",
                dropdown(
                    Some(settings.hardware_acceleration),
                    HardwareAccelerationMode::ALL,
                    |mode| mode.label().into(),
                    Message::DraftHardwareAccelerationSelected,
                )
                .width(CONTROL_WIDTH)
                .into()
            ),
            divider(),
            utility::description("Switching to software requires a restart."),
            utility::description("Hardware diagnostic mode helps diagnose graphics issues."),
        ]
        .spacing(12)
        .into()
    )]
    .into()
}

fn appearance_pane(settings: &EditorSettings, system_dark: bool) -> Element<'_, Message> {
    let modes = [
        AppearanceMode::System,
        AppearanceMode::Light,
        AppearanceMode::Dark,
    ]
    .into_iter()
    .fold(row![].spacing(8), |row, mode| {
        row.push(appearance_choice(mode, settings.appearance == mode))
    });
    column![
        section("Color mode", modes.into()),
        section(
            "Text & syntax",
            column![
                setting_row(
                    "Syntax theme",
                    dropdown(
                        Some(settings.syntax_theme.family()),
                        highlighter::Theme::ALL,
                        highlighter::Theme::to_string,
                        Message::DraftThemeSelected,
                    )
                    .width(CONTROL_WIDTH)
                    .into()
                ),
                utility::description("Colors adapt to the light or dark app theme."),
                rule::horizontal(1).style(styles::utility_rule),
                setting_row(
                    "Editor zoom",
                    stepper(
                        format!("{:.0}%", settings.zoom * 100.0),
                        Message::SettingsZoomOut,
                        Message::SettingsZoomIn,
                        Message::SettingsZoomReset,
                        settings.zoom > EditorSettings::MIN_ZOOM,
                        settings.zoom < EditorSettings::MAX_ZOOM,
                    )
                ),
                syntax_preview(settings, system_dark),
            ]
            .spacing(10)
            .into()
        ),
    ]
    .spacing(14)
    .into()
}

fn appearance_choice(mode: AppearanceMode, selected: bool) -> Element<'static, Message> {
    let label = match mode {
        AppearanceMode::System => "System",
        AppearanceMode::Light => "Light",
        AppearanceMode::Dark => "Dark",
    };
    let preview: Element<'static, Message> = if mode == AppearanceMode::System {
        row![miniature(false), miniature(true)].spacing(3).into()
    } else {
        miniature(mode == AppearanceMode::Dark)
    };
    button(
        column![
            container(preview)
                .padding(3)
                .style(styles::appearance_preview_frame),
            row![
                text(label).size(13).font(utility::semibold()),
                space::horizontal(),
                text(if selected { "Selected" } else { "" }).size(10)
            ]
            .align_y(Center),
        ]
        .spacing(8),
    )
    .padding(8)
    .width(Fill)
    .style(styles::utility_selection(selected))
    .on_press(Message::DraftAppearanceSelected(mode))
    .into()
}

/// A small, code-native window illustration; both color modes remain visible in any theme.
fn miniature(dark: bool) -> Element<'static, Message> {
    let surface = if dark {
        Color::from_rgb8(26, 27, 29)
    } else {
        Color::from_rgb8(250, 250, 250)
    };
    let chrome = if dark {
        Color::from_rgb8(53, 55, 60)
    } else {
        Color::from_rgb8(230, 231, 233)
    };
    let ink = if dark {
        Color::from_rgb8(172, 175, 181)
    } else {
        Color::from_rgb8(128, 131, 136)
    };
    let line = move |width| {
        container(space::horizontal())
            .width(width)
            .height(3)
            .style(move |_| container::Style {
                background: Some(ink.into()),
                border: iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
    };
    container(column![
        container(space::horizontal())
            .height(11)
            .width(Fill)
            .style(move |_| container::Style {
                background: Some(chrome.into()),
                ..Default::default()
            }),
        row![
            container(space::horizontal())
                .width(14)
                .height(Fill)
                .style(move |_| container::Style {
                    background: Some(chrome.into()),
                    ..Default::default()
                }),
            column![line(30), line(20), line(27)]
                .spacing(6)
                .padding(10)
                .width(Fill),
        ]
        .height(Fill),
    ])
    .height(52)
    .width(Fill)
    .clip(true)
    .style(move |_| container::Style {
        background: Some(surface.into()),
        border: iced::Border {
            color: chrome,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn syntax_preview(settings: &EditorSettings, system_dark: bool) -> Element<'_, Message> {
    let mut highlighter = highlighter::Highlighter::new(&highlighter::Settings {
        token: "rs".into(),
        theme: settings.resolved_syntax_theme(system_dark),
    });
    let mut lines = column![].spacing(3);
    for (index, line) in [
        "fn header(count: u32) -> Element<Message> {",
        "    // Types, functions and macros",
        "    let retries = 3;",
        "    column![text(\"Ready\").size(retries)]",
        "}",
    ]
    .into_iter()
    .enumerate()
    {
        let spans: Vec<iced::widget::text::Span<'_, (), Font>> = highlighter
            .highlight_line(line)
            .map(|(range, highlight)| {
                let mut part = span(&line[range]).font(highlight.font().unwrap_or(Font::MONOSPACE));
                if let Some(color) = highlight.color() {
                    part = part.color(color);
                }
                part
            })
            .collect();
        let mut line_row = row![].spacing(16).align_y(Center);
        if settings.decorations.show_line_numbers {
            line_row = line_row.push(
                container(text((index + 1).to_string()).size(13).font(Font::MONOSPACE))
                    .width(20)
                    .style(styles::info_muted),
            );
        }
        lines = lines.push(
            line_row.push(
                rich_text(spans)
                    .font(Font::MONOSPACE)
                    .size(16.0 * settings.zoom)
                    .wrapping(text::Wrapping::None),
            ),
        );
    }
    let preview = container(column![
        container(utility::description("Preview")).padding([8, 12]),
        rule::horizontal(1).style(styles::utility_rule),
        scrollable(container(lines).padding(10))
            .style(styles::scrollable)
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .height(132)
            .width(Fill),
    ])
    .width(Fill)
    .style(styles::utility_card);
    iced::widget::themer(styles::modern_theme(settings.appearance), preview).into()
}

fn editor_pane(dialog: &SettingsDialogState) -> Element<'_, Message> {
    let settings = &dialog.draft;
    let fixed_wrap = settings.wrap_column_limit.is_some();
    let mut wrap_controls = column![setting_row(
        "Wrap width",
        dropdown(
            Some(fixed_wrap),
            &[false, true],
            |fixed| if *fixed {
                "Fixed column".into()
            } else {
                "Window width".into()
            },
            Message::DraftFixedWrapSelected,
        )
        .width(CONTROL_WIDTH)
        .into(),
    ),]
    .spacing(10);
    if fixed_wrap {
        wrap_controls = wrap_controls
            .push(setting_row(
                "Column limit",
                number_input(
                    &EditorSettings::DEFAULT_WRAP_COLUMN.to_string(),
                    &dialog.wrap_column_input,
                    Message::DraftWrapColumnChanged,
                ),
            ))
            .push(
                row![
                    space::horizontal(),
                    controls::compact_command_button("80", 12, Message::DraftWrapColumnPreset(80)),
                    controls::compact_command_button(
                        "100",
                        12,
                        Message::DraftWrapColumnPreset(100)
                    ),
                    controls::compact_command_button(
                        "120",
                        12,
                        Message::DraftWrapColumnPreset(120)
                    ),
                ]
                .spacing(6),
            )
            .push(
                container(
                    text(format!(
                        "{}–{} columns. Long lines wrap earlier in narrower windows.",
                        EditorSettings::MIN_WRAP_COLUMN,
                        EditorSettings::MAX_WRAP_COLUMN,
                    ))
                    .size(12),
                )
                .style(styles::info_muted),
            );
    }
    column![
        section(
            "Indentation",
            column![described_row(
                "Insert with Tab",
                "Tabs or spaces inserted by the Tab key.",
                dropdown(
                    Some(settings.indentation),
                    INDENTATION_OPTIONS,
                    indentation_label,
                    Message::DraftIndentationSelected
                )
                .width(CONTROL_WIDTH)
                .into()
            ),]
            .spacing(10)
            .into()
        ),
        section(
            "Line wrapping",
            column![
                described_toggle(
                    "Word wrap",
                    "Wraps long lines without inserting line breaks.",
                    settings.word_wrap,
                    Message::DraftWordWrapToggled,
                ),
                divider(),
                wrap_controls,
            ]
            .spacing(12)
            .into(),
        ),
        section(
            "Scrolling",
            described_row(
                "Scroll speed",
                "Mouse wheel scroll distance.",
                stepper(
                    format!("{:.2}×", settings.scroll_speed),
                    Message::SettingsScrollSpeedDecrease,
                    Message::SettingsScrollSpeedIncrease,
                    Message::SettingsScrollSpeedReset,
                    settings.scroll_speed > EditorSettings::MIN_SCROLL_SPEED,
                    settings.scroll_speed < EditorSettings::MAX_SCROLL_SPEED
                ),
            )
        ),
    ]
    .spacing(14)
    .into()
}

fn display_pane(settings: &EditorSettings) -> Element<'_, Message> {
    column![
        section(
            "Line numbers & folding",
            column![
                toggle_row(
                    "Show line numbers",
                    settings.decorations.show_line_numbers,
                    Message::DraftLineNumbersToggled
                ),
                rule::horizontal(1).style(styles::utility_rule),
                described_toggle(
                    "Show folding controls",
                    "Controls for collapsing and expanding text blocks.",
                    settings.decorations.show_folding_controls,
                    Message::DraftFoldingControlsToggled
                ),
            ]
            .spacing(10)
            .into()
        ),
        section(
            "Guides & wrapping",
            column![
                described_toggle(
                    "Indentation guides",
                    "Vertical guides at each indentation level.",
                    settings.decorations.show_indentation_guides,
                    Message::DraftIndentationGuidesToggled
                ),
                divider(),
                described_toggle(
                    "Wrap markers",
                    "Markers at the start of wrapped lines.",
                    settings.decorations.show_wrap_indicator,
                    Message::DraftWrapIndicatorToggled
                ),
                divider(),
                described_toggle(
                    "Column guide",
                    if settings.word_wrap {
                        "Vertical guide at the wrap boundary.".to_owned()
                    } else {
                        format!(
                            "Vertical guide at column {}.",
                            settings
                                .wrap_column_limit
                                .unwrap_or(EditorSettings::DEFAULT_WRAP_COLUMN)
                        )
                    },
                    settings.decorations.show_wrap_guide,
                    Message::DraftWrapGuideToggled
                ),
            ]
            .spacing(12)
            .into()
        ),
        section(
            "Whitespace",
            column![
                utility::description("Visible symbols for spaces, tabs, and line endings."),
                toggle_row(
                    "Show spaces",
                    settings.decorations.show_spaces,
                    Message::DraftVisibleSpacesToggled
                ),
                rule::horizontal(1).style(styles::utility_rule),
                toggle_row(
                    "Show tabs",
                    settings.decorations.show_tabs,
                    Message::DraftVisibleTabsToggled
                ),
                rule::horizontal(1).style(styles::utility_rule),
                toggle_row(
                    "Show line endings",
                    settings.decorations.show_end_of_line_markers,
                    Message::DraftEolMarkersToggled
                ),
            ]
            .spacing(10)
            .into()
        ),
    ]
    .spacing(14)
    .into()
}

fn search_pane(dialog: &SettingsDialogState) -> Element<'_, Message> {
    column![
        section(
            "Results",
            described_row(
                "Maximum results",
                format!(
                    "Maximum matches displayed ({}–{}).",
                    SearchResultSettings::MIN_RESULT_LIMIT,
                    SearchResultSettings::MAX_RESULT_LIMIT
                ),
                number_input(
                    &SearchResultSettings::DEFAULT_RESULT_LIMIT.to_string(),
                    &dialog.result_limit_input,
                    Message::DraftSearchResultLimitChanged
                ),
            )
        ),
        section(
            "Match previews",
            column![
                described_row(
                    "Preview length",
                    format!(
                        "Characters displayed per result ({}–{}).",
                        SearchResultSettings::MIN_PREVIEW_CHARS,
                        SearchResultSettings::MAX_PREVIEW_CHARS
                    ),
                    number_input(
                        &SearchResultSettings::DEFAULT_PREVIEW_CHARS.to_string(),
                        &dialog.preview_chars_input,
                        Message::DraftSearchPreviewCharsChanged
                    )
                ),
                divider(),
                described_row(
                    "Context before match",
                    "Characters included before each match in the preview.",
                    number_input(
                        &SearchResultSettings::DEFAULT_CONTEXT_BEFORE.to_string(),
                        &dialog.context_before_input,
                        Message::DraftSearchContextBeforeChanged
                    )
                ),
            ]
            .spacing(12)
            .into()
        ),
        row![
            space::horizontal(),
            controls::compact_command_button(
                "Restore search defaults",
                12,
                Message::DraftSearchResultsReset,
            )
        ],
        utility::description("Also available in Find and Replace."),
    ]
    .spacing(14)
    .into()
}

fn shortcuts_pane(dialog: &SettingsDialogState) -> Element<'_, Message> {
    let groups = ShortcutGroup::ALL
        .into_iter()
        .fold(row![].spacing(4), |row, group| {
            row.push(
                button(text(group.label()).size(13))
                    .padding([6, 11])
                    .style(styles::settings_category_button(
                        group == dialog.shortcut_group,
                    ))
                    .on_press(Message::ShortcutGroupSelected(group)),
            )
        });
    let commands: Vec<_> = ShortcutCommand::ALL
        .into_iter()
        .filter(|command| command.group() == dialog.shortcut_group)
        .collect();
    let pane = column![
        shortcut_status(dialog),
        row![
            groups,
            space::horizontal(),
            button(text("Restore all defaults").size(12))
                .padding([6, 9])
                .style(styles::command_button)
                .on_press(Message::ShortcutsResetToDefaults)
        ]
        .spacing(8)
        .align_y(Center),
    ]
    .spacing(10);
    let mut rows = column![].spacing(0);
    for (index, command) in commands.into_iter().enumerate() {
        if index > 0 {
            rows = rows.push(rule::horizontal(1).style(styles::utility_rule));
        }
        rows = rows.push(shortcut_row(
            &dialog.draft,
            command,
            dialog.capturing_shortcut,
            dialog.shortcut_notice_animation.pulse(),
        ));
    }
    pane.push(
        keyed_column![(
            dialog.shortcut_group,
            scrollable(container(rows).style(styles::utility_card))
                .style(styles::scrollable)
                .spacing(6)
                .smooth_scroll(true)
                .height(Fill)
        )]
        .height(Fill),
    )
    .height(Fill)
    .into()
}

fn shortcut_status(dialog: &SettingsDialogState) -> Element<'_, Message> {
    let animation = dialog.shortcut_notice_animation;
    let fallback_current = animation.rendered() == ShortcutNoticeKind::None
        && (dialog.capturing_shortcut.is_some() || dialog.shortcut_conflict.is_some());
    let kind = match animation.rendered() {
        ShortcutNoticeKind::None if dialog.capturing_shortcut.is_some() => {
            ShortcutNoticeKind::Listening(dialog.capturing_shortcut.unwrap())
        }
        ShortcutNoticeKind::None if dialog.shortcut_conflict.is_some() => {
            ShortcutNoticeKind::Conflict(dialog.shortcut_conflict.unwrap())
        }
        kind => kind,
    };

    let content: Element<'_, Message> = match kind {
        ShortcutNoticeKind::None => container(utility::description(
            "Select a shortcut, then press a new key combination.",
        ))
        .width(Fill)
        .center_y(SHORTCUT_STATUS_HEIGHT)
        .into(),
        ShortcutNoticeKind::Listening(command) => container(
            row![
                text("Listening").size(12).font(utility::semibold()),
                text(format!(
                    "Press a new key combination for {}.",
                    command.label()
                ))
                .size(12)
                .width(Fill),
                button(text("Cancel").size(12))
                    .padding([6, 9])
                    .style(styles::command_button)
                    .on_press(Message::ShortcutGroupSelected(dialog.shortcut_group)),
            ]
            .spacing(10)
            .align_y(Center),
        )
        .padding([8, 10])
        .width(Fill)
        .height(SHORTCUT_STATUS_HEIGHT)
        .style(styles::listening_notice)
        .into(),
        ShortcutNoticeKind::Conflict(conflict) => container(
            row![
                text("Conflict").size(12).font(utility::semibold()),
                text(format!(
                    "{} is already used for {}.",
                    conflict.binding.display(),
                    conflict.command.label()
                ))
                .size(12)
                .width(Fill),
                button(text("Dismiss").size(12))
                    .padding([6, 9])
                    .style(styles::command_button)
                    .on_press(Message::ShortcutConflictDismissed),
            ]
            .spacing(10)
            .align_y(Center),
        )
        .padding([8, 10])
        .width(Fill)
        .height(SHORTCUT_STATUS_HEIGHT)
        .style(styles::utility_notice)
        .into(),
    };

    let opacity = if fallback_current {
        1.0
    } else {
        animation.opacity()
    };
    let content = if kind == ShortcutNoticeKind::None {
        content
    } else {
        // Keep the notice in its fixed slot while its paint fades as a group.
        // The slot itself remains mounted, so the table never moves.
        motion::fade(content, opacity, styles::settings_panel_background, true)
    };

    container(content)
        .width(Fill)
        .height(SHORTCUT_STATUS_HEIGHT)
        .clip(true)
        .into()
}

fn shortcut_row(
    settings: &EditorSettings,
    command: ShortcutCommand,
    capturing: Option<ShortcutCommand>,
    pulse: f32,
) -> Element<'_, Message> {
    let recording = capturing == Some(command);
    let binding = settings.shortcuts.binding(command);
    let binding_view: Element<'_, Message> = if recording {
        text("Press keys…").size(12).into()
    } else {
        shortcut_binding_view(binding)
    };
    let binding_button: Element<'_, Message> = if recording {
        button(container(binding_view).center_x(Fill))
            .padding([6, 9])
            .width(174)
            .style(styles::listening_command_button(pulse))
            .on_press(Message::ShortcutCaptureStarted(command))
            .into()
    } else {
        button(container(binding_view).center_x(Fill))
            .padding([6, 9])
            .width(174)
            .style(styles::command_button)
            .on_press(Message::ShortcutCaptureStarted(command))
            .into()
    };
    container(
        row![
            text(command.label()).size(13).width(Fill),
            binding_button,
            button(text("Clear").size(12))
                .padding([6, 5])
                .style(styles::text_button)
                .on_press_maybe(binding.map(|_| Message::ShortcutCleared(command))),
        ]
        .spacing(8)
        .align_y(Center),
    )
    .padding([8, 12])
    .width(Fill)
    .into()
}

fn shortcut_binding_view(binding: Option<KeyBinding>) -> Element<'static, Message> {
    let Some(binding) = binding else {
        return utility::description("Assign shortcut");
    };
    let display = binding.display_parts();
    let mut parts = row![].spacing(4).align_y(Center);
    for modifier in display.modifiers {
        let part: Element<'_, Message> = match modifier {
            ShortcutDisplayPart::Text(label) => text(label).size(12).into(),
            ShortcutDisplayPart::Icon(icon) => shortcut::icon_with_color(
                match icon {
                    ShortcutModifierIcon::Command => ShortcutIcon::Command,
                    ShortcutModifierIcon::Option => ShortcutIcon::Option,
                    ShortcutModifierIcon::Shift => ShortcutIcon::Shift,
                    ShortcutModifierIcon::Windows => ShortcutIcon::Windows,
                },
                14,
                styles::shortcut_text_color,
            ),
        };
        parts = parts.push(part);
    }
    parts
        .push(text(display.key).size(12).font(utility::semibold()))
        .into()
}

fn section<'a>(title: &'static str, content: Element<'a, Message>) -> Element<'a, Message> {
    container(column![text(title).size(15).font(utility::semibold()), content,].spacing(12))
        .padding(12)
        .width(Fill)
        .style(styles::utility_card)
        .into()
}

fn setting_row<'a>(title: &'static str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(title).size(13).font(utility::semibold()).width(Fill),
        control
    ]
    .spacing(12)
    .align_y(Center)
    .into()
}

fn described_row<'a>(
    title: &'static str,
    description: impl Into<String>,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        column![
            text(title).size(13).font(utility::semibold()),
            container(text(description.into()).size(12)).style(styles::info_muted),
        ]
        .spacing(4)
        .width(Fill),
        control,
    ]
    .spacing(16)
    .align_y(Center)
    .into()
}

fn described_toggle<'a>(
    title: &'static str,
    description: impl Into<String>,
    enabled: bool,
    message: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    described_row(
        title,
        description,
        toggler(enabled)
            .style(styles::toggler)
            .size(20)
            .on_toggle(message)
            .into(),
    )
}

fn number_input<'a>(
    placeholder: &str,
    value: &str,
    message: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    // Inputs share the same control column as dropdowns and steppers.
    container(
        text_input(placeholder, value)
            .on_input(message)
            .style(styles::input)
            .width(100),
    )
    .align_right(CONTROL_WIDTH)
    .into()
}

fn divider() -> Element<'static, Message> {
    rule::horizontal(1).style(styles::utility_rule).into()
}

fn toggle_row<'a>(
    title: &'static str,
    enabled: bool,
    message: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    setting_row(
        title,
        toggler(enabled)
            .style(styles::toggler)
            .size(20)
            .on_toggle(message)
            .into(),
    )
}

fn stepper<'a>(
    value: String,
    decrease: Message,
    increase: Message,
    reset: Message,
    can_decrease: bool,
    can_increase: bool,
) -> Element<'a, Message> {
    let icon = |icon| hero::icon(icon, 14, IconTone::Text);
    let controls = row![
        button(icon(HeroIcon::Minus))
            .padding(6)
            .style(styles::command_button)
            .on_press_maybe(can_decrease.then_some(decrease)),
        container(text(value).size(13).font(utility::semibold())).center_x(58),
        button(icon(HeroIcon::Plus))
            .padding(6)
            .style(styles::command_button)
            .on_press_maybe(can_increase.then_some(increase)),
        controls::compact_command_button("Reset", 12, reset),
    ]
    .spacing(4)
    .align_y(Center);
    container(controls).align_right(CONTROL_WIDTH).into()
}

fn footer_button(
    label: &'static str,
    message: Option<Message>,
    primary: bool,
) -> Element<'static, Message> {
    button(container(text(label).size(13)).center_x(if primary { 84 } else { 54 }))
        .padding([7, 10])
        .style(if primary && message.is_some() {
            styles::primary_command_button
        } else {
            styles::command_button
        })
        .on_press_maybe(message)
        .into()
}

fn indentation_label(indentation: &IndentationMode) -> String {
    match indentation {
        IndentationMode::Tabs => "Tabs".into(),
        IndentationMode::Spaces(width) => format!("{width} spaces"),
    }
}
