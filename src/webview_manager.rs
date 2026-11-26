use crate::config::{AccountConfig, AppConfig};
use crate::platform::{is_low_power_mode, open_external_url};
#[cfg(target_os = "macos")]
use objc2::rc::Weak;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tao::event_loop::EventLoopProxy;
use tao::window::Window;
use uuid::Uuid;
#[cfg(target_os = "macos")]
use wry::{Rect, WebView, WebViewBuilder, WebViewBuilderExtDarwin, WebViewExtMacOS};
#[cfg(not(target_os = "macos"))]
use wry::{Rect, WebView};

#[cfg(target_os = "macos")]
type AccountWebView = WebView;
#[cfg(target_os = "windows")]
type AccountWebView = crate::win_profile_webview::WinProfileWebView;

const WHATSAPP_URL: &str = "https://web.whatsapp.com";
const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const INJECT_JS: &str = include_str!("../assets/inject.js");
const WAKE_WINDOW_SECS: u64 = 30;
const STAGGER_INTERVAL_MS: u64 = 1500;
const MAX_STAGGER_CONCURRENT: usize = 2;
const MAX_IPC_PER_SEC: u32 = 20;
const BACKOFF_SECS: &[u64] = &[5, 15, 60, 300];

#[derive(Debug, Clone)]
pub enum AppEvent {
    WebViewIpc {
        account_id: Uuid,
        origin: String,
        body: String,
    },
    UiIpc {
        body: String,
    },
    AuthResult(crate::auth::AuthOutcome),
    InvoiceResult(crate::invoices::InvoiceOutcome),
    LifecycleTick,
    FocusWindow,
    #[cfg(debug_assertions)]
    DebugTestStep(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleState {
    Active,
    Background,
    Hibernated,
}

struct RateLimiter {
    window_start: Instant,
    count: u32,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self {
            window_start: Instant::now(),
            count: 0,
        }
    }
}

impl RateLimiter {
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            self.window_start = now;
            self.count = 0;
        }
        self.count += 1;
        self.count <= MAX_IPC_PER_SEC
    }
}

pub struct AccountRuntime {
    pub webview: Option<AccountWebView>,
    pub lifecycle: LifecycleState,
    pub unread: u32,
    pub connection_state: String,
    pub needs_login: bool,
    pub needs_login_notified: bool,
    pub offline: bool,
    pub reconnecting: bool,
    pub backoff_step: usize,
    pub next_retry_at: Option<Instant>,
    pub last_active: Instant,
    pub last_user_interaction: Instant,
    pub last_woke: Option<Instant>,
    pub waking_until: Option<Instant>,
    rate_limiter: RateLimiter,
}

impl Default for AccountRuntime {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            webview: None,
            lifecycle: LifecycleState::Hibernated,
            unread: 0,
            connection_state: "hibernated".to_string(),
            needs_login: false,
            needs_login_notified: false,
            offline: false,
            reconnecting: false,
            backoff_step: 0,
            next_retry_at: None,
            last_active: now,
            last_user_interaction: now,
            last_woke: None,
            waking_until: None,
            rate_limiter: RateLimiter::default(),
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
    pub stale_unread: bool,
    pub muted: bool,
    pub keep_connected: bool,
    pub notif_sound: &'a str,
    pub group: Option<&'a str>,
    pub locked: bool,
    pub short_sleep_summary: String,
    pub connection_state: String,
    pub needs_login: bool,
    pub offline: bool,
    pub reconnecting: bool,
    pub lifecycle: LifecycleState,
    pub is_active: bool,
}

pub struct WebViewManager {
    runtimes: HashMap<Uuid, AccountRuntime>,
    active_id: Option<Uuid>,
    pending_loads: VecDeque<Uuid>,
    last_stagger_at: Option<Instant>,
    active_downloads: Arc<AtomicUsize>,
}

impl WebViewManager {
    pub fn new() -> Self {
        Self {
            runtimes: HashMap::new(),
            active_id: None,
            pending_loads: VecDeque::new(),
            last_stagger_at: None,
            active_downloads: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn seed_from_config(&mut self, accounts: &[AccountConfig]) {
        for acct in accounts {
            let rt = self.runtimes.entry(acct.id).or_default();
            rt.unread = acct.last_known_unread;
        }
    }

    pub fn active_id(&self) -> Option<Uuid> {
        self.active_id
    }

    pub fn active_downloads_count(&self) -> usize {
        self.active_downloads.load(Ordering::Relaxed)
    }

    pub fn allow_ipc(&mut self, id: Uuid, origin: &str) -> bool {
        if !is_trusted_ipc_origin(origin) {
            return false;
        }
        self.runtimes.entry(id).or_default().rate_limiter.allow()
    }

    pub fn total_unread(&self, accounts: &[AccountConfig]) -> u32 {
        accounts
            .iter()
            .filter(|a| !a.muted)
            .filter_map(|a| self.runtimes.get(&a.id))
            .filter(|r| r.lifecycle != LifecycleState::Hibernated)
            .map(|r| r.unread)
            .sum()
    }

    pub fn set_unread(&mut self, id: Uuid, count: u32) {
        let rt = self.runtimes.entry(id).or_default();
        rt.unread = count;
    }

    pub fn set_connection_state(&mut self, id: Uuid, state: String) -> bool {
        let rt = self.runtimes.entry(id).or_default();
        let is_qr = state == "qr";
        rt.connection_state = state;
        rt.needs_login = is_qr;
        if !is_qr {
            rt.needs_login_notified = false;
            rt.offline = false;
            rt.reconnecting = false;
            rt.backoff_step = 0;
            rt.next_retry_at = None;
            return false;
        }
        if !rt.needs_login_notified {
            rt.needs_login_notified = true;
            return true;
        }
        false
    }

    pub fn set_offline(&mut self, id: Uuid, offline: bool) {
        let rt = self.runtimes.entry(id).or_default();
        rt.offline = offline;
        if offline {
            rt.next_retry_at = None;
        } else if rt.reconnecting || rt.connection_state != "ready" {
            let delay = BACKOFF_SECS[rt.backoff_step.min(BACKOFF_SECS.len() - 1)];
            rt.next_retry_at = Some(Instant::now() + Duration::from_secs(delay));
            rt.backoff_step = (rt.backoff_step + 1).min(BACKOFF_SECS.len() - 1);
        }
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

    pub fn on_window_hidden_or_minimized(&mut self) {
        if let Some(active_id) = self.active_id
            && let Some(rt) = self.runtimes.get_mut(&active_id)
            && let Some(wv) = &rt.webview
        {
            let prev = rt.lifecycle;
            let _ = wv.set_visible(false);
            rt.lifecycle = LifecycleState::Background;
            rt.last_active = Instant::now();
            rt.last_user_interaction = Instant::now();
            log_transition(active_id, prev, LifecycleState::Background, "window_hidden_or_minimized", 0);
        }
    }

    pub fn on_window_restored(
        &mut self,
        accounts: &[AccountConfig],
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        bounds: Rect,
        max_live: usize,
        visible: bool,
    ) {
        if let Some(active_id) = self.active_id {
            let has_wv = self
                .runtimes
                .get(&active_id)
                .and_then(|r| r.webview.as_ref())
                .is_some();
            if has_wv {
                if let Some(rt) = self.runtimes.get_mut(&active_id)
                    && let Some(wv) = &rt.webview
                {
                    let prev = rt.lifecycle;
                    let _ = set_account_bounds(wv, bounds, window.scale_factor());
                    let _ = wv.set_visible(visible);
                    if visible {
                        let _ = wv.focus();
                    }
                    rt.lifecycle = LifecycleState::Active;
                    rt.last_active = Instant::now();
                    rt.last_user_interaction = Instant::now();
                    if prev != LifecycleState::Active {
                        log_transition(active_id, prev, LifecycleState::Active, "window_restored", 0);
                    }
                }
            } else {
                let _ = self.switch_to(active_id, accounts, window, proxy, bounds, max_live, visible);
            }
        }
    }

    pub fn queue_cold_start_staggered(
        &mut self,
        accounts: &[AccountConfig],
        skip_id: Option<Uuid>,
    ) {
        for acct in accounts {
            if Some(acct.id) == skip_id {
                continue;
            }
            if acct.keep_connected && !self.pending_loads.contains(&acct.id) {
                self.pending_loads.push_back(acct.id);
            }
        }
    }

    pub fn pump_staggered_loads(
        &mut self,
        accounts: &[AccountConfig],
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        bounds: Rect,
        max_live: usize,
    ) -> bool {
        if self.pending_loads.is_empty() {
            return false;
        }
        let now = Instant::now();
        if let Some(last) = self.last_stagger_at
            && now.duration_since(last) < Duration::from_millis(STAGGER_INTERVAL_MS)
        {
            return false;
        }

        let mut loaded_any = false;
        let mut count = 0;
        while count < MAX_STAGGER_CONCURRENT {
            let Some(next_id) = self.pending_loads.pop_front() else {
                break;
            };
            if let Some(acct) = accounts.iter().find(|a| a.id == next_id) {
                if self.ensure_webview(acct, window, proxy, bounds, false).is_ok() {
                    loaded_any = true;
                    count += 1;
                }
            }
        }

        if loaded_any {
            self.last_stagger_at = Some(now);
            self.enforce_max_live(accounts, max_live);
        }
        loaded_any
    }

    pub fn switch_to(
        &mut self,
        target_id: Uuid,
        accounts: &[AccountConfig],
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        bounds: Rect,
        max_live: usize,
        visible: bool,
    ) -> Result<(), String> {
        if let Some(prev_id) = self.active_id
            && prev_id != target_id
            && let Some(prev) = self.runtimes.get_mut(&prev_id)
        {
            let old_state = prev.lifecycle;
            if let Some(wv) = &prev.webview {
                let _ = wv.set_visible(false);
                prev.lifecycle = LifecycleState::Background;
                prev.last_active = Instant::now();
                log_transition(prev_id, old_state, LifecycleState::Background, "switched_away", 0);
            } else {
                prev.lifecycle = LifecycleState::Hibernated;
            }
        }

        let Some(acct) = accounts.iter().find(|a| a.id == target_id) else {
            return Err("Account not found".to_string());
        };

        self.pending_loads.retain(|id| *id != target_id);
        let prev_target_state = self
            .runtimes
            .get(&target_id)
            .map(|r| r.lifecycle)
            .unwrap_or(LifecycleState::Hibernated);
        self.ensure_webview(acct, window, proxy, bounds, visible)?;
        let rt = self.runtimes.entry(target_id).or_default();
        if let Some(wv) = &rt.webview {
            let _ = set_account_bounds(wv, bounds, window.scale_factor());
            let _ = wv.set_visible(visible);
            if visible {
                let _ = wv.focus();
            }
        }
        rt.lifecycle = LifecycleState::Active;
        rt.last_active = Instant::now();
        rt.last_user_interaction = Instant::now();
        rt.waking_until = None;
        log_transition(
            target_id,
            prev_target_state,
            LifecycleState::Active,
            "user_selected",
            0,
        );
        self.active_id = Some(target_id);
        self.enforce_max_live(accounts, max_live);
        Ok(())
    }

    fn enforce_max_live(&mut self, accounts: &[AccountConfig], max_live: usize) {
        let effective_max = if is_low_power_mode() {
            max_live.saturating_sub(1).max(1)
        } else {
            max_live.max(1)
        };

        loop {
            let live_count = self
                .runtimes
                .values()
                .filter(|r| r.webview.is_some())
                .count();
            if live_count <= effective_max {
                break;
            }

            let candidate = self
                .runtimes
                .iter()
                .filter(|(id, rt)| {
                    Some(**id) != self.active_id
                        && rt.webview.is_some()
                        && accounts
                            .iter()
                            .find(|a| a.id == **id)
                            .map(|a| !a.keep_connected)
                            .unwrap_or(true)
                })
                .min_by_key(|(_, rt)| rt.last_active)
                .map(|(id, _)| *id);

            let Some(victim) = candidate else {
                break;
            };
            self.hibernate(victim);
        }
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
        let dl_counter_start = Arc::clone(&self.active_downloads);
        let dl_counter_end = Arc::clone(&self.active_downloads);

        let target_url: String = {
            #[cfg(debug_assertions)]
            {
                std::env::var("HOPR_DEBUG_URL").unwrap_or_else(|_| WHATSAPP_URL.to_string())
            }
            #[cfg(not(debug_assertions))]
            {
                WHATSAPP_URL.to_string()
            }
        };

        #[cfg(target_os = "macos")]
        let webview = WebViewBuilder::new()
            .with_data_store_identifier(acct.uuid_bytes())
            .with_url(&target_url)
            .with_user_agent(SAFARI_UA)
            .with_devtools(cfg!(debug_assertions))
            .with_initialization_script(INJECT_JS)
            .with_visible(visible)
            .with_bounds(bounds)
            .with_ipc_handler(move |req| {
                let origin = req.uri().to_string();
                let _ = ipc_proxy.send_event(AppEvent::WebViewIpc {
                    account_id,
                    origin,
                    body: req.into_body(),
                });
            })
            .with_navigation_handler(|url| allow_whatsapp_or_open_external(&url))
            .with_new_window_req_handler(|url, _| {
                let _ = allow_whatsapp_or_open_external(&url);
                wry::NewWindowResponse::Deny
            })
            .with_download_started_handler(move |url, dest| {
                *dest = resolve_download_dest(dest, &url);
                dl_counter_start.fetch_add(1, Ordering::Relaxed);
                true
            })
            .with_download_completed_handler(move |_url, _dest, _success| {
                dl_counter_end.fetch_sub(1, Ordering::Relaxed);
            })
            .build_as_child(window)
            .map_err(|e| e.to_string())?;

        #[cfg(target_os = "windows")]
        let webview = {
            let _ = (&ipc_proxy, &dl_counter_end);
            crate::win_profile_webview::WinProfileWebView::create(
                account_id,
                &acct.id.to_string(),
                &target_url,
                SAFARI_UA,
                INJECT_JS,
                bounds,
                visible,
                window,
                proxy,
                dl_counter_start,
            )?
        };

        rt.webview = Some(webview);
        rt.lifecycle = if visible {
            LifecycleState::Active
        } else {
            LifecycleState::Background
        };
        rt.connection_state = "loading".to_string();
        rt.reconnecting = false;
        Ok(())
    }

    pub fn hibernate(&mut self, id: Uuid) {
        self.hibernate_with_reason(id, "manual_sleep");
    }

    pub fn hibernate_with_reason(&mut self, id: Uuid, reason: &str) {
        if let Some(rt) = self.runtimes.get_mut(&id) {
            let prev = rt.lifecycle;
            let idle_secs = Instant::now()
                .duration_since(rt.last_user_interaction)
                .as_secs();
            if let Some(wv) = rt.webview.take() {
                teardown_account_webview(wv, &format!("acct-{id}"));
            }
            rt.lifecycle = LifecycleState::Hibernated;
            rt.connection_state = "hibernated".to_string();
            rt.waking_until = None;
            if prev != LifecycleState::Hibernated {
                log_transition(id, prev, LifecycleState::Hibernated, reason, idle_secs);
            }
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        if let Some(mut rt) = self.runtimes.remove(&id)
            && let Some(wv) = rt.webview.take()
        {
            teardown_account_webview(wv, &format!("remove-{id}"));
        }
        self.pending_loads.retain(|x| *x != id);
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
            self.hibernate_with_reason(id, "hibernate_all_inactive");
        }
    }

    pub fn shutdown_all(&mut self) {
        self.pending_loads.clear();
        for (id, rt) in self.runtimes.iter_mut() {
            if let Some(wv) = rt.webview.take() {
                teardown_account_webview(wv, &format!("shutdown-{id}"));
            }
            rt.lifecycle = LifecycleState::Hibernated;
        }
    }

    pub fn tick_lifecycle(
        &mut self,
        cfg: &AppConfig,
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        bounds: Rect,
    ) -> bool {
        let now = Instant::now();
        let mut changed = false;

        if self.pump_staggered_loads(&cfg.accounts, window, proxy, bounds, cfg.max_live_webviews) {
            changed = true;
        }

        let wake_interval_mins = if cfg.periodic_wake_enabled && cfg.periodic_wake_mins > 0 {
            if is_low_power_mode() {
                cfg.periodic_wake_mins * 2
            } else {
                cfg.periodic_wake_mins
            }
        } else {
            0
        };

        for acct in &cfg.accounts {
            let is_live_active = self
                .runtimes
                .get(&acct.id)
                .map(|r| r.lifecycle == LifecycleState::Active)
                .unwrap_or(false);
            if is_live_active {
                if let Some(rt) = self.runtimes.get_mut(&acct.id)
                    && let Some(retry_at) = rt.next_retry_at
                    && now >= retry_at
                {
                    rt.next_retry_at = None;
                    rt.reconnecting = true;
                    if let Some(wv) = &rt.webview {
                        let _ = wv.load_url(WHATSAPP_URL);
                    }
                    changed = true;
                }
                continue;
            }
            let rt = self.runtimes.entry(acct.id).or_default();

            match rt.lifecycle {
                LifecycleState::Background => {
                    if let Some(until) = rt.waking_until {
                        if now >= until {
                            if let Some(wv) = rt.webview.take() {
                                teardown_account_webview(wv, &format!("wake-end-{}", acct.id));
                            }
                            rt.lifecycle = LifecycleState::Hibernated;
                            rt.connection_state = "hibernated".to_string();
                            rt.waking_until = None;
                            log_transition(
                                acct.id,
                                LifecycleState::Background,
                                LifecycleState::Hibernated,
                                "periodic_wake_window_ended",
                                now.duration_since(rt.last_user_interaction).as_secs(),
                            );
                            changed = true;
                        }
                    } else if !acct.keep_connected
                        && let Some(hibernate_dur) = debug_hibernate_duration(cfg)
                        && now.duration_since(rt.last_user_interaction) >= hibernate_dur
                    {
                        let idle_secs = now.duration_since(rt.last_user_interaction).as_secs();
                        if let Some(wv) = rt.webview.take() {
                            teardown_account_webview(wv, &format!("auto-idle-{}", acct.id));
                        }
                        rt.lifecycle = LifecycleState::Hibernated;
                        rt.connection_state = "hibernated".to_string();
                        log_transition(
                            acct.id,
                            LifecycleState::Background,
                            LifecycleState::Hibernated,
                            "auto_hibernate_idle",
                            idle_secs,
                        );
                        changed = true;
                    } else if let Some(retry_at) = rt.next_retry_at
                        && now >= retry_at
                    {
                        rt.next_retry_at = None;
                        rt.reconnecting = true;
                        if let Some(wv) = &rt.webview {
                            let _ = wv.load_url(WHATSAPP_URL);
                        }
                        changed = true;
                    }
                }
                LifecycleState::Hibernated => {
                    if wake_interval_mins > 0 {
                        let due = rt
                            .last_woke
                            .map(|t| {
                                now.duration_since(t) >= Duration::from_secs(wake_interval_mins * 60)
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

    pub fn resize_all(&self, bounds: Rect, scale_factor: f64) {
        for rt in self.runtimes.values() {
            if let Some(wv) = &rt.webview {
                let _ = set_account_bounds(wv, bounds, scale_factor);
            }
        }
    }

    pub fn load_url_on_active(&self, url: &str) -> bool {
        if let Some(id) = self.active_id
            && let Some(rt) = self.runtimes.get(&id)
            && let Some(wv) = &rt.webview
        {
            let _ = wv.load_url(url);
            return true;
        }
        false
    }

    pub fn account_views<'a>(
        &self,
        accounts: &'a [AccountConfig],
        global_hibernate_mins: u64,
        periodic_wake_enabled: bool,
        periodic_wake_mins: u64,
    ) -> Vec<AccountView<'a>> {
        accounts
            .iter()
            .map(|acct| {
                let rt = self.runtimes.get(&acct.id);
                let lifecycle = rt.map(|r| r.lifecycle).unwrap_or(LifecycleState::Hibernated);
                AccountView {
                    id: acct.id,
                    name: acct.effective_name(),
                    label: &acct.label,
                    phone: acct.phone.as_deref(),
                    avatar_url: acct.effective_avatar(),
                    has_custom_avatar: acct.custom_avatar_url.is_some(),
                    color: &acct.color,
                    unread: rt.map(|r| r.unread).unwrap_or(acct.last_known_unread),
                    stale_unread: lifecycle == LifecycleState::Hibernated,
                    muted: acct.muted,
                    keep_connected: acct.keep_connected,
                    notif_sound: &acct.notif_sound,
                    group: acct.group.as_deref(),
                    locked: acct.locked,
                    short_sleep_summary: acct.short_sleep_summary(
                        global_hibernate_mins,
                        periodic_wake_enabled,
                        periodic_wake_mins,
                    ),
                    connection_state: rt
                        .map(|r| r.connection_state.clone())
                        .unwrap_or_else(|| "hibernated".to_string()),
                    needs_login: rt.map(|r| r.needs_login).unwrap_or(false),
                    offline: rt.map(|r| r.offline).unwrap_or(false),
                    reconnecting: rt.map(|r| r.reconnecting).unwrap_or(false),
                    lifecycle,
                    is_active: self.active_id == Some(acct.id),
                }
            })
            .collect()
    }
}

pub fn log_transition(
    id: Uuid,
    from: LifecycleState,
    to: LifecycleState,
    reason: &str,
    idle_secs: u64,
) {
    eprintln!(
        "[lifecycle] acct={id} from={from:?} to={to:?} reason={reason} idle_secs={idle_secs}"
    );
}

fn debug_hibernate_duration(cfg: &AppConfig) -> Option<Duration> {
    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var("HOPR_DEBUG_HIBERNATE_SECS")
        && let Ok(secs) = raw.trim().parse::<u64>()
        && secs > 0
    {
        return Some(Duration::from_secs(secs));
    }
    if cfg.hibernate_after_mins > 0 {
        Some(Duration::from_secs(cfg.hibernate_after_mins * 60))
    } else {
        None
    }
}

fn set_account_bounds(wv: &AccountWebView, bounds: Rect, scale_factor: f64) {
    #[cfg(target_os = "macos")]
    {
        let _ = scale_factor;
        let _ = wv.set_bounds(bounds);
    }
    #[cfg(target_os = "windows")]
    {
        wv.set_bounds(bounds, scale_factor);
    }
}

fn teardown_account_webview(wv: AccountWebView, label: &str) {
    #[cfg(target_os = "macos")]
    {
        teardown_webview(wv, label);
    }
    #[cfg(not(target_os = "macos"))]
    {
        wv.load_url("about:blank");
        drop(wv);
        eprintln!("[teardown] label={label}");
    }
}

pub fn teardown_webview(wv: WebView, label: &str) {
    #[cfg(target_os = "macos")]
    {
        let wk = wv.webview();
        let mgr = wv.manager();
        let weak_wk = Weak::from_retained(&wk);

        let wk_rc_before: usize = unsafe { objc2::msg_send![&*wk, retainCount] };
        let mgr_rc_before: usize = unsafe { objc2::msg_send![&*mgr, retainCount] };

        unsafe {
            wk.stopLoading();
        }
        let _ = wv.load_url("about:blank");

        unsafe {
            mgr.removeAllScriptMessageHandlers();
            mgr.removeAllUserScripts();
            wk.setNavigationDelegate(None);
            wk.setUIDelegate(None);
            wk.removeFromSuperview();
        }

        drop(mgr);
        drop(wk);
        drop(wv);

        let after_alive = weak_wk.load();
        let wk_rc_after = after_alive
            .as_ref()
            .map(|obj| unsafe { objc2::msg_send![&**obj, retainCount] })
            .unwrap_or(0usize);
        eprintln!(
            "[teardown] label={label} wk_rc_before={wk_rc_before} mgr_rc_before={mgr_rc_before} wk_alive_after_drop={} wk_rc_after={wk_rc_after}",
            after_alive.is_some()
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = wv.load_url("about:blank");
        drop(wv);
        eprintln!("[teardown] label={label}");
    }
}

pub fn resolve_download_dest(suggested: &std::path::Path, url: &str) -> PathBuf {
    resolve_download_dest_in(suggested, url, &crate::platform::downloads_dir())
}

fn resolve_download_dest_in(
    suggested: &std::path::Path,
    url: &str,
    dir: &std::path::Path,
) -> PathBuf {
    let raw_name = suggested
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::path::Path::new(url.split('?').next().unwrap_or("download"))
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("whatsapp-download");

    let safe_name = raw_name.replace(['/', '\\', '\0'], "_");
    let candidate = dir.join(&safe_name);
    if !candidate.exists() {
        return candidate;
    }

    let path = std::path::Path::new(&safe_name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("whatsapp-download");
    let ext = path.extension().and_then(|s| s.to_str());

    for n in 1..1000 {
        let next_name = match ext {
            Some(e) if !e.is_empty() => format!("{stem} ({n}).{e}"),
            _ => format!("{stem} ({n})"),
        };
        let next = dir.join(next_name);
        if !next.exists() {
            return next;
        }
    }
    candidate
}

pub fn is_trusted_ipc_origin(origin: &str) -> bool {
    origin == "https://web.whatsapp.com"
        || origin.starts_with("https://web.whatsapp.com/")
}

pub fn allow_whatsapp_or_open_external(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("file:")
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("blob:")
    {
        return false;
    }

    let host = lower
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");

    let allowed = host == "web.whatsapp.com"
        || host.ends_with(".whatsapp.com")
        || host.ends_with(".whatsapp.net")
        || host.ends_with(".fbcdn.net");

    if !allowed && (lower.starts_with("http://") || lower.starts_with("https://")) {
        open_external_url(url);
        return false;
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_blocks_dangerous_schemes_and_allows_whatsapp() {
        assert!(allow_whatsapp_or_open_external("https://web.whatsapp.com/"));
        assert!(allow_whatsapp_or_open_external("https://pps.whatsapp.net/v/t61.jpg"));
        assert!(allow_whatsapp_or_open_external("https://scontent.fbcdn.net/img.png"));
        assert!(!allow_whatsapp_or_open_external("file:///etc/passwd"));
        assert!(!allow_whatsapp_or_open_external("javascript:alert(1)"));
        assert!(!allow_whatsapp_or_open_external("data:text/html,<script>alert(1)</script>"));
        assert!(!allow_whatsapp_or_open_external("blob:https://evil.com/123"));
    }

    #[test]
    fn ipc_origin_and_rate_limiter_enforce_limits() {
        assert!(is_trusted_ipc_origin("https://web.whatsapp.com"));
        assert!(is_trusted_ipc_origin("https://web.whatsapp.com/foo"));
        assert!(!is_trusted_ipc_origin("https://evil.com"));
        assert!(!is_trusted_ipc_origin("http://web.whatsapp.com"));

        let mut rl = RateLimiter::default();
        for _ in 0..MAX_IPC_PER_SEC {
            assert!(rl.allow());
        }
        assert!(!rl.allow());
    }

    #[test]
    fn download_dest_preserves_suggested_filename_over_blob_url() {
        let tmp = std::env::temp_dir().join(format!("hopr-dl-test-{}", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp);
        let blob_url = "blob:https://web.whatsapp.com/8f3a1c2d-1234-5678-9abc-def012345678";
        let suggested = PathBuf::from("/tmp/invoice-2026.pdf");
        let dest = resolve_download_dest_in(&suggested, blob_url, &tmp);
        assert_eq!(dest, tmp.join("invoice-2026.pdf"));

        std::fs::write(&dest, b"pdf").unwrap();
        let dest2 = resolve_download_dest_in(&suggested, blob_url, &tmp);
        assert_eq!(dest2, tmp.join("invoice-2026 (1).pdf"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
