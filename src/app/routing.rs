//! Exhaustive routing from application messages to feature handlers.

use super::App;
use crate::editor::EditorAction;
use crate::message::{ApplicationMessage, Message, RoutedMessage};
use iced::Task;

impl App {
    pub(super) fn dispatch(&mut self, message: Message) -> Task<Message> {
        match message.route() {
            RoutedMessage::Settings(message) => self.update_settings(message),
            RoutedMessage::Files(message) => self.update_file(message),
            RoutedMessage::Search(message) => self.update_search(message),
            RoutedMessage::Window(message) => self.update_window(message),
            RoutedMessage::Menu(message) => {
                self.menu.update(message);
                Task::none()
            }
            RoutedMessage::GoToLine(message) => self.update_go_to_line(message),
            RoutedMessage::History(message) => self.update_editor_command(message),
            RoutedMessage::Application(message) => self.dispatch_application(message),
        }
    }

    fn dispatch_application(&mut self, message: ApplicationMessage) -> Task<Message> {
        match message {
            #[cfg(debug_assertions)]
            ApplicationMessage::ToggleTitleBarStyle => self.toggle_title_bar_style(),
            ApplicationMessage::SyntaxParsed(id, result) => self.complete_syntax_parse(id, result),
            ApplicationMessage::ForwardedFiles(paths, request, receipt) => {
                self.forward_files(paths, request, receipt)
            }
            ApplicationMessage::OpenPaths(paths) => self.open_paths(paths),
            ApplicationMessage::StartupReady => self.startup_ready(),
            ApplicationMessage::StartupFrameReady => {
                crate::startup::report_first_frame_ready();
                Task::none()
            }
            ApplicationMessage::SessionLoaded(result) => self.session_loaded(result),
            ApplicationMessage::SessionFlush => self.flush_session(),
            ApplicationMessage::SessionPersisted(result) => self.session_persisted(result),
            ApplicationMessage::ShutdownPersisted(result) => self.shutdown_persisted(result),
            ApplicationMessage::SettingsFlush => self.flush_settings(),
            ApplicationMessage::RefreshLoadingFind => self.refresh_loading_find(),
            ApplicationMessage::DocumentAnalyzed(result) => self.complete_document_analysis(result),
            ApplicationMessage::None => Task::none(),
            ApplicationMessage::SingleInstanceShowRequested(request) => {
                self.show_main_window(request)
            }
            ApplicationMessage::Shortcut(shortcut) => self.update_shortcut(shortcut),
            ApplicationMessage::RuntimeEvent(event, status, window_id) => {
                self.update_runtime_event(event, status, window_id)
            }
            ApplicationMessage::EditorAction(document_id, action) => {
                self.update_editor(document_id, action)
            }
            ApplicationMessage::OutlineParseCompleted(result) => {
                self.complete_outline_parse(result)
            }
            ApplicationMessage::ClipboardRead(request, result) => {
                self.update_clipboard_read(request, result)
            }
            ApplicationMessage::ClipboardWritten(_result) => Task::none(),
            ApplicationMessage::BackendBoostRequested => self.request_gpu_boost(),
            ApplicationMessage::BackendBoostConfigured(result) => self.complete_gpu_boost(result),
            ApplicationMessage::ChromeAnimationFrame(at) => self.update_chrome_animation_frame(at),
            ApplicationMessage::LanguageSelected(syntax_token) => {
                self.update_language(syntax_token)
            }
            ApplicationMessage::ToggleFunctionList => self.toggle_function_list(),
            ApplicationMessage::FunctionListQueryChanged(query) => {
                self.function_list_query = query;
                iced::widget::operation::snap_to(
                    crate::ui::function_list_panel::SCROLL_ID,
                    iced::widget::scrollable::RelativeOffset::START,
                )
            }
            ApplicationMessage::FunctionListEntrySelected(position) => {
                self.select_function_list_entry(position)
            }
            ApplicationMessage::AboutOpened => self.open_about_dialog(),
            ApplicationMessage::AboutTabSelected(tab) => self.select_about_tab(tab),
            ApplicationMessage::AboutClosed => self.close_about_dialog(),
            ApplicationMessage::WindowListOpened => self.set_window_list_visible(true),
            ApplicationMessage::WindowListClosed => self.set_window_list_visible(false),
            ApplicationMessage::WindowFocusRequested(target) => self.focus_window(target),
            ApplicationMessage::WindowFocusNext => self.focus_adjacent_window(1),
            ApplicationMessage::WindowFocusPrevious => self.focus_adjacent_window(-1),
            ApplicationMessage::FoldCurrent => {
                self.update_active_editor_command(EditorAction::FoldCurrent)
            }
            ApplicationMessage::UnfoldCurrent => {
                self.update_active_editor_command(EditorAction::UnfoldCurrent)
            }
            ApplicationMessage::ToggleCurrentFold => {
                self.update_active_editor_command(EditorAction::ToggleCurrentFold)
            }
            ApplicationMessage::FoldAll => self.update_active_editor_command(EditorAction::FoldAll),
            ApplicationMessage::UnfoldAll => {
                self.update_active_editor_command(EditorAction::UnfoldAll)
            }
            ApplicationMessage::GoToMatchingDelimiter => {
                self.update_active_editor_command(EditorAction::GoToMatchingDelimiter)
            }
            ApplicationMessage::SelectMatchingDelimiter => {
                self.update_active_editor_command(EditorAction::SelectMatchingDelimiter)
            }
            ApplicationMessage::NextFunction => {
                self.update_active_editor_command(EditorAction::NextFunction)
            }
            ApplicationMessage::PreviousFunction => {
                self.update_active_editor_command(EditorAction::PreviousFunction)
            }
            ApplicationMessage::SelectCurrentFunction => {
                self.update_active_editor_command(EditorAction::SelectCurrentFunction)
            }
            ApplicationMessage::SelectCurrentFunctionBody => {
                self.update_active_editor_command(EditorAction::SelectCurrentFunctionBody)
            }
            ApplicationMessage::Uppercase => {
                self.update_active_editor_command(EditorAction::Uppercase)
            }
            ApplicationMessage::Lowercase => {
                self.update_active_editor_command(EditorAction::Lowercase)
            }
            ApplicationMessage::TrimTrailingSpaces => {
                self.update_active_editor_command(EditorAction::TrimTrailingSpaces)
            }
            ApplicationMessage::JoinLines => {
                self.update_active_editor_command(EditorAction::JoinLines)
            }
            ApplicationMessage::Cut => self.update_active_editor_command(EditorAction::Cut),
            ApplicationMessage::Copy => self.update_active_editor_command(EditorAction::Copy),
            ApplicationMessage::Paste => self.update_active_editor_command(EditorAction::Paste),
            ApplicationMessage::Delete => self.update_active_editor_command(EditorAction::Delete),
        }
    }
}
