pub mod app;
pub mod assets;
pub mod core;
pub mod editor;
pub mod ipc;
pub mod message;
pub mod perf_trace;
mod platform;
pub mod search_dialog;
pub mod services;
pub mod settings_dialog;
pub mod startup;
pub mod ui;

/// Share this guard between tests that load fonts or assert exact cache reuse.
#[cfg(test)]
pub(crate) fn font_system_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Finish lazy bundled-font loading before any cache records a font version.
    let _ = editor::widget::regional_cjk_font(editor::cjk::CjkLanguage::Japanese);
    guard
}
