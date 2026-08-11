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
/// - `window.__grokSaveUrl` for Imagine / CDN assets opened via window.open
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

  // Save a remote/blob URL as a file download (triggers WebView2 DownloadStarting).
  // Imagine often uses window.open(cdnUrl) which our host maps here instead of navigating away.
  window.__grokSaveUrl = function (url, filename) {
    filename = filename || "grok-imagine.png";
    function clickDownload(href, name) {
      var a = document.createElement("a");
      a.href = href;
      a.download = name;
      a.rel = "noopener";
      a.style.display = "none";
      document.body.appendChild(a);
      a.click();
      a.remove();
    }
    // blob: / data: — direct download attribute works.
    if (/^(blob:|data:)/i.test(url)) {
      clickDownload(url, filename);
      return Promise.resolve(true);
    }
    // Signed CDN URLs usually allow CORS GET without cookies (auth in query string).
    return fetch(url, { credentials: "omit", mode: "cors", cache: "no-store" })
      .then(function (res) {
        if (!res.ok) throw new Error("HTTP " + res.status);
        return res.blob();
      })
      .then(function (blob) {
        var objectUrl = URL.createObjectURL(blob);
        clickDownload(objectUrl, filename);
        setTimeout(function () { URL.revokeObjectURL(objectUrl); }, 60000);
        return true;
      })
      .catch(function () {
        // Last resort: download attribute on cross-origin (browser may still download
        // when Content-Disposition is attachment; otherwise may navigate — host avoids that).
        clickDownload(url, filename);
        return false;
      });
  };

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
    /// `window.open` / target=_blank for normal pages → load in this window.
    Navigate(String),
    /// Media / blob / Imagine asset → save via in-page download helper (don't navigate).
    SaveUrl(String),
    /// Notify UI after WebView2 finishes a download.
    DownloadFinished {
        url: String,
        path: Option<PathBuf>,
        success: bool,
    },
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
    } else if cleaned.len() > 180 {
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

fn guess_extension_from_url(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
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
    let decoded = raw.replace("%20", " ").replace("%2f", "_").replace("%2F", "_");
    let name = sanitize_filename(&decoded);
    if std::path::Path::new(&name).extension().is_some() {
        Some(name)
    } else {
        None
    }
}

fn generated_download_name(url: &str) -> String {
    let ext = guess_extension_from_url(url).unwrap_or("png");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("grok-imagine-{ts}.{ext}")
}

/// Suggested file name only (for the OS Save dialog).
fn suggested_filename(url: &str, webview_path: &std::path::Path) -> String {
    let from_webview = webview_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(sanitize_filename)
        .filter(|n| {
            !n.is_empty()
                && !n.eq_ignore_ascii_case("download")
                && !n.eq_ignore_ascii_case("untitled")
                && !n.eq_ignore_ascii_case("untitled.bin")
        });

    let mut name = from_webview
        .or_else(|| filename_from_url(url))
        .unwrap_or_else(|| generated_download_name(url));

    if std::path::Path::new(&name).extension().is_none() {
        let ext = guess_extension_from_url(url).unwrap_or("png");
        name = format!("{name}.{ext}");
    }
    name
}

/// Native Windows/macOS/Linux "Save As" dialog — let the OS pick the path.
fn os_save_as_dialog(suggested_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Save file")
        .set_file_name(suggested_name)
        .set_directory(downloads_dir())
        .add_filter("PNG image", &["png"])
        .add_filter("JPEG image", &["jpg", "jpeg"])
        .add_filter("WebP image", &["webp"])
        .add_filter("GIF image", &["gif"])
        .add_filter("Video", &["mp4", "webm", "mov"])
        .add_filter("All files", &["*"])
        .save_file()
}

/// URLs that should be saved as files, not opened as a full page navigation.
/// Imagine's download control often does window.open(cdnUrl) / target=_blank.
fn should_save_instead_of_navigate(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    if u.starts_with("blob:") || u.starts_with("data:image") || u.starts_with("data:video") {
        return true;
    }
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return false;
    }

    // Keep first-party app pages as navigations (chat, imagine UI, auth).
    let is_app_page = (u.contains("grok.com/") || u.contains("://grok.com"))
        && !u.contains("blob.core.windows.net");
    let is_auth = u.contains("accounts.x.ai") || u.contains("/auth");
    if is_app_page || is_auth {
        return false;
    }

    // CDN / binary asset hosts and explicit media types in the URL.
    if u.contains("blob.core.windows.net")
        || u.contains("media.x.ai")
        || u.contains("assets.x.ai")
        || u.contains("cdn.")
        || u.contains("format=png")
        || u.contains("format=jpeg")
        || u.contains("format=webp")
        || u.contains("mime=image")
    {
        return true;
    }

    // Path ends with a media extension (ignore query string).
    guess_extension_from_url(url).is_some()
}

fn js_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn app_log_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("GrokBrowser").join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn log_download(msg: &str) {
    eprintln!("{msg}");
    let path = app_log_dir().join("download.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// Download http(s) URL to an exact path the user chose in the OS Save dialog.
fn http_download_to(url: &str, dest: &std::path::Path) -> Result<u64, String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }

    let response = ureq::get(url)
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) GrokBrowser/0.3",
        )
        .set("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
        .call()
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }

    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("create file: {e}"))?;
    let bytes = std::io::copy(&mut reader, &mut file).map_err(|e| format!("write file: {e}"))?;
    if bytes == 0 {
        let _ = std::fs::remove_file(dest);
        return Err("empty response body".into());
    }
    Ok(bytes)
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
    let finished_proxy = proxy.clone();
    let save_proxy = proxy.clone();

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
            // Ask the OS where to save (native Save As), then let WebView2 write the file.
            let name = suggested_filename(&url, path);
            log_download(&format!(
                "download started url={url} suggested_name={name} webview_path={}",
                path.display()
            ));
            match os_save_as_dialog(&name) {
                Some(dest) => {
                    log_download(&format!("user chose save path {}", dest.display()));
                    *path = dest;
                    true
                }
                None => {
                    log_download("user cancelled Save As dialog");
                    false
                }
            }
        })
        .with_download_completed_handler(move |url, path, success| {
            log_download(&format!(
                "download finished success={success} url={url} path={:?}",
                path.as_ref().map(|p| p.display().to_string())
            ));
            let _ = finished_proxy.send_event(UserEvent::DownloadFinished {
                url,
                path,
                success,
            });
        })
        // No real second window. Auth links navigate in-place; media/Imagine assets save.
        .with_new_window_req_handler(move |url, _features| {
            if should_save_instead_of_navigate(&url) {
                log_download(&format!("new-window -> save url={url}"));
                let _ = nav_proxy.send_event(UserEvent::SaveUrl(url));
            } else {
                log_download(&format!("new-window -> navigate url={url}"));
                let _ = nav_proxy.send_event(UserEvent::Navigate(url));
            }
            NewWindowResponse::Deny
        })
        // DevTools in debug builds (right-click Inspect / F12 with accelerator keys).
        .with_devtools(cfg!(debug_assertions));

    let webview = builder.build(&window).expect("Failed to build WebView");

    log_download(&format!(
        "{APP_NAME} v{APP_VERSION} started profile={} url={START_URL}",
        data_dir.display()
    ));

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
                    log_download(&format!("navigation failed url={url} err={err}"));
                }
            }
            Event::UserEvent(UserEvent::SaveUrl(url)) => {
                // blob:/data: must stay in the page (host can't fetch them).
                if url.starts_with("blob:") || url.starts_with("data:") {
                    let name = generated_download_name(&url);
                    let script = format!(
                        "window.__grokSaveUrl && window.__grokSaveUrl({}, {});",
                        js_string_literal(&url),
                        js_string_literal(&name)
                    );
                    log_download(&format!("blob/data save via page helper name={name}"));
                    if let Err(err) = webview.evaluate_script(&script) {
                        log_download(&format!("__grokSaveUrl script failed: {err}"));
                    }
                } else {
                    // OS Save As, then download the bytes ourselves (signed CDN URLs).
                    let name = generated_download_name(&url);
                    if let Some(dest) = os_save_as_dialog(&name) {
                        log_download(&format!(
                            "host download after Save As dest={} url={url}",
                            dest.display()
                        ));
                        let done = save_proxy.clone();
                        std::thread::spawn(move || {
                            let result = http_download_to(&url, &dest);
                            match result {
                                Ok(bytes) => {
                                    log_download(&format!(
                                        "host download ok bytes={bytes} dest={}",
                                        dest.display()
                                    ));
                                    let _ = done.send_event(UserEvent::DownloadFinished {
                                        url,
                                        path: Some(dest),
                                        success: true,
                                    });
                                }
                                Err(err) => {
                                    log_download(&format!("host download failed: {err}"));
                                    let _ = done.send_event(UserEvent::DownloadFinished {
                                        url,
                                        path: None,
                                        success: false,
                                    });
                                }
                            }
                        });
                    } else {
                        log_download("user cancelled Save As (host download path)");
                    }
                }
            }
            Event::UserEvent(UserEvent::DownloadFinished {
                url,
                path,
                success,
            }) => {
                if success {
                    if let Some(ref path) = path {
                        log_download(&format!("saved ok {}", path.display()));
                        let note = format!(
                            "console.log('[Grok] Saved to {}');",
                            path.display().to_string().replace('\\', "\\\\")
                        );
                        let _ = webview.evaluate_script(&note);
                    }
                } else {
                    log_download(&format!("save failed url={url}"));
                    let _ = webview.evaluate_script(
                        "console.warn('[Grok] Save failed — see %LOCALAPPDATA%\\\\GrokBrowser\\\\logs\\\\download.log');",
                    );
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
