use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use std::io::Cursor;
use tray_icon::menu::Menu as TrayMenu;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const TRAY_PNG: &[u8] = include_bytes!("../assets/tray-32.png");

pub struct AppMenuIds {
    pub new_account: muda::MenuId,
    pub hibernate_inactive: muda::MenuId,
    pub toggle_mute: muda::MenuId,
    pub open_settings: muda::MenuId,
    pub switch_accounts: Vec<muda::MenuId>,
    pub tray_show: muda::MenuId,
    pub tray_quit: muda::MenuId,
}

pub fn build_app_menu() -> (Menu, AppMenuIds) {
    let menu = Menu::new();

    let app_menu = Submenu::new("Hopr", true);
    let open_settings = MenuItem::new(
        "Settings…",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)),
    );
    let _ = app_menu.append_items(&[
        &PredefinedMenuItem::about(Some("About Hopr"), None),
        &PredefinedMenuItem::separator(),
        &open_settings,
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
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
    for (idx, code) in codes.into_iter().enumerate() {
        let item = MenuItem::new(
            format!("Switch to Account {}", idx + 1),
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), code)),
        );
        let _ = accounts_menu.append(&item);
        switch_accounts.push(item.id().clone());
    }

    let window_menu = Submenu::new("Window", true);
    let _ = window_menu.append_items(&[
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::close_window(None),
    ]);

    let _ = menu.append_items(&[&app_menu, &edit_menu, &accounts_menu, &window_menu]);

    let tray_show = MenuItem::new("Show Hopr", true, None);
    let tray_quit = MenuItem::new("Quit Hopr", true, None);

    let ids = AppMenuIds {
        new_account: new_account.id().clone(),
        hibernate_inactive: hibernate_inactive.id().clone(),
        toggle_mute: toggle_mute.id().clone(),
        open_settings: open_settings.id().clone(),
        switch_accounts,
        tray_show: tray_show.id().clone(),
        tray_quit: tray_quit.id().clone(),
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
