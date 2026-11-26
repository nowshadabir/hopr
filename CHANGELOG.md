# Changelog

## v1.4.8

- Windows: share one `ICoreWebView2Environment` (`webview2-env`) across all accounts using persistent named `ICoreWebView2Profile` instances (`win_profile_webview.rs`) to reduce duplicate browser/GPU/network process overhead.
- Windows: set `MemoryUsageTargetLevel::Low` on background live accounts and `Normal` on active accounts so notifications stay instant while idle memory is trimmed.
- Cross-platform: diff-based sidebar DOM updates (`sidebar.html`), `sync_ui` payload deduplication (`main.rs`), and background `inject.js` status polling throttling (`15s` when hidden).

## v1.4.7

- Add `sidebar_ready` IPC handshake so Windows WebView2 always populates the sidebar on startup.
- Use a dedicated `ui-webview` `WebContext` for sidebar, settings, and lock WebViews on Windows.
- Set explicit Windows window/taskbar icon (`with_window_icon`) and kill running `Hopr.exe` before NSIS Setup upgrade.

## v1.4.6

- Fix Windows WebView2 IPC (`window.ipc.postMessage`) in `sidebar.html`, `settings.html`, and `inject.js` so sidebar, Settings, and WhatsApp Web badges/notifications work on Windows.
- Fix Windows `Window -> Close Window` menu item and `Ctrl+W` shortcut.
- Reduce Windows WebView2 RAM usage via Chromium memory/feature flags and releasing `WebContext` on hibernation.

## v1.4.5

- Fix Windows CI release pipeline (`cargo test --release`) so `Hopr-1.4.5-win-x64.exe` and `Hopr-1.4.5-Setup.exe` build and publish alongside `Hopr-1.4.5.dmg`.
- Includes all v1.4.4 fixes (window dragging, account reordering, inline Remove confirmation, and Windows icon planes).

## v1.4.4

- Fix window dragging on macOS from sidebar, settings, and lock screen areas.
- Add account reordering (drag-and-drop in sidebar, up/down buttons in Settings).
- Fix Settings account Remove button using inline confirmation instead of `window.confirm()`.
- Fix `assets/hopr.ico` icon planes for Windows resource compilation.

## v1.4.2

- Fix NSIS path in Windows CI workflow to package `Hopr-1.4.2-Setup.exe` alongside `Hopr-1.4.2-win-x64.exe` and `Hopr-1.4.2.dmg`.

## v1.4.1

- Fix Windows CI compilation (`Win32_Security` feature and Unix-only permissions test guard) so `Hopr-1.4.1-win-x64.exe` and `Hopr-1.4.1-Setup.exe` build alongside `Hopr-1.4.1.dmg`.

## v1.4.0

- Custom branded HTML release notes in the Sparkle auto-updater UI.
- Added full Windows support (isolated Edge WebView2 profiles, Windows Toast notifications, single-instance mutex, `.ico` app icon).
- Added Windows CI builds producing both `Hopr-1.4.0-win-x64.exe` and `Hopr-1.4.0-Setup.exe` alongside `Hopr-1.4.0.dmg`.

## v1.3.0

- Redesigned two-pane Settings UI with account cards, status pills, lifecycle controls, reminders, and light/dark themes.
- Fixed WebView auto-hibernation idle tracking and explicit `WKWebView` teardown so hibernated accounts release their `WebContent` processes.
- Added microphone and camera entitlements/usage descriptions for WhatsApp voice and video calls.
- Centered `logo.svg` (`1024x1024`) with transparent tray and Dock icons.
- Enabled Sparkle framework loading under ad-hoc Hardened Runtime (`disable-library-validation`).

## v1.0.0

- Initial public release of **Hopr** for macOS 14.0+.
- Isolated per-account `WKWebsiteDataStore` sessions (persistent logins, no QR rescan).
- Smart WebView lifecycle (`Active`, `Background`, `Hibernated`), max live limit, and periodic wake.
- Collapsible sidebar account groups, pinned accounts, per-account notification sounds, local reminders, and Touch ID account lock.
- Sparkle 2 automatic updates with EdDSA signature verification.
