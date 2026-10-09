//! Render real utility windows in both themes at normal and minimum sizes.
//! `cargo run --locked --example preview_dialogs`
//! Select a panel: `cargo run --locked --example preview_dialogs -- --panel settings`.
//! List available panels: `cargo run --locked --example preview_dialogs -- --help`.
//! Images are written to target/dialog-review/.

use fragile_notepad::{
    core::{
        AppearanceMode, Document, DocumentId, EditorSettings, SearchMode, SearchResultSettings,
        ShortcutCommand, ShortcutConflict, ShortcutGroup, Workspace,
    },
    message::{AdvancedSearchTab, Message, SettingsCategory, WindowTarget},
    search_dialog::SearchDialogState,
    settings_dialog::SettingsDialogState,
    ui::{
        advanced_search_panel, go_to_line_prompt, settings_panel, styles,
        window_list_dialog::{self, WindowListEntry},
    },
};
use iced::advanced::graphics::core::shell::Waker;
use iced::advanced::renderer::{self, Headless, Renderer as _};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Shell, layout, mouse};
use iced::{Element, Event, Rectangle, Renderer, Size, Theme, window};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const USAGE: &str = "Usage: preview_dialogs [--panel <name>]
Panels: all (default), settings, search, go-to-line, windows, editor
Output: target/dialog-review/
Use --help to display this help.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewPanel {
    All,
    Settings,
    Search,
    GoToLine,
    Windows,
    Editor,
}

impl PreviewPanel {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "all" => Some(Self::All),
            "settings" => Some(Self::Settings),
            "search" => Some(Self::Search),
            "go-to-line" => Some(Self::GoToLine),
            "windows" => Some(Self::Windows),
            "editor" => Some(Self::Editor),
            _ => None,
        }
    }

    fn includes(self, panel: Self) -> bool {
        self == Self::All || self == panel
    }
}

fn parse_args() -> Result<Option<PreviewPanel>, String> {
    let mut args = std::env::args().skip(1);
    let mut panel = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--panel" => {
                if panel.is_some() {
                    return Err("--panel can only be specified once.".into());
                }
                let name = args.next().ok_or("--panel requires a panel name.")?;
                panel = Some(
                    PreviewPanel::parse(&name).ok_or_else(|| format!("Unknown panel: {name}"))?,
                );
            }
            _ => return Err(format!("Unknown argument: {argument}")),
        }
    }
    Ok(Some(panel.unwrap_or(PreviewPanel::All)))
}

fn main() -> ExitCode {
    let selected = match parse_args() {
        Ok(Some(panel)) => panel,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    std::fs::create_dir_all("target/dialog-review").unwrap();
    let mut renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    for (appearance, name) in [
        (AppearanceMode::Light, "light"),
        (AppearanceMode::Dark, "dark"),
    ] {
        let theme = styles::modern_theme(appearance).unwrap();
        for (width, height, size_name) in [(900, 600, "normal"), (640, 364, "minimum")] {
            if selected.includes(PreviewPanel::GoToLine) {
                for (value, error, suffix) in [
                    ("120", None, "ready"),
                    ("abc", Some("Enter a valid line number."), "invalid"),
                ] {
                    render(
                        &mut renderer,
                        go_to_line_prompt::view(value, error, 1.0, true),
                        &theme,
                        Size::new(width, height),
                        &format!("go-to-line-{suffix}-{name}-{size_name}"),
                    );
                }
                render(
                    &mut renderer,
                    go_to_line_prompt::view("120", None, 0.5, false),
                    &theme,
                    Size::new(width, height),
                    &format!("go-to-line-transition-{name}-{size_name}"),
                );
            }
            if selected.includes(PreviewPanel::Windows) {
                let entries = vec![
                    WindowListEntry {
                        target: WindowTarget::Main,
                        title: "Release notes — September.md - Fragile Notepad".into(),
                        is_focused: true,
                    },
                    WindowListEntry {
                        target: WindowTarget::AdvancedSearch,
                        title: "Find and Replace - Fragile Notepad".into(),
                        is_focused: false,
                    },
                    WindowListEntry {
                        target: WindowTarget::Settings,
                        title: "Settings - Fragile Notepad".into(),
                        is_focused: false,
                    },
                ];
                render(
                    &mut renderer,
                    window_list_dialog::view(entries),
                    &theme,
                    Size::new(width, height),
                    &format!("windows-{name}-{size_name}"),
                );
            }
        }
        if selected.includes(PreviewPanel::Search) {
            for (width, height, size_name) in [(800, 604, "normal"), (720, 524, "minimum")] {
                for (tab, tab_name) in [
                    (AdvancedSearchTab::Find, "find"),
                    (AdvancedSearchTab::Replace, "replace"),
                    (AdvancedSearchTab::FindInFiles, "find-open"),
                    (AdvancedSearchTab::ReplaceInFiles, "replace-open"),
                ] {
                    let mut dialog = SearchDialogState::new();
                    dialog.active_tab = tab;
                    if tab != AdvancedSearchTab::Find {
                        dialog.query = "release".into();
                        dialog.replacement = "launch".into();
                        let mut workspace = Workspace::new();
                        workspace.push_document(Document::from_path(DocumentId::new(20), "release-notes.md", "# September release\nPrepare the release notes.\nReview the release checklist.\n"));
                        workspace.push_document(Document::from_path(
                            DocumentId::new(21),
                            "src/main.rs",
                            "fn prepare_release() {}\n// Publish the release after review.",
                        ));
                        if matches!(
                            tab,
                            AdvancedSearchTab::FindInFiles | AdvancedSearchTab::ReplaceInFiles
                        ) {
                            dialog.refresh_from_workspace(&workspace);
                        } else {
                            dialog.refresh_from_documents([&workspace.documents()[1]]);
                        }
                    }
                    render(
                        &mut renderer,
                        advanced_search_panel::view(&dialog),
                        &theme,
                        Size::new(width, height),
                        &format!("search-{tab_name}-{name}-{size_name}"),
                    );
                    let mut options_dialog = dialog.clone();
                    options_dialog.result_options_visible = true;
                    options_dialog.set_query("release");
                    options_dialog.set_replacement("launch");
                    options_dialog.set_result_settings(SearchResultSettings {
                        result_limit: 2,
                        ..SearchResultSettings::default()
                    });
                    let mut options_workspace = Workspace::new();
                    options_workspace.push_document(Document::from_path(
                    DocumentId::new(30),
                    "release-notes.md",
                    &format!(
                        "{}release checklist.\nPrepare the release notes.\nReview the release checklist.\nPublish the release after review.\n",
                        "A long introductory note before the match. ".repeat(12),
                    ),
                ));
                    options_workspace.push_document(Document::from_path(
                        DocumentId::new(31),
                        "src/main.rs",
                        "fn prepare_release() {}\n// Publish the release after review.",
                    ));
                    if matches!(
                        tab,
                        AdvancedSearchTab::FindInFiles | AdvancedSearchTab::ReplaceInFiles
                    ) {
                        options_dialog.refresh_from_workspace(&options_workspace);
                    } else {
                        options_dialog.refresh_from_documents([&options_workspace.documents()[1]]);
                    }
                    render(
                        &mut renderer,
                        advanced_search_panel::view(&options_dialog),
                        &theme,
                        Size::new(width, height),
                        &format!("search-{tab_name}-options-{name}-{size_name}"),
                    );
                    for (suffix, limit, preview, context) in [
                        ("limit", "0", "160", "40"),
                        ("preview", "2", "2001", "40"),
                        ("context", "2", "160", "160"),
                    ] {
                        let mut invalid_dialog = options_dialog.clone();
                        invalid_dialog.result_limit_input = limit.into();
                        invalid_dialog.preview_chars_input = preview.into();
                        invalid_dialog.context_before_input = context.into();
                        render(
                            &mut renderer,
                            advanced_search_panel::view(&invalid_dialog),
                            &theme,
                            Size::new(width, height),
                            &format!(
                                "search-{tab_name}-options-invalid-{suffix}-{name}-{size_name}"
                            ),
                        );
                    }
                    if matches!(
                        tab,
                        AdvancedSearchTab::Replace | AdvancedSearchTab::ReplaceInFiles
                    ) {
                        for (mode, mode_name) in [
                            (SearchMode::Regex, "regex"),
                            (SearchMode::Extended, "escapes"),
                        ] {
                            dialog.mode = mode;
                            render(
                                &mut renderer,
                                advanced_search_panel::view(&dialog),
                                &theme,
                                Size::new(width, height),
                                &format!("search-{tab_name}-{mode_name}-{name}-{size_name}"),
                            );
                        }
                    }
                }
            }
        }
        if selected.includes(PreviewPanel::Settings) {
            for (width, height, size_name) in [(820, 524, "normal"), (720, 424, "minimum")] {
                for &category in SettingsCategory::ALL {
                    let category_name =
                        settings_panel::category_label(category).to_ascii_lowercase();
                    let mut dialog = SettingsDialogState::new(&EditorSettings {
                        appearance,
                        ..EditorSettings::default()
                    });
                    dialog.system_dark = appearance == AppearanceMode::Dark;
                    dialog.category = category;
                    dialog.shortcut_group = ShortcutGroup::Edit;
                    render(
                        &mut renderer,
                        settings_panel::view(&dialog),
                        &theme,
                        Size::new(width, height),
                        &format!("preferences-{category_name}-{name}-{size_name}"),
                    );
                    if category == SettingsCategory::Search {
                        for (suffix, preview, context) in [
                            ("invalid-preview", "2001", "40"),
                            ("invalid-context", "160", "160"),
                        ] {
                            let mut invalid_dialog = dialog.clone();
                            invalid_dialog.preview_chars_input = preview.into();
                            invalid_dialog.context_before_input = context.into();
                            render(
                                &mut renderer,
                                settings_panel::view(&invalid_dialog),
                                &theme,
                                Size::new(width, height),
                                &format!("preferences-search-{suffix}-{name}-{size_name}"),
                            );
                        }
                    }
                    if category == SettingsCategory::Shortcuts {
                        dialog.capturing_shortcut = Some(ShortcutCommand::Copy);
                        render(
                            &mut renderer,
                            settings_panel::view(&dialog),
                            &theme,
                            Size::new(width, height),
                            &format!("preferences-recording-{name}-{size_name}"),
                        );
                        dialog.capturing_shortcut = None;
                        dialog.shortcut_conflict = Some(ShortcutConflict {
                            binding: dialog
                                .draft
                                .shortcuts
                                .binding(ShortcutCommand::Copy)
                                .unwrap(),
                            command: ShortcutCommand::Copy,
                        });
                        render(
                            &mut renderer,
                            settings_panel::view(&dialog),
                            &theme,
                            Size::new(width, height),
                            &format!("preferences-conflict-{name}-{size_name}"),
                        );
                    }
                    if category == SettingsCategory::Editor {
                        dialog.draft.wrap_column_limit = Some(EditorSettings::DEFAULT_WRAP_COLUMN);
                        render(
                            &mut renderer,
                            settings_panel::view(&dialog),
                            &theme,
                            Size::new(width, height),
                            &format!("preferences-editor-fixed-wrap-{name}-{size_name}"),
                        );
                    }
                    if category == SettingsCategory::Appearance {
                        for (index, &preset) in iced::highlighter::Theme::ALL.iter().enumerate() {
                            dialog.draft.set_syntax_theme(preset);
                            render(
                                &mut renderer,
                                settings_panel::view(&dialog),
                                &theme,
                                Size::new(width, height),
                                &format!("preferences-syntax-{index}-{name}-{size_name}"),
                            );
                        }
                    }
                }
            }
        }
        if selected.includes(PreviewPanel::Editor) {
            let mut settings = EditorSettings {
                appearance,
                ..EditorSettings::default()
            };
            for (index, &preset) in iced::highlighter::Theme::ALL.iter().enumerate() {
                settings.set_syntax_theme(preset);
                let mut document = Document::from_path(
                    DocumentId::new(900),
                    "about_dialog.rs",
                    include_str!("../src/ui/about_dialog.rs"),
                );
                document.scroll.first_visible_row = 150;
                document.ensure_syntax_cache(
                    settings.resolved_syntax_theme(appearance == AppearanceMode::Dark),
                );
                render(
                    &mut renderer,
                    fragile_notepad::ui::editor::view(&document, &settings),
                    &theme,
                    Size::new(1200, 800),
                    &format!("editor-syntax-{index}-{name}"),
                );
            }
        }
    }
    println!("Screenshots: target/dialog-review/");
    ExitCode::SUCCESS
}

fn render(
    renderer: &mut Renderer,
    mut content: Element<'_, Message>,
    theme: &Theme,
    pixels: Size<u32>,
    name: &str,
) {
    let size = Size::new(pixels.width as f32, pixels.height as f32);
    let viewport = Rectangle::with_size(size);
    let mut tree = Tree::empty();
    tree.diff(content.as_widget_mut());
    let node =
        content
            .as_widget_mut()
            .layout(&mut tree, renderer, &layout::Limits::new(size, size));
    let start = Instant::now();
    for elapsed in [Duration::ZERO, Duration::from_millis(200)] {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
        content.as_widget_mut().update(
            &mut tree,
            &Event::Window(window::Event::RedrawRequested(start + elapsed)),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            renderer,
            &mut shell,
            &viewport,
        );
    }
    renderer.reset(viewport);
    content.as_widget().draw(
        &tree,
        renderer,
        theme,
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &viewport,
    );
    let bytes = renderer.screenshot(pixels, 1.0, iced::Color::from_rgb8(110, 116, 126));
    tiny_skia::Pixmap::from_vec(
        bytes,
        tiny_skia::IntSize::from_wh(pixels.width, pixels.height).unwrap(),
    )
    .unwrap()
    .save_png(format!("target/dialog-review/{name}.png"))
    .unwrap();
}
