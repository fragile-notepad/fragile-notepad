#[cfg(test)]
mod tests;

use iced::highlighter;
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, stack, text, tooltip,
};
use iced::{Center, Element, Fill};

use crate::core::{Document, EditorSettings, ShortcutCommand, TextEncoding};
use crate::editor::EditorAction;
use crate::message::{BulkScope, Menu, Message};
use crate::ui::controls::centered_button_content;
use crate::ui::icons::colored::{self, ColoredIcon};
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::menu::{self, MenuNode, MenuTree};
use crate::ui::styles;

const MENU_BAR_PADDING: [u16; 2] = [1, 4];
const MENU_LABEL_PADDING: [u16; 2] = [2, 8];
const MENU_ITEMS: &[(Menu, &str)] = &[
    (Menu::File, "File"),
    (Menu::Edit, "Edit"),
    (Menu::Search, "Search"),
    (Menu::View, "View"),
    (Menu::Encoding, "Encoding"),
    (Menu::Language, "Language"),
    (Menu::Settings, "Settings"),
    (Menu::Window, "Window"),
    (Menu::Help, "?"),
];

/// The native macOS menu uses the same commands and availability as the
/// in-window menu on other platforms.
#[cfg(target_os = "macos")]
pub(crate) fn native_menu_trees(
    state: WindowMenuState,
    settings: &EditorSettings,
    document: Option<&Document>,
) -> Vec<(&'static str, MenuTree)> {
    MENU_ITEMS
        .iter()
        .map(|(kind, label)| {
            let mut tree = menu_tree(*kind, state.clone(), settings);
            apply_editor_availability(&mut tree.entries, document);
            (if *kind == Menu::Help { "Help" } else { *label }, tree)
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub(crate) fn native_menu_availability(document: Option<&Document>) -> Vec<bool> {
    ShortcutCommand::ALL
        .into_iter()
        .map(Message::Shortcut)
        .chain([
            Message::Delete,
            Message::Uppercase,
            Message::Lowercase,
            Message::TrimTrailingSpaces,
            Message::JoinLines,
        ])
        .map(|message| editor_command_available(&message, document))
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowMenuState {
    pub open_window_count: usize,
    pub editor_windows: Vec<(iced::window::Id, String)>,
}

impl WindowMenuState {
    const fn can_cycle(&self) -> bool {
        self.open_window_count > 1
    }
}

pub fn menu_bar<'a>(active_menu: Option<Menu>) -> Element<'a, Message> {
    let menu_items = MENU_ITEMS
        .iter()
        .fold(row![].spacing(0), |row, (menu, label)| {
            row.push(menu_button(label, *menu, active_menu))
        });

    container(
        menu_items
            .push(space::horizontal())
            .padding(MENU_BAR_PADDING)
            .align_y(Center)
            .width(Fill),
    )
    .height(23)
    .width(Fill)
    .style(styles::menu_bar)
    .into()
}

pub fn menu_overlay<'a>(
    active_menu: Option<Menu>,
    active_path: &'a [String],
    window_menu_state: WindowMenuState,
    settings: &'a EditorSettings,
    document: Option<&'a Document>,
) -> Element<'a, Message> {
    let Some(menu) = active_menu else {
        return space::horizontal().into();
    };

    stack![
        mouse_area(space::vertical().height(Fill).width(Fill)).on_press(Message::MenuClosed),
        column![
            mouse_area(
                column![
                    menu_bar(active_menu),
                    row![
                        menu_prefix(menu),
                        opaque(menu_drop_down(
                            menu,
                            active_path,
                            window_menu_state,
                            settings,
                            document
                        )),
                        space::horizontal(),
                    ]
                    .spacing(0)
                    .padding(iced::Padding {
                        top: 4.0,
                        right: f32::from(MENU_BAR_PADDING[1]),
                        bottom: 8.0,
                        left: f32::from(MENU_BAR_PADDING[1]),
                    })
                    .width(Fill),
                ]
                .width(Fill)
            ),
            space::vertical(),
        ]
        .width(Fill)
        .height(Fill),
    ]
    .into()
}

fn menu_prefix<'a>(active_menu: Menu) -> Element<'a, Message> {
    MENU_ITEMS
        .iter()
        .take_while(|(menu, _)| *menu != active_menu)
        .fold(row![].spacing(0), |row, (menu, label)| {
            row.push(
                container(menu_label_content(label, *menu))
                    .padding(MENU_LABEL_PADDING)
                    .style(styles::transparent),
            )
        })
        .into()
}

pub fn tool_bar<'a>(
    document: Option<&Document>,
    settings: &EditorSettings,
    is_function_list_visible: bool,
) -> Element<'a, Message> {
    let files = tool_group([
        icon_button(ColoredIcon::New, "New", Message::NewFile),
        icon_button(ColoredIcon::Open, "Open", Message::OpenFile),
        icon_button(ColoredIcon::Save, "Save", Message::SaveFile),
        icon_button(
            ColoredIcon::SaveAll,
            "Save all in this window",
            Message::SaveScoped(BulkScope::ThisWindow),
        ),
        icon_button(ColoredIcon::Close, "Close", Message::CloseFile),
        icon_button(
            ColoredIcon::CloseAll,
            "Close all in this window",
            Message::CloseScoped(BulkScope::ThisWindow),
        ),
    ]);
    let editing = tool_group([
        editor_icon_button(ColoredIcon::Undo, "Undo", Message::Undo, document),
        editor_icon_button(ColoredIcon::Redo, "Redo", Message::Redo, document),
        editor_icon_button(ColoredIcon::Cut, "Cut", Message::Cut, document),
        editor_icon_button(ColoredIcon::Copy, "Copy", Message::Copy, document),
        editor_icon_button(ColoredIcon::Paste, "Paste", Message::Paste, document),
    ]);
    let search = tool_group([
        icon_button(ColoredIcon::Find, "Find", Message::ToggleFind),
        icon_button(ColoredIcon::Replace, "Replace", Message::ShowInlineReplace),
    ]);
    let decorations = &settings.decorations;
    let view = tool_group([
        icon_button(ColoredIcon::ZoomOut, "Zoom out", Message::ZoomOut),
        icon_button(ColoredIcon::ZoomIn, "Zoom in", Message::ZoomIn),
        toggle_icon_button(
            ColoredIcon::WordWrap,
            "Word wrap",
            Message::ToggleWordWrap,
            settings.word_wrap,
        ),
        toggle_icon_button(
            ColoredIcon::AllCharacters,
            "Show all characters",
            Message::ToggleAllCharacters,
            decorations.show_spaces
                && decorations.show_tabs
                && decorations.show_end_of_line_markers,
        ),
        toggle_icon_button(
            ColoredIcon::IndentGuide,
            "Indent guide",
            Message::ToggleIndentationGuides,
            decorations.show_indentation_guides,
        ),
        toggle_icon_button(
            ColoredIcon::FunctionList,
            "Function List",
            Message::ToggleFunctionList,
            is_function_list_visible,
        ),
    ]);
    let buttons = row![
        files,
        separator(),
        editing,
        separator(),
        search,
        separator(),
        view
    ]
    .padding([4, 8])
    .align_y(Center);

    container(
        scrollable(buttons)
            .style(styles::scrollable)
            .smooth_scroll(true)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(3).scroller_width(3),
            ))
            .width(Fill)
            .height(Fill),
    )
    .height(36)
    .width(Fill)
    .style(styles::tool_bar)
    .into()
}

fn tool_group<'a>(buttons: impl IntoIterator<Item = Element<'a, Message>>) -> Element<'a, Message> {
    row(buttons).spacing(1).align_y(Center).into()
}

fn menu_button<'a>(
    label: &'static str,
    menu: Menu,
    active_menu: Option<Menu>,
) -> Element<'a, Message> {
    mouse_area(
        container(menu_label_content(label, menu))
            .padding(MENU_LABEL_PADDING)
            .style(styles::menu_label(active_menu == Some(menu))),
    )
    .on_press(Message::MenuToggled(menu))
    .on_enter(Message::MenuHovered(menu))
    .into()
}

fn menu_label_content<'a>(label: &'static str, menu: Menu) -> Element<'a, Message> {
    if menu == Menu::Help {
        hero::icon(HeroIcon::QuestionMarkCircle, 15, IconTone::Text)
    } else {
        text(label).size(13).into()
    }
}

fn menu_drop_down<'a>(
    menu_kind: Menu,
    active_path: &'a [String],
    window_menu_state: WindowMenuState,
    settings: &'a EditorSettings,
    document: Option<&'a Document>,
) -> Element<'a, Message> {
    let mut tree = menu_tree(menu_kind, window_menu_state, settings);
    apply_editor_availability(&mut tree.entries, document);
    menu::view(menu_kind, tree, active_path)
}

fn menu_tree(
    menu_kind: Menu,
    window_menu_state: WindowMenuState,
    settings: &EditorSettings,
) -> MenuTree {
    match menu_kind {
        Menu::File => MenuTree::new(file_menu_entries(settings), menu_width(menu_kind)),
        Menu::Edit => MenuTree::new(edit_menu_entries(settings), menu_width(menu_kind)),
        Menu::Search => MenuTree::new(search_menu_entries(settings), menu_width(menu_kind)),
        Menu::View => MenuTree::new(view_menu_entries(settings), menu_width(menu_kind)),
        Menu::Encoding => MenuTree::new(encoding_menu_entries(), menu_width(menu_kind)),
        Menu::Language => MenuTree::new(
            highlighter::syntaxes()
                .iter()
                .map(|syntax| {
                    menu::item(
                        syntax.name.clone(),
                        Message::LanguageSelected(syntax.token.clone()),
                    )
                })
                .collect(),
            menu_width(menu_kind),
        )
        .max_height(360.0),
        Menu::Settings => MenuTree::new(
            vec![menu::item("Preferences...", Message::ToggleSettingsPanel)],
            menu_width(menu_kind),
        ),
        Menu::Window => MenuTree::new(
            window_menu_entries(window_menu_state, settings),
            menu_width(menu_kind),
        ),
        Menu::Help => MenuTree::new(
            vec![menu::item("About Fragile Notepad", Message::AboutOpened)],
            menu_width(menu_kind),
        ),
    }
}

fn window_menu_entries(state: WindowMenuState, settings: &EditorSettings) -> Vec<MenuNode> {
    let mut entries = vec![
        menu_item(
            settings,
            "New Window",
            ShortcutCommand::NewWindow,
            Message::NewWindow,
        ),
        menu_item(
            settings,
            "Close Window",
            ShortcutCommand::CloseWindow,
            Message::CloseWindow,
        ),
        menu::separator(),
        menu::item(
            "Move Selected Tabs to New Window",
            Message::MoveTabsToNewWindow,
        ),
    ];
    if !state.editor_windows.is_empty() {
        entries.push(menu::submenu(
            "move-tabs-to-window",
            "Move Selected Tabs to Window",
            state
                .editor_windows
                .iter()
                .map(|(id, title)| menu::item(title.clone(), Message::MoveTabsToWindow(*id)))
                .collect(),
        ));
    }
    entries.extend([
        menu::separator(),
        menu::item("Windows...", Message::WindowListOpened),
        menu::item("Tabs...", Message::TabListOpened),
        window_cycle_item("Next Window", Message::WindowFocusNext, &state),
        window_cycle_item("Previous Window", Message::WindowFocusPrevious, &state),
    ]);
    entries
}

fn window_cycle_item(label: &'static str, message: Message, state: &WindowMenuState) -> MenuNode {
    if state.can_cycle() {
        menu::item(label, message)
    } else {
        menu::disabled(label)
    }
}

fn file_menu_entries(settings: &EditorSettings) -> Vec<MenuNode> {
    let mut entries = vec![
        menu_item(settings, "New", ShortcutCommand::NewFile, Message::NewFile),
        menu_item(
            settings,
            "New Window",
            ShortcutCommand::NewWindow,
            Message::NewWindow,
        ),
        menu_item(
            settings,
            "Open...",
            ShortcutCommand::OpenFile,
            Message::OpenFile,
        ),
        menu_item(
            settings,
            "Save",
            ShortcutCommand::SaveFile,
            Message::SaveFile,
        ),
        menu_item(
            settings,
            "Save As...",
            ShortcutCommand::SaveFileAs,
            Message::SaveFileAs,
        ),
        menu::item("Save a Copy As...", Message::SaveCopyAs),
        menu::submenu(
            "save-scoped",
            "Save Multiple Documents",
            vec![
                menu::item(
                    "Save Selected Tabs",
                    Message::SaveScoped(BulkScope::SelectedTabs),
                ),
                menu::item(
                    "Save This Window",
                    Message::SaveScoped(BulkScope::ThisWindow),
                ),
                menu::item(
                    "Save All Windows",
                    Message::SaveScoped(BulkScope::AllWindows),
                ),
            ],
        ),
        menu::separator(),
        menu::item("Reload from Disk", Message::ReloadFromDisk),
        menu::separator(),
        menu_item(
            settings,
            "Close",
            ShortcutCommand::CloseFile,
            Message::CloseFile,
        ),
        menu::submenu(
            "close-scoped",
            "Close Tabs",
            vec![
                menu::item(
                    "Close Selected Tabs",
                    Message::CloseScoped(BulkScope::SelectedTabs),
                ),
                menu::item(
                    "Close This Window's Tabs",
                    Message::CloseScoped(BulkScope::ThisWindow),
                ),
                menu::item(
                    "Close All Windows' Tabs",
                    Message::CloseScoped(BulkScope::AllWindows),
                ),
            ],
        ),
        close_multiple_documents_menu(),
        menu::separator(),
        menu_item(settings, "Quit", ShortcutCommand::Quit, Message::Quit),
    ];
    if !settings.open_history.is_empty() {
        entries.push(menu::separator());
        entries.push(menu::submenu(
            "recent-files",
            "Recent Files",
            settings
                .open_history
                .iter()
                .map(|path| {
                    menu::item(
                        path.to_string_lossy().into_owned(),
                        Message::OpenPaths(vec![path.clone()]),
                    )
                })
                .collect(),
        ));
    }
    entries
}

pub(super) fn edit_menu_entries(settings: &EditorSettings) -> Vec<MenuNode> {
    vec![
        menu_item(settings, "Undo", ShortcutCommand::Undo, Message::Undo),
        menu_item(settings, "Redo", ShortcutCommand::Redo, Message::Redo),
        menu::separator(),
        menu_item(settings, "Cut", ShortcutCommand::Cut, Message::Cut),
        menu_item(settings, "Copy", ShortcutCommand::Copy, Message::Copy),
        menu_item(settings, "Paste", ShortcutCommand::Paste, Message::Paste),
        menu::item_with_shortcut("Delete", "Del", Message::Delete),
        menu_item(
            settings,
            "Select All",
            ShortcutCommand::SelectAll,
            Message::Shortcut(ShortcutCommand::SelectAll),
        ),
        menu::separator(),
        selection_operations_menu(settings),
        line_operations_menu(settings),
        indent_menu(settings),
        transformation_menu(),
    ]
}

fn search_menu_entries(settings: &EditorSettings) -> Vec<MenuNode> {
    let mut entries = vec![
        menu_item(
            settings,
            "Find...",
            ShortcutCommand::ToggleFind,
            Message::ToggleFind,
        ),
        menu::item("Replace...", Message::ShowInlineReplace),
        menu_item(
            settings,
            "Advanced Find...",
            ShortcutCommand::AdvancedFind,
            Message::ToggleAdvancedSearch(crate::message::AdvancedSearchTab::Find),
        ),
        menu_item(
            settings,
            "Advanced Replace...",
            ShortcutCommand::AdvancedReplace,
            Message::ToggleAdvancedSearch(crate::message::AdvancedSearchTab::Replace),
        ),
        menu_item(
            settings,
            "Go To Line...",
            ShortcutCommand::GoToLine,
            Message::GoToLineOpened,
        ),
        menu::separator(),
    ];

    entries.extend(matching_and_function_items(settings));
    entries
}

fn view_menu_entries(settings: &EditorSettings) -> Vec<MenuNode> {
    vec![
        menu::item("Word Wrap", Message::ToggleWordWrap),
        menu::item("Line Numbers", Message::ToggleLineNumbers),
        show_symbol_menu(),
        menu::item("Function List", Message::ToggleFunctionList),
        zoom_menu(settings),
        menu::separator(),
        fold_commands_menu(settings),
        menu::item("Folding Controls", Message::ToggleFoldingControls),
    ]
}

pub(super) fn apply_editor_availability(entries: &mut [MenuNode], document: Option<&Document>) {
    for entry in entries {
        match entry {
            MenuNode::Item {
                label,
                shortcut,
                message,
            } if !editor_command_available(message, document) => {
                *entry = MenuNode::Disabled {
                    label: label.clone(),
                    shortcut: shortcut.clone(),
                    message: Some(message.clone()),
                };
            }
            MenuNode::Submenu { children, .. } => apply_editor_availability(children, document),
            _ => {}
        }
    }
}

fn editor_command_available(message: &Message, document: Option<&Document>) -> bool {
    let action = match message {
        Message::EditorAction(_, action) => Some(action.clone()),
        Message::Shortcut(command) => EditorAction::from_shortcut(*command),
        Message::Undo => Some(EditorAction::Undo),
        Message::Redo => Some(EditorAction::Redo),
        Message::Cut => Some(EditorAction::Cut),
        Message::Copy => Some(EditorAction::Copy),
        Message::Paste => Some(EditorAction::Paste),
        Message::Delete => Some(EditorAction::Delete),
        Message::Uppercase => Some(EditorAction::Uppercase),
        Message::Lowercase => Some(EditorAction::Lowercase),
        Message::TrimTrailingSpaces => Some(EditorAction::TrimTrailingSpaces),
        Message::JoinLines => Some(EditorAction::JoinLines),
        Message::FoldCurrent => Some(EditorAction::FoldCurrent),
        Message::UnfoldCurrent => Some(EditorAction::UnfoldCurrent),
        Message::ToggleCurrentFold => Some(EditorAction::ToggleCurrentFold),
        Message::FoldAll => Some(EditorAction::FoldAll),
        Message::UnfoldAll => Some(EditorAction::UnfoldAll),
        Message::GoToMatchingDelimiter => Some(EditorAction::GoToMatchingDelimiter),
        Message::SelectMatchingDelimiter => Some(EditorAction::SelectMatchingDelimiter),
        Message::NextFunction => Some(EditorAction::NextFunction),
        Message::PreviousFunction => Some(EditorAction::PreviousFunction),
        Message::SelectCurrentFunction => Some(EditorAction::SelectCurrentFunction),
        Message::SelectCurrentFunctionBody => Some(EditorAction::SelectCurrentFunctionBody),
        _ => None,
    };
    let Some(action) = action else {
        return true;
    };
    let Some(document) = document else {
        return false;
    };
    let complete = document.has_complete_text_index();
    if action.mutates_document() && !complete {
        return false;
    }
    let has_text = document.buffer.len_bytes() > 0;
    match action {
        EditorAction::Undo => document.can_undo(),
        EditorAction::Redo => document.can_redo(),
        EditorAction::Copy | EditorAction::CopyLine => has_text,
        EditorAction::Cut | EditorAction::CutLine | EditorAction::DeleteLine => has_text,
        EditorAction::Uppercase | EditorAction::Lowercase => document
            .selection_set()
            .ranges()
            .iter()
            .any(|range| !range.range().is_empty()),
        EditorAction::Delete => document.selection_set().ranges().iter().any(|range| {
            !range.range().is_empty()
                || range.cursor < crate::editor::document_end(&document.buffer)
        }),
        EditorAction::SelectAll => has_text,
        EditorAction::FoldAll => document
            .folds
            .ranges()
            .iter()
            .any(|range| !document.folds.is_collapsed(*range)),
        EditorAction::UnfoldAll => document.folds.collapsed_ranges().next().is_some(),
        EditorAction::FoldCurrent
        | EditorAction::UnfoldCurrent
        | EditorAction::ToggleCurrentFold => document
            .folds
            .range_at_or_parent(document.main_selection().cursor.line)
            .is_some_and(|range| match action {
                EditorAction::FoldCurrent => !document.folds.is_collapsed(range),
                EditorAction::UnfoldCurrent => document.folds.is_collapsed(range),
                _ => true,
            }),
        EditorAction::NextFunction
        | EditorAction::PreviousFunction
        | EditorAction::SelectCurrentFunction
        | EditorAction::SelectCurrentFunctionBody => document.can_run_full_document_analysis(),
        _ => true,
    }
}

fn editor_icon_button<'a>(
    icon: ColoredIcon,
    label: &'static str,
    message: Message,
    document: Option<&Document>,
) -> Element<'a, Message> {
    if editor_command_available(&message, document) {
        icon_button(icon, label, message)
    } else {
        disabled_icon_button(icon, label)
    }
}

pub(super) fn menu_item(
    settings: &EditorSettings,
    label: impl Into<String>,
    command: ShortcutCommand,
    message: Message,
) -> MenuNode {
    if let Some(binding) = settings.shortcuts.binding(command) {
        menu::item_with_shortcut_binding(label, binding, message)
    } else {
        menu::item(label, message)
    }
}

fn close_multiple_documents_menu() -> MenuNode {
    menu::submenu(
        "close-multiple",
        "Close Multiple Documents",
        vec![
            menu::item(
                "Close All but Active Document",
                Message::CloseAllButActiveFile,
            ),
            menu::item(
                "Close All but Pinned Documents",
                Message::CloseAllButPinnedFiles,
            ),
            menu::item("Close All to the Left", Message::CloseAllToLeft),
            menu::item("Close All to the Right", Message::CloseAllToRight),
            menu::item("Close All Unchanged", Message::CloseAllUnchanged),
        ],
    )
}

fn indent_menu(settings: &EditorSettings) -> MenuNode {
    menu::submenu(
        "indent",
        "Indent",
        vec![
            menu_item(
                settings,
                "Increase Line Indent",
                ShortcutCommand::Indent,
                Message::Shortcut(ShortcutCommand::Indent),
            ),
            menu_item(
                settings,
                "Decrease Line Indent",
                ShortcutCommand::Unindent,
                Message::Shortcut(ShortcutCommand::Unindent),
            ),
        ],
    )
}

fn transformation_menu() -> MenuNode {
    menu::submenu(
        "transformations",
        "Convert Case / Transform",
        vec![
            menu::item("UPPERCASE", Message::Uppercase),
            menu::item("lowercase", Message::Lowercase),
            menu::item("Trim Trailing Spaces", Message::TrimTrailingSpaces),
            menu::item("Join Lines", Message::JoinLines),
        ],
    )
}

fn selection_operations_menu(settings: &EditorSettings) -> MenuNode {
    menu::submenu(
        "selection-operations",
        "Selection",
        vec![
            menu_item(
                settings,
                "Add Caret Above",
                ShortcutCommand::AddCaretAbove,
                Message::Shortcut(ShortcutCommand::AddCaretAbove),
            ),
            menu_item(
                settings,
                "Add Caret Below",
                ShortcutCommand::AddCaretBelow,
                Message::Shortcut(ShortcutCommand::AddCaretBelow),
            ),
            menu_item(
                settings,
                "Split Selection into Lines",
                ShortcutCommand::SplitSelectionIntoLines,
                Message::Shortcut(ShortcutCommand::SplitSelectionIntoLines),
            ),
            menu_item(
                settings,
                "Convert Selection to Rectangle",
                ShortcutCommand::ConvertSelectionToRectangle,
                Message::Shortcut(ShortcutCommand::ConvertSelectionToRectangle),
            ),
        ],
    )
}

fn line_operations_menu(settings: &EditorSettings) -> MenuNode {
    menu::submenu(
        "line-operations",
        "Line Operations",
        vec![
            menu_item(
                settings,
                "Duplicate Current Line",
                ShortcutCommand::DuplicateLine,
                Message::Shortcut(ShortcutCommand::DuplicateLine),
            ),
            menu_item(
                settings,
                "Delete Current Line",
                ShortcutCommand::DeleteLine,
                Message::Shortcut(ShortcutCommand::DeleteLine),
            ),
            menu_item(
                settings,
                "Copy Current Line",
                ShortcutCommand::CopyLine,
                Message::Shortcut(ShortcutCommand::CopyLine),
            ),
            menu_item(
                settings,
                "Cut Current Line",
                ShortcutCommand::CutLine,
                Message::Shortcut(ShortcutCommand::CutLine),
            ),
        ],
    )
}

pub(super) fn matching_and_function_items(settings: &EditorSettings) -> Vec<MenuNode> {
    vec![
        menu_item(
            settings,
            "Go to Matching Brace",
            ShortcutCommand::GoToMatchingDelimiter,
            Message::GoToMatchingDelimiter,
        ),
        menu_item(
            settings,
            "Select All In-between {} [] or ()",
            ShortcutCommand::SelectMatchingDelimiter,
            Message::SelectMatchingDelimiter,
        ),
        menu_item(
            settings,
            "Next Function",
            ShortcutCommand::NextFunction,
            Message::NextFunction,
        ),
        menu_item(
            settings,
            "Previous Function",
            ShortcutCommand::PreviousFunction,
            Message::PreviousFunction,
        ),
        menu_item(
            settings,
            "Select Current Function",
            ShortcutCommand::SelectCurrentFunction,
            Message::SelectCurrentFunction,
        ),
        menu_item(
            settings,
            "Select Current Function Body",
            ShortcutCommand::SelectCurrentFunctionBody,
            Message::SelectCurrentFunctionBody,
        ),
    ]
}

fn show_symbol_menu() -> MenuNode {
    menu::submenu(
        "show-symbol",
        "Show Symbol",
        vec![
            menu::item("Show Space and Tab", Message::ToggleSpaceAndTab),
            menu::item("Show End of Line", Message::ToggleEolMarkers),
            menu::disabled("Show Non-Printing Characters"),
            menu::disabled("Show Control Characters && Unicode EOL"),
            menu::item("Show All Characters", Message::ToggleAllCharacters),
            menu::separator(),
            menu::item("Show Indent Guide", Message::ToggleIndentationGuides),
            menu::disabled("Show Wrap Symbol"),
        ],
    )
}

fn zoom_menu(settings: &EditorSettings) -> MenuNode {
    menu::submenu(
        "zoom",
        "Zoom",
        vec![
            menu_item(
                settings,
                "Zoom In (Ctrl+Mouse Wheel Up)",
                ShortcutCommand::ZoomIn,
                Message::ZoomIn,
            ),
            menu_item(
                settings,
                "Zoom Out (Ctrl+Mouse Wheel Down)",
                ShortcutCommand::ZoomOut,
                Message::ZoomOut,
            ),
            menu_item(
                settings,
                "Restore Default Zoom",
                ShortcutCommand::ZoomReset,
                Message::ZoomReset,
            ),
        ],
    )
}

pub(super) fn fold_commands_menu(settings: &EditorSettings) -> MenuNode {
    menu::submenu(
        "fold",
        "Fold",
        vec![
            menu_item(
                settings,
                "Fold All",
                ShortcutCommand::FoldAll,
                Message::FoldAll,
            ),
            menu_item(
                settings,
                "Unfold All",
                ShortcutCommand::UnfoldAll,
                Message::UnfoldAll,
            ),
            menu_item(
                settings,
                "Fold Current Level",
                ShortcutCommand::FoldCurrent,
                Message::FoldCurrent,
            ),
            menu_item(
                settings,
                "Unfold Current Level",
                ShortcutCommand::UnfoldCurrent,
                Message::UnfoldCurrent,
            ),
            menu_item(
                settings,
                "Toggle Current Fold",
                ShortcutCommand::ToggleCurrentFold,
                Message::ToggleCurrentFold,
            ),
        ],
    )
}

fn encoding_menu_entries() -> Vec<MenuNode> {
    vec![
        menu::item("ANSI", Message::EncodingSelected(TextEncoding::Windows1252)),
        menu::item("UTF-8", Message::EncodingSelected(TextEncoding::Utf8)),
        menu::item(
            "UTF-8-BOM",
            Message::EncodingSelected(TextEncoding::Utf8Bom),
        ),
        menu::item(
            "UTF-16 BE BOM",
            Message::EncodingSelected(TextEncoding::Utf16BeBom),
        ),
        menu::item(
            "UTF-16 LE BOM",
            Message::EncodingSelected(TextEncoding::Utf16LeBom),
        ),
        menu::submenu(
            "character-sets",
            "Character sets",
            encoding_character_sets(),
        ),
        menu::separator(),
        menu::item(
            "Convert to ANSI",
            Message::EncodingConverted(TextEncoding::Windows1252),
        ),
        menu::item(
            "Convert to UTF-8",
            Message::EncodingConverted(TextEncoding::Utf8),
        ),
        menu::item(
            "Convert to UTF-8-BOM",
            Message::EncodingConverted(TextEncoding::Utf8Bom),
        ),
        menu::item(
            "Convert to UTF-16 BE BOM",
            Message::EncodingConverted(TextEncoding::Utf16BeBom),
        ),
        menu::item(
            "Convert to UTF-16 LE BOM",
            Message::EncodingConverted(TextEncoding::Utf16LeBom),
        ),
    ]
}

fn encoding_character_sets() -> Vec<MenuNode> {
    vec![
        menu::submenu(
            "arabic",
            "Arabic",
            vec![
                menu::item(
                    "ISO 8859-6",
                    Message::EncodingSelected(TextEncoding::Iso8859_6),
                ),
                menu::item("OEM 720", Message::EncodingSelected(TextEncoding::Oem720)),
                menu::item(
                    "Windows-1256",
                    Message::EncodingSelected(TextEncoding::Windows1256),
                ),
            ],
        ),
        menu::submenu(
            "baltic",
            "Baltic",
            vec![
                menu::item(
                    "ISO 8859-4",
                    Message::EncodingSelected(TextEncoding::Iso8859_4),
                ),
                menu::item(
                    "ISO 8859-13",
                    Message::EncodingSelected(TextEncoding::Iso8859_13),
                ),
                menu::item("OEM 775", Message::EncodingSelected(TextEncoding::Oem775)),
                menu::item(
                    "Windows-1257",
                    Message::EncodingSelected(TextEncoding::Windows1257),
                ),
            ],
        ),
        menu::submenu(
            "celtic",
            "Celtic",
            vec![menu::item(
                "ISO 8859-14",
                Message::EncodingSelected(TextEncoding::Iso8859_14),
            )],
        ),
        menu::submenu(
            "cyrillic",
            "Cyrillic",
            vec![
                menu::item(
                    "ISO 8859-5",
                    Message::EncodingSelected(TextEncoding::Iso8859_5),
                ),
                menu::item("KOI8-R", Message::EncodingSelected(TextEncoding::Koi8R)),
                menu::item("KOI8-U", Message::EncodingSelected(TextEncoding::Koi8U)),
                menu::item(
                    "Macintosh",
                    Message::EncodingSelected(TextEncoding::Macintosh),
                ),
                menu::item("OEM 855", Message::EncodingSelected(TextEncoding::Oem855)),
                menu::item("OEM 866", Message::EncodingSelected(TextEncoding::Oem866)),
                menu::item(
                    "Windows-1251",
                    Message::EncodingSelected(TextEncoding::Windows1251),
                ),
            ],
        ),
        menu::submenu(
            "central-european",
            "Central European",
            vec![
                menu::item("OEM 852", Message::EncodingSelected(TextEncoding::Oem852)),
                menu::item(
                    "Windows-1250",
                    Message::EncodingSelected(TextEncoding::Windows1250),
                ),
            ],
        ),
        menu::submenu(
            "chinese",
            "Chinese",
            vec![
                menu::item(
                    "Big5 (Traditional)",
                    Message::EncodingSelected(TextEncoding::Big5),
                ),
                menu::item(
                    "GB18030 (Simplified)",
                    Message::EncodingSelected(TextEncoding::Gb18030),
                ),
            ],
        ),
        menu::submenu(
            "eastern-european",
            "Eastern European",
            vec![menu::item(
                "ISO 8859-2",
                Message::EncodingSelected(TextEncoding::Iso8859_2),
            )],
        ),
        menu::submenu(
            "greek",
            "Greek",
            vec![
                menu::item(
                    "ISO 8859-7",
                    Message::EncodingSelected(TextEncoding::Iso8859_7),
                ),
                menu::item("OEM 737", Message::EncodingSelected(TextEncoding::Oem737)),
                menu::item("OEM 869", Message::EncodingSelected(TextEncoding::Oem869)),
                menu::item(
                    "Windows-1253",
                    Message::EncodingSelected(TextEncoding::Windows1253),
                ),
            ],
        ),
        menu::submenu(
            "hebrew",
            "Hebrew",
            vec![
                menu::item(
                    "ISO 8859-8",
                    Message::EncodingSelected(TextEncoding::Iso8859_8),
                ),
                menu::item("OEM 862", Message::EncodingSelected(TextEncoding::Oem862)),
                menu::item(
                    "Windows-1255",
                    Message::EncodingSelected(TextEncoding::Windows1255),
                ),
            ],
        ),
        menu::submenu(
            "japanese",
            "Japanese",
            vec![menu::item(
                "Shift-JIS",
                Message::EncodingSelected(TextEncoding::ShiftJis),
            )],
        ),
        menu::submenu(
            "korean",
            "Korean",
            vec![
                menu::item(
                    "Windows 949",
                    Message::EncodingSelected(TextEncoding::EucKr),
                ),
                menu::item("EUC-KR", Message::EncodingSelected(TextEncoding::EucKr)),
            ],
        ),
        menu::submenu(
            "north-european",
            "North European",
            vec![
                menu::item(
                    "OEM 861 : Icelandic",
                    Message::EncodingSelected(TextEncoding::Oem861),
                ),
                menu::item(
                    "OEM 865 : Nordic",
                    Message::EncodingSelected(TextEncoding::Oem865),
                ),
            ],
        ),
        menu::submenu(
            "thai",
            "Thai",
            vec![menu::item(
                "TIS-620",
                Message::EncodingSelected(TextEncoding::Tis620),
            )],
        ),
        menu::submenu(
            "turkish",
            "Turkish",
            vec![
                menu::item(
                    "ISO 8859-3",
                    Message::EncodingSelected(TextEncoding::Iso8859_3),
                ),
                menu::item(
                    "ISO 8859-9",
                    Message::EncodingSelected(TextEncoding::Iso8859_9),
                ),
                menu::item("OEM 857", Message::EncodingSelected(TextEncoding::Oem857)),
                menu::item(
                    "Windows-1254",
                    Message::EncodingSelected(TextEncoding::Windows1254),
                ),
            ],
        ),
        menu::submenu(
            "western-european",
            "Western European",
            vec![
                menu::item(
                    "ISO 8859-1",
                    Message::EncodingSelected(TextEncoding::Iso8859_1),
                ),
                menu::item(
                    "ISO 8859-15",
                    Message::EncodingSelected(TextEncoding::Iso8859_15),
                ),
                menu::item("OEM 850", Message::EncodingSelected(TextEncoding::Oem850)),
                menu::item("OEM 858", Message::EncodingSelected(TextEncoding::Oem858)),
                menu::item(
                    "OEM 860 : Portuguese",
                    Message::EncodingSelected(TextEncoding::Oem860),
                ),
                menu::item(
                    "OEM 863 : French",
                    Message::EncodingSelected(TextEncoding::Oem863),
                ),
                menu::item("OEM-US", Message::EncodingSelected(TextEncoding::Oem437)),
                menu::item(
                    "Windows-1252",
                    Message::EncodingSelected(TextEncoding::Windows1252),
                ),
            ],
        ),
        menu::submenu(
            "vietnamese",
            "Vietnamese",
            vec![menu::item(
                "Windows-1258",
                Message::EncodingSelected(TextEncoding::Windows1258),
            )],
        ),
    ]
}

fn menu_width(menu: Menu) -> f32 {
    match menu {
        Menu::Language | Menu::Encoding => 260.0,
        Menu::Window => 244.0,
        _ => 202.0,
    }
}

fn icon_button<'a>(
    icon: ColoredIcon,
    label: &'static str,
    message: Message,
) -> Element<'a, Message> {
    toolbar_button(icon, label, Some(message), false)
}

fn disabled_icon_button<'a>(icon: ColoredIcon, label: &'static str) -> Element<'a, Message> {
    toolbar_button(icon, label, None, false)
}

fn toggle_icon_button<'a>(
    icon: ColoredIcon,
    label: &'static str,
    message: Message,
    is_selected: bool,
) -> Element<'a, Message> {
    toolbar_button(icon, label, Some(message), is_selected)
}

fn toolbar_button<'a>(
    icon: ColoredIcon,
    label: &'static str,
    message: Option<Message>,
    is_selected: bool,
) -> Element<'a, Message> {
    let icon = if message.is_some() {
        colored::icon(icon, 18)
    } else {
        colored::disabled_icon(icon, 18)
    };
    tooltip(
        button(centered_button_content(icon))
            .width(28)
            .height(28)
            .padding(0)
            .style(styles::toolbar_button(is_selected))
            .on_press_maybe(message),
        container(text(label).size(12))
            .padding([4, 7])
            .style(styles::tooltip),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn separator<'a>() -> Element<'a, Message> {
    // Paint only the line, leaving the space between groups transparent.
    container(container(space::horizontal().width(1).height(16)).style(styles::separator))
        .center_x(13)
        .center_y(28)
        .into()
}
