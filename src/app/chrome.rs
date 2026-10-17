//! Application chrome and modal animation coordination.

use super::App;
use crate::message::{AboutTab, Message};
use iced::Task;
use std::time::Instant;

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
            self.menu.close();
        }
        self.is_window_list_visible = visible;
        Task::none()
    }

    pub(super) fn open_about_dialog(&mut self) -> Task<Message> {
        self.menu.close();

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
        self.settings_dialog.update_shortcut_notice_animation(at);
        let close = self
            .close_prompt
            .update_frame(at)
            .map(Message::DirtyCloseFadeFinished)
            .map_or_else(Task::none, Task::done);
        Task::batch([close, self.update_go_to_line_frame(at)])
    }
}
