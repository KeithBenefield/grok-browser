use std::path::PathBuf;

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Icon, WindowBuilder},
};
use wry::{NewWindowResponse, WebContext, WebViewBuilder};

#[cfg(target_os = "windows")]
use tao::platform::windows::IconExtWindows;

const START_URL: &str = "https://grok.com";
const APP_NAME: &str = "Grok";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    let dir = std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Downloads"))
        .unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Strip characters Windows rejects in file names.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.');
    if cleaned.is_empty() {
        "download".into()
    } else {
        // Keep names reasonable for Explorer / long CDN blobs.
        if cleaned.len() > 180 {
            let ext = std::path::Path::new(cleaned)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let stem: String = cleaned.chars().take(160).collect();
            if ext.is_empty() {
                stem
            } else {
                format!("{stem}.{ext}")
            }
        } else {
            cleaned.to_string()
        }
    }
}

fn guess_extension_from_url(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    // Path or query often includes type for CDN / Imagine assets.
    const PAIRS: &[(&str, &str)] = &[
        (".png", "png"),
        (".jpg", "jpg"),
        (".jpeg", "jpeg"),
        (".webp", "webp"),
        (".gif", "gif"),
        (".mp4", "mp4"),
        (".webm", "webm"),
        (".mov", "mov"),
        (".svg", "svg"),
        (".pdf", "pdf"),
        (".zip", "zip"),
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
        ("image/gif", "gif"),
        ("video/mp4", "mp4"),
    ];
    for (needle, ext) in PAIRS {
        if lower.contains(needle) {
            return Some(ext);
        }
    }
    None
}

fn filename_from_url(url: &str) -> Option<String> {
    let path_part = url.split(['?', '#']).next().unwrap_or(url);
    let raw = path_part.rsplit('/').next().unwrap_or("");
    if raw.is_empty() {
        return None;
    }
    // Basic percent-decoding for spaces / common sequences.
    let decoded = raw.replace("%20", " ").replace("%2f", "_").replace("%2F", "_");
    let name = sanitize_filename(&decoded);
    // Only trust URL segment if it looks like a real file name with extension.
    if std::path::Path::new(&name).extension().is_some() {
        Some(name)
    } else {
        None
    }
}

fn unique_path(dir: &std::path::Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let path = std::path::Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    for i in 1..10_000 {
        let name = if ext.is_empty() {
            format!("{stem} ({i})")
        } else {
            format!("{stem} ({i}).{ext}")
        };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
    }
    dir.join(file_name)
}

/// Resolve a safe absolute download path under Downloads.
/// Prefer WebView2's suggested file name (Content-Disposition), then URL, then a generated name.
fn resolve_download_path(url: &str, suggested: &std::path::Path) -> PathBuf {
    let dir = downloads_dir();

    let from_webview = suggested
        .file_name()
        .and_then(|n| n.to_str())
        .map(sanitize_filename)
        .filter(|n| {
            !n.is_empty()
                && !n.eq_ignore_ascii_case("download")
                && !n.eq_ignore_ascii_case("untitled")
                && !n.eq_ignore_ascii_case("untitled.bin")
        });

    let name = from_webview
        .or_else(|| filename_from_url(url))
        .unwrap_or_else(|| {
            let ext = guess_extension_from_url(url).unwrap_or("bin");
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("grok-imagine-{ts}.{ext}")
        });

    // If we still have no extension but the URL hints at one, append it.
    let name = if std::path::Path::new(&name).extension().is_none() {
        if let Some(ext) = guess_extension_from_url(url) {
            format!("{name}.{ext}")
        } else {
            name
        }
    } else {
        name
    };

    unique_path(&dir, &name)
}

fn default_window_title() -> String {
    format!("{APP_NAME} · v{APP_VERSION}")
}

fn set_window_title(window: &tao::window::Window, page_title: &str) {
    // Always include the package version so we can tell which build is running.
    // Page title from grok.com is used when present; avoid "Grok · Grok · v…".
    let title = if page_title.is_empty() || page_title.eq_ignore_ascii_case(APP_NAME) {
        default_window_title()
    } else {
        format!("{page_title} · v{APP_VERSION}")
    };
    window.set_title(&title);
}

/// Window / taskbar icon: prefer the icon embedded in the .exe (works when you
/// only ship the binary), then fall back to assets/icon.ico on disk.
fn load_window_icon() -> Option<Icon> {
    #[cfg(target_os = "windows")]
    {
        // winres embeds the app icon as resource id 1.
        if let Ok(icon) = Icon::from_resource(1, None) {
            return Some(icon);
        }

        let candidates = [
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/icon.ico"),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("assets/icon.ico")))
                .unwrap_or_default(),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("icon.ico")))
                .unwrap_or_default(),
        ];
        for path in candidates {
            if path.as_os_str().is_empty() {
                continue;
            }
            if let Ok(icon) = Icon::from_path(&path, None) {
                return Some(icon);
            }
        }
    }
    None
}

pub fn setup_webview() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let mut window_builder = WindowBuilder::new()
        .with_title(default_window_title())
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(640.0, 480.0));

    if let Some(icon) = load_window_icon() {
        window_builder = window_builder.with_window_icon(Some(icon));
    }

    let window = window_builder
        .build(&event_loop)
        .expect("Failed to create window");

    // Taskbar pin: AUMID + relaunch props on the HWND (process-level alone is often not enough).
    #[cfg(target_os = "windows")]
    {
        use tao::platform::windows::WindowExtWindows;
        crate::windows_app::set_window_relaunch_props(
            window.hwnd(),
            &crate::windows_app::current_exe_path(),
        );
    }

    let data_dir = user_data_dir();
    // Keep WebContext alive for the life of the WebView.
    let mut web_context = WebContext::new(Some(data_dir.clone()));

    let title_proxy = proxy.clone();
    let nav_proxy = proxy.clone();

    let builder = WebViewBuilder::new_with_web_context(&mut web_context)
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
            // WebView2 may already suggest a path (Content-Disposition). Prefer that
            // name; never use raw CDN path segments alone (Imagine often has no ext).
            let dest = resolve_download_path(&url, path);
            eprintln!("download started:\n  url:  {url}\n  file: {}", dest.display());
            *path = dest;
            true
        })
        .with_download_completed_handler(|url, path, success| {
            if success {
                if let Some(path) = path {
                    eprintln!("download complete: {}", path.display());
                } else {
                    eprintln!("download complete (no path): {url}");
                }
            } else {
                eprintln!("download failed: {url}");
            }
        })
        // No second window: open links / window.open in this shell.
        .with_new_window_req_handler(move |url, _features| {
            let _ = nav_proxy.send_event(UserEvent::Navigate(url));
            NewWindowResponse::Deny
        })
        // DevTools in debug builds (right-click Inspect / F12 with accelerator keys).
        .with_devtools(cfg!(debug_assertions));

    let webview = builder.build(&window).expect("Failed to build WebView");

    eprintln!(
        "{APP_NAME} v{APP_VERSION} started — profile: {} — {START_URL}",
        data_dir.display(),
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
