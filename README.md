<h1 align="center">Hopr</h1> <p align="center"> <strong>Hop between WhatsApp accounts on your Mac and Windows PC, without the bloat.</strong><br> A small, native desktop app written in Rust, built by <a href="https://vivagotechnologies.com">Vivago Technologies</a>. </p> <p align="center"> <a href="LICENSE"><img alt="License: Apache 2.0" src="https://img.shields.io/badge/license-Apache%202.0-blue.svg"></a> <img alt="Platform: macOS 14+" src="https://img.shields.io/badge/macOS-14%2B-lightgrey.svg"> <img alt="Platform: Windows 10 and 11" src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4.svg"> <img alt="Built with Rust" src="https://img.shields.io/badge/built%20with-Rust-orange.svg"> </p>
What is Hopr?

Hopr lets you use multiple WhatsApp accounts side by side in one lightweight desktop app, on macOS and Windows. Each account runs in its own isolated session, so you log in once and stay logged in, with no QR rescans after a restart.

Hopr wraps the official WhatsApp Web client. It does not reimplement the WhatsApp protocol, does not read your messages, and does not automate anything.

Hopr is not affiliated with, endorsed by, or sponsored by WhatsApp or Meta. "WhatsApp" is a trademark of its respective owner.

Why Hopr?

Most multi-account wrappers keep every account alive all the time. Hopr's goal is to be small itself and to let you decide which accounts are awake.

A tiny shell. On macOS the app is about 5 MB on disk and measured around 30 MB of memory for its own process, with near-zero idle CPU. <!-- VERIFY: add Windows numbers once measured -->
Smart sleep. Accounts you don't use can hibernate: the webview is released while the login is kept on disk. You choose which accounts are "Always connected".
Honest about WhatsApp Web. A live account is still a full WhatsApp Web page, and WhatsApp Web itself can use a lot of memory (often several hundred MB, sometimes over 1 GB with a large chat history). Hopr can't make that smaller; it helps you avoid keeping accounts live that you don't need.
Features
Isolated sessions. Every account has its own cookies and storage. Nothing is shared between accounts.
Native notifications with a privacy toggle for message previews.
Unread badges per account, plus a total on the Dock icon (macOS) or the taskbar and tray (Windows).
Lifecycle control. Active, Background and Hibernated states, auto-hibernate timer, a limit on live webviews, battery-saver awareness, and optional periodic wake.
Menu bar (macOS) and system tray (Windows) item for quick access.
Keyboard shortcuts to switch accounts (⌘1–⌘9 on macOS, Ctrl+1–Ctrl+9 on Windows).
Local reminders for follow-ups. They never send WhatsApp messages.
Custom names, groups and photos, stored on your device only.
Launch at login, hidden in the menu bar or tray.
Per-account lock with Touch ID or your password on macOS.
<!-- VERIFY: check every Windows cell in this table against the real Windows build -->
Feature	macOS	Windows
Isolated sessions per account	✓	✓
Native notifications	✓	✓
Menu bar / tray item	✓	✓
Hibernate and lifecycle control	✓	✓
Launch at login	✓	✓
Account lock	Touch ID / password	Not yet
Automatic updates	✓ (Sparkle)	See Releases
Requirements
macOS: 14 (Sonoma) or later, Apple Silicon or Intel (universal binary).
Windows: Windows 10 or 11 (64-bit) with the Microsoft Edge WebView2 Runtime. It is preinstalled on Windows 11 and on up-to-date Windows 10. <!-- VERIFY: ARM64 support? -->
Install

Download the latest build for your system from the Releases page.

macOS: open the .dmg, drag Hopr to Applications, and launch it. Updates are delivered automatically after that.
Windows: run the .exe installer and launch Hopr from the Start menu. Check the Releases page for new versions. <!-- VERIFY: does the Windows build have an in-app updater? If yes, update this line, the feature table and the architecture table -->
Build from source

You need Rust (stable).

bash
git clone https://github.com/nowshadabir/hopr.git
cd hopr

# Run a debug build
cargo run

macOS: install the Xcode command line tools, then:

bash
# Build the signed, universal .app bundle with Sparkle embedded
./scripts/bundle.sh

# Package a DMG
./scripts/package-dmg.sh

Windows: install the Visual Studio C++ Build Tools and the WebView2 Runtime, then:

powershell
cargo build --release
<!-- VERIFY: add the Windows packaging command or script -->

Release signing and notarization need your own Apple Developer ID and Sparkle keys (macOS), and a code-signing certificate (Windows). See RELEASING.md. Contributors don't need any of these to build and run locally.

How it works
Rust shell (tao + wry)
 ├─ Sidebar webview            account rail, badges, groups
 ├─ Account webviews           one webview per account, own data store
 ├─ Lifecycle manager          Active / Background / Hibernated
 ├─ Notification bridge        injected JS → native notifications
 └─ Config                     per-user app data folder
Area	macOS	Windows
Window and webview	tao + wry (WKWebView)	tao + wry (WebView2)
Session isolation	One WKWebsiteDataStore per account	One WebView2 user-data folder per account
Menus and tray	muda, tray-icon	muda, tray-icon
Notifications and OS APIs	objc2 crates	Windows toast notifications
Updates	Sparkle 2	See Releases
Config location	~/Library/Application Support/Hopr/	%APPDATA%\Hopr\
<!-- VERIFY: confirm the Windows column (isolation method, notification API, config path, updater) -->

No Tauri runtime is used.

Privacy and security
No telemetry, no analytics. Hopr doesn't phone home, apart from the update check.
Your chats stay in the webview. Hopr doesn't read, store or log message content. It only reads the page title (unread count), notification events, and your own profile name, photo and number for the sidebar.
Strict navigation. Only WhatsApp domains load in the app; other links open in your default browser. Pop-ups are blocked.
Validated bridge. Messages from the page to the app are origin-checked, size-limited, schema-checked and rate-limited.
Local files stay in your user profile folder. On macOS they use owner-only permissions.
Signed builds. macOS builds are signed and notarized with Hardened Runtime, and updates are signed.

To report a vulnerability, please don't open a public issue. Email hello@getvivago.com with the details.

Fair use

Hopr is a wrapper around the official WhatsApp Web client for personal and business use. Please don't use it to send bulk or automated messages, scrape chats, or bypass WhatsApp's rules. Doing so can get your accounts banned, and Hopr isn't built for it.

Contributing

Contributions are welcome.

Open an issue to discuss larger changes first.
Fork the repo and create a branch.
Run cargo fmt and cargo clippy before you commit.
Open a pull request describing what changed, which platform you tested on, and how you tested it.

Good first areas: memory and CPU profiling, hibernation edge cases, a native sidebar, Windows polish, and localization.

By contributing, you agree that your contributions are licensed under the Apache License 2.0.

Roadmap
[ ] Memory-based recycling of heavy background accounts
[ ] Memory pressure and "away" mode hibernation
[ ] Memory dashboard per account
[ ] Native sidebar (AppKit on macOS)
[ ] Account lock on Windows
[ ] Quiet hours and workspaces
License

Copyright 2026 Vivago Technologies.

Licensed under the Apache License, Version 2.0. You may not use this project except in compliance with the License. Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, without warranties or conditions of any kind.

Third-party components keep their own licenses, including Sparkle (macOS updates) and the Rust crates listed in Cargo.toml. Windows and WebView2 are trademarks of Microsoft.

<p align="center">Made by <a href="https://vivagotechnologies.com">Vivago Technologies</a></p>
