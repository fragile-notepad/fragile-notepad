use crate::message::FileMessage;
use crate::message::GoToLineMessage;
use crate::message::SearchMessage;
use crate::message::SettingsMessage;
use iced::event::{Event, Status};
use iced::{Task, keyboard, mouse, window};

use crate::core::{KeyBinding, ShortcutCommand};
use crate::editor::EditorAction;
use crate::message::Message;

use super::App;
use super::windowing::managed::ManagedWindow;

impl App {
    pub(super) fn update_runtime_event(
        &mut self,
        event: Event,
        status: Status,
        window_id: window::Id,
    ) -> Task<Message> {
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) && self.dragged_tab.is_some()
        {
            self.dragged_tab = None;
            self.hovered_drop_tab = None;
            return Task::none();
        }

        if let Event::Window(window::Event::FileDropped(path)) = event {
            return self.update_file(FileMessage::FileDropped(window_id, path));
        }

        if matches!(event, Event::Window(window::Event::Focused)) {
            self.focused_window_id = Some(window_id);
            return self.refresh_window_state(window_id);
        }
        if matches!(event, Event::Window(window::Event::Unfocused)) {
            if self.focused_window_id == Some(window_id) {
                self.focused_window_id = None;
            }
            if self.main_window_id == Some(window_id) {
                return self.queue_auto_save(self.workspace.active_document_id());
            }
            return Task::none();
        }
        if matches!(event, Event::Window(window::Event::Resized(_))) {
            return self.refresh_window_state(window_id);
        }

        if self.main_window_id == Some(window_id)
            && self.close_prompt.document().is_some()
            && !matches!(event, Event::Keyboard(keyboard::Event::ModifiersChanged(_)))
        {
            return Task::none();
        }

        if self.main_window_id == Some(window_id) && self.go_to_line_prompt.is_some() {
            if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
                self.keyboard_modifiers = modifiers;
            }
            if let Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = &event {
                return match key {
                    keyboard::Key::Named(keyboard::key::Named::Escape) => {
                        self.update_go_to_line(GoToLineMessage::GoToLineClosed)
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) => {
                        self.update_go_to_line(GoToLineMessage::GoToLineSubmitted)
                    }
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        iced::widget::operation::focus(crate::ui::go_to_line_prompt::INPUT_ID)
                    }
                    _ => Task::none(),
                };
            }
            return Task::none();
        }
        let advanced_search = self
            .advanced_search_window
            .is_some_and(|search_window| search_window.is(window_id));
        let inline_search = self.main_window_id == Some(window_id) && self.is_find_visible;
        if (advanced_search || inline_search)
            && let Some(action) = search_key_action(&event, status)
        {
            return match action {
                SearchKeyAction::Close => self.update_search(if advanced_search {
                    SearchMessage::AdvancedSearchClosed
                } else {
                    SearchMessage::HideFind
                }),
                SearchKeyAction::Next => self.update_search(if advanced_search {
                    SearchMessage::AdvancedFindNextRun
                } else {
                    SearchMessage::FindNext
                }),
                SearchKeyAction::Previous => self.update_search(if advanced_search {
                    SearchMessage::AdvancedFindPreviousRun
                } else {
                    SearchMessage::FindPrevious
                }),
                SearchKeyAction::FindAll if advanced_search => {
                    self.update_search(SearchMessage::AdvancedSearchRun)
                }
                SearchKeyAction::FindAll => Task::none(),
                SearchKeyAction::Focus { previous } => {
                    let scope = if advanced_search {
                        crate::ui::advanced_search_panel::PANEL_ID
                    } else {
                        crate::ui::find_panel::PANEL_ID
                    };
                    use iced::advanced::widget::{operate, operation};
                    if previous {
                        operate(operation::scope(
                            scope.into(),
                            operation::focusable::focus_previous(),
                        ))
                    } else {
                        operate(operation::scope(
                            scope.into(),
                            operation::focusable::focus_next(),
                        ))
                    }
                }
            };
        }
        // Escape is forwarded even when a text input consumes it, so the prompt
        // can dismiss in one press. Other captured keys keep their normal behavior.
        if status == Status::Captured
            && matches!(event, Event::Keyboard(keyboard::Event::KeyPressed { .. }))
        {
            return Task::none();
        }

        if let Some(command) = self.shortcut_capture_for_event(&event) {
            return self.update_settings(SettingsMessage::ShortcutCaptured(command.0, command.1));
        }

        if let Some(command) = self.shortcut_for_key_event(&event) {
            return self.update_shortcut(command);
        }

        if let Some(shortcut) = self.shortcut_for_runtime_event(&event, self.keyboard_modifiers) {
            return self.update_shortcut(shortcut);
        }

        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            self.keyboard_modifiers = modifiers;
        }

        Task::none()
    }

    pub(super) fn update_shortcut(&mut self, shortcut: ShortcutCommand) -> Task<Message> {
        self.menu.close();
        if let Some(action) = EditorAction::from_shortcut(shortcut) {
            return Task::batch([
                self.update_editor(self.workspace.active_document_id(), action),
                iced::widget::operation::focus(crate::ui::editor::EDITOR_ID),
            ]);
        }

        match shortcut {
            ShortcutCommand::ZoomIn => self.update_settings(SettingsMessage::ZoomIn),
            ShortcutCommand::ZoomOut => self.update_settings(SettingsMessage::ZoomOut),
            ShortcutCommand::ZoomReset => self.update_settings(SettingsMessage::ZoomReset),
            ShortcutCommand::NewFile => self.update_file(FileMessage::NewFile),
            ShortcutCommand::OpenFile => self.update_file(FileMessage::OpenFile),
            ShortcutCommand::SaveFile => self.update_file(FileMessage::SaveFile),
            ShortcutCommand::SaveFileAs => self.update_file(FileMessage::SaveFileAs),
            ShortcutCommand::GoToLine => self.update_go_to_line(GoToLineMessage::GoToLineOpened),
            ShortcutCommand::ToggleFind => self.update_search(SearchMessage::ToggleFind),
            ShortcutCommand::AdvancedFind => self.update_search(
                SearchMessage::ToggleAdvancedSearch(crate::message::AdvancedSearchTab::Find),
            ),
            ShortcutCommand::AdvancedReplace => self.update_search(
                SearchMessage::ToggleAdvancedSearch(crate::message::AdvancedSearchTab::Replace),
            ),
            _ => unreachable!("editor shortcuts are dispatched through EditorAction"),
        }
    }

    fn shortcut_capture_for_event(
        &mut self,
        event: &Event,
    ) -> Option<(ShortcutCommand, KeyBinding)> {
        let command = self.settings_dialog.capturing_shortcut?;
        let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            ..
        }) = event
        else {
            return None;
        };
        let binding = KeyBinding::from_event(modified_key, *modifiers)
            .or_else(|| KeyBinding::from_event(key, *modifiers))?;

        Some((command, binding))
    }

    pub(super) fn shortcut_for_runtime_event(
        &self,
        event: &Event,
        modifiers: keyboard::Modifiers,
    ) -> Option<ShortcutCommand> {
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if command_or_control(modifiers) =>
            {
                shortcut_for_scroll(*delta)
            }
            _ => None,
        }
    }

    fn shortcut_for_key_event(&self, event: &Event) -> Option<ShortcutCommand> {
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                modifiers,
                ..
            }) => self
                .settings
                .shortcuts
                .resolve(key, modified_key, *modifiers),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchKeyAction {
    Close,
    Next,
    Previous,
    FindAll,
    Focus { previous: bool },
}

fn search_key_action(event: &Event, status: Status) -> Option<SearchKeyAction> {
    let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
        return Some(SearchKeyAction::Close);
    }
    // Inputs leave Enter to this handler. Keys consumed by editing or a
    // dropdown keep that widget's behavior, including Enter inside the editor.
    if status == Status::Captured || modifiers.alt() {
        return None;
    }
    let command = command_or_control(*modifiers);
    match key {
        keyboard::Key::Named(keyboard::key::Named::Enter) if command => {
            Some(SearchKeyAction::FindAll)
        }
        keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::F3)
            if !command =>
        {
            Some(if modifiers.shift() {
                SearchKeyAction::Previous
            } else {
                SearchKeyAction::Next
            })
        }
        keyboard::Key::Named(keyboard::key::Named::Tab) if !command => {
            Some(SearchKeyAction::Focus {
                previous: modifiers.shift(),
            })
        }
        _ => None,
    }
}

pub fn event_to_message(event: Event, status: Status, window_id: window::Id) -> Option<Message> {
    should_forward_runtime_event(&event, status)
        .then_some(Message::RuntimeEvent(event, status, window_id))
}

fn should_forward_runtime_event(event: &Event, status: Status) -> bool {
    status == Status::Ignored
        || matches!(
            event,
            Event::Keyboard(keyboard::Event::ModifiersChanged(_))
                | Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                })
                | Event::Mouse(mouse::Event::WheelScrolled { .. })
                | Event::Window(window::Event::FileDropped(_))
                | Event::Window(
                    window::Event::Focused | window::Event::Unfocused | window::Event::Resized(_)
                )
        )
}

fn shortcut_for_scroll(delta: mouse::ScrollDelta) -> Option<ShortcutCommand> {
    let y = match delta {
        mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => y,
    };

    if y > 0.0 {
        Some(ShortcutCommand::ZoomIn)
    } else if y < 0.0 {
        Some(ShortcutCommand::ZoomOut)
    } else {
        None
    }
}

fn command_or_control(modifiers: keyboard::Modifiers) -> bool {
    modifiers.command() || modifiers.control()
}

#[cfg(test)]
mod tests {
    use super::{
        SearchKeyAction, event_to_message, search_key_action, shortcut_for_scroll,
        should_forward_runtime_event,
    };
    use crate::core::ShortcutCommand;
    use crate::message::Message;
    use iced::event::Event;
    use iced::{keyboard, mouse, window};
    use std::path::PathBuf;

    fn search_key(named: keyboard::key::Named, modifiers: keyboard::Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(named),
            modified_key: keyboard::Key::Named(named),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn search_navigation_respects_modifiers_and_captured_editor_keys() {
        use iced::event::Status;
        use keyboard::Modifiers;
        use keyboard::key::Named;
        for key in [Named::Enter, Named::F3] {
            assert_eq!(
                search_key_action(&search_key(key, Modifiers::empty()), Status::Ignored),
                Some(SearchKeyAction::Next)
            );
            assert_eq!(
                search_key_action(&search_key(key, Modifiers::SHIFT), Status::Ignored),
                Some(SearchKeyAction::Previous)
            );
            assert_eq!(
                search_key_action(&search_key(key, Modifiers::empty()), Status::Captured),
                None,
                "search must not consume an editor's captured key"
            );
        }
        assert_eq!(
            search_key_action(&search_key(Named::Enter, Modifiers::CTRL), Status::Ignored),
            Some(SearchKeyAction::FindAll)
        );
        assert_eq!(
            search_key_action(&search_key(Named::Enter, Modifiers::ALT), Status::Ignored),
            None
        );
        assert_eq!(
            search_key_action(&search_key(Named::Tab, Modifiers::SHIFT), Status::Ignored),
            Some(SearchKeyAction::Focus { previous: true })
        );
        assert_eq!(
            search_key_action(
                &search_key(Named::Escape, Modifiers::empty()),
                Status::Captured
            ),
            Some(SearchKeyAction::Close)
        );
    }

    #[test]
    fn scroll_direction_maps_to_zoom_shortcuts() {
        assert_eq!(
            shortcut_for_scroll(mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }),
            Some(ShortcutCommand::ZoomIn)
        );
        assert_eq!(
            shortcut_for_scroll(mouse::ScrollDelta::Pixels { x: 0.0, y: -1.0 }),
            Some(ShortcutCommand::ZoomOut)
        );
        assert_eq!(
            shortcut_for_scroll(mouse::ScrollDelta::Lines { x: 0.0, y: 0.0 }),
            None
        );
    }

    #[test]
    fn captured_wheel_and_modifier_events_are_forwarded_to_shortcut_processor() {
        let wheel = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });
        let modifiers =
            Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL));
        let captured_key = Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("x".into()),
            modified_key: keyboard::Key::Character("x".into()),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
            repeat: false,
        });

        assert!(should_forward_runtime_event(
            &wheel,
            iced::event::Status::Captured
        ));
        assert!(should_forward_runtime_event(
            &modifiers,
            iced::event::Status::Captured
        ));
        assert!(!should_forward_runtime_event(
            &captured_key,
            iced::event::Status::Captured
        ));
    }

    #[test]
    fn file_drop_events_are_forwarded_even_when_captured() {
        let path = PathBuf::from("dropped.txt");
        let event = Event::Window(window::Event::FileDropped(path.clone()));
        let window_id = window::Id::unique();

        assert!(should_forward_runtime_event(
            &event,
            iced::event::Status::Captured
        ));

        assert!(matches!(
            event_to_message(event, iced::event::Status::Captured, window_id),
            Some(Message::RuntimeEvent(
                Event::Window(window::Event::FileDropped(forwarded_path)),
                iced::event::Status::Captured,
                forwarded_window_id
            )) if forwarded_path == path && forwarded_window_id == window_id
        ));
    }
}
