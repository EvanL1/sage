use serde_json::{json, Value};
use tauri::State;

use super::{claude_memory_dir, format_existing_memories, get_provider, map_err, trigger_memory_sync};
use crate::AppState;

#[tauri::command]
pub async fn get_memories(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let memories = state.store.load_memories().map_err(map_err)?;
    memories
        .into_iter()
        .map(|m| serde_json::to_value(&m).map_err(map_err))
        .collect()
}

#[tauri::command]
pub async fn get_all_memories(state: State<'_, AppState>, limit: Option<usize>) -> Result<Vec<Value>, String> {
    let mut memories = state.store.load_active_memories().map_err(map_err)?;
    if let Some(n) = limit {
        memories.truncate(n);
    }
    memories
        .into_iter()
        .map(|m| serde_json::to_value(&m).map_err(map_err))
        .collect()
}

#[tauri::command]
pub async fn extract_memories(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<Value>, String> {
    let messages = state
        .store
        .load_session_messages(&session_id)
        .map_err(map_err)?;
    if messages.len() < 2 {
        return Ok(vec![]);
    }

    let existing_text = format_existing_memories(&state.store);

    let conversation = messages
        .iter()
        .map(|m| {
            let role = if m.role == "user" { "用户" } else { "Sage" };
            format!("{}: {}", role, m.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let provider = get_provider(&state.store)?;

    let lang = state.store.prompt_lang();
    let extraction_prompt =
        sage_core::prompts::cmd_extract_memories_user(&lang, &existing_text, &conversation);
    let system = sage_core::prompts::cmd_extract_memories_system(&lang);

    let result = provider
        .invoke(&extraction_prompt, Some(system))
        .await
        .map_err(map_err)?;

    let json_str = result
        .find('[')
        .and_then(|start| result.rfind(']').map(|end| &result[start..=end]))
        .unwrap_or("[]");

    let insights: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_default();

    let mut saved = Vec::new();
    for insight in &insights {
        if let (Some(category), Some(content), Some(confidence)) = (
            insight.get("category").and_then(|v| v.as_str()),
            insight.get("content").and_then(|v| v.as_str()),
            insight.get("confidence").and_then(|v| v.as_f64()),
        ) {
            let action_line = format!(
                "save_memory_visible | {category} | {content} | confidence:{confidence:.1} | visibility:private"
            );
            let id = sage_core::pipeline::actions::execute_single_action(
                &action_line, &["save_memory_visible"], &state.store, "tauri_memory",
            )
            .ok_or_else(|| format!("记忆保存失败（ACTION 约束层拒绝）: {category}"))?;
            // LLM 指定的 depth 覆盖 infer_depth
            if let Some(depth) = insight.get("depth").and_then(|v| v.as_str()) {
                let valid = ["episodic", "semantic", "procedural", "axiom"];
                if valid.contains(&depth) {
                    let _ = state.store.update_memory_depth(id, depth);
                }
            }
            saved.push(json!({
                "id": id,
                "category": category,
                "content": content,
                "confidence": confidence,
            }));
        }
    }

    if !saved.is_empty() {
        trigger_memory_sync(&state.store);
    }
    Ok(saved)
}

#[tauri::command]
pub async fn sync_memory(state: State<'_, AppState>) -> Result<String, String> {
    let dir = claude_memory_dir().ok_or("Claude Code memory directory not found")?;
    state.store.sync_to_claude_memory(&dir).map_err(map_err)?;
    Ok(format!("Synced to {}", dir.display()))
}

#[tauri::command]
pub async fn delete_memory(state: State<'_, AppState>, memory_id: i64) -> Result<(), String> {
    state.store.delete_memory(memory_id).map_err(map_err)?;
    trigger_memory_sync(&state.store);
    Ok(())
}

#[tauri::command]
pub async fn save_assessment(
    state: State<'_, AppState>,
    dimensions: Vec<Value>,
) -> Result<(), String> {
    for dim in &dimensions {
        if let (Some(content), Some(confidence)) = (
            dim.get("content").and_then(|v| v.as_str()),
            dim.get("confidence").and_then(|v| v.as_f64()),
        ) {
            state
                .store
                .save_memory_with_visibility(
                    "personality",
                    content,
                    "assessment",
                    confidence,
                    "public",
                )
                .map_err(map_err)?;
        }
    }
    trigger_memory_sync(&state.store);
    Ok(())
}

#[tauri::command]
pub async fn export_memories(state: State<'_, AppState>) -> Result<String, String> {
    let memories = state.store.load_memories().map_err(map_err)?;
    let profile = state.store.load_profile().map_err(map_err)?;

    let mut md = String::from("# Sage Memory Export\n\n");

    if let Some(p) = profile {
        md.push_str("## Profile\n\n");
        if !p.identity.name.is_empty() {
            md.push_str(&format!("**Name**: {}\n\n", p.identity.name));
        }
        if !p.identity.role.is_empty() {
            md.push_str(&format!("**Role**: {}\n\n", p.identity.role));
        }
    }

    let category_labels: &[(&str, &str)] = &[
        ("identity", "身份认同"),
        ("personality", "人格特质"),
        ("values", "价值观"),
        ("behavior", "行为模式"),
        ("thinking", "思维方式"),
        ("emotion", "情绪线索"),
        ("growth", "成长方向"),
    ];

    for (cat, label) in category_labels {
        let items: Vec<_> = memories.iter().filter(|m| m.category == *cat).collect();
        if items.is_empty() {
            continue;
        }
        md.push_str(&format!("## {label}\n\n"));
        for m in items {
            let conf = format!("{:.0}%", m.confidence * 100.0);
            md.push_str(&format!("- {} (confidence: {})\n", m.content, conf));
        }
        md.push('\n');
    }

    let known: Vec<&str> = category_labels.iter().map(|(c, _)| *c).collect();
    let other: Vec<_> = memories
        .iter()
        .filter(|m| !known.contains(&m.category.as_str()))
        .collect();
    if !other.is_empty() {
        md.push_str("## Other\n\n");
        for m in other {
            md.push_str(&format!(
                "- [{}] {} (confidence: {:.0}%)\n",
                m.category,
                m.content,
                m.confidence * 100.0
            ));
        }
        md.push('\n');
    }

    md.push_str("---\n*Exported from Sage*\n");

    use std::io::Write;
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("pbcopy failed: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(md.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;
    }
    child
        .wait()
        .map_err(|e| format!("pbcopy wait failed: {e}"))?;

    Ok(md)
}

#[tauri::command]
pub async fn import_memories(
    state: State<'_, AppState>,
    entries: Vec<Value>,
) -> Result<usize, String> {
    let mut count = 0;
    for entry in &entries {
        if let (Some(category), Some(content)) = (
            entry.get("category").and_then(|v| v.as_str()),
            entry.get("content").and_then(|v| v.as_str()),
        ) {
            let confidence = entry
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.8);
            let source = entry
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("import");
            state
                .store
                .save_memory_with_visibility(category, content, source, confidence, "public")
                .map_err(map_err)?;
            count += 1;
        }
    }
    if count > 0 {
        trigger_memory_sync(&state.store);
    }
    Ok(count)
}

/// 用户主动告诉 Sage 想被记住的内容
#[tauri::command]
pub async fn add_user_memory(state: State<'_, AppState>, content: String) -> Result<i64, String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("内容不能为空".to_string());
    }
    let id = state
        .store
        .save_memory_with_visibility("user_input", &content, "user", 1.0, "private")
        .map_err(map_err)?;
    trigger_memory_sync(&state.store);
    Ok(id)
}

/// 解析用户从其他 AI 助手粘贴的原始文本，通过 LLM 结构化后保存为记忆
#[tauri::command]
pub async fn import_raw_memories(
    state: State<'_, AppState>,
    text: String,
) -> Result<usize, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("内容不能为空".to_string());
    }

    let provider = get_provider(&state.store)?;

    let lang = state.store.prompt_lang();
    let prompt = sage_core::prompts::cmd_import_ai_memory_user(&lang, text);

    let raw = provider.invoke(&prompt, None).await.map_err(map_err)?;

    let mut count = 0;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
            if let (Some(category), Some(content)) = (
                obj.get("category").and_then(|v| v.as_str()),
                obj.get("content").and_then(|v| v.as_str()),
            ) {
                if !content.is_empty() {
                    let action_line = format!(
                        "save_memory_visible | {category} | {content} | confidence:0.7 | visibility:public"
                    );
                    if let Some(id) = sage_core::pipeline::actions::execute_single_action(
                        &action_line, &["save_memory_visible"], &state.store, "tauri_memory",
                    ) {
                        if let Some(depth) = obj.get("depth").and_then(|v| v.as_str()) {
                            let valid = ["episodic", "semantic", "procedural", "axiom"];
                            if valid.contains(&depth) {
                                let _ = state.store.update_memory_depth(id, depth);
                            }
                        }
                        count += 1;
                    }
                }
            }
        }
    }

    if count > 0 {
        trigger_memory_sync(&state.store);
    }
    Ok(count)
}

// ─── Tag 命令 ──────────────────────────────────

/// 获取所有标签及其记忆数量
#[tauri::command]
pub async fn get_all_tags(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let tags = state.store.get_all_tags().map_err(map_err)?;
    Ok(tags
        .iter()
        .map(|(tag, count)| json!({ "tag": tag, "count": count }))
        .collect())
}

/// 获取某条记忆的标签
#[tauri::command]
pub async fn get_memory_tags(
    state: State<'_, AppState>,
    memory_id: i64,
) -> Result<Vec<String>, String> {
    state.store.get_tags(memory_id).map_err(map_err)
}

/// 给记忆添加标签
#[tauri::command]
pub async fn add_memory_tag(
    state: State<'_, AppState>,
    memory_id: i64,
    tag: String,
) -> Result<(), String> {
    state.store.add_tag(memory_id, &tag).map_err(map_err)
}

/// 删除记忆的某个标签
#[tauri::command]
pub async fn remove_memory_tag(
    state: State<'_, AppState>,
    memory_id: i64,
    tag: String,
) -> Result<(), String> {
    state.store.remove_tag(memory_id, &tag).map_err(map_err)
}

/// 按标签筛选记忆 ID
#[tauri::command]
pub async fn get_memories_by_tag(
    state: State<'_, AppState>,
    tag: String,
) -> Result<Vec<i64>, String> {
    state.store.get_memories_by_tag(&tag).map_err(map_err)
}

/// 获取最近一条苏格拉底式每日问题
#[tauri::command]
pub async fn get_daily_question(state: State<'_, AppState>) -> Result<Option<Value>, String> {
    match state.store.get_daily_question().map_err(map_err)? {
        Some(s) => Ok(Some(serde_json::to_value(&s).map_err(map_err)?)),
        None => Ok(None),
    }
}

/// 获取记忆图谱数据（全部活跃记忆）
#[tauri::command]
pub async fn get_memory_graph(state: State<'_, AppState>) -> Result<Value, String> {
    let memories = state.store.load_memories().map_err(map_err)?;
    let edges = state.store.get_all_memory_edges().map_err(map_err)?;

    let node_ids: std::collections::HashSet<i64> = memories.iter().map(|m| m.id).collect();

    let nodes: Vec<Value> = memories
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "category": m.category,
                "content": m.content,
                "confidence": m.confidence,
                "depth": m.depth,
            })
        })
        .collect();

    let edge_list: Vec<Value> = edges
        .iter()
        .filter(|e| node_ids.contains(&e.from_id) && node_ids.contains(&e.to_id))
        .map(|e| {
            json!({
                "id": e.id,
                "from": e.from_id,
                "to": e.to_id,
                "relation": e.relation,
                "weight": e.weight,
            })
        })
        .collect();

    Ok(json!({ "nodes": nodes, "edges": edge_list }))
}

// ─── 人物认知 ─────────────────────────────────

/// 手动触发人物认知提取
#[tauri::command]
pub async fn trigger_person_extract(state: State<'_, AppState>) -> Result<String, String> {
    let daemon = state.daemon.clone();
    tauri::async_runtime::spawn(async move {
        match daemon.trigger_person_observer().await {
            Ok(true) => {
                let _ = sage_core::applescript::notify(
                    "Person Observer",
                    "已提取人物认知",
                    "/people",
                )
                .await;
            }
            Ok(false) => {
                let _ = sage_core::applescript::notify(
                    "Person Observer",
                    "今日暂无新的人物信息",
                    "/people",
                )
                .await;
            }
            Err(e) => tracing::error!("PersonObserver failed: {e}"),
        }
    });
    Ok("人物提取已在后台启动…".into())
}

/// 获取所有已知人名
#[tauri::command]
pub async fn get_known_persons(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.store.get_known_persons().map_err(map_err)
}

/// 获取关于某人的所有记忆
#[tauri::command]
pub async fn get_memories_about_person(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<Value>, String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.len() > 200 {
        return Err("人名无效".to_string());
    }
    let memories = state.store.get_memories_about_person(&name).map_err(map_err)?;
    memories
        .into_iter()
        .map(|m| serde_json::to_value(&m).map_err(map_err))
        .collect()
}

/// 单独触发记忆图谱连接
#[tauri::command]
pub async fn trigger_memory_linking(state: State<'_, AppState>) -> Result<Value, String> {
    let linked = state
        .daemon
        .trigger_memory_linking()
        .await
        .map_err(map_err)?;
    Ok(json!({ "linked": linked }))
}
