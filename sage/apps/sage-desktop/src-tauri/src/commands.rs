use sage_core::feedback::{FeedbackEffect, FeedbackProcessor};
use sage_core::onboarding::OnboardingState;
use sage_core::profile;
use sage_types::{
    BehaviorPrefs, CommPrefs, FeedbackAction, Project, ProviderConfig, Report, Stakeholder,
    UserIdentity, UserProfile, WorkContext, WorkSchedule,
};
use serde_json::{json, Value};
use tauri::State;

use crate::AppState;

fn map_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// 从 LLM 响应中提取 ```sage-memory JSON 块，写入 store，返回清理后的显示文本和保存数量
fn extract_and_save_memories(raw: &str, store: &sage_core::store::Store) -> (String, usize) {
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

    // 解析 JSON 数组
    let items: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (raw.to_string(), 0),
    };

    let mut saved = 0;
    for item in &items {
        let mem_type = item["type"].as_str().unwrap_or("");
        let content = item["content"].as_str().unwrap_or("");
        if content.is_empty() {
            continue;
        }
        let result = match mem_type {
            "task" => store.save_memory("task", content, "chat", 1.0),
            "insight" => store.save_memory("behavior", content, "chat", 0.8),
            "decision" => store.append_decision("chat", content),
            "reminder" => store.save_memory("task", &format!("[提醒] {content}"), "chat", 1.0),
            _ => store.save_memory("behavior", content, "chat", 0.8),
        };
        if result.is_ok() {
            saved += 1;
        }
    }

    // 从显示文本中移除 sage-memory 块
    let display = format!("{}{}", raw[..start_idx].trim_end(), &raw[block_end..]);
    (display.trim().to_string(), saved)
}

/// 从用户消息中提取关键词，自动匹配并回答 open_questions
fn auto_answer_open_questions(message: &str, store: &sage_core::store::Store) {
    // 提取 ≥3 字符的有意义词（中文每个字算 3 字节，取前 5 个关键词搜索）
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

fn default_agent_config() -> sage_core::config::AgentConfig {
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
fn claude_memory_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    // 从配置读取 project_dir，编码为路径格式
    let config_path = std::path::PathBuf::from(format!("{home}/.sage/config.toml"));
    let config = sage_core::config::Config::load_or_default(&config_path);
    let project_dir = config.agent.project_dir;
    let expanded = if project_dir.starts_with('~') {
        project_dir.replacen('~', &home, 1)
    } else {
        project_dir
    };
    let encoded = expanded.replace('/', "-");
    let dir = std::path::PathBuf::from(format!(
        "{home}/.claude/projects/{encoded}/memory"
    ));
    if dir.exists() { Some(dir) } else { None }
}

/// 触发 Sage → Claude Code 记忆同步（静默失败，不影响主流程）
fn trigger_memory_sync(store: &sage_core::store::Store) {
    if let Some(dir) = claude_memory_dir() {
        if let Err(e) = store.sync_to_claude_memory(&dir) {
            tracing::warn!("Memory sync to Claude Code failed: {e}");
        }
    }
}

/// 基于 UserProfile 生成个性化的"第一印象"，存为 insight 记忆，并返回文本
/// 如果 provider 未配置或调用失败，返回 None（静默跳过）
async fn generate_first_impression(
    state: &AppState,
    profile: &sage_types::UserProfile,
) -> Option<String> {
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().ok()?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)?;

    let agent_config = default_agent_config();
    let provider =
        sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let name = if profile.identity.name.is_empty() {
        "新用户"
    } else {
        &profile.identity.name
    };
    let role = profile.identity.role.as_str();
    let projects: Vec<&str> = profile
        .work_context
        .projects
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let stakeholders: Vec<&str> = profile
        .work_context
        .stakeholders
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    let reporting_line = profile.identity.reporting_line.join(" → ");

    let profile_summary = format!(
        "姓名：{name}\n\
         角色：{role}\n\
         汇报线：{reporting_line}\n\
         在推项目：{}\n\
         关键协作者：{}",
        projects.join("、"),
        stakeholders.join("、"),
    );

    let prompt = format!(
        "你刚认识了一个新用户。以下是他的人格画像：\n\n{profile_summary}\n\n\
         请用温暖、真诚的语气写 2-3 句你对这个人的第一印象。\
         不要泛泛而谈，要具体指向画像中的某个特质。\
         用中文。不要用任何 Markdown 格式，直接输出纯文字。"
    );

    let system = "你是 Sage，一个有温度的个人参谋。用简短真诚的语言描述你对这个人的第一印象。";

    match provider.invoke(&prompt, Some(system)).await {
        Ok(text) => {
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                return None;
            }
            let _ = state
                .store
                .save_memory("insight", &trimmed, "onboarding", 0.9);
            trigger_memory_sync(&state.store);
            tracing::info!("Onboarding first impression 已生成并存储");
            Some(trimmed)
        }
        Err(e) => {
            tracing::warn!("生成 first impression 失败（跳过）: {e}");
            None
        }
    }
}

#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>) -> Result<Option<Value>, String> {
    let profile = state.store.load_profile().map_err(map_err)?;
    match profile {
        Some(p) => Ok(Some(serde_json::to_value(&p).map_err(map_err)?)),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn save_profile(state: State<'_, AppState>, profile: Value) -> Result<(), String> {
    let p: sage_types::UserProfile = serde_json::from_value(profile).map_err(map_err)?;
    state.store.save_profile(&p).map_err(map_err)?;
    Ok(())
}

#[tauri::command]
pub async fn submit_onboarding_step(
    state: State<'_, AppState>,
    data: Value,
) -> Result<Value, String> {
    // 用内层作用域限制 MutexGuard 的生命周期，确保在 .await 之前释放锁
    let completed_profile: Option<(sage_types::UserProfile, String)> = {
        let mut guard = state.onboarding.lock().map_err(map_err)?;

        if guard.is_none() {
            *guard = Some(OnboardingState::new());
        }

        let ob = guard.as_mut().unwrap();
        ob.submit_step(data).map_err(map_err)?;

        if ob.is_complete() {
            let final_profile = guard.take().unwrap().into_profile();
            let sop_preview = profile::generate_sop(&final_profile);
            state.store.save_profile(&final_profile).map_err(map_err)?;
            Some((final_profile, sop_preview))
        } else {
            let (index, total) = ob.progress();
            let sop_preview = ob.preview_sop();
            return Ok(json!({
                "step": format!("{:?}", ob.current_step()),
                "index": index,
                "total": total,
                "sop_preview": sop_preview,
            }));
        }
    };

    // guard 已释放，可以安全 .await
    if let Some((final_profile, sop_preview)) = completed_profile {
        let first_impression = generate_first_impression(&state, &final_profile).await;

        return Ok(json!({
            "step": "Completed",
            "index": 7,
            "total": 7,
            "sop_preview": sop_preview,
            "first_impression": first_impression,
        }));
    }

    Ok(json!({"step": "Unknown"}))
}

#[tauri::command]
pub async fn get_suggestions(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let suggestions = state
        .store
        .get_recent_suggestions(limit.unwrap_or(50))
        .map_err(map_err)?;

    suggestions
        .into_iter()
        .map(|s| serde_json::to_value(&s).map_err(map_err))
        .collect()
}

#[tauri::command]
pub async fn submit_feedback(
    state: State<'_, AppState>,
    suggestion_id: i64,
    action: String,
) -> Result<Value, String> {
    let feedback_action = match action.as_str() {
        "useful" => FeedbackAction::Useful,
        "not_useful" => FeedbackAction::NotUseful,
        _ if action.starts_with("never:") => {
            FeedbackAction::NeverDoThis(action[6..].to_string())
        }
        _ if action.starts_with("correction:") => {
            FeedbackAction::Correction(action[11..].to_string())
        }
        _ => return Err(format!("未知的反馈类型: {action}")),
    };

    let processor = FeedbackProcessor::new(&state.store);
    let effect = processor.process(suggestion_id, feedback_action).map_err(map_err)?;

    match effect {
        FeedbackEffect::Recorded => Ok(json!({"effect": "recorded"})),
        FeedbackEffect::DemotionSuggested { category, count } => {
            Ok(json!({"effect": "demotion_suggested", "category": category, "count": count}))
        }
        FeedbackEffect::NegativeRuleAdded { rule } => {
            Ok(json!({"effect": "negative_rule_added", "rule": rule}))
        }
    }
}

#[tauri::command]
pub async fn delete_suggestion(
    state: State<'_, AppState>,
    suggestion_id: i64,
) -> Result<(), String> {
    state.store.delete_suggestion(suggestion_id).map_err(map_err)
}

#[tauri::command]
pub async fn update_suggestion(
    state: State<'_, AppState>,
    suggestion_id: i64,
    response: String,
) -> Result<(), String> {
    state.store.update_suggestion_response(suggestion_id, &response).map_err(map_err)
}

/// 批量保存 provider 优先级（接收有序的 provider_id 列表）
#[tauri::command]
pub async fn save_provider_priorities(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let saved = state.store.load_provider_configs().map_err(map_err)?;
    for (i, id) in ordered_ids.iter().enumerate() {
        let existing = saved.iter().find(|c| c.provider_id == *id);
        let config = sage_types::ProviderConfig {
            provider_id: id.clone(),
            api_key: existing.and_then(|c| c.api_key.clone()),
            model: existing.and_then(|c| c.model.clone()),
            base_url: existing.and_then(|c| c.base_url.clone()),
            enabled: existing.map(|c| c.enabled).unwrap_or(true),
            priority: Some(i as u8),
        };
        state.store.save_provider_config(&config).map_err(map_err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn reset_onboarding(state: State<'_, AppState>) -> Result<(), String> {
    let mut onboarding = state.onboarding.lock().map_err(map_err)?;
    *onboarding = Some(OnboardingState::new());
    Ok(())
}

#[tauri::command]
pub async fn get_system_status(state: State<'_, AppState>) -> Result<Value, String> {
    let has_profile = state.store.load_profile().map_err(map_err)?.is_some();
    let sop_version = state.store.get_sop_version().map_err(map_err)?;

    Ok(json!({
        "status": if has_profile { "ready" } else { "needs_onboarding" },
        "has_profile": has_profile,
        "sop_version": sop_version,
    }))
}

// ─── Provider 相关命令 ──────────────────────────────

#[tauri::command]
pub async fn discover_providers(
    state: State<'_, AppState>,
) -> Result<Vec<sage_types::ProviderInfo>, String> {
    Ok(sage_core::discovery::discover_providers(&state.store))
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn quick_setup(
    state: State<'_, AppState>,
    name: String,
    role: Option<String>,
    reporting_line: Option<Vec<String>>,
    projects: Option<Vec<Value>>,
    stakeholders: Option<Vec<Value>>,
    schedule: Option<Value>,
    communication: Option<Value>,
    api_key: Option<String>,
    provider_id: Option<String>,
) -> Result<Value, String> {
    let parsed_projects: Vec<Project> = projects
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    let parsed_stakeholders: Vec<Stakeholder> = stakeholders
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    let parsed_schedule: WorkSchedule = schedule
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let parsed_comm: CommPrefs = communication
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let profile = UserProfile {
        identity: UserIdentity {
            name,
            role: role.unwrap_or_default(),
            reporting_line: reporting_line.unwrap_or_default(),
            primary_language: "zh".into(),
            secondary_language: "en".into(),
        },
        work_context: WorkContext {
            projects: parsed_projects,
            stakeholders: parsed_stakeholders,
            tech_stack: Vec::new(),
        },
        communication: parsed_comm,
        schedule: parsed_schedule,
        preferences: BehaviorPrefs::default(),
        negative_rules: Vec::new(),
        sop_version: 1,
    };

    if let (Some(key), Some(pid)) = (api_key, provider_id) {
        let config = ProviderConfig {
            provider_id: pid,
            api_key: Some(key),
            model: None,
            base_url: None,
            enabled: true,
            priority: None,
        };
        state.store.save_provider_config(&config).map_err(map_err)?;
    }

    state.store.save_profile(&profile).map_err(map_err)?;

    Ok(json!({ "status": "ready" }))
}

#[tauri::command]
pub async fn save_provider_config(
    state: State<'_, AppState>,
    config: sage_types::ProviderConfig,
) -> Result<(), String> {
    state.store.save_provider_config(&config).map_err(map_err)
}

#[tauri::command]
pub async fn get_provider_configs(
    state: State<'_, AppState>,
) -> Result<Vec<sage_types::ProviderConfig>, String> {
    state.store.load_provider_configs().map_err(map_err)
}

#[tauri::command]
pub async fn test_provider(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Value, String> {
    let providers = sage_core::discovery::discover_providers(&state.store);
    let info = providers
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Provider not found: {provider_id}"))?;

    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let config = configs
        .into_iter()
        .find(|c| c.provider_id == provider_id)
        .unwrap_or(ProviderConfig {
            provider_id: provider_id.clone(),
            api_key: None,
            model: None,
            base_url: None,
            enabled: true,
            priority: None,
        });

    let agent_config = default_agent_config();

    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    match provider.invoke("Say hello in one sentence.", None).await {
        Ok(response) => Ok(json!({ "success": true, "response": response })),
        Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
    }
}

#[tauri::command]
pub async fn chat(
    state: State<'_, AppState>,
    message: String,
    session_id: Option<String>,
) -> Result<Value, String> {
    // 1. 生成或使用 session_id
    let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // 2. 保存用户消息
    state
        .store
        .save_chat_message("user", &message, &sid)
        .map_err(map_err)?;

    // 3. 加载 profile
    let profile = state.store.load_profile().map_err(map_err)?;
    let user_name = profile
        .as_ref()
        .map(|p| p.identity.name.as_str())
        .unwrap_or("朋友")
        .to_string();

    // 4. 搜索相关记忆（FTS5，最多 10 条最相关）
    let memories = state.store.search_memories(&message, 10).map_err(map_err)?;
    let memory_text = if memories.is_empty() {
        "（还没有积累足够的了解，需要通过更多对话来认识你）".to_string()
    } else {
        memories
            .iter()
            .map(|m| format!("- [{}] {}", m.category, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // 4a. 加载行为观察
    let observations = state.store.load_recent_observations(20).unwrap_or_default();
    let obs_text = if observations.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = observations
            .iter()
            .map(|(cat, obs)| format!("- [{}] {}", cat, obs))
            .collect();
        format!("\n\n## 行为观察\n{}", items.join("\n"))
    };

    // 4b. 加载建议反馈
    let suggestion_feedback = state
        .store
        .get_suggestions_with_feedback(10)
        .unwrap_or_default();
    let feedback_text = if suggestion_feedback.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = suggestion_feedback
            .iter()
            .map(|(source, response, fb)| {
                let fb_label = match fb.as_deref() {
                    Some("useful") => "✓ 有用",
                    Some("not_useful") => "✗ 没用",
                    Some(other) => other,
                    None => "未评价",
                };
                format!(
                    "- [{}] {} → {}",
                    source,
                    response.chars().take(80).collect::<String>(),
                    fb_label
                )
            })
            .collect();
        format!("\n\n## 用户对建议的反馈\n{}", items.join("\n"))
    };

    // 5. 加载对话历史（取最近 20 条，窗口化压缩避免 prompt 过长）
    let history = state
        .store
        .get_recent_messages_for_prompt(&sid, 20)
        .map_err(map_err)?;

    // 6. 发现并选择 provider
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("没有可用的 AI 服务。请在设置中配置 API Key。")?;

    let agent_config = default_agent_config();

    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    // 7. 确定对话层级
    let session_count = state.store.count_distinct_sessions().unwrap_or(1);
    let memory_count = memories.len();

    let layer = if session_count <= 3 && memory_count < 5 {
        "safety"       // 安全基底
    } else if session_count <= 10 || memory_count < 15 {
        "patterns"     // 模式识别
    } else {
        "deep"         // 深度工作
    };

    // 8. 路由到合适的 chat skill
    let skill_name = sage_core::skills::route_chat_skill(&message);
    let skill_prompt = sage_core::skills::load_chat_skill(skill_name, &user_name, layer);

    // 9. 构建完整 system prompt = skill + 用户上下文 + 共享能力
    let now = chrono::Local::now();
    let mut system_prompt = String::with_capacity(4000);
    system_prompt.push_str(&skill_prompt);
    system_prompt.push_str(&format!(
        "\n\n## 当前时间（所有时间推理以此为准）\n{} ({})\n\n## 关于 {}\n{}{}{}\n\n",
        now.format("%Y-%m-%d %A %H:%M"),
        now.format("%Z UTC%:z"),
        user_name, memory_text, obs_text, feedback_text
    ));

    // --- 记忆写入能力（所有 skill 共享）---
    system_prompt.push_str("\
## 记忆写入
你可以将重要信息持久化保存。当用户要求你「记住」「记下」「提醒我」某事，或你发现值得保存的洞察时，在回复末尾添加 JSON 块：
```sage-memory
[{\"type\": \"task\", \"content\": \"准备 PULSE 拓扑图给 Bob\"}, {\"type\": \"insight\", \"content\": \"用户倾向于...\"}]
```
type 可选值：task（待办任务）、insight（关于用户的洞察）、decision（用户做的决定）、reminder（定时提醒）。
**只在需要时添加，不要每次都加。** 用户不会看到这个 JSON 块。\n\n");

    // --- 安全协议（所有 skill 共享）---
    system_prompt.push_str("\
## 安全协议
当出现自我伤害暗示、严重抑郁/绝望表达、解离或闪回迹象时：
1. 直接确认：\"我听到你了，这很重要\"
2. 安全评估：\"你现在安全吗？\"
3. 引导专业帮助：建议联系心理咨询师
4. 提供资源：心理援助热线 400-161-9995\n");

    // 9. 构建包含历史的 prompt
    // history 已通过 get_recent_messages_for_prompt 窗口化（最多 20 条），
    // 排除最后一条（刚保存的用户消息，避免重复）
    let recent_history: Vec<_> = if history.len() > 1 {
        history[..history.len() - 1].to_vec()
    } else {
        vec![]
    };

    let mut full_prompt = String::new();
    for msg in &recent_history {
        let role_label = if msg.role == "user" { "用户" } else { "Sage" };
        full_prompt.push_str(&format!("{}: {}\n\n", role_label, msg.content));
    }
    full_prompt.push_str(&format!("用户: {}", message));

    // 10. 调用 LLM
    let raw_response = provider
        .invoke(&full_prompt, Some(&system_prompt))
        .await
        .map_err(map_err)?;

    // 11. 解析并持久化 sage-memory 块
    let (display_response, memories_saved) = extract_and_save_memories(&raw_response, &state.store);

    // 11a. 轻量级 open_questions 自动回答检测
    // 从用户消息中提取关键词，匹配 open questions 并标记为 answered
    auto_answer_open_questions(&message, &state.store);

    // 12. 保存 Sage 回复（只保存用户可见部分）
    state
        .store
        .save_chat_message("sage", &display_response, &sid)
        .map_err(map_err)?;

    // 13. 返回
    Ok(json!({
        "response": display_response,
        "session_id": sid,
        "memories_saved": memories_saved,
    }))
}

#[tauri::command]
pub async fn list_chat_sessions(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let sessions = state
        .store
        .list_sessions(limit.unwrap_or(30))
        .map_err(map_err)?;
    sessions
        .into_iter()
        .map(|s| serde_json::to_value(&s).map_err(map_err))
        .collect()
}

#[tauri::command]
pub async fn get_chat_history(
    state: State<'_, AppState>,
    session_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let messages = if let Some(sid) = session_id {
        state
            .store
            .load_session_messages(&sid)
            .map_err(map_err)?
    } else {
        state
            .store
            .load_recent_messages(limit.unwrap_or(50))
            .map_err(map_err)?
    };
    messages
        .into_iter()
        .map(|m| serde_json::to_value(&m).map_err(map_err))
        .collect()
}

#[tauri::command]
pub async fn get_memories(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let memories = state.store.load_memories().map_err(map_err)?;
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
    // 1. 加载 session 对话
    let messages = state
        .store
        .load_session_messages(&session_id)
        .map_err(map_err)?;
    if messages.len() < 2 {
        // 对话太短，不值得提取
        return Ok(vec![]);
    }

    // 2. 加载已有记忆
    let existing = state.store.load_memories().map_err(map_err)?;
    let existing_text = if existing.is_empty() {
        "（暂无）".to_string()
    } else {
        existing
            .iter()
            .map(|m| {
                format!(
                    "[{}] {} (置信度: {:.1})",
                    m.category, m.content, m.confidence
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // 3. 构建对话文本
    let conversation = messages
        .iter()
        .map(|m| {
            let role = if m.role == "user" { "用户" } else { "Sage" };
            format!("{}: {}", role, m.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 4. 选择 provider
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("没有可用的 AI 服务")?;

    let agent_config = default_agent_config();

    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    // 5. 让 LLM 提取洞察
    let extraction_prompt = format!(
        "分析以下对话，提取关于用户的关键洞察。\n\n\
         关注以下维度：\n\
         - identity: 用户是谁，自我认知\n\
         - values: 什么对用户最重要\n\
         - behavior: 行为模式、习惯\n\
         - thinking: 思维方式、决策风格\n\
         - emotion: 情绪线索、触发因素\n\
         - growth: 成长方向、追求\n\n\
         已有记忆：\n{}\n\n\
         对话内容：\n{}\n\n\
         请以 JSON 数组格式输出新的洞察，每条包含：\n\
         - category: 上述维度之一\n\
         - content: 具体观察（一句话）\n\
         - confidence: 0.0-1.0 的置信度\n\n\
         只输出 JSON 数组，不要其他文字。如果没有新洞察，输出空数组 []。\n\
         示例：[{{\"category\":\"values\",\"content\":\"重视团队成长胜过个人表现\",\"confidence\":0.6}}]",
        existing_text, conversation
    );

    let system =
        "你是一个专业的心理观察者和行为分析师。你的任务是从对话中提取关于用户的洞察。只输出 JSON。";

    let result = provider
        .invoke(&extraction_prompt, Some(system))
        .await
        .map_err(map_err)?;

    // 6. 解析 JSON 并保存
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
            let id = state
                .store
                .save_memory(category, content, "chat", confidence)
                .map_err(map_err)?;
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
    state
        .store
        .sync_to_claude_memory(&dir)
        .map_err(map_err)?;
    Ok(format!("Synced to {}", dir.display()))
}

#[tauri::command]
pub async fn delete_memory(
    state: State<'_, AppState>,
    memory_id: i64,
) -> Result<(), String> {
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
                .save_memory("personality", content, "assessment", confidence)
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

    // Profile summary
    if let Some(p) = profile {
        md.push_str("## Profile\n\n");
        if !p.identity.name.is_empty() {
            md.push_str(&format!("**Name**: {}\n\n", p.identity.name));
        }
        if !p.identity.role.is_empty() {
            md.push_str(&format!("**Role**: {}\n\n", p.identity.role));
        }
    }

    // Group by category
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

    // Uncategorized
    let known: Vec<&str> = category_labels.iter().map(|(c, _)| *c).collect();
    let other: Vec<_> = memories.iter().filter(|m| !known.contains(&m.category.as_str())).collect();
    if !other.is_empty() {
        md.push_str("## Other\n\n");
        for m in other {
            md.push_str(&format!("- [{}] {} (confidence: {:.0}%)\n", m.category, m.content, m.confidence * 100.0));
        }
        md.push('\n');
    }

    md.push_str("---\n*Exported from Sage*\n");

    // 直接用 pbcopy 写入系统剪贴板（macOS only）
    use std::io::Write;
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("pbcopy failed: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(md.as_bytes()).map_err(|e| format!("write failed: {e}"))?;
    }
    child.wait().map_err(|e| format!("pbcopy wait failed: {e}"))?;

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
                .save_memory(category, content, source, confidence)
                .map_err(map_err)?;
            count += 1;
        }
    }
    if count > 0 {
        trigger_memory_sync(&state.store);
    }
    Ok(count)
}

// ─── 用户主动输入记忆 ──────────────────────────

/// 用户主动告诉 Sage 想被记住的内容
#[tauri::command]
pub async fn add_user_memory(
    state: State<'_, AppState>,
    content: String,
) -> Result<i64, String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("内容不能为空".to_string());
    }
    let id = state
        .store
        .save_memory("user_input", &content, "user", 1.0)
        .map_err(map_err)?;
    trigger_memory_sync(&state.store);
    Ok(id)
}

// ─── Questioner 命令 ──────────────────────────

/// 获取最近一条苏格拉底式每日问题（由 Questioner 模块生成）
#[tauri::command]
pub async fn get_daily_question(state: State<'_, AppState>) -> Result<Option<Value>, String> {
    match state.store.get_daily_question().map_err(map_err)? {
        Some(s) => Ok(Some(serde_json::to_value(&s).map_err(map_err)?)),
        None => Ok(None),
    }
}

// ─── Report 命令 ──────────────────────────────

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

/// 手动触发报告生成（通过 Daemon 的 trigger_report 方法）
#[tauri::command]
pub async fn trigger_report(
    state: State<'_, AppState>,
    report_type: String,
) -> Result<String, String> {
    let valid_types = ["morning", "evening", "weekly", "week_start"];
    if !valid_types.contains(&report_type.as_str()) {
        return Err(format!("未知报告类型: {report_type}，支持: morning/evening/weekly/week_start"));
    }
    state.daemon.trigger_report(&report_type).await.map_err(map_err)
}

// ─── 外部 AI 记忆导入（Claude/Gemini/ChatGPT 记忆粘贴） ──────────────────────────

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

    // 发现并选择 provider
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("没有可用的 AI 服务。请在设置中配置 API Key。")?;

    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let prompt = format!(
        "以下是用户从其他 AI 助手（Claude/Gemini/ChatGPT）导出的记忆或个人信息：\n\n{text}\n\n\
         请将其解析为结构化记忆条目。每条输出一行 JSON：\n\
         {{\"category\": \"...\", \"content\": \"...\"}}\n\n\
         可用 category：identity, personality, values, behavior, thinking, emotion, \
         growth, decision, pattern, preference, skill, relationship, goal\n\n\
         要求：\n\
         - 保留原始信息的核心内容，忠于原文\n\
         - 每条记忆简洁明了（1-2句话）\n\
         - 只输出 JSON 行，不要其他内容（不要 markdown 代码块）"
    );

    let raw = provider
        .invoke(&prompt, None)
        .await
        .map_err(map_err)?;

    // 解析 LLM 输出的 JSON 行
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
                    state
                        .store
                        .save_memory(category, content, "ai_import", 0.7)
                        .map_err(map_err)?;
                    count += 1;
                }
            }
        }
    }

    if count > 0 {
        trigger_memory_sync(&state.store);
    }
    Ok(count)
}
