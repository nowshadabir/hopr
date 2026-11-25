mod config;
mod instance;
mod macos;
mod menu;
mod sparkle;
mod webview_manager;

use config::{AppConfig, Reminder};
use instance::{acquire_single_instance, spawn_instance_listener};
use macos::{
    Notifications, authenticate_touch_id, hide_application, pick_image_data_url, preview_sound,
    pump_runloop_for_flush, remove_data_store_for_account, set_dock_badge, set_launch_at_login,
    unhide_and_activate_application,
};
use menu::{
    AppMenuIds, ContextMenuActionIds, build_app_menu, build_tray_icon, show_account_context_menu,
    sync_tray_menu,
};
use sparkle::SparkleUpdater;
use muda::MenuEvent;
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::WindowBuilderExtMacOS;
use tao::window::{Window, WindowBuilder};
use tray_icon::{TrayIcon, TrayIconEvent};
use uuid::Uuid;
use webview_manager::{AccountView, AppEvent, WebViewManager};
use wry::{Rect, WebView, WebViewBuilder};

const SIDEBAR_WIDTH: f64 = 80.0;
const MAX_IPC_BODY_BYTES: usize = 300 * 1024;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WebViewIpcMsg {
    Notif {
        title: String,
        body: String,
        #[allow(dead_code)]
        tag: String,
    },
    Badge {
        n: u32,
    },
    Status {
        state: String,
    },
    Network {
        online: bool,
    },
    Profile {
        display_name: Option<String>,
        phone: Option<String>,
        avatar_url: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiIpcMsg {
    SwitchAccount { id: Uuid },
    HibernateAccount { id: Uuid },
    AccountContextMenu { id: Uuid, x: f64, y: f64 },
    ToggleGroup { group: String },
    AddAccount,
    RemoveAccount { id: Uuid },
    SetKeepConnected { id: Uuid, keep_connected: bool },
    RenameAccount { id: Uuid, name: String },
    CycleNotifSound { id: Uuid },
    SetGroup { id: Uuid, group: Option<String> },
    ToggleAccountLock { id: Uuid },
    AddReminder { account_id: Uuid, note: String, delay_secs: u64 },
    RemoveReminder { id: Uuid },
    PickCustomAvatar { id: Uuid },
    SetCustomAvatar { id: Uuid, avatar_url: Option<String> },
    ToggleSettings,
    CloseSettings,
    SettingsReady,
    TogglePreviews,
    ToggleDockBadge,
    CycleHibernateMins,
    CycleMaxLive,
    TogglePeriodicWake,
    ToggleQuitOnClose,
    ToggleLaunchAtLogin,
}

#[derive(Serialize)]
struct UiStatePayload<'a> {
    accounts: Vec<AccountView<'a>>,
    reminders: &'a [Reminder],
    collapsed_groups: &'a [String],
    settings_open: bool,
    show_message_previews: bool,
    dock_badge_enabled: bool,
    hibernate_after_mins: u64,
    max_live_webviews: usize,
    periodic_wake_enabled: bool,
    quit_on_close: bool,
    launch_at_login: bool,
}

struct LayoutRects {
    sidebar: Rect,
    content: Rect,
    settings: Rect,
}

fn layout_rects(window: &Window) -> LayoutRects {
    let scale = window.scale_factor();
    let size = window.inner_size().to_logical::<f64>(scale);
    let w = size.width.max(SIDEBAR_WIDTH + 240.0);
    let h = size.height.max(200.0);
    let main_w = w - SIDEBAR_WIDTH;

    LayoutRects {
        sidebar: Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(SIDEBAR_WIDTH, h).into(),
        },
        content: Rect {
            position: LogicalPosition::new(SIDEBAR_WIDTH, 0.0).into(),
            size: LogicalSize::new(main_w, h).into(),
        },
        settings: Rect {
            position: LogicalPosition::new(SIDEBAR_WIDTH, 0.0).into(),
            size: LogicalSize::new(main_w, h).into(),
        },
    }
}

fn sync_ui(
    sidebar: &WebView,
    settings_wv: Option<&WebView>,
    menu_ids: &AppMenuIds,
    tray_icon: Option<&TrayIcon>,
    cfg: &AppConfig,
    manager: &WebViewManager,
    settings_open: bool,
) {
    menu_ids.sync_account_menu(&cfg.accounts);
    let views = manager.account_views(
        &cfg.accounts,
        cfg.hibernate_after_mins,
        cfg.periodic_wake_enabled,
        cfg.periodic_wake_mins,
    );

    let tray_items: Vec<(Uuid, String, u32)> = views
        .iter()
        .map(|v| (v.id, v.name.to_string(), v.unread))
        .collect();
    sync_tray_menu(tray_icon, menu_ids, &tray_items);

    let payload = UiStatePayload {
        accounts: views,
        reminders: &cfg.reminders,
        collapsed_groups: &cfg.collapsed_groups,
        settings_open,
        show_message_previews: cfg.show_message_previews,
        dock_badge_enabled: cfg.dock_badge_enabled,
        hibernate_after_mins: cfg.hibernate_after_mins,
        max_live_webviews: cfg.max_live_webviews,
        periodic_wake_enabled: cfg.periodic_wake_enabled,
        quit_on_close: cfg.quit_on_close,
        launch_at_login: cfg.launch_at_login,
    };

    if let Ok(json) = serde_json::to_string(&payload) {
        let sb_script = format!("window.__updateSidebar && window.__updateSidebar({json});");
        let _ = sidebar.evaluate_script(&sb_script);
        if let Some(swv) = settings_wv {
            let st_script = format!("window.__updateSettings && window.__updateSettings({json});");
            let _ = swv.evaluate_script(&st_script);
        }
    }

    let badge = if cfg.dock_badge_enabled {
        manager.total_unread(&cfg.accounts)
    } else {
        0
    };
    set_dock_badge(badge);
}

fn set_settings_mode(
    settings_open: bool,
    window: &Window,
    proxy: &EventLoopProxy<AppEvent>,
    settings_wv: &mut Option<WebView>,
    manager: &WebViewManager,
) {
    if settings_open {
        if settings_wv.is_none() {
            let rects = layout_rects(window);
            *settings_wv = Some(build_ui_webview(
                include_str!("../assets/settings.html"),
                rects.settings,
                true,
                window,
                proxy,
            ));
        }
    } else {
        *settings_wv = None;
    }
    manager.set_active_visible(!settings_open);
}

fn spawn_lifecycle_ticker(proxy: EventLoopProxy<AppEvent>) {
    thread::spawn(move || {
        // Initial quick stagger ticks (2s) for cold start, then 15s coalesced lifecycle checks
        for _ in 0..4 {
            thread::sleep(Duration::from_secs(2));
            if proxy.send_event(AppEvent::LifecycleTick).is_err() {
                return;
            }
        }
        loop {
            thread::sleep(Duration::from_secs(15));
            if proxy.send_event(AppEvent::LifecycleTick).is_err() {
                break;
            }
        }
    });
}

fn build_ui_webview(
    html: &str,
    bounds: Rect,
    visible: bool,
    window: &Window,
    proxy: &EventLoopProxy<AppEvent>,
) -> WebView {
    let ui_proxy = proxy.clone();
    WebViewBuilder::new()
        .with_html(html)
        .with_bounds(bounds)
        .with_visible(visible)
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |req| {
            let _ = ui_proxy.send_event(AppEvent::UiIpc {
                body: req.into_body(),
            });
        })
        .build_as_child(window)
        .expect("Failed to create UI webview")
}

fn graceful_quit(manager: &mut WebViewManager, control_flow: &mut ControlFlow) {
    if manager.active_downloads_count() > 0 {
        eprintln!("[hopr] Active download in progress; waiting briefly before exit");
    }
    manager.shutdown_all();
    // Pump the main NSRunLoop so WebKit flushes cookies, IndexedDB, and localStorage to disk.
    pump_runloop_for_flush(900);
    *control_flow = ControlFlow::Exit;
}

fn main() {
    let Some(instance_listener) = acquire_single_instance() else {
        return;
    };

    let start_hidden = std::env::args().any(|a| a == "--hidden");
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    spawn_instance_listener(instance_listener, proxy.clone());

    let window = WindowBuilder::new()
        .with_title("Hopr")
        .with_titlebar_transparent(true)
        .with_fullsize_content_view(true)
        .with_title_hidden(true)
        .with_visible(!start_hidden)
        .with_inner_size(LogicalSize::new(1240.0, 820.0))
        .with_min_inner_size(LogicalSize::new(740.0, 500.0))
        .build(&event_loop)
        .expect("Failed to create main window");

    let (_app_menu, menu_ids) = build_app_menu();
    let tray_icon = build_tray_icon(&menu_ids).ok();
    let notifications = Notifications::init();
    let sparkle = SparkleUpdater::init();

    let mut cfg = AppConfig::load_or_init();
    let mut manager = WebViewManager::new();
    manager.seed_from_config(&cfg.accounts);
    let mut settings_open = false;
    let mut pending_ctx: Option<ContextMenuActionIds> = None;

    let rects = layout_rects(&window);
    let sidebar = build_ui_webview(
        include_str!("../assets/sidebar.html"),
        rects.sidebar,
        true,
        &window,
        &proxy,
    );
    let mut settings_wv: Option<WebView> = None;

    // Cold start: draw sidebar immediately, load only active account first (unless launched hidden),
    // then stagger remaining Always Connected accounts.
    let initial_id = if start_hidden {
        None
    } else {
        cfg.active_account
            .filter(|id| !cfg.get(*id).map(|a| a.locked).unwrap_or(false))
            .or_else(|| cfg.accounts.iter().find(|a| !a.locked).map(|a| a.id))
    };

    if let Some(id) = initial_id
        && let Err(err) = manager.switch_to(
            id,
            &cfg.accounts,
            &window,
            &proxy,
            rects.content,
            cfg.max_live_webviews,
            true,
        )
    {
        eprintln!("[hopr] Failed to load initial account: error={err}");
    }
    manager.queue_cold_start_staggered(&cfg.accounts, initial_id);

    sync_ui(
        &sidebar,
        settings_wv.as_ref(),
        &menu_ids,
        tray_icon.as_ref(),
        &cfg,
        &manager,
        settings_open,
    );
    spawn_lifecycle_ticker(proxy.clone());

    let menu_rx = MenuEvent::receiver();
    let tray_rx = TrayIconEvent::receiver();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        while let Ok(menu_evt) = menu_rx.try_recv() {
            if let Some(ctx) = pending_ctx.as_ref() {
                if menu_evt.id == ctx.toggle_lock {
                    let id = ctx.account_id;
                    if let Some(acct) = cfg.get(id).cloned() {
                        if acct.locked {
                            if authenticate_touch_id(&format!("Unlock {}", acct.effective_name())) {
                                cfg.set_locked(id, false);
                                settings_open = false;
                                set_settings_mode(false, &window, &proxy, &mut settings_wv, &manager);
                                let rects = layout_rects(&window);
                                let _ = manager.switch_to(
                                    id,
                                    &cfg.accounts,
                                    &window,
                                    &proxy,
                                    rects.content,
                                    cfg.max_live_webviews,
                                    true,
                                );
                            }
                        } else if authenticate_touch_id(&format!("Lock {}", acct.effective_name())) {
                            cfg.set_locked(id, true);
                            if manager.active_id() == Some(id) {
                                manager.set_active_visible(false);
                            }
                        }
                        sync_ui(
                            &sidebar,
                            settings_wv.as_ref(),
                            &menu_ids,
                            tray_icon.as_ref(),
                            &cfg,
                            &manager,
                            settings_open,
                        );
                    }
                    pending_ctx = None;
                    continue;
                } else if menu_evt.id == ctx.toggle_pin {
                    let id = ctx.account_id;
                    let next_pin = !cfg.get(id).map(|a| a.keep_connected).unwrap_or(false);
                    cfg.set_keep_connected(id, next_pin);
                    sync_ui(
                        &sidebar,
                        settings_wv.as_ref(),
                        &menu_ids,
                        tray_icon.as_ref(),
                        &cfg,
                        &manager,
                        settings_open,
                    );
                    pending_ctx = None;
                    continue;
                } else if menu_evt.id == ctx.add_reminder {
                    settings_open = true;
                    set_settings_mode(true, &window, &proxy, &mut settings_wv, &manager);
                    sync_ui(
                        &sidebar,
                        settings_wv.as_ref(),
                        &menu_ids,
                        tray_icon.as_ref(),
                        &cfg,
                        &manager,
                        settings_open,
                    );
                    pending_ctx = None;
                    continue;
                } else if menu_evt.id == ctx.sleep_account {
                    manager.hibernate(ctx.account_id);
                    sync_ui(
                        &sidebar,
                        settings_wv.as_ref(),
                        &menu_ids,
                        tray_icon.as_ref(),
                        &cfg,
                        &manager,
                        settings_open,
                    );
                    pending_ctx = None;
                    continue;
                }
            }
            if menu_evt.id == menu_ids.check_for_updates {
                sparkle.check_for_updates();
            } else if menu_evt.id == menu_ids.new_account {
                let new_id = cfg.add_account().id;
                cfg.active_account = Some(new_id);
                let _ = cfg.save();
                settings_open = false;
                set_settings_mode(false, &window, &proxy, &mut settings_wv, &manager);
                let rects = layout_rects(&window);
                let _ = manager.switch_to(
                    new_id,
                    &cfg.accounts,
                    &window,
                    &proxy,
                    rects.content,
                    cfg.max_live_webviews,
                    true,
                );
                sync_ui(
                    &sidebar,
                    settings_wv.as_ref(),
                    &menu_ids,
                    tray_icon.as_ref(),
                    &cfg,
                    &manager,
                    settings_open,
                );
            } else if menu_evt.id == menu_ids.hibernate_inactive {
                manager.hibernate_all_inactive();
                sync_ui(
                    &sidebar,
                    settings_wv.as_ref(),
                    &menu_ids,
                    tray_icon.as_ref(),
                    &cfg,
                    &manager,
                    settings_open,
                );
            } else if menu_evt.id == menu_ids.toggle_mute {
                if let Some(active) = manager.active_id() {
                    cfg.toggle_mute(active);
                    sync_ui(
                        &sidebar,
                        settings_wv.as_ref(),
                        &menu_ids,
                        tray_icon.as_ref(),
                        &cfg,
                        &manager,
                        settings_open,
                    );
                }
            } else if menu_evt.id == menu_ids.open_settings {
                settings_open = !settings_open;
                set_settings_mode(settings_open, &window, &proxy, &mut settings_wv, &manager);
                sync_ui(
                    &sidebar,
                    settings_wv.as_ref(),
                    &menu_ids,
                    tray_icon.as_ref(),
                    &cfg,
                    &manager,
                    settings_open,
                );
            } else if menu_evt.id == menu_ids.tray_show {
                unhide_and_activate_application();
                window.set_visible(true);
                window.set_focus();
                let rects = layout_rects(&window);
                manager.on_window_restored(
                    &cfg.accounts,
                    &window,
                    &proxy,
                    rects.content,
                    cfg.max_live_webviews,
                    !settings_open,
                );
                sync_ui(
                    &sidebar,
                    settings_wv.as_ref(),
                    &menu_ids,
                    tray_icon.as_ref(),
                    &cfg,
                    &manager,
                    settings_open,
                );
            } else if menu_evt.id == menu_ids.tray_quit {
                graceful_quit(&mut manager, control_flow);
                return;
            } else if let Some(idx) = menu_ids
                .switch_accounts
                .iter()
                .position(|id| *id == menu_evt.id)
                && let Some(acct) = cfg.accounts.get(idx)
            {
                let id = acct.id;
                window.set_visible(true);
                window.set_focus();
                cfg.active_account = Some(id);
                let _ = cfg.save();
                settings_open = false;
                set_settings_mode(false, &window, &proxy, &mut settings_wv, &manager);
                let rects = layout_rects(&window);
                let _ = manager.switch_to(
                    id,
                    &cfg.accounts,
                    &window,
                    &proxy,
                    rects.content,
                    cfg.max_live_webviews,
                    true,
                );
                sync_ui(
                    &sidebar,
                    settings_wv.as_ref(),
                    &menu_ids,
                    tray_icon.as_ref(),
                    &cfg,
                    &manager,
                    settings_open,
                );
            }
        }

        while let Ok(_tray_evt) = tray_rx.try_recv() {}

        match event {
            Event::Reopen {
                has_visible_windows,
                ..
            } => {
                if !has_visible_windows {
                    unhide_and_activate_application();
                    window.set_visible(true);
                    window.set_focus();
                    let rects = layout_rects(&window);
                    manager.on_window_restored(
                        &cfg.accounts,
                        &window,
                        &proxy,
                        rects.content,
                        cfg.max_live_webviews,
                        !settings_open,
                    );
                    sync_ui(
                        &sidebar,
                        settings_wv.as_ref(),
                        &menu_ids,
                        tray_icon.as_ref(),
                        &cfg,
                        &manager,
                        settings_open,
                    );
                }
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let rects = layout_rects(&window);
                let _ = sidebar.set_bounds(rects.sidebar);
                if let Some(swv) = &settings_wv {
                    let _ = swv.set_bounds(rects.settings);
                }
                manager.resize_all(rects.content);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if cfg.quit_on_close {
                    graceful_quit(&mut manager, control_flow);
                    return;
                } else {
                    if window.fullscreen().is_some() {
                        window.set_fullscreen(None);
                    }
                    manager.on_window_hidden_or_minimized();
                    window.set_visible(false);
                    hide_application();
                    sync_ui(
                        &sidebar,
                        settings_wv.as_ref(),
                        &menu_ids,
                        tray_icon.as_ref(),
                        &cfg,
                        &manager,
                        settings_open,
                    );
                }
            }
            Event::UserEvent(app_event) => match app_event {
                AppEvent::FocusWindow => {
                    unhide_and_activate_application();
                    window.set_visible(true);
                    window.set_focus();
                    let rects = layout_rects(&window);
                    manager.on_window_restored(
                        &cfg.accounts,
                        &window,
                        &proxy,
                        rects.content,
                        cfg.max_live_webviews,
                        !settings_open,
                    );
                }
                AppEvent::WebViewIpc {
                    account_id,
                    origin,
                    body,
                } => {
                    if body.len() > MAX_IPC_BODY_BYTES || !manager.allow_ipc(account_id, &origin) {
                        return;
                    }
                    if let Ok(msg) = serde_json::from_str::<WebViewIpcMsg>(&body) {
                        match msg {
                            WebViewIpcMsg::Badge { n } => {
                                manager.set_unread(account_id, n);
                                cfg.set_last_known_unread(account_id, n);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            WebViewIpcMsg::Notif { title, body, .. } => {
                                if let Some(acct) = cfg.get(account_id)
                                    && !acct.muted
                                {
                                    notifications.post(
                                        account_id,
                                        acct.effective_name(),
                                        &title,
                                        &body,
                                        cfg.show_message_previews && !acct.locked,
                                        &acct.notif_sound,
                                    );
                                }
                            }
                            WebViewIpcMsg::Status { state } => {
                                if manager.set_connection_state(account_id, state)
                                    && let Some(acct) = cfg.get(account_id)
                                {
                                    notifications.post(
                                        account_id,
                                        acct.effective_name(),
                                        "Needs login",
                                        "Scan QR code to reconnect your account.",
                                        true,
                                        &acct.notif_sound,
                                    );
                                }
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            WebViewIpcMsg::Network { online } => {
                                manager.set_offline(account_id, !online);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            WebViewIpcMsg::Profile {
                                display_name,
                                phone,
                                avatar_url,
                            } => {
                                if cfg.update_profile(account_id, display_name, phone, avatar_url) {
                                    sync_ui(
                                        &sidebar,
                                        settings_wv.as_ref(),
                                        &menu_ids,
                                        tray_icon.as_ref(),
                                        &cfg,
                                        &manager,
                                        settings_open,
                                    );
                                }
                            }
                        }
                    }
                }
                AppEvent::UiIpc { body } => {
                    if let Ok(msg) = serde_json::from_str::<UiIpcMsg>(&body) {
                        match msg {
                            UiIpcMsg::SwitchAccount { id } => {
                                if let Some(acct) = cfg.get(id).cloned()
                                    && acct.locked
                                {
                                    if !authenticate_touch_id(&format!(
                                        "Unlock {}",
                                        acct.effective_name()
                                    )) {
                                        return;
                                    }
                                    cfg.set_locked(id, false);
                                }
                                cfg.active_account = Some(id);
                                let _ = cfg.save();
                                settings_open = false;
                                set_settings_mode(
                                    false,
                                    &window,
                                    &proxy,
                                    &mut settings_wv,
                                    &manager,
                                );
                                let rects = layout_rects(&window);
                                let _ = manager.switch_to(
                                    id,
                                    &cfg.accounts,
                                    &window,
                                    &proxy,
                                    rects.content,
                                    cfg.max_live_webviews,
                                    true,
                                );
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::HibernateAccount { id } => {
                                manager.hibernate(id);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::AccountContextMenu { id, x, y } => {
                                if let Some(acct) = cfg.get(id) {
                                    pending_ctx =
                                        Some(show_account_context_menu(&window, acct, x, y));
                                }
                            }
                            UiIpcMsg::ToggleGroup { group } => {
                                cfg.toggle_group_collapsed(&group);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::AddAccount => {
                                let new_id = cfg.add_account().id;
                                cfg.active_account = Some(new_id);
                                let _ = cfg.save();
                                settings_open = false;
                                set_settings_mode(
                                    false,
                                    &window,
                                    &proxy,
                                    &mut settings_wv,
                                    &manager,
                                );
                                let rects = layout_rects(&window);
                                let _ = manager.switch_to(
                                    new_id,
                                    &cfg.accounts,
                                    &window,
                                    &proxy,
                                    rects.content,
                                    cfg.max_live_webviews,
                                    true,
                                );
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::RemoveAccount { id } => {
                                let uuid_bytes = cfg.get(id).map(|a| a.uuid_bytes());
                                if cfg.remove_account(id) {
                                    manager.remove(id);
                                    if let Some(bytes) = uuid_bytes {
                                        remove_data_store_for_account(bytes);
                                    }
                                    if manager.active_id().is_none()
                                        && let Some(next_id) = cfg.active_account
                                    {
                                        let rects = layout_rects(&window);
                                        let _ = manager.switch_to(
                                            next_id,
                                            &cfg.accounts,
                                            &window,
                                            &proxy,
                                            rects.content,
                                            cfg.max_live_webviews,
                                            !settings_open,
                                        );
                                    }
                                    sync_ui(
                                        &sidebar,
                                        settings_wv.as_ref(),
                                        &menu_ids,
                                        tray_icon.as_ref(),
                                        &cfg,
                                        &manager,
                                        settings_open,
                                    );
                                }
                            }
                            UiIpcMsg::SetKeepConnected { id, keep_connected } => {
                                cfg.set_keep_connected(id, keep_connected);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::RenameAccount { id, name } => {
                                cfg.rename_account(id, name);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::CycleNotifSound { id } => {
                                let next = cfg.cycle_notif_sound(id);
                                preview_sound(&next);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::SetGroup { id, group } => {
                                cfg.set_group(id, group);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::ToggleAccountLock { id } => {
                                if let Some(acct) = cfg.get(id).cloned() {
                                    let target_lock = !acct.locked;
                                    let reason = if target_lock {
                                        format!("Lock {}", acct.effective_name())
                                    } else {
                                        format!("Unlock {}", acct.effective_name())
                                    };
                                    if authenticate_touch_id(&reason) {
                                        cfg.set_locked(id, target_lock);
                                        if target_lock && manager.active_id() == Some(id) {
                                            manager.set_active_visible(false);
                                        }
                                        sync_ui(
                                            &sidebar,
                                            settings_wv.as_ref(),
                                            &menu_ids,
                                            tray_icon.as_ref(),
                                            &cfg,
                                            &manager,
                                            settings_open,
                                        );
                                    }
                                }
                            }
                            UiIpcMsg::AddReminder {
                                account_id,
                                note,
                                delay_secs,
                            } => {
                                cfg.add_reminder(account_id, note, delay_secs);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::RemoveReminder { id } => {
                                cfg.remove_reminder(id);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::PickCustomAvatar { id } => {
                                if let Some(data_url) = pick_image_data_url() {
                                    cfg.set_custom_avatar(id, Some(data_url));
                                    sync_ui(
                                        &sidebar,
                                        settings_wv.as_ref(),
                                        &menu_ids,
                                        tray_icon.as_ref(),
                                        &cfg,
                                        &manager,
                                        settings_open,
                                    );
                                }
                            }
                            UiIpcMsg::SetCustomAvatar { id, avatar_url } => {
                                cfg.set_custom_avatar(id, avatar_url);
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::ToggleSettings => {
                                settings_open = !settings_open;
                                set_settings_mode(
                                    settings_open,
                                    &window,
                                    &proxy,
                                    &mut settings_wv,
                                    &manager,
                                );
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::CloseSettings => {
                                settings_open = false;
                                set_settings_mode(
                                    false,
                                    &window,
                                    &proxy,
                                    &mut settings_wv,
                                    &manager,
                                );
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::SettingsReady => {
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::TogglePreviews => {
                                cfg.show_message_previews = !cfg.show_message_previews;
                                let _ = cfg.save();
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::ToggleDockBadge => {
                                cfg.dock_badge_enabled = !cfg.dock_badge_enabled;
                                let _ = cfg.save();
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::CycleHibernateMins => {
                                cfg.hibernate_after_mins = match cfg.hibernate_after_mins {
                                    5 => 15,
                                    15 => 30,
                                    30 => 60,
                                    _ => 5,
                                };
                                let _ = cfg.save();
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::CycleMaxLive => {
                                cfg.max_live_webviews = match cfg.max_live_webviews {
                                    1 => 2,
                                    2 => 3,
                                    3 => 4,
                                    4 => 5,
                                    _ => 1,
                                };
                                let _ = cfg.save();
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::TogglePeriodicWake => {
                                cfg.periodic_wake_enabled = !cfg.periodic_wake_enabled;
                                let _ = cfg.save();
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::ToggleQuitOnClose => {
                                cfg.quit_on_close = !cfg.quit_on_close;
                                let _ = cfg.save();
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                            UiIpcMsg::ToggleLaunchAtLogin => {
                                cfg.launch_at_login = !cfg.launch_at_login;
                                set_launch_at_login(cfg.launch_at_login);
                                let _ = cfg.save();
                                sync_ui(
                                    &sidebar,
                                    settings_wv.as_ref(),
                                    &menu_ids,
                                    tray_icon.as_ref(),
                                    &cfg,
                                    &manager,
                                    settings_open,
                                );
                            }
                        }
                    }
                }
                AppEvent::LifecycleTick => {
                    let rects = layout_rects(&window);
                    let mut changed = manager.tick_lifecycle(&cfg, &window, &proxy, rects.content);
                    let due = cfg.take_due_reminders();
                    if !due.is_empty() {
                        changed = true;
                        for rem in due {
                            if let Some(acct) = cfg.get(rem.account_id) {
                                notifications.post(
                                    acct.id,
                                    acct.effective_name(),
                                    "Reminder",
                                    &rem.note,
                                    true,
                                    &acct.notif_sound,
                                );
                            }
                        }
                    }
                    if changed {
                        sync_ui(
                            &sidebar,
                            settings_wv.as_ref(),
                            &menu_ids,
                            tray_icon.as_ref(),
                            &cfg,
                            &manager,
                            settings_open,
                        );
                    }
                }
            },
            _ => {}
        }
    });
}
