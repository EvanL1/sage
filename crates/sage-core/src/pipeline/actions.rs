//! ACTION 约束系统 — 文档生成、参数验证、执行分发
//!
//! 所有 UserDefinedStage（包括预设和自定义）的 ACTION 都通过这个模块处理。
//! 硬约束：授权白名单 + 参数验证 + rate limit
//! 软约束：action_docs 注入到 prompt 中

use std::path::PathBuf;
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
        // ── Evolution ACTION 类型 ──────────────────────────────────
        "dedup_memory" => Some(
            "- `ACTION dedup_memory | memory_id | reason`\n  归档重复记忆\n\n"
        ),
        "compile_memories" => Some(
            "- `ACTION compile_memories | source_ids | content | category | confidence:0.0-1.0`\n  合并多条记忆为一条（source_ids 逗号分隔）\n\n"
        ),
        "condense_memory" => Some(
            "- `ACTION condense_memory | memory_id | new_content`\n  精简冗长记忆内容\n\n"
        ),
        "link_memories" => Some(
            "- `ACTION link_memories | id1 | id2 | relation | weight:0.0-1.0`\n  创建记忆关系边（relation: causes/supports/contradicts/co_occurred/derived_from/similar）\n\n"
        ),
        "promote_memory" => Some(
            "- `ACTION promote_memory | memory_id | new_depth`\n  提升记忆深度（episodic/semantic/procedural/axiom）\n\n"
        ),
        "decay_memory" => Some(
            "- `ACTION decay_memory | memory_id | reason`\n  归档过期记忆\n\n"
        ),
        // ── Meta ACTION 类型 ──────────────────────────────────────
        "set_pipeline_override" => Some(
            "- `ACTION set_pipeline_override | stage_name | key | value | reason`\n  调整 pipeline 参数（不可禁用 evolution 核心阶段）\n\n"
        ),
        "rewrite_prompt" => Some(
            "- `ACTION rewrite_prompt | prompt_name | new_content`\n  重写 prompt 文件（备份原文件为 .bak）\n\n"
        ),
        "save_custom_page" => Some(
            "- `ACTION save_custom_page | title | content`\n  生成 UI 自定义页面\n\n"
        ),
        // ── Verifier / Integrator ACTION 类型 ────────────────────
        "verify_confirm" => Some(
            "- `ACTION verify_confirm | memory_id | evidence_summary`\n  确认记忆（增加验证次数，微调 confidence +0.02）\n\n"
        ),
        "verify_challenge" => Some(
            "- `ACTION verify_challenge | memory_id | counter_evidence`\n  质疑记忆（降低 confidence -0.05，记录反例）\n\n"
        ),
        "flag_contradiction" => Some(
            "- `ACTION flag_contradiction | memory_id_1 | memory_id_2 | explanation`\n  标记两条记忆互相矛盾（写入 contradicts 图边）\n\n"
        ),
        "demote_memory" => Some(
            "- `ACTION demote_memory | memory_id | new_depth | reason`\n  降级记忆（必须逐级降）\n\n"
        ),
        "archive_challenged" => Some(
            "- `ACTION archive_challenged | memory_id | reason`\n  归档反复被挑战的认知\n\n"
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
        // ── Evolution 验证 ────────────────────────────────────────
        "dedup_memory" => validate_memory_id_reason(parts),
        "compile_memories" => validate_compile_memories(parts),
        "condense_memory" => validate_condense_memory(parts),
        "link_memories" => validate_link_memories(parts),
        "promote_memory" => validate_promote_memory(parts),
        "decay_memory" => validate_memory_id_reason(parts),
        // ── Meta 验证 ─────────────────────────────────────────────
        "set_pipeline_override" => validate_pipeline_override(parts),
        "rewrite_prompt" => validate_rewrite_prompt(parts),
        "save_custom_page" => validate_custom_page(parts),
        // ── Verifier / Integrator 验证 ────────────────────────────
        "verify_confirm" => validate_memory_id_optional_str(parts),
        "verify_challenge" => validate_memory_id_optional_str(parts),
        "flag_contradiction" => validate_flag_contradiction(parts),
        "demote_memory" => validate_demote_memory(parts),
        "archive_challenged" => validate_memory_id_optional_str(parts),
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

fn validate_memory_id_reason(parts: &[&str]) -> Option<String> {
    let id_str = parts.get(1).unwrap_or(&"");
    if id_str.parse::<i64>().is_err() {
        return Some(format!("memory_id 格式错误: {id_str}"));
    }
    None
}

fn validate_compile_memories(parts: &[&str]) -> Option<String> {
    if parts.get(1).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("source_ids 为空".into());
    }
    if parts.get(2).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("content 为空".into());
    }
    validate_confidence(parts, 4)
}

fn validate_condense_memory(parts: &[&str]) -> Option<String> {
    // 复用 memory_id 校验，再追加 new_content 非空检查
    validate_memory_id_reason(parts).or_else(|| validate_non_empty(parts, 2, "new_content"))
}

fn validate_link_memories(parts: &[&str]) -> Option<String> {
    for (idx, label) in [(1i64, "id1"), (2, "id2")] {
        let s = parts.get(idx as usize).unwrap_or(&"");
        if s.parse::<i64>().is_err() {
            return Some(format!("{label} 格式错误: {s}"));
        }
    }
    let relation = parts.get(3).unwrap_or(&"");
    const VALID: &[&str] = &["causes", "supports", "contradicts", "co_occurred", "derived_from", "similar"];
    if !VALID.contains(relation) {
        return Some(format!("非法 relation: {relation}"));
    }
    if let Some(w) = parts.get(4).and_then(|s| s.strip_prefix("weight:")) {
        if let Ok(v) = w.parse::<f64>() {
            if !(0.0..=1.0).contains(&v) {
                return Some(format!("weight 超出范围: {v}"));
            }
        }
    }
    None
}

fn validate_promote_memory(parts: &[&str]) -> Option<String> {
    let id_str = parts.get(1).unwrap_or(&"");
    if id_str.parse::<i64>().is_err() {
        return Some(format!("memory_id 格式错误: {id_str}"));
    }
    let depth = parts.get(2).unwrap_or(&"");
    const DEPTHS: &[&str] = &["episodic", "semantic", "procedural", "axiom"];
    if !DEPTHS.contains(depth) {
        return Some(format!("非法 depth: {depth}（需要 episodic/semantic/procedural/axiom）"));
    }
    None
}

fn validate_pipeline_override(parts: &[&str]) -> Option<String> {
    let stage = parts.get(1).unwrap_or(&"");
    if stage.is_empty() {
        return Some("stage_name 为空".into());
    }
    if !stage.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Some(format!("stage_name 含非法字符: {stage}"));
    }
    if parts.get(2).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("key 为空".into());
    }
    if parts.get(3).map(|s| s.is_empty()).unwrap_or(true) {
        return Some("value 为空".into());
    }
    // 禁止禁用 evolution 核心阶段
    let key = parts.get(2).unwrap_or(&"");
    let value = parts.get(3).unwrap_or(&"");
    if *key == "enabled" && *value == "false" && *stage == "evolution" {
        return Some("禁止禁用 evolution 核心阶段".into());
    }
    None
}

/// 校验 parts[1] 非空（name_label）且 parts[2] 非空且 <10000 字节（content_label）
fn validate_name_content(parts: &[&str], name_label: &str, content_label: &str) -> Option<String> {
    if parts.get(1).map(|s| s.is_empty()).unwrap_or(true) {
        return Some(format!("{name_label}为空"));
    }
    let content = parts.get(2).unwrap_or(&"");
    if content.is_empty() {
        return Some(format!("{content_label}为空"));
    }
    if content.len() >= 10000 {
        return Some(format!("{content_label}超出长度限制（{}≥10000）", content.len()));
    }
    None
}

fn validate_rewrite_prompt(parts: &[&str]) -> Option<String> {
    validate_name_content(parts, "prompt_name ", "new_content ")
}

fn validate_custom_page(parts: &[&str]) -> Option<String> {
    validate_name_content(parts, "title ", "content ")
}

/// 验证 parts[1] 是合法 i64 memory_id（parts[2..] 可选）
fn validate_memory_id_optional_str(parts: &[&str]) -> Option<String> {
    let id_str = parts.get(1).unwrap_or(&"");
    if id_str.parse::<i64>().is_err() {
        return Some(format!("memory_id 格式错误: {id_str}"));
    }
    None
}

fn validate_flag_contradiction(parts: &[&str]) -> Option<String> {
    for (idx, label) in [(1usize, "memory_id_1"), (2, "memory_id_2")] {
        let s = parts.get(idx).unwrap_or(&"");
        if s.parse::<i64>().is_err() {
            return Some(format!("{label} 格式错误: {s}"));
        }
    }
    None
}

fn validate_demote_memory(parts: &[&str]) -> Option<String> {
    let id_str = parts.get(1).unwrap_or(&"");
    if id_str.parse::<i64>().is_err() {
        return Some(format!("memory_id 格式错误: {id_str}"));
    }
    let depth = parts.get(2).unwrap_or(&"");
    const DEPTHS: &[&str] = &["episodic", "semantic", "procedural", "axiom"];
    if !DEPTHS.contains(depth) {
        return Some(format!("非法 depth: {depth}（需要 episodic/semantic/procedural/axiom）"));
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
        // ── Evolution handlers ────────────────────────────────────
        "dedup_memory" => handle_dedup_memory(parts, store, stage),
        "compile_memories" => handle_compile_memories(parts, store, stage),
        "condense_memory" => handle_condense_memory(parts, store, stage),
        "link_memories" => handle_link_memories(parts, store, stage),
        "promote_memory" => handle_promote_memory(parts, store, stage),
        "decay_memory" => handle_decay_memory(parts, store, stage),
        // ── Meta handlers ─────────────────────────────────────────
        "set_pipeline_override" => handle_pipeline_override(parts, store, stage),
        "rewrite_prompt" => handle_rewrite_prompt(parts, store, stage),
        "save_custom_page" => handle_custom_page(parts, store, stage),
        // ── Verifier / Integrator handlers ────────────────────────
        "verify_confirm" => handle_verify_confirm(parts, store, stage),
        "verify_challenge" => handle_verify_challenge(parts, store, stage),
        "flag_contradiction" => handle_flag_contradiction(parts, store, stage),
        "demote_memory" => handle_demote_memory(parts, store, stage),
        "archive_challenged" => handle_archive_challenged(parts, store, stage),
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
    // observer 阶段生成的记忆是原始观察的直接重述，标记为可重新推导（derivable=1）
    let is_observer = stage == "observer";
    match store.save_memory_with_visibility_derivable(
        category, content, &format!("stage:{stage}"), confidence, visibility, is_observer,
    ) {
        Ok(id) => { info!("Stage {stage}: ✓ saved memory [{category}] vis={visibility} derivable={is_observer}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_memory_visible failed: {e}"); None }
    }
}

/// 通用 osascript 通知：发送系统通知并记录日志
fn send_osascript_notification(title: &str, body: &str, stage: &str, log_msg: &str) -> Option<i64> {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        body.replace('"', "\\\""), title.replace('"', "\\\""),
    );
    let _ = std::process::Command::new("osascript").arg("-e").arg(&script).output();
    info!("Stage {stage}: {log_msg}");
    Some(0)
}

fn handle_send_notification(parts: &[&str], stage: &str) -> Option<i64> {
    let title = parts[1];
    let body = parts.get(2).unwrap_or(&"");
    send_osascript_notification(title, body, stage, "✓ sent notification")
}

fn handle_notify_user(parts: &[&str], stage: &str) -> Option<i64> {
    let title = parts[1];
    let body = parts.get(2).unwrap_or(&"");
    // 使用 osascript 同步发送通知（和 send_notification 共享同一实现）
    send_osascript_notification(title, body, stage, &format!("✓ notified user: {title}"))
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

// ─── Evolution handlers ──────────────────────────────────────────────

fn handle_dedup_memory(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    let reason = parts.get(2).unwrap_or(&"重复记忆");
    match store.archive_memory(id, reason) {
        Ok(()) => { info!("Stage {stage}: ✓ dedup_memory #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: dedup_memory failed: {e}"); None }
    }
}

fn handle_compile_memories(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let source_ids_str = parts[1];
    let content = parts[2];
    let category = parts.get(3).unwrap_or(&"insight");
    let confidence = parse_confidence(parts, 4);
    // 创建合并后的新记忆
    let new_id = match store.save_memory(category, content, &format!("stage:{stage}"), confidence) {
        Ok(id) => id,
        Err(e) => { warn!("Stage {stage}: compile_memories save failed: {e}"); return None; }
    };
    // 归档各来源记忆
    for id_str in source_ids_str.split(',') {
        let id_str = id_str.trim();
        if let Ok(sid) = id_str.parse::<i64>() {
            if let Err(e) = store.archive_memory(sid, "compiled") {
                warn!("Stage {stage}: compile_memories archive #{sid} failed: {e}");
            }
        }
    }
    info!("Stage {stage}: ✓ compile_memories → #{new_id}");
    Some(new_id)
}

fn handle_condense_memory(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    let new_content = parts[2];
    match store.update_memory_content(id, new_content) {
        Ok(()) => { info!("Stage {stage}: ✓ condense_memory #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: condense_memory failed: {e}"); None }
    }
}

fn handle_link_memories(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id1: i64 = parts[1].parse().ok()?;
    let id2: i64 = parts[2].parse().ok()?;
    let relation = parts[3];
    let weight = parts.get(4)
        .and_then(|s| s.strip_prefix("weight:"))
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    match store.save_memory_edge(id1, id2, relation, weight) {
        Ok(id) => { info!("Stage {stage}: ✓ link_memories {id1}→{id2} [{relation}]"); Some(id) }
        Err(e) => { warn!("Stage {stage}: link_memories failed: {e}"); None }
    }
}

fn handle_promote_memory(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    let target_depth = parts[2];

    // 读取当前记忆状态做门控
    if let Ok(memories) = store.get_memories_since("2000-01-01") {
        if let Some(m) = memories.iter().find(|m| m.id == id) {
            // 门控 1：不能跳级（必须逐级提升）
            let current = m.depth.as_str();
            let valid_next = match current {
                "episodic" => "semantic",
                "semantic" => "procedural",
                "procedural" => "axiom",
                _ => { warn!("Stage {stage}: promote #{id} blocked — current depth '{current}' cannot promote"); return None; }
            };
            if target_depth != valid_next {
                warn!("Stage {stage}: promote #{id} blocked — must promote {current}→{valid_next}, not {current}→{target_depth}");
                return None;
            }

            // 门控 2：→ axiom 需要 validation_count ≥ 10 且 confidence ≥ 0.9
            if target_depth == "axiom" {
                if m.validation_count < 10 {
                    warn!("Stage {stage}: promote #{id} → axiom blocked — validation_count {} < 10", m.validation_count);
                    return None;
                }
                if m.confidence < 0.9 {
                    warn!("Stage {stage}: promote #{id} → axiom blocked — confidence {:.2} < 0.9", m.confidence);
                    return None;
                }
                // 门控 3：calibration / decision / report_insight 类别不能成为 axiom
                let banned = ["calibration", "calibration_task", "decision", "report_insight", "session"];
                if banned.contains(&m.category.as_str()) {
                    warn!("Stage {stage}: promote #{id} → axiom blocked — category '{}' is not axiom-eligible", m.category);
                    return None;
                }
            }

            // 门控 4：→ procedural 需要 validation_count ≥ 5
            if target_depth == "procedural" && m.validation_count < 5 {
                warn!("Stage {stage}: promote #{id} → procedural blocked — validation_count {} < 5", m.validation_count);
                return None;
            }
        }
    }

    match store.update_memory_depth(id, target_depth) {
        Ok(()) => { info!("Stage {stage}: ✓ promote_memory #{id} → {target_depth}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: promote_memory failed: {e}"); None }
    }
}

fn handle_decay_memory(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    let reason = parts.get(2).unwrap_or(&"过期衰减");
    match store.archive_memory(id, reason) {
        Ok(()) => { info!("Stage {stage}: ✓ decay_memory #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: decay_memory failed: {e}"); None }
    }
}

// ─── Meta handlers ────────────────────────────────────────────────────

fn handle_pipeline_override(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let stage_name = parts[1];
    let key = parts[2];
    let value = parts[3];
    let reason = parts.get(4).unwrap_or(&"");
    match store.set_pipeline_override(stage_name, key, value, reason) {
        Ok(()) => { info!("Stage {stage}: ✓ set_pipeline_override {stage_name}.{key}={value}"); Some(0) }
        Err(e) => { warn!("Stage {stage}: set_pipeline_override failed: {e}"); None }
    }
}

fn handle_rewrite_prompt(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let name = parts[1];
    let content = parts[2];
    let lang = store.prompt_lang();
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    let dir = PathBuf::from(&home).join(".sage").join("prompts").join(&lang);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("Stage {stage}: rewrite_prompt create_dir failed: {e}");
        return None;
    }
    let path = dir.join(format!("{name}.md"));
    // 备份原文件
    if path.exists() {
        let bak = dir.join(format!("{name}.md.bak"));
        let _ = std::fs::copy(&path, &bak);
    }
    match std::fs::write(&path, content) {
        Ok(()) => { info!("Stage {stage}: ✓ rewrite_prompt {lang}/{name}.md"); Some(0) }
        Err(e) => { warn!("Stage {stage}: rewrite_prompt write failed: {e}"); None }
    }
}

fn handle_custom_page(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let title = parts[1];
    let content = parts[2];
    match store.save_custom_page(title, content) {
        Ok(id) => { info!("Stage {stage}: ✓ save_custom_page [{title}] #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: save_custom_page failed: {e}"); None }
    }
}

// ─── Verifier / Integrator handlers ──────────────────────────────────────────

fn handle_verify_confirm(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    match store.verify_memory(id) {
        Ok(()) => { info!("Stage {stage}: ✓ verify_confirm #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: verify_confirm #{id} failed: {e}"); None }
    }
}

fn handle_verify_challenge(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    let counter_evidence = parts.get(2).unwrap_or(&"");
    match store.challenge_memory(id, counter_evidence) {
        Ok(()) => { info!("Stage {stage}: ⚠ verify_challenge #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: verify_challenge #{id} failed: {e}"); None }
    }
}

fn handle_flag_contradiction(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id1: i64 = parts[1].parse().ok()?;
    let id2: i64 = parts[2].parse().ok()?;
    let explanation = parts.get(3).unwrap_or(&"");
    // 写入 contradicts 图边
    if let Err(e) = store.save_memory_edge(id1, id2, "contradicts", 0.8) {
        warn!("Stage {stage}: flag_contradiction edge failed: {e}");
    }
    // 记录为 observation 供 integrator 读取
    let note = format!("矛盾: #{id1} vs #{id2} — {explanation}");
    match store.record_observation(&format!("custom_{stage}"), &note, None) {
        Ok(()) => { info!("Stage {stage}: ✓ flag_contradiction #{id1} vs #{id2}"); Some(id1) }
        Err(e) => { warn!("Stage {stage}: flag_contradiction obs failed: {e}"); None }
    }
}

fn handle_demote_memory(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    let new_depth = parts[2];
    let reason = parts.get(3).unwrap_or(&"integrator demotion");

    // 读取当前 depth 做门控（必须逐级降）
    let current_depth = store.get_memory_by_id(id).ok()??. depth;
    let valid = matches!(
        (current_depth.as_str(), new_depth),
        ("axiom", "procedural") | ("procedural", "semantic") | ("semantic", "episodic")
    );
    if !valid {
        warn!("Stage {stage}: demote #{id} blocked — {current_depth}→{new_depth} not a valid demotion");
        return None;
    }

    if let Err(e) = store.update_memory_depth(id, new_depth) {
        warn!("Stage {stage}: demote #{id} depth update failed: {e}");
        return None;
    }
    if let Err(e) = store.set_evolution_note(id, &format!("降级: {reason}")) {
        warn!("Stage {stage}: demote #{id} note failed: {e}");
    }
    info!("Stage {stage}: ✓ demote #{id} {current_depth}→{new_depth}");
    Some(id)
}

fn handle_archive_challenged(parts: &[&str], store: &Store, stage: &str) -> Option<i64> {
    let id: i64 = parts[1].parse().ok()?;
    let reason = parts.get(2).unwrap_or(&"反复被挑战");
    match store.archive_memory(id, reason) {
        Ok(()) => { info!("Stage {stage}: ✓ archive_challenged #{id}"); Some(id) }
        Err(e) => { warn!("Stage {stage}: archive_challenged #{id} failed: {e}"); None }
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
                    ctx.push_str(&format!("- [记忆:id={}] [{}] {}\n", m.id, m.category, m.content));
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
            // 相似记忆聚类（供 evolution_transform）— 小批量轮转，最旧优先
            "similar_memories" => {
                let mems = store.load_oldest_episodic_batch(15).unwrap_or_default();
                for m in &mems {
                    ctx.push_str(&format!("- [记忆:id={}] {}\n", m.id, m.content));
                }
                // 标记已访问，下次轮转到其他记忆
                for m in &mems {
                    let _ = store.touch_memory(m.id);
                }
            }
            // 冗长记忆（供 evolution_transform）— 小批量轮转
            "verbose_memories" => {
                let mems = store.load_oldest_verbose_batch(10).unwrap_or_default();
                for m in &mems {
                    ctx.push_str(&format!("- [记忆:id={},len={}] {}\n", m.id, m.content.chars().count(), m.content));
                }
                for m in &mems {
                    let _ = store.touch_memory(m.id);
                }
            }
            // 管线执行统计（供 meta_params）
            "pipeline_stats" => {
                for (name, ok, empty, error) in store.get_pipeline_summary(14).unwrap_or_default() {
                    ctx.push_str(&format!("- [stage:{name}] ok={ok} empty={empty} error={error}\n"));
                }
            }
            // 校准规则（供 meta_prompts）
            "calibration_rules" => {
                let cats = ["calibration", "calibration_task"];
                for cat in cats {
                    for m in store.get_memories_by_category(cat).unwrap_or_default() {
                        ctx.push_str(&format!("- [规则] {}\n", m.content));
                    }
                }
            }
            // 待验证认知：semantic/procedural/axiom，按最后访问时间 ASC（轮换）（供 verifier）
            "verifiable_memories" => {
                let eligible_cats = "'thinking','behavior','personality','values','growth','communication','emotion','coach_insight','strategy_insight'";
                let sql = format!(
                    "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
                     about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
                     FROM memories \
                     WHERE status = 'active' \
                     AND depth IN ('semantic', 'procedural', 'axiom') \
                     AND category IN ({eligible_cats}) \
                     AND created_at < datetime('now', '-7 days') \
                     ORDER BY last_accessed_at ASC NULLS FIRST, validation_count ASC \
                     LIMIT 10"
                );
                if let Ok(mems) = store.query_memories_by_raw(&sql) {
                    for m in mems {
                        ctx.push_str(&format!(
                            "- [验证:id={},depth={},validated={}次,confidence={:.2}] {}\n",
                            m.id, m.depth, m.validation_count, m.confidence, m.content
                        ));
                    }
                }
            }
            // 核心认知记忆：semantic/procedural/axiom（供 contradiction_detector）
            "core_memories" => {
                let mems = store.load_memories_by_depths(&["semantic", "procedural", "axiom"]).unwrap_or_default();
                for m in mems.iter().take(40) {
                    ctx.push_str(&format!("- [id={},depth={}] {}\n", m.id, m.depth, m.content));
                }
            }
            // 验证结果：verifier stage 产生的 observations（供 integrator）
            "verification_results" => {
                for obs in store.load_recent_observations_by_category("custom_verifier", 20).unwrap_or_default() {
                    ctx.push_str(&format!("- [验证结果] {}\n", obs.observation));
                }
            }
            // 矛盾检测结果（供 integrator）
            "contradiction_results" => {
                for obs in store.load_recent_observations_by_category("custom_contradiction_detector", 10).unwrap_or_default() {
                    ctx.push_str(&format!("- [矛盾] {}\n", obs.observation));
                }
            }
            // 深度分布统计（供 integrator）
            "depth_summary" => {
                for (depth, count) in store.get_depth_distribution().unwrap_or_default() {
                    ctx.push_str(&format!("- {}: {} 条\n", depth, count));
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
