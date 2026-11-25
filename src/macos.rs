use crate::config::AppConfig;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{
    MainThreadMarker, NSBundle, NSDate, NSError, NSProcessInfo, NSRunLoop, NSString, NSUUID,
};
use objc2_local_authentication::{LAContext, LAPolicy};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationSound, UNUserNotificationCenter,
};
use objc2_web_kit::WKWebsiteDataStore;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

static NOTIF_UNSUPPORTED_LOGGED: AtomicBool = AtomicBool::new(false);

pub fn open_external_url(url: &str) {
    let _ = Command::new("open").arg(url).spawn();
}

pub fn hide_application() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).hide(None);
    }
}

pub fn unhide_and_activate_application() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.unhide(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

pub fn pump_runloop_for_flush(millis: u64) {
    let run_loop = NSRunLoop::currentRunLoop();
    let until = NSDate::dateWithTimeIntervalSinceNow((millis as f64) / 1000.0);
    run_loop.runUntilDate(&until);
}

pub fn preview_sound(sound_name: &str) {
    if sound_name == "default" {
        return;
    }
    let path = format!("/System/Library/Sounds/{sound_name}.aiff");
    if std::path::Path::new(&path).exists() {
        let _ = Command::new("afplay").arg(&path).spawn();
    }
}

pub fn authenticate_touch_id(reason: &str) -> bool {
    unsafe {
        let context = LAContext::new();
        let policy = LAPolicy::DeviceOwnerAuthentication;
        if !context.canEvaluatePolicy_error(policy).is_ok() {
            return true;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let block = RcBlock::new(move |success: objc2::runtime::Bool, _err: *mut NSError| {
            let _ = tx.send(success.as_bool());
        });
        let reason_ns = NSString::from_str(reason);
        context.evaluatePolicy_localizedReason_reply(policy, &reason_ns, &block);
        rx.recv_timeout(std::time::Duration::from_secs(60))
            .unwrap_or(false)
    }
}

pub fn is_low_power_mode() -> bool {
    NSProcessInfo::processInfo().isLowPowerModeEnabled()
}

pub fn downloads_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join("Downloads")
    } else {
        PathBuf::from("Downloads")
    }
}

pub fn remove_data_store_for_account(uuid_bytes: [u8; 16]) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let ns_uuid = NSUUID::from_bytes(uuid_bytes);
    let block = RcBlock::new(|_err: *mut NSError| {});
    unsafe {
        WKWebsiteDataStore::removeDataStoreForIdentifier_completionHandler(&ns_uuid, &block, mtm);
    }
}

pub fn set_launch_at_login(enabled: bool) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let agent_path = PathBuf::from(home).join("Library/LaunchAgents/com.hopr.app.plist");
    if !enabled {
        let _ = std::fs::remove_file(&agent_path);
        return;
    }

    let exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/Applications/Hopr.app/Contents/MacOS/hopr".to_string());

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.hopr.app</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>--hidden</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#
    );

    let _ = AppConfig::write_secure_file(&agent_path, plist.as_bytes());
}

pub fn pick_image_data_url() -> Option<String> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setMessage(Some(&NSString::from_str(
        "Choose a profile photo (saved on this device only)",
    )));

    if panel.runModal() != NSModalResponseOK {
        return None;
    }

    let url = panel.URL()?;
    let path_ns = url.path()?;
    let src_path = path_ns.to_string();

    let tmp_out = std::env::temp_dir().join(format!("hopr-avatar-{}.jpg", Uuid::new_v4()));
    let status = Command::new("sips")
        .args([
            "-s",
            "format",
            "jpeg",
            "-s",
            "formatOptions",
            "85",
            "-Z",
            "128",
            &src_path,
            "--out",
        ])
        .arg(&tmp_out)
        .output()
        .ok()?;

    if !status.status.success() {
        return None;
    }

    let bytes = std::fs::read(&tmp_out).ok()?;
    let _ = std::fs::remove_file(&tmp_out);
    if bytes.len() > 250 * 1024 {
        return None;
    }
    Some(format!("data:image/jpeg;base64,{}", encode_base64(&bytes)))
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

pub fn set_dock_badge(total_unread: u32) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let tile = app.dockTile();
    if total_unread == 0 {
        tile.setBadgeLabel(None);
    } else {
        let label = NSString::from_str(&total_unread.to_string());
        tile.setBadgeLabel(Some(&label));
    }
}

pub struct Notifications {
    bundled: bool,
}

impl Notifications {
    pub fn init() -> Self {
        let bundle = NSBundle::mainBundle();
        let bundled = bundle.bundleIdentifier().is_some();

        if bundled {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let options = UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound
                | UNAuthorizationOptions::Badge;
            let block = RcBlock::new(|_granted: objc2::runtime::Bool, _err: *mut NSError| {});
            center.requestAuthorizationWithOptions_completionHandler(options, &block);
        } else if !NOTIF_UNSUPPORTED_LOGGED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "[hopr] Running unbundled binary; native UNUserNotificationCenter notifications require an .app bundle (use scripts/bundle.sh)."
            );
        }

        Self { bundled }
    }

    pub fn post(
        &self,
        account_id: Uuid,
        account_name: &str,
        title: &str,
        body: &str,
        show_previews: bool,
        sound_name: &str,
    ) {
        let display_body = if show_previews {
            body
        } else {
            "New message"
        };

        if !self.bundled {
            eprintln!("[notif] account={account_id} preview_mode={show_previews} sound={sound_name}");
            return;
        }

        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();

        let full_title = if !show_previews || title.is_empty() {
            account_name.to_string()
        } else {
            format!("{title} ({account_name})")
        };
        content.setTitle(&NSString::from_str(&full_title));
        content.setBody(&NSString::from_str(display_body));

        let sound = if sound_name == "default" {
            UNNotificationSound::defaultSound()
        } else {
            let named = NSString::from_str(&format!("{sound_name}.aiff"));
            UNNotificationSound::soundNamed(&named)
        };
        content.setSound(Some(&sound));

        let req_id = NSString::from_str(&format!("{account_id}-{}", Uuid::new_v4()));
        let request: Retained<UNNotificationRequest> =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&req_id, &content, None);

        let block = RcBlock::new(|err: *mut NSError| {
            if !err.is_null() {
                eprintln!("[hopr] Failed to post notification");
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&block));
    }
}
