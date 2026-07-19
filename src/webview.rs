use std::path::PathBuf;

use wry::{
    application::{
        dpi::LogicalSize,
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::WindowBuilder,
    },
    webview::{WebContext, WebViewBuilder},
};

const START_URL: &str = "https://grok.com";
const APP_NAME: &str = "Grok Browser";

/// WebView2 / browser profile directory outside the build tree so
/// `target/debug` does not accumulate unbounded cache growth.
fn user_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);

    let dir = base.join("GrokBrowser").join("WebView2");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("warning: could not create data dir {}: {err}", dir.display());
    }
    dir
}

fn downloads_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Downloads"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

pub fn setup_webview() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(640.0, 480.0))
        .build(&event_loop)
        .expect("Failed to create window");

    let data_dir = user_data_dir();
    // Keep WebContext alive for the life of the WebView.
    let mut web_context = WebContext::new(Some(data_dir.clone()));

    let mut builder = WebViewBuilder::new(window)
        .expect("Failed to create WebViewBuilder")
        .with_web_context(&mut web_context)
        .with_url(START_URL)
        .expect("Failed to set URL")
        .with_clipboard(true)
        .with_hotkeys_zoom(true)
        .with_document_title_changed_handler(|window, title| {
            if title.is_empty() {
                window.set_title(APP_NAME);
            } else {
                window.set_title(&format!("{title} — {APP_NAME}"));
            }
        })
        .with_download_started_handler(move |url, path| {
            // Suggest a filename under the user's Downloads folder.
            let name = url
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty() && s.contains('.'))
                .unwrap_or("download");
            *path = downloads_dir().join(name);
            true
        })
        .with_download_completed_handler(|_url, path, success| {
            if success {
                if let Some(path) = path {
                    eprintln!("download complete: {}", path.display());
                }
            } else {
                eprintln!("download failed");
            }
        })
        // Allow in-page navigations; block popups that open a second window
        // (user can still navigate via links in the same webview).
        .with_new_window_req_handler(|url| {
            eprintln!("blocked new-window request (open in-tab): {url}");
            false
        });

    // DevTools only in debug builds (right-click → Inspect, or open programmatically).
    #[cfg(debug_assertions)]
    {
        builder = builder.with_devtools(true);
    }

    let webview = builder.build().expect("Failed to build WebView");

    eprintln!(
        "{APP_NAME} started — profile: {} — {START_URL}",
        data_dir.display()
    );

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // Keep webview + context alive for the event loop lifetime.
        let _ = (&webview, &web_context);

        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
