use crate::config::AccountConfig;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{ContextMenu, Menu, MenuItem, PredefinedMenuItem, Submenu};
use std::io::Cursor;
use tao::dpi::LogicalPosition;
use tao::platform::macos::WindowExtMacOS;
use tao::window::Window;
use tray_icon::menu::Menu as TrayMenu;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use uuid::Uuid;

const TRAY_PNG: &[u8] = include_bytes!("../assets/tray-32.png");

pub struct AppMenuIds {
    pub check_for_updates: muda::MenuId,
    pub new_account: muda::MenuId,
    pub hibernate_inactive: muda::MenuId,
    pub toggle_mute: muda::MenuId,
    pub open_settings: muda::MenuId,
    pub switch_accounts: Vec<muda::MenuId>,
    pub switch_account_items: Vec<MenuItem>,
    pub tray_show: muda::MenuId,
    pub tray_quit: muda::MenuId,
}

impl AppMenuIds {
    pub fn sync_account_menu(&self, accounts: &[AccountConfig]) {
        for (idx, item) in self.switch_account_items.iter().enumerate() {
            if let Some(acct) = accounts.get(idx) {
                item.set_text(format!("{} ({})", acct.effective_name(), acct.label));
                item.set_enabled(true);
            } else {
                item.set_text(format!("Account {} (Empty)", idx + 1));
                item.set_enabled(false);
            }
        }
    }
}

pub fn sync_tray_menu(
    tray: Option<&TrayIcon>,
    ids: &AppMenuIds,
    accounts: &[(Uuid, String, u32)],
) {
    let Some(tray) = tray else {
        return;
    };
    let tray_menu = TrayMenu::new();
    let show_item = MenuItem::with_id(ids.tray_show.clone(), "Open Hopr", true, None);
    let _ = tray_menu.append(&show_item);
    let _ = tray_menu.append(&PredefinedMenuItem::separator());

    for (idx, (_, name, unread)) in accounts.iter().take(9).enumerate() {
        let title = if *unread > 0 {
            format!("{name} ({unread})")
        } else {
            name.clone()
        };
        if let Some(menu_id) = ids.switch_accounts.get(idx) {
            let item = MenuItem::with_id(menu_id.clone(), title, true, None);
            let _ = tray_menu.append(&item);
        }
    }

    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let quit_item = MenuItem::with_id(ids.tray_quit.clone(), "Quit Hopr", true, None);
    let _ = tray_menu.append(&quit_item);
    tray.set_menu(Some(Box::new(tray_menu)));
}

pub struct ContextMenuActionIds {
    pub account_id: Uuid,
    pub toggle_lock: muda::MenuId,
    pub toggle_pin: muda::MenuId,
    pub add_reminder: muda::MenuId,
    pub sleep_account: muda::MenuId,
}

pub fn show_account_context_menu(
    window: &Window,
    acct: &AccountConfig,
    x: f64,
    y: f64,
) -> ContextMenuActionIds {
    let menu = Menu::new();
    let lock_label = if acct.locked {
        "Unlock Account (Touch ID)"
    } else {
        "Lock Account (Touch ID)"
    };
    let pin_label = if acct.keep_connected {
        "Unpin Account"
    } else {
        "Pin Account (Always Connected)"
    };

    let toggle_lock = MenuItem::new(lock_label, true, None);
    let toggle_pin = MenuItem::new(pin_label, true, None);
    let add_reminder = MenuItem::new("Add Reminder…", true, None);
    let sleep_account = MenuItem::new("Sleep Now", !acct.keep_connected, None);

    let _ = menu.append_items(&[
        &toggle_lock,
        &toggle_pin,
        &add_reminder,
        &PredefinedMenuItem::separator(),
        &sleep_account,
    ]);

    let ids = ContextMenuActionIds {
        account_id: acct.id,
        toggle_lock: toggle_lock.id().clone(),
        toggle_pin: toggle_pin.id().clone(),
        add_reminder: add_reminder.id().clone(),
        sleep_account: sleep_account.id().clone(),
    };

    let ns_view = window.ns_view();
    unsafe {
        let _ = menu.show_context_menu_for_nsview(
            ns_view,
            Some(muda::dpi::Position::Logical(LogicalPosition::new(x, y))),
        );
    }
    ids
}

pub fn build_app_menu() -> (Menu, AppMenuIds) {
    let menu = Menu::new();

    let app_menu = Submenu::new("Hopr", true);
    let check_for_updates = MenuItem::new("Check for Updates…", true, None);
    let open_settings = MenuItem::new(
        "Settings…",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)),
    );
    let quit_item = MenuItem::new(
        "Quit Hopr",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyQ)),
    );
    let _ = app_menu.append_items(&[
        &PredefinedMenuItem::about(Some("About Hopr"), None),
        &check_for_updates,
        &PredefinedMenuItem::separator(),
        &open_settings,
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &quit_item,
    ]);

    let edit_menu = Submenu::new("Edit", true);
    let _ = edit_menu.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]);

    let accounts_menu = Submenu::new("Accounts", true);
    let new_account = MenuItem::new(
        "New Account",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyN)),
    );
    let hibernate_inactive = MenuItem::new(
        "Sleep Inactive Accounts",
        true,
        Some(Accelerator::new(
            Some(Modifiers::SUPER | Modifiers::SHIFT),
            Code::KeyH,
        )),
    );
    let toggle_mute = MenuItem::new(
        "Mute / Unmute Current Account",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyM)),
    );

    let _ = accounts_menu.append_items(&[
        &new_account,
        &hibernate_inactive,
        &toggle_mute,
        &PredefinedMenuItem::separator(),
    ]);

    let codes = [
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    let mut switch_accounts = Vec::with_capacity(codes.len());
    let mut switch_account_items = Vec::with_capacity(codes.len());
    for (idx, code) in codes.into_iter().enumerate() {
        let item = MenuItem::new(
            format!("Account {}", idx + 1),
            false,
            Some(Accelerator::new(Some(Modifiers::SUPER), code)),
        );
        let _ = accounts_menu.append(&item);
        switch_accounts.push(item.id().clone());
        switch_account_items.push(item);
    }

    let window_menu = Submenu::new("Window", true);
    let _ = window_menu.append_items(&[
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::close_window(None),
    ]);

    let _ = menu.append_items(&[&app_menu, &edit_menu, &accounts_menu, &window_menu]);

    let tray_show = MenuItem::new("Show Hopr", true, None);

    let ids = AppMenuIds {
        check_for_updates: check_for_updates.id().clone(),
        new_account: new_account.id().clone(),
        hibernate_inactive: hibernate_inactive.id().clone(),
        toggle_mute: toggle_mute.id().clone(),
        open_settings: open_settings.id().clone(),
        switch_accounts,
        switch_account_items,
        tray_show: tray_show.id().clone(),
        tray_quit: quit_item.id().clone(),
    };

    menu.init_for_nsapp();
    (menu, ids)
}

pub fn build_tray_icon(ids: &AppMenuIds) -> Result<TrayIcon, String> {
    let tray_menu = TrayMenu::new();
    let show_item = MenuItem::with_id(ids.tray_show.clone(), "Show Hopr", true, None);
    let quit_item = MenuItem::with_id(ids.tray_quit.clone(), "Quit Hopr", true, None);
    let _ = tray_menu.append_items(&[&show_item, &PredefinedMenuItem::separator(), &quit_item]);

    let icon = decode_tray_png()?;
    TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Hopr")
        .with_icon(icon)
        .build()
        .map_err(|e| e.to_string())
}

fn decode_tray_png() -> Result<Icon, String> {
    let mut decoder = png::Decoder::new(Cursor::new(TRAY_PNG));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let rgba = buf[..info.buffer_size()].to_vec();
    Icon::from_rgba(rgba, info.width, info.height).map_err(|e| e.to_string())
}
