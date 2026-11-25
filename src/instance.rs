#[cfg(unix)]
mod imp {
    use crate::config::AppConfig;
    use crate::webview_manager::AppEvent;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::thread;
    use tao::event_loop::EventLoopProxy;

    pub type InstanceHandle = UnixListener;

    fn socket_path() -> PathBuf {
        AppConfig::app_support_dir().join("hopr.sock")
    }

    pub fn acquire_single_instance() -> Option<InstanceHandle> {
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

    pub fn spawn_instance_listener(listener: InstanceHandle, proxy: EventLoopProxy<AppEvent>) {
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
}

#[cfg(windows)]
mod imp {
    use crate::webview_manager::AppEvent;
    use tao::event_loop::EventLoopProxy;
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    pub struct InstanceHandle(#[allow(dead_code)] HANDLE);

    pub fn acquire_single_instance() -> Option<InstanceHandle> {
        let name: Vec<u16> = "Local\\HoprSingleInstance\0".encode_utf16().collect();
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if handle.is_null() {
                return None;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let title: Vec<u16> = "Hopr\0".encode_utf16().collect();
                let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                if !hwnd.is_null() {
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                }
                return None;
            }
            Some(InstanceHandle(handle))
        }
    }

    pub fn spawn_instance_listener(_handle: InstanceHandle, _proxy: EventLoopProxy<AppEvent>) {}
}

pub use imp::{acquire_single_instance, spawn_instance_listener};
