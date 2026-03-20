pub mod chat;
pub mod dashboard;
pub mod feed;
pub mod memory;
pub mod messages;
pub mod profile;
pub mod providers;
pub mod reports;
pub mod tasks;

// Re-export everything so main.rs invoke_handler paths don't change
pub use chat::*;
pub use dashboard::*;
pub use feed::*;
pub use memory::*;
pub use messages::*;
pub use profile::*;
pub use providers::*;
pub use reports::*;
pub use tasks::*;

// ─── Shared helpers (used across sub-modules) ──────────────────────────────

pub(crate) fn map_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn default_agent_config() -> sage_core::config::AgentConfig {
    sage_core::config::AgentConfig {
        provider: "claude".into(),
        claude_binary: "claude".into(),
        codex_binary: String::new(),
        gemini_binary: String::new(),
        default_model: "claude-sonnet-4-6".into(),
        project_dir: ".".into(),
        max_budget_usd: 1.0,
        permission_mode: "default".into(),
        max_iterations: 10,
    }
}

/// 获取 Claude Code 的记忆目录路径
/// ~/.claude/projects/-{project_path_encoded}/memory/
pub(crate) fn claude_memory_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let config_path = std::path::PathBuf::from(format!("{home}/.sage/config.toml"));
    let config = sage_core::config::Config::load_or_default(&config_path);
    let project_dir = config.agent.project_dir;
    let expanded = if project_dir.starts_with('~') {
        project_dir.replacen('~', &home, 1)
    } else {
        project_dir
    };
    let encoded = expanded.replace('/', "-");
    let dir = std::path::PathBuf::from(format!("{home}/.claude/projects/{encoded}/memory"));
    if dir.exists() {
        Some(dir)
    } else {
        None
    }
}

/// 触发 Sage → Claude Code 记忆同步（静默失败，不影响主流程）
pub(crate) fn trigger_memory_sync(store: &sage_core::store::Store) {
    if let Some(dir) = claude_memory_dir() {
        if let Err(e) = store.sync_to_claude_memory(&dir) {
            tracing::warn!("Memory sync to Claude Code failed: {e}");
        }
    }
}

/// 从 LLM 响应中提取 ```sage-memory JSON 块，写入 store，返回清理后的显示文本和保存数量
///
/// TODO: Route through `MemoryIntegrator` so chat-extracted memories go through LLM arbitration.
pub(crate) async fn extract_and_save_memories(
    raw: &str,
    store: &std::sync::Arc<sage_core::store::Store>,
) -> (String, usize) {
    let marker_start = "```sage-memory";
    let marker_end = "```";

    let Some(start_idx) = raw.find(marker_start) else {
        return (raw.to_string(), 0);
    };
    let json_start = start_idx + marker_start.len();
    let Some(end_offset) = raw[json_start..].find(marker_end) else {
        return (raw.to_string(), 0);
    };
    let json_str = raw[json_start..json_start + end_offset].trim();
    let block_end = json_start + end_offset + marker_end.len();

    let items: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (raw.to_string(), 0),
    };

    let mut entries = Vec::new();
    let mut tags_map: Vec<(String, Vec<String>)> = Vec::new();
    for item in &items {
        let mem_type = item["type"].as_str().unwrap_or("");
        let content = item["content"].as_str().unwrap_or("");
        if content.is_empty() {
            continue;
        }
        let about_person = item["about"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from);

        if mem_type == "decision" {
            let _ = store.append_decision("chat", content);
            continue;
        }

        let (category, confidence) = match mem_type {
            "task" | "reminder" => ("task", 1.0),
            "insight" => ("behavior", 0.8),
            _ => ("behavior", 0.8),
        };
        let final_content = if mem_type == "reminder" {
            format!("[提醒] {content}")
        } else {
            content.to_string()
        };

        let tag_list: Vec<String> = if let Some(tags) = item["tags"].as_array() {
            tags.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        } else {
            vec![mem_type.to_string()]
        };
        tags_map.push((final_content.clone(), tag_list));

        entries.push(sage_core::memory_integrator::IncomingMemory {
            content: final_content,
            category: category.to_string(),
            source: "chat".to_string(),
            confidence,
            about_person,
        });
    }

    let saved;
    let integrator =
        sage_core::memory_integrator::MemoryIntegrator::new(std::sync::Arc::clone(store));
    let discovered = sage_core::discovery::discover_providers(store);
    let configs = store.load_provider_configs().unwrap_or_default();
    if let Some((info, config)) = sage_core::discovery::select_best_provider(&discovered, &configs)
    {
        let agent_config = default_agent_config();
        let provider =
            sage_core::provider::create_provider_from_config(&info, &config, &agent_config);
        match integrator.integrate(entries, provider.as_ref()).await {
            Ok(result) => {
                saved = result.created + result.updated;
            }
            Err(e) => {
                tracing::warn!("Memory integration failed, skipping: {e}");
                saved = 0;
            }
        }
    } else {
        saved = entries.len();
        for entry in &entries {
            if let Some(ref person) = entry.about_person {
                let _ = store.save_memory_about_person(
                    &entry.category,
                    &entry.content,
                    "chat",
                    entry.confidence,
                    "private",
                    person,
                );
            } else {
                let _ = store.save_memory_with_visibility(
                    &entry.category,
                    &entry.content,
                    "chat",
                    entry.confidence,
                    "private",
                );
            }
        }
    }

    let display = format!("{}{}", raw[..start_idx].trim_end(), &raw[block_end..]);
    (display.trim().to_string(), saved)
}

/// 测试通知（开发用，验证通知 + 跳转是否工作）
#[tauri::command]
pub async fn test_notification(route: String) -> Result<(), String> {
    sage_core::applescript::notify("Sage 测试", &format!("点击跳转到 {route}"), &route)
        .await
        .map_err(|e| e.to_string())
}

/// 从用户消息中提取关键词，自动匹配并回答 open_questions
pub(crate) fn auto_answer_open_questions(message: &str, store: &sage_core::store::Store) {
    let keywords: Vec<&str> = message
        .split(|c: char| c.is_whitespace() || c == '，' || c == '。' || c == '？' || c == '！')
        .filter(|w| w.chars().count() >= 2)
        .take(5)
        .collect();

    for kw in keywords {
        if let Ok(matches) = store.search_open_questions(kw) {
            for (qid, _text) in matches {
                if let Err(e) = store.answer_question(qid) {
                    tracing::warn!("Failed to auto-answer question {qid}: {e}");
                }
            }
        }
    }
}
