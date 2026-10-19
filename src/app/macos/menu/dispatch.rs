//! Objective-C menu targets and the application message channel.

use std::cell::RefCell;
use std::sync::Mutex;

use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::{SinkExt, StreamExt};
use iced::{Subscription, stream};
use objc2::rc::Retained;
use objc2::{MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};

use super::model::{MenuCommand, MenuState};
use crate::message::Message;

static OUTPUT: Mutex<Option<UnboundedSender<Message>>> = Mutex::new(None);

thread_local! {
    // AppKit does not retain menu item targets. Keep the target and its command
    // table alive for exactly as long as the installed native menu.
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

struct Bridge {
    _target: Retained<MenuTarget>,
    commands: Vec<MenuCommand>,
    state: MenuState,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FragileNotepadMenuTarget"]
    pub(super) struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(performMenuAction:))]
        fn perform(&self, item: &NSMenuItem) {
            let Some(command) = command(item) else { return };
            let app = NSApplication::sharedApplication(MainThreadMarker::from(self));
            if (command.edit.is_none() || capturing_shortcut())
                && let Some(event) = app.currentEvent()
                && is_accelerator(&event, item)
                && let Some(responder) = app.keyWindow().and_then(|window| window.firstResponder())
            {
                // Deliver accelerators to the window, bypassing NSApplication's
                // menu matching. Recording shortcuts must also receive Edit
                // keys before they become focus-aware editing commands.
                responder.keyDown(&event);
                return;
            }
            if let Some(edit) = command.edit {
                let action = edit.responder_action();
                // A native file panel owns its text editing. Passing nil as the
                // target lets AppKit find the panel's current text responder.
                if unsafe { app.sendAction_to_from(action, None, Some(item)) } {
                    return;
                }
                // Route both clicks and accelerators through focus-aware
                // editing. Iced fields do not support every editor command.
                if command.enabled {
                    send(command.message);
                }
                return;
            }
            if command.enabled {
                send(command.message);
            }
        }

        #[unsafe(method(validateMenuItem:))]
        fn validate(&self, item: &NSMenuItem) -> bool {
            if capturing_shortcut() && !item.keyEquivalent().to_string().is_empty() {
                true
            } else {
                command(item).is_some_and(|command| {
                    if let Some(edit) = command.edit {
                        let action = edit.responder_action();
                        let app = NSApplication::sharedApplication(MainThreadMarker::from(self));
                        if unsafe { app.targetForAction_to_from(action, None, Some(item)) }.is_some() {
                            return true;
                        }
                    }
                    command.enabled
                })
            }
        }
    }

    unsafe impl NSObjectProtocol for MenuTarget {}
);

impl MenuTarget {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![mtm.alloc::<Self>(), init] }
    }
}

fn command(item: &NSMenuItem) -> Option<MenuCommand> {
    let index = usize::try_from(item.tag()).ok()?;
    BRIDGE.with(|bridge| bridge.borrow().as_ref()?.commands.get(index).cloned())
}

fn capturing_shortcut() -> bool {
    BRIDGE.with(|bridge| {
        bridge
            .borrow()
            .as_ref()
            .is_some_and(|bridge| bridge.state.capturing_shortcut)
    })
}

fn is_accelerator(event: &NSEvent, item: &NSMenuItem) -> bool {
    if event.r#type() != NSEventType::KeyDown {
        return false;
    }
    let key = item.keyEquivalent().to_string();
    let Some(characters) = event.charactersIgnoringModifiers() else {
        return false;
    };
    let relevant = NSEventModifierFlags::Command
        | NSEventModifierFlags::Control
        | NSEventModifierFlags::Option
        | NSEventModifierFlags::Shift;
    !key.is_empty()
        && characters.to_string().to_lowercase() == key.to_lowercase()
        && event.modifierFlags() & relevant == item.keyEquivalentModifierMask() & relevant
}

fn send(message: Message) {
    if let Ok(output) = OUTPUT.lock()
        && let Some(output) = output.as_ref()
    {
        let _ = output.unbounded_send(message);
    }
}

pub(in crate::app) fn subscription() -> Subscription<Message> {
    Subscription::run(messages)
}

fn messages() -> impl futures::Stream<Item = Message> {
    stream::channel(32, async |mut output| {
        let (sender, mut receiver) = unbounded();
        if let Ok(mut current) = OUTPUT.lock() {
            *current = Some(sender);
        }
        while let Some(message) = receiver.next().await {
            if output.send(message).await.is_err() {
                break;
            }
        }
    })
}

pub(super) fn configure_item(
    item: &NSMenuItem,
    target: &MenuTarget,
    commands: &mut Vec<MenuCommand>,
    command: MenuCommand,
) {
    item.setTag(commands.len() as isize);
    unsafe {
        item.setTarget(Some(target));
        item.setAction(Some(sel!(performMenuAction:)));
    }
    commands.push(command);
}

pub(super) fn is_current(state: &MenuState) -> bool {
    BRIDGE.with(|bridge| {
        bridge
            .borrow()
            .as_ref()
            .is_some_and(|bridge| bridge.state == *state)
    })
}

pub(super) fn install(target: Retained<MenuTarget>, commands: Vec<MenuCommand>, state: MenuState) {
    BRIDGE.with(|bridge| {
        *bridge.borrow_mut() = Some(Bridge {
            _target: target,
            commands,
            state,
        });
    });
}
