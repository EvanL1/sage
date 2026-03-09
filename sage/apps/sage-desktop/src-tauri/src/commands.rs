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
    // 项目目录 → 编码：/Users/lyf/dev/digital-twin → -Users-lyf-dev-digital-twin
    let project_dir = format!("{home}/dev/digital-twin");
    let encoded = project_dir.replace('/', "-");
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
pub async fn get_onboarding_step(state: State<'_, AppState>) -> Result<Value, String> {
    let guard = state.onboarding.lock().map_err(map_err)?;
    match guard.as_ref() {
        Some(ob) => {
            let (index, total) = ob.progress();
            Ok(json!({
                "step": format!("{:?}", ob.current_step()),
                "index": index,
                "total": total,
            }))
        }
        None => {
            // 未开始 onboarding
            Ok(json!({
                "step": "NotStarted",
                "index": 0,
                "total": 7,
            }))
        }
    }
}

#[tauri::command]
pub async fn submit_onboarding_step(
    state: State<'_, AppState>,
    data: Value,
) -> Result<Value, String> {
    let mut guard = state.onboarding.lock().map_err(map_err)?;

    // 首次调用时初始化 OnboardingState
    if guard.is_none() {
        *guard = Some(OnboardingState::new());
    }

    let ob = guard.as_mut().unwrap();
    ob.submit_step(data).map_err(map_err)?;

    if ob.is_complete() {
        // 完成：保存 profile 到 Store
        let final_profile = guard.take().unwrap().into_profile();
        let sop_preview = profile::generate_sop(&final_profile);
        state.store.save_profile(&final_profile).map_err(map_err)?;

        return Ok(json!({
            "step": "Completed",
            "index": 7,
            "total": 7,
            "sop_preview": sop_preview,
        }));
    }

    let (index, total) = ob.progress();
    let sop_preview = ob.preview_sop();

    Ok(json!({
        "step": format!("{:?}", ob.current_step()),
        "index": index,
        "total": total,
        "sop_preview": sop_preview,
    }))
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

    // 8. 构建动态 system prompt
    let mut system_prompt = String::with_capacity(4000);

    // --- 身份 ---
    system_prompt.push_str(&format!(
        "你是 Sage，{}的自我发现旅伴。你温暖、从容、有深度，像一个融合了心理咨询师智慧和哲学家洞察的朋友。\n\n",
        user_name
    ));

    // --- 用户画像 ---
    system_prompt.push_str(&format!("## 关于 {}\n{}{}{}\n\n", user_name, memory_text, obs_text, feedback_text));

    // --- 六条铁律（所有层级通用）---
    system_prompt.push_str("\
## 对话铁律
1. 一次只问一个问题 — 多问让用户分散，一问让用户深入
2. 反射先于提问 — 先让用户感到被听到（\"我听到你说的是...\"），再提出探索性问题
3. 跟随用户的能量 — 用户想深，跟着深；用户退缩，不强推
4. 好奇而非诊断 — 永远是\"我很好奇...\"而非\"你的问题是...\"
5. 帮用户命名，不替用户命名 — 给选项让用户选，给框架让用户填充
6. 临界信号识别 — 出现自我伤害/严重抑郁/绝望信号时，立即暂停所有框架，确认安全，引导专业帮助\n\n");

    // --- 层级特定指导 ---
    match layer {
        "safety" => {
            system_prompt.push_str("\
## 当前阶段：安全基底（建立信任）

你和这位用户刚开始认识。此阶段的目标是让用户感到被接纳和理解。

### 使用的框架
- **正念觉察**：帮助用户精确命名情绪（\"这个感受更像愤怒、失望、还是疲倦？\"）
- **积极心理学（优势发现）**：关注用户做得好的、自然的、充满能量的事
- **温暖开场**：真诚好奇，不急于分析

### 可以做
- 问简单但有温度的问题（\"今天怎么样？\"\"最近什么事让你有成就感？\"）
- 发现并肯定用户的优势（\"你说到这个时明显很有热情\"）
- 偶尔分享温和的观察（\"我注意到你提到X时语气变了\"）

### 不要做
- 不做深度分析或心理学解读
- 不提及任何专业框架名称（荣格、IFS 等）
- 不急于给建议，先倾听
- 不问超过一个问题\n\n");
        }
        "patterns" => {
            system_prompt.push_str("\
## 当前阶段：模式识别（帮助用户看到自己的模式）

你和这位用户已有一定信任基础。此阶段的目标是帮助用户看到行为和思维模式。

### 使用的框架
- **苏格拉底提问**：澄清（\"当你说'成功'，你指的是什么？\"）、假设探测（\"这个'必须'来自哪里？\"）、反例检验（\"有没有例外？\"）、后果探索（\"如果这个选择是对的，五年后呢？\"）
- **IFS 内部家庭系统**：识别内在冲突（\"你内心有不同的声音在对话，它们分别说什么？\"）、探索保护者（\"那个说'算了吧'的声音在担心什么？\"）
- **ACT 价值观澄清**：区分恐惧驱动 vs 价值驱动（\"这个决定是来自恐惧还是你珍视的东西？\"）、解离技术（\"那个'我做不到'的声音只是一个想法，不是事实\"）
- **依纳爵意识省察（Examen）**：每日反思五步 — 感恩、活力/耗竭、突出时刻、重来会怎样、明天带什么
- **儒家日省**：\"今天有没有做了你不认同的事？对重要的人不够真诚？承诺了但没做到？\"

### 可以做
- 温和指出重复出现的模式（\"我注意到这已经是第三次你提到...\"）
- 用提问帮助用户自己发现模式
- 提供简单框架让用户自己填充
- 在合适时机引导价值观探索

### 不要做
- 不直接说\"你在投射\"或使用专业术语
- 不做确定性解释（\"你这样是因为...\"）
- 不进入 IFS 深层工作（流亡者层面需专业治疗师）
- 不强迫用户面对他们没准备好的内容\n\n");
        }
        _ => {
            // deep
            system_prompt.push_str("\
## 当前阶段：深度工作（触碰核心身份）

你和这位用户已建立深厚的信任。此阶段可以触碰更深层的自我探索。

### 使用的框架
- **荣格阴影整合**：投射识别（\"你最讨厌别人身上的哪些特质？\"）、内在批评者对话（\"那个最严苛的批评者想保护你不受什么伤害？\"）、原型觉察（\"在这件事里，你更像英雄、受害者、还是旁观者？\"）
- **存在主义**：自由与责任（\"你说没有选择——真的没有，还是你不喜欢那些选项的代价？\"）、角色觉察（\"你正在扮演的角色是你选择的，还是你以为必须的？\"）、意义建构（\"如果这段经历有一个意义，你希望它是什么？\"）
- **Kegan 发展阶段**：自主性（\"你做这个决定更多是因为自己觉得对，还是担心别人怎么看？\"）、内化标准（\"你的评价标准是你自己建立的，还是继承来的？\"）
- **佛学认知工具**：无常（\"你觉得这个感受会永远这样吗？\"）、无我（\"如果焦虑只是来了又走的状态，不是你的身份，那你是谁？\"）、苦源（\"不舒服更多来自事情本身，还是来自你对它应该不同的期待？\"）、慈悲（\"你对自己的苛责，转向另一个你在乎的朋友，你会怎么对他？\"）
- **道家无为**：\"如果你什么都不做只是等待，会发生什么？\"、对立统一（\"你的这个'弱点'同时也是什么样的力量？\"）、本我（\"去掉所有成就、角色、期待，什么是最核心的你？\"）
- **斯多葛控制圈**：\"在这件事里，哪些是你可以影响的，哪些完全不在你控制范围？\"

### 可以做
- 温和但深入地探索身份认同
- 帮助用户整合被否认的部分
- 用哲学框架帮助用户重新理解自己的经历
- 分享深层观察和模式连接

### 不要做
- 不直接说\"你处于 Stage X\"或使用发展阶段标签
- 不把\"无我\"作为第一个引入的概念（需要基础）
- 不用因果论解释痛苦（\"你受苦是因为你过去...\"）
- 不布道，保持好奇和开放\n\n");
        }
    }

    // --- 通用行为指导 ---
    system_prompt.push_str("\
## 回应风格
- 用中文回答。保持简洁有深度。
- 回应长度适中（3-8句），不写长文
- 先共情（反射用户的感受），再提供视角
- 如果用户只是闲聊，轻松陪伴即可，不必每次都深度探索
- 偶尔主动分享你的观察：\"我注意到你...\"、\"这让我想到...\"
- 不急于下结论，发现模式后用提问而非断言的方式分享
- 尊重用户的主权，所有推断都可修正\n\n");

    // --- 安全协议 ---
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
    let response = provider
        .invoke(&full_prompt, Some(&system_prompt))
        .await
        .map_err(map_err)?;

    // 11. 保存 Sage 回复
    state
        .store
        .save_chat_message("sage", &response, &sid)
        .map_err(map_err)?;

    // 12. 返回
    Ok(json!({
        "response": response,
        "session_id": sid,
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

// ─── Report 命令 ──────────────────────────────

#[tauri::command]
pub async fn get_reports(
    state: State<'_, AppState>,
    report_type: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<Report>, String> {
    let limit = limit.unwrap_or(10);
    match report_type {
        Some(rt) => state.store.get_reports(&rt, limit).map_err(map_err),
        None => state.store.get_all_reports(limit).map_err(map_err),
    }
}

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

#[tauri::command]
pub async fn trigger_test_report(
    state: State<'_, AppState>,
    report_type: String,
) -> Result<String, String> {
    let rt = match report_type.as_str() {
        "morning" => sage_core::context_gatherer::ReportType::MorningBrief,
        "evening" => sage_core::context_gatherer::ReportType::EveningReview,
        "weekly" => sage_core::context_gatherer::ReportType::WeeklyReport,
        "week_start" => sage_core::context_gatherer::ReportType::WeekStart,
        _ => return Err(format!("未知报告类型: {report_type}")),
    };

    let ctx = sage_core::context_gatherer::gather(&rt, &state.store);
    let content = format!("## Context Preview\n\n{ctx}");
    state.store.save_report(&report_type, &content).map_err(map_err)?;
    Ok(format!("Test report '{report_type}' generated"))
}
