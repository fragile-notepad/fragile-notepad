//! Application chrome and modal animation coordination.

use super::App;
use crate::core::DocumentId;
use crate::message::{AboutTab, Message, WindowTarget};
use iced::Task;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub(super) enum SelectorCloseAction {
    FocusWindow(WindowTarget),
    SelectTab(DocumentId),
}

impl App {
    #[cfg(debug_assertions)]
    pub(super) fn toggle_title_bar_style(&mut self) -> Task<Message> {
        self.title_bar_style = self.title_bar_style.toggled();
        Task::none()
    }

    pub(super) fn select_about_tab(&mut self, tab: AboutTab) -> Task<Message> {
        self.about_tab = tab;
        Task::none()
    }

    pub(super) fn close_about_dialog(&mut self) -> Task<Message> {
        self.is_about_visible = false;
        self.chrome_animation.about.set_visible(false);
        Task::none()
    }

    pub(super) fn set_window_list_visible(&mut self, visible: bool) -> Task<Message> {
        if visible {
            self.open_selector(false)
        } else if self.is_window_list_visible {
            self.close_selector()
        } else {
            Task::none()
        }
    }

    pub(super) fn submit_window_list(&mut self) -> Task<Message> {
        if !self.is_window_list_visible || !self.chrome_animation.selector.target_visible() {
            return Task::none();
        }
        self.window_list_entries()
            .into_iter()
            .find(|entry| entry.matches_query(&self.window_list_query))
            .map_or_else(Task::none, |entry| self.focus_window(entry.target))
    }

    pub(super) fn set_tab_list_visible(&mut self, visible: bool) -> Task<Message> {
        if visible {
            self.open_selector(true)
        } else if self.is_tab_list_visible {
            self.close_selector()
        } else {
            Task::none()
        }
    }

    fn open_selector(&mut self, tabs: bool) -> Task<Message> {
        if self.close_prompt.document().is_some()
            || self.go_to_line_prompt.is_some()
            || self.chrome_animation.about.rendered_visible()
            || ((self.is_window_list_visible || self.is_tab_list_visible)
                && !self.chrome_animation.selector.target_visible())
        {
            return Task::none();
        }
        self.menu.close();
        self.selector_close_action = None;
        self.is_window_list_visible = !tabs;
        self.is_tab_list_visible = tabs;
        if tabs {
            self.tab_list_query.clear();
        } else {
            self.window_list_query.clear();
        }
        self.chrome_animation.selector.set_visible(true);
        iced::widget::operation::focus(if tabs {
            crate::ui::tab_list_dialog::INPUT_ID
        } else {
            crate::ui::window_list_dialog::INPUT_ID
        })
    }

    fn close_selector(&mut self) -> Task<Message> {
        if !self.chrome_animation.selector.target_visible() {
            return Task::none();
        }
        self.chrome_animation.selector.set_visible(false);
        // Retain the modal and its input barrier until its last fade frame.
        if self.chrome_animation.selector.rendered_visible() {
            Task::none()
        } else {
            self.finish_selector_close()
        }
    }

    pub(super) fn defer_selector_action(&mut self, action: SelectorCloseAction) -> Task<Message> {
        if !self.chrome_animation.selector.target_visible() {
            return Task::none();
        }
        self.selector_close_action = Some(action);
        self.close_selector()
    }

    pub(super) fn dismiss_selectors(&mut self) {
        self.is_window_list_visible = false;
        self.is_tab_list_visible = false;
        self.selector_close_action = None;
        self.chrome_animation.selector = super::animation::RevealAnimation::hidden();
    }

    fn finish_selector_close(&mut self) -> Task<Message> {
        let action = self.selector_close_action.take();
        self.dismiss_selectors();
        match action {
            Some(SelectorCloseAction::FocusWindow(target)) => self.focus_window(target),
            // A tab may have moved to another editor while the selector faded.
            Some(SelectorCloseAction::SelectTab(id))
                if self.workspace.owner_of(id) == Some(self.workspace.current_window_id()) =>
            {
                let select = self.update_file(crate::message::FileMessage::TabSelected(id));
                Task::batch([select, self.focus_editor_after_close_prompt()])
            }
            _ => self.focus_editor_after_close_prompt(),
        }
    }

    pub(super) fn submit_tab_list(&mut self) -> Task<Message> {
        let document = self
            .workspace
            .window_documents(self.workspace.current_window_id())
            .find(|document| {
                crate::ui::tab_list_dialog::matches_query(document, &self.tab_list_query)
            })
            .map(|document| document.id);
        document.map_or_else(Task::none, |id| self.select_tab_from_list(id))
    }

    pub(super) fn select_tab_from_list(&mut self, id: crate::core::DocumentId) -> Task<Message> {
        if !self.is_tab_list_visible
            || !self.chrome_animation.selector.target_visible()
            || self.workspace.owner_of(id) != Some(self.workspace.current_window_id())
        {
            return Task::none();
        }
        self.defer_selector_action(SelectorCloseAction::SelectTab(id))
    }

    pub(super) fn open_about_dialog(&mut self) -> Task<Message> {
        self.menu.close();
        self.dismiss_selectors();

        self.is_about_visible = true;
        if !self.chrome_animation.about.rendered_visible() {
            self.about_tab = AboutTab::About;
        }
        self.chrome_animation.about.set_visible(true);

        self.request_about_gpu_boost()
    }

    pub(super) fn needs_animation_frames(&self) -> bool {
        self.editor_windows
            .values()
            .filter_map(|window| window.ui.as_ref())
            .any(|ui| {
                ui.chrome_animation.needs_frames()
                    || ui.close_prompt.needs_frames()
                    || ui
                        .go_to_line_prompt
                        .as_ref()
                        .is_some_and(|p| p.animation.needs_frames())
            })
            || self.chrome_animation.needs_frames()
            || self.close_prompt.needs_frames()
            || self.settings_dialog.shortcut_notice_needs_frames()
            || self
                .go_to_line_prompt
                .as_ref()
                .is_some_and(|prompt| prompt.animation.needs_frames())
    }

    pub(super) fn update_chrome_animation_frame(&mut self, at: Instant) -> Task<Message> {
        self.chrome_animation.update_frame(at);
        let selector = if (self.is_window_list_visible || self.is_tab_list_visible)
            && !self.chrome_animation.selector.rendered_visible()
        {
            self.finish_selector_close()
        } else {
            Task::none()
        };
        self.settings_dialog.update_shortcut_notice_animation(at);
        let close = self
            .close_prompt
            .update_frame(at)
            .map(Message::DirtyCloseFadeFinished)
            .map_or_else(Task::none, Task::done);
        Task::batch([selector, close, self.update_go_to_line_frame(at)])
    }
}
