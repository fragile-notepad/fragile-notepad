use super::test_support::*;
use iced::backend;
use iced::event::{Event, Status};
use iced::mouse;

#[test]
fn custom_caption_close_preserves_dirty_document_and_settings_cancel_behavior() {
    use crate::ui::title_bar::Action;
    let (mut app, _) = App::new();
    let main = app.main_window_id.unwrap();
    let document = app.workspace.active_document_id();
    app.workspace.active_document_mut().unwrap().mark_dirty();
    let _ = app.update(Message::WindowChrome(main, Action::Close));
    assert_eq!(app.close_prompt.document(), Some(document));
    assert!(app.workspace.document(document).is_some());
    let _ = app.update(Message::DirtyCloseResolved(
        document,
        DirtyCloseDecision::Cancel,
    ));
    assert_eq!(app.files.close_goal(), CloseGoal::KeepOpen);

    let _ = app.update(Message::ToggleSettingsPanel);
    let settings = app.settings_window.unwrap().id();
    let saved_wrap = app.settings.word_wrap;
    let _ = app.update(Message::DraftWordWrapToggled(!saved_wrap));
    let _ = app.update(Message::WindowChrome(settings, Action::Close));
    assert_eq!(app.settings_dialog.draft.word_wrap, saved_wrap);
    assert_eq!(app.files.close_goal(), CloseGoal::KeepOpen);
    assert!(app.workspace.document(document).is_some());
}

#[test]
fn caption_state_follows_native_focus_and_rejects_closed_window_results() {
    let (mut app, _) = App::new();
    let main = app.main_window_id.unwrap();
    let _ = app.update(Message::ToggleSettingsPanel);
    let settings = app.settings_window.unwrap().id();
    let _ = app.update(Message::RuntimeEvent(
        Event::Window(iced::window::Event::Focused),
        Status::Captured,
        settings,
    ));
    let _ = app.update(Message::RuntimeEvent(
        Event::Window(iced::window::Event::Unfocused),
        Status::Captured,
        main,
    ));
    assert_eq!(app.focused_window_id, Some(settings));
    let _ = app.update(Message::WindowMaximized(settings, true));
    assert_eq!(app.maximized_windows.get(&settings), Some(&true));
    let _ = app.update(Message::WindowClosed(settings));
    let _ = app.update(Message::WindowMaximized(settings, true));
    assert!(!app.maximized_windows.contains_key(&settings));
    assert_eq!(app.focused_window_id, None);
}

#[test]
fn auto_save_on_main_window_unfocus_saves_the_active_named_document() {
    let (mut app, _) = App::new();
    app.settings.auto_save = true;
    let main = app.main_window_id.unwrap();
    let document_id = app.workspace.active_document_id();
    let path = PathBuf::from("unfocused-auto-save.txt");
    {
        let document = app.workspace.document_mut(document_id).unwrap();
        document.set_path(path);
        document.mark_dirty();
    }

    let _ = app.update(Message::RuntimeEvent(
        Event::Window(iced::window::Event::Unfocused),
        Status::Captured,
        main,
    ));

    assert_eq!(
        app.files.pending_save().map(|request| request.document_id),
        Some(document_id)
    );
}

#[test]
#[cfg(debug_assertions)]
fn caption_preview_toggle_does_not_modify_settings_or_recovery() {
    let (mut app, _) = App::new();
    let settings = app.settings.clone();
    let style = app.title_bar_style;
    app.session.set_enabled(true);
    let _ = app.update(Message::WindowMaximized(app.main_window_id.unwrap(), true));
    assert!(!app.session.is_dirty());
    let _ = app.update(Message::ToggleTitleBarStyle);
    assert_ne!(app.title_bar_style, style);
    assert_eq!(app.settings, settings);
    assert!(!app.session.is_dirty());
    let _ = app.update(Message::ToggleTitleBarStyle);
    assert_eq!(app.title_bar_style, style);
}

fn strict_handoff_result(
    completed_phase: backend::StrictHandoffPhase,
    rollback: backend::StrictRollbackStatus,
) -> backend::StrictHandoffOutcome {
    Ok(backend::StrictHandoffResult {
        completed_phase,
        rollback,
        windows: Vec::new(),
    })
}

fn strict_handoff_error(
    category: backend::StrictHandoffFailureCategory,
) -> backend::StrictHandoffOutcome {
    Err(backend::StrictHandoffError {
        phase: match category {
            backend::StrictHandoffFailureCategory::WarmUp => backend::StrictHandoffPhase::Warming,
            backend::StrictHandoffFailureCategory::Commit => {
                backend::StrictHandoffPhase::CommitPending
            }
            backend::StrictHandoffFailureCategory::FirstPresent
            | backend::StrictHandoffFailureCategory::RendererEvidenceMissing => {
                backend::StrictHandoffPhase::AwaitingFirstPresent
            }
            _ => backend::StrictHandoffPhase::Preparing,
        },
        category,
        rollback: backend::StrictRollbackStatus::Restored,
        windows: Vec::new(),
        message: format!("{category:?} injected failure"),
    })
}

fn strict_handoff_success() -> backend::StrictHandoffOutcome {
    strict_handoff_result(
        backend::StrictHandoffPhase::Completed,
        backend::StrictRollbackStatus::ReleasedAfterSuccess,
    )
}

#[test]
fn runtime_left_button_release_clears_tab_drag() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    let _ = app.update(Message::TabDragStarted(document_id));
    assert_eq!(app.dragged_tab, Some(document_id));
    assert_eq!(app.hovered_drop_tab, Some(document_id));

    let window_id = app.main_window_id.expect("main window id");
    let _ = app.update(Message::RuntimeEvent(
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        Status::Ignored,
        window_id,
    ));

    assert_eq!(app.dragged_tab, None);
    assert_eq!(app.hovered_drop_tab, None);
}

fn render_backend_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct RenderBackendEnvGuard {
    previous: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

impl RenderBackendEnvGuard {
    fn new(value: Option<&str>) -> Self {
        let lock = render_backend_env_lock()
            .lock()
            .expect("render backend env lock should not be poisoned");
        let previous = std::env::var(crate::app::rendering::RENDER_BACKEND_ENV).ok();

        unsafe {
            match value {
                Some(value) => std::env::set_var(crate::app::rendering::RENDER_BACKEND_ENV, value),
                None => std::env::remove_var(crate::app::rendering::RENDER_BACKEND_ENV),
            }
        }

        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for RenderBackendEnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(crate::app::rendering::RENDER_BACKEND_ENV, value),
                None => std::env::remove_var(crate::app::rendering::RENDER_BACKEND_ENV),
            }
        }
    }
}

#[test]
fn default_startup_stays_software_first_until_loaded_settings_request_boost() {
    let (mut app, _) = App::new();
    let main_window = app
        .main_window_id
        .expect("main window should be tracked after boot");

    assert_eq!(
        app.settings.hardware_acceleration,
        HardwareAccelerationMode::Lazy
    );
    assert!(!app.lifecycle.pending_startup_gpu_boost);

    let task = app.update(Message::WindowOpened(main_window));

    assert_eq!(task.units(), 0);
    assert!(!app.lifecycle.pending_startup_gpu_boost);
    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::Software
    );
}

#[test]
fn secondary_window_open_does_not_consume_startup_gpu_boost() {
    let (mut app, _) = App::new();
    let secondary_window = iced::window::Id::unique();

    app.lifecycle.pending_startup_gpu_boost = true;
    let task = app.update(Message::WindowOpened(secondary_window));

    assert_eq!(task.units(), 0);
    assert!(app.lifecycle.pending_startup_gpu_boost);
    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::Software
    );
}

#[test]
fn main_window_open_consumes_startup_gpu_boost() {
    let _env = RenderBackendEnvGuard::new(None);
    let (mut app, _) = App::new();
    let main_window = app
        .main_window_id
        .expect("main window should be tracked after boot");

    app.settings.hardware_acceleration = HardwareAccelerationMode::Lazy;
    // Simulates an explicit startup boost queued after persisted settings load
    // or a diagnostic override, not the App::new default.
    app.lifecycle.pending_startup_gpu_boost = true;
    let _ = app.update(Message::WindowOpened(main_window));

    assert!(!app.lifecycle.pending_startup_gpu_boost);
    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::PreparingHardware
    );
}

#[test]
fn manual_gpu_boost_request_still_starts_immediately() {
    let _env = RenderBackendEnvGuard::new(None);
    let (mut app, _) = App::new();

    app.settings.hardware_acceleration = HardwareAccelerationMode::Lazy;
    let _ = app.update(Message::BackendBoostRequested);

    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::PreparingHardware
    );
}

#[test]
fn applying_lazy_rendering_setting_requests_runtime_gpu_boost() {
    let _env = RenderBackendEnvGuard::new(None);
    let (mut app, _) = App::new();

    app.settings.hardware_acceleration = HardwareAccelerationMode::Off;
    app.settings_dialog.reset_from(&app.settings);
    let _ = app.update(Message::DraftHardwareAccelerationSelected(
        HardwareAccelerationMode::Lazy,
    ));
    let _ = app.update(Message::ApplySettings);

    assert_eq!(
        app.settings.hardware_acceleration,
        HardwareAccelerationMode::Lazy
    );
    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::PreparingHardware
    );
}

#[test]
fn saving_diagnostic_rendering_setting_requests_runtime_gpu_boost() {
    let _env = RenderBackendEnvGuard::new(None);
    let (mut app, _) = App::new();

    let _ = app.update(Message::DraftHardwareAccelerationSelected(
        HardwareAccelerationMode::Diagnostic,
    ));
    let _ = app.update(Message::SaveSettings);

    assert_eq!(
        app.settings.hardware_acceleration,
        HardwareAccelerationMode::Diagnostic
    );
    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::PreparingHardware
    );
}

#[test]
fn software_rendering_backend_env_suppresses_applied_hardware_boost() {
    let _env = RenderBackendEnvGuard::new(Some("software"));
    let (mut app, _) = App::new();

    let _ = app.update(Message::DraftHardwareAccelerationSelected(
        HardwareAccelerationMode::Diagnostic,
    ));
    let _ = app.update(Message::ApplySettings);

    assert_eq!(
        app.settings.hardware_acceleration,
        HardwareAccelerationMode::Diagnostic
    );
    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::Software
    );
}

#[test]
fn strict_completed_released_handoff_enters_hardware_state() {
    let (mut app, _) = App::new();

    app.rendering = crate::app::rendering::RenderingState::PreparingHardware;
    let _ = app.update(Message::BackendBoostConfigured(strict_handoff_success()));

    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::Hardware
    );
}

#[test]
fn strict_ok_result_fails_closed_without_completed_released_success() {
    let cases = [
        (
            backend::StrictHandoffPhase::AwaitingFirstPresent,
            backend::StrictRollbackStatus::ReleasedAfterSuccess,
        ),
        (
            backend::StrictHandoffPhase::Completed,
            backend::StrictRollbackStatus::NotNeeded,
        ),
    ];

    for (completed_phase, rollback) in cases {
        let (mut app, _) = App::new();
        app.rendering = crate::app::rendering::RenderingState::PreparingHardware;

        let _ = app.update(Message::BackendBoostConfigured(strict_handoff_result(
            completed_phase,
            rollback,
        )));

        assert_eq!(
            app.rendering,
            crate::app::rendering::RenderingState::Failed(
                crate::app::rendering::RenderFailureCategory::Unknown
            ),
            "phase {completed_phase:?} rollback {rollback:?} should fail closed"
        );
    }
}

#[test]
fn strict_handoff_error_categories_map_to_render_failures() {
    let cases = [
        (
            backend::StrictHandoffFailureCategory::Prepare,
            crate::app::rendering::RenderFailureCategory::Prepare,
        ),
        (
            backend::StrictHandoffFailureCategory::WarmUp,
            crate::app::rendering::RenderFailureCategory::WarmUp,
        ),
        (
            backend::StrictHandoffFailureCategory::Commit,
            crate::app::rendering::RenderFailureCategory::Commit,
        ),
        (
            backend::StrictHandoffFailureCategory::FirstPresent,
            crate::app::rendering::RenderFailureCategory::FirstPresent,
        ),
        (
            backend::StrictHandoffFailureCategory::AlreadyInProgress,
            crate::app::rendering::RenderFailureCategory::AlreadyInProgress,
        ),
        (
            backend::StrictHandoffFailureCategory::NoActiveWindow,
            crate::app::rendering::RenderFailureCategory::NoActiveWindow,
        ),
        (
            backend::StrictHandoffFailureCategory::Cancelled,
            crate::app::rendering::RenderFailureCategory::Cancelled,
        ),
        (
            backend::StrictHandoffFailureCategory::Unsupported,
            crate::app::rendering::RenderFailureCategory::Unsupported,
        ),
        (
            backend::StrictHandoffFailureCategory::RendererEvidenceMissing,
            crate::app::rendering::RenderFailureCategory::RendererEvidenceMissing,
        ),
        (
            backend::StrictHandoffFailureCategory::RollbackFailed,
            crate::app::rendering::RenderFailureCategory::Rollback,
        ),
    ];

    for (strict_category, expected_category) in cases {
        let (mut app, _) = App::new();
        app.rendering = crate::app::rendering::RenderingState::PreparingHardware;

        let _ = app.update(Message::BackendBoostConfigured(strict_handoff_error(
            strict_category,
        )));

        assert_eq!(
            app.rendering,
            crate::app::rendering::RenderingState::Failed(expected_category),
            "strict category {strict_category:?} should map structurally"
        );
    }
}

#[test]
fn settings_window_close_request_discards_draft_and_tracks_closed_event() {
    let (mut app, _) = App::new();
    let original_word_wrap = app.settings.word_wrap;

    let _ = app.update(Message::ToggleSettingsPanel);
    let settings_window = app
        .settings_window
        .expect("settings window should be tracked")
        .id();
    let _ = app.update(Message::DraftWordWrapToggled(!original_word_wrap));

    assert!(
        app.settings_window
            .is_some_and(|window| window.is(settings_window))
    );
    assert_eq!(app.settings_dialog.draft.word_wrap, !original_word_wrap);

    let _ = app.update(Message::WindowCloseRequested(settings_window));

    assert!(
        app.settings_window
            .is_some_and(|window| window.is(settings_window))
    );
    assert_eq!(app.settings_dialog.draft.word_wrap, original_word_wrap);

    let _ = app.update(Message::WindowClosed(settings_window));

    assert!(app.settings_window.is_none());
    assert_eq!(app.settings_dialog.draft.word_wrap, original_word_wrap);
}

#[test]
fn advanced_search_window_close_tracks_native_lifecycle_and_reopens_cleanly() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::FindQueryChanged("needle".to_owned()));
    let _ = app.update(Message::ToggleAdvancedSearch(
        crate::message::AdvancedSearchTab::Find,
    ));
    let search_window = app
        .advanced_search_window
        .expect("advanced search window should be tracked")
        .id();

    assert_eq!(app.search_dialog.query, "needle");
    assert_eq!(
        app.search_dialog.active_tab,
        crate::message::AdvancedSearchTab::Find
    );

    let _ = app.update(Message::AdvancedSearchQueryChanged("draft".to_owned()));
    let _ = app.update(Message::WindowCloseRequested(search_window));

    assert!(
        app.advanced_search_window
            .is_some_and(|window| window.is(search_window))
    );
    assert_eq!(app.search_dialog.query, "draft");

    let _ = app.update(Message::WindowClosed(search_window));

    assert!(app.advanced_search_window.is_none());
    assert_eq!(app.search_dialog.query, "draft");

    let _ = app.update(Message::ToggleAdvancedSearch(
        crate::message::AdvancedSearchTab::Replace,
    ));
    let reopened_window = app
        .advanced_search_window
        .expect("advanced search window should reopen")
        .id();

    assert_ne!(reopened_window, search_window);
    assert_eq!(
        app.search_dialog.active_tab,
        crate::message::AdvancedSearchTab::Replace
    );
    assert_eq!(
        app.search_dialog.query, app.find.query,
        "reopening advanced search should sync from the inline find model"
    );
}

#[test]
fn toggling_advanced_search_while_open_reuses_window_and_updates_tab() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::ToggleAdvancedSearch(
        crate::message::AdvancedSearchTab::Find,
    ));
    let search_window = app
        .advanced_search_window
        .expect("advanced search window should be tracked")
        .id();

    let _ = app.update(Message::ToggleAdvancedSearch(
        crate::message::AdvancedSearchTab::FindInFiles,
    ));

    assert!(
        app.advanced_search_window
            .is_some_and(|window| window.is(search_window))
    );
    assert_eq!(
        app.search_dialog.active_tab,
        crate::message::AdvancedSearchTab::FindInFiles
    );
}

#[test]
fn close_clean_exits() {
    let (mut app, _) = App::new();
    let main_window = app
        .main_window_id
        .expect("main window should be tracked after boot");

    let task = app.update(Message::WindowCloseRequested(main_window));

    assert_eq!(task.units(), 1);
}

#[test]
fn close_dirty_prompts() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let main_window = app
        .main_window_id
        .expect("main window should be tracked after boot");

    app.workspace
        .active_document_mut()
        .expect("active document")
        .mark_dirty();

    let task = app.update(Message::WindowCloseRequested(main_window));

    assert_eq!(task.units(), 0);
    assert_eq!(app.close_prompt.document(), Some(document_id));
    assert_eq!(app.files.close_goal(), crate::app::CloseGoal::ExitApp);
    assert!(app.workspace.document(document_id).is_some());
}

#[test]
fn cancel_exit_keeps_app_open() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let main_window = app
        .main_window_id
        .expect("main window should be tracked after boot");

    app.workspace
        .active_document_mut()
        .expect("active document")
        .mark_dirty();

    let _ = app.update(Message::WindowCloseRequested(main_window));
    let task = app.update(Message::DirtyCloseResolved(
        document_id,
        DirtyCloseDecision::Cancel,
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(app.files.close_goal(), CloseGoal::KeepOpen);
    assert!(app.workspace.document(document_id).is_some());
}

#[test]
fn save_cancel_keeps_app_open() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let main_window = app
        .main_window_id
        .expect("main window should be tracked after boot");

    app.workspace
        .active_document_mut()
        .expect("active document")
        .mark_dirty();

    let _ = app.update(Message::WindowCloseRequested(main_window));
    let _ = app.update(Message::DirtyCloseResolved(
        document_id,
        DirtyCloseDecision::Save,
    ));
    let request = app
        .files
        .pending_save()
        .cloned()
        .expect("dirty close save should start a save");
    let task = app.update(Message::FileSaved(
        request,
        Err(crate::services::types::FileError::DialogClosed),
    ));

    assert_eq!(task.units(), 0);
    assert_eq!(app.files.close_goal(), CloseGoal::KeepOpen);
    assert!(app.workspace.document(document_id).is_some());
}

#[test]
fn settings_persist_error_sets_visible_status() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::SettingsPersisted(Err(
        crate::services::types::SettingsError::Io(std::io::ErrorKind::PermissionDenied),
    )));

    assert_eq!(
        app.file_status.as_deref(),
        Some("Settings save failed: I/O error")
    );
}

#[test]
fn about_dialog_opens_switches_tabs_and_closes() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::MenuToggled(Menu::Help));
    let _ = app.update(Message::MenuPathHovered(crate::message::MenuPath {
        depth: 0,
        segments: vec![String::from("about")],
    }));
    let _ = app.update(Message::AboutOpened);

    assert!(app.is_about_visible);
    assert_eq!(app.about_tab, AboutTab::About);
    assert_eq!(app.menu.active(), None);
    assert!(app.menu.path().is_empty());

    let _ = app.update(Message::AboutTabSelected(AboutTab::Licenses));
    assert_eq!(app.about_tab, AboutTab::Licenses);

    let _ = app.update(Message::AboutClosed);
    assert!(!app.is_about_visible);
}

#[test]
fn about_requests_hardware_once_and_preserves_fade_lifecycle() {
    use crate::app::rendering::{RenderFailureCategory, RenderingState};
    let _guard = RenderBackendEnvGuard::new(None);

    for rendering in [
        RenderingState::Software,
        RenderingState::PreparingHardware,
        RenderingState::Hardware,
        RenderingState::Failed(RenderFailureCategory::Prepare),
    ] {
        let (mut app, _) = App::new();
        app.rendering = rendering;
        app.settings.hardware_acceleration = HardwareAccelerationMode::Lazy;

        let task = app.update(Message::AboutOpened);
        let requests_hardware =
            cfg!(feature = "hybrid-rendering") && rendering == RenderingState::Software;
        let expected = if requests_hardware {
            RenderingState::PreparingHardware
        } else {
            rendering
        };

        assert!(app.is_about_visible);
        assert_eq!(app.rendering, expected);
        assert!(app.needs_animation_frames());
        assert!(app.chrome_animation_info().about_rendered_visible);
        assert!(app.chrome_animation_info().about_interactive);
        assert_eq!(app.chrome_animation_info().about_progress, 0.0);
        assert_eq!(task.units() > 0, requests_hardware);
        assert_eq!(app.update(Message::AboutOpened).units(), 0);

        let first = std::time::Instant::now();
        let settled = first + std::time::Duration::from_millis(140);
        let _ = app.update(Message::ChromeAnimationFrame(first));
        let _ = app.update(Message::ChromeAnimationFrame(settled));
        assert_eq!(app.chrome_animation_info().about_progress, 1.0);
        assert!(!app.needs_animation_frames());

        let _ = app.update(Message::AboutClosed);
        assert!(!app.is_about_visible);
        assert!(!app.chrome_animation_info().about_interactive);
        assert!(app.chrome_animation_info().about_rendered_visible);
        assert!(app.needs_animation_frames());
        let _ = app.update(Message::ChromeAnimationFrame(settled));
        let _ = app.update(Message::ChromeAnimationFrame(
            settled + std::time::Duration::from_millis(140),
        ));
        assert_eq!(app.chrome_animation_info().about_progress, 0.0);
        assert!(!app.chrome_animation_info().about_rendered_visible);
        assert!(!app.needs_animation_frames());
        assert_eq!(app.rendering, expected);
    }
}

#[test]
fn about_respects_software_setting_and_environment_override() {
    for (mode, override_value) in [
        (HardwareAccelerationMode::Off, None),
        (HardwareAccelerationMode::Lazy, Some("software")),
    ] {
        let _guard = RenderBackendEnvGuard::new(override_value);
        let (mut app, _) = App::new();
        app.settings.hardware_acceleration = mode;
        app.file_status = Some(String::from("Saved notes.txt"));
        let task = app.update(Message::AboutOpened);
        assert!(app.is_about_visible);
        assert_eq!(
            app.rendering,
            crate::app::rendering::RenderingState::Software
        );
        assert_eq!(task.units(), 0);
        assert_eq!(app.file_status.as_deref(), Some("Saved notes.txt"));
    }
}

#[test]
fn about_tabs_keep_fade_progress_and_reopening_reverses_exit_without_a_jump() {
    let (mut app, _) = App::new();
    let first = std::time::Instant::now();
    let middle = first + std::time::Duration::from_millis(70);
    let _ = app.update(Message::AboutOpened);
    let _ = app.update(Message::ChromeAnimationFrame(first));
    let _ = app.update(Message::ChromeAnimationFrame(middle));
    let opening = app.chrome_animation_info().about_progress;
    assert!(opening > 0.0 && opening < 1.0);

    let _ = app.update(Message::AboutTabSelected(AboutTab::Licenses));
    assert_eq!(app.chrome_animation_info().about_progress, opening);
    let _ = app.update(Message::ChromeAnimationFrame(
        first + std::time::Duration::from_millis(140),
    ));
    assert_eq!(app.chrome_animation_info().about_progress, 1.0);
    assert!(!app.needs_animation_frames());

    let _ = app.update(Message::AboutClosed);
    let close_start = first + std::time::Duration::from_millis(150);
    let reverse_at = close_start + std::time::Duration::from_millis(70);
    let _ = app.update(Message::ChromeAnimationFrame(close_start));
    let _ = app.update(Message::ChromeAnimationFrame(reverse_at));
    let closing = app.chrome_animation_info().about_progress;
    assert!(closing > 0.0 && closing < 1.0);
    assert!(!app.chrome_animation_info().about_interactive);

    let _ = app.update(Message::AboutOpened);
    assert_eq!(app.chrome_animation_info().about_progress, closing);
    assert_eq!(app.about_tab, AboutTab::Licenses);
    assert!(app.chrome_animation_info().about_interactive);
    let _ = app.update(Message::ChromeAnimationFrame(reverse_at));
    let _ = app.update(Message::ChromeAnimationFrame(
        reverse_at + std::time::Duration::from_millis(140),
    ));
    assert_eq!(app.chrome_animation_info().about_progress, 1.0);
    assert!(!app.needs_animation_frames());
}

#[test]
fn about_closed_before_first_frame_does_not_leave_an_invisible_modal() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::AboutOpened);
    let _ = app.update(Message::AboutClosed);
    assert!(!app.is_about_visible);
    assert!(!app.chrome_animation_info().about_rendered_visible);
    assert_eq!(app.chrome_animation_info().about_progress, 0.0);
    assert!(!app.needs_animation_frames());
}

#[test]
fn about_dialog_remains_usable_after_boost_failure() {
    let (mut app, _) = App::new();

    app.rendering = crate::app::rendering::RenderingState::PreparingHardware;
    let _ = app.update(Message::AboutOpened);
    let _ = app.update(Message::BackendBoostConfigured(strict_handoff_error(
        backend::StrictHandoffFailureCategory::RendererEvidenceMissing,
    )));

    assert!(app.is_about_visible);
    assert_eq!(
        app.rendering,
        crate::app::rendering::RenderingState::Failed(
            crate::app::rendering::RenderFailureCategory::RendererEvidenceMissing
        )
    );
    assert!(
        app.file_status
            .as_deref()
            .is_some_and(|status| status.contains("renderer-evidence"))
    );

    let _ = app.update(Message::AboutTabSelected(AboutTab::Debug));
    assert_eq!(app.about_tab, AboutTab::Debug);

    let _ = app.update(Message::AboutClosed);
    assert!(!app.is_about_visible);

    let _ = app.update(Message::AboutOpened);
    assert!(app.is_about_visible);
    assert_eq!(app.about_tab, AboutTab::About);
}

#[test]
fn chrome_find_closed_before_first_frame_is_removed_immediately() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::ToggleFind);
    let _ = app.update(Message::HideFind);

    let closed = app.chrome_animation_info();
    assert!(!closed.find_rendered_visible);
    assert!(!closed.inline_replace_rendered_visible);
    assert!(!app.needs_animation_frames());
}

#[test]
fn chrome_find_reversal_continues_from_visible_progress_and_finishes() {
    let (mut app, _) = App::new();
    let first_frame = std::time::Instant::now();
    let midway_frame = first_frame + std::time::Duration::from_millis(70);

    let _ = app.update(Message::ToggleFind);
    let _ = app.update(Message::ChromeAnimationFrame(first_frame));
    let _ = app.update(Message::ChromeAnimationFrame(midway_frame));
    let midway = app.chrome_animation_info().find_progress;
    assert!(midway > 0.0 && midway < 1.0);

    let _ = app.update(Message::HideFind);
    assert_eq!(app.chrome_animation_info().find_progress, midway);
    let _ = app.update(Message::ChromeAnimationFrame(midway_frame));
    let _ = app.update(Message::ChromeAnimationFrame(
        midway_frame + std::time::Duration::from_millis(140),
    ));

    assert!(!app.chrome_animation_info().find_rendered_visible);
    assert_eq!(app.chrome_animation_info().find_progress, 0.0);
    assert!(!app.needs_animation_frames());
}

#[test]
fn chrome_repeated_show_does_not_restart_running_transition() {
    let (mut app, _) = App::new();
    let first_frame = std::time::Instant::now();

    let _ = app.update(Message::ShowInlineReplace);
    let _ = app.update(Message::ChromeAnimationFrame(first_frame));
    let _ = app.update(Message::ChromeAnimationFrame(
        first_frame + std::time::Duration::from_millis(70),
    ));
    let midway = app.chrome_animation_info();
    assert!(midway.find_progress > 0.0 && midway.find_progress < 1.0);

    let _ = app.update(Message::ShowInlineReplace);
    let _ = app.update(Message::ChromeAnimationFrame(
        first_frame + std::time::Duration::from_millis(140),
    ));

    let opened = app.chrome_animation_info();
    assert_eq!(opened.find_progress, 1.0);
    assert_eq!(opened.inline_replace_progress, 1.0);
    assert!(!app.needs_animation_frames());
}

#[test]
fn chrome_find_panel_reveal_runs_only_while_transitioning() {
    let (mut app, _) = App::new();
    let first_frame = std::time::Instant::now();
    let later_frame = first_frame + std::time::Duration::from_secs(1);

    let initial = app.chrome_animation_info();
    assert!(!initial.find_rendered_visible);
    assert_eq!(initial.find_progress, 0.0);
    assert!(!app.needs_animation_frames());

    let _ = app.update(Message::ToggleFind);

    let opening = app.chrome_animation_info();
    assert!(app.is_find_visible);
    assert!(opening.find_rendered_visible);
    assert_eq!(opening.find_progress, 0.0);
    assert!(app.needs_animation_frames());

    let _ = app.update(Message::ChromeAnimationFrame(first_frame));
    let _ = app.update(Message::ChromeAnimationFrame(later_frame));

    let opened = app.chrome_animation_info();
    assert!(opened.find_rendered_visible);
    assert_eq!(opened.find_progress, 1.0);
    assert!(!app.needs_animation_frames());

    let _ = app.update(Message::HideFind);

    let closing = app.chrome_animation_info();
    assert!(!app.is_find_visible);
    assert!(closing.find_rendered_visible);
    assert_eq!(closing.find_progress, 1.0);
    assert!(app.needs_animation_frames());

    let _ = app.update(Message::ChromeAnimationFrame(later_frame));
    let _ = app.update(Message::ChromeAnimationFrame(
        later_frame + std::time::Duration::from_secs(1),
    ));

    let closed = app.chrome_animation_info();
    assert!(!closed.find_rendered_visible);
    assert_eq!(closed.find_progress, 0.0);
    assert!(!app.needs_animation_frames());
}

#[test]
fn chrome_inline_replace_reveal_is_independent_of_find_panel_reveal() {
    let (mut app, _) = App::new();
    let first_frame = std::time::Instant::now();
    let later_frame = first_frame + std::time::Duration::from_secs(1);

    let _ = app.update(Message::ShowInlineReplace);

    assert!(app.is_find_visible);
    assert!(app.is_inline_replace_visible);
    assert!(app.chrome_animation_info().find_rendered_visible);
    assert!(app.chrome_animation_info().inline_replace_rendered_visible);

    let _ = app.update(Message::ChromeAnimationFrame(first_frame));
    let _ = app.update(Message::ChromeAnimationFrame(later_frame));

    let opened = app.chrome_animation_info();
    assert_eq!(opened.find_progress, 1.0);
    assert_eq!(opened.inline_replace_progress, 1.0);

    let _ = app.update(Message::ToggleInlineReplace);

    let closing_replace = app.chrome_animation_info();
    assert!(app.is_find_visible);
    assert!(!app.is_inline_replace_visible);
    assert!(closing_replace.find_rendered_visible);
    assert!(closing_replace.inline_replace_rendered_visible);
    assert_eq!(closing_replace.find_progress, 1.0);
    assert_eq!(closing_replace.inline_replace_progress, 1.0);
    assert!(app.needs_animation_frames());

    let _ = app.update(Message::ChromeAnimationFrame(later_frame));
    let _ = app.update(Message::ChromeAnimationFrame(
        later_frame + std::time::Duration::from_secs(1),
    ));

    let closed_replace = app.chrome_animation_info();
    assert!(closed_replace.find_rendered_visible);
    assert_eq!(closed_replace.find_progress, 1.0);
    assert!(!closed_replace.inline_replace_rendered_visible);
    assert_eq!(closed_replace.inline_replace_progress, 0.0);
    assert!(!app.needs_animation_frames());
}

#[test]
fn chrome_function_list_reveal_tracks_panel_visibility() {
    let (mut app, _) = App::new();
    let first_frame = std::time::Instant::now();
    let later_frame = first_frame + std::time::Duration::from_secs(1);

    let _ = app.update(Message::MenuToggled(Menu::View));
    let _ = app.update(Message::ToggleFunctionList);

    let opening = app.chrome_animation_info();
    assert_eq!(app.menu.active(), None);
    assert!(app.is_function_list_visible);
    assert!(opening.function_list_rendered_visible);
    assert_eq!(opening.function_list_progress, 0.0);
    assert!(app.needs_animation_frames());

    let _ = app.update(Message::ChromeAnimationFrame(first_frame));
    let _ = app.update(Message::ChromeAnimationFrame(later_frame));

    let opened = app.chrome_animation_info();
    assert!(opened.function_list_rendered_visible);
    assert_eq!(opened.function_list_progress, 1.0);
    assert!(!app.needs_animation_frames());

    let _ = app.update(Message::ToggleFunctionList);

    let closing = app.chrome_animation_info();
    assert!(!app.is_function_list_visible);
    assert!(closing.function_list_rendered_visible);
    assert_eq!(closing.function_list_progress, 1.0);
    assert!(app.needs_animation_frames());

    let _ = app.update(Message::ChromeAnimationFrame(later_frame));
    let _ = app.update(Message::ChromeAnimationFrame(
        later_frame + std::time::Duration::from_secs(1),
    ));

    let closed = app.chrome_animation_info();
    assert!(!closed.function_list_rendered_visible);
    assert_eq!(closed.function_list_progress, 0.0);
    assert!(!app.needs_animation_frames());
}

#[test]
fn language_selection_updates_active_document_without_dirtying_it() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::MenuToggled(Menu::Language));
    let _ = app.update(Message::LanguageSelected("rs".to_owned()));

    let document = app
        .workspace
        .active_document()
        .expect("workspace should have an active document");

    assert_eq!(document.syntax_token, "rs");
    assert!(!document.is_dirty);
    assert_eq!(app.menu.active(), None);
}

#[test]
fn menu_path_tracks_generic_flyout_state_and_resets_on_menu_change() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::MenuToggled(Menu::Encoding));
    let _ = app.update(Message::MenuPathHovered(crate::message::MenuPath {
        depth: 0,
        segments: vec!["character-sets".to_owned()],
    }));

    assert_eq!(app.menu.active(), Some(Menu::Encoding));
    assert_eq!(app.menu.path(), vec!["character-sets"]);

    let _ = app.update(Message::MenuHovered(Menu::File));

    assert_eq!(app.menu.active(), Some(Menu::File));
    assert!(app.menu.path().is_empty());
}

#[test]
fn menu_path_can_collapse_to_parent_and_ignores_closed_menu_hover() {
    let (mut app, _) = App::new();

    let _ = app.update(Message::MenuToggled(Menu::Encoding));
    let _ = app.update(Message::MenuPathHovered(crate::message::MenuPath {
        depth: 1,
        segments: vec!["character-sets".to_owned(), "western-european".to_owned()],
    }));

    assert_eq!(app.menu.path(), vec!["character-sets", "western-european"]);

    let _ = app.update(Message::MenuPathHovered(crate::message::MenuPath {
        depth: 1,
        segments: vec!["character-sets".to_owned()],
    }));

    assert_eq!(app.menu.path(), vec!["character-sets"]);

    let _ = app.update(Message::MenuClosed);
    let _ = app.update(Message::MenuPathHovered(crate::message::MenuPath {
        depth: 0,
        segments: vec!["character-sets".to_owned()],
    }));

    assert_eq!(app.menu.active(), None);
    assert!(app.menu.path().is_empty());
}

#[test]
fn opening_rust_file_defers_syntax_parsing_to_worker() {
    let (mut app, _) = App::new();
    let contents = (0..200)
        .map(|line| format!("pub fn function_{line}() -> usize {{ {line} }}"))
        .collect::<Vec<_>>()
        .join("\n");

    let _ = app.update(Message::FileOpened(Ok(OpenedFile {
        disk_revision: crate::core::FileRevision::from_bytes(b"fixture"),
        path: PathBuf::from("widget.rs"),
        contents: Arc::new(crate::core::DecodedText {
            text: contents,
            encoding: crate::core::TextEncoding::Utf8,
            had_errors: false,
        }),
    })));

    let document = app.workspace.active_document().expect("active document");

    assert_eq!(document.syntax_token, "rs");
    assert_eq!(
        document.syntax_cache.borrow().cached_line_count(),
        0,
        "opening a highlighted file must not parse on the UI thread"
    );
}

#[test]
fn undo_and_redo_commands_update_active_document() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();

    let _ = app.update(Message::EditorAction(
        document_id,
        EditorAction::InsertText("x".to_owned()),
    ));

    assert_eq!(app.workspace.active_document().expect("active").text(), "x");
    assert!(app.workspace.active_document().expect("active").is_dirty);

    let _ = app.update(Message::Undo);

    assert_eq!(app.workspace.active_document().expect("active").text(), "");
    assert!(!app.workspace.active_document().expect("active").is_dirty);

    let _ = app.update(Message::Redo);

    assert_eq!(app.workspace.active_document().expect("active").text(), "x");
    assert!(app.workspace.active_document().expect("active").is_dirty);
}
