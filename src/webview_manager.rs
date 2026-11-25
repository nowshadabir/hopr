use crate::config::AccountConfig;
use crate::macos::open_external_url;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tao::event_loop::EventLoopProxy;
use tao::window::Window;
use uuid::Uuid;
use wry::{Rect, WebView, WebViewBuilder, WebViewBuilderExtDarwin};

const WHATSAPP_URL: &str = "https://web.whatsapp.com";
const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const INJECT_JS: &str = include_str!("../assets/inject.js");
const WAKE_WINDOW_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub enum AppEvent {
    WebViewIpc { account_id: Uuid, body: String },
    UiIpc { body: String },
    LifecycleTick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleState {
    Active,
    Background,
    Hibernated,
}

pub struct AccountRuntime {
    pub webview: Option<WebView>,
    pub lifecycle: LifecycleState,
    pub unread: u32,
    pub connection_state: String,
    pub last_active: Instant,
    pub last_woke: Option<Instant>,
    pub waking_until: Option<Instant>,
}

impl Default for AccountRuntime {
    fn default() -> Self {
        Self {
            webview: None,
            lifecycle: LifecycleState::Hibernated,
            unread: 0,
            connection_state: "hibernated".to_string(),
            last_active: Instant::now(),
            last_woke: None,
            waking_until: None,
        }
    }
}

#[derive(Serialize)]
pub struct AccountView<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub label: &'a str,
    pub phone: Option<&'a str>,
    pub avatar_url: Option<&'a str>,
    pub has_custom_avatar: bool,
    pub color: &'a str,
    pub unread: u32,
    pub muted: bool,
    pub keep_connected: bool,
    pub sleep_summary: String,
    pub short_sleep_summary: String,
    pub connection_state: String,
    pub lifecycle: LifecycleState,
    pub is_active: bool,
}

pub struct WebViewManager {
    runtimes: HashMap<Uuid, AccountRuntime>,
    active_id: Option<Uuid>,
}

impl WebViewManager {
    pub fn new() -> Self {
        Self {
            runtimes: HashMap::new(),
            active_id: None,
        }
    }

    pub fn active_id(&self) -> Option<Uuid> {
        self.active_id
    }

    pub fn total_unread(&self, accounts: &[AccountConfig]) -> u32 {
        accounts
            .iter()
            .filter(|a| !a.muted)
            .filter_map(|a| self.runtimes.get(&a.id))
            .map(|r| r.unread)
            .sum()
    }

    pub fn set_unread(&mut self, id: Uuid, count: u32) {
        let rt = self.runtimes.entry(id).or_default();
        rt.unread = count;
    }

    pub fn set_connection_state(&mut self, id: Uuid, state: String) {
        let rt = self.runtimes.entry(id).or_default();
        rt.connection_state = state;
    }

    pub fn set_active_visible(&self, visible: bool) {
        if let Some(active_id) = self.active_id
            && let Some(rt) = self.runtimes.get(&active_id)
            && rt.lifecycle == LifecycleState::Active
            && let Some(wv) = &rt.webview
        {
            let _ = wv.set_visible(visible);
        }
    }

    pub fn switch_to(
        &mut self,
        target_id: Uuid,
        accounts: &[AccountConfig],
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        bounds: Rect,
        visible: bool,
    ) -> Result<(), String> {
        if let Some(prev_id) = self.active_id
            && prev_id != target_id
            && let Some(prev) = self.runtimes.get_mut(&prev_id)
        {
            if let Some(wv) = &prev.webview {
                let _ = wv.set_visible(false);
                prev.lifecycle = LifecycleState::Background;
                prev.last_active = Instant::now();
            } else {
                prev.lifecycle = LifecycleState::Hibernated;
            }
        }

        let Some(acct) = accounts.iter().find(|a| a.id == target_id) else {
            return Err("Account not found".to_string());
        };

        self.ensure_webview(acct, window, proxy, bounds, visible)?;
        let rt = self.runtimes.entry(target_id).or_default();
        if let Some(wv) = &rt.webview {
            let _ = wv.set_bounds(bounds);
            let _ = wv.set_visible(visible);
            if visible {
                let _ = wv.focus();
            }
        }
        rt.lifecycle = LifecycleState::Active;
        rt.last_active = Instant::now();
        rt.waking_until = None;
        self.active_id = Some(target_id);
        Ok(())
    }

    fn ensure_webview(
        &mut self,
        acct: &AccountConfig,
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        bounds: Rect,
        visible: bool,
    ) -> Result<(), String> {
        let rt = self.runtimes.entry(acct.id).or_default();
        if rt.webview.is_some() {
            return Ok(());
        }

        let account_id = acct.id;
        let ipc_proxy = proxy.clone();

        let webview = WebViewBuilder::new()
            .with_url(WHATSAPP_URL)
            .with_data_store_identifier(acct.uuid_bytes())
            .with_user_agent(SAFARI_UA)
            .with_initialization_script(INJECT_JS)
            .with_visible(visible)
            .with_bounds(bounds)
            .with_ipc_handler(move |req| {
                let _ = ipc_proxy.send_event(AppEvent::WebViewIpc {
                    account_id,
                    body: req.into_body(),
                });
            })
            .with_navigation_handler(|url| allow_whatsapp_or_open_external(&url))
            .build_as_child(window)
            .map_err(|e| e.to_string())?;

        rt.webview = Some(webview);
        rt.lifecycle = if visible {
            LifecycleState::Active
        } else {
            LifecycleState::Background
        };
        rt.connection_state = "loading".to_string();
        Ok(())
    }

    pub fn hibernate(&mut self, id: Uuid) {
        if let Some(rt) = self.runtimes.get_mut(&id) {
            rt.webview = None;
            rt.lifecycle = LifecycleState::Hibernated;
            rt.connection_state = "hibernated".to_string();
            rt.waking_until = None;
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        self.runtimes.remove(&id);
        if self.active_id == Some(id) {
            self.active_id = None;
        }
    }

    pub fn hibernate_all_inactive(&mut self) {
        let ids: Vec<Uuid> = self
            .runtimes
            .keys()
            .copied()
            .filter(|id| Some(*id) != self.active_id)
            .collect();
        for id in ids {
            self.hibernate(id);
        }
    }

    pub fn tick_lifecycle(
        &mut self,
        accounts: &[AccountConfig],
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        bounds: Rect,
    ) -> bool {
        let now = Instant::now();
        let mut changed = false;

        for acct in accounts {
            if Some(acct.id) == self.active_id {
                continue;
            }
            let rt = self.runtimes.entry(acct.id).or_default();

            match rt.lifecycle {
                LifecycleState::Background => {
                    if let Some(until) = rt.waking_until {
                        if now >= until {
                            rt.webview = None;
                            rt.lifecycle = LifecycleState::Hibernated;
                            rt.connection_state = "hibernated".to_string();
                            rt.waking_until = None;
                            changed = true;
                        }
                    } else if !acct.keep_connected
                        && acct.hibernate_after_mins > 0
                        && now.duration_since(rt.last_active)
                            >= Duration::from_secs(acct.hibernate_after_mins * 60)
                    {
                        rt.webview = None;
                        rt.lifecycle = LifecycleState::Hibernated;
                        rt.connection_state = "hibernated".to_string();
                        changed = true;
                    }
                }
                LifecycleState::Hibernated => {
                    if acct.periodic_wake_mins > 0 {
                        let due = rt
                            .last_woke
                            .map(|t| {
                                now.duration_since(t)
                                    >= Duration::from_secs(acct.periodic_wake_mins * 60)
                            })
                            .unwrap_or(true);
                        if due
                            && self
                                .ensure_webview(acct, window, proxy, bounds, false)
                                .is_ok()
                        {
                            if let Some(rt_after) = self.runtimes.get_mut(&acct.id) {
                                rt_after.last_woke = Some(now);
                                rt_after.waking_until =
                                    Some(now + Duration::from_secs(WAKE_WINDOW_SECS));
                            }
                            changed = true;
                        }
                    }
                }
                LifecycleState::Active => {}
            }
        }

        changed
    }

    pub fn resize_all(&self, bounds: Rect) {
        for rt in self.runtimes.values() {
            if let Some(wv) = &rt.webview {
                let _ = wv.set_bounds(bounds);
            }
        }
    }

    pub fn account_views<'a>(&self, accounts: &'a [AccountConfig]) -> Vec<AccountView<'a>> {
        accounts
            .iter()
            .map(|acct| {
                let rt = self.runtimes.get(&acct.id);
                AccountView {
                    id: acct.id,
                    name: acct.effective_name(),
                    label: &acct.label,
                    phone: acct.phone.as_deref(),
                    avatar_url: acct.effective_avatar(),
                    has_custom_avatar: acct.custom_avatar_url.is_some(),
                    color: &acct.color,
                    unread: rt.map(|r| r.unread).unwrap_or(0),
                    muted: acct.muted,
                    keep_connected: acct.keep_connected,
                    sleep_summary: acct.sleep_summary(),
                    short_sleep_summary: acct.short_sleep_summary(),
                    connection_state: rt
                        .map(|r| r.connection_state.clone())
                        .unwrap_or_else(|| "hibernated".to_string()),
                    lifecycle: rt.map(|r| r.lifecycle).unwrap_or(LifecycleState::Hibernated),
                    is_active: self.active_id == Some(acct.id),
                }
            })
            .collect()
    }
}

fn allow_whatsapp_or_open_external(url: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    let allowed = host == "web.whatsapp.com"
        || host.ends_with(".whatsapp.com")
        || host.ends_with(".whatsapp.net")
        || host.ends_with(".fbcdn.net");

    if !allowed && (url.starts_with("http://") || url.starts_with("https://")) {
        open_external_url(url);
        return false;
    }
    allowed
}
