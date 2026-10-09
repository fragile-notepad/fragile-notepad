use crate::core::{EditorSettings, ShortcutCommand, ShortcutConflict, ShortcutGroup};
use crate::message::SettingsCategory;
use std::time::{Duration, Instant};

const NOTICE_TRANSITION_DURATION: Duration = Duration::from_millis(140);
const LISTENING_PULSE_PERIOD: f32 = 1.2;

#[derive(Debug, Clone)]
pub struct SettingsDialogState {
    pub draft: EditorSettings,
    pub system_dark: bool,
    pub wrap_column_input: String,
    pub recent_file_limit_input: String,
    pub result_limit_input: String,
    pub preview_chars_input: String,
    pub context_before_input: String,
    pub category: SettingsCategory,
    pub shortcut_group: ShortcutGroup,
    pub capturing_shortcut: Option<ShortcutCommand>,
    pub shortcut_conflict: Option<ShortcutConflict>,
    pub shortcut_notice_animation: ShortcutNoticeAnimation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutNoticeKind {
    None,
    Listening(ShortcutCommand),
    Conflict(ShortcutConflict),
}

#[derive(Debug, Clone, Copy)]
pub struct ShortcutNoticeAnimation {
    rendered: ShortcutNoticeKind,
    target: ShortcutNoticeKind,
    started_at: Option<Instant>,
    opacity_from: f32,
    opacity: f32,
    pulse_started_at: Option<Instant>,
    pulse: f32,
}

impl Default for ShortcutNoticeAnimation {
    fn default() -> Self {
        Self {
            rendered: ShortcutNoticeKind::None,
            target: ShortcutNoticeKind::None,
            started_at: None,
            opacity_from: 0.0,
            opacity: 0.0,
            pulse_started_at: None,
            pulse: 0.0,
        }
    }
}

impl ShortcutNoticeAnimation {
    pub fn sync(&mut self, capturing: Option<ShortcutCommand>, conflict: Option<ShortcutConflict>) {
        let target = capturing.map_or_else(
            || conflict.map_or(ShortcutNoticeKind::None, ShortcutNoticeKind::Conflict),
            ShortcutNoticeKind::Listening,
        );
        if target == self.target {
            return;
        }

        let was_visible = self.rendered != ShortcutNoticeKind::None;
        self.target = target;
        self.opacity_from = if target == ShortcutNoticeKind::None || !was_visible {
            self.opacity
        } else {
            0.0
        };
        self.opacity = self.opacity_from;
        self.started_at = Some(Instant::now());
        self.pulse_started_at =
            matches!(target, ShortcutNoticeKind::Listening(_)).then_some(Instant::now());
        if target != ShortcutNoticeKind::None {
            self.rendered = target;
        }
    }

    pub fn needs_frames(self) -> bool {
        self.started_at.is_some() || matches!(self.target, ShortcutNoticeKind::Listening(_))
    }

    pub fn update(&mut self, at: Instant) {
        if matches!(self.target, ShortcutNoticeKind::Listening(_)) {
            let started = *self.pulse_started_at.get_or_insert(at);
            let seconds = at.saturating_duration_since(started).as_secs_f32();
            self.pulse =
                0.5 + 0.5 * (seconds * std::f32::consts::TAU / LISTENING_PULSE_PERIOD).sin();
        } else {
            self.pulse = 0.0;
            self.pulse_started_at = None;
        }

        let Some(started) = self.started_at else {
            return;
        };
        let elapsed = at.saturating_duration_since(started);
        let raw = (elapsed.as_secs_f32() / NOTICE_TRANSITION_DURATION.as_secs_f32()).min(1.0);
        let target = f32::from(self.target != ShortcutNoticeKind::None);
        self.opacity = self.opacity_from + ((target - self.opacity_from) * ease_in_out_cubic(raw));
        if raw >= 1.0 {
            self.opacity = target;
            self.started_at = None;
            self.opacity_from = target;
            if self.target == ShortcutNoticeKind::None {
                self.rendered = ShortcutNoticeKind::None;
            }
        }
    }

    pub fn rendered(self) -> ShortcutNoticeKind {
        self.rendered
    }

    pub fn pulse(self) -> f32 {
        self.pulse.clamp(0.0, 1.0)
    }

    pub fn opacity(self) -> f32 {
        self.opacity.clamp(0.0, 1.0)
    }
}

fn ease_in_out_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    if progress < 0.5 {
        4.0 * progress * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
    }
}

impl SettingsDialogState {
    pub fn new(settings: &EditorSettings) -> Self {
        Self {
            draft: settings.clone(),
            system_dark: false,
            wrap_column_input: settings
                .wrap_column_limit
                .unwrap_or(EditorSettings::DEFAULT_WRAP_COLUMN)
                .to_string(),
            recent_file_limit_input: settings.recent_file_limit.to_string(),
            result_limit_input: settings.search_results.result_limit.to_string(),
            preview_chars_input: settings.search_results.preview_chars.to_string(),
            context_before_input: settings.search_results.context_before.to_string(),
            category: SettingsCategory::General,
            shortcut_group: ShortcutGroup::File,
            capturing_shortcut: None,
            shortcut_conflict: None,
            shortcut_notice_animation: ShortcutNoticeAnimation::default(),
        }
    }

    pub(crate) fn reset_from(&mut self, settings: &EditorSettings) {
        self.draft = settings.clone();
        self.wrap_column_input = settings
            .wrap_column_limit
            .unwrap_or(EditorSettings::DEFAULT_WRAP_COLUMN)
            .to_string();
        self.recent_file_limit_input = settings.recent_file_limit.to_string();
        self.sync_search_result_inputs();
        self.category = SettingsCategory::General;
        self.shortcut_group = ShortcutGroup::File;
        self.capturing_shortcut = None;
        self.shortcut_conflict = None;
        self.shortcut_notice_animation = ShortcutNoticeAnimation::default();
    }

    pub(crate) fn apply_to(&self, settings: &mut EditorSettings) {
        let history = std::mem::take(&mut settings.open_history);
        *settings = self.draft.clone();
        settings.open_history = history;
        settings.set_recent_file_limit(self.draft.recent_file_limit);
    }

    pub fn validation_error(&self) -> Option<String> {
        if self
            .recent_file_limit_input
            .parse::<usize>()
            .ok()
            .filter(|value| {
                (EditorSettings::MIN_RECENT_FILE_LIMIT..=EditorSettings::MAX_RECENT_FILE_LIMIT)
                    .contains(value)
            })
            .is_none()
        {
            return Some(format!(
                "History limit must be between {} and {}.",
                EditorSettings::MIN_RECENT_FILE_LIMIT,
                EditorSettings::MAX_RECENT_FILE_LIMIT
            ));
        }
        if self.draft.wrap_column_limit.is_some()
            && self
                .wrap_column_input
                .parse::<usize>()
                .ok()
                .filter(|value| EditorSettings::valid_wrap_column(*value))
                .is_none()
        {
            return Some(format!(
                "Wrap width must be between {} and {} columns.",
                EditorSettings::MIN_WRAP_COLUMN,
                EditorSettings::MAX_WRAP_COLUMN
            ));
        }
        self.parsed_search_results().err().map(str::to_owned)
    }

    pub(crate) fn sync_search_result_inputs(&mut self) {
        self.result_limit_input = self.draft.search_results.result_limit.to_string();
        self.preview_chars_input = self.draft.search_results.preview_chars.to_string();
        self.context_before_input = self.draft.search_results.context_before.to_string();
    }

    pub(crate) fn update_search_results_from_inputs(&mut self) {
        if let Ok(settings) = self.parsed_search_results() {
            self.draft.search_results = settings;
        }
    }

    fn parsed_search_results(&self) -> Result<crate::core::SearchResultSettings, &'static str> {
        crate::search_dialog::parse_result_settings(
            &self.result_limit_input,
            &self.preview_chars_input,
            &self.context_before_input,
        )
    }

    pub(crate) fn sync_shortcut_notice_animation(&mut self) {
        self.shortcut_notice_animation
            .sync(self.capturing_shortcut, self.shortcut_conflict);
    }

    pub(crate) fn update_shortcut_notice_animation(&mut self, at: Instant) {
        self.shortcut_notice_animation.update(at);
    }

    pub(crate) fn shortcut_notice_needs_frames(&self) -> bool {
        self.shortcut_notice_animation.needs_frames()
    }
}
