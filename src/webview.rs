use std::path::PathBuf;

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::{NewWindowResponse, WebContext, WebViewBuilder};

const START_URL: &str = "https://grok.com";
const APP_NAME: &str = "Grok Browser";

/// Host-injected (not page-eval) helpers:
/// - Ensure form controls have a `name` so Chromium's autofill audit is quieter
/// - Never uses eval / string timers (avoids CSP `unsafe-eval` issues from *our* code)
const PAGE_HELPERS_JS: &str = r#"
(function () {
  if (window.__grokBrowserHelpers) return;
  window.__grokBrowserHelpers = true;

  function ensureFieldNames(root) {
    try {
      var nodes = (root || document).querySelectorAll(
        "input:not([name]):not([id]), select:not([name]):not([id]), textarea:not([name]):not([id])"
      );
      for (var i = 0; i < nodes.length; i++) {
        var el = nodes[i];
        if (!el.getAttribute("name") && !el.id) {
          el.setAttribute("name", "gb-field-" + (el.type || el.tagName.toLowerCase()) + "-" + i);
        }
      }
    } catch (_) {}
  }

  function run() {
    ensureFieldNames(document);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", run, { once: true });
  } else {
    run();
  }

  try {
    new MutationObserver(function () {
      run();
    }).observe(document.documentElement, { childList: true, subtree: true });
  } catch (_) {}
})();
"#;

enum UserEvent {
    TitleChanged(String),
    /// `window.open` / target=_blank → load in this window instead of a popup.
    Navigate(String),
}

/// WebView2 profile outside the build tree so `target/` does not accumulate cache.
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

fn set_window_title(window: &tao::window::Window, title: &str) {
    if title.is_empty() {
        window.set_title(APP_NAME);
    } else {
        window.set_title(&format!("{title} — {APP_NAME}"));
    }
}

pub fn setup_webview() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(640.0, 480.0))
        .build(&event_loop)
        .expect("Failed to create window");

    let data_dir = user_data_dir();
    // Keep WebContext alive for the life of the WebView.
    let mut web_context = WebContext::new(Some(data_dir.clone()));

    let title_proxy = proxy.clone();
    let nav_proxy = proxy.clone();

    let mut builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_url(START_URL)
        .with_clipboard(true)
        .with_hotkeys_zoom(true)
        .with_focused(true)
        .with_general_autofill_enabled(true)
        .with_initialization_script(PAGE_HELPERS_JS)
        .with_document_title_changed_handler(move |title| {
            let _ = title_proxy.send_event(UserEvent::TitleChanged(title));
        })
        .with_download_started_handler(move |url, path| {
            let name = url
                .rsplit('/')
                .next()
                .and_then(|s| {
                    let clean = s.split('?').next().unwrap_or(s);
                    if !clean.is_empty() && clean.contains('.') {
                        Some(clean)
                    } else {
                        None
                    }
                })
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
        // No second window: open links / window.open in this shell.
        .with_new_window_req_handler(move |url, _features| {
            let _ = nav_proxy.send_event(UserEvent::Navigate(url));
            NewWindowResponse::Deny
        });

    // DevTools available in debug builds (right-click Inspect / F12 with accelerator keys).
    #[cfg(debug_assertions)]
    {
        builder = builder.with_devtools(true);
    }

    let webview = builder.build(&window).expect("Failed to build WebView");

    eprintln!(
        "{APP_NAME} started — profile: {} — wry {} — {START_URL}",
        data_dir.display(),
        env!("CARGO_PKG_VERSION"),
    );

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // Keep webview + context alive for the event loop lifetime.
        let _ = (&webview, &web_context);

        match event {
            Event::UserEvent(UserEvent::TitleChanged(title)) => {
                set_window_title(&window, &title);
            }
            Event::UserEvent(UserEvent::Navigate(url)) => {
                if let Err(err) = webview.load_url(&url) {
                    eprintln!("navigation failed ({url}): {err}");
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
