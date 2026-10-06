use iced::{Task, backend};

use crate::core::{EditorSettings, HardwareAccelerationMode};
use crate::message::Message;

use super::App;

pub const RENDER_BACKEND_ENV: &str = "FRAGILE_NOTEPAD_RENDER_BACKEND";
pub const SOFTWARE_BACKEND_VALUE: &str = "software";
pub const LAZY_GPU_BACKEND_VALUE: &str = "lazy-gpu";
pub const HARDWARE_DIAGNOSTIC_BACKEND_VALUE: &str = "hardware-diagnostic";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderingState {
    Software,
    PreparingHardware,
    Hardware,
    Failed(RenderFailureCategory),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderFailureCategory {
    Prepare,
    WarmUp,
    Commit,
    FirstPresent,
    AlreadyInProgress,
    NoActiveWindow,
    Cancelled,
    Unsupported,
    RendererEvidenceMissing,
    Rollback,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderBackendPolicy {
    Software,
    LazyGpu,
    HardwareDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoostStart {
    Started,
    Ignored(RenderBoostSuppression),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderBoostSuppression {
    ForcedSoftware,
    AlreadyPreparing,
    AlreadyHardware,
    FailureCooldown(RenderFailureCategory),
}

struct RenderBoostFailure {
    category: RenderFailureCategory,
    message: String,
}

pub(super) fn startup_gpu_boost_requested(settings: &EditorSettings) -> bool {
    matches!(
        render_backend_policy(settings, std::env::var(RENDER_BACKEND_ENV).ok().as_deref()),
        RenderBackendPolicy::LazyGpu | RenderBackendPolicy::HardwareDiagnostic
    )
}

impl App {
    pub(super) fn request_about_gpu_boost(&mut self) -> Task<Message> {
        // Automatic animation requests should stay quiet in software-only
        // mode and leave existing file/failure status messages intact.
        if cfg!(feature = "hybrid-rendering")
            && self.rendering == RenderingState::Software
            && startup_gpu_boost_requested(&self.settings)
        {
            self.request_gpu_boost()
        } else {
            Task::none()
        }
    }

    pub(super) fn request_gpu_boost(&mut self) -> Task<Message> {
        let policy = render_backend_policy(
            &self.settings,
            std::env::var(RENDER_BACKEND_ENV).ok().as_deref(),
        );

        match start_gpu_boost(&mut self.rendering, policy) {
            BoostStart::Started => configure_hardware_backend(policy),
            BoostStart::Ignored(suppression) => {
                if matches!(
                    suppression,
                    RenderBoostSuppression::ForcedSoftware
                        | RenderBoostSuppression::FailureCooldown(_)
                ) {
                    self.file_status = Some(suppression.status_message());
                }

                Task::none()
            }
        }
    }

    pub(super) fn complete_gpu_boost(
        &mut self,
        outcome: backend::StrictHandoffOutcome,
    ) -> Task<Message> {
        let failure = strict_handoff_failure(outcome);
        complete_gpu_boost_state(
            &mut self.rendering,
            failure.as_ref().map(|failure| failure.category),
        );

        if let Some(failure) = failure {
            self.file_status = Some(format!(
                "GPU acceleration unavailable ({}): {error}",
                failure.category.label(),
                error = failure.message
            ));
        }

        Task::none()
    }
}

impl RenderingState {
    pub(super) fn label(self) -> String {
        match self {
            Self::Software => String::from("software (tiny-skia)"),
            Self::PreparingHardware => String::from("preparing hardware handoff"),
            Self::Hardware => String::from("hardware (wgpu)"),
            Self::Failed(category) => {
                format!("software fallback after {} failure", category.label())
            }
        }
    }
}

pub(super) fn current_policy_label(settings: &EditorSettings) -> String {
    let env_override = std::env::var(RENDER_BACKEND_ENV).ok();
    let policy = render_backend_policy(settings, env_override.as_deref());

    match env_override {
        Some(value) if parse_render_backend_override(&value).is_some() => {
            format!("{} (from {RENDER_BACKEND_ENV})", policy.label())
        }
        Some(value) => format!(
            "{} (ignored invalid {RENDER_BACKEND_ENV}={value})",
            policy.label()
        ),
        None => format!("{} (saved setting)", policy.label()),
    }
}

impl RenderBoostSuppression {
    fn status_message(self) -> String {
        match self {
            Self::ForcedSoftware => String::from("GPU acceleration disabled by rendering policy"),
            Self::FailureCooldown(category) => format!(
                "GPU acceleration retry suppressed after {} failure",
                category.label()
            ),
            Self::AlreadyPreparing => String::from("GPU acceleration is already preparing"),
            Self::AlreadyHardware => String::from("GPU acceleration is already active"),
        }
    }
}

impl RenderBackendPolicy {
    fn label(self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::LazyGpu => "lazy hybrid",
            Self::HardwareDiagnostic => "hardware diagnostic",
        }
    }
}

impl RenderFailureCategory {
    fn label(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::WarmUp => "warm-up",
            Self::Commit => "commit",
            Self::FirstPresent => "first-present",
            Self::AlreadyInProgress => "already-in-progress",
            Self::NoActiveWindow => "no-active-window",
            Self::Cancelled => "cancelled",
            Self::Unsupported => "unsupported",
            Self::RendererEvidenceMissing => "renderer-evidence",
            Self::Rollback => "rollback",
            Self::Unknown => "unknown",
        }
    }

    fn from_strict_category(category: backend::StrictHandoffFailureCategory) -> Self {
        match category {
            backend::StrictHandoffFailureCategory::AlreadyInProgress => Self::AlreadyInProgress,
            backend::StrictHandoffFailureCategory::NoActiveWindow => Self::NoActiveWindow,
            backend::StrictHandoffFailureCategory::Prepare => Self::Prepare,
            backend::StrictHandoffFailureCategory::WarmUp => Self::WarmUp,
            backend::StrictHandoffFailureCategory::Commit => Self::Commit,
            backend::StrictHandoffFailureCategory::FirstPresent => Self::FirstPresent,
            backend::StrictHandoffFailureCategory::Cancelled => Self::Cancelled,
            backend::StrictHandoffFailureCategory::Unsupported => Self::Unsupported,
            backend::StrictHandoffFailureCategory::RendererEvidenceMissing => {
                Self::RendererEvidenceMissing
            }
            backend::StrictHandoffFailureCategory::RollbackFailed => Self::Rollback,
        }
    }
}

fn strict_handoff_failure(outcome: backend::StrictHandoffOutcome) -> Option<RenderBoostFailure> {
    match outcome {
        Ok(result) if strict_handoff_completed(&result) => None,
        Ok(result) => Some(RenderBoostFailure {
            category: RenderFailureCategory::Unknown,
            message: format!(
                "strict handoff completed with phase {} and rollback {}",
                strict_handoff_phase_label(result.completed_phase),
                strict_rollback_status_label(result.rollback)
            ),
        }),
        Err(error) => Some(RenderBoostFailure {
            category: RenderFailureCategory::from_strict_category(error.category),
            message: format!(
                "{} (phase {}, rollback {})",
                error.message,
                strict_handoff_phase_label(error.phase),
                strict_rollback_status_label(error.rollback)
            ),
        }),
    }
}

fn strict_handoff_completed(result: &backend::StrictHandoffResult) -> bool {
    result.completed_phase == backend::StrictHandoffPhase::Completed
        && result.rollback == backend::StrictRollbackStatus::ReleasedAfterSuccess
}

fn strict_handoff_phase_label(phase: backend::StrictHandoffPhase) -> &'static str {
    match phase {
        backend::StrictHandoffPhase::Preparing => "preparing",
        backend::StrictHandoffPhase::Warming => "warming",
        backend::StrictHandoffPhase::CommitPending => "commit-pending",
        backend::StrictHandoffPhase::AwaitingFirstPresent => "awaiting-first-present",
        backend::StrictHandoffPhase::Completed => "completed",
        backend::StrictHandoffPhase::Cancelled => "cancelled",
        backend::StrictHandoffPhase::Rollback => "rollback",
    }
}

fn strict_rollback_status_label(rollback: backend::StrictRollbackStatus) -> &'static str {
    match rollback {
        backend::StrictRollbackStatus::NotNeeded => "not-needed",
        backend::StrictRollbackStatus::Retained => "retained",
        backend::StrictRollbackStatus::Restored => "restored",
        backend::StrictRollbackStatus::RestoreFailed => "restore-failed",
        backend::StrictRollbackStatus::ReleasedAfterSuccess => "released-after-success",
        backend::StrictRollbackStatus::NotAvailable => "not-available",
    }
}

fn render_backend_policy(
    settings: &EditorSettings,
    env_override: Option<&str>,
) -> RenderBackendPolicy {
    match env_override.and_then(parse_render_backend_override) {
        Some(policy) => policy,
        None => match settings.hardware_acceleration {
            HardwareAccelerationMode::Off => RenderBackendPolicy::Software,
            HardwareAccelerationMode::Lazy => RenderBackendPolicy::LazyGpu,
            HardwareAccelerationMode::Diagnostic => RenderBackendPolicy::HardwareDiagnostic,
        },
    }
}

fn parse_render_backend_override(value: &str) -> Option<RenderBackendPolicy> {
    match value {
        SOFTWARE_BACKEND_VALUE => Some(RenderBackendPolicy::Software),
        LAZY_GPU_BACKEND_VALUE => Some(RenderBackendPolicy::LazyGpu),
        HARDWARE_DIAGNOSTIC_BACKEND_VALUE => Some(RenderBackendPolicy::HardwareDiagnostic),
        _ => None,
    }
}

fn start_gpu_boost(state: &mut RenderingState, policy: RenderBackendPolicy) -> BoostStart {
    if policy == RenderBackendPolicy::Software {
        return BoostStart::Ignored(RenderBoostSuppression::ForcedSoftware);
    }

    match *state {
        RenderingState::Software => {
            *state = RenderingState::PreparingHardware;
            BoostStart::Started
        }
        RenderingState::PreparingHardware => {
            BoostStart::Ignored(RenderBoostSuppression::AlreadyPreparing)
        }
        RenderingState::Hardware => BoostStart::Ignored(RenderBoostSuppression::AlreadyHardware),
        RenderingState::Failed(category) => {
            BoostStart::Ignored(RenderBoostSuppression::FailureCooldown(category))
        }
    }
}

fn complete_gpu_boost_state(state: &mut RenderingState, failure: Option<RenderFailureCategory>) {
    *state = match failure {
        Some(category) => RenderingState::Failed(category),
        None => RenderingState::Hardware,
    };
}

#[cfg(feature = "hybrid-rendering")]
fn configure_hardware_backend(policy: RenderBackendPolicy) -> Task<Message> {
    use iced::Backend;
    use iced::backend::Api;

    backend::prepare_warm_and_commit(backend::Settings {
        backend: Backend::Hardware(Api::Vulkan),
        power_preference: match policy {
            RenderBackendPolicy::HardwareDiagnostic => backend::PowerPreference::HighPerformance,
            _ => backend::PowerPreference::LowPower,
        },
        antialiasing: false,
        vsync: true,
    })
    .map(Message::BackendBoostConfigured)
}

#[cfg(not(feature = "hybrid-rendering"))]
fn configure_hardware_backend(_policy: RenderBackendPolicy) -> Task<Message> {
    Task::done(Message::BackendBoostConfigured(Err(
        backend::StrictHandoffError {
            phase: backend::StrictHandoffPhase::Preparing,
            category: backend::StrictHandoffFailureCategory::Unsupported,
            rollback: backend::StrictRollbackStatus::NotNeeded,
            windows: Vec::new(),
            message: String::from("hybrid-rendering feature is disabled"),
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        BoostStart, RenderBackendPolicy, RenderBoostSuppression, RenderFailureCategory,
        RenderingState, complete_gpu_boost_state, parse_render_backend_override,
        render_backend_policy, start_gpu_boost,
    };
    use crate::core::{EditorSettings, HardwareAccelerationMode};

    #[test]
    fn render_backend_override_values_are_explicit() {
        assert_eq!(
            parse_render_backend_override("software"),
            Some(RenderBackendPolicy::Software)
        );
        assert_eq!(
            parse_render_backend_override("lazy-gpu"),
            Some(RenderBackendPolicy::LazyGpu)
        );
        assert_eq!(
            parse_render_backend_override("hardware-diagnostic"),
            Some(RenderBackendPolicy::HardwareDiagnostic)
        );
        assert_eq!(parse_render_backend_override(""), None);
        assert_eq!(parse_render_backend_override("lazy-gpu "), None);
    }

    #[test]
    fn env_override_takes_precedence_over_persisted_settings() {
        let mut settings = EditorSettings {
            hardware_acceleration: HardwareAccelerationMode::Diagnostic,
            ..EditorSettings::default()
        };

        assert_eq!(
            render_backend_policy(&settings, Some("software")),
            RenderBackendPolicy::Software
        );

        settings.hardware_acceleration = HardwareAccelerationMode::Off;
        assert_eq!(
            render_backend_policy(&settings, Some("lazy-gpu")),
            RenderBackendPolicy::LazyGpu
        );
    }

    #[test]
    fn persisted_policy_applies_when_env_override_is_absent() {
        let mut settings = EditorSettings::default();
        assert_eq!(
            render_backend_policy(&settings, None),
            RenderBackendPolicy::LazyGpu
        );

        settings.hardware_acceleration = HardwareAccelerationMode::Off;
        assert_eq!(
            render_backend_policy(&settings, None),
            RenderBackendPolicy::Software
        );

        settings.hardware_acceleration = HardwareAccelerationMode::Lazy;
        assert_eq!(
            render_backend_policy(&settings, None),
            RenderBackendPolicy::LazyGpu
        );
    }

    #[test]
    fn boost_request_starts_only_from_software_when_enabled() {
        let mut state = RenderingState::Software;

        assert_eq!(
            start_gpu_boost(&mut state, RenderBackendPolicy::LazyGpu),
            BoostStart::Started
        );
        assert_eq!(state, RenderingState::PreparingHardware);
        assert_eq!(
            start_gpu_boost(&mut state, RenderBackendPolicy::LazyGpu),
            BoostStart::Ignored(RenderBoostSuppression::AlreadyPreparing)
        );
    }

    #[test]
    fn software_policy_suppresses_boost() {
        let mut state = RenderingState::Software;

        assert_eq!(
            start_gpu_boost(&mut state, RenderBackendPolicy::Software),
            BoostStart::Ignored(RenderBoostSuppression::ForcedSoftware)
        );
        assert_eq!(state, RenderingState::Software);
    }

    #[test]
    fn failed_boost_enters_cooldown_and_suppresses_retry() {
        let mut state = RenderingState::PreparingHardware;

        complete_gpu_boost_state(&mut state, Some(RenderFailureCategory::Prepare));

        assert_eq!(
            state,
            RenderingState::Failed(RenderFailureCategory::Prepare)
        );
        assert_eq!(
            start_gpu_boost(&mut state, RenderBackendPolicy::LazyGpu),
            BoostStart::Ignored(RenderBoostSuppression::FailureCooldown(
                RenderFailureCategory::Prepare
            ))
        );
    }

    #[test]
    fn successful_boost_records_hardware_state() {
        let mut state = RenderingState::PreparingHardware;

        complete_gpu_boost_state(&mut state, None);

        assert_eq!(state, RenderingState::Hardware);
    }
}
