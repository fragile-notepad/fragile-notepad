//! Projects application state into window views and captions.

use super::{
    App, rendering,
    windowing::{managed::ManagedWindow, title::Title},
};
use crate::{message::Message, ui};
use iced::{Element, Theme, window};

impl App {
    pub fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        let perf_span = crate::perf_trace::span("app_view", format_args!("window={window_id:?}"));

        crate::startup::report_first_view_ready();

        let editor_id = self.editor_id(window_id);
        let editor_ui = editor_id.and_then(|id| self.window_ui(id));
        let element = if let Some(settings_window) = &self.settings_window
            && settings_window.is(window_id)
        {
            settings_window.view(&self.settings_dialog)
        } else if let Some(search_window) = &self.advanced_search_window
            && search_window.is(window_id)
        {
            search_window.view(&self.search_dialog)
        } else if let Some(ui) = &editor_ui {
            ui::workbench::view(ui::view_model::WorkbenchView {
                workspace: &self.workspace,
                editor_window: editor_id.unwrap(),
                find: ui.find,
                settings: &self.settings,
                is_find_visible: *ui.is_find_visible,
                is_inline_replace_visible: *ui.is_inline_replace_visible,
                is_function_list_visible: *ui.is_function_list_visible,
                function_list_query: ui.function_list_query,
                chrome_animation: Self::animation_info(*ui.chrome_animation, ui.close_prompt),
                active_menu: ui.menu.active(),
                active_menu_path: ui.menu.path(),
                window_menu_state: self.window_menu_state_for(window_id),
                dragged_tab: self.tab_drag.as_ref().map(|drag| drag.pressed),
                hovered_drop_tab: *ui.hovered_drop_tab,
                dirty_close_document: ui
                    .close_prompt
                    .document()
                    .and_then(|id| self.workspace.document(id)),
                about_tab: ui
                    .chrome_animation
                    .about
                    .rendered_visible()
                    .then_some(*ui.about_tab),
                rendering_debug_info: ui::about_dialog::RenderingDebugInfo {
                    title_bar_style: self.title_bar_style,
                    current_renderer: self.rendering.label(),
                    rendering_policy: rendering::current_policy_label(&self.settings),
                },
                window_list_entries: ui
                    .is_window_list_visible
                    .then(|| self.window_list_entries()),
                window_list_query: ui.window_list_query,
                file_status: ui.file_status.as_deref(),
                active_outline_state: self
                    .workspace
                    .active_document_in(editor_id.unwrap())
                    .and_then(|d| self.outline_parsing.state_for(d)),
            })
        } else {
            iced::widget::container(iced::widget::text("")).into()
        };
        let element = if let Some(prompt) = editor_ui
            .as_ref()
            .and_then(|ui| ui.go_to_line_prompt.as_ref())
        {
            iced::widget::stack![
                ui::motion::fade(element, 1.0, ui::styles::editor_background, false),
                ui::go_to_line_prompt::view(
                    &prompt.input,
                    prompt.error.as_deref(),
                    prompt.animation.progress(),
                    prompt.animation.target_visible()
                ),
            ]
            .into()
        } else {
            element
        };
        if let Some(span) = perf_span {
            span.end_with("");
        }

        let element = if ui::title_bar::SUPPORTED {
            ui::title_bar::frame(
                element,
                window_id,
                self.title(window_id),
                self.title_bar_style,
                self.focused_window_id == Some(window_id),
                self.maximized_windows
                    .get(&window_id)
                    .copied()
                    .unwrap_or(false),
            )
        } else {
            element
        };
        element.map(move |message| Message::ForWindow(window_id, Box::new(message)))
    }

    pub fn title(&self, window_id: window::Id) -> String {
        let windows: [Option<&dyn ManagedWindow>; 2] = [
            self.settings_window.as_ref().map(|window| window as _),
            self.advanced_search_window
                .as_ref()
                .map(|window| window as _),
        ];

        windows
            .into_iter()
            .flatten()
            .find(|window| window.is(window_id))
            .map_or_else(
                || {
                    self.editor_id(window_id)
                        .and_then(|id| self.workspace.active_document_in(id))
                        .map(|document| format!("{} - Fragile Notepad", document.title()))
                        .unwrap_or_else(|| "Fragile Notepad".into())
                },
                Title::title,
            )
    }

    pub fn theme(&self, _window_id: window::Id) -> Option<Theme> {
        ui::styles::modern_theme(self.settings.appearance)
    }

    #[cfg(test)]
    pub(super) fn chrome_animation_info(&self) -> ui::view_model::ChromeAnimationInfo {
        Self::animation_info(self.chrome_animation, &self.close_prompt)
    }

    fn animation_info(
        animation: super::animation::ChromeAnimation,
        close_prompt: &super::close_prompt::ClosePrompt,
    ) -> ui::view_model::ChromeAnimationInfo {
        let find = animation.find;
        let inline_replace = animation.inline_replace;
        let function_list = animation.function_list;
        let about = animation.about;

        ui::view_model::ChromeAnimationInfo {
            find_rendered_visible: find.rendered_visible(),
            find_progress: find.progress(),
            inline_replace_rendered_visible: inline_replace.rendered_visible(),
            inline_replace_progress: inline_replace.progress(),
            function_list_rendered_visible: function_list.rendered_visible(),
            function_list_progress: function_list.progress(),
            about_rendered_visible: about.rendered_visible(),
            about_progress: about.progress(),
            about_interactive: about.target_visible(),
            dirty_close_progress: close_prompt.progress(),
            dirty_close_interactive: close_prompt.interactive(),
        }
    }
}
