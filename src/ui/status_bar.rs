use iced::widget::{Row, button, column, container, responsive, row, space, text, tooltip};
use iced::{Center, Element, Fill};
use unicode_width::UnicodeWidthStr;

use crate::core::{DiskState, Document, DocumentLoadState, EditorSettings};
use crate::editor::layout::visual_column_for;
use crate::message::Message;
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::styles::{self, StatusTone};

#[cfg(test)]
mod tests;

const STATUS_TEXT_SIZE: u32 = 12;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const STATUS_ITEM_HEIGHT: f32 = 22.0;
const ITEM_PADDING: f32 = 8.0;
const HORIZONTAL_PADDING: f32 = 10.0;
const MIN_PATH_WIDTH: f32 = 96.0;
const FILE_SPACING: f32 = 7.0;
const INDICATOR_WIDTH: f32 = 6.0;
const DIVIDER_WIDTH: f32 = 13.0;

pub fn view<'a>(
    document: Option<&'a Document>,
    settings: &'a EditorSettings,
    file_status: Option<&'a str>,
) -> Element<'a, Message> {
    let Some(document) = document else {
        return bar(container(status_text("No document"))
            .padding([0.0, HORIZONTAL_PADDING])
            .center_y(STATUS_BAR_HEIGHT - 1.0)
            .width(Fill));
    };

    let data = StatusBarData::new(document, settings, file_status);
    bar(responsive(move |size| {
        status_bar_for_width(&data, size.width)
    }))
}

/// Reports the document's state, giving disk and loading problems priority over file messages.
pub fn document_status_label<'a>(document: &Document, file_status: Option<&'a str>) -> &'a str {
    document_status(document, file_status).0
}

fn document_status<'a>(document: &Document, file_status: Option<&'a str>) -> (&'a str, StatusTone) {
    use StatusTone::*;

    if document.is_loading_or_indexing() {
        ("indexing", Accent)
    } else if document.disk_state == DiskState::Missing {
        ("Missing on disk", Danger)
    } else if matches!(document.disk_state, DiskState::Unreadable(_)) {
        ("File unavailable", Danger)
    } else if matches!(document.load_state, DocumentLoadState::Failed { .. }) {
        ("Load failed", Danger)
    } else if document.disk_state == DiskState::Changed {
        ("Changed on disk", Warning)
    } else if let Some(file_status) = file_status {
        (file_status, Muted)
    } else if document.had_decoding_errors() {
        ("Decoding errors", Warning)
    } else if document.path.is_none() {
        ("Not saved", Warning)
    } else if document.is_dirty {
        ("Modified", Warning)
    } else {
        ("Saved", Success)
    }
}

fn bar<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(column![
        container(space::horizontal().height(1)).style(styles::separator),
        content.into(),
    ])
    .height(STATUS_BAR_HEIGHT)
    .width(Fill)
    .clip(true)
    .style(styles::status_bar)
    .into()
}

fn status_text(label: impl Into<String>) -> iced::widget::Text<'static> {
    text(label.into())
        .size(STATUS_TEXT_SIZE)
        .wrapping(text::Wrapping::None)
        .ellipsis(text::Ellipsis::End)
}

fn with_tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl Into<String>,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(label.into()).size(STATUS_TEXT_SIZE))
            .padding([6, 9])
            .max_width(600)
            .style(styles::tooltip),
        tooltip::Position::Top,
    )
    .gap(5)
    .delay(std::time::Duration::from_millis(450))
    .animation(std::time::Duration::from_millis(120))
    .into()
}

#[derive(Debug)]
struct StatusBarData {
    path_or_title: String,
    document_status: String,
    file_tooltip: String,
    tone: StatusTone,
    cursor: String,
    selection: Option<String>,
    line_count: String,
    syntax: String,
    encoding: String,
    line_ending: String,
    word_wrap: bool,
    zoom: String,
}

impl StatusBarData {
    fn new(document: &Document, settings: &EditorSettings, file_status: Option<&str>) -> Self {
        let cursor = document.main_selection().cursor;
        let cursor_column = cursor_display_column(document, settings.decorations.indent_width);
        let selections = document.selection_set();
        let line_count = document.buffer.line_count();
        let path_or_title = document
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| format!("Untitled {}", document.id));
        let (label, tone) = document_status(document, file_status);
        let document_status = if document.is_loading_or_indexing() {
            "Indexing"
        } else {
            label
        };
        let mut file_tooltip = format!("{path_or_title}\n{document_status}");
        if let Some(message) = file_status
            && message != document_status
        {
            file_tooltip.push('\n');
            file_tooltip.push_str(message);
        }

        Self {
            path_or_title,
            document_status: document_status.to_owned(),
            file_tooltip,
            tone,
            cursor: format!("Ln {}, Col {}", cursor.line + 1, cursor_column),
            selection: if selections.is_single_caret() {
                None
            } else if selections.is_single() {
                Some(String::from("1 selection"))
            } else if selections.ranges().iter().all(|range| range.is_caret()) {
                Some(format!("{} cursors", selections.len()))
            } else {
                Some(format!("{} selections", selections.len()))
            },
            line_count: format!(
                "{line_count} {}",
                if line_count == 1 { "line" } else { "lines" }
            ),
            syntax: document.syntax_token.to_uppercase(),
            encoding: document.encoding.label().to_owned(),
            line_ending: document
                .line_ending
                .map(|ending| ending.as_str().replace('\r', "CR").replace('\n', "LF"))
                .unwrap_or_else(|| String::from("Unknown")),
            word_wrap: settings.word_wrap,
            zoom: format!("{:.0}%", settings.zoom * 100.0),
        }
    }

    fn status_width(&self) -> f32 {
        label_width(&self.document_status).min(200.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StatusItem {
    Cursor,
    Selection,
    LineCount,
    Syntax,
    Encoding,
    LineEnding,
    Wrap,
    Zoom,
}

impl StatusItem {
    fn label(self, data: &StatusBarData) -> &str {
        match self {
            Self::Cursor => &data.cursor,
            Self::Selection => data.selection.as_deref().unwrap_or_default(),
            Self::LineCount => &data.line_count,
            Self::Syntax => &data.syntax,
            Self::Encoding => &data.encoding,
            Self::LineEnding => &data.line_ending,
            Self::Wrap if data.word_wrap => "Wrap",
            Self::Wrap => "No wrap",
            Self::Zoom => &data.zoom,
        }
    }

    fn width(self, data: &StatusBarData) -> f32 {
        label_width(self.label(data)) + if self == Self::Wrap { 20.0 } else { 0.0 }
    }

    const fn group(self) -> u8 {
        match self {
            Self::Cursor | Self::Selection => 0,
            Self::LineCount | Self::Syntax | Self::Encoding | Self::LineEnding => 1,
            Self::Wrap | Self::Zoom => 2,
        }
    }

    fn view(self, data: &StatusBarData) -> Element<'static, Message> {
        let label = self.label(data);
        let content: Element<'static, Message> = if self == Self::Wrap {
            row![
                hero::icon(
                    HeroIcon::ArrowTurnDownLeft,
                    14,
                    if data.word_wrap {
                        IconTone::Text
                    } else {
                        IconTone::Muted
                    },
                ),
                status_text(label),
            ]
            .spacing(6)
            .align_y(Center)
            .into()
        } else {
            status_text(label).into()
        };
        let action = match self {
            Self::Cursor => Some((Message::GoToLineOpened, "Go to line")),
            Self::Wrap => Some((Message::ToggleWordWrap, "Toggle word wrap")),
            Self::Zoom => Some((Message::ZoomReset, "Reset zoom to 100%")),
            _ => None,
        };

        if let Some((message, help)) = action {
            with_tooltip(
                button(container(content).center_y(Fill))
                    .padding([0.0, ITEM_PADDING])
                    .width(self.width(data))
                    .height(STATUS_ITEM_HEIGHT)
                    .style(styles::status_action(self == Self::Wrap && data.word_wrap))
                    .on_press(message),
                help,
            )
        } else {
            with_tooltip(
                container(content)
                    .padding([0.0, ITEM_PADDING])
                    .width(self.width(data))
                    .center_y(STATUS_ITEM_HEIGHT)
                    .style(if self == Self::Selection {
                        styles::status_selection
                    } else {
                        styles::status_segment
                    }),
                match self {
                    Self::Syntax => format!("Language: {label}"),
                    Self::Encoding => format!("Text encoding: {label}"),
                    Self::LineEnding => format!("Line endings: {label}"),
                    _ => label.to_owned(),
                },
            )
        }
    }
}

// Leave room around each label, including on platforms with wider UI fonts.
fn label_width(label: &str) -> f32 {
    label.width() as f32 * 7.5 + ITEM_PADDING * 2.0
}

fn status_bar_for_width<'a>(data: &StatusBarData, available_width: f32) -> Element<'a, Message> {
    let visible = visible_status_items(data, available_width);
    let mut details = Row::new().align_y(Center);
    let mut previous_group = 0;
    let mut details_width = 0.0;

    for item in visible {
        if item.group() != previous_group {
            details = details.push(
                container(
                    container(space::horizontal().width(1).height(12)).style(styles::separator),
                )
                .center_x(DIVIDER_WIDTH),
            );
            details_width += DIVIDER_WIDTH;
            previous_group = item.group();
        }
        details = details.push(item.view(data));
        details_width += item.width(data);
    }

    let file_width = (available_width - HORIZONTAL_PADDING * 2.0 - details_width).max(0.0);

    row![file_summary(data, file_width), details]
        .padding([0.0, HORIZONTAL_PADDING])
        .align_y(Center)
        .height(STATUS_BAR_HEIGHT - 1.0)
        .width(Fill)
        .into()
}

fn file_summary<'a>(data: &StatusBarData, file_width: f32) -> Element<'a, Message> {
    let status_width = (file_width - MIN_PATH_WIDTH - INDICATOR_WIDTH - FILE_SPACING * 2.0)
        .clamp(0.0, data.status_width());
    let show_status = status_width >= 48.0;
    let path_width = (file_width
        - INDICATOR_WIDTH
        - FILE_SPACING
        - if show_status {
            status_width + FILE_SPACING
        } else {
            0.0
        })
    .max(0.0);
    let tone = data.tone;
    let mut file = row![
        container(
            space::horizontal()
                .width(INDICATOR_WIDTH)
                .height(INDICATOR_WIDTH)
        )
        .style(move |theme| container::Style {
            background: Some(styles::status_color(theme, tone).into()),
            border: iced::Border {
                radius: INDICATOR_WIDTH.into(),
                ..Default::default()
            },
            ..Default::default()
        }),
        container(status_text(data.path_or_title.clone()).ellipsis(text::Ellipsis::Middle))
            .max_width(path_width)
            .clip(true)
            .style(styles::status_path),
    ]
    .spacing(FILE_SPACING)
    .align_y(Center)
    .width(Fill);

    if show_status {
        file = file.push(
            container(status_text(data.document_status.clone()))
                .width(status_width)
                .style(move |theme| container::Style {
                    text_color: Some(styles::status_color(theme, tone)),
                    ..Default::default()
                }),
        );
    }

    with_tooltip(file, data.file_tooltip.clone())
}

fn visible_status_items(data: &StatusBarData, available_width: f32) -> Vec<StatusItem> {
    use StatusItem::*;

    let mut visible = vec![Cursor];
    let mut used_width = HORIZONTAL_PADDING * 2.0
        + MIN_PATH_WIDTH
        + INDICATOR_WIDTH
        + FILE_SPACING * 2.0
        + data.status_width()
        + Cursor.width(data)
        + DIVIDER_WIDTH * 2.0;

    // Position and active selections take priority over document metadata.
    for item in [
        Selection, Syntax, LineEnding, Zoom, Encoding, Wrap, LineCount,
    ] {
        if item == Selection && data.selection.is_none() {
            continue;
        }
        let next_width = used_width + item.width(data);
        if next_width > available_width {
            break;
        }
        visible.push(item);
        used_width = next_width;
    }

    visible.sort_unstable();
    visible
}

fn cursor_display_column(document: &Document, tab_width: usize) -> usize {
    document
        .buffer
        .line(document.main_selection().cursor.line)
        .map(|line| {
            visual_column_for(&line, document.main_selection().cursor.column, tab_width) + 1
        })
        .unwrap_or(1)
}
