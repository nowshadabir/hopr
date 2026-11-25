use crate::config::AppConfig;
use crate::webview_manager::AppEvent;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;
use tao::event_loop::EventLoopProxy;

fn socket_path() -> PathBuf {
    AppConfig::app_support_dir().join("hopr.sock")
}

/// Attempts to acquire the single-instance socket.
/// Returns `Some(UnixListener)` if this is the primary instance, or `None` if an existing
/// instance was signaled to focus and this process should exit.
pub fn acquire_single_instance() -> Option<UnixListener> {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        let _ = AppConfig::ensure_secure_dir(parent);
    }

    if let Ok(mut stream) = UnixStream::connect(&path) {
        let _ = stream.write_all(b"FOCUS\n");
        return None;
    }

    let _ = fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(listener) => {
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            Some(listener)
        }
        Err(_) => None,
    }
}

pub fn spawn_instance_listener(listener: UnixListener, proxy: EventLoopProxy<AppEvent>) {
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut buf = [0u8; 16];
            if let Ok(n) = s.read(&mut buf)
                && n > 0
                && proxy.send_event(AppEvent::FocusWindow).is_err()
            {
                break;
            }
        }
    });
}
