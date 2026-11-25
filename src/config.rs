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
    pub lock_pin_hash: Option<String>,
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
            lock_pin_hash: None,
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

    pub fn has_lock_pin(&self) -> bool {
        self.lock_pin_hash
            .as_deref()
            .is_some_and(|h| !h.is_empty())
    }

    pub fn set_lock_pin(&mut self, pin: &str) -> Result<(), String> {
        let trimmed = pin.trim();
        if trimmed.len() < 4 || trimmed.len() > 32 {
            return Err("PIN must be between 4 and 32 characters.".to_string());
        }
        self.lock_pin_hash = Some(hash_lock_pin(trimmed));
        let _ = self.save();
        Ok(())
    }

    pub fn clear_lock_pin(&mut self) {
        self.lock_pin_hash = None;
        let _ = self.save();
    }

    pub fn verify_lock_pin(&self, pin: &str) -> bool {
        let Some(expected) = self.lock_pin_hash.as_deref() else {
            return false;
        };
        let actual = hash_lock_pin(pin.trim());
        constant_time_eq(expected.as_bytes(), actual.as_bytes())
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

    #[cfg(unix)]
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

    #[test]
    fn lock_pin_set_verify_and_clear() {
        let mut cfg = AppConfig::default();
        assert!(!cfg.has_lock_pin());
        assert!(!cfg.verify_lock_pin("1234"));
        assert!(cfg.set_lock_pin("12").is_err());
        assert!(cfg.set_lock_pin("2580").is_ok());
        assert!(cfg.has_lock_pin());
        assert!(cfg.verify_lock_pin("2580"));
        assert!(!cfg.verify_lock_pin("0000"));
        cfg.clear_lock_pin();
        assert!(!cfg.has_lock_pin());
        assert!(!cfg.verify_lock_pin("2580"));
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn hash_lock_pin(pin: &str) -> String {
    let mut data = Vec::with_capacity(12 + pin.len());
    data.extend_from_slice(b"hopr-pin-v1:");
    data.extend_from_slice(pin.as_bytes());
    sha256_hex(&data)
}

fn sha256_hex(input: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let bit_len = (input.len() as u64) * 8;
    let mut msg = input.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = String::with_capacity(64);
    for word in h {
        out.push_str(&format!("{word:08x}"));
    }
    out
}
