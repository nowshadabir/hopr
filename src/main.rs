mod config;
mod macos;
mod menu;
mod webview_manager;

use config::AppConfig;
use macos::{Notifications, set_dock_badge};
use menu::{build_app_menu, build_tray_icon};
use muda::MenuEvent;
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Window, WindowBuilder};
use tray_icon::TrayIconEvent;
use uuid::Uuid;
use webview_manager::{AccountView, AppEvent, WebViewManager};
use wry::{Rect, WebView, WebViewBuilder};

const SIDEBAR_WIDTH: f64 = 68.0;
const TOPBAR_HEIGHT: f64 = 54.0;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
    SleepCurrent,
    AddAccount,
    RemoveAccount { id: Uuid },
    SetKeepConnected { id: Uuid, keep_connected: bool },
    SetCustomAvatar { id: Uuid, avatar_url: Option<String> },
    ToggleSettings,
    CloseSettings,
    TogglePreviews,
    ToggleDockBadge,
}

#[derive(Serialize)]
struct UiStatePayload<'a> {
    accounts: Vec<AccountView<'a>>,
    settings_open: bool,
    show_message_previews: bool,
    dock_badge_enabled: bool,
}

struct LayoutRects {
    sidebar: Rect,
    topbar: Rect,
    content: Rect,
    settings: Rect,
}

fn layout_rects(window: &Window) -> LayoutRects {
    let scale = window.scale_factor();
    let size = window.inner_size().to_logical::<f64>(scale);
    let w = size.width.max(SIDEBAR_WIDTH + 240.0);
    let h = size.height.max(TOPBAR_HEIGHT + 200.0);
    let main_w = w - SIDEBAR_WIDTH;

    LayoutRects {
        sidebar: Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(SIDEBAR_WIDTH, h).into(),
        },
        topbar: Rect {
            position: LogicalPosition::new(SIDEBAR_WIDTH, 0.0).into(),
            size: LogicalSize::new(main_w, TOPBAR_HEIGHT).into(),
        },
        content: Rect {
            position: LogicalPosition::new(SIDEBAR_WIDTH, TOPBAR_HEIGHT).into(),
            size: LogicalSize::new(main_w, h - TOPBAR_HEIGHT).into(),
        },
        settings: Rect {
            position: LogicalPosition::new(SIDEBAR_WIDTH, 0.0).into(),
            size: LogicalSize::new(main_w, h).into(),
        },
    }
}

fn sync_ui(
    sidebar: &WebView,
    topbar: &WebView,
    settings_wv: &WebView,
    cfg: &AppConfig,
    manager: &WebViewManager,
    settings_open: bool,
) {
    let views = manager.account_views(&cfg.accounts);
    let active_view = views.iter().find(|a| a.is_active);

    if let Some(active) = active_view
        && let Ok(active_json) = serde_json::to_string(active)
    {
        let script = format!("window.__updateTopbar && window.__updateTopbar({active_json});");
        let _ = topbar.evaluate_script(&script);
    }

    let payload = UiStatePayload {
        accounts: views,
        settings_open,
        show_message_previews: cfg.show_message_previews,
        dock_badge_enabled: cfg.dock_badge_enabled,
    };

    if let Ok(json) = serde_json::to_string(&payload) {
        let sb_script = format!("window.__updateSidebar && window.__updateSidebar({json});");
        let _ = sidebar.evaluate_script(&sb_script);
        let st_script = format!("window.__updateSettings && window.__updateSettings({json});");
        let _ = settings_wv.evaluate_script(&st_script);
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
    topbar: &WebView,
    settings_wv: &WebView,
    manager: &WebViewManager,
) {
    let _ = settings_wv.set_visible(settings_open);
    let _ = topbar.set_visible(!settings_open);
    manager.set_active_visible(!settings_open);
}

fn spawn_lifecycle_ticker(proxy: EventLoopProxy<AppEvent>) {
    thread::spawn(move || {
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
        .with_ipc_handler(move |req| {
            let _ = ui_proxy.send_event(AppEvent::UiIpc {
                body: req.into_body(),
            });
        })
        .build_as_child(window)
        .expect("Failed to create UI webview")
}

fn main() {
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Hopr")
        .with_inner_size(LogicalSize::new(1240.0, 820.0))
        .with_min_inner_size(LogicalSize::new(740.0, 500.0))
        .build(&event_loop)
        .expect("Failed to create main window");

    let (_app_menu, menu_ids) = build_app_menu();
    let _tray_icon = build_tray_icon(&menu_ids).ok();
    let notifications = Notifications::init();

    let mut cfg = AppConfig::load_or_init();
    let mut manager = WebViewManager::new();
    let mut settings_open = false;

    let rects = layout_rects(&window);
    let sidebar = build_ui_webview(
        include_str!("../assets/sidebar.html"),
        rects.sidebar,
        true,
        &window,
        &proxy,
    );
    let topbar = build_ui_webview(
        include_str!("../assets/topbar.html"),
        rects.topbar,
        true,
        &window,
        &proxy,
    );
    let settings_wv = build_ui_webview(
        include_str!("../assets/settings.html"),
        rects.settings,
        false,
        &window,
        &proxy,
    );

    if let Some(initial_id) = cfg.active_account.or_else(|| cfg.accounts.first().map(|a| a.id)) {
        if let Err(err) =
            manager.switch_to(initial_id, &cfg.accounts, &window, &proxy, rects.content, true)
        {
            eprintln!("[hopr] Failed to load initial account: {err}");
        }
    }

    sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
    spawn_lifecycle_ticker(proxy.clone());

    let menu_rx = MenuEvent::receiver();
    let tray_rx = TrayIconEvent::receiver();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        while let Ok(menu_evt) = menu_rx.try_recv() {
            if menu_evt.id == menu_ids.new_account {
                let new_id = cfg.add_account().id;
                cfg.active_account = Some(new_id);
                let _ = cfg.save();
                settings_open = false;
                set_settings_mode(false, &topbar, &settings_wv, &manager);
                let rects = layout_rects(&window);
                let _ = manager.switch_to(new_id, &cfg.accounts, &window, &proxy, rects.content, true);
                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
            } else if menu_evt.id == menu_ids.hibernate_inactive {
                manager.hibernate_all_inactive();
                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
            } else if menu_evt.id == menu_ids.toggle_mute {
                if let Some(active) = manager.active_id() {
                    cfg.toggle_mute(active);
                    sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                }
            } else if menu_evt.id == menu_ids.open_settings {
                settings_open = !settings_open;
                set_settings_mode(settings_open, &topbar, &settings_wv, &manager);
                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
            } else if menu_evt.id == menu_ids.tray_show {
                window.set_visible(true);
                window.set_focus();
            } else if menu_evt.id == menu_ids.tray_quit {
                *control_flow = ControlFlow::Exit;
            } else if let Some(idx) = menu_ids
                .switch_accounts
                .iter()
                .position(|id| *id == menu_evt.id)
                && let Some(acct) = cfg.accounts.get(idx)
            {
                let id = acct.id;
                cfg.active_account = Some(id);
                let _ = cfg.save();
                settings_open = false;
                set_settings_mode(false, &topbar, &settings_wv, &manager);
                let rects = layout_rects(&window);
                let _ = manager.switch_to(id, &cfg.accounts, &window, &proxy, rects.content, true);
                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
            }
        }

        while let Ok(_tray_evt) = tray_rx.try_recv() {}

        match event {
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let rects = layout_rects(&window);
                let _ = sidebar.set_bounds(rects.sidebar);
                let _ = topbar.set_bounds(rects.topbar);
                let _ = settings_wv.set_bounds(rects.settings);
                manager.resize_all(rects.content);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                window.set_visible(false);
            }
            Event::UserEvent(app_event) => match app_event {
                AppEvent::WebViewIpc { account_id, body } => {
                    if let Ok(msg) = serde_json::from_str::<WebViewIpcMsg>(&body) {
                        match msg {
                            WebViewIpcMsg::Badge { n } => {
                                manager.set_unread(account_id, n);
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
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
                                        cfg.show_message_previews,
                                    );
                                }
                            }
                            WebViewIpcMsg::Status { state } => {
                                manager.set_connection_state(account_id, state);
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            WebViewIpcMsg::Profile {
                                display_name,
                                phone,
                                avatar_url,
                            } => {
                                if cfg.update_profile(account_id, display_name, phone, avatar_url) {
                                    sync_ui(
                                        &sidebar,
                                        &topbar,
                                        &settings_wv,
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
                                cfg.active_account = Some(id);
                                let _ = cfg.save();
                                settings_open = false;
                                set_settings_mode(false, &topbar, &settings_wv, &manager);
                                let rects = layout_rects(&window);
                                let _ = manager.switch_to(
                                    id,
                                    &cfg.accounts,
                                    &window,
                                    &proxy,
                                    rects.content,
                                    true,
                                );
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::HibernateAccount { id } => {
                                manager.hibernate(id);
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::SleepCurrent => {
                                if let Some(active_id) = manager.active_id() {
                                    manager.hibernate(active_id);
                                    sync_ui(
                                        &sidebar,
                                        &topbar,
                                        &settings_wv,
                                        &cfg,
                                        &manager,
                                        settings_open,
                                    );
                                }
                            }
                            UiIpcMsg::AddAccount => {
                                let new_id = cfg.add_account().id;
                                cfg.active_account = Some(new_id);
                                let _ = cfg.save();
                                settings_open = false;
                                set_settings_mode(false, &topbar, &settings_wv, &manager);
                                let rects = layout_rects(&window);
                                let _ = manager.switch_to(
                                    new_id,
                                    &cfg.accounts,
                                    &window,
                                    &proxy,
                                    rects.content,
                                    true,
                                );
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::RemoveAccount { id } => {
                                if cfg.remove_account(id) {
                                    manager.remove(id);
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
                                            !settings_open,
                                        );
                                    }
                                    sync_ui(
                                        &sidebar,
                                        &topbar,
                                        &settings_wv,
                                        &cfg,
                                        &manager,
                                        settings_open,
                                    );
                                }
                            }
                            UiIpcMsg::SetKeepConnected { id, keep_connected } => {
                                cfg.set_keep_connected(id, keep_connected);
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::SetCustomAvatar { id, avatar_url } => {
                                cfg.set_custom_avatar(id, avatar_url);
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::ToggleSettings => {
                                settings_open = !settings_open;
                                set_settings_mode(settings_open, &topbar, &settings_wv, &manager);
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::CloseSettings => {
                                settings_open = false;
                                set_settings_mode(false, &topbar, &settings_wv, &manager);
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::TogglePreviews => {
                                cfg.show_message_previews = !cfg.show_message_previews;
                                let _ = cfg.save();
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                            UiIpcMsg::ToggleDockBadge => {
                                cfg.dock_badge_enabled = !cfg.dock_badge_enabled;
                                let _ = cfg.save();
                                sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                            }
                        }
                    }
                }
                AppEvent::LifecycleTick => {
                    let rects = layout_rects(&window);
                    if manager.tick_lifecycle(&cfg.accounts, &window, &proxy, rects.content) {
                        sync_ui(&sidebar, &topbar, &settings_wv, &cfg, &manager, settings_open);
                    }
                }
            },
            _ => {}
        }
    });
}
