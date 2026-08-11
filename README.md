# Grok Browser

A minimal Windows desktop shell for [grok.com](https://grok.com), built with [wry](https://github.com/tauri-apps/wry) 0.55 + [tao](https://github.com/tauri-apps/tao) (WebView2).

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

The app icon lives at `assets/icon.ico` and is embedded into the Windows `.exe` at build time (Explorer, taskbar, window title bar).

## Install for daily use (recommended)

`target\release\` is temporary build output (deleted by `cargo clean`). For a stable path and taskbar pin, copy the release binary into Local AppData:

```powershell
cargo build --release
New-Item -ItemType Directory -Force -Path "$env:LOCALAPPDATA\GrokBrowser\bin" | Out-Null
Copy-Item target\release\grok-browser.exe "$env:LOCALAPPDATA\GrokBrowser\bin\Grok.exe" -Force
```

Optional: create/update the Start Menu shortcut used for **Pin to taskbar** (AppUserModelID-matched):

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-start-shortcut.ps1
```

Then pin **Grok Desktop** from the Start menu (not the Brave Apps “Grok” PWA entry).

### After you change the code

Rebuild and overwrite the installed copy (your taskbar pin can stay):

```powershell
cargo build --release
Copy-Item target\release\grok-browser.exe "$env:LOCALAPPDATA\GrokBrowser\bin\Grok.exe" -Force
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

Older builds wrote the profile next to the debug binary. Reclaim space with:

```powershell
Remove-Item -Recurse -Force ".\target\debug\grok-browser.exe.WebView2" -ErrorAction SilentlyContinue
cargo clean
```

## Features

- Loads https://grok.com in a native window (no address bar)
- Persistent profile under LocalAppData
- Page title synced to the window title
- Clipboard and zoom hotkeys enabled
- Downloads go to your Downloads folder (unique names; works with Imagine CDN URLs)
- `window.open` / target=_blank open in the same window
- Host init script adds `name` on anonymous form fields (quieter autofill audits)
- DevTools enabled in debug builds

## DevTools Issues panel notes

Some items under **Issues** are from **grok.com’s own page**, not this shell:

| Issue | Source | Can the shell fix it? |
|-------|--------|------------------------|
| CSP blocks `eval` | Grok’s Content-Security-Policy | No (by design; weakening CSP would be worse) |
| Form field missing id/name | Grok HTML | Partially (we inject `name` on empty fields) |
| History item marked skippable | SPA `pushState` without a user gesture | No (Chromium policy for SPAs) |

If chat works after login, treat remaining CSP/history Issues as site noise.

## License

MIT — see [LICENSE](LICENSE).
