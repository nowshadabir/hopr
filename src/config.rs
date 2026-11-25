use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

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
}

fn default_label() -> String {
    "Personal".to_string()
}

fn default_hibernate_mins() -> u64 {
    15
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
        }
    }

    pub fn effective_name(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
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

    pub fn sleep_summary(&self) -> String {
        if self.keep_connected {
            "Always connected".to_string()
        } else if self.periodic_wake_mins > 0 {
            format!(
                "Sleeps after {} min idle · wake every {} min",
                self.hibernate_after_mins, self.periodic_wake_mins
            )
        } else {
            format!("Sleeps after {} min idle", self.hibernate_after_mins)
        }
    }

    pub fn short_sleep_summary(&self) -> String {
        if self.keep_connected {
            "Sleep: Never".to_string()
        } else if self.periodic_wake_mins > 0 {
            format!(
                "Sleep: {} min, wake every {}",
                self.hibernate_after_mins, self.periodic_wake_mins
            )
        } else {
            format!("Sleep: {} min", self.hibernate_after_mins)
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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            active_account: None,
            show_message_previews: true,
            dock_badge_enabled: true,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        if let Some(dirs) = ProjectDirs::from("com", "hopr", "app") {
            dirs.data_dir().join("accounts.toml")
        } else {
            PathBuf::from("accounts.toml")
        }
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
            return cfg;
        }

        let default_acct = AccountConfig::new("Personal", "Personal", 0, true);
        let active = Some(default_acct.id);
        let cfg = Self {
            accounts: vec![default_acct],
            active_account: active,
            show_message_previews: true,
            dock_badge_enabled: true,
        };
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, body).map_err(|e| e.to_string())
    }

    pub fn add_account(&mut self) -> &AccountConfig {
        let idx = self.accounts.len();
        let name = format!("Account {}", idx + 1);
        let label = match idx {
            0 => "Personal",
            1 => "Work",
            _ => "Account",
        };
        let acct = AccountConfig::new(name, label, idx, false);
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

    pub fn set_custom_avatar(&mut self, id: Uuid, avatar_url: Option<String>) {
        if let Some(acct) = self.accounts.iter_mut().find(|a| a.id == id) {
            acct.custom_avatar_url = avatar_url.filter(|s| !s.trim().is_empty());
            let _ = self.save();
        }
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
        if let Some(url) = avatar_url.filter(|s| !s.trim().is_empty())
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
