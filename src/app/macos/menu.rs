//! Cocoa rendering of the workbench menu model.

mod dispatch;
mod model;
mod shortcut;

use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};

use crate::message::Message;
use crate::ui::menu::MenuNode;

use dispatch::{MenuTarget, configure_item};
use model::{MenuCommand, MenuState};
use shortcut::key_equivalent;

pub(in crate::app) use dispatch::subscription;
pub(super) use model::MenuModel;

pub(super) fn sync(model: MenuModel<'_>) {
    // Unit tests exercise App on worker threads without an AppKit event loop.
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    let Some(menubar) = ns_app.mainMenu() else {
        return;
    };
    // Compare typed inputs before allocating language/encoding rows or touching
    // Cocoa. Edits and animation frames usually leave the menu unchanged.
    if dispatch::is_current(&model.state) {
        return;
    }
    let mut builder = MenuBuilder {
        target: MenuTarget::new(mtm),
        commands: Vec::new(),
        state: &model.state,
    };
    // Retain AppKit's application menu, including Services, Hide and native Quit.
    while menubar.numberOfItems() > 1 {
        menubar.removeItemAtIndex(1);
    }
    if let Some(app_item) = menubar.itemAtIndex(0) {
        app_item.setTitle(&NSString::from_str("Fragile Notepad"));
        if let Some(app_menu) = app_item.submenu() {
            app_menu.setTitle(&NSString::from_str("Fragile Notepad"));
            if let Some(about) = app_menu.itemAtIndex(0) {
                builder.bind(&about, model.state.command(&Message::AboutOpened, true));
            }
        }
    }
    for (label, tree) in model.trees() {
        let menu = NSMenu::new(mtm);
        menu.setTitle(&NSString::from_str(label));
        menu.setAutoenablesItems(true);
        builder.append_nodes(&menu, &tree.entries);
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(label));
        item.setSubmenu(Some(&menu));
        menubar.addItem(&item);
    }
    dispatch::install(builder.target, builder.commands, model.state);
}

struct MenuBuilder<'a> {
    target: Retained<MenuTarget>,
    commands: Vec<MenuCommand>,
    state: &'a MenuState,
}

impl MenuBuilder<'_> {
    fn append_nodes(&mut self, menu: &NSMenu, nodes: &[MenuNode]) {
        let mtm = MainThreadMarker::from(menu);
        for node in nodes {
            let (label, shortcut, message, available) = match node {
                MenuNode::Separator => {
                    menu.addItem(&NSMenuItem::separatorItem(mtm));
                    continue;
                }
                MenuNode::Submenu {
                    label, children, ..
                } => {
                    let submenu = NSMenu::new(mtm);
                    submenu.setTitle(&NSString::from_str(label));
                    submenu.setAutoenablesItems(true);
                    self.append_nodes(&submenu, children);
                    let item = NSMenuItem::new(mtm);
                    item.setTitle(&NSString::from_str(label));
                    item.setSubmenu(Some(&submenu));
                    menu.addItem(&item);
                    continue;
                }
                MenuNode::Item {
                    label,
                    shortcut,
                    message,
                } => (label, shortcut.as_ref(), Some(message), true),
                // Keep command identity when availability changes: native file
                // panels can still handle editing through their responder chain.
                MenuNode::Disabled {
                    label,
                    shortcut,
                    message,
                } => (label, shortcut.as_ref(), message.as_ref(), false),
            };
            let (key, modifiers) = key_equivalent(shortcut);
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str(label),
                    None,
                    &NSString::from_str(&key),
                )
            };
            item.setKeyEquivalentModifierMask(modifiers);
            if let Some(message) = message {
                item.setState(if self.state.checked(message) { 1 } else { 0 });
                self.bind(&item, self.state.command(message, available));
            } else {
                item.setEnabled(false);
            }
            menu.addItem(&item);
        }
    }

    fn bind(&mut self, item: &NSMenuItem, command: MenuCommand) {
        configure_item(item, &self.target, &mut self.commands, command);
    }
}
