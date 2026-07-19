// Hide the console window in release builds; keep it in debug for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod webview;

/// Stable Application User Model ID so Windows can pin this app to the taskbar.
/// Must match any Start Menu / desktop shortcut AppUserModel.ID property.
#[cfg(target_os = "windows")]
const APP_USER_MODEL_ID: &str = "KeithBenefield.GrokBrowser";

#[cfg(target_os = "windows")]
fn set_app_user_model_id() {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }

    let wide: Vec<u16> = std::ffi::OsStr::new(APP_USER_MODEL_ID)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Ignore errors: pinning still works without this on some Windows builds.
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
    }
}

fn main() {
    #[cfg(target_os = "windows")]
    set_app_user_model_id();

    webview::setup_webview();
}
