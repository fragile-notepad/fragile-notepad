use iced::{Backend, Settings};

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupOptions {
    pub files: Vec<PathBuf>,
    pub restore_session: bool,
}

impl Default for StartupOptions {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            restore_session: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupCommand {
    Launch(StartupOptions),
    Help,
    Version,
}

pub const CLI_HELP: &str = "Usage: fragile-notepad [OPTIONS] [--] [FILE ...]\n\nOptions:\n  --no-session  Start without restoring the previous session\n  -h, --help    Print this help\n  -V, --version Print the version\n\nRelative file paths are resolved from the current directory.\nUse -- before a filename that starts with -.";

pub fn parse_arguments(
    arguments: impl IntoIterator<Item = OsString>,
    working_directory: &Path,
) -> Result<StartupCommand, String> {
    let mut options = StartupOptions::default();
    let mut positional_only = false;
    for argument in arguments {
        if !positional_only {
            match argument.to_str() {
                Some("--") => {
                    positional_only = true;
                    continue;
                }
                Some("--no-session") => {
                    options.restore_session = false;
                    continue;
                }
                Some("--help" | "-h") => return Ok(StartupCommand::Help),
                Some("--version" | "-V") => return Ok(StartupCommand::Version),
                Some(value) if value.starts_with('-') => {
                    return Err(format!(
                        "Unknown option: {value}. Use -- before filenames starting with -."
                    ));
                }
                _ => {}
            }
        }
        if argument.is_empty() {
            return Err("A file path cannot be empty.".into());
        }
        let path = PathBuf::from(argument);
        let path = if path.is_absolute() {
            path
        } else {
            working_directory.join(path)
        };
        #[cfg(windows)]
        let path = std::path::absolute(path).map_err(|error| error.to_string())?;
        options.files.push(path);
    }
    Ok(StartupCommand::Launch(options))
}

const STARTUP_PROBE_ENV: &str = "FRAGILE_NOTEPAD_STARTUP_PROBE";
const STARTUP_PROBE_OUTPUT_PREFIX: &str = "FRAGILE_NOTEPAD_FIRST_VIEW_READY_MS=";
const STARTUP_FRAME_OUTPUT_PREFIX: &str = "FRAGILE_NOTEPAD_FIRST_FRAME_READY_MS=";

static STARTED_AT: OnceLock<Instant> = OnceLock::new();
static REPORTED_FIRST_VIEW: AtomicBool = AtomicBool::new(false);
static REPORTED_FIRST_FRAME: AtomicBool = AtomicBool::new(false);

pub fn iced_settings() -> Settings {
    Settings {
        backend: Backend::Software,
        antialiasing: false,
        vsync: false,
        ..Settings::default()
    }
}

/// Additional interface scaling, useful on low-density or virtual displays.
/// The window system's display scale is still applied independently.
pub fn ui_scale_factor() -> f32 {
    std::env::var("FRAGILE_NOTEPAD_UI_SCALE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && (0.75..=3.0).contains(value))
        .unwrap_or(1.0)
}

pub fn startup_probe_enabled() -> bool {
    std::env::var_os(STARTUP_PROBE_ENV).is_some()
}

/// Package validation exits only after the first screenshot has completed.
pub fn exit_after_first_frame() -> bool {
    startup_probe_enabled() && std::env::var_os("FRAGILE_NOTEPAD_EXIT_AFTER_FRAME").is_some()
}

pub fn mark_startup_started() {
    let _ = STARTED_AT.set(Instant::now());
}

pub fn report_first_view_ready() {
    if !startup_probe_enabled() || REPORTED_FIRST_VIEW.swap(true, Ordering::Relaxed) {
        return;
    }

    let elapsed = STARTED_AT.get_or_init(Instant::now).elapsed();
    println!(
        "{STARTUP_PROBE_OUTPUT_PREFIX}{}",
        elapsed.as_secs_f64() * 1_000.0
    );
    let _ = io::stdout().flush();
}

/// Reports completion of the startup screenshot, after layout and rendering.
pub fn report_first_frame_ready() {
    if !startup_probe_enabled() || REPORTED_FIRST_FRAME.swap(true, Ordering::Relaxed) {
        return;
    }
    let elapsed = STARTED_AT.get_or_init(Instant::now).elapsed();
    println!(
        "{STARTUP_FRAME_OUTPUT_PREFIX}{}",
        elapsed.as_secs_f64() * 1_000.0
    );
    let _ = io::stdout().flush();
}
