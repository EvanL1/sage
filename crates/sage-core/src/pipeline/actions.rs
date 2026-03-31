//! ACTION 约束系统 — 文档生成、参数验证、执行分发
//!
//! 所有 UserDefinedStage（包括预设和自定义）的 ACTION 都通过这个模块处理。
//! 硬约束：授权白名单 + 参数验证 + rate limit
//! 软约束：action_docs 注入到 prompt 中

use tracing::{info, warn};

use crate::store::Store;

/// ACTION 执行结果
pub struct ActionResult {
    pub count: usize,
    /// 每个执行的 action 的 (name, result_id)
    pub results: Vec<(String, i64)>,
}

/// Action 说明文档，注入到 prompt 中
pub fn action_docs(actions: &[String]) -> String {
    let mut doc = String::from("## 可用动作\n你可以输出以下 ACTION 命令，每行一条：\n\n");
    for action in actions {
        if let Some(d) = action_doc_entry(action) {
            doc.push_str(d);
        }
    }
    doc.push_str("如果没有需要执行的动作，只输出分析文本即可。不要编造不必要的 ACTION。\n");
    doc
}

fn action_doc_entry(action: &str) -> Option<&'static str> {
    match action {
        "create_task" => Some(
            "- `ACTION create_task | 任务内容 | priority:normal/P0/P1/P2 | due:YYYY-MM-DD`\n  创建一个待办任务\n\n"
        ),
        "save_memory" => Some(
            "- `ACTION save_memory | category | 记忆内容 | confidence:0.0-1.0`\n  保存一条认知记忆（默认 public 可见）\n\n"
        ),
        "save_memory_visible" => Some(
            "- `ACTION save_memory_visible | category | 记忆内容 | confidence:0.0-1.0 | visibility:public/subconscious/private`\n  保存一条认知记忆（可指定可见性）\n\n"
        ),
        "send_notification" => Some(
            "- `ACTION send_notification | 标题 | 内容`\n  发送 macOS 通知（原始 osascript）\n\n"
        ),
        "notify_user" => Some(
            "- `ACTION notify_user | 标题 | 内容`\n  发送 macOS 通知\n\n"
        ),
        "save_observation" => Some(
            "- `ACTION save_observation | 观察内容`\n  保存一条观察记录供后续管线消费\n\n"
        ),
        "record_suggestion_dedup" => Some(
            "- `ACTION record_suggestion_dedup | source_key | dedup_key | 内容`\n  记录建议（自动去重，同一 source+key 24h 内不重复）\n\n"
        ),
        "save_open_question" => Some(
            "- `ACTION save_open_question | 问题内容 | suggestion_id:N`\n  保存一个开放性问题（可选关联 suggestion_id）\n\n"
        ),
        "bump_question" => Some(
            "- `ACTION bump_question | question_id:N`\n  复现一个已有问题（增加 ask 计数）\n\n"
        ),
        "save_person_memory" => Some(
            "- `ACTION save_person_memory | 人名 | category | 观察内容 | confidence:0.0-1.0 | visibility:public/subconscious/private`\n  保存关于特定人物的记忆\n\n"
        ),
        "save_memory_integrated" => Some(
            "- `ACTION save_memory_integrated | category | 记忆内容 | confidence:0.0-1.0 | visibility:public/subconscious/private`\n  保存记忆（经过 MemoryIntegrator 去重仲裁）\n\n"
        ),
        "save_report" => Some(
            "- `ACTION save_report | report_type | 报告内容`\n  保存一份报告（morning/evening/weekly/week_start/mirror_weekly）\n\n"
        ),
        "save_calibration_rule" => Some(
            "- `ACTION save_calibration_rule | 规则内容 | confidence:0.0-1.0`\n  保存校准规则（同时写入 calibration 记忆 + negative_rules）\n\n"
        ),
        _ => None,
    }
}

// ─── 硬约束：参数验证 ──────────────────────────────────────────────

/// 验证 ACTION 参数合法性。返回 None = 合法，Some(reason) = 拒绝
pub fn validate_action_params(action: &str, parts: &[&str]) -> Option<String> {
    match action {
        "create_task" => validate_create_task(parts),
        "save_memory" => validate_save_memory(parts),
        "save_memory_visible" => validate_save_memory_visible(parts),
        "send_notification" | "notify_user" => validate_notification(parts),
        "save_observation" => validate_non_empty(parts, 1, "观察内容"),
        "record_suggestion_dedup" => validate_suggestion_dedup(parts),
        "save_open_question" => validate_non_empty(parts, 1, "问题内容"),
        "bump_question" => validate_bump_question(parts),
        "save_person_memory" => validate_person_memory(parts),
        "save_memory_integrated" => validate_save_memory_visible(parts),
        "save_report" => {
            if parts.get(1).map(|s| s.is_empty()).unwrap_or(true) {
                return Some("report_type 为空".into());
            }
            if parts.get(2).map(|s| s.is_empty()).unwrap_or(true) {
                return Some("报告内容为空".into());
            }
            None
        }
        "save_calibration_rule" => validate_non_empty(parts, 1, "规则内容"),
        _ => Some(format!("未知 action: {action}")),
    }
}

fn validate_create_task(parts: &[&str]) -> Option<String> {
    if parts.get(1).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("任务内容为空".into());
    }
    if let Some(p) = parts.get(2).and_then(|s| s.strip_prefix("priority:")) {
        if !["normal", "P0", "P1", "P2"].contains(&p) {
            return Some(format!("非法 priority: {p}"));
        }
    }
    if let Some(d) = parts.get(3).and_then(|s| s.strip_prefix("due:")) {
        if !d.is_empty() && d.len() != 10 {
            return Some(format!("due_date 格式错误: {d}（需要 YYYY-MM-DD）"));
        }
    }
    None
}

fn validate_save_memory(parts: &[&str]) -> Option<String> {
    if parts.get(2).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("记忆内容为空".into());
    }
    validate_confidence(parts, 3)
}

fn validate_save_memory_visible(parts: &[&str]) -> Option<String> {
    if parts.get(2).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("记忆内容为空".into());
    }
    if let Some(v) = parts.get(4).and_then(|s| s.strip_prefix("visibility:")) {
        if !["public", "subconscious", "private"].contains(&v) {
            return Some(format!("非法 visibility: {v}（需要 public/subconscious/private）"));
        }
    }
    validate_confidence(parts, 3)
}

fn validate_notification(parts: &[&str]) -> Option<String> {
    if parts.get(1).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("通知标题为空".into());
    }
    None
}

fn validate_suggestion_dedup(parts: &[&str]) -> Option<String> {
    for (idx, label) in [(1, "source_key"), (2, "dedup_key"), (3, "内容")] {
        if parts.get(idx).map(|s| s.is_empty()).unwrap_or(true) {
            return Some(format!("{label}为空"));
        }
    }
    None
}

fn validate_bump_question(parts: &[&str]) -> Option<String> {
    let raw = parts.get(1).and_then(|s| s.strip_prefix("question_id:")).unwrap_or("");
    if raw.parse::<i64>().is_err() {
        return Some(format!("question_id 格式错误: {raw}"));
    }
    None
}

fn validate_person_memory(parts: &[&str]) -> Option<String> {
    if parts.get(1).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("人名为空".into());
    }
    if parts.get(3).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("观察内容为空".into());
    }
    if let Some(v) = parts.get(5).and_then(|s| s.strip_prefix("visibility:")) {
        if !["public", "subconscious", "private"].contains(&v) {
            return Some(format!("非法 visibility: {v}"));
        }
    }
    validate_confidence(parts, 4)
}

fn validate_confidence(parts: &[&str], idx: usize) -> Option<String> {
    if let Some(c) = parts.get(idx).and_then(|s| s.strip_prefix("confidence:")) {
        if let Ok(v) = c.parse::<f64>() {
            if !(0.0..=1.0).contains(&v) {
                return Some(format!("confidence 超出范围: {v}（需要 0.0-1.0）"));
            }
        }
    }
    None
}

fn validate_non_empty(parts: &[&str], idx: usize, label: &str) -> Option<String> {
    if parts.get(idx).map(|s| s.is_empty()).unwrap_or(true) {
        return Some(format!("{label}为空"));
    }
    None
}

// ─── ACTION 执行分发 ──────────────────────────────────────────────────

/// 从 LLM 输出中解析、验证并执行 ACTION 命令（带 rate limit）
pub fn execute_actions(
    text: &str, actions: &[String], store: &Store,
    stage_name: &str, max_actions: usize,
) -> ActionResult {
    let mut result = ActionResult { count: 0, results: Vec::new() };

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("ACTION ") { continue; }
        if result.count >= max_actions {
            warn!("Stage {stage_name}: rate limit reached ({max_actions}), skipping remaining ACTIONs");
            break;
        }

        let parts: Vec<&str> = line[7..].splitn(6, '|').map(|s| s.trim()).collect();
        if parts.is_empty() { continue; }

        let action_name = parts[0];

        // 硬约束：授权检查
        if !actions.contains(&action_name.to_string()) {
            warn!("Stage {stage_name}: BLOCKED unauthorized action '{action_name}'");
            continue;
        }

        // 硬约束：参数验证
        if let Some(reason) = validate_action_params(action_name, &parts) {
            warn!("Stage {stage_name}: BLOCKED invalid {action_name}: {reason}");
            continue;
        }

        if let Some(id) = dispatch_action(action_name, &parts, store, stage_name) {
            result.results.push((action_name.to_string(), id));
            result.count += 1;
        }
    }
    result
}

/// 分发单个 action 到对应的 store 方法。返回 Some(id) 表示成功
fn dispatch_action(action: &str, parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    match action {
        "create_task" => handle_create_task(parts, store, stage),
        "save_memory" => handle_save_memory(parts, store, stage),
        "save_memory_visible" => handle_save_memory_visible(parts, store, stage),
        "send_notification" => handle_send_notification(parts, stage),
        "notify_user" => handle_notify_user(parts, stage),
        "save_observation" => handle_save_observation(parts, store, stage),
        "record_suggestion_dedup" => handle_suggestion_dedup(parts, store, stage),
        "save_open_question" => handle_save_open_question(parts, store, stage),
        "bump_question" => handle_bump_question(parts, store, stage),
        "save_person_memory" => handle_save_person_memory(parts, store, stage),
        "save_memory_integrated" => handle_save_memory_integrated(parts, store, stage),
        "save_report" => handle_save_report(parts, store, stage),
        "save_calibration_rule" => handle_save_calibration_rule(parts, store, stage),
        _ => None,
    }
}

// ─── 各 Action handler ──────────────────────────────────────────────

fn handle_create_task(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let content = parts[1];
    let priority = parts.get(2).and_then(|p| p.strip_prefix("priority:")).unwrap_or("normal");
    let due = parts.get(3).and_then(|d| d.strip_prefix("due:")).filter(|d| !d.is_empty());
    match store.create_task(content, &format!("stage:{stage}"), None, Some(priority), due, None) {
        Ok(id) => { info!("Stage {stage}: ✓ created task #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: create_task failed: {e}"); None }
    }
}

fn handle_save_memory(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let category = parts.get(1).unwrap_or(&"insight");
    let content = parts[2];
    let confidence = parse_confidence(parts, 3);
    match store.save_memory(category, content, &format!("stage:{stage}"), confidence) {
        Ok(id) => { info!("Stage {stage}: ✓ saved memory [{category}]"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_memory failed: {e}"); None }
    }
}

fn handle_save_memory_visible(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let category = parts.get(1).unwrap_or(&"insight");
    let content = parts[2];
    let confidence = parse_confidence(parts, 3);
    let visibility = parts.get(4).and_then(|s| s.strip_prefix("visibility:")).unwrap_or("public");
    match store.save_memory_with_visibility(
        category, content, &format!("stage:{stage}"), confidence, visibility,
    ) {
        Ok(id) => { info!("Stage {stage}: ✓ saved memory [{category}] vis={visibility}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_memory_visible failed: {e}"); None }
    }
}

fn handle_send_notification(parts: &[&str], stage: &str) -> Option<i64> {
    let title = parts[1];
    let body = parts.get(2).unwrap_or(&"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        body.replace('"', "\\\""), title.replace('"', "\\\""),
    );
    let _ = std::process::Command::new("osascript").arg("-e").arg(&script).output();
    info!("Stage {stage}: ✓ sent notification");
    Some(0)
}

fn handle_notify_user(parts: &[&str], stage: &str) -> Option<i64> {
    let title = parts[1];
    let body = parts.get(2).unwrap_or(&"");
    // 使用 applescript::notify 的同步 fallback（和 send_notification 一样的 osascript）
    // 实际的 async 调用在 UserDefinedStage::run 中通过 post_notifications 处理
    // 这里先收集通知请求，返回 sentinel
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        body.replace('"', "\\\""), title.replace('"', "\\\""),
    );
    let _ = std::process::Command::new("osascript").arg("-e").arg(&script).output();
    info!("Stage {stage}: ✓ notified user: {title}");
    Some(0)
}

fn handle_save_observation(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let content = parts[1];
    let category = format!("custom_{stage}");
    match store.record_observation(&category, content, None) {
        Ok(_) => { info!("Stage {stage}: ✓ saved observation"); Some(0) }
        Err(e) => { warn!("Stage {stage}: save_observation failed: {e}"); None }
    }
}

fn handle_suggestion_dedup(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let source = parts[1];
    let key = parts[2];
    let content = parts[3];
    // 去重检查
    if store.has_recent_suggestion(source, key) {
        info!("Stage {stage}: skipped suggestion (dedup: {source}/{key})");
        return Some(0);
    }
    match store.record_suggestion(source, key, content) {
        Ok(id) => { info!("Stage {stage}: ✓ recorded suggestion #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: record_suggestion failed: {e}"); None }
    }
}

fn handle_save_open_question(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let text = parts[1];
    let sid = parts.get(2)
        .and_then(|s| s.strip_prefix("suggestion_id:"))
        .and_then(|s| s.parse::<i64>().ok());
    match store.save_open_question(text, sid) {
        Ok(id) => { info!("Stage {stage}: ✓ saved open question #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_open_question failed: {e}"); None }
    }
}

fn handle_bump_question(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].strip_prefix("question_id:").unwrap_or("0").parse().unwrap_or(0);
    match store.bump_question_ask(id) {
        Ok(()) => { info!("Stage {stage}: ✓ bumped question #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: bump_question failed: {e}"); None }
    }
}

fn handle_save_person_memory(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let name = parts[1];
    let category = parts.get(2).unwrap_or(&"behavior");
    let content = parts[3];
    let confidence = parse_confidence(parts, 4);
    let visibility = parts.get(5).and_then(|s| s.strip_prefix("visibility:")).unwrap_or("private");
    match store.save_memory_about_person(
        category, content, &format!("stage:{stage}"), confidence, visibility, name,
    ) {
        Ok(id) => { info!("Stage {stage}: ✓ saved person memory [{name}]"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_person_memory failed: {e}"); None }
    }
}

fn handle_save_memory_integrated(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let category = parts.get(1).unwrap_or(&"insight");
    let content = parts[2];
    let confidence = parse_confidence(parts, 3);
    let visibility = parts.get(4).and_then(|s| s.strip_prefix("visibility:")).unwrap_or("public");
    // TODO: 未来升级为真正的 MemoryIntegrator 去重仲裁（需要 async + LlmProvider）
    // 当前 fallback：直接调用 save_memory_with_visibility，Store 层的 text_similarity > 0.6 提供基础去重
    match store.save_memory_with_visibility(
        category, content, &format!("stage:{stage}"), confidence, visibility,
    ) {
        Ok(id) => { info!("Stage {stage}: ✓ saved integrated memory [{category}]"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_memory_integrated failed: {e}"); None }
    }
}

fn handle_save_report(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let report_type = parts[1];
    let content = parts[2];
    match store.save_report(report_type, content) {
        Ok(id) => { info!("Stage {stage}: ✓ saved report [{report_type}]"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_report failed: {e}"); None }
    }
}

fn handle_save_calibration_rule(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let content = parts[1];
    let confidence = parse_confidence(parts, 2);
    match store.save_memory("calibration", content, &format!("stage:{stage}"), confidence) {
        Ok(id) => {
            // 同步写入 negative_rules
            if let Err(e) = store.append_negative_rule(content) {
                warn!("Stage {stage}: append_negative_rule failed: {e}");
            }
            info!("Stage {stage}: ✓ saved calibration rule");
            Some(id)
        }
        Err(e) => { warn!("Stage {stage}: save_calibration_rule failed: {e}"); None }
    }
}

fn parse_confidence(parts: &[&str], idx: usize) -> f64 {
    parts.get(idx)
        .and_then(|c| c.strip_prefix("confidence:"))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0.7)
}

// ─── 公共约束层（供 pipeline 外模块调用）─────────────────────────────

/// 受约束的单条 ACTION 执行（不依赖完整 stage 上下文）
/// action_line 格式："create_task | content | priority:P1 | due:2026-04-01"（不含 "ACTION " 前缀）
pub fn execute_single_action(
    action_line: &str, whitelist: &[&str], store: &Store, caller: &str,
) -> Option<i64> {
    let parts: Vec<&str> = action_line.splitn(6, '|').map(|s| s.trim()).collect();
    if parts.is_empty() { return None; }
    let action_name = parts[0];
    if !whitelist.contains(&action_name) {
        warn!("{caller}: BLOCKED unauthorized action '{action_name}'");
        return None;
    }
    if let Some(reason) = validate_action_params(action_name, &parts) {
        warn!("{caller}: BLOCKED invalid {action_name}: {reason}");
        return None;
    }
    dispatch_action(action_name, &parts, store, caller)
}

/// 受约束的批量执行（带 rate limit）
pub fn execute_constrained_actions(
    action_lines: &[String], whitelist: &[&str], store: &Store,
    caller: &str, max_actions: usize,
) -> Vec<Option<i64>> {
    let mut results = Vec::new();
    for line in action_lines {
        if results.len() >= max_actions {
            warn!("{caller}: rate limit reached ({max_actions}), skipping remaining");
            break;
        }
        results.push(execute_single_action(line, whitelist, store, caller));
    }
    results
}

// ─── 输入过滤 ──────────────────────────────────────────────────────

/// 按 allowed_inputs 声明过滤上下文（硬约束：不在列表中的数据源不传给 LLM）
pub fn load_filtered_context(store: &Store, allowed: &[String]) -> String {
    let mut ctx = String::new();
    for input in allowed {
        match input.as_str() {
            "observer_notes" => {
                for n in store.get_today_observer_notes().unwrap_or_default().iter().take(15) {
                    ctx.push_str(&format!("- [观察] {n}\n"));
                }
            }
            "coach_insights" => {
                for i in store.get_today_coach_insights().unwrap_or_default().iter().take(10) {
                    ctx.push_str(&format!("- [洞察] {i}\n"));
                }
            }
            "emails" => {
                for e in store.get_today_email_summaries(15).unwrap_or_default() {
                    ctx.push_str(&format!("- [邮件] {e}\n"));
                }
            }
            "messages" => {
                for m in store.get_today_message_summaries(20).unwrap_or_default() {
                    ctx.push_str(&format!("- [消息] {m}\n"));
                }
            }
            "memories" => {
                let since = (chrono::Local::now() - chrono::Duration::days(3)).to_rfc3339();
                for m in store.get_memories_since(&since).unwrap_or_default().iter().take(15) {
                    ctx.push_str(&format!("- [记忆] [{}] {}\n", m.category, m.content));
                }
            }
            "raw_observations" => {
                for obs in store.load_unprocessed_observations(50).unwrap_or_default() {
                    ctx.push_str(&format!("- [raw] [{}] {}\n", obs.category, obs.observation));
                }
            }
            "corrections" => {
                for rtype in ["morning", "evening", "weekly"] {
                    for c in store.get_corrections_for_pattern(rtype).unwrap_or_default() {
                        ctx.push_str(&format!("- [纠正/{rtype}] {} → {}\n", c.wrong_claim, c.correct_fact));
                    }
                }
            }
            // 近期历史观察（已处理），供需要回看过去行为的 stage 使用
            "recent_observations" => {
                for (cat, obs) in store.load_recent_observations(200).unwrap_or_default().iter().take(100) {
                    ctx.push_str(&format!("- [历史:{cat}] {obs}\n"));
                }
            }
            // 本周信号：最近 7 天的 suggestions（供周度镜像汇总）
            "weekly_signals" => {
                for s in store.get_recent_suggestions(20).unwrap_or_default().iter().take(20) {
                    ctx.push_str(&format!("- [信号] {}\n", s.response));
                }
            }
            _ => {}
        }
    }
    ctx
}

/// 执行 pre_condition SQL（返回 >0 才通过）
pub fn check_pre_condition(store: &Store, sql: &str) -> bool {
    if sql.is_empty() { return true; }
    let lower = sql.trim().to_lowercase();
    if !lower.starts_with("select") {
        warn!("Pre-condition rejected: only SELECT allowed, got: {}", &sql[..sql.len().min(30)]);
        return false;
    }
    store.execute_condition_query(sql).unwrap_or(false)
}
