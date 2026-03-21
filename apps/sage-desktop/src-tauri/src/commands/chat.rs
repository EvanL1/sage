use serde_json::{json, Value};
use tauri::State;

use super::{auto_answer_open_questions, default_agent_config, extract_and_save_memories, map_err};
use crate::AppState;

#[tauri::command]
pub async fn chat(
    state: State<'_, AppState>,
    message: String,
    session_id: Option<String>,
) -> Result<Value, String> {
    // 0. Chat 触发时 ingest 最近 24h 的 Claude Code sessions（仅在 Chat 使用时更新）
    {
        let claude_dir = sage_core::session_analyzer::default_claude_dir();
        if let Err(e) = sage_core::session_analyzer::ingest_sessions(&claude_dir, &state.store, 24) {
            tracing::debug!("Session ingest skipped: {e}");
        }
    }

    // 1. 生成或使用 session_id
    let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // 2. 页面生成请求检测 — 在主 prompt 构建之前短路
    if sage_core::skills::is_page_gen_request(&message) {
        let lang = state.store.prompt_lang();
        let discovered = sage_core::discovery::discover_providers(&state.store);
        let configs = state.store.load_provider_configs().map_err(map_err)?;
        if let Some((info, config)) =
            sage_core::discovery::select_best_provider(&discovered, &configs)
        {
            let agent_config = super::default_agent_config();
            let provider =
                sage_core::provider::create_provider_from_config(&info, &config, &agent_config);
            let system_prompt = sage_core::prompts::page_gen_system(&lang).to_string();
            if let Ok(markdown) = provider.invoke(&message, Some(&system_prompt)).await {
                let title = extract_page_title(&markdown);
                if let Ok(page_id) = state.store.save_custom_page(&title, &markdown) {
                    let reply = if lang == "en" {
                        format!("I've generated the page **{title}** for you.")
                    } else {
                        format!("已为你生成页面「{title}」。")
                    };
                    state
                        .store
                        .save_chat_message("user", &message, &sid)
                        .map_err(map_err)?;
                    state
                        .store
                        .save_chat_message("sage", &reply, &sid)
                        .map_err(map_err)?;
                    return Ok(json!({
                        "response": reply,
                        "session_id": sid,
                        "memories_saved": 0,
                        "page_id": page_id,
                    }));
                }
            }
        }
    }

    // 3. 保存用户消息
    state
        .store
        .save_chat_message("user", &message, &sid)
        .map_err(map_err)?;

    // 4. 加载 profile
    let lang = state.store.prompt_lang();
    let profile = state.store.load_profile().map_err(map_err)?;
    let fallback_name = if lang == "en" { "friend" } else { "朋友" };
    let user_name = profile
        .as_ref()
        .map(|p| p.identity.name.as_str())
        .unwrap_or(fallback_name)
        .to_string();

    // 4. 核心记忆（始终注入，不依赖搜索）+ 相关记忆（按话题搜索）
    let core_memories = state.store.load_core_memories().unwrap_or_default();
    let search_memories = state
        .store
        .search_memories_with_graph(&message, 6, 15)
        .map_err(map_err)?;

    // 合并去重：核心记忆在前，搜索记忆在后，按 id 去重
    let core_ids: std::collections::HashSet<i64> = core_memories.iter().map(|m| m.id).collect();
    let mut all_memories = core_memories;
    for m in search_memories {
        if !core_ids.contains(&m.id) {
            all_memories.push(m);
        }
    }

    let memory_text = if all_memories.is_empty() {
        if lang == "en" {
            "(Not enough context yet — more conversation will help me know you better)".to_string()
        } else {
            "（还没有积累足够的了解，需要通过更多对话来认识你）".to_string()
        }
    } else {
        all_memories
            .iter()
            .map(|m| {
                let person_tag = m
                    .about_person
                    .as_deref()
                    .map(|p| format!(" @{p}"))
                    .unwrap_or_default();
                format!("- [{}{}] {}", m.category, person_tag, m.content)
            })
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
        let header = if lang == "en" { "## Behavioral Observations" } else { "## 行为观察" };
        format!("\n\n{}\n{}", header, items.join("\n"))
    };

    // 4b. 加载近期浏览器行为（Teams 消息 + 页面访问 + 活动模式）
    let browser_text = build_browser_text(&state, &lang);

    // 4c. 加载建议反馈
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
                    Some("useful") => "✓ Helpful",
                    Some("not_useful") => "✗ Not helpful",
                    Some(other) => other,
                    None => "No rating",
                };
                format!(
                    "- [{}] {} → {}",
                    source,
                    response.chars().take(80).collect::<String>(),
                    fb_label
                )
            })
            .collect();
        let header = if lang == "en" { "## User Feedback on Suggestions" } else { "## 用户对建议的反馈" };
        format!("\n\n{}\n{}", header, items.join("\n"))
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
        .ok_or(if lang == "en" {
            "No AI service available. Please configure an API key in Settings."
        } else {
            "没有可用的 AI 服务。请在设置中配置 API Key。"
        })?;

    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    // 7. 确定对话层级
    let session_count = state.store.count_distinct_sessions().unwrap_or(1);
    let memory_count = all_memories.len();

    let layer = if session_count <= 3 && memory_count < 5 {
        "safety"
    } else if session_count <= 10 || memory_count < 15 {
        "patterns"
    } else {
        "deep"
    };

    // 8. 路由到合适的 chat skill
    let skill_name = sage_core::skills::route_chat_skill(&message);
    let skill_prompt = sage_core::skills::load_chat_skill(skill_name, &user_name, layer);

    // 9. 构建完整 system prompt
    let now = chrono::Local::now();
    let mut system_prompt = String::with_capacity(4000);
    system_prompt.push_str(&skill_prompt);

    let (time_header, about_header) = if lang == "en" {
        ("## Current Time (use this for all time-based reasoning)", "## About")
    } else {
        ("## 当前时间（所有时间推理以此为准）", "## 关于")
    };
    system_prompt.push_str(&format!(
        "\n\n{}\n{} ({})\n\n{} {}\n{}{}{}{}\n\n",
        time_header,
        now.format("%Y-%m-%d %A %H:%M"),
        now.format("%Z UTC%:z"),
        about_header,
        user_name,
        memory_text,
        obs_text,
        browser_text,
        feedback_text
    ));

    system_prompt.push_str(sage_core::prompts::chat_memory_write_protocol(&lang));
    system_prompt.push_str(sage_core::prompts::chat_safety_protocol(&lang));

    // Build conversation prompt with history
    let recent_history: Vec<_> = if history.len() > 1 {
        history[..history.len() - 1].to_vec()
    } else {
        vec![]
    };

    let user_label = if lang == "en" { "User" } else { "用户" };
    let mut full_prompt = String::new();
    for msg in &recent_history {
        let role_label = if msg.role == "user" { user_label } else { "Sage" };
        full_prompt.push_str(&format!("{}: {}\n\n", role_label, msg.content));
    }
    full_prompt.push_str(&format!("{}: {}", user_label, message));

    // 10. 调用 LLM（可取消）
    let llm_task =
        tokio::spawn(async move { provider.invoke(&full_prompt, Some(&system_prompt)).await });

    {
        let mut abort = state.chat_abort.lock().unwrap_or_else(|e| e.into_inner());
        *abort = Some(llm_task.abort_handle());
    }

    let raw_response = match llm_task.await {
        Ok(result) => result.map_err(map_err)?,
        Err(e) if e.is_cancelled() => {
            return Ok(json!({
                "response": "",
                "session_id": sid,
                "cancelled": true,
            }));
        }
        Err(e) => return Err(format!("Internal error: {e}")),
    };

    {
        let mut abort = state.chat_abort.lock().unwrap_or_else(|e| e.into_inner());
        *abort = None;
    }

    // 11. 解析并持久化 sage-memory 块
    let (display_response, memories_saved) =
        extract_and_save_memories(&raw_response, &state.store).await;

    // 11a. open_questions 自动回答检测
    auto_answer_open_questions(&message, &state.store);

    // 11b. 新记忆写入后，后台异步建立图谱连接 + 认知调和
    if memories_saved > 0 {
        let daemon = state.daemon.clone();
        let reconcile_text = raw_response.clone();
        tokio::spawn(async move {
            if let Err(e) = daemon.trigger_memory_linking().await {
                tracing::warn!("Auto-link after chat failed: {e}");
            }
            if let Err(e) = daemon.run_reconcile(&reconcile_text).await {
                tracing::warn!("Reconciler after chat failed: {e}");
            }
        });
    }

    // 11d. 更新 last_accessed_at + validation_count（每次被注入上下文计为一次验证）
    if !all_memories.is_empty() {
        let access_ids: Vec<i64> = all_memories.iter().map(|m| m.id).collect();
        let _ = state.store.touch_memories(&access_ids);
        for id in &access_ids {
            let _ = state.store.increment_validation(*id);
        }
    }

    // 11e. 冷边衰减
    let _ = state.store.decay_cold_edges(30, 0.9, 0.1);

    // 12. 保存 Sage 回复
    state
        .store
        .save_chat_message("sage", &display_response, &sid)
        .map_err(map_err)?;

    Ok(json!({
        "response": display_response,
        "session_id": sid,
        "memories_saved": memories_saved,
    }))
}

fn build_browser_text(state: &AppState, lang: &str) -> String {
    let behaviors = state
        .store
        .get_browser_behaviors_since(
            &(chrono::Local::now() - chrono::Duration::hours(24)).to_rfc3339(),
        )
        .unwrap_or_default();
    if behaviors.is_empty() {
        return String::new();
    }

    let mut teams_senders: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut domain_secs: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut patterns = Vec::new();

    for b in &behaviors {
        let meta: serde_json::Value = b
            .metadata
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        match b.event_type.as_str() {
            "message_received" if b.source == "teams" => {
                let sender = meta["sender"].as_str().unwrap_or("?").to_string();
                *teams_senders.entry(sender).or_insert(0) += 1;
            }
            "page_visit" => {
                let domain = meta["domain"].as_str().unwrap_or("?").to_string();
                let dur = meta["duration_seconds"].as_i64().unwrap_or(0);
                *domain_secs.entry(domain).or_insert(0) += dur;
            }
            "activity_pattern" => {
                let p = meta["pattern"].as_str().unwrap_or("?");
                let d = meta["domain"].as_str().unwrap_or("");
                patterns.push(if d.is_empty() {
                    p.to_string()
                } else {
                    format!("{p}: {d}")
                });
            }
            _ => {}
        }
    }

    let mut parts = Vec::new();
    if !teams_senders.is_empty() {
        let mut sorted: Vec<_> = teams_senders.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let lines: Vec<String> = sorted
            .iter()
            .take(10)
            .map(|(s, c)| {
                if lang == "en" {
                    format!("  - {s}: {c} msgs")
                } else {
                    format!("  - {s}：{c} 条")
                }
            })
            .collect();
        let msg_count = behaviors
            .iter()
            .filter(|b| b.event_type == "message_received" && b.source == "teams")
            .count();
        if lang == "en" {
            parts.push(format!("Teams messages ({msg_count} total):\n{}", lines.join("\n")));
        } else {
            parts.push(format!("Teams 消息（共 {} 条）：\n{}", msg_count, lines.join("\n")));
        }
    }
    if !domain_secs.is_empty() {
        let mut sorted: Vec<_> = domain_secs.into_iter().filter(|(_, s)| *s >= 30).collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let lines: Vec<String> = sorted
            .iter()
            .take(8)
            .map(|(d, s)| format!("  - {d}: {}m", s / 60))
            .collect();
        if !lines.is_empty() {
            if lang == "en" {
                parts.push(format!("Sites visited:\n{}", lines.join("\n")));
            } else {
                parts.push(format!("网站访问：\n{}", lines.join("\n")));
            }
        }
    }
    if !patterns.is_empty() {
        if lang == "en" {
            parts.push(format!("Activity patterns: {}", patterns.join(", ")));
        } else {
            parts.push(format!("活动模式：{}", patterns.join("、")));
        }
    }

    if parts.is_empty() {
        String::new()
    } else {
        let header = if lang == "en" { "## Browser Activity (Today)" } else { "## 浏览器活动（今日）" };
        format!("\n\n{}\n{}", header, parts.join("\n"))
    }
}

/// 从 markdown 文本首行提取页面标题（去掉 `# ` 前缀）
fn extract_page_title(markdown: &str) -> String {
    for line in markdown.lines() {
        if let Some(title) = line.trim().strip_prefix("# ") {
            return title.trim().to_string();
        }
    }
    "Untitled Page".to_string()
}

/// 取消正在进行的 Chat LLM 调用
#[tauri::command]
pub async fn cancel_chat(state: State<'_, AppState>) -> Result<(), String> {
    let mut abort = state.chat_abort.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(handle) = abort.take() {
        handle.abort();
    }
    Ok(())
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
pub async fn delete_chat_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<usize, String> {
    state.store.delete_session(&session_id).map_err(map_err)
}

#[tauri::command]
pub async fn get_chat_history(
    state: State<'_, AppState>,
    session_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let messages = if let Some(sid) = session_id {
        state.store.load_session_messages(&sid).map_err(map_err)?
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

/// Digital Twin 外部对话 — 只使用 public 记忆，只读模式
#[tauri::command]
pub async fn chat_external(state: State<'_, AppState>, message: String) -> Result<String, String> {
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let lang = state.store.prompt_lang();
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or_else(|| if lang == "en" {
            "LLM provider not configured. Please add an API key in Settings.".to_string()
        } else {
            "未配置 LLM provider，请先在 Settings 中配置 API key".to_string()
        })?;

    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let persona = sage_core::persona::Persona::new(std::sync::Arc::clone(&state.store));
    persona
        .chat(&message, provider.as_ref())
        .await
        .map_err(map_err)
}
