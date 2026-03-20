use sage_core::plugin::{PluginEvent, TaskSnapshot};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

use super::{default_agent_config, map_err};
use crate::AppState;

/// Look up a task by id and build a TaskSnapshot. Returns None if not found.
fn load_snapshot(store: &sage_core::store::Store, task_id: i64) -> Option<TaskSnapshot> {
    let row = store.get_task(task_id).ok()??;
    Some(TaskSnapshot {
        id: task_id,
        content: row.0,
        status: row.1,
        priority: row.2,
        due_date: row.3,
        description: row.4,
        outcome: row.5,
    })
}

/// Fire-and-forget plugin dispatch (spawns a task so commands stay fast).
fn dispatch_plugin(runner: Arc<sage_core::plugin::PluginRunner>, event: PluginEvent) {
    tokio::spawn(async move {
        runner.dispatch(event).await;
    });
}

#[tauri::command]
pub async fn create_task(
    state: State<'_, AppState>,
    content: String,
    source: Option<String>,
    source_id: Option<i64>,
    priority: Option<String>,
    due_date: Option<String>,
    description: Option<String>,
) -> Result<i64, String> {
    tracing::info!(
        "create_task called: content={:?}, source={:?}, due={:?}",
        content,
        source,
        due_date
    );
    let id = state
        .store
        .create_task(
            &content,
            &source.unwrap_or("manual".into()),
            source_id,
            priority.as_deref(),
            due_date.as_deref(),
            description.as_deref(),
        )
        .map_err(|e| {
            tracing::error!("create_task failed: {e}");
            e.to_string()
        })?;

    if let Some(snapshot) = load_snapshot(&state.store, id) {
        dispatch_plugin(
            Arc::clone(&state.plugin_runner),
            PluginEvent { event_type: "task.created".into(), task: snapshot, changes: None },
        );
    }

    Ok(id)
}

#[tauri::command]
pub async fn list_tasks(
    state: State<'_, AppState>,
    status: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let tasks = state
        .store
        .list_tasks(status.as_deref(), limit.unwrap_or(50))
        .map_err(|e| e.to_string())?;
    Ok(tasks
        .into_iter()
        .map(
            |(
                id,
                content,
                st,
                priority,
                due,
                src,
                created,
                updated,
                outcome,
                verification,
                description,
            )| {
                json!({
                    "id": id, "content": content, "status": st, "priority": priority,
                    "due_date": due, "source": src, "created_at": created, "updated_at": updated,
                    "outcome": outcome, "verification": verification, "description": description
                })
            },
        )
        .collect())
}

#[tauri::command]
pub async fn update_task_status(
    state: State<'_, AppState>,
    task_id: i64,
    status: String,
) -> Result<(), String> {
    tracing::info!(
        "update_task_status called: id={}, status={}",
        task_id,
        status
    );
    state
        .store
        .update_task_status(task_id, &status)
        .map_err(|e| {
            tracing::error!("update_task_status failed: {e}");
            e.to_string()
        })?;

    if let Some(snapshot) = load_snapshot(&state.store, task_id) {
        dispatch_plugin(
            Arc::clone(&state.plugin_runner),
            PluginEvent {
                event_type: "task.updated".into(),
                task: snapshot,
                changes: Some(json!({ "status": status })),
            },
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn update_task_due_date(
    state: State<'_, AppState>,
    task_id: i64,
    due_date: Option<String>,
) -> Result<(), String> {
    state
        .store
        .update_task_due_date(task_id, due_date.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_task(
    state: State<'_, AppState>,
    task_id: i64,
    content: String,
    priority: Option<String>,
    due_date: Option<String>,
    description: Option<String>,
) -> Result<(), String> {
    state
        .store
        .update_task(
            task_id,
            &content,
            priority.as_deref(),
            due_date.as_deref(),
            description.as_deref(),
        )
        .map_err(|e| e.to_string())?;

    if let Some(snapshot) = load_snapshot(&state.store, task_id) {
        dispatch_plugin(
            Arc::clone(&state.plugin_runner),
            PluginEvent {
                event_type: "task.updated".into(),
                task: snapshot,
                changes: Some(json!({
                    "content": content,
                    "priority": priority,
                    "due_date": due_date,
                    "description": description,
                })),
            },
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_task(state: State<'_, AppState>, task_id: i64) -> Result<(), String> {
    state.store.delete_task(task_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn complete_task(
    state: State<'_, AppState>,
    task_id: i64,
    status: String,
    outcome: Option<String>,
) -> Result<(), String> {
    tracing::info!(
        "complete_task called: id={}, status={}, outcome={:?}",
        task_id,
        status,
        outcome
    );
    state
        .store
        .update_task_with_outcome(task_id, &status, outcome.as_deref())
        .map_err(|e| {
            tracing::error!("complete_task failed: {e}");
            e.to_string()
        })?;

    if let Some(snapshot) = load_snapshot(&state.store, task_id) {
        dispatch_plugin(
            Arc::clone(&state.plugin_runner),
            PluginEvent {
                event_type: "task.updated".into(),
                task: snapshot,
                changes: Some(json!({ "status": status, "outcome": outcome })),
            },
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn generate_tasks(
    state: State<'_, AppState>,
    report_type: Option<String>,
) -> Result<Vec<Value>, String> {
    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let mut context = format!("当前时间：{}\n\n", now.format("%Y-%m-%d %A %H:%M"));

    let types_to_read: Vec<&str> = match &report_type {
        Some(rt) => vec![rt.as_str()],
        None => vec!["morning", "evening", "weekly", "week_start"],
    };
    for rtype in types_to_read {
        if let Ok(Some(r)) = state.store.get_latest_report(rtype) {
            context.push_str(&format!("## {} Report\n{}\n\n", rtype, r.content));
        }
    }
    if let Ok(suggestions) = state.store.get_recent_suggestions(8) {
        if !suggestions.is_empty() {
            context.push_str("## 待处理建议\n");
            for s in &suggestions {
                context.push_str(&format!("- {}\n", s.response));
            }
            context.push('\n');
        }
    }
    let memories = state.store.load_memories().unwrap_or_default();
    if !memories.is_empty() {
        context.push_str("## 近期记忆\n");
        for m in memories.iter().take(8) {
            context.push_str(&format!("- [{}] {}\n", m.category, m.content));
        }
        context.push('\n');
    }
    if let Ok(existing) = state.store.list_tasks(Some("open"), 20) {
        if !existing.is_empty() {
            context.push_str("## 已有待办（不要重复）\n");
            for (_, content, _, _, _, _, _, _, _, _, _) in &existing {
                context.push_str(&format!("- {}\n", content));
            }
            context.push('\n');
        }
    }

    let lang = state.store.prompt_lang();
    let system = sage_core::prompts::cmd_task_extraction_system(&lang, &today);

    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("未配置 AI 服务")?;
    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);
    let raw = provider
        .invoke(&context, Some(&system))
        .await
        .map_err(map_err)?;

    let json_str = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let tasks: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_default();

    let mut created = Vec::new();
    for t in &tasks {
        let content = t["content"].as_str().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let priority = t["priority"].as_str().unwrap_or("P1");
        let due = t["due_date"].as_str();
        if let Ok(id) = state
            .store
            .create_task(content, "ai", None, Some(priority), due, None)
        {
            if let Some(veri) = t.get("verification") {
                if !veri.is_null() {
                    if let Ok(veri_str) = serde_json::to_string(veri) {
                        let _ = state.store.update_task_verification(id, &veri_str);
                    }
                }
            }
            created.push(
                json!({ "id": id, "content": content, "priority": priority, "due_date": due }),
            );
        }
    }

    Ok(created)
}

#[tauri::command]
pub async fn generate_verification(state: State<'_, AppState>, task_id: i64) -> Result<(), String> {
    let tasks = state
        .store
        .list_tasks(None, 500)
        .map_err(|e| e.to_string())?;
    let task_content = tasks
        .iter()
        .find(|(id, _, _, _, _, _, _, _, _, _, _)| *id == task_id)
        .map(|(_, content, _, _, _, _, _, _, _, _, _)| content.clone())
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = match sage_core::discovery::select_best_provider(&discovered, &configs) {
        Some(p) => p,
        None => return Ok(()),
    };
    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let lang = state.store.prompt_lang();
    let system = sage_core::prompts::cmd_verification_system(&lang);
    let prompt = sage_core::prompts::cmd_verification_user(&lang, &task_content);

    match provider.invoke(&prompt, Some(system)).await {
        Ok(raw) => {
            let json_str = raw
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            if let Ok(items) = serde_json::from_str::<serde_json::Value>(json_str) {
                // 新格式 {"done":[...],"cancelled":[...]} 或旧格式 [...]
                if items.is_object() || items.is_array() {
                    if let Ok(veri_str) = serde_json::to_string(&items) {
                        let _ = state.store.update_task_verification(task_id, &veri_str);
                    }
                }
            }
            Ok(())
        }
        Err(_) => Ok(()),
    }
}

// ─── Task Signals ──────────────────────────────

#[tauri::command]
pub async fn get_task_signals(state: State<'_, AppState>) -> Result<Value, String> {
    let signals = state
        .store
        .get_pending_signals()
        .map_err(|e| e.to_string())?;
    let items: Vec<Value> = signals
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "signalType": s.signal_type,
                "taskId": s.task_id,
                "title": s.title,
                "evidence": s.evidence,
                "suggestedOutcome": s.suggested_outcome,
                "status": s.status,
                "createdAt": s.created_at,
                "importance": s.importance,
            })
        })
        .collect();
    Ok(json!(items))
}

#[tauri::command]
pub async fn dismiss_signal(state: State<'_, AppState>, signal_id: i64) -> Result<(), String> {
    state
        .store
        .update_signal_status(signal_id, "dismissed")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn accept_signal(state: State<'_, AppState>, signal_id: i64) -> Result<(), String> {
    state
        .store
        .update_signal_status(signal_id, "accepted")
        .map_err(|e| e.to_string())
}
