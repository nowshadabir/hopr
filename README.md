<h1 align="center">Hopr</h1> <p align="center"> <strong>Hop between WhatsApp accounts on your Mac, without the bloat.</strong><br> A small, native macOS app written in Rust, built by <a href="https://vivagotechnologies.com">Vivago Technologies</a>. </p> <p align="center"> <a href="LICENSE"><img alt="License: Apache 2.0" src="https://img.shields.io/badge/license-Apache%202.0-blue.svg"></a> <img alt="Platform: macOS 14+" src="https://img.shields.io/badge/macOS-14%2B-lightgrey.svg"> <img alt="Built with Rust" src="https://img.shields.io/badge/built%20with-Rust-orange.svg"> </p>
What is Hopr?

Hopr lets you use multiple WhatsApp accounts side by side in one lightweight Mac app. Each account runs in its own isolated session, so you log in once and stay logged in, with no QR rescans after a restart.

Hopr wraps the official WhatsApp Web client. It does not reimplement the WhatsApp protocol, does not read your messages, and does not automate anything.

Hopr is not affiliated with, endorsed by, or sponsored by WhatsApp or Meta. "WhatsApp" is a trademark of its respective owner.

Why Hopr?

Most multi-account wrappers keep every account alive all the time. Hopr's goal is to be small itself and to let you decide which accounts are awake.

A tiny shell. The app is about 5 MB on disk and measured around 30 MB of memory for its own process, with near-zero idle CPU.
Smart sleep. Accounts you don't use can hibernate: the webview is released while the login is kept on disk. You choose which accounts are "Always connected".
Honest about WhatsApp Web. A live account is still a full WhatsApp Web page, and WhatsApp Web itself can use a lot of memory (often several hundred MB, sometimes over 1 GB with a large chat history). Hopr can't make that smaller; it helps you avoid keeping accounts live that you don't need.
Features
Isolated sessions. One WKWebsiteDataStore per account. No shared cookies or storage.
Native notifications with optional per-account sounds and a privacy toggle for message previews.
Unread badges per account and a total on the Dock icon.
Lifecycle control. Active, Background and Hibernated states, auto-hibernate timer, a limit on live webviews, Low Power Mode awareness, and optional periodic wake.
Menu bar item and keyboard shortcuts (⌘1–⌘9 to switch accounts, ⌘N, ⌘,, ⌘M).
Touch ID lock per account.
Local reminders for follow-ups. They never send WhatsApp messages.
Custom names, groups and photos, stored on your device only.
Launch at login, hidden in the menu bar.
Auto-update through Sparkle, with signed updates.
Requirements
macOS 14 (Sonoma) or later. Per-account data stores need macOS 14+.
Apple Silicon or Intel (universal binary).
Install

Download the latest .dmg from the Releases page, drag Hopr to Applications, and open it. Updates are delivered automatically after that.

Build from source

You need Rust (stable) and Xcode command line tools.

bash
git clone https://github.com/nowshadabir/hopr.git
cd hopr

# Run a debug build
cargo run

# Build the signed, universal .app bundle with Sparkle embedded
./scripts/bundle.sh

# Package a DMG
./scripts/package-dmg.sh

Release signing and notarization need your own Apple Developer ID and Sparkle keys. See RELEASING.md. Contributors don't need them to build and run locally.

How it works
Rust shell (tao + wry)
 ├─ Sidebar webview            account rail, badges, groups
 ├─ Account webviews           one WKWebView per account, own data store
 ├─ Lifecycle manager          Active / Background / Hibernated
 ├─ Notification bridge        injected JS → native macOS notifications
 └─ Config                     ~/Library/Application Support/Hopr/
Area	Choice
Window and webview	tao + wry (WKWebView), no Tauri runtime
Menus and tray	muda, tray-icon
macOS APIs	objc2 crates (notifications, Touch ID, Dock badge)
Updates	Sparkle 2
Privacy and security
No telemetry, no analytics. Hopr doesn't phone home, apart from Sparkle's update check.
Your chats stay in WebKit. Hopr doesn't read, store or log message content. It only reads the page title (unread count), notification events, and your own profile name, photo and number for the sidebar.
Strict navigation. Only WhatsApp domains load in the app; other links open in your default browser. Pop-ups are blocked.
Validated bridge. Messages from the page to the app are origin-checked, size-limited, schema-checked and rate-limited.
Local files are kept in ~/Library/Application Support/Hopr/ with owner-only permissions.
Signed, notarized builds with Hardened Runtime, and signed updates.

To report a vulnerability, please don't open a public issue. Email hello@getvivago with the details.

Fair use

Hopr is a wrapper around the official WhatsApp Web client for personal and business use. Please don't use it to send bulk or automated messages, scrape chats, or bypass WhatsApp's rules. Doing so can get your accounts banned, and Hopr isn't built for it.

Contributing

Contributions are welcome.

Open an issue to discuss larger changes first.
Fork the repo and create a branch.
Run cargo fmt and cargo clippy before you commit.
Open a pull request describing what changed and how you tested it.

Good first areas: memory and CPU profiling, hibernation edge cases, a native sidebar, and localization.

By contributing, you agree that your contributions are licensed under the Apache License 2.0.

Roadmap
[ ] Memory-based recycling of heavy background accounts
[ ] Memory pressure and "away" mode hibernation
[ ] Memory dashboard per account
[ ] Native (AppKit) sidebar
[ ] Quiet hours and workspaces
License

Copyright 2026 Vivago Technologies.

Licensed under the Apache License, Version 2.0. You may not use this project except in compliance with the License. Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, without warranties or conditions of any kind.

Third-party components keep their own licenses, including Sparkle (MIT-style) and the Rust crates listed in Cargo.toml.

<p align="center">Made by <a href="https://vivagotechnologies.com">Vivago Technologies</a></p>
