use crate::ipc::ActivationRequest;

pub(super) fn prepare_for_presentation(_request: &ActivationRequest) {
    activate_macos_application();
}

#[cfg(target_os = "macos")]
fn activate_macos_application() {
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    // NSApplication.activate requires macOS 14; retain Ventura support.
    #[allow(deprecated)]
    objc2_app_kit::NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

#[cfg(not(target_os = "macos"))]
fn activate_macos_application() {}
