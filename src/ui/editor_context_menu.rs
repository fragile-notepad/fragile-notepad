use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer, text};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::core::{Document, DocumentId, EditorSettings};
use crate::editor::widget::line_cache::measured_position_point_with_context;
use crate::editor::{AdvancedEditorState, EditorMetrics};
use crate::message::Message;
use crate::ui::context_menu::{ContextMenu, State as PopupState, is_context_menu_key};
use crate::ui::menu::{self, MenuNode};
use crate::ui::toolbar;

pub(super) fn wrap<'a>(
    editor: Element<'a, Message>,
    document: &'a Document,
    settings: &'a EditorSettings,
    metrics: EditorMetrics,
) -> Element<'a, Message> {
    Element::new(EditorContextMenu {
        editor,
        document,
        settings,
        metrics,
    })
}

struct EditorContextMenu<'a> {
    editor: Element<'a, Message>,
    document: &'a Document,
    settings: &'a EditorSettings,
    metrics: EditorMetrics,
}

#[derive(Default)]
struct State {
    document: Option<DocumentId>,
    popup: PopupState,
}

impl Widget<Message, Theme, Renderer> for EditorContextMenu<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.document != Some(self.document.id) {
            state.popup.close();
            state.document = Some(self.document.id);
        }
        tree.diff_children(std::slice::from_mut(&mut self.editor));
    }

    fn size(&self) -> Size<Length> {
        self.editor.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.editor
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        self.editor.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.traverse(&mut |operation| {
            self.editor
                .as_widget_mut()
                .operate(&mut tree.children[0], layout, renderer, operation);
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
        let bounds = layout.bounds();
        let pointer_open = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
        ) && cursor.position_in(bounds).is_some_and(|point| {
            point.x >= self.metrics.text_origin_x(&self.document.decorations)
                && point.x < bounds.width - 14.0
        });
        let focused = tree.children[0]
            .state
            .downcast_ref::<AdvancedEditorState<<Renderer as text::Renderer>::Paragraph>>()
            .is_focused();
        let keyboard_open = focused && is_context_menu_key(event);
        if keyboard_open {
            let editor_state = tree.children[0]
                .state
                .downcast_mut::<AdvancedEditorState<<Renderer as text::Renderer>::Paragraph>>();
            editor_state.finish_scrolling();
            editor_state.unfocus();
            editor_state.focus();
            let editor_layout = crate::editor::EditorLayout::new(
                self.metrics,
                self.document.scroll,
                bounds.width,
                bounds.height,
            );
            let caret = self.document.main_selection().cursor;
            let caret_point = measured_position_point_with_context(
                &self.document.buffer,
                &self.document.viewport,
                &self.document.decorations,
                editor_layout,
                caret,
                self.document.caret_visible_row(),
                renderer,
                Some(&self.document.cjk_context()),
            );
            let anchor = Point::new(
                caret_point.x.clamp(
                    self.metrics
                        .text_origin_x(&self.document.decorations)
                        .min(bounds.width),
                    bounds.width,
                ),
                (caret_point.y + self.metrics.line_height).clamp(0.0, bounds.height),
            );
            tree.state.downcast_mut::<State>().popup.open(
                anchor,
                &entries(self.document, self.settings),
                true,
            );
            shell.publish(Message::MenuClosed);
            *shell.input_method_mut() = iced::advanced::InputMethod::Disabled;
            shell.capture_event();
            shell.invalidate_widgets();
            shell.request_redraw();
            return;
        }
        self.editor.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
        if pointer_open {
            tree.state.downcast_mut::<State>().popup.open(
                cursor.position_in(bounds).unwrap(),
                &[],
                false,
            );
            shell.publish(Message::MenuClosed);
            shell.capture_event();
            shell.invalidate_widgets();
            shell.request_redraw();
        } else if matches!(event, Event::Window(iced::window::Event::Unfocused)) {
            tree.state.downcast_mut::<State>().popup.close();
        }
        if tree.state.downcast_ref::<State>().popup.is_open() {
            *shell.input_method_mut() = iced::advanced::InputMethod::Disabled;
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.editor.as_widget().mouse_interaction(
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
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        let anchor =
            state.popup.anchor()? + Vector::new(layout.bounds().x, layout.bounds().y) + translation;
        Some(overlay::Element::new(Box::new(ContextMenu::new(
            &mut state.popup,
            entries(self.document, self.settings),
            self.settings,
            anchor,
        ))))
    }
}

fn entries(document: &Document, settings: &EditorSettings) -> Vec<MenuNode> {
    let mut entries = toolbar::edit_menu_entries(settings);
    entries.push(menu::separator());
    entries.push(menu::submenu(
        "context-search",
        "Search",
        vec![
            toolbar::menu_item(
                settings,
                "Find…",
                crate::core::ShortcutCommand::ToggleFind,
                Message::ToggleFind,
            ),
            toolbar::menu_item(
                settings,
                "Replace…",
                crate::core::ShortcutCommand::AdvancedReplace,
                Message::ToggleAdvancedSearch(crate::message::AdvancedSearchTab::Replace),
            ),
            menu::item("Find Next", Message::FindNext),
            menu::item("Find Previous", Message::FindPrevious),
        ],
    ));
    entries.push(menu::submenu(
        "context-navigation",
        "Navigation",
        toolbar::matching_and_function_items(settings),
    ));
    entries.push(toolbar::fold_commands_menu(settings));
    toolbar::apply_editor_availability(&mut entries, Some(document));
    entries
}

#[cfg(test)]
mod tests;
