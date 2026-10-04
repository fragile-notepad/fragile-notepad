use iced::widget::{container, responsive, row, space, text};
use iced::{Center, Element, Fill, Length};

use crate::core::{Document, EditorSettings};
use crate::editor::layout::visual_column_for;
use crate::message::Message;
use crate::ui::styles;

const STATUS_TEXT_SIZE: u32 = 13;
const STATUS_BAR_HEIGHT: f32 = 26.0;
const STATUS_BAR_HORIZONTAL_PADDING: f32 = 12.0;
const STATUS_SEGMENT_SPACING: f32 = 3.0;
const MIN_PATH_WIDTH: f32 = 120.0;

pub fn view<'a>(
    document: Option<&'a Document>,
    settings: &'a EditorSettings,
    file_status: Option<&'a str>,
) -> Element<'a, Message> {
    let Some(document) = document else {
        return container(row![segment("No document", 112.0), space::horizontal()])
            .padding([2, 6])
            .height(Length::Fixed(STATUS_BAR_HEIGHT))
            .width(Fill)
            .style(styles::status_bar)
            .into();
    };

    let cursor = document.main_selection().cursor;
    let cursor_column = cursor_display_column(document, settings.decorations.indent_width);
    let selection_count = document.selection_set().len();
    let line_count = document.buffer.line_count();
    let path_or_title = document
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| document.title());
    let document_status = document_status_label(document, file_status);
    let line_ending = document
        .line_ending
        .map(|ending| ending.as_str().replace('\r', "CR").replace('\n', "LF"))
        .unwrap_or_else(|| String::from("Unknown"));

    let data = StatusBarData {
        path_or_title,
        document_status: document_status.to_owned(),
        cursor: format!("Ln {}, Col {}", cursor.line + 1, cursor_column),
        selection: if selection_count > 1 {
            format!("{} selections", selection_count)
        } else {
            String::from("1 selection")
        },
        line_count: format!("{} lines", line_count),
        syntax: document.syntax_token.to_uppercase(),
        line_ending,
        wrap: if settings.word_wrap {
            String::from("Wrap")
        } else {
            String::from("No wrap")
        },
        zoom: format!("{:.0}%", settings.zoom * 100.0),
    };

    container(responsive(move |size| {
        status_bar_for_width(&data, size.width)
    }))
    .height(Length::Fixed(STATUS_BAR_HEIGHT))
    .width(Fill)
    .into()
}

pub fn document_status_label<'a>(document: &Document, file_status: Option<&'a str>) -> &'a str {
    if document.is_loading_or_indexing() {
        "indexing"
    } else if let Some(file_status) = file_status {
        file_status
    } else if document.is_dirty {
        "Modified"
    } else {
        "Saved"
    }
}

fn segment<'a>(label: impl Into<String>, width: f32) -> Element<'a, Message> {
    container(text(label.into()).size(STATUS_TEXT_SIZE))
        .padding([3, 6])
        .height(22)
        .width(Length::Fixed(width))
        .center_y(22)
        .style(styles::status_segment)
        .into()
}

#[derive(Debug)]
struct StatusBarData {
    path_or_title: String,
    document_status: String,
    cursor: String,
    selection: String,
    line_count: String,
    syntax: String,
    line_ending: String,
    wrap: String,
    zoom: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusSegment {
    DocumentStatus,
    Cursor,
    Selection,
    LineCount,
    Syntax,
    LineEnding,
    Wrap,
    Zoom,
}

impl StatusSegment {
    const fn width(self) -> f32 {
        match self {
            Self::DocumentStatus => 148.0,
            Self::Cursor => 118.0,
            Self::Selection => 104.0,
            Self::LineCount => 86.0,
            Self::Syntax => 68.0,
            Self::LineEnding => 58.0,
            Self::Wrap => 72.0,
            Self::Zoom => 54.0,
        }
    }
}

fn status_bar_for_width<'a>(data: &StatusBarData, available_width: f32) -> Element<'a, Message> {
    let visible = visible_status_segments(available_width);
    let mut row = row![
        container(text(data.path_or_title.clone()).size(STATUS_TEXT_SIZE))
            .padding([3, 2])
            .width(Fill)
            .style(styles::status_path)
    ]
    .spacing(STATUS_SEGMENT_SPACING)
    .padding([2, 6])
    .align_y(Center);

    for segment_kind in visible {
        row = row.push(match segment_kind {
            StatusSegment::DocumentStatus => {
                segment(data.document_status.clone(), segment_kind.width())
            }
            StatusSegment::Cursor => segment(data.cursor.clone(), segment_kind.width()),
            StatusSegment::Selection => segment(data.selection.clone(), segment_kind.width()),
            StatusSegment::LineCount => segment(data.line_count.clone(), segment_kind.width()),
            StatusSegment::Syntax => segment(data.syntax.clone(), segment_kind.width()),
            StatusSegment::LineEnding => segment(data.line_ending.clone(), segment_kind.width()),
            StatusSegment::Wrap => segment(data.wrap.clone(), segment_kind.width()),
            StatusSegment::Zoom => segment(data.zoom.clone(), segment_kind.width()),
        });
    }

    container(row)
        .height(Length::Fixed(STATUS_BAR_HEIGHT))
        .width(Fill)
        .style(styles::status_bar)
        .into()
}

fn visible_status_segments(available_width: f32) -> Vec<StatusSegment> {
    let mut visible = vec![StatusSegment::Cursor];
    let mut used_width = MIN_PATH_WIDTH
        + STATUS_BAR_HORIZONTAL_PADDING
        + StatusSegment::Cursor.width()
        + STATUS_SEGMENT_SPACING;

    for segment in [
        StatusSegment::DocumentStatus,
        StatusSegment::Selection,
        StatusSegment::LineCount,
        StatusSegment::Syntax,
        StatusSegment::LineEnding,
        StatusSegment::Wrap,
        StatusSegment::Zoom,
    ] {
        let next_width = used_width + STATUS_SEGMENT_SPACING + segment.width();
        if next_width <= available_width {
            visible.push(segment);
            used_width = next_width;
        }
    }

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

#[cfg(test)]
mod tests {
    use super::{StatusSegment, cursor_display_column, visible_status_segments};
    use crate::core::{Document, DocumentId};
    use crate::editor::{EditorPosition, EditorSelection};

    #[test]
    fn cursor_display_column_uses_unicode_and_configured_tab_stops() {
        let mut document = Document::from_path(DocumentId::new(1), "unicode.txt", "é\t好");
        let caret = EditorPosition::new(0, "é\t".len());
        document.set_main_selection(EditorSelection::new(caret, caret));
        for (tab_width, column) in [(2, 3), (4, 5), (8, 9)] {
            assert_eq!(cursor_display_column(&document, tab_width), column);
        }
    }

    #[test]
    fn status_bar_keeps_cursor_segment_on_narrow_widths() {
        assert_eq!(visible_status_segments(260.0), vec![StatusSegment::Cursor]);
    }

    #[test]
    fn status_bar_adds_segments_as_width_allows() {
        assert_eq!(
            visible_status_segments(520.0),
            vec![
                StatusSegment::Cursor,
                StatusSegment::DocumentStatus,
                StatusSegment::Selection,
            ]
        );
        assert_eq!(
            visible_status_segments(900.0),
            vec![
                StatusSegment::Cursor,
                StatusSegment::DocumentStatus,
                StatusSegment::Selection,
                StatusSegment::LineCount,
                StatusSegment::Syntax,
                StatusSegment::LineEnding,
                StatusSegment::Wrap,
                StatusSegment::Zoom,
            ]
        );
    }
}
