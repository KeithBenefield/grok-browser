// src/main.rs
#![windows_subsystem = "windows"] // Add this line

mod webview;

fn main() {
    webview::setup_webview();
}