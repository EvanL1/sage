use std::ffi::{CString, c_char};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

extern "C" {
    fn sage_notification_init(on_click: extern "C" fn(*const c_char));
    fn sage_notification_send(title: *const c_char, body: *const c_char, route: *const c_char);
}

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// 通知点击回调（从 ObjC delegate 调用，在系统线程上执行）
extern "C" fn on_notification_click(route: *const c_char) {
    let route = unsafe { std::ffi::CStr::from_ptr(route) }
        .to_str()
        .unwrap_or("/");

    tracing::info!("Notification clicked, route={route}");

    if let Some(handle) = APP_HANDLE.get() {
        // 确保窗口可见
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
        let _ = handle.emit("navigate-to", route);
    }
}

/// 初始化原生通知系统（在 Tauri setup 中调用一次）
pub fn init(handle: &AppHandle) {
    let _ = APP_HANDLE.set(handle.clone());
    unsafe { sage_notification_init(on_notification_click) };
    tracing::info!("Native notification system initialized");
}

/// 发送系统通知
pub fn send(title: &str, body: &str, route: &str) {
    let title = CString::new(title).unwrap_or_default();
    let body = CString::new(body).unwrap_or_default();
    let route = CString::new(route).unwrap_or_default();
    unsafe { sage_notification_send(title.as_ptr(), body.as_ptr(), route.as_ptr()) };
}
