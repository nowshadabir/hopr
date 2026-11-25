# Lightweight multi-account WhatsApp app for macOS in Rust

## The key decision

The Rust shell will use about 10-20 MB of RAM. The real cost is WhatsApp Web itself, which typically uses a few hundred MB per live account inside WebKit's content process. So the architecture should focus on **controlling webview lifecycles**, not shaving the shell.

You have two realistic paths:

| | A. Native shell + WhatsApp Web (recommended) | B. Reimplement the protocol in Rust |
|---|---|---|
| RAM per account | high (WebKit) | very low |
| Effort | weeks | many months (full UI, media, E2EE, sync) |
| Risk | low; same approach as Franz/Rambox/WebCatalog | unofficial API, account-ban risk, breaks when WhatsApp changes the protocol |
| Shippable as a product | yes | legally and operationally fragile |

I'd go with **A** and design it so the heavy parts can be put to sleep.

## Architecture

```
┌──────────────────────── Rust process (~15 MB) ────────────────────────┐
│                                                                       │
│  tao (window/event loop)      muda (menu)     tray-icon (menu bar)    │
│          │                                                            │
│  ┌───────▼────────┐   ┌──────────────────┐   ┌─────────────────────┐  │
│  │ App Core       │◄─►│ Account Manager  │   │ Notification Bridge │  │
│  │ (event bus,    │   │ (config, UUIDs)  │   │ (UNUserNotification │  │
│  │  state machine)│   └──────────────────┘   │  Center via objc2)  │  │
│  └───────┬────────┘                          └──────────▲──────────┘  │
│          │                                              │             │
│  ┌───────▼───────────────────────────────┐              │             │
│  │ WebView Manager (lifecycle policy)    │──── IPC ─────┘             │
│  │  Active | Background | Hibernated     │   (JSON: badge, notif,     │
│  └───────┬───────────────────────────────┘    link, status)           │
└──────────┼────────────────────────────────────────────────────────────┘
           │ wry (WKWebView), one per account, isolated data store
   ┌───────▼──────┐  ┌──────────────┐  ┌──────────────┐
   │ Account A    │  │ Account B    │  │ Account C    │
   │ store UUID-A │  │ store UUID-B │  │ store UUID-C │
   │ web.whatsapp │  │ web.whatsapp │  │ (hibernated) │
   └──────────────┘  └──────────────┘  └──────────────┘
```

## Components

**1. Shell: `tao` + `wry` (skip full Tauri)**
Tauri adds a command layer, bundler plumbing, and an asset pipeline you don't need. `tao` and `wry` give you a window and WKWebView with a small binary. Add `muda` for menus, `tray-icon` for the menu bar, and `objc2` crates for anything AppKit-specific.

**2. Session isolation: per-account `WKWebsiteDataStore`**
This is what makes multi-account work. Each account gets a UUID, and that UUID becomes the webview's data store identifier, so cookies, IndexedDB, and local storage are fully separate. Sessions survive restarts, so no QR rescan.

```rust
let id: [u8; 16] = account.uuid.into_bytes();

let webview = WebViewBuilder::new()
    .with_url("https://web.whatsapp.com")
    .with_data_store_identifier(id)            // macOS 14+ (WebViewBuilderExtDarwin)
    .with_user_agent(SAFARI_UA)                // WhatsApp Web rejects unknown UAs
    .with_initialization_script(include_str!("inject.js"))
    .with_ipc_handler(move |req| bus.send(Event::Ipc(acct_id, req.into_body())))
    .with_navigation_handler(|url| allow_only_whatsapp_else_open_in_browser(url))
    .build_as_child(&window)?;
```

Verify the exact API against the current wry docs. This requires **macOS 14+** as the minimum target. Older macOS only gives you one persistent store, which breaks isolation.

**3. WebView lifecycle manager (where the memory savings come from)**

| State | What it means | Memory | Notifications |
|---|---|---|---|
| Active | visible, focused | full | yes |
| Background | alive, hidden | full (WebKit throttles timers) | yes |
| Hibernated | webview destroyed, data store kept | ~0 | no, unless you wake it periodically |

Policy options to expose to the user:
- Per-account "Keep always connected" toggle (default for the main account).
- Auto-hibernate after N minutes of inactivity.
- Periodic wake: load the page for about 30 seconds every few minutes, let it sync, fire notifications, then sleep again. This is a good compromise for low-priority accounts.

**4. Injected JS bridge (`inject.js`)**
WKWebView doesn't give you web notifications or badge counts for free, so you inject a small script that:
- Overrides `window.Notification` to post `{type:"notif", title, body, tag}` to Rust via `window.webkit.messageHandlers.ipc`.
- Watches `document.title` with a `MutationObserver` to extract the unread count from `(3) WhatsApp`, then posts `{type:"badge", n:3}`.
- Optionally exposes connection state (QR screen vs. logged in) for the sidebar UI.

**5. Native notifications**
Rust receives the IPC message and posts it with `UNUserNotificationCenter` (via `objc2-user-notifications`). Clicking a notification focuses the right account's webview. Sum the badge counts across accounts for the Dock badge.

**6. Account manager and config**
Use a TOML or JSON file in `~/Library/Application Support/<app>/accounts.toml` with `serde`. It holds the UUID, display name, color, icon, order, mute flag, and hibernation policy. You don't need SQLite. WebKit already persists all the actual WhatsApp data.

**7. Sidebar UI**
- **Lightest:** native AppKit via `objc2-app-kit` (an `NSStackView` of account buttons with badges). No extra web process.
- **Fastest to build:** a tiny static-HTML webview as the sidebar. It costs roughly 30 MB extra but is much easier to style.

I'd start with the HTML sidebar and swap to native later if the memory matters.

**8. Security and navigation**
- Allowlist `web.whatsapp.com` and its CDN hosts in the navigation handler. Open everything else in the default browser.
- Handle downloads via the WKDownload delegate (needs a small `objc2` shim, since wry's download handler covers basics).
- Grant mic and camera permission prompts only if you support calls.

## Suggested crates

`tao`, `wry`, `muda`, `tray-icon`, `objc2` + `objc2-app-kit` + `objc2-user-notifications`, `serde`, `toml`, `uuid`, `directories`, `tokio` (only if you need timers; `tao`'s event loop with user events can handle most of it).

## Build and distribution

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

- Build a universal binary (`aarch64-apple-darwin` + `x86_64-apple-darwin`, then `lipo`).
- Bundle with `cargo-bundle` or a small script that builds the `.app`.
- Sign and notarize. Enable the Hardened Runtime with the network client entitlement.
- Ship as a DMG. The Mac App Store sandbox is workable, but notifications and custom UAs get fiddlier.

## Pitfalls to plan for

1. **WhatsApp Web changes.** Your injected selectors and title parsing can break, so keep the JS tiny and version it separately from the Rust binary (consider loading it from your own config).
2. **Terms of service.** Wrapping the official web client is the common, tolerated approach. Don't automate messages or scrape. That's where bans happen.
3. **Hidden webviews still cost RAM.** Only hibernation actually frees it.
4. **Honesty in marketing.** "Lightweight" is true of the shell, but each live account is still a WebKit page. Say "lightweight, with smart sleeping."

## Build order

1. Single window, one webview, Safari UA, persistent login.
2. Multiple accounts with data store identifiers, plus a sidebar to switch between them.
3. JS bridge: badge counts and native notifications.
4. Lifecycle manager: background, hibernate, and periodic wake.
5. Menu bar item, Dock badge, keyboard shortcuts (⌘1-9 to switch accounts), and per-account muting.
6. Signing, notarization, and auto-update (Sparkle via FFI, or a simple version-check).

If you'd like, I can sketch the `WebViewManager` state machine in Rust or write the `inject.js` bridge next.