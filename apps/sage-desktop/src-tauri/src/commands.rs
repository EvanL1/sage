pub mod chat;
pub mod dashboard;
pub mod feed;
pub mod memory;
pub mod messages;
pub mod pages;
pub mod profile;
pub mod providers;
pub mod reports;
pub mod email;
pub mod tasks;

// Re-export everything so main.rs invoke_handler paths don't change
pub use chat::*;
pub use dashboard::*;
pub use email::*;
pub use feed::*;
pub use memory::*;
pub use messages::*;
pub use pages::*;
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

// ─── 自然语言配置更新 ─────────────────────────────────────────────────────

/// 自然语言修改 config.toml（脱敏后交给 LLM 理解意图，更新非敏感字段）
#[tauri::command]
pub async fn update_config_natural(
    state: tauri::State<'_, crate::AppState>,
    text: String,
) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("内容不能为空".into());
    }

    let config_path = dirs::home_dir()
        .map(|h| h.join(".sage/config.toml"))
        .ok_or("无法确定 home 目录")?;
    let raw_toml = std::fs::read_to_string(&config_path).unwrap_or_default();

    // 脱敏：移除 API key 相关行
    let sanitized: String = raw_toml
        .lines()
        .map(|line| {
            let lower = line.to_lowercase();
            if lower.contains("api_key") || lower.contains("apikey") || lower.contains("secret") {
                format!("# [REDACTED] {}", line.split('=').next().unwrap_or("").trim())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"你是配置助手。用户用自然语言描述了要修改的配置。

当前配置（TOML，API key 已脱敏）：
```toml
{sanitized}
```

用户说："{text}"

请理解用户意图，输出修改后的**完整** TOML 配置。规则：
1. 保留用户没提到的配置不变
2. 被脱敏的 `# [REDACTED]` 行必须原样保留（输出 `# [REDACTED] 字段名`），不要猜测或补充 key 值
3. 只输出 TOML，不要解释，不要 ```toml 标记
4. 保持 TOML 格式正确"#
    );

    // 调用 LLM
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) =
        sage_core::discovery::select_best_provider(&discovered, &configs)
            .ok_or("没有可用的 AI 服务")?;
    let agent_config = default_agent_config();
    let provider =
        sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let result = provider.invoke(&prompt, None).await.map_err(map_err)?;

    // 清理 LLM 输出（可能带 ```toml 标记）
    let clean = result
        .trim()
        .strip_prefix("```toml")
        .or_else(|| result.trim().strip_prefix("```"))
        .unwrap_or(result.trim())
        .strip_suffix("```")
        .unwrap_or(result.trim())
        .trim();

    // 验证 TOML 合法性
    let _: toml::Value = clean.parse().map_err(|e| format!("LLM 输出的 TOML 不合法: {e}"))?;

    // 还原脱敏行：用原始文件中的真实值替换 [REDACTED]
    let mut final_lines: Vec<String> = Vec::new();
    let original_lines: Vec<&str> = raw_toml.lines().collect();
    for line in clean.lines() {
        if line.contains("[REDACTED]") {
            // 找到原始行
            let field = line
                .trim_start_matches('#')
                .trim()
                .trim_start_matches("[REDACTED]")
                .trim();
            if let Some(orig) = original_lines.iter().find(|l| {
                let lf = l.split('=').next().unwrap_or("").trim();
                lf == field
            }) {
                final_lines.push(orig.to_string());
            } else {
                final_lines.push(line.to_string());
            }
        } else {
            final_lines.push(line.to_string());
        }
    }

    let final_toml = final_lines.join("\n");
    std::fs::write(&config_path, &final_toml).map_err(|e| format!("写入配置失败: {e}"))?;
    tracing::info!("Config updated via natural language");

    Ok("配置已更新".into())
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

        let depth = item["depth"].as_str().map(String::from);
        entries.push(sage_core::memory_integrator::IncomingMemory {
            content: final_content,
            category: category.to_string(),
            source: "chat".to_string(),
            confidence,
            about_person,
            depth,
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
            let id = if let Some(ref person) = entry.about_person {
                store.save_memory_about_person(
                    &entry.category,
                    &entry.content,
                    "chat",
                    entry.confidence,
                    "private",
                    person,
                )
            } else {
                store.save_memory_with_visibility(
                    &entry.category,
                    &entry.content,
                    "chat",
                    entry.confidence,
                    "private",
                )
            };
            if let (Ok(id), Some(ref depth)) = (&id, &entry.depth) {
                let valid = ["episodic", "semantic", "procedural", "axiom"];
                if valid.contains(&depth.as_str()) {
                    let _ = store.update_memory_depth(*id, depth);
                }
            }
        }
    }

    let display = format!("{}{}", raw[..start_idx].trim_end(), &raw[block_end..]);
    (display.trim().to_string(), saved)
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
