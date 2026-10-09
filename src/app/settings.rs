use crate::message::SettingsMessage;
use iced::Task;

use super::App;
use crate::core::EditorSettings;
use crate::message::Message;
use crate::services::settings_store;

#[derive(Clone, Copy)]
#[repr(u32)]
enum SettingsEdit {
    Zoom = 1,
    WordWrap = 2,
    LineNumbers = 4,
    Spaces = 8,
    Tabs = 16,
    EolMarkers = 32,
    IndentationGuides = 64,
    FoldingControls = 128,
    SearchResults = 512,
    SpacesAndTabs = 8 | 16,
    Characters = 8 | 16 | 32,
    All = u32::MAX,
}

#[derive(Debug, Default)]
pub(super) struct SettingsPersistence {
    loaded: bool,
    read_failed: bool,
    initial_edits: u32,
    dirty: bool,
    flush_scheduled: bool,
    changed: bool,
}

impl SettingsPersistence {
    pub(super) fn is_loaded(&self) -> bool {
        self.loaded
    }
    pub(super) fn read_failed(&self) -> bool {
        self.read_failed
    }

    #[cfg(test)]
    pub(super) fn has_early_edits(&self) -> bool {
        self.initial_edits != 0
    }
    #[cfg(test)]
    pub(super) fn flush_scheduled(&self) -> bool {
        self.flush_scheduled
    }

    fn edit(
        &mut self,
        settings: &mut EditorSettings,
        edit: SettingsEdit,
        apply: impl FnOnce(&mut EditorSettings),
    ) {
        // Frequent zoom/display edits compare Copy fields. Only applying the full
        // dialog needs to compare shortcut maps and path history.
        let before = (
            settings.zoom,
            settings.word_wrap,
            settings.auto_save,
            settings.decorations,
            settings.search_results,
        );
        let before_all = matches!(edit, SettingsEdit::All).then(|| settings.clone());
        apply(settings);
        self.changed |= before_all.map_or_else(
            || {
                before
                    != (
                        settings.zoom,
                        settings.word_wrap,
                        settings.auto_save,
                        settings.decorations,
                        settings.search_results,
                    )
            },
            |before| before != *settings,
        );
        if !self.loaded {
            self.initial_edits |= edit as u32;
        }
    }

    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn request_save(&mut self) -> bool {
        self.dirty = true;
        if !self.loaded || self.read_failed || self.flush_scheduled {
            return false;
        }
        self.flush_scheduled = true;
        true
    }

    fn begin_flush(&mut self) -> bool {
        self.flush_scheduled = false;
        if !self.dirty || !self.loaded || self.read_failed {
            return false;
        }
        self.dirty = false;
        true
    }
}

impl App {
    pub(super) fn update_settings(&mut self, message: SettingsMessage) -> Task<Message> {
        let task = self.dispatch_settings(message);
        self.settings_dialog.sync_shortcut_notice_animation();
        task
    }

    fn dispatch_settings(&mut self, message: SettingsMessage) -> Task<Message> {
        match message {
            SettingsMessage::DraftThemeSelected(theme) => {
                self.settings_dialog.draft.set_syntax_theme(theme);
                Task::none()
            }
            SettingsMessage::SystemColorModeChanged(mode) => {
                let dark = mode == iced::theme::Mode::Dark;
                if self.system_dark != dark {
                    self.system_dark = dark;
                    self.settings_dialog.system_dark = dark;
                    self.pending_work.request(super::events::Work::Syntax);
                }
                Task::none()
            }
            SettingsMessage::DraftWordWrapToggled(word_wrap) => {
                self.settings_dialog.draft.set_word_wrap(word_wrap);
                Task::none()
            }
            SettingsMessage::DraftFixedWrapSelected(fixed) => {
                self.settings_dialog.draft.wrap_column_limit = if fixed {
                    Some(
                        self.settings_dialog
                            .wrap_column_input
                            .parse::<usize>()
                            .ok()
                            .filter(|value| EditorSettings::valid_wrap_column(*value))
                            .unwrap_or(EditorSettings::DEFAULT_WRAP_COLUMN),
                    )
                } else {
                    None
                };
                Task::none()
            }
            SettingsMessage::DraftWrapColumnChanged(value) => {
                if let Ok(columns) = value.parse::<usize>()
                    && EditorSettings::valid_wrap_column(columns)
                {
                    self.settings_dialog.draft.wrap_column_limit = Some(columns);
                }
                self.settings_dialog.wrap_column_input = value;
                Task::none()
            }
            SettingsMessage::DraftWrapColumnPreset(columns) => {
                self.settings_dialog.wrap_column_input = columns.to_string();
                self.settings_dialog.draft.wrap_column_limit = Some(columns);
                Task::none()
            }
            SettingsMessage::DraftAutoSaveToggled(auto_save) => {
                self.settings_dialog.draft.set_auto_save(auto_save);
                Task::none()
            }
            SettingsMessage::DraftNewFileEncodingSelected(encoding) => {
                self.settings_dialog.draft.new_file_encoding = encoding;
                Task::none()
            }
            SettingsMessage::DraftNewFileLineEndingSelected(line_ending) => {
                self.settings_dialog.draft.new_file_line_ending =
                    if line_ending == iced::widget::text_editor::LineEnding::None {
                        EditorSettings::DEFAULT_NEW_FILE_LINE_ENDING
                    } else {
                        line_ending
                    };
                Task::none()
            }
            SettingsMessage::DraftRecentFileLimitChanged(value) => {
                if let Ok(limit) = value.parse::<usize>()
                    && (EditorSettings::MIN_RECENT_FILE_LIMIT
                        ..=EditorSettings::MAX_RECENT_FILE_LIMIT)
                        .contains(&limit)
                {
                    self.settings_dialog.draft.set_recent_file_limit(limit);
                }
                self.settings_dialog.recent_file_limit_input = value;
                Task::none()
            }
            SettingsMessage::DraftSearchResultLimitChanged(value) => {
                self.settings_dialog.result_limit_input = value;
                self.settings_dialog.update_search_results_from_inputs();
                Task::none()
            }
            SettingsMessage::DraftSearchPreviewCharsChanged(value) => {
                self.settings_dialog.preview_chars_input = value;
                self.settings_dialog.update_search_results_from_inputs();
                Task::none()
            }
            SettingsMessage::DraftSearchContextBeforeChanged(value) => {
                self.settings_dialog.context_before_input = value;
                self.settings_dialog.update_search_results_from_inputs();
                Task::none()
            }
            SettingsMessage::DraftSearchResultsReset => {
                self.settings_dialog.draft.search_results =
                    crate::core::SearchResultSettings::default();
                self.settings_dialog.sync_search_result_inputs();
                Task::none()
            }
            SettingsMessage::DraftAppearanceSelected(appearance) => {
                self.settings_dialog.draft.set_appearance(appearance);
                Task::none()
            }
            SettingsMessage::DraftHardwareAccelerationSelected(mode) => {
                self.settings_dialog.draft.set_hardware_acceleration(mode);
                Task::none()
            }
            SettingsMessage::DraftIndentationSelected(indentation) => {
                self.settings_dialog.draft.set_indentation(indentation);
                Task::none()
            }
            SettingsMessage::DraftLineNumbersToggled(show_line_numbers) => {
                self.settings_dialog
                    .draft
                    .set_show_line_numbers(show_line_numbers);
                Task::none()
            }
            SettingsMessage::DraftVisibleSpacesToggled(show_spaces) => {
                self.settings_dialog.draft.set_show_spaces(show_spaces);
                Task::none()
            }
            SettingsMessage::DraftVisibleTabsToggled(show_tabs) => {
                self.settings_dialog.draft.set_show_tabs(show_tabs);
                Task::none()
            }
            SettingsMessage::DraftEolMarkersToggled(show_end_of_line_markers) => {
                self.settings_dialog
                    .draft
                    .set_show_end_of_line_markers(show_end_of_line_markers);
                Task::none()
            }
            SettingsMessage::DraftIndentationGuidesToggled(show_indentation_guides) => {
                self.settings_dialog
                    .draft
                    .set_show_indentation_guides(show_indentation_guides);
                Task::none()
            }
            SettingsMessage::DraftFoldingControlsToggled(show_folding_controls) => {
                self.settings_dialog
                    .draft
                    .set_show_folding_controls(show_folding_controls);
                Task::none()
            }
            SettingsMessage::DraftWrapIndicatorToggled(show) => {
                self.settings_dialog.draft.decorations.show_wrap_indicator = show;
                Task::none()
            }
            SettingsMessage::DraftWrapGuideToggled(show) => {
                self.settings_dialog.draft.decorations.show_wrap_guide = show;
                Task::none()
            }
            SettingsMessage::SettingsCategorySelected(category) => {
                self.settings_dialog.category = category;
                self.settings_dialog.capturing_shortcut = None;
                self.settings_dialog.shortcut_conflict = None;
                Task::none()
            }
            SettingsMessage::ShortcutGroupSelected(group) => {
                self.settings_dialog.shortcut_group = group;
                self.settings_dialog.capturing_shortcut = None;
                self.settings_dialog.shortcut_conflict = None;
                Task::none()
            }
            SettingsMessage::SettingsZoomIn => {
                self.settings_dialog.draft.zoom_in();
                Task::none()
            }
            SettingsMessage::SettingsZoomOut => {
                self.settings_dialog.draft.zoom_out();
                Task::none()
            }
            SettingsMessage::SettingsZoomReset => {
                self.settings_dialog.draft.reset_zoom();
                Task::none()
            }
            SettingsMessage::SettingsScrollSpeedIncrease => {
                self.settings_dialog.draft.increase_scroll_speed();
                Task::none()
            }
            SettingsMessage::SettingsScrollSpeedDecrease => {
                self.settings_dialog.draft.decrease_scroll_speed();
                Task::none()
            }
            SettingsMessage::SettingsScrollSpeedReset => {
                self.settings_dialog.draft.reset_scroll_speed();
                Task::none()
            }
            SettingsMessage::ApplySettings => {
                if self.settings_dialog.validation_error().is_some() {
                    return Task::none();
                }
                let (request_boost, search_task) = self.apply_settings_dialog();
                let boost_task = if request_boost {
                    self.request_gpu_boost()
                } else {
                    Task::none()
                };
                Task::batch([search_task, boost_task])
            }
            SettingsMessage::SaveSettings => {
                if self.settings_dialog.validation_error().is_some() {
                    return Task::none();
                }
                let (request_boost, search_task) = self.apply_settings_dialog();
                let boost_task = if request_boost {
                    self.request_gpu_boost()
                } else {
                    Task::none()
                };

                Task::batch([
                    self.persist_settings(),
                    self.close_settings_window(),
                    search_task,
                    boost_task,
                ])
            }
            SettingsMessage::SettingsLoaded(result) => {
                self.settings_persistence.loaded = true;
                if let Err(error) = &result {
                    self.settings_persistence.read_failed = true;
                    self.file_status = Some(format!(
                        "Settings could not be read: {}. The existing settings file will be preserved.",
                        error.summary()
                    ));
                }
                let mut tasks = Vec::new();
                if let Ok(Some(settings)) = result {
                    self.settings = merge_initial_settings(
                        &self.settings,
                        settings,
                        self.settings_persistence.initial_edits,
                    );
                    self.apply_initial_new_file_defaults();
                    self.settings_persistence.changed = true;
                    self.settings_dialog.reset_from(&self.settings);
                    self.search_dialog
                        .set_result_settings(self.settings.search_results);

                    if super::rendering::startup_gpu_boost_requested(&self.settings) {
                        if self.lifecycle.main_window_opened {
                            tasks.push(self.request_gpu_boost());
                        } else {
                            self.lifecycle.pending_startup_gpu_boost = true;
                        }
                    }
                }

                tasks.push(self.restore_startup());
                if self.settings_persistence.dirty {
                    tasks.push(self.persist_settings());
                }
                Task::batch(tasks)
            }
            SettingsMessage::SettingsPersisted(result) => {
                if let Err(error) = result {
                    self.settings_persistence.dirty = true;
                    self.file_status = Some(format!("Settings save failed: {}", error.summary()));
                }

                Task::none()
            }
            SettingsMessage::CancelSettings => self.cancel_settings_dialog(),
            SettingsMessage::ToggleSettingsPanel => self.toggle_settings_window(),
            SettingsMessage::ShortcutCaptureStarted(command) => {
                self.settings_dialog.capturing_shortcut = Some(command);
                self.settings_dialog.shortcut_conflict = None;
                Task::none()
            }
            SettingsMessage::ShortcutCaptured(command, binding) => {
                self.settings_dialog.capturing_shortcut = None;
                match self
                    .settings_dialog
                    .draft
                    .shortcuts
                    .set_binding(command, binding)
                {
                    Ok(()) => {
                        self.settings_dialog.shortcut_conflict = None;
                    }
                    Err(conflict) => {
                        self.settings_dialog.shortcut_conflict = Some(conflict);
                    }
                }
                Task::none()
            }
            SettingsMessage::ShortcutCleared(command) => {
                self.settings_dialog.capturing_shortcut = None;
                self.settings_dialog.shortcut_conflict = None;
                self.settings_dialog.draft.shortcuts.clear(command);
                Task::none()
            }
            SettingsMessage::ShortcutsResetToDefaults => {
                self.settings_dialog.capturing_shortcut = None;
                self.settings_dialog.shortcut_conflict = None;
                self.settings_dialog.draft.shortcuts.reset_to_defaults();
                Task::none()
            }
            SettingsMessage::ShortcutConflictDismissed => {
                self.settings_dialog.shortcut_conflict = None;
                Task::none()
            }
            SettingsMessage::ShortcutCaptureConflict(conflict) => {
                self.settings_dialog.capturing_shortcut = None;
                self.settings_dialog.shortcut_conflict = Some(conflict);
                Task::none()
            }
            SettingsMessage::ZoomIn => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::Zoom,
                    |settings| {
                        settings.zoom_in();
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ZoomOut => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::Zoom,
                    |settings| {
                        settings.zoom_out();
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ZoomReset => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::Zoom,
                    |settings| {
                        settings.reset_zoom();
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleWordWrap => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::WordWrap,
                    |settings| {
                        settings.set_word_wrap(!settings.word_wrap);
                    },
                );
                Task::batch([
                    self.persist_settings(),
                    iced::widget::operation::focus(crate::ui::editor::EDITOR_ID),
                ])
            }
            SettingsMessage::ToggleLineNumbers => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::LineNumbers,
                    |settings| {
                        settings.set_show_line_numbers(!settings.decorations.show_line_numbers);
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleSpaceAndTab => {
                self.menu.close();
                let show_space_and_tab =
                    !(self.settings.decorations.show_spaces && self.settings.decorations.show_tabs);
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::SpacesAndTabs,
                    |settings| {
                        settings.set_show_spaces(show_space_and_tab);
                        settings.set_show_tabs(show_space_and_tab);
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleVisibleSpaces => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::Spaces,
                    |settings| {
                        settings.set_show_spaces(!settings.decorations.show_spaces);
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleVisibleTabs => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::Tabs,
                    |settings| {
                        settings.set_show_tabs(!settings.decorations.show_tabs);
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleEolMarkers => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::EolMarkers,
                    |settings| {
                        settings.set_show_end_of_line_markers(
                            !settings.decorations.show_end_of_line_markers,
                        );
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleAllCharacters => {
                self.menu.close();
                let show_all = !(self.settings.decorations.show_spaces
                    && self.settings.decorations.show_tabs
                    && self.settings.decorations.show_end_of_line_markers);
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::Characters,
                    |settings| {
                        settings.set_show_spaces(show_all);
                        settings.set_show_tabs(show_all);
                        settings.set_show_end_of_line_markers(show_all);
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleIndentationGuides => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::IndentationGuides,
                    |settings| {
                        settings.set_show_indentation_guides(
                            !settings.decorations.show_indentation_guides,
                        );
                    },
                );
                self.persist_settings()
            }
            SettingsMessage::ToggleFoldingControls => {
                self.menu.close();
                self.settings_persistence.edit(
                    &mut self.settings,
                    SettingsEdit::FoldingControls,
                    |settings| {
                        settings
                            .set_show_folding_controls(!settings.decorations.show_folding_controls);
                    },
                );
                self.persist_settings()
            }
        }
    }

    fn apply_settings_dialog(&mut self) -> (bool, Task<Message>) {
        let old_hardware_acceleration = self.settings.hardware_acceleration;
        let old_search_results = self.settings.search_results;
        self.settings_persistence
            .edit(&mut self.settings, SettingsEdit::All, |settings| {
                self.settings_dialog.apply_to(settings);
            });

        let search_task = if self.settings.search_results != old_search_results {
            self.search_dialog
                .set_result_settings(self.settings.search_results);
            self.search_result_settings_changed()
        } else {
            Task::none()
        };
        (
            self.settings.hardware_acceleration != old_hardware_acceleration
                && super::rendering::startup_gpu_boost_requested(&self.settings),
            search_task,
        )
    }

    fn cancel_settings_dialog(&mut self) -> Task<Message> {
        self.settings_dialog.reset_from(&self.settings);
        self.close_settings_window()
    }

    pub(super) fn persist_settings(&mut self) -> Task<Message> {
        if !self.settings_persistence.request_save() {
            return Task::none();
        }
        Task::perform(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            },
            |_| Message::SettingsFlush,
        )
    }

    pub(super) fn set_search_result_settings(
        &mut self,
        limits: crate::core::SearchResultSettings,
    ) -> Task<Message> {
        self.settings_persistence.edit(
            &mut self.settings,
            SettingsEdit::SearchResults,
            |settings| {
                settings.search_results = limits.normalized();
            },
        );
        self.settings_dialog.draft.search_results = self.settings.search_results;
        self.settings_dialog.sync_search_result_inputs();
        self.persist_settings()
    }

    pub(super) fn flush_settings(&mut self) -> Task<Message> {
        if !self.settings_persistence.begin_flush() {
            return Task::none();
        }
        Task::perform(
            settings_store::save_settings(self.settings.clone()),
            Message::SettingsPersisted,
        )
    }
}

pub(super) fn merge_initial_settings(
    current: &EditorSettings,
    mut loaded: EditorSettings,
    edits: u32,
) -> EditorSettings {
    let defaults = EditorSettings::default();
    macro_rules! keep_edits { ($($field:ident),*) => { $(if current.$field != defaults.$field { loaded.$field = current.$field.clone(); })* }; }
    keep_edits!(
        word_wrap,
        wrap_column_limit,
        auto_save,
        new_file_encoding,
        new_file_line_ending,
        recent_file_limit,
        zoom,
        scroll_speed,
        indentation,
        appearance,
        hardware_acceleration,
        syntax_theme,
        shortcuts,
        search_results
    );
    if edits & 1 != 0 {
        loaded.zoom = current.zoom;
    }
    if edits & 2 != 0 {
        loaded.word_wrap = current.word_wrap;
    }
    if edits & 256 != 0 {
        loaded.auto_save = current.auto_save;
    }
    if edits & 512 != 0 {
        loaded.search_results = current.search_results;
    }
    macro_rules! decoration_edit {
        ($field:ident) => {
            if current.decorations.$field != defaults.decorations.$field {
                loaded.decorations.$field = current.decorations.$field;
            }
        };
        ($field:ident, $mask:expr) => {
            if edits & $mask != 0 || current.decorations.$field != defaults.decorations.$field {
                loaded.decorations.$field = current.decorations.$field;
            }
        };
    }
    decoration_edit!(show_line_numbers, 4);
    decoration_edit!(show_spaces, 8);
    decoration_edit!(show_tabs, 16);
    decoration_edit!(show_end_of_line_markers, 32);
    decoration_edit!(show_indentation_guides, 64);
    decoration_edit!(show_folding_controls, 128);
    decoration_edit!(show_wrap_indicator);
    decoration_edit!(show_wrap_guide);
    if edits == u32::MAX {
        let history = loaded.open_history;
        loaded = current.clone();
        loaded.open_history = history;
    }
    loaded.set_recent_file_limit(loaded.recent_file_limit);
    for path in current.open_history.iter().rev() {
        loaded.record_open_history_path(path.clone());
    }
    loaded
}

pub(super) fn apply_to_workspace(
    event: super::events::Event,
    settings: &EditorSettings,
    workspace: &mut crate::core::Workspace,
) {
    use super::events::Event;
    use crate::core::workspace::changes::WorkspaceEvent;
    let apply = |document: &mut crate::core::Document| {
        document.set_decoration_settings(settings.decoration_settings());
        document.set_wrap_column_limit(settings.wrap_column_limit);
        document.set_word_wrap(settings.word_wrap);
    };
    match event {
        Event::SettingsChanged => {
            workspace.edit_documents(apply);
        }
        Event::Workspace(WorkspaceEvent::DocumentOpened(id)) => {
            if let Some(document) = workspace.document_mut(id) {
                apply_new_file_defaults(document, settings);
                apply(document);
            }
        }
        _ => {}
    }
}

fn apply_new_file_defaults(document: &mut crate::core::Document, settings: &EditorSettings) {
    if document.path.is_none()
        && document.load_state == crate::core::DocumentLoadState::Complete
        && document.buffer.len_bytes() == 0
        && !document.is_dirty
    {
        // Initial save properties do not make a new empty document dirty.
        document.encoding = settings.new_file_encoding;
        document.line_ending = Some(match settings.new_file_line_ending {
            iced::widget::text_editor::LineEnding::None => {
                EditorSettings::DEFAULT_NEW_FILE_LINE_ENDING
            }
            ending => ending,
        });
    }
}

impl App {
    pub(super) fn apply_initial_new_file_defaults(&mut self) {
        // Settings arrive after the first scratch document is opened. Keep any
        // user edits and recovery metadata while updating that initial placeholder.
        if self.workspace.documents().len() == 1
            && let Some(document) = self.workspace.document_mut(crate::core::DocumentId::new(1))
        {
            apply_new_file_defaults(document, &self.settings);
        }
    }
}
