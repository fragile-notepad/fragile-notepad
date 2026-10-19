//! macOS application integrations.

mod editing;
mod menu;

use objc2_foundation::{NSProcessInfo, NSString};

use super::App;

pub(super) use editing::edit;
pub(super) use menu::subscription;

pub(super) fn initialize_name() {
    NSProcessInfo::processInfo().setProcessName(&NSString::from_str("Fragile Notepad"));
}

pub(super) fn sync(app: &App) {
    menu::sync(menu::MenuModel::from_app(app));
}
