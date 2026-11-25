use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::NSApplication;
use objc2_foundation::{MainThreadMarker, NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationSound, UNUserNotificationCenter,
};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

static NOTIF_UNSUPPORTED_LOGGED: AtomicBool = AtomicBool::new(false);

pub fn open_external_url(url: &str) {
    let _ = Command::new("open").arg(url).spawn();
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
    ) {
        let display_body = if show_previews {
            body
        } else {
            "New message"
        };

        if !self.bundled {
            eprintln!("[notif:{account_name}] {title}: {display_body}");
            return;
        }

        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();

        let full_title = if title.is_empty() {
            account_name.to_string()
        } else {
            format!("{title} ({account_name})")
        };
        content.setTitle(&NSString::from_str(&full_title));
        content.setBody(&NSString::from_str(display_body));

        let sound = UNNotificationSound::defaultSound();
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
