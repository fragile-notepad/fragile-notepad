use iced::highlighter;
use iced::widget::{container, text};
use iced::{Element, Fill};

use crate::core::{Document, EditorSettings};
use crate::editor::{AdvancedEditor, EditorMetrics};
use crate::message::Message;

pub const EDITOR_ID: &str = "fragile-notepad-editor";
const BASE_TEXT_SIZE: f32 = 16.0;

pub(crate) fn metrics(zoom: f32) -> EditorMetrics {
    let size = BASE_TEXT_SIZE * zoom;
    EditorMetrics {
        line_height: size * 1.25,
        character_width: crate::editor::widget::editor_character_width(size),
        ..EditorMetrics::default()
    }
}

pub fn view<'a>(document: &'a Document, settings: &'a EditorSettings) -> Element<'a, Message> {
    let document_id = document.id;
    let metrics = metrics(settings.zoom).with_line_count(document.buffer.line_count());
    let editor = AdvancedEditor::new(
        &document.buffer,
        &document.viewport,
        &document.decorations,
        &document.syntax_cache,
        highlighter::Settings {
            token: document.render_syntax_token().to_owned(),
            theme: settings.syntax_theme,
        },
        document.selection_set().clone(),
        move |action| Message::EditorAction(document_id, action),
    )
    .id(EDITOR_ID)
    .viewport_key(document_id.get())
    .cjk_context(document.cjk_context())
    .height(Fill)
    .metrics(metrics)
    .scroll(document.scroll)
    .caret_row(document.caret_visible_row())
    .caret_rows(document.caret_row_affinities())
    .scroll_speed(settings.scroll_speed)
    .wrap_guide_column(
        settings
            .wrap_column_limit
            .unwrap_or(EditorSettings::DEFAULT_WRAP_COLUMN),
    )
    .shortcuts(&settings.shortcuts)
    .into();

    super::editor_context_menu::wrap(editor, document, settings, metrics)
}

pub fn empty<'a>() -> Element<'a, Message> {
    container(text("No document open").size(18))
        .center_x(Fill)
        .center_y(Fill)
        .height(Fill)
        .into()
}
