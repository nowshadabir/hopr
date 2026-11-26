use crate::config::AppConfig;
use crate::platform::downloads_dir;
use crate::webview_manager::{AppEvent, allow_whatsapp_or_open_external};
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use tao::event_loop::EventLoopProxy;
use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use uuid::Uuid;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED, COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, CreateCoreWebView2EnvironmentWithOptions,
    ICoreWebView2, ICoreWebView2_4, ICoreWebView2_19, ICoreWebView2Controller,
    ICoreWebView2Environment, ICoreWebView2Environment10, ICoreWebView2Settings2,
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DownloadStartingEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, StateChangedEventHandler, WebMessageReceivedEventHandler,
    take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
};
use windows_core::{HSTRING, Interface, PCWSTR};
use wry::Rect;

const SHARED_BROWSER_ARGS: &str = "\
    --disable-features=msEdgeTranslate,msEdgeShopping,msEdgeSidebarV2,OptimizationHints,msEdgeMiniMenu,msWebOOUI,msPdfOOUI,msSmartScreenProtection \
    --disable-background-networking \
    --disable-default-apps \
    --disable-extensions \
    --disable-sync \
    --metrics-recording-only \
    --no-first-run \
    --js-flags=--max-old-space-size=192";

pub struct WinProfileWebView {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

impl Drop for WinProfileWebView {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }
}

fn pump_until_received<T>(rx: mpsc::Receiver<Result<T, String>>) -> Result<T, String> {
    loop {
        match rx.try_recv() {
            Ok(res) => return res,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("WebView2 async callback channel disconnected".to_string());
            }
            Err(mpsc::TryRecvError::Empty) => unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            },
        }
    }
}

fn shared_environment() -> Result<ICoreWebView2Environment, String> {
    thread_local! {
        static SHARED_ENV: RefCell<Option<ICoreWebView2Environment>> = const { RefCell::new(None) };
    }
    SHARED_ENV.with(|cell| {
        if let Some(env) = cell.borrow().as_ref() {
            return Ok(env.clone());
        }

        let env_dir = AppConfig::app_support_dir().join("webview2-env");
        AppConfig::ensure_secure_dir(&env_dir)?;
        let env_dir_h = HSTRING::from(env_dir.to_string_lossy().as_ref());

        let options = webview2_com::CoreWebView2EnvironmentOptions::default();
        unsafe {
            options.set_additional_browser_arguments(SHARED_BROWSER_ARGS.to_string());
        }
        let env_opts: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2EnvironmentOptions =
            options.into();

        let (tx, rx) = mpsc::channel();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |error_code, environment| {
                let res = error_code
                    .map_err(|e| format!("CreateCoreWebView2Environment failed: {e}"))
                    .and_then(|_| {
                        environment.ok_or_else(|| "Missing ICoreWebView2Environment".to_string())
                    });
                let _ = tx.send(res);
                Ok(())
            },
        ));

        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                &env_dir_h,
                &env_opts,
                &handler,
            )
            .map_err(|e| format!("CreateCoreWebView2EnvironmentWithOptions failed: {e}"))?;
        }

        let env = pump_until_received(rx)?;
        *cell.borrow_mut() = Some(env.clone());
        Ok(env)
    })
}

fn add_init_script(webview: &ICoreWebView2, js: &str) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
        move |error_code, _| {
            let _ = tx.send(
                error_code.map_err(|e| format!("AddScriptToExecuteOnDocumentCreated failed: {e}")),
            );
            Ok(())
        },
    ));
    let js_h = HSTRING::from(js);
    unsafe {
        webview
            .AddScriptToExecuteOnDocumentCreated(&js_h, &handler)
            .map_err(|e| format!("AddScriptToExecuteOnDocumentCreated call failed: {e}"))?;
    }
    pump_until_received(rx)
}

fn rect_to_win32(bounds: Rect, scale_factor: f64) -> RECT {
    let pos = bounds.position.to_physical::<i32>(scale_factor);
    let size = bounds.size.to_physical::<i32>(scale_factor);
    RECT {
        left: pos.x,
        top: pos.y,
        right: pos.x + size.width,
        bottom: pos.y + size.height,
    }
}

impl WinProfileWebView {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        account_id: Uuid,
        profile_name: &str,
        target_url: &str,
        user_agent: &str,
        init_js: &str,
        bounds: Rect,
        visible: bool,
        window: &Window,
        proxy: &EventLoopProxy<AppEvent>,
        active_downloads: Arc<AtomicUsize>,
    ) -> Result<Self, String> {
        let env = shared_environment()?;
        let env10: ICoreWebView2Environment10 = env
            .cast()
            .map_err(|e| format!("WebView2 Runtime does not support multi-profile (ICoreWebView2Environment10): {e}"))?;

        let ctrl_opts = unsafe {
            env10
                .CreateCoreWebView2ControllerOptions()
                .map_err(|e| format!("CreateCoreWebView2ControllerOptions failed: {e}"))?
        };
        let profile_h = HSTRING::from(profile_name);
        unsafe {
            ctrl_opts
                .SetProfileName(&profile_h)
                .map_err(|e| format!("SetProfileName failed: {e}"))?;
            ctrl_opts
                .SetIsInPrivateModeEnabled(false)
                .map_err(|e| format!("SetIsInPrivateModeEnabled failed: {e}"))?;
        }

        let parent_hwnd = HWND(window.hwnd() as *mut c_void);
        let (tx, rx) = mpsc::channel();
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
            move |error_code, controller| {
                let res = error_code
                    .map_err(|e| format!("CreateCoreWebView2ControllerWithOptions failed: {e}"))
                    .and_then(|_| {
                        controller.ok_or_else(|| "Missing ICoreWebView2Controller".to_string())
                    });
                let _ = tx.send(res);
                Ok(())
            },
        ));

        unsafe {
            env10
                .CreateCoreWebView2ControllerWithOptions(parent_hwnd, &ctrl_opts, &handler)
                .map_err(|e| format!("CreateCoreWebView2ControllerWithOptions call failed: {e}"))?;
        }

        let controller = pump_until_received(rx)?;
        let webview = unsafe {
            controller
                .CoreWebView2()
                .map_err(|e| format!("CoreWebView2() failed: {e}"))?
        };

        unsafe {
            if let Ok(settings) = webview.Settings() {
                let _ = settings.SetAreDevToolsEnabled(cfg!(debug_assertions));
                if let Ok(settings2) = settings.cast::<ICoreWebView2Settings2>() {
                    let ua_h = HSTRING::from(user_agent);
                    let _ = settings2.SetUserAgent(&ua_h);
                }
            }
        }

        add_init_script(
            &webview,
            "Object.defineProperty(window, 'ipc', { value: Object.freeze({ postMessage: s => window.chrome.webview.postMessage(s) }) });",
        )?;
        add_init_script(&webview, init_js)?;

        let ipc_proxy = proxy.clone();
        let mut token = 0i64;
        unsafe {
            let _ = webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut src = windows_core::PWSTR::null();
                    args.Source(&mut src)?;
                    let origin = take_pwstr(src);
                    let mut msg = windows_core::PWSTR::null();
                    args.TryGetWebMessageAsString(&mut msg)?;
                    let body = take_pwstr(msg);
                    let _ = ipc_proxy.send_event(AppEvent::WebViewIpc {
                        account_id,
                        origin,
                        body,
                    });
                    Ok(())
                })),
                &mut token,
            );

            let _ = webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = windows_core::PWSTR::null();
                    args.Uri(&mut uri)?;
                    let url = take_pwstr(uri);
                    if !allow_whatsapp_or_open_external(&url) {
                        args.SetCancel(true)?;
                    }
                    Ok(())
                })),
                &mut token,
            );

            let _ = webview.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = windows_core::PWSTR::null();
                    args.Uri(&mut uri)?;
                    let url = take_pwstr(uri);
                    let _ = allow_whatsapp_or_open_external(&url);
                    args.SetHandled(true)?;
                    Ok(())
                })),
                &mut token,
            );

            let dl_start = Arc::clone(&active_downloads);
            let dl_end = Arc::clone(&active_downloads);
            if let Ok(webview4) = webview.cast::<ICoreWebView2_4>() {
                let _ = webview4.add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        if let Ok(op) = args.DownloadOperation() {
                            let mut uri = windows_core::PWSTR::null();
                            let filename = if op.Uri(&mut uri).is_ok() {
                                let url = take_pwstr(uri);
                                Path::new(url.split('?').next().unwrap_or("download"))
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("whatsapp-download")
                                    .to_string()
                            } else {
                                "whatsapp-download".to_string()
                            };
                            let dest = downloads_dir().join(filename);
                            let dest_h = HSTRING::from(dest.to_string_lossy().as_ref());
                            let _ = args.SetResultFilePath(&dest_h);
                            dl_start.fetch_add(1, Ordering::Relaxed);
                            let dl_done = Arc::clone(&dl_end);
                            let mut st_token = 0i64;
                            let _ = op.add_StateChanged(
                                &StateChangedEventHandler::create(Box::new(move |op, _| {
                                    if let Some(op) = op {
                                        let mut state = Default::default();
                                        if op.State(&mut state).is_ok()
                                            && (state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED
                                                || state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED)
                                        {
                                            dl_done.fetch_sub(1, Ordering::Relaxed);
                                        }
                                    }
                                    Ok(())
                                })),
                                &mut st_token,
                            );
                        }
                        Ok(())
                    })),
                    &mut token,
                );
            }

            let scale = window.scale_factor();
            let _ = controller.SetBounds(rect_to_win32(bounds, scale));
            let _ = controller.SetIsVisible(visible);
            let url_h = HSTRING::from(target_url);
            webview
                .Navigate(&url_h)
                .map_err(|e| format!("Navigate failed: {e}"))?;
        }

        let instance = Self { controller, webview };
        instance.set_memory_usage_low(!visible);
        Ok(instance)
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = self.controller.SetIsVisible(visible);
        }
        self.set_memory_usage_low(!visible);
    }

    pub fn set_bounds(&self, bounds: Rect, scale_factor: f64) {
        unsafe {
            let _ = self.controller.SetBounds(rect_to_win32(bounds, scale_factor));
        }
    }

    pub fn focus(&self) {
        unsafe {
            let _ = self
                .controller
                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    }

    pub fn load_url(&self, url: &str) {
        let url_h = HSTRING::from(url);
        unsafe {
            let _ = self.webview.Navigate(&url_h);
        }
    }

    pub fn set_memory_usage_low(&self, low: bool) {
        if let Ok(wv19) = self.webview.cast::<ICoreWebView2_19>() {
            let level = if low {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            };
            unsafe {
                let _ = wv19.SetMemoryUsageTargetLevel(level);
            }
        }
    }
}
