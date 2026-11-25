use serde::{Deserialize, Serialize};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const NOTIF_SOUNDS: &[&str] = &[
    "default",
    "Glass",
    "Ping",
    "Pop",
    "Purr",
    "Submarine",
    "Tink",
];

const MAX_AVATAR_BYTES: usize = 256 * 1024;

const PALETTE: &[&str] = &[
    "#135D43", "#4F6DB8", "#8B4E84", "#6B7C6E", "#C67D30", "#2A7B88", "#5E5CE6",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "default_label")]
    pub label: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub custom_avatar_url: Option<String>,
    pub color: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub order: usize,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub keep_connected: bool,
    #[serde(default = "default_hibernate_mins")]
    pub hibernate_after_mins: u64,
    #[serde(default)]
    pub periodic_wake_mins: u64,
    #[serde(default)]
    pub last_known_unread: u32,
    #[serde(default = "default_sound")]
    pub notif_sound: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: Uuid,
    pub account_id: Uuid,
    pub note: String,
    pub due_at_epoch_secs: u64,
}

fn default_sound() -> String {
    "default".to_string()
}

fn default_label() -> String {
    "Personal".to_string()
}

fn default_hibernate_mins() -> u64 {
    15
}

fn default_max_live() -> usize {
    3
}

fn default_periodic_wake_mins() -> u64 {
    10
}

fn default_lock_idle_mins() -> u64 {
    5
}

fn default_true() -> bool {
    true
}

impl AccountConfig {
    pub fn new(
        name: impl Into<String>,
        label: impl Into<String>,
        order: usize,
        keep_connected: bool,
    ) -> Self {
        let color = PALETTE[order % PALETTE.len()].to_string();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            label: label.into(),
            display_name: None,
            custom_name: None,
            phone: None,
            avatar_url: None,
            custom_avatar_url: None,
            color,
            icon: None,
            order,
            muted: false,
            keep_connected,
            hibernate_after_mins: default_hibernate_mins(),
            periodic_wake_mins: 0,
            last_known_unread: 0,
            notif_sound: default_sound(),
            group: None,
            locked: false,
        }
    }

    pub fn effective_name(&self) -> &str {
        self.custom_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                self.display_name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or(&self.name)
    }

    pub fn effective_avatar(&self) -> Option<&str> {
        self.custom_avatar_url
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                self.avatar_url
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
            })
    }

    pub fn uuid_bytes(&self) -> [u8; 16] {
        *self.id.as_bytes()
    }

    pub fn short_sleep_summary(&self, global_hibernate_mins: u64, periodic_wake_enabled: bool, periodic_wake_mins: u64) -> String {
        if self.keep_connected {
            "Sleep: Never".to_string()
        } else if periodic_wake_enabled && periodic_wake_mins > 0 {
            format!(
                "Sleep: {} min, wake every {} min",
                global_hibernate_mins, periodic_wake_mins
            )
        } else {
            format!("Sleep: {} min", global_hibernate_mins)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub accounts: Vec<AccountConfig>,
    pub active_account: Option<Uuid>,
    #[serde(default = "default_true")]
    pub show_message_previews: bool,
    #[serde(default = "default_true")]
    pub dock_badge_enabled: bool,
    #[serde(default = "default_hibernate_mins")]
    pub hibernate_after_mins: u64,
    #[serde(default = "default_max_live")]
    pub max_live_webviews: usize,
    #[serde(default)]
    pub quit_on_close: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default)]
    pub periodic_wake_enabled: bool,
    #[serde(default = "default_periodic_wake_mins")]
    pub periodic_wake_mins: u64,
    #[serde(default)]
    pub app_lock_enabled: bool,
    #[serde(default = "default_lock_idle_mins")]
    pub app_lock_idle_mins: u64,
    #[serde(default)]
    pub reminders: Vec<Reminder>,
    #[serde(default)]
    pub collapsed_groups: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            active_account: None,
            show_message_previews: true,
            dock_badge_enabled: true,
            hibernate_after_mins: default_hibernate_mins(),
            max_live_webviews: default_max_live(),
            quit_on_close: false,
            launch_at_login: false,
            periodic_wake_enabled: false,
            periodic_wake_mins: default_periodic_wake_mins(),
            app_lock_enabled: false,
            app_lock_idle_mins: default_lock_idle_mins(),
            reminders: Vec::new(),
            collapsed_groups: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn app_support_dir() -> PathBuf {
        #[cfg(debug_assertions)]
        if let Ok(dir) = std::env::var("HOPR_DATA_DIR")
            && !dir.trim().is_empty()
        {
            return PathBuf::from(dir);
        }
        #[cfg(target_os = "windows")]
        {
            if let Some(appdata) = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")) {
                return PathBuf::from(appdata).join("Hopr");
            }
            return PathBuf::from("Hopr");
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Some(home) = std::env::var_os("HOME") {
                PathBuf::from(home).join("Library/Application Support/Hopr")
            } else {
                PathBuf::from("Hopr")
            }
        }
    }

    pub fn config_path() -> PathBuf {
        Self::app_support_dir().join("accounts.toml")
    }

    pub fn avatars_dir() -> PathBuf {
        Self::app_support_dir().join("avatars")
    }

    pub fn ensure_secure_dir(path: &Path) -> Result<(), String> {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn write_secure_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            Self::ensure_secure_dir(parent)?;
        }
        fs::write(path, bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_or_init() -> Self {
        let path = Self::config_path();
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(mut cfg) = toml::from_str::<AppConfig>(&raw)
        {
            if cfg.accounts.is_empty() {
                cfg.accounts
                    .push(AccountConfig::new("Personal", "Personal", 0, true));
            }
            cfg.accounts.sort_by_key(|a| a.order);
            if cfg.active_account.is_none() {
                cfg.active_account = cfg.accounts.first().map(|a| a.id);
            }
            let _ = Self::ensure_secure_dir(&Self::app_support_dir());
            let _ = Self::ensure_secure_dir(&Self::avatars_dir());
            return cfg;
        }

        let default_acct = AccountConfig::new("Personal", "Personal", 0, true);
        let active = Some(default_acct.id);
        let cfg = Self {
            accounts: vec![default_acct],
            active_account: active,
            ..Self::default()
        };
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        let body = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        Self::write_secure_file(&path, body.as_bytes())
    }

    pub fn add_account(&mut self) -> &AccountConfig {
        let idx = self.accounts.len();
        let name = format!("Account {}", idx + 1);
        let label = match idx {
            0 => "Personal",
            1 => "Work",
            _ => "Account",
        };
        let acct = AccountConfig::new(name, label, idx, idx == 0);
        self.accounts.push(acct);
        let _ = self.save();
        self.accounts.last().expect("just pushed")
    }

    pub fn get(&self, id: Uuid) -> Option<&AccountConfig> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn toggle_mute(&mut self, id: Uuid) -> bool {
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.muted = !acct.muted;
            let muted = acct.muted;
            let _ = self.save();
            return muted;
        }
        false
    }

    pub fn set_keep_connected(&mut self, id: Uuid, keep_connected: bool) {
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.keep_connected = keep_connected;
            let _ = self.save();
        }
    }

    pub fn rename_account(&mut self, id: Uuid, name: String) {
        let trimmed = name.trim().to_string();
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.custom_name = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
            let _ = self.save();
        }
    }

    pub fn set_custom_avatar(&mut self, id: Uuid, avatar_url: Option<String>) {
        let bounded = avatar_url.filter(|s| !s.trim().is_empty() && s.len() <= MAX_AVATAR_BYTES);
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.custom_avatar_url = bounded;
            let _ = self.save();
        }
    }

    pub fn set_last_known_unread(&mut self, id: Uuid, unread: u32) {
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id)
            && acct.last_known_unread != unread
        {
            acct.last_known_unread = unread;
            let _ = self.save();
        }
    }

    pub fn cycle_notif_sound(&mut self, id: Uuid) -> String {
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            let idx = NOTIF_SOUNDS
                .iter()
                .position(|s| *s == acct.notif_sound)
                .unwrap_or(0);
            let next = NOTIF_SOUNDS[(idx + 1) % NOTIF_SOUNDS.len()].to_string();
            acct.notif_sound = next.clone();
            let _ = self.save();
            return next;
        }
        "default".to_string()
    }

    pub fn set_notif_sound(&mut self, id: Uuid, sound: String) -> String {
        let valid = NOTIF_SOUNDS
            .iter()
            .find(|s| s.eq_ignore_ascii_case(sound.trim()))
            .copied()
            .unwrap_or("default")
            .to_string();
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.notif_sound = valid.clone();
            let _ = self.save();
        }
        valid
    }

    pub fn set_group(&mut self, id: Uuid, group: Option<String>) {
        let normalized = group.map(|g| g.trim().to_string()).filter(|g| !g.is_empty());
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.group = normalized;
            let _ = self.save();
        }
    }

    pub fn set_locked(&mut self, id: Uuid, locked: bool) {
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.locked = locked;
            let _ = self.save();
        }
    }

    pub fn toggle_group_collapsed(&mut self, group: &str) {
        if let Some(pos) = self.collapsed_groups.iter().position(|g| g == group) {
            self.collapsed_groups.remove(pos);
        } else {
            self.collapsed_groups.push(group.to_string());
        }
        let _ = self.save();
    }

    pub fn add_reminder(&mut self, account_id: Uuid, note: String, delay_secs: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.reminders.push(Reminder {
            id: Uuid::new_v4(),
            account_id,
            note: note.trim().to_string(),
            due_at_epoch_secs: now + delay_secs,
        });
        let _ = self.save();
    }

    pub fn remove_reminder(&mut self, id: Uuid) {
        let before = self.reminders.len();
        self.reminders.retain(|r| r.id != id);
        if self.reminders.len() != before {
            let _ = self.save();
        }
    }

    pub fn take_due_reminders(&mut self) -> Vec<Reminder> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (due, remaining): (Vec<_>, Vec<_>) = self
            .reminders
            .iter()
            .cloned()
            .partition(|r| r.due_at_epoch_secs <= now);
        if !due.is_empty() {
            self.reminders = remaining;
            let _ = self.save();
        }
        due
    }

    pub fn remove_account(&mut self, id: Uuid) -> bool {
        if self.accounts.len() <= 1 {
            return false;
        }
        let before = self.accounts.len();
        self.accounts.retain(|a| a.id != id);
        if self.accounts.len() != before {
            for (i, acct) in self.accounts.iter_mut().enumerate() {
                acct.order = i;
            }
            if self.active_account == Some(id) {
                self.active_account = self.accounts.first().map(|a| a.id);
            }
            let avatar_cache = Self::avatars_dir().join(format!("{id}.jpg"));
            let _ = fs::remove_file(avatar_cache);
            let _ = self.save();
            return true;
        }
        false
    }

    pub fn update_profile(
        &mut self,
        id: Uuid,
        display_name: Option<String>,
        phone: Option<String>,
        avatar_url: Option<String>,
    ) -> bool {
        let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        let mut changed = false;
        if let Some(name) = display_name.filter(|s| !s.trim().is_empty())
            && acct.display_name.as_deref() != Some(name.as_str())
        {
            acct.display_name = Some(name);
            changed = true;
        }
        if let Some(p) = phone.filter(|s| !s.trim().is_empty())
            && acct.phone.as_deref() != Some(p.as_str())
        {
            acct.phone = Some(p);
            changed = true;
        }
        if let Some(url) = avatar_url.filter(|s| !s.trim().is_empty() && s.len() <= MAX_AVATAR_BYTES)
            && acct.avatar_url.as_deref() != Some(url.as_str())
        {
            acct.avatar_url = Some(url);
            changed = true;
        }
        if changed {
            let _ = self.save();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_file_permissions_are_0700_and_0600() {
        let base = std::env::temp_dir().join(format!("hopr-perm-test-{}", Uuid::new_v4()));
        let file = base.join("nested/accounts.toml");
        AppConfig::write_secure_file(&file, b"test = true\n").unwrap();

        let dir_mode = fs::metadata(file.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;

        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn oversized_avatar_is_rejected() {
        let mut cfg = AppConfig::default();
        let id = cfg.add_account().id;
        let huge = "a".repeat(MAX_AVATAR_BYTES + 1);
        cfg.set_custom_avatar(id, Some(huge));
        assert!(cfg.get(id).unwrap().custom_avatar_url.is_none());
    }
}
