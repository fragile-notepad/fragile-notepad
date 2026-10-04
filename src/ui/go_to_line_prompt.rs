use iced::widget::{
    button, column, container, mouse_area, opaque, row, space, stack, text, text_input,
};
use iced::{Element, Fill};

use crate::message::Message;
use crate::ui::{motion, styles, utility};

pub const INPUT_ID: &str = "go-to-line-input";

pub fn view<'a>(
    input: &'a str,
    error: Option<&'a str>,
    progress: f32,
    interactive: bool,
) -> Element<'a, Message> {
    let progress = progress.clamp(0.0, 1.0);
    let mut content = column![
        text("Go to line").size(18).font(utility::semibold()),
        text_input("Line number", input)
            .id(INPUT_ID)
            .on_input(Message::GoToLineChanged)
            .on_submit(Message::GoToLineSubmitted)
            .padding([8, 10])
            .size(14)
            .style(move |theme, status| {
                let mut style = styles::input(theme, status);
                style.background = style.background.scale_alpha(progress);
                style.border.color = style.border.color.scale_alpha(progress);
                style.icon = style.icon.scale_alpha(progress);
                style.placeholder = style.placeholder.scale_alpha(progress);
                style.value = style.value.scale_alpha(progress);
                style.selection = style.selection.scale_alpha(progress);
                style
            }),
    ]
    .spacing(12);
    if let Some(error) = error {
        content = content.push(text(error).size(12));
    }
    content = content.push(
        row![
            space::horizontal(),
            button(text("Cancel").size(13))
                .padding([8, 14])
                .style(move |theme, status| motion::fade_button(
                    styles::command_button(theme, status),
                    progress
                ))
                .on_press(Message::GoToLineClosed),
            button(text("Go").size(13))
                .padding([8, 20])
                .style(move |theme, status| motion::fade_button(
                    styles::primary_command_button(theme, status),
                    progress
                ))
                .on_press_maybe((!input.trim().is_empty()).then_some(Message::GoToLineSubmitted)),
        ]
        .spacing(8),
    );
    let surface =
        stack![
            opaque(
                mouse_area(container(space::vertical()).width(Fill).height(Fill).style(
                    move |theme| motion::fade_container(styles::modal_scrim(theme), progress)
                ))
                .on_press(Message::GoToLineClosed)
            ),
            container(motion::popup_with_progress(
                opaque(
                    container(content)
                        .padding(20)
                        .width(360)
                        .style(move |theme| motion::fade_container(
                            styles::utility_dialog(theme),
                            progress
                        ))
                ),
                progress
            ))
            .padding(24)
            .center(Fill),
        ];
    // Keep blocking the editor while the prompt fades out, with its controls disabled.
    opaque(motion::fade(
        surface,
        1.0,
        styles::editor_background,
        interactive,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::{self, Headless};
    use iced::advanced::widget::{Tree, operation};
    use iced::advanced::{Layout, Shell, layout, mouse};
    use iced::{Event, Point, Rectangle, Renderer, Size, keyboard, window};

    #[test]
    fn go_to_line_input_replaces_selection_submits_and_backdrop_dismisses() {
        let renderer = futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut content: Element<'_, Message> = stack![
            button(space::vertical().width(Fill).height(Fill))
                .width(Fill)
                .height(Fill)
                .on_press(Message::NewFile),
            view("120", None, 1.0, true),
        ]
        .into();
        let viewport = Rectangle::with_size(Size::new(640.0, 364.0));
        let mut tree = Tree::empty();
        tree.diff(content.as_widget_mut());
        let node = content.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(viewport.size(), viewport.size()),
        );
        content.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut operation::focusable::focus::<()>(INPUT_ID.into()),
        );
        content.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut operation::text_input::select_all::<()>(INPUT_ID.into()),
        );
        let mut send = |event, cursor| {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            content.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &renderer,
                &mut shell,
                &viewport,
            );
            messages
        };
        let key_event = |key: keyboard::Key, text| {
            Event::Keyboard(keyboard::Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text,
                repeat: false,
            })
        };
        let typed = send(
            key_event(keyboard::Key::Character("8".into()), Some("8".into())),
            mouse::Cursor::Unavailable,
        );
        assert!(matches!(typed.as_slice(), [Message::GoToLineChanged(value)] if value == "8"));
        let enter = send(
            key_event(keyboard::Key::Named(keyboard::key::Named::Enter), None),
            mouse::Cursor::Unavailable,
        );
        assert!(matches!(enter.as_slice(), [Message::GoToLineSubmitted]));
        let click = send(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            mouse::Cursor::Available(Point::new(10.0, 10.0)),
        );
        assert!(matches!(click.as_slice(), [Message::GoToLineClosed]));
        let release = send(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Available(Point::new(10.0, 10.0)),
        );
        assert!(
            release.is_empty(),
            "backdrop click must not activate editor controls"
        );
    }
    #[test]
    fn go_to_line_fades_every_surface_and_blocks_clicks_while_closing() {
        use iced::advanced::Renderer as _;
        let mut renderer = futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .unwrap();
        let pixels = Size::new(640, 364);
        let viewport = Rectangle::with_size(Size::new(640.0, 364.0));
        let backdrop = iced::Color::from_rgb8(23, 61, 97);
        renderer.reset(viewport);
        let background = renderer.screenshot(pixels, 1.0, backdrop);
        for theme in [iced::Theme::Light, iced::Theme::Dark] {
            let mut snapshots = Vec::new();
            for progress in [0.0, 0.5, 1.0] {
                let mut content = view("120", Some("Enter a valid line number."), progress, false);
                let mut tree = Tree::empty();
                tree.diff(content.as_widget_mut());
                let node = content.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(viewport.size(), viewport.size()),
                );
                content.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut operation::focusable::focus::<()>(INPUT_ID.into()),
                );
                content.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut operation::text_input::select_all::<()>(INPUT_ID.into()),
                );
                renderer.reset(viewport);
                content.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &theme,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &viewport,
                );
                snapshots.push(renderer.screenshot(pixels, 1.0, backdrop));
                let mut messages = Vec::new();
                for point in [Point::new(10.0, 10.0), Point::new(452.0, 238.0)] {
                    for event in [
                        mouse::Event::ButtonPressed(mouse::Button::Left),
                        mouse::Event::ButtonReleased(mouse::Button::Left),
                    ] {
                        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
                        content.as_widget_mut().update(
                            &mut tree,
                            &Event::Mouse(event),
                            Layout::new(&node),
                            mouse::Cursor::Available(point),
                            &renderer,
                            &mut shell,
                            &viewport,
                        );
                        if matches!(event, mouse::Event::ButtonPressed(_)) {
                            assert!(
                                shell.is_event_captured(),
                                "closing must keep the input barrier"
                            );
                        }
                    }
                }
                assert!(
                    messages.is_empty(),
                    "closing controls must not submit or cancel again"
                );
            }
            assert!(
                snapshots[0] == background,
                "zero opacity must fade text, selection, input, buttons, dialog and scrim"
            );
            assert!(snapshots[1] != background);
            assert!(snapshots[1] != snapshots[2]);
        }
    }
}
