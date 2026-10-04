use iced::widget::{button, column, container, opaque, row, space, stack, text};
use iced::{Alignment, Center, Element, Fill, Length};

use crate::core::DirtyCloseDecision;
use crate::core::Document;
use crate::message::Message;
use crate::ui::controls::centered_button_label;
use crate::ui::{motion, styles};

#[cfg(test)]
mod tests;

pub fn view(document: &Document, progress: f32, interactive: bool) -> Element<'_, Message> {
    let progress = progress.clamp(0.0, 1.0);
    let content = stack![
        opaque(
            container(space::vertical())
                .width(Fill)
                .height(Fill)
                .style(move |theme| motion::fade_container(styles::modal_scrim(theme), progress))
        ),
        container(motion::popup_with_progress(
            dialog(document, progress),
            progress
        ))
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill),
    ];
    // Retain the input barrier while the closing controls are disabled.
    opaque(motion::fade(
        content,
        1.0,
        styles::editor_background,
        interactive,
    ))
}

fn dialog(document: &Document, progress: f32) -> Element<'_, Message> {
    let document_id = document.id;

    container(
        column![
            column![
                text("Save changes?").size(18),
                text(format!(
                    "Do you want to save changes to \"{}\" before closing?",
                    document.title()
                ))
                .size(13)
                .width(Fill),
            ]
            .spacing(8),
            row![
                space::horizontal(),
                button(centered_button_label("Discard", 13))
                    .padding([7, 14])
                    .style(move |theme, status| motion::fade_button(
                        styles::danger_command_button(theme, status),
                        progress
                    ))
                    .on_press(Message::DirtyCloseResolved(
                        document_id,
                        DirtyCloseDecision::Discard
                    )),
                button(centered_button_label("Cancel", 13))
                    .padding([7, 14])
                    .style(move |theme, status| motion::fade_button(
                        styles::command_button(theme, status),
                        progress
                    ))
                    .on_press(Message::DirtyCloseResolved(
                        document_id,
                        DirtyCloseDecision::Cancel
                    )),
                button(centered_button_label("Save", 13))
                    .padding([7, 18])
                    .style(move |theme, status| motion::fade_button(
                        styles::primary_command_button(theme, status),
                        progress
                    ))
                    .on_press(Message::DirtyCloseResolved(
                        document_id,
                        DirtyCloseDecision::Save
                    )),
            ]
            .spacing(8)
            .align_y(Center)
            .width(Fill),
        ]
        .spacing(18)
        .align_x(Alignment::Start),
    )
    .width(Length::Fixed(420.0))
    .padding(20)
    .style(move |theme| motion::fade_container(styles::modal_dialog(theme), progress))
    .into()
}
