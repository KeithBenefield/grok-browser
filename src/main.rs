// Hide the console window in release builds; keep it in debug for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod webview;

#[cfg(target_os = "windows")]
mod windows_app;

fn main() {
    // Must run before any HWND is created (taskbar / pin identity).
    #[cfg(target_os = "windows")]
    windows_app::set_process_app_id();

    webview::setup_webview();
}
