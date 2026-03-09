// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod tray;

use std::sync::Mutex;

use sage_core::config::Config;
use sage_core::onboarding::OnboardingState;
use sage_core::store::Store;
use sage_core::Daemon;

/// Tauri 应用共享状态
pub struct AppState {
    pub store: Store,
    pub onboarding: Mutex<Option<OnboardingState>>,
}

/// 检测独立 daemon 进程是否已在运行（精确匹配进程名）
fn is_external_daemon_running() -> bool {
    std::process::Command::new("pgrep")
        .arg("-x")
        .arg("sage-daemon")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn main() {
    let data_dir = dirs::home_dir()
        .map(|h| h.join(".sage/data"))
        .expect("无法确定 home 目录");
    std::fs::create_dir_all(&data_dir).expect("创建数据目录失败");

    let db_path = data_dir.join("sage.db");
    let store = Store::open(&db_path).expect("打开数据库失败");

    let app_state = AppState {
        store,
        onboarding: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::get_profile,
            commands::save_profile,
            commands::get_onboarding_step,
            commands::submit_onboarding_step,
            commands::get_suggestions,
            commands::submit_feedback,
            commands::get_system_status,
            commands::reset_onboarding,
            commands::discover_providers,
            commands::quick_setup,
            commands::save_provider_config,
            commands::get_provider_configs,
            commands::test_provider,
            commands::chat,
            commands::list_chat_sessions,
            commands::get_chat_history,
            commands::get_memories,
            commands::extract_memories,
            commands::sync_memory,
            commands::delete_memory,
            commands::export_memories,
            commands::import_memories,
            commands::save_assessment,
            commands::get_reports,
            commands::get_latest_reports,
            commands::trigger_test_report,
            commands::ingest_sessions,
        ])
        .setup(|app| {
            tray::setup_tray(app)?;

            // 嵌入 daemon 事件循环：新用户无需安装独立 daemon
            if is_external_daemon_running() {
                tracing::info!("检测到独立 daemon 进程，跳过内嵌事件循环");
            } else {
                let config_path = dirs::home_dir()
                    .map(|h| h.join(".sage/config.toml"))
                    .expect("无法确定 home 目录");
                let config = Config::load_or_default(&config_path);
                tracing::info!(
                    "启动内嵌 daemon（heartbeat: {}s）",
                    config.daemon.heartbeat_interval_secs
                );

                tauri::async_runtime::spawn(async move {
                    match Daemon::new(config) {
                        Ok(daemon) => {
                            if let Err(e) = daemon.run().await {
                                tracing::error!("内嵌 daemon 错误: {e}");
                            }
                        }
                        Err(e) => tracing::error!("内嵌 daemon 启动失败: {e}"),
                    }
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
