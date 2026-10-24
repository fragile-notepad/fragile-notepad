//! A workbench popup addressed to the clicked tab in its scrolling strip.

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, window};

use crate::core::{DocumentId, EditorSettings, EditorWindowId, ShortcutCommand, Workspace};
use crate::message::{Message, TabContextAction};
use crate::ui::context_menu::{ContextMenu, State as PopupState};
use crate::ui::menu::{self, MenuNode};
use crate::ui::{tabs, toolbar};

pub(super) fn wrap<'a>(
    content: Element<'a, Message>,
    workspace: &'a Workspace,
    editor_window: EditorWindowId,
    settings: &'a EditorSettings,
    windows: Vec<(window::Id, String)>,
    dragging: bool,
) -> Element<'a, Message> {
    Element::new(TabContextMenu {
        content,
        workspace,
        editor_window,
        settings,
        windows,
        dragging,
    })
}

struct TabContextMenu<'a> {
    content: Element<'a, Message>,
    workspace: &'a Workspace,
    editor_window: EditorWindowId,
    settings: &'a EditorSettings,
    windows: Vec<(window::Id, String)>,
    dragging: bool,
}

#[derive(Default)]
struct State {
    popup: PopupState,
    target: Option<DocumentId>,
    entries: Vec<MenuNode>,
}

impl Widget<Message, Theme, Renderer> for TabContextMenu<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if self.dragging
            || state
                .target
                .is_some_and(|id| self.workspace.owner_of(id) != Some(self.editor_window))
        {
            state.popup.close();
        }
        if !state.popup.is_open() {
            state.target = None;
            state.entries.clear();
        }
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if shell.is_event_captured() {
            return;
        }
        if !self.dragging
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
            )
            && let Some(point) = cursor.position()
        {
            let geometry = tabs::geometry(
                &mut self.content,
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
            );
            if let Some(target) = geometry.tab_at(point) {
                let entries = entries(
                    self.workspace,
                    self.editor_window,
                    target,
                    self.settings,
                    &self.windows,
                );
                let state = tree.state.downcast_mut::<State>();
                state.popup.open(
                    point - Vector::new(layout.bounds().x, layout.bounds().y),
                    &entries,
                    false,
                );
                state.target = Some(target);
                // Retain the menu snapshot while open: a window closing cannot
                // invalidate an expanded submenu's indices. Commands validate IDs.
                state.entries = entries;
                shell.publish(Message::MenuClosed);
                *shell.input_method_mut() = iced::advanced::InputMethod::Disabled;
                shell.capture_event();
                shell.invalidate_widgets();
                shell.request_redraw();
                return;
            }
        }
        if matches!(
            event,
            Event::Window(window::Event::Unfocused | window::Event::Resized(_))
        ) {
            tree.state.downcast_mut::<State>().popup.close();
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
        if tree.state.downcast_ref::<State>().popup.is_open() {
            *shell.input_method_mut() = iced::advanced::InputMethod::Disabled;
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if let Some(anchor) = state.popup.anchor() {
            let target = state.target?;
            Some(overlay::Element::new(Box::new(
                ContextMenu::new(
                    &mut state.popup,
                    state.entries.clone(),
                    self.settings,
                    anchor + Vector::new(layout.bounds().x, layout.bounds().y) + translation,
                )
                .with_shortcut_override(
                    ShortcutCommand::CloseFile,
                    Message::TabContextAction(target, TabContextAction::Close),
                ),
            )))
        } else {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}

fn entries(
    workspace: &Workspace,
    editor_window: EditorWindowId,
    target: DocumentId,
    settings: &EditorSettings,
    windows: &[(window::Id, String)],
) -> Vec<MenuNode> {
    let Some(document) = workspace.document(target) else {
        return Vec::new();
    };
    let ids = workspace.window_tab_ids(editor_window);
    let index = ids.iter().position(|id| *id == target).unwrap_or(0);
    let item = |label, action, available| {
        if available {
            menu::item(label, Message::TabContextAction(target, action))
        } else {
            menu::disabled(label)
        }
    };
    let close = toolbar::menu_item(
        settings,
        "Close",
        ShortcutCommand::CloseFile,
        Message::TabContextAction(target, TabContextAction::Close),
    );
    let mut entries = vec![
        close,
        item(
            "Close Other Tabs",
            TabContextAction::CloseOthers,
            ids.len() > 1,
        ),
        item(
            "Close All Tabs",
            TabContextAction::CloseAll,
            !ids.is_empty(),
        ),
        item(
            "Close Unmodified Tabs",
            TabContextAction::CloseUnmodified,
            workspace
                .window_documents(editor_window)
                .any(|doc| !doc.is_dirty),
        ),
        item(
            "Close Tabs to the Left",
            TabContextAction::CloseToLeft,
            index > 0,
        ),
        item(
            "Close Tabs to the Right",
            TabContextAction::CloseToRight,
            index + 1 < ids.len(),
        ),
        item(
            "Close All but Pinned Tabs",
            TabContextAction::CloseUnpinned,
            workspace
                .window_documents(editor_window)
                .any(|doc| !doc.is_pinned),
        ),
        menu::separator(),
        item(
            "Copy Full Path",
            TabContextAction::CopyPath,
            document.path.is_some(),
        ),
        menu::separator(),
        item(
            if document.is_pinned {
                "Unpin Tab"
            } else {
                "Pin Tab"
            },
            TabContextAction::TogglePin,
            true,
        ),
        item(
            "Move Tab to New Window",
            TabContextAction::MoveToNewWindow,
            ids.len() > 1,
        ),
    ];
    if !windows.is_empty() {
        entries.push(menu::submenu(
            "move-tab-to-window",
            "Move Tab to Window",
            windows
                .iter()
                .map(|(id, title)| {
                    menu::item(
                        title.clone(),
                        Message::TabContextAction(target, TabContextAction::MoveToWindow(*id)),
                    )
                })
                .collect(),
        ));
    }
    entries
}

#[cfg(test)]
mod tests;
