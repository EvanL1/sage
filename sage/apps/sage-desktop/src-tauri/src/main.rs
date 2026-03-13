// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod tray;

use std::sync::{Arc, Mutex};

use tauri::Manager;
use sage_core::config::Config;
use sage_core::onboarding::OnboardingState;
use sage_core::store::Store;
use sage_core::Daemon;

/// Tauri 应用共享状态
pub struct AppState {
    pub store: Arc<Store>,
    pub onboarding: Mutex<Option<OnboardingState>>,
    pub daemon: Arc<Daemon>,
}

fn main() {
    // 初始化 tracing：输出到 stderr（LaunchAgent 会重定向到 sage.err.log）
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sage_core=info,sage_desktop=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let data_dir = dirs::home_dir()
        .map(|h| h.join(".sage/data"))
        .expect("无法确定 home 目录");
    std::fs::create_dir_all(&data_dir).expect("创建数据目录失败");

    let db_path = data_dir.join("sage.db");
    let store = Arc::new(Store::open(&db_path).expect("打开数据库失败"));

    let config_path = dirs::home_dir()
        .map(|h| h.join(".sage/config.toml"))
        .expect("无法确定 home 目录");
    let config = Config::load_or_default(&config_path);

    let daemon = Arc::new(
        Daemon::with_store(config, Arc::clone(&store)).expect("Daemon 创建失败"),
    );

    let background = std::env::args().any(|a| a == "--background");
    let daemon_for_spawn = daemon.clone();

    let app_state = AppState {
        store,
        onboarding: Mutex::new(None),
        daemon,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::get_profile,
            commands::save_profile,
            commands::submit_onboarding_step,
            commands::get_suggestions,
            commands::delete_suggestion,
            commands::update_suggestion,
            commands::submit_feedback,
            commands::save_provider_priorities,
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
            commands::import_raw_memories,
            commands::save_assessment,
            commands::get_latest_reports,
            commands::add_user_memory,
            commands::get_daily_question,
            commands::trigger_report,
            commands::trigger_memory_evolution,
            commands::trigger_strategist,
            commands::get_memory_graph,
            commands::trigger_memory_linking,
            commands::get_all_tags,
            commands::get_memory_tags,
            commands::add_memory_tag,
            commands::remove_memory_tag,
            commands::get_memories_by_tag,
            commands::get_connections_status,
            commands::get_messages,
            commands::get_message_channels,
            commands::summarize_messages,
            commands::chat_external,
        ])
        .setup(move |app| {
            tray::setup_tray(app)?;

            // 启动 daemon 事件循环
            tauri::async_runtime::spawn(async move {
                if let Err(e) = daemon_for_spawn.run().await {
                    tracing::error!("Daemon 事件循环错误: {e}");
                }
            });

            // --background 模式下不显示窗口（LaunchAgent 开机自启用）
            if !background {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // macOS Dock 图标点击：窗口已隐藏时重新显示
            if let tauri::RunEvent::Reopen { has_visible_windows, .. } = event {
                if !has_visible_windows {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        });
}
