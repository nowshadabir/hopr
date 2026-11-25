use crate::config::AppConfig;
use std::path::PathBuf;
use std::process::Command;
use uuid::Uuid;
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use winrt_notification::{Duration, Sound, Toast};

pub fn open_external_url(url: &str) {
    let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
}

pub fn hide_application() {}
pub fn unhide_and_activate_application() {}

pub fn pump_runloop_for_flush(millis: u64) {
    std::thread::sleep(std::time::Duration::from_millis(millis));
}

pub fn preview_sound(_sound_name: &str) {}

pub fn can_authenticate_biometric() -> bool {
    false
}

pub fn authenticate_touch_id(_reason: &str) -> bool {
    false
}

pub fn is_low_power_mode() -> bool {
    unsafe {
        let mut status: SYSTEM_POWER_STATUS = std::mem::zeroed();
        if GetSystemPowerStatus(&mut status) != 0 {
            return status.SystemStatusFlag == 1
                || (status.BatteryLifePercent <= 20 && status.ACLineStatus == 0);
        }
    }
    false
}

pub fn downloads_dir() -> PathBuf {
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        PathBuf::from(profile).join("Downloads")
    } else {
        PathBuf::from("Downloads")
    }
}

pub fn remove_data_store_for_account(uuid_bytes: [u8; 16]) {
    let id = Uuid::from_bytes(uuid_bytes);
    let profile_dir = AppConfig::app_support_dir().join("profiles").join(id.to_string());
    let _ = std::fs::remove_dir_all(profile_dir);
}

pub fn set_launch_at_login(enabled: bool) {
    let exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "hopr.exe".to_string());
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if enabled {
        let val = format!("\"{exe}\" --hidden");
        let _ = Command::new("reg")
            .args(["add", key, "/v", "Hopr", "/t", "REG_SZ", "/d", &val, "/f"])
            .spawn();
    } else {
        let _ = Command::new("reg")
            .args(["delete", key, "/v", "Hopr", "/f"])
            .spawn();
    }
}

pub fn pick_image_data_url() -> Option<String> {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$dlg = New-Object System.Windows.Forms.OpenFileDialog
$dlg.Filter = 'Images|*.png;*.jpg;*.jpeg;*.webp;*.bmp'
if ($dlg.ShowDialog() -eq 'OK') {
  $img = [System.Drawing.Image]::FromFile($dlg.FileName)
  $bmp = New-Object System.Drawing.Bitmap(128, 128)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.DrawImage($img, 0, 0, 128, 128)
  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Jpeg)
  [Convert]::ToBase64String($ms.ToArray())
}
"#;
    let out = Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let b64 = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if b64.is_empty() || b64.len() > 250 * 1024 {
        return None;
    }
    Some(format!("data:image/jpeg;base64,{b64}"))
}

pub fn set_dock_badge(_total_unread: u32) {}

pub struct Notifications;

impl Notifications {
    pub fn init() -> Self {
        Self
    }

    pub fn post(
        &self,
        _account_id: Uuid,
        account_name: &str,
        title: &str,
        body: &str,
        show_previews: bool,
        _sound_name: &str,
    ) {
        let display_body = if show_previews { body } else { "New message" };
        let full_title = if !show_previews || title.is_empty() {
            account_name.to_string()
        } else {
            format!("{title} ({account_name})")
        };

        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(&full_title)
            .text1(display_body)
            .sound(Some(Sound::Default))
            .duration(Duration::Short)
            .show();
    }
}
