# Grok Browser

A minimal Windows desktop shell for [grok.com](https://grok.com), built with [wry](https://github.com/tauri-apps/wry) (WebView2).

## Requirements

- Windows 10/11
- [Rust](https://rustup.rs/) (stable)
- WebView2 Runtime (usually preinstalled on modern Windows)

## Build & run

```powershell
cargo run
```

Release build:

```powershell
cargo build --release
.\target\release\grok-browser.exe
```

## Notes

- WebView2 stores its user data next to the executable by default (e.g. `target\debug\grok-browser.exe.WebView2`). That folder can grow large with cache and service workers; it is gitignored via `/target/`.
- The current build may inject a demo init script on page load; treat that as experimental.

## License

MIT (or as otherwise noted if a `LICENSE` file is added).
