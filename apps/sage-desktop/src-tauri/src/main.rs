// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod tray;

use std::sync::{Arc, Mutex};

use sage_core::config::Config;
use sage_core::onboarding::OnboardingState;
use sage_core::plugin::PluginRunner;
use sage_core::store::Store;
use sage_core::Daemon;
use tauri::{Emitter, Manager};
use tokio::task::AbortHandle;

/// Tauri 应用共享状态
pub struct AppState {
    pub store: Arc<Store>,
    pub onboarding: Mutex<Option<OnboardingState>>,
    pub daemon: Arc<Daemon>,
    /// Chat LLM 调用的取消句柄
    pub chat_abort: Mutex<Option<AbortHandle>>,
    pub plugin_runner: Arc<PluginRunner>,
}

fn main() {
    // 初始化 tracing：写入 ~/.sage/logs/sage.err.log，目录不可用时降级到 stderr
    let log_file = dirs::home_dir()
        .map(|h| h.join(".sage/logs/sage.err.log"))
        .expect("无法确定 home 目录");
    let writer: Box<dyn std::io::Write + Send + Sync> =
        match log_file.parent().ok_or_else(|| std::io::Error::other("no parent")).and_then(|p| {
            std::fs::create_dir_all(p)
        }).and_then(|_| {
            std::fs::OpenOptions::new().create(true).append(true).open(&log_file)
        }) {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::stderr()),
        };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sage_core=info,sage_desktop=info".parse().unwrap()),
        )
        .with_writer(std::sync::Mutex::new(writer))
        .with_ansi(false)
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

    let plugin_runner = Arc::new(PluginRunner::new(config.plugins.clone()));
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut daemon_init = Daemon::with_store(config, Arc::clone(&store)).expect("Daemon 创建失败");
    daemon_init.set_event_tx(event_tx);
    let daemon = Arc::new(daemon_init);

    let background = std::env::args().any(|a| a == "--background");
    let daemon_for_spawn = daemon.clone();

    let app_state = AppState {
        store,
        onboarding: Mutex::new(None),
        daemon,
        chat_abort: Mutex::new(None),
        plugin_runner,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        // 拦截 WebView 导航：外部 URL 用系统浏览器打开，不在 WebView 内跳转
        .plugin(tauri::plugin::Builder::<tauri::Wry, ()>::new("nav-guard")
            .on_navigation(|_webview, url| {
                let scheme = url.scheme();
                if scheme == "tauri" || scheme == "about" {
                    return true;
                }
                if (scheme == "http" || scheme == "https")
                    && matches!(url.host_str(), Some("localhost" | "tauri.localhost")) {
                    return true;
                }
                // 外部 URL → 系统浏览器
                tracing::info!("拦截外部导航: {url}");
                let url_string = url.to_string();
                std::thread::spawn(move || { let _ = open::that(&url_string); });
                false
            })
            .build())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::update_config_natural,
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
            commands::get_provider_errors,
            commands::chat,
            commands::cancel_chat,
            commands::list_chat_sessions,
            commands::delete_chat_session,
            commands::get_chat_history,
            commands::get_memories,
            commands::get_memories_by_ids,
            commands::get_all_memories,
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
            commands::get_evolution_progress,
            commands::trigger_reconcile,
            commands::list_custom_stages,
            commands::create_custom_stage,
            commands::delete_custom_stage,
            commands::toggle_custom_stage,
            commands::get_memory_graph,
            commands::trigger_memory_linking,
            commands::trigger_person_extract,
            commands::get_known_persons,
            commands::get_memories_about_person,
            commands::merge_persons,
            commands::get_all_tags,
            commands::get_memory_tags,
            commands::add_memory_tag,
            commands::remove_memory_tag,
            commands::get_memories_by_tag,
            commands::get_connections_status,
            commands::get_message_graph,
            commands::get_messages,
            commands::get_message_channels,
            commands::summarize_messages,
            commands::summarize_channel,
            commands::get_situation_summary,
            commands::chat_external,
            commands::curate_homepage,
            commands::get_dashboard_snapshot,
            commands::get_dashboard_stats,
            commands::save_report_correction,
            commands::get_report_corrections,
            commands::delete_report_correction,
            commands::get_reflective_signals,
            commands::resolve_reflective_signal,
            commands::create_task,
            commands::create_task_natural,
            commands::list_tasks,
            commands::update_task_status,
            commands::update_task_due_date,
            commands::update_task,
            commands::delete_task,
            commands::complete_task,
            commands::generate_tasks,
            commands::generate_verification,
            commands::get_task_signals,
            commands::dismiss_signal,
            commands::accept_signal,
            commands::get_feed_items,
            commands::search_feed_topic,
            commands::trigger_feed_poll,
            commands::get_feed_config,
            commands::save_feed_config,
            commands::update_feed_natural,
            commands::get_feed_digest,
            commands::regenerate_feed_digest,
            commands::archive_feed_item,
            commands::unarchive_feed_item,
            commands::bookmark_feed_item,
            commands::unbookmark_feed_item,
            commands::deep_learn_feed_item,
            commands::get_feed_note,
            commands::summarize_user_interests,
            commands::generate_page,
            commands::get_custom_page,
            commands::list_custom_pages,
            commands::update_custom_page,
            commands::delete_custom_page,
            commands::get_message_sources,
            commands::save_message_source,
            commands::delete_message_source,
            commands::test_source_connection,
            commands::fetch_emails,
            commands::get_cached_emails,
            commands::get_email_detail,
            commands::mark_email_read,
            commands::dismiss_email,
            commands::delete_message,
            commands::send_email,
            commands::summarize_email,
            commands::smart_reply,
            commands::start_oauth_flow,
            commands::ensure_oauth_token,
            commands::check_outlook_status,
            commands::fetch_outlook_emails,
        ])
        .setup(move |app| {
            tray::setup_tray(app)?;

            // 启动 daemon 事件循环
            tauri::async_runtime::spawn(async move {
                if let Err(e) = daemon_for_spawn.run().await {
                    tracing::error!("Daemon 事件循环错误: {e}");
                }
            });

            // 转发 daemon 数据变更事件到前端（接收端无限循环直到 sender 关闭）
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event_name) = event_rx.recv().await {
                    let _ = app_handle.emit(&event_name, ());
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
