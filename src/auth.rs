use crate::config::AppConfig;
use crate::webview_manager::AppEvent;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tao::event_loop::EventLoopProxy;

pub const AUTH_API_BASE: &str = "https://hopr-auth.ontomeel.com";
const KEYRING_SERVICE: &str = "com.hopr.desktop";
const KEYRING_USER: &str = "session_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: u64,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub subscription_status: String,
    #[serde(default)]
    pub max_accounts: Option<usize>,
    #[serde(default)]
    pub modules: Vec<String>,
}

impl AuthUser {
    pub fn can_add_account(&self, current_count: usize) -> bool {
        self.max_accounts.is_none_or(|limit| current_count < limit)
    }

    #[allow(dead_code)]
    pub fn has_module(&self, module: &str) -> bool {
        self.modules.iter().any(|m| m == module)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthAction {
    Login,
    Signup,
    VerifyOtp,
    ResendOtp,
    CheckSession,
}

#[derive(Debug, Clone)]
pub enum AuthOutcome {
    Authenticated {
        user: AuthUser,
    },
    SignupOtpSent {
        email: String,
        message: String,
    },
    OtpResent {
        message: String,
    },
    LoggedOut,
    Error {
        action: AuthAction,
        status: u16,
        code: String,
        message: String,
        retry_after_secs: Option<u64>,
    },
}

#[derive(Deserialize)]
struct ApiAuthSuccess {
    ok: bool,
    token: Option<String>,
    user: Option<AuthUser>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    #[allow(dead_code)]
    ok: Option<bool>,
    code: Option<String>,
    message: Option<String>,
}

pub fn device_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Hopr Desktop (macOS)"
    } else if cfg!(target_os = "windows") {
        "Hopr Desktop (Windows)"
    } else {
        "Hopr Desktop"
    }
}

pub fn get_stored_token() -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    match entry.get_password() {
        Ok(tok) if !tok.trim().is_empty() => Some(tok),
        _ => None,
    }
}

pub fn store_token(token: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.set_password(token);
    }
}

pub fn clear_token() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
    let _ = fs::remove_file(cached_user_path());
}

fn cached_user_path() -> PathBuf {
    AppConfig::app_support_dir().join("auth_user.json")
}

pub fn load_cached_user() -> Option<AuthUser> {
    let raw = fs::read_to_string(cached_user_path()).ok()?;
    serde_json::from_str::<AuthUser>(&raw).ok()
}

pub fn save_cached_user(user: &AuthUser) {
    if let Ok(bytes) = serde_json::to_vec_pretty(user) {
        let _ = AppConfig::write_secure_file(&cached_user_path(), &bytes);
    }
}

fn http_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(15));
    if let Ok(connector) = native_tls::TlsConnector::new() {
        builder = builder.tls_connector(Arc::new(connector));
    }
    builder.build()
}

fn map_ureq_error(action: AuthAction, err: ureq::Error) -> AuthOutcome {
    match err {
        ureq::Error::Status(status, resp) => {
            let retry_after_secs = resp
                .header("Retry-After")
                .and_then(|v| v.trim().parse::<u64>().ok());
            let parsed = resp.into_json::<ApiErrorBody>().ok();
            let code = parsed
                .as_ref()
                .and_then(|b| b.code.clone())
                .unwrap_or_else(|| format!("HTTP_{status}"));
            let message = parsed
                .and_then(|b| b.message)
                .unwrap_or_else(|| format!("Request failed (HTTP {status})."));
            AuthOutcome::Error {
                action,
                status,
                code,
                message,
                retry_after_secs,
            }
        }
        ureq::Error::Transport(t) => AuthOutcome::Error {
            action,
            status: 0,
            code: "NETWORK_ERROR".to_string(),
            message: format!("Network error: {t}"),
            retry_after_secs: None,
        },
    }
}

pub fn spawn_login(
    proxy: EventLoopProxy<AppEvent>,
    email: String,
    password: String,
) {
    thread::spawn(move || {
        let url = format!("{AUTH_API_BASE}/auth/login");
        let payload = serde_json::json!({
            "email": email.trim(),
            "password": password,
            "device_name": device_name(),
        });
        let outcome = match http_agent().post(&url).send_json(payload) {
            Ok(resp) => match resp.into_json::<ApiAuthSuccess>() {
                Ok(data) if data.ok => {
                    if let (Some(token), Some(user)) = (data.token, data.user) {
                        store_token(&token);
                        save_cached_user(&user);
                        AuthOutcome::Authenticated { user }
                    } else {
                        AuthOutcome::Error {
                            action: AuthAction::Login,
                            status: 500,
                            code: "INVALID_RESPONSE".to_string(),
                            message: "Server did not return a session token.".to_string(),
                            retry_after_secs: None,
                        }
                    }
                }
                Ok(data) => AuthOutcome::Error {
                    action: AuthAction::Login,
                    status: 400,
                    code: "LOGIN_FAILED".to_string(),
                    message: data.message.unwrap_or_else(|| "Login failed.".to_string()),
                    retry_after_secs: None,
                },
                Err(e) => AuthOutcome::Error {
                    action: AuthAction::Login,
                    status: 500,
                    code: "PARSE_ERROR".to_string(),
                    message: format!("Invalid server response: {e}"),
                    retry_after_secs: None,
                },
            },
            Err(e) => map_ureq_error(AuthAction::Login, e),
        };
        let _ = proxy.send_event(AppEvent::AuthResult(outcome));
    });
}

pub fn spawn_signup(
    proxy: EventLoopProxy<AppEvent>,
    name: String,
    phone: String,
    email: String,
    password: String,
) {
    thread::spawn(move || {
        let trimmed_email = email.trim().to_string();
        let url = format!("{AUTH_API_BASE}/auth/signup");
        let payload = serde_json::json!({
            "name": name.trim(),
            "phone": phone.trim(),
            "email": trimmed_email,
            "password": password,
        });
        let outcome = match http_agent().post(&url).send_json(payload) {
            Ok(resp) => match resp.into_json::<ApiAuthSuccess>() {
                Ok(data) if data.ok => AuthOutcome::SignupOtpSent {
                    email: trimmed_email,
                    message: data
                        .message
                        .unwrap_or_else(|| "Verification code sent to your email.".to_string()),
                },
                Ok(data) => AuthOutcome::Error {
                    action: AuthAction::Signup,
                    status: 400,
                    code: "SIGNUP_FAILED".to_string(),
                    message: data.message.unwrap_or_else(|| "Sign up failed.".to_string()),
                    retry_after_secs: None,
                },
                Err(e) => AuthOutcome::Error {
                    action: AuthAction::Signup,
                    status: 500,
                    code: "PARSE_ERROR".to_string(),
                    message: format!("Invalid server response: {e}"),
                    retry_after_secs: None,
                },
            },
            Err(e) => map_ureq_error(AuthAction::Signup, e),
        };
        let _ = proxy.send_event(AppEvent::AuthResult(outcome));
    });
}

pub fn spawn_verify_otp(proxy: EventLoopProxy<AppEvent>, email: String, otp: String) {
    thread::spawn(move || {
        let url = format!("{AUTH_API_BASE}/auth/verify-otp");
        let payload = serde_json::json!({
            "email": email.trim(),
            "otp": otp.trim(),
            "device_name": device_name(),
        });
        let outcome = match http_agent().post(&url).send_json(payload) {
            Ok(resp) => match resp.into_json::<ApiAuthSuccess>() {
                Ok(data) if data.ok => {
                    if let (Some(token), Some(user)) = (data.token, data.user) {
                        store_token(&token);
                        save_cached_user(&user);
                        AuthOutcome::Authenticated { user }
                    } else {
                        AuthOutcome::Error {
                            action: AuthAction::VerifyOtp,
                            status: 500,
                            code: "INVALID_RESPONSE".to_string(),
                            message: "Server did not return a session token.".to_string(),
                            retry_after_secs: None,
                        }
                    }
                }
                Ok(data) => AuthOutcome::Error {
                    action: AuthAction::VerifyOtp,
                    status: 400,
                    code: "INVALID_OTP".to_string(),
                    message: data.message.unwrap_or_else(|| "Invalid verification code.".to_string()),
                    retry_after_secs: None,
                },
                Err(e) => AuthOutcome::Error {
                    action: AuthAction::VerifyOtp,
                    status: 500,
                    code: "PARSE_ERROR".to_string(),
                    message: format!("Invalid server response: {e}"),
                    retry_after_secs: None,
                },
            },
            Err(e) => map_ureq_error(AuthAction::VerifyOtp, e),
        };
        let _ = proxy.send_event(AppEvent::AuthResult(outcome));
    });
}

pub fn spawn_resend_otp(proxy: EventLoopProxy<AppEvent>, email: String) {
    thread::spawn(move || {
        let url = format!("{AUTH_API_BASE}/auth/resend-otp");
        let payload = serde_json::json!({ "email": email.trim() });
        let outcome = match http_agent().post(&url).send_json(payload) {
            Ok(resp) => match resp.into_json::<ApiAuthSuccess>() {
                Ok(data) if data.ok => AuthOutcome::OtpResent {
                    message: data
                        .message
                        .unwrap_or_else(|| "A new verification code was sent.".to_string()),
                },
                Ok(data) => AuthOutcome::Error {
                    action: AuthAction::ResendOtp,
                    status: 400,
                    code: "RESEND_FAILED".to_string(),
                    message: data.message.unwrap_or_else(|| "Failed to resend code.".to_string()),
                    retry_after_secs: None,
                },
                Err(e) => AuthOutcome::Error {
                    action: AuthAction::ResendOtp,
                    status: 500,
                    code: "PARSE_ERROR".to_string(),
                    message: format!("Invalid server response: {e}"),
                    retry_after_secs: None,
                },
            },
            Err(e) => map_ureq_error(AuthAction::ResendOtp, e),
        };
        let _ = proxy.send_event(AppEvent::AuthResult(outcome));
    });
}

pub fn spawn_check_session(proxy: EventLoopProxy<AppEvent>, token: String) {
    thread::spawn(move || {
        let url = format!("{AUTH_API_BASE}/auth/me");
        let auth_header = format!("Bearer {token}");
        let outcome = match http_agent()
            .get(&url)
            .set("Authorization", &auth_header)
            .call()
        {
            Ok(resp) => match resp.into_json::<ApiAuthSuccess>() {
                Ok(data) if data.ok => {
                    if let Some(user) = data.user {
                        save_cached_user(&user);
                        AuthOutcome::Authenticated { user }
                    } else {
                        AuthOutcome::Error {
                            action: AuthAction::CheckSession,
                            status: 500,
                            code: "INVALID_RESPONSE".to_string(),
                            message: "Invalid /auth/me response.".to_string(),
                            retry_after_secs: None,
                        }
                    }
                }
                Ok(data) => AuthOutcome::Error {
                    action: AuthAction::CheckSession,
                    status: 401,
                    code: "UNAUTHORIZED".to_string(),
                    message: data.message.unwrap_or_else(|| "Session invalid.".to_string()),
                    retry_after_secs: None,
                },
                Err(e) => AuthOutcome::Error {
                    action: AuthAction::CheckSession,
                    status: 500,
                    code: "PARSE_ERROR".to_string(),
                    message: format!("Invalid server response: {e}"),
                    retry_after_secs: None,
                },
            },
            Err(e) => map_ureq_error(AuthAction::CheckSession, e),
        };
        let _ = proxy.send_event(AppEvent::AuthResult(outcome));
    });
}

pub fn spawn_logout(proxy: EventLoopProxy<AppEvent>) {
    let token = get_stored_token();
    clear_token();
    thread::spawn(move || {
        if let Some(tok) = token {
            let url = format!("{AUTH_API_BASE}/auth/logout");
            let auth_header = format!("Bearer {tok}");
            let _ = http_agent()
                .post(&url)
                .set("Authorization", &auth_header)
                .send_json(serde_json::json!({}));
        }
        let _ = proxy.send_event(AppEvent::AuthResult(AuthOutcome::LoggedOut));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_auth_success_and_error_payloads() {
        let ok_json = r#"{
            "ok": true,
            "token": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "user": {
                "id": 7,
                "name": "Alice Hopper",
                "email": "alice@example.com",
                "phone": "+1 555-0100",
                "subscription_status": "active"
            }
        }"#;
        let parsed: ApiAuthSuccess = serde_json::from_str(ok_json).unwrap();
        assert!(parsed.ok);
        let user = parsed.user.unwrap();
        assert_eq!(user.email, "alice@example.com");
        assert!(user.can_add_account(10));
        assert!(user.modules.is_empty());

        let with_limits = r#"{
            "ok": true,
            "user": {
                "id": 8,
                "name": "Bob",
                "email": "bob@example.com",
                "phone": "+8801700000000",
                "subscription_status": "active",
                "max_accounts": 2,
                "modules": ["whatsapp", "invoices"]
            }
        }"#;
        let parsed2: ApiAuthSuccess = serde_json::from_str(with_limits).unwrap();
        let u2 = parsed2.user.unwrap();
        assert!(u2.can_add_account(1));
        assert!(!u2.can_add_account(2));
        assert!(u2.has_module("invoices"));
        assert!(!u2.has_module("quotes"));

        let err_json = r#"{"ok":false,"code":"SUBSCRIPTION_REQUIRED","message":"Active subscription required."}"#;
        let err: ApiErrorBody = serde_json::from_str(err_json).unwrap();
        assert_eq!(err.code.as_deref(), Some("SUBSCRIPTION_REQUIRED"));
    }
}
