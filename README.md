# Grok Browser

A minimal Windows desktop shell for [grok.com](https://grok.com), built with [wry](https://github.com/tauri-apps/wry) (WebView2).

## Requirements

- Windows 10/11
- [Rust](https://rustup.rs/) (stable)
- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (usually preinstalled on modern Windows)

## Build & run

```powershell
cargo run
```

Release build (no console window):

```powershell
cargo build --release
.\target\release\grok-browser.exe
```

## Data directory

Browser profile data (cookies, cache, service workers) is stored under:

```text
%LOCALAPPDATA%\GrokBrowser\WebView2
```

This is **outside** the Cargo `target/` tree so debug builds do not accumulate an unbounded WebView2 profile next to the executable.

To wipe login/session/cache and start fresh:

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\GrokBrowser"
```

### Legacy profile cleanup

Older builds wrote the profile next to the debug binary. You can reclaim space with:

```powershell
Remove-Item -Recurse -Force ".\target\debug\grok-browser.exe.WebView2" -ErrorAction SilentlyContinue
# Or fully clean build artifacts:
cargo clean
```

You will need to sign in again after moving to the new data directory (profiles are not migrated automatically).

## Features

- Loads https://grok.com in a native window
- Persistent profile under LocalAppData
- Page title synced to the window title
- Clipboard and zoom hotkeys enabled
- Downloads default to your Downloads folder
- New-window popups blocked (navigation stays in the same window)
- DevTools enabled only in debug builds

## License

MIT — see [LICENSE](LICENSE).
