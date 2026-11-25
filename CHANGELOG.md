# Changelog

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
