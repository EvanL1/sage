// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod tray;

use std::sync::{Arc, Mutex};

use fs2::FileExt;
use sage_core::config::Config;
use sage_core::onboarding::OnboardingState;
use sage_core::store::Store;
use sage_core::Daemon;

/// Tauri 应用共享状态
pub struct AppState {
    pub store: Arc<Store>,
    pub onboarding: Mutex<Option<OnboardingState>>,
    pub daemon: Option<Arc<Daemon>>,
}

/// 尝试获取事件循环排他锁（PID 锁文件）
/// 成功返回 File handle（持有锁直到进程退出），失败返回 None
fn try_acquire_event_loop_lock() -> Option<std::fs::File> {
    let lock_path = dirs::home_dir()?.join(".sage/data/event-loop.pid");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .ok()?;
    match file.try_lock_exclusive() {
        Ok(()) => {
            // 写入 PID 方便调试
            use std::io::Write;
            let mut f = &file;
            let _ = f.write_all(format!("{}", std::process::id()).as_bytes());
            Some(file)
        }
        Err(_) => None, // 其他进程已持有锁
    }
}

fn main() {
    let data_dir = dirs::home_dir()
        .map(|h| h.join(".sage/data"))
        .expect("无法确定 home 目录");
    std::fs::create_dir_all(&data_dir).expect("创建数据目录失败");

    let db_path = data_dir.join("sage.db");
    let store = Arc::new(Store::open(&db_path).expect("打开数据库失败"));

    // 尝试获取事件循环锁 + 创建 Daemon
    let (daemon, _lock_file) = match try_acquire_event_loop_lock() {
        Some(lock_file) => {
            let config_path = dirs::home_dir()
                .map(|h| h.join(".sage/config.toml"))
                .expect("无法确定 home 目录");
            let config = Config::load_or_default(&config_path);
            match Daemon::with_store(config, Arc::clone(&store)) {
                Ok(d) => {
                    tracing::info!(
                        "内嵌 daemon 就绪（heartbeat: {}s）",
                        d.heartbeat_interval_secs()
                    );
                    (Some(Arc::new(d)), Some(lock_file))
                }
                Err(e) => {
                    tracing::error!("内嵌 daemon 创建失败: {e}");
                    (None, Some(lock_file))
                }
            }
        }
        None => {
            tracing::info!("其他进程已持有事件循环锁，跳过内嵌 daemon");
            (None, None)
        }
    };

    // spawn daemon 事件循环
    let daemon_for_spawn = daemon.clone();

    let app_state = AppState {
        store,
        onboarding: Mutex::new(None),
        daemon,
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
        .setup(move |app| {
            tray::setup_tray(app)?;

            // 启动内嵌 daemon 事件循环
            if let Some(d) = daemon_for_spawn {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = d.run().await {
                        tracing::error!("内嵌 daemon 错误: {e}");
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
