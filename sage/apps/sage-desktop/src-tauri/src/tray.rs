use sage_core::config::Config;
use sage_core::Daemon;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, Manager,
};

pub fn setup_tray(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "打开面板", true, None::<&str>)?;
    let brief = MenuItem::with_id(app, "brief", "立即生成简报", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open, &brief, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("Sage — 你的参谋")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "brief" => {
                tauri::async_runtime::spawn(async {
                    let config_path = dirs::home_dir()
                        .map(|h| h.join(".sage/config.toml"))
                        .unwrap_or_default();
                    let config = Config::load_or_default(&config_path);
                    match Daemon::new(config) {
                        Ok(daemon) => {
                            if let Err(e) = daemon.heartbeat_once().await {
                                tracing::error!("手动简报失败: {e}");
                            }
                        }
                        Err(e) => tracing::error!("简报初始化失败: {e}"),
                    }
                });
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
