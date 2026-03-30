use sage_types::Report;
use serde_json::{json, Value};
use tauri::State;

use super::{map_err, trigger_memory_sync};
use crate::AppState;

#[tauri::command]
pub async fn get_latest_reports(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Report>, String> {
    let types = ["morning", "evening", "weekly", "week_start"];
    let mut map = std::collections::HashMap::new();
    for t in types {
        if let Ok(Some(r)) = state.store.get_latest_report(t) {
            map.insert(t.to_string(), r);
        }
    }
    Ok(map)
}

/// 手动触发报告生成
#[tauri::command]
pub async fn trigger_report(
    state: State<'_, AppState>,
    report_type: String,
) -> Result<String, String> {
    let valid_types = ["morning", "evening", "weekly", "week_start"];
    if !valid_types.contains(&report_type.as_str()) {
        return Err(format!(
            "未知报告类型: {report_type}，支持: morning/evening/weekly/week_start"
        ));
    }
    let daemon = state.daemon.clone();
    let rt = report_type.clone();
    // 同步等待报告生成，直接把结果/错误返回前端
    match daemon.trigger_report(&rt).await {
        Ok(content) => {
            tracing::info!("报告生成成功: {rt}");
            Ok(content)
        }
        Err(e) => {
            tracing::error!("报告生成失败 ({rt}): {e}");
            Err(format!("Report failed: {e}"))
        }
    }
}

/// 手动触发记忆进化
#[tauri::command]
pub async fn trigger_memory_evolution(state: State<'_, AppState>) -> Result<String, String> {
    let daemon = state.daemon.clone();
    let store = state.store.clone();
    tauri::async_runtime::spawn(async move {
        match daemon.trigger_memory_evolution().await {
            Ok(r) => {
                let msg = r.summary.clone();
                tracing::info!("Memory evolution done: {msg}");
                let _ = sage_core::applescript::notify("Memory Evolution", &msg, "/about").await;
                trigger_memory_sync(&store);
            }
            Err(e) => {
                tracing::error!("Memory evolution failed: {e}");
                let _ =
                    sage_core::applescript::notify("Memory Evolution", &format!("失败: {e}"), "/about").await;
            }
        }
    });
    Ok("Evolution 已在后台启动…".into())
}

/// 查询 evolution 实时进度（前端轮询用）
#[tauri::command]
pub async fn get_evolution_progress(state: State<'_, AppState>) -> Result<String, String> {
    state.store.kv_get("evolution_progress")
        .map_err(map_err)
        .map(|v| v.unwrap_or_default())
}

#[tauri::command]
pub async fn trigger_reconcile(state: State<'_, AppState>) -> Result<Value, String> {
    let revised = state.daemon.run_reconcile_full().await.map_err(map_err)?;
    trigger_memory_sync(&state.store);
    Ok(json!({ "revised": revised }))
}

/// 手动触发战略家分析
#[tauri::command]
pub async fn trigger_strategist(state: State<'_, AppState>) -> Result<Value, String> {
    let ran = state.daemon.trigger_strategist().await.map_err(map_err)?;
    Ok(json!({ "ran": ran }))
}

/// 获取自定义管线阶段列表
#[tauri::command]
pub async fn list_custom_stages(state: State<'_, AppState>) -> Result<Value, String> {
    let stages = state.store.list_custom_stages().map_err(map_err)?;
    let items: Vec<Value> = stages
        .iter()
        .map(|s| {
            json!({
                "id": s.id, "name": s.name, "description": s.description,
                "prompt": s.prompt, "insert_after": s.insert_after, "enabled": s.enabled,
                "output_format": s.output_format, "available_actions": s.available_actions,
                "allowed_inputs": s.allowed_inputs, "max_actions": s.max_actions, "pre_condition": s.pre_condition,
            })
        })
        .collect();
    Ok(json!(items))
}

/// 创建自定义管线阶段
#[tauri::command]
pub async fn create_custom_stage(
    state: State<'_, AppState>,
    name: String,
    description: String,
    prompt: String,
    insert_after: String,
    output_format: Option<String>,
    available_actions: Option<String>,
    allowed_inputs: Option<String>,
    max_actions: Option<i32>,
    pre_condition: Option<String>,
) -> Result<i64, String> {
    state
        .store
        .create_custom_stage(
            &name, &description, &prompt, &insert_after,
            &output_format.unwrap_or_default(),
            &available_actions.unwrap_or_default(),
            &allowed_inputs.unwrap_or_else(|| "observer_notes,coach_insights".into()),
            max_actions.unwrap_or(5),
            &pre_condition.unwrap_or_default(),
        )
        .map_err(map_err)
}

/// 删除自定义管线阶段
#[tauri::command]
pub async fn delete_custom_stage(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    state.store.delete_custom_stage(id).map_err(map_err)
}

/// 切换自定义阶段开关
#[tauri::command]
pub async fn toggle_custom_stage(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    state.store.toggle_custom_stage(id, enabled).map_err(map_err)
}

#[tauri::command]
pub async fn save_report_correction(
    state: State<'_, AppState>,
    report_type: String,
    wrong_claim: String,
    correct_fact: String,
    context_hint: String,
) -> Result<i64, String> {
    state
        .store
        .save_correction(&report_type, &wrong_claim, &correct_fact, &context_hint)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_report_corrections(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let corrections = state
        .store
        .get_all_corrections()
        .map_err(|e| e.to_string())?;
    Ok(corrections
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "report_type": c.report_type,
                "wrong_claim": c.wrong_claim,
                "correct_fact": c.correct_fact,
                "context_hint": c.context_hint,
                "confidence": c.confidence,
                "applied_count": c.applied_count,
                "created_at": c.created_at,
            })
        })
        .collect())
}

#[tauri::command]
pub async fn delete_report_correction(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.store.delete_correction(id).map_err(|e| e.to_string())
}

/// 获取未解决的反思信号 / Get unresolved reflective signals
#[tauri::command]
pub async fn get_reflective_signals(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let signals = state
        .store
        .get_unresolved_signals(limit.unwrap_or(50))
        .map_err(|e| e.to_string())?;
    Ok(signals
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "timestamp": s.timestamp,
                "source": s.source,
                "signal_type": s.signal_type,
                "raw_text": s.raw_text,
                "context": s.context,
                "baseline_divergence": s.baseline_divergence,
                "armor_pattern": s.armor_pattern,
                "intensity": s.intensity,
                "resolved": s.resolved,
                "resolution_text": s.resolution_text,
                "created_at": s.created_at,
            })
        })
        .collect())
}

/// 标记反思信号为已解决 / Resolve a reflective signal
#[tauri::command]
pub async fn resolve_reflective_signal(
    state: State<'_, AppState>,
    id: i64,
    resolution_text: String,
) -> Result<(), String> {
    state
        .store
        .resolve_signal(id, &resolution_text)
        .map_err(|e| e.to_string())
}
