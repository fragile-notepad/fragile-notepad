//! Focus-aware editing shared by menu projection and native dispatch.

use iced::Task;
use iced::advanced::widget::{self, operation};
use objc2::runtime::Sel;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{MainThreadMarker, NSPoint, NSString};

use crate::core::ShortcutCommand;
use crate::message::{Message, NativeEditCommand};

impl NativeEditCommand {
    pub(super) fn from_message(message: &Message) -> Option<Self> {
        Some(match message {
            Message::Undo => Self::Undo,
            Message::Redo => Self::Redo,
            Message::Cut => Self::Cut,
            Message::Copy => Self::Copy,
            Message::Paste => Self::Paste,
            Message::Delete => Self::Delete,
            Message::Shortcut(ShortcutCommand::SelectAll) => Self::SelectAll,
            _ => return None,
        })
    }

    pub(super) fn editor_message(self) -> Message {
        match self {
            Self::Undo => Message::Undo,
            Self::Redo => Message::Redo,
            Self::Cut => Message::Cut,
            Self::Copy => Message::Copy,
            Self::Paste => Message::Paste,
            Self::Delete => Message::Delete,
            Self::SelectAll => Message::Shortcut(ShortcutCommand::SelectAll),
        }
    }

    pub(super) fn responder_action(self) -> Sel {
        match self {
            Self::Undo => sel!(undo:),
            Self::Redo => sel!(redo:),
            Self::Cut => sel!(cut:),
            Self::Copy => sel!(copy:),
            Self::Paste => sel!(paste:),
            Self::Delete => sel!(delete:),
            Self::SelectAll => sel!(selectAll:),
        }
    }

    pub(super) fn supports_text_input(self) -> bool {
        !matches!(self, Self::Undo | Self::Redo)
    }
}

pub(in crate::app) fn edit(command: NativeEditCommand) -> Task<Message> {
    widget::operate(EditFocused {
        command,
        focused_input: FocusedInput::None,
    })
}

#[derive(Clone, Copy)]
enum FocusedInput {
    None,
    Editor,
    TextField,
}

struct EditFocused {
    command: NativeEditCommand,
    focused_input: FocusedInput,
}

impl widget::Operation<Message> for EditFocused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation<Message>)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        id: Option<&widget::Id>,
        _bounds: iced::Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        if state.is_focused() {
            self.focused_input = if id == Some(&widget::Id::new(crate::ui::editor::EDITOR_ID)) {
                FocusedInput::Editor
            } else {
                FocusedInput::TextField
            };
        }
    }

    fn finish(&self) -> operation::Outcome<Message> {
        match self.focused_input {
            FocusedInput::Editor => operation::Outcome::Some(self.command.editor_message()),
            FocusedInput::TextField => {
                // Operations run on the UI thread and target the focused Iced field.
                inject_text_edit(self.command);
                operation::Outcome::None
            }
            FocusedInput::None => operation::Outcome::None,
        }
    }
}

fn inject_text_edit(command: NativeEditCommand) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(window) = app.keyWindow() else {
        return;
    };
    let Some(responder) = window.firstResponder() else {
        return;
    };
    let (character, key_code, modifiers) = match command {
        NativeEditCommand::Cut => ("x", 7, NSEventModifierFlags::Command),
        NativeEditCommand::Copy => ("c", 8, NSEventModifierFlags::Command),
        NativeEditCommand::Paste => ("v", 9, NSEventModifierFlags::Command),
        NativeEditCommand::SelectAll => ("a", 0, NSEventModifierFlags::Command),
        NativeEditCommand::Delete => ("\u{f728}", 117, NSEventModifierFlags::empty()),
        // Iced text input has no undo stack. Sending CmdZ would fall through
        // to editor shortcuts and alter the background document.
        NativeEditCommand::Undo | NativeEditCommand::Redo => return,
    };
    let characters = NSString::from_str(character);
    let time = app.currentEvent().map_or(0.0, |event| event.timestamp());
    let make_event = |kind, flags| {
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::new(0.0, 0.0),
            flags,
            time,
            window.windowNumber(),
            None,
            &characters,
            &characters,
            false,
            key_code,
        )
    };
    if let Some(event) = make_event(NSEventType::KeyDown, modifiers) {
        responder.keyDown(&event);
    }
    if let Some(event) = make_event(NSEventType::KeyUp, modifiers) {
        responder.keyUp(&event);
    }
    // The injected chord must not leave Command/Shift pressed inside winit.
    let actual_modifiers = app
        .currentEvent()
        .map_or(NSEventModifierFlags::empty(), |event| event.modifierFlags());
    if let Some(event) = make_event(NSEventType::FlagsChanged, actual_modifiers) {
        responder.flagsChanged(&event);
    }
}
