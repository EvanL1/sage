use serde_json::{json, Value};
use tauri::State;

use super::{get_provider, map_err};
use crate::AppState;

/// 获取消息列表 — 按 channel/source 过滤
#[tauri::command]
pub async fn get_messages(
    state: State<'_, AppState>,
    channel: Option<String>,
    source: Option<String>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(50);
    let messages = if let Some(ch) = &channel {
        state
            .store
            .get_messages_by_channel(ch, limit)
            .map_err(map_err)?
    } else if let Some(src) = &source {
        state
            .store
            .get_messages_by_source(src, limit)
            .map_err(map_err)?
    } else {
        state
            .store
            .get_messages_by_source("teams", limit)
            .map_err(map_err)?
    };
    Ok(serde_json::json!(messages))
}

/// 获取所有消息频道列表
#[tauri::command]
pub async fn get_message_channels(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let channels = state.store.get_message_channels().map_err(map_err)?;
    let result: Vec<serde_json::Value> = channels
        .into_iter()
        .map(|(channel, source, count)| {
            serde_json::json!({ "channel": channel, "source": source, "count": count })
        })
        .collect();
    Ok(serde_json::json!(result))
}

/// AI 消息洞察 — 分析当前频道/来源的消息并给出简要见解
#[tauri::command]
pub async fn summarize_messages(
    state: State<'_, AppState>,
    context: String,
    label: String,
) -> Result<String, String> {
    let lang = state.store.prompt_lang();
    let provider = get_provider(&state.store)?;

    let prompt = sage_core::prompts::cmd_analyze_message_flow_user(&lang, &label, &context);
    let system = sage_core::prompts::cmd_analyze_message_flow_system(&lang);

    let resp = provider
        .invoke(&prompt, Some(system))
        .await
        .map_err(map_err)?;
    Ok(resp)
}

/// Parse LLM channel-summary response into (summary, Vec<(priority, description)>).
fn parse_channel_summary(resp: &str) -> (String, Vec<(String, String)>) {
    let mut summary = String::new();
    let mut actions: Vec<(String, String)> = Vec::new();
    let mut in_actions = false;

    for line in resp.lines() {
        let line = line.trim();
        if let Some(rest) = line
            .strip_prefix("SUMMARY:")
            .or_else(|| line.strip_prefix("摘要："))
            .or_else(|| line.strip_prefix("摘要:"))
        {
            summary = rest.trim().to_string();
            in_actions = false;
        } else if line.starts_with("ACTIONS:") || line.starts_with("待办：") || line.starts_with("待办:") {
            let rest = line.split_once(':').map(|(_, r)| r.trim()).unwrap_or("");
            in_actions = !rest.eq_ignore_ascii_case("NONE") && rest != "无";
        } else if in_actions && line.starts_with("- ") {
            let item = line.trim_start_matches("- ").trim();
            let (priority, desc) = if item.starts_with("[P0]") {
                ("P0", &item[4..])
            } else if item.starts_with("[P1]") {
                ("P1", &item[4..])
            } else if item.starts_with("[P2]") {
                ("P2", &item[4..])
            } else {
                ("P1", item)
            };
            let task_content = desc.split('|').next().unwrap_or(desc).trim();
            if !task_content.is_empty() {
                actions.push((priority.to_string(), task_content.to_string()));
            }
        }
    }

    if summary.is_empty() {
        summary = resp.to_string();
    }
    (summary, actions)
}

/// 频道摘要 + 待办事项提取 — 可选自动创建 Task
#[tauri::command]
pub async fn summarize_channel(
    state: State<'_, AppState>,
    channel: String,
    source: Option<String>,
    create_tasks: Option<bool>,
) -> Result<Value, String> {
    let _source = source.unwrap_or_else(|| "teams".to_string());
    let messages = state.store.get_messages_by_channel(&channel, 100).map_err(map_err)?;

    if messages.is_empty() {
        return Ok(json!({"summary": "No messages to summarize.", "tasks_created": 0}));
    }

    let messages_text: String = messages
        .iter()
        .rev()
        .map(|m| format!(
            "[{}] {}: {}",
            m.timestamp.chars().take(16).collect::<String>(),
            m.sender,
            m.content.as_deref().unwrap_or("")
        ))
        .collect::<Vec<_>>()
        .join("\n");

    let chat_type = messages.first().map(|m| m.message_type.as_str()).unwrap_or("unknown");

    let lang = state.store.prompt_lang();
    let provider = get_provider(&state.store)?;

    let prompt = sage_core::prompts::cmd_summarize_channel_prompt(&lang, &channel, chat_type, &messages_text);
    let resp = provider.invoke(&prompt, None).await.map_err(map_err)?;

    let (summary, actions) = parse_channel_summary(&resp);

    let mut tasks_created = 0;
    if create_tasks.unwrap_or(true) {
        for (priority, desc) in &actions {
            let p = match priority.as_str() { "P0" => "P0", "P2" => "P2", _ => "P1" };
            let action_line = format!("create_task | {desc} | priority:{p}");
            if sage_core::pipeline::actions::execute_single_action(
                &action_line, &["create_task"], &state.store, "tauri_messages",
            ).is_some() {
                tasks_created += 1;
            } else {
                tracing::warn!("Failed to create task from channel summary (ACTION rejected)");
            }
        }
    }

    Ok(json!({
        "summary": summary,
        "actions": actions.iter().map(|(p, d)| json!({"priority": p, "description": d})).collect::<Vec<_>>(),
        "tasks_created": tasks_created,
        "message_count": messages.len(),
    }))
}

/// 处境纵览 — 通讯维度的实时态势感知
#[tauri::command]
pub async fn get_situation_summary(state: State<'_, AppState>) -> Result<String, String> {
    let lang = state.store.prompt_lang();
    let provider = get_provider(&state.store)?;

    // Gather: recent 48h messages + open tasks
    let recent_msgs = state.store.get_messages_by_source("email", 30).map_err(map_err)?;
    let teams_msgs = state.store.get_messages_by_source("teams", 30).map_err(map_err)?;
    let open_tasks = state.store.list_tasks(Some("open"), 20).map_err(map_err)?;
    let in_progress = state.store.list_tasks(Some("in_progress"), 10).map_err(map_err)?;

    let mut context = String::new();

    if !recent_msgs.is_empty() || !teams_msgs.is_empty() {
        context.push_str("## 近期通讯（48小时内）\n");
        for msg in recent_msgs.iter().chain(teams_msgs.iter()).take(40) {
            let dir = if msg.direction == "sent" { "→发出" } else { "←收到" };
            let preview: String = msg.content.as_deref().unwrap_or("").chars().take(100).collect();
            context.push_str(&format!(
                "- [{}][{}] {} | {} | {}\n",
                msg.source, dir, msg.sender, msg.channel, preview
            ));
        }
    }

    if !open_tasks.is_empty() || !in_progress.is_empty() {
        context.push_str("\n## 当前任务\n");
        // tuple: (id, content, status, priority, due_date, source, created_at, updated_at, outcome, verification, description)
        for task in in_progress.iter().chain(open_tasks.iter()).take(15) {
            context.push_str(&format!("- [{}][{}] {}\n", task.2, task.3, task.1));
        }
    }

    if context.is_empty() {
        return Ok(if lang == "en" { "No recent communications or tasks." } else { "暂无近期通讯和任务。" }.into());
    }

    let prompt = if lang == "en" {
        format!("Based on the following recent communications and tasks, generate a situational awareness briefing.\n\
                 Focus on: who is waiting for a response, what has been handled, what needs attention.\n\
                 Be concise — bullet points, no fluff. Use the person's perspective (\"you\").\n\n{context}")
    } else {
        format!("基于以下近期通讯和任务，生成一份**处境纵览**。\n\
                 重点关注：谁在等我回复、什么事已处理、什么需要注意。\n\
                 与任务进度对照：哪些通讯和任务有关联。\n\
                 简洁——用要点，不废话。站在「我」的视角。\n\n{context}")
    };

    provider.invoke(&prompt, None).await.map_err(map_err)
}

/// 获取通信社交图（person ↔ person，基于共同频道通信）
#[tauri::command]
pub async fn get_message_graph(state: State<'_, AppState>) -> Result<Value, String> {
    let data = state.store.get_message_graph_data().map_err(map_err)?;

    let mut person_set = std::collections::HashSet::new();
    for (a, b, _, _) in &data {
        person_set.insert(a.clone());
        person_set.insert(b.clone());
    }

    let mut id_map = std::collections::HashMap::new();
    let mut next_id = 1i64;
    let mut nodes = Vec::new();

    for person in &person_set {
        id_map.insert(person.clone(), next_id);
        nodes.push(json!({ "id": next_id, "label": person }));
        next_id += 1;
    }

    let edges: Vec<Value> = data
        .iter()
        .map(|(a, b, shared_ch, total_msgs)| {
            json!({
                "from": id_map[a],
                "to": id_map[b],
                "shared_channels": shared_ch,
                "weight": total_msgs,
            })
        })
        .collect();

    Ok(json!({ "nodes": nodes, "edges": edges }))
}

/// 获取各连接/工具的状态
#[tauri::command]
pub fn get_connections_status(state: State<'_, AppState>) -> Result<Value, String> {
    let now = chrono::Utc::now().timestamp();
    let lang = state.store.prompt_lang();
    let en = lang == "en";

    let bridge_last_seen = sage_core::bridge::bridge_last_seen();
    let browser_status = if bridge_last_seen == 0 {
        json!({ "status": "never", "label": if en { "Never connected" } else { "从未连接" } })
    } else {
        let ago = now - bridge_last_seen;
        if ago < 120 {
            json!({ "status": "connected", "label": if en { "Connected" } else { "已连接" }, "ago_seconds": ago })
        } else {
            let mins = ago / 60;
            let label = if en {
                if mins < 60 { format!("{}m ago", mins) } else { format!("{}h ago", mins / 60) }
            } else if mins < 60 {
                format!("{}分钟前", mins)
            } else {
                format!("{}小时前", mins / 60)
            };
            json!({ "status": "stale", "label": label, "ago_seconds": ago })
        }
    };

    let outlook_running = std::process::Command::new("pgrep")
        .arg("-x")
        .arg("Microsoft Outlook")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let outlook_status = if outlook_running {
        json!({ "status": "connected", "label": if en { "Running" } else { "运行中" } })
    } else {
        json!({ "status": "offline", "label": if en { "Not running" } else { "未运行" } })
    };

    let claude_status = {
        let sessions_dir = dirs::home_dir().map(|h| h.join(".claude/projects"));
        let has_recent = sessions_dir
            .and_then(|dir| {
                std::fs::read_dir(&dir)
                    .ok()
                    .map(|entries| entries.filter_map(|e| e.ok()).any(|_| true))
            })
            .unwrap_or(false);
        if has_recent {
            json!({ "status": "connected", "label": if en { "Configured" } else { "已配置" } })
        } else {
            json!({ "status": "offline", "label": if en { "Not detected" } else { "未检测到" } })
        }
    };

    let behavior_count = state
        .store
        .get_browser_behaviors(1)
        .map(|b| b.len())
        .unwrap_or(0);
    let behavior_status = if behavior_count > 0 {
        json!({ "status": "connected", "label": if en { "Collecting data" } else { "数据收集中" } })
    } else {
        json!({ "status": "idle", "label": if en { "No data yet" } else { "暂无数据" } })
    };

    // Email sources (IMAP)
    let imap_sources = state.store.get_message_sources_by_type("imap").unwrap_or_default();
    let imap_status = if imap_sources.is_empty() {
        json!({ "status": "offline", "label": if en { "Not configured" } else { "未配置" } })
    } else {
        let count = imap_sources.len();
        json!({ "status": "connected", "label": if en { format!("{count} source(s)") } else { format!("{count} 个源") } })
    };

    // Calendar (AppleScript)
    let calendar_ok = std::process::Command::new("osascript")
        .arg("-e").arg("tell application \"System Events\" to (name of processes) contains \"Microsoft Outlook\"")
        .output().map(|o| String::from_utf8_lossy(&o.stdout).contains("true")).unwrap_or(false);
    let calendar_status = if calendar_ok {
        json!({ "status": "connected", "label": if en { "Outlook Calendar" } else { "Outlook 日历" } })
    } else {
        json!({ "status": "offline", "label": if en { "Outlook not running" } else { "Outlook 未运行" } })
    };

    Ok(json!({
        "email_imap": imap_status,
        "outlook": outlook_status,
        "calendar": calendar_status,
        "claude_code": claude_status,
        "browser_extension": browser_status,
        "behavior_tracking": behavior_status,
    }))
}

/// 获取建议列表
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
    use sage_core::feedback::{FeedbackEffect, FeedbackProcessor};
    use sage_types::FeedbackAction;

    let feedback_action = match action.as_str() {
        "useful" => FeedbackAction::Useful,
        "not_useful" => FeedbackAction::NotUseful,
        _ if action.starts_with("never:") => {
            let user_complaint = action[6..].to_string();
            // LLM 把用户吐槽 + 原始建议上下文转化为可执行规则
            let refined = refine_negative_rule(&state, suggestion_id, &user_complaint).await;
            FeedbackAction::NeverDoThis(refined)
        }
        _ if action.starts_with("correction:") => {
            FeedbackAction::Correction(action[11..].to_string())
        }
        _ => {
            let lang = state.store.prompt_lang();
            return Err(if lang == "en" {
                format!("Unknown feedback type: {action}")
            } else {
                format!("未知的反馈类型: {action}")
            });
        }
    };

    let processor = FeedbackProcessor::new(&state.store);
    let effect = processor
        .process(suggestion_id, feedback_action)
        .map_err(map_err)?;

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

/// 用 LLM 把用户的模糊吐槽转化为具体可执行规则
async fn refine_negative_rule(
    state: &State<'_, AppState>,
    suggestion_id: i64,
    user_complaint: &str,
) -> String {
    // 查找原始建议内容
    let original = state
        .store
        .get_recent_suggestions(500)
        .ok()
        .and_then(|ss| ss.into_iter().find(|s| s.id == suggestion_id))
        .map(|s| format!("[{}] {}", s.event_source, s.response))
        .unwrap_or_default();

    if original.is_empty() {
        return user_complaint.to_string();
    }

    // 尝试用 LLM 精炼
    let provider = match get_provider(&state.store) {
        Ok(p) => p,
        Err(_) => return user_complaint.to_string(),
    };

    let lang = state.store.prompt_lang();
    let prompt = match lang.as_str() {
        "en" => format!(
            "The user rejected this AI suggestion:\n\"{original}\"\n\n\
             Their complaint: \"{user_complaint}\"\n\n\
             Convert this into ONE specific, actionable rule for the AI to follow. \
             The rule must clearly state what NOT to do and in what context. \
             Output only the rule, nothing else. Max 30 words."
        ),
        _ => format!(
            "用户拒绝了这条 AI 建议：\n「{original}」\n\n\
             用户的吐槽：「{user_complaint}」\n\n\
             把用户的吐槽转化为一条具体、可执行的规则，明确说明在什么场景下不要做什么。\
             只输出规则本身，不要其他内容。不超过30字。"
        ),
    };

    match provider.invoke(&prompt, None).await {
        Ok(refined) => {
            let rule = refined.trim().to_string();
            // 约束层：验证精炼后的规则（空或过长则回退到原始吐槽）
            if rule.is_empty() || rule.len() > 500 {
                tracing::warn!("Feedback: BLOCKED invalid negative rule (empty or too long)");
                user_complaint.to_string()
            } else {
                rule
            }
        }
        Err(_) => user_complaint.to_string(),
    }
}

#[tauri::command]
pub async fn delete_suggestion(
    state: State<'_, AppState>,
    suggestion_id: i64,
) -> Result<(), String> {
    state
        .store
        .delete_suggestion(suggestion_id)
        .map_err(map_err)
}

#[tauri::command]
pub async fn update_suggestion(
    state: State<'_, AppState>,
    suggestion_id: i64,
    response: String,
) -> Result<(), String> {
    state
        .store
        .update_suggestion_response(suggestion_id, &response)
        .map_err(map_err)
}
