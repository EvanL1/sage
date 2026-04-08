use anyhow::{anyhow, Result};
use tracing::{info, warn};

use crate::agent::Agent;
use crate::pipeline::{actions, parser};
use crate::prompts;
use crate::store::Store;
use crate::text_utils::truncate_with_ellipsis as truncate;

// ─── Typed Task Signal Commands ─────────────────────────────────────────────

/// 结构化的任务信号命令
#[derive(Debug, Clone, PartialEq)]
pub enum TaskSignalCommand {
    /// 任务已完成
    Done { task_id: i64, evidence: String, suggested_outcome: Option<String> },
    /// 任务应取消
    Cancel { task_id: i64, reason: String, suggested_outcome: Option<String> },
    /// 新任务建议
    New { content: String, evidence: String, due_date: Option<String> },
}

/// 单行解析任务信号命令
fn parse_task_signal_command(line: &str) -> Result<Option<TaskSignalCommand>> {
    if let Some(rest) = line.strip_prefix("DONE ") {
        let parts: Vec<&str> = rest.splitn(3, " | ").collect();
        if parts.len() < 2 {
            return Err(anyhow!("DONE: need at least task_id | evidence"));
        }
        let task_id: i64 = parts[0].trim().parse()
            .map_err(|_| anyhow!("DONE: invalid task_id '{}'", parts[0].trim()))?;
        let evidence = truncate(parts[1].trim(), 120);
        let suggested = parts.get(2).map(|s| truncate(s.trim(), 120));
        return Ok(Some(TaskSignalCommand::Done { task_id, evidence, suggested_outcome: suggested }));
    }

    if let Some(rest) = line.strip_prefix("CANCEL ") {
        let parts: Vec<&str> = rest.splitn(3, " | ").collect();
        if parts.len() < 2 {
            return Err(anyhow!("CANCEL: need at least task_id | reason"));
        }
        let task_id: i64 = parts[0].trim().parse()
            .map_err(|_| anyhow!("CANCEL: invalid task_id '{}'", parts[0].trim()))?;
        let reason = truncate(parts[1].trim(), 120);
        let suggested = parts.get(2).map(|s| truncate(s.trim(), 120));
        return Ok(Some(TaskSignalCommand::Cancel { task_id, reason, suggested_outcome: suggested }));
    }

    if let Some(rest) = line.strip_prefix("NEW | ") {
        let parts: Vec<&str> = rest.splitn(3, " | ").collect();
        if parts.is_empty() || parts[0].trim().is_empty() {
            return Err(anyhow!("NEW: empty task content"));
        }
        let content = truncate(parts[0].trim(), 120);
        let evidence = parts.get(1).map(|s| truncate(s.trim(), 120)).unwrap_or_default();
        // 从第3段或 evidence 中提取 due:YYYY-MM-DD
        let due_date = parts.get(2)
            .and_then(|s| extract_due_date(s))
            .or_else(|| extract_due_date(&evidence));
        return Ok(Some(TaskSignalCommand::New { content, evidence, due_date }));
    }

    // 非命令行（叙述文本）→ 跳过
    Ok(None)
}

/// 从 LLM 响应解析任务信号，使用通用 parser 的 XML 提取
fn parse_task_signal_response(text: &str) -> parser::ParseResult<TaskSignalCommand> {
    parser::parse_commands(text, parse_task_signal_command)
}

/// Heuristic importance score for a single event, range [0.0, 1.0].
/// No LLM call — purely rule-based for fast, cheap filtering.
pub fn score_event_importance(category: &str, title: &str, body: &str) -> f32 {
    let base = match category {
        "urgent" => 0.85,
        "scheduled" => 0.7,
        "normal" => 0.4,
        "background" => 0.2,
        "feed" => 0.15,
        _ => 0.3,
    };

    let urgency_words = [
        "urgent", "deadline", "asap", "blocked", "critical",
        "紧急", "截止", "阻塞", "尽快", "immediately",
    ];
    let combined = format!("{} {}", title, body).to_lowercase();
    let unique_hits = urgency_words
        .iter()
        .filter(|w| combined.contains(*w))
        .count();
    let boost = (unique_hits as f32 * 0.1).min(0.3);

    (base + boost).clamp(0.0, 1.0)
}

/// Focused single-event task suggestion: calls LLM with one event and open tasks.
/// Returns the number of new task signals created.
pub async fn suggest_from_event(
    agent: &Agent,
    store: &Store,
    category: &str,
    title: &str,
    body: &str,
    importance: f32,
) -> Result<usize> {
    // Dedup: skip if we recently suggested a task for this same event title
    if store.has_recent_suggestion("_async_task", title) {
        return Ok(0);
    }

    // Load top 5 open tasks for dedup context
    let open_tasks = store.list_tasks(Some("open"), 5).unwrap_or_default();
    let open_titles: Vec<String> = open_tasks
        .iter()
        .map(|(_, content, _, _, _, _, _, _, _, _, _)| content.clone())
        .collect();

    let today = chrono::Local::now().format("%Y-%m-%d");
    let system = format!(
        "You are a task detection assistant. Given a single event and the user's current \
        open tasks, determine if this event requires a new task. \
        GRANULARITY: Only suggest ATOMIC tasks (one action, < 2 hours). \
        For multi-step work, suggest only the next single action. \
        Each suggested task needs an action_key in format \"verb:entity:person\". \
        Output format: `NEW | <task title> | <evidence> | due:YYYY-MM-DD` for each new task (max 2). \
        Include due:YYYY-MM-DD when a deadline is mentioned or implied (today is {today}). \
        If nothing actionable, output `NONE`."
    );

    let open_tasks_text = if open_titles.is_empty() {
        "(none)".to_string()
    } else {
        open_titles.join("\n")
    };

    let prompt = format!(
        "Event:\n  category: {category}\n  title: {title}\n  body: {body}\n  importance: {importance:.2}\n\n\
         Current open tasks:\n{open_tasks_text}\n\n\
         Does this event require a new task? Output NEW or NONE."
    );

    let resp = agent.invoke(&prompt, Some(&system)).await?;
    let count = parse_and_save_new_task_signals(&resp.text, store, importance)?;

    // Record dedup marker (source=_async_task with underscore prefix → hidden from History page)
    // Task signals go to tasks table only, not the suggestion feed
    let _ = store.record_suggestion("_async_task", title, &resp.text);

    if count > 0 {
        info!("suggest_from_event: {} new signals from '{}'", count, title);
    }
    Ok(count)
}

/// 构建约束层 ACTION 行并执行 create_task
fn constrained_create_task(content: &str, due_date: Option<&str>, store: &Store, caller: &str) -> Option<i64> {
    let due_part = due_date.map(|d| format!(" | due:{d}")).unwrap_or_default();
    let action_line = format!("create_task | {content} | priority:normal{due_part}");
    actions::execute_single_action(&action_line, &["create_task"], store, caller)
}

const MAX_SUGGEST_TASKS: usize = 2;
const MAX_DETECT_TASKS: usize = 3;

fn parse_and_save_new_task_signals(response: &str, store: &Store, importance: f32) -> Result<usize> {
    let parsed = parse_task_signal_response(response);
    let mut count = 0usize;
    for cmd in &parsed.commands {
        if count >= MAX_SUGGEST_TASKS { break; }
        if let TaskSignalCommand::New { content, evidence, due_date } = cmd {
            let title = format!("Suggested new task: {}", truncate(content, 50));
            let id = store.save_task_signal_with_importance(
                "new_task", None, &title, evidence, Some(content), importance,
            )?;
            if id > 0 {
                match constrained_create_task(content, due_date.as_deref(), store, "task_suggest") {
                    Some(tid) if tid > 0 => {
                        let _ = store.update_signal_status(id, "accepted");
                        count += 1;
                    }
                    _ => { let _ = store.update_signal_status(id, "dismissed"); }
                }
            }
        }
    }
    Ok(count)
}

/// EMA 校准：根据用户 accept/dismiss 历史自适应调整阈值。
/// 阈值向 accept_rate 收敛：高接受率 → 阈值上移（质量已足够，收紧门槛）；
/// 低接受率 → 阈值下移但不低于 0.4（放宽以获取更多信号）。
/// 返回 None 表示样本不足（< 5），不调整。
pub fn calibrate_threshold(accepted: usize, total: usize, current_threshold: f32) -> Option<f32> {
    const MIN_SAMPLES: usize = 5;
    const EMA_WEIGHT: f32 = 0.2;
    if total < MIN_SAMPLES {
        return None;
    }
    let accept_rate = accepted as f32 / total as f32;
    let new_threshold = current_threshold * (1.0 - EMA_WEIGHT) + accept_rate * EMA_WEIGHT;
    Some(new_threshold.clamp(0.4, 0.9))
}

/// Task Intelligence: compare recent events against open tasks,
/// generate completion / cancellation / new-task signals for user review.
pub async fn detect_task_signals(agent: &Agent, store: &Store) -> Result<usize> {
    let open_tasks = store.list_tasks(Some("open"), 50)?;
    let observations = store.load_recent_observations(30)?;
    let messages = store.load_recent_messages(20)?;

    // Skip when there is nothing to compare
    if open_tasks.is_empty() && observations.is_empty() && messages.is_empty() {
        info!("Task intelligence: nothing to compare, skipping");
        return Ok(0);
    }

    // Skip when there are no open tasks (no point detecting completion/cancellation)
    if open_tasks.is_empty() {
        info!("Task intelligence: no open tasks, skipping");
        return Ok(0);
    }

    // Build tasks section
    let tasks_text = open_tasks
        .iter()
        .map(|(id, content, _, _, due, _, _, _, _, _, _)| {
            let due_str = due.as_deref().unwrap_or("no due date");
            format!("[id={id}] {content} (due: {due_str})")
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Build done/cancelled tasks section — prevent suggesting tasks that already exist
    let done_tasks = store.list_tasks(Some("done"), 30).unwrap_or_default();
    let done_section = if done_tasks.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = done_tasks
            .iter()
            .map(|(_, content, _, _, _, _, _, _, _, _, _)| content.clone())
            .collect();
        format!(
            "\nALREADY COMPLETED TASKS (do NOT suggest these again):\n{}\n",
            items.join("\n")
        )
    };

    // Build recent actions section
    let mut recent_actions: Vec<String> = Vec::new();

    for (cat, obs) in observations.iter().take(20) {
        let snippet = truncate(obs, 80);
        recent_actions.push(format!("[observation:{cat}] {snippet}"));
    }

    for msg in messages.iter().take(15) {
        let snippet = truncate(&msg.content, 80);
        recent_actions.push(format!("[chat] {snippet}"));
    }

    if recent_actions.is_empty() {
        info!("Task intelligence: no recent actions to compare against tasks");
        return Ok(0);
    }

    let actions_text = recent_actions.join("\n");

    // 已有 pending + 已接受 + 已拒绝 signals，告诉 LLM 避免重复
    let pending = store.get_pending_signals().unwrap_or_default();
    let accepted = store.get_recent_accepted_signals(20).unwrap_or_default();
    let dismissed = store.get_recent_dismissed_signals(30).unwrap_or_default();
    let mut dedup_items: Vec<String> = Vec::new();
    for s in &pending {
        let display = s.suggested_outcome.as_deref().unwrap_or(&s.title);
        dedup_items.push(format!("[pending:{}] {}", s.signal_type, display));
    }
    for s in &accepted {
        let display = s.suggested_outcome.as_deref().unwrap_or(&s.title);
        dedup_items.push(format!("[accepted:{}] {}", s.signal_type, display));
    }
    for s in &dismissed {
        let display = s.suggested_outcome.as_deref().unwrap_or(&s.title);
        dedup_items.push(format!("[dismissed:{}] {}", s.signal_type, display));
    }
    let pending_section = if dedup_items.is_empty() {
        String::new()
    } else {
        format!(
            "\nALREADY SUGGESTED (do NOT repeat anything similar to these):\n{}\n",
            dedup_items.join("\n")
        )
    };

    // 加载已学习的校准规则，注入 prompt
    let learned_rules = load_learned_rules(store);

    let lang = store.prompt_lang();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let prompt = prompts::task_intelligence_user_template(&lang)
        .replace("{tasks_text}", &tasks_text)
        .replace("{actions_text}", &actions_text)
        .replace("{pending_section}", &pending_section)
        .replace("{done_section}", &done_section)
        .replace("{learned_rules}", &learned_rules)
        .replace("{today}", &today);
    let system = prompts::task_intelligence_system(&lang);
    let resp = agent.invoke(&prompt, Some(system)).await?;
    let response = resp.text;
    let new_count = parse_and_save_signals(&response, store)?;

    // Auto-dismiss old signals (>3 days)
    if let Ok(dismissed) = store.dismiss_old_signals() {
        if dismissed > 0 {
            info!("Task intelligence: dismissed {} old signals", dismissed);
        }
    }

    // Calibrate importance threshold from accept/dismiss history
    let (accept_count, total) = store.get_signal_accept_rate(30)?;
    let current = store.get_importance_threshold()?;
    if let Some(new_threshold) = calibrate_threshold(accept_count, total, current) {
        store.set_importance_threshold(new_threshold)?;
        info!("Importance threshold calibrated: {current:.2} → {new_threshold:.2}");
    }

    // 自我进化：检测 dismiss 模式，触发规则反思
    if let Err(e) = self_reflect_on_dismissals(agent, store).await {
        warn!("Task self-reflection failed: {e}");
    }

    if new_count > 0 {
        info!("Task intelligence: {} new signals detected", new_count);
    }

    Ok(new_count)
}

fn parse_and_save_signals(response: &str, store: &Store) -> Result<usize> {
    let parsed = parse_task_signal_response(response);
    if !parsed.rejected.is_empty() {
        warn!("Task intelligence: {} commands rejected", parsed.rejected.len());
    }

    let mut count = 0usize;
    let mut task_count = 0usize;
    for cmd in &parsed.commands {
        match cmd {
            TaskSignalCommand::Done { task_id, evidence, suggested_outcome } => {
                let title = format!("Task looks completed: {}", truncate(evidence, 50));
                let id = store.save_task_signal(
                    "completion", Some(*task_id), &title, evidence, suggested_outcome.as_deref(),
                )?;
                if id > 0 { count += 1; }
            }
            TaskSignalCommand::Cancel { task_id, reason, suggested_outcome } => {
                let title = format!("Task may be irrelevant: {}", truncate(reason, 50));
                let id = store.save_task_signal(
                    "cancellation", Some(*task_id), &title, reason, suggested_outcome.as_deref(),
                )?;
                if id > 0 { count += 1; }
            }
            TaskSignalCommand::New { content, evidence, due_date } => {
                if task_count >= MAX_DETECT_TASKS {
                    warn!("detect_task_signals: rate limit reached ({MAX_DETECT_TASKS} tasks), skipping");
                    continue;
                }
                let title = format!("Suggested new task: {}", truncate(content, 50));
                let id = store.save_task_signal("new_task", None, &title, evidence, Some(content))?;
                if id > 0 {
                    match constrained_create_task(content, due_date.as_deref(), store, "task_detect") {
                        Some(tid) if tid > 0 => {
                            let _ = store.update_signal_status(id, "accepted");
                            task_count += 1;
                            count += 1;
                        }
                        _ => { let _ = store.update_signal_status(id, "dismissed"); }
                    }
                }
            }
        }
    }
    Ok(count)
}

/// 加载 task 相关的校准规则，格式化为 prompt 段落
fn load_learned_rules(store: &Store) -> String {
    let rules = store.get_memories_by_category("calibration_task").unwrap_or_default();
    if rules.is_empty() {
        return String::new();
    }
    let items: Vec<String> = rules
        .iter()
        .map(|m| format!("- {}", m.content.trim()))
        .collect();
    format!(
        "\nLEARNED RULES (from past mistakes — follow strictly):\n{}\n",
        items.join("\n")
    )
}

/// 分析 dismiss 模式，当同类信号被连续 dismiss >= 3 次时触发 LLM 反思生成规则
async fn self_reflect_on_dismissals(agent: &Agent, store: &Store) -> Result<()> {
    const REFLECT_THRESHOLD: usize = 3;

    let dismissed = store.get_recent_dismissed_signals(30).unwrap_or_default();
    if dismissed.len() < REFLECT_THRESHOLD {
        return Ok(());
    }

    // 检查上次反思时间，避免频繁反思（至少间隔 7 天）
    let existing = store.get_memories_by_category("calibration_task").unwrap_or_default();
    if let Some(latest) = existing.last() {
        let week_ago = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(7))
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y-%m-%d")
            .to_string();
        if latest.created_at > week_ago {
            return Ok(());
        }
    }

    // 构建被拒绝信号的摘要
    let dismiss_summary: Vec<String> = dismissed
        .iter()
        .take(10)
        .map(|s| format!("- [{}] {}", s.signal_type, s.title))
        .collect();

    let prompt = format!(
        "The following task suggestions were dismissed by the user:\n{}\n\n\
         Analyze the pattern: what types of suggestions does the user NOT want?\n\
         Output 1-3 concise rules (one per line, prefix with \"Rule:\").\n\
         Each rule should be specific and actionable, e.g.:\n\
         Rule: Do not suggest tasks about routine meetings that the user always attends\n\
         Rule: Do not create tasks for information-only emails\n\
         Output ONLY the rules, nothing else.",
        dismiss_summary.join("\n")
    );

    let resp = agent.invoke(&prompt, None).await?;
    const MAX_RULES: usize = 3;
    let mut rule_count = 0;
    for line in resp.text.lines() {
        if rule_count >= MAX_RULES { break; }
        let trimmed = line.trim();
        if let Some(rule) = trimmed.strip_prefix("Rule:").or_else(|| trimmed.strip_prefix("规则：")).or_else(|| trimmed.strip_prefix("规则:")) {
            let rule = rule.trim();
            if !rule.is_empty() {
                let action = format!("save_memory | calibration_task | {rule} | confidence:0.8");
                if actions::execute_single_action(&action, &["save_memory"], store, "task_reflect").is_none() {
                    continue;
                }
                rule_count += 1;
                info!("Task self-evolution: new rule learned — {rule}");
            }
        }
    }
    if rule_count > 0 {
        info!("Task intelligence self-evolved: {rule_count} new rules from dismiss patterns");
    }
    Ok(())
}

/// 从文本中提取 due:YYYY-MM-DD 或裸 YYYY-MM-DD 格式的日期
fn extract_due_date(s: &str) -> Option<String> {
    let s = s.trim();
    // 优先匹配 due:YYYY-MM-DD
    if let Some(rest) = s.strip_prefix("due:").or_else(|| s.strip_prefix("due：")) {
        let date = rest.trim();
        if date.len() >= 10 && date[..10].chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Some(date[..10].to_string());
        }
    }
    // 回退：匹配裸 YYYY-MM-DD
    for word in s.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '-');
        if w.len() == 10 && w.chars().filter(|&c| c == '-').count() == 2
            && w[..4].chars().all(|c| c.is_ascii_digit())
        {
            return Some(w.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- score_event_importance ---

    #[test]
    fn test_score_event_importance_urgent() {
        let score = score_event_importance("urgent", "紧急会议", "需要立即处理");
        assert!(score >= 0.8, "urgent category + keyword should be >= 0.8, got {score}");
    }

    #[test]
    fn test_score_event_importance_normal_email() {
        let score = score_event_importance("normal", "周会纪要", "本周工作进展");
        assert!((score - 0.4).abs() < 0.01, "normal/no keyword should be ~0.4, got {score}");
    }

    #[test]
    fn test_score_event_importance_keyword_boost() {
        let base = score_event_importance("normal", "weekly update", "");
        let boosted = score_event_importance("normal", "deadline for project", "");
        assert!(boosted > base, "keyword 'deadline' should boost score");
        assert!(boosted > 0.4);
    }

    #[test]
    fn test_score_event_importance_multiple_keywords() {
        // Three keyword matches → boost should be capped at +0.3
        let score = score_event_importance("normal", "urgent deadline asap", "blocked critical immediately");
        assert!(score <= 1.0);
        // base 0.4 + max_boost 0.3 = 0.7
        assert!((score - 0.7).abs() < 0.01, "capped boost expected 0.7, got {score}");
    }

    #[test]
    fn test_score_event_importance_feed_low() {
        let score = score_event_importance("feed", "Tech News", "Some article about Rust");
        assert!((score - 0.15).abs() < 0.01, "feed with no keywords should be ~0.15, got {score}");
    }

    // --- calibrate_threshold ---

    #[test]
    fn test_calibrate_threshold_insufficient_data() {
        let result = calibrate_threshold(2, 3, 0.65);
        assert!(result.is_none(), "should return None when total < 5");
    }

    #[test]
    fn test_calibrate_threshold_ema() {
        // accepted=8, total=10 → accept_rate=0.8
        // new = 0.65 * 0.8 + 0.8 * 0.2 = 0.52 + 0.16 = 0.68
        let result = calibrate_threshold(8, 10, 0.65);
        let expected = 0.68f32;
        let actual = result.expect("should return Some");
        assert!((actual - expected).abs() < 0.001, "expected ~{expected}, got {actual}");
    }

    #[test]
    fn test_calibrate_threshold_clamp_low() {
        // accepted=0, total=10 → accept_rate=0.0
        // new = 0.65 * 0.8 + 0.0 * 0.2 = 0.52, but clamp to [0.4, 0.9]
        let result = calibrate_threshold(0, 10, 0.65);
        let actual = result.expect("should return Some");
        assert!(actual >= 0.4, "should be clamped to >= 0.4, got {actual}");
    }

    #[test]
    fn test_calibrate_threshold_clamp_high() {
        // accepted=10, total=10, current=0.95 → new = 0.95*0.8 + 1.0*0.2 = 0.96, clamped to 0.9
        let result = calibrate_threshold(10, 10, 0.95);
        let actual = result.expect("should return Some");
        assert!((actual - 0.9).abs() < 0.001, "should clamp to 0.9, got {actual}");
    }

    // ─── Task Signal Parser Tests ──────────────────────────────────────────

    #[test]
    fn parse_done_signal() {
        let cmd = parse_task_signal_command("DONE 42 | user replied | mark complete").unwrap().unwrap();
        assert_eq!(cmd, TaskSignalCommand::Done {
            task_id: 42,
            evidence: "user replied".into(),
            suggested_outcome: Some("mark complete".into()),
        });
    }

    #[test]
    fn parse_done_without_outcome() {
        let cmd = parse_task_signal_command("DONE 7 | evidence here").unwrap().unwrap();
        match cmd {
            TaskSignalCommand::Done { task_id, suggested_outcome, .. } => {
                assert_eq!(task_id, 7);
                assert!(suggested_outcome.is_none());
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn parse_done_invalid_id() {
        assert!(parse_task_signal_command("DONE abc | evidence").is_err());
    }

    #[test]
    fn parse_cancel_signal() {
        let cmd = parse_task_signal_command("CANCEL 10 | meeting cancelled | remove").unwrap().unwrap();
        assert_eq!(cmd, TaskSignalCommand::Cancel {
            task_id: 10,
            reason: "meeting cancelled".into(),
            suggested_outcome: Some("remove".into()),
        });
    }

    #[test]
    fn parse_new_signal() {
        let cmd = parse_task_signal_command("NEW | Review budget | mentioned in standup").unwrap().unwrap();
        assert_eq!(cmd, TaskSignalCommand::New {
            content: "Review budget".into(),
            evidence: "mentioned in standup".into(),
            due_date: None,
        });
    }

    #[test]
    fn parse_new_without_evidence() {
        let cmd = parse_task_signal_command("NEW | Do something").unwrap().unwrap();
        match cmd {
            TaskSignalCommand::New { content, evidence, .. } => {
                assert_eq!(content, "Do something");
                assert!(evidence.is_empty());
            }
            _ => panic!("expected New"),
        }
    }

    #[test]
    fn parse_new_with_due_date() {
        let cmd = parse_task_signal_command("NEW | Finish report | user said today | due:2026-03-31")
            .unwrap().unwrap();
        assert_eq!(cmd, TaskSignalCommand::New {
            content: "Finish report".into(),
            evidence: "user said today".into(),
            due_date: Some("2026-03-31".into()),
        });
    }

    #[test]
    fn parse_new_due_date_in_evidence() {
        let cmd = parse_task_signal_command("NEW | Finish report | deadline 2026-04-01")
            .unwrap().unwrap();
        match cmd {
            TaskSignalCommand::New { due_date, .. } => {
                assert_eq!(due_date, Some("2026-04-01".into()));
            }
            _ => panic!("expected New"),
        }
    }

    #[test]
    fn extract_due_date_variants() {
        assert_eq!(extract_due_date("due:2026-03-31"), Some("2026-03-31".into()));
        assert_eq!(extract_due_date("due：2026-04-01"), Some("2026-04-01".into()));
        assert_eq!(extract_due_date("deadline 2026-04-02"), Some("2026-04-02".into()));
        assert_eq!(extract_due_date("no date here"), None);
        assert_eq!(extract_due_date(""), None);
    }

    #[test]
    fn parse_new_empty_content_is_err() {
        assert!(parse_task_signal_command("NEW |  | evidence").is_err());
    }

    #[test]
    fn parse_narrative_returns_none() {
        assert!(parse_task_signal_command("Looking at the tasks...").unwrap().is_none());
    }

    #[test]
    fn parse_response_with_output_tags() {
        let text = "Here is my analysis:\n<output>\nDONE 1 | shipped | close\nNEW | Test X | reason\nNONE\n</output>";
        let result = parse_task_signal_response(text);
        assert_eq!(result.commands.len(), 2);
        assert!(result.rejected.is_empty());
    }

    #[test]
    fn parse_response_without_tags() {
        let text = "DONE 1 | evidence | outcome\nNONE";
        let result = parse_task_signal_response(text);
        assert_eq!(result.commands.len(), 1);
    }

    #[test]
    fn parse_response_mixed_valid_and_rejected() {
        let text = "<output>\nDONE 1 | ok | done\nDONE abc | bad id\nNEW | task | ev\n</output>";
        let result = parse_task_signal_response(text);
        assert_eq!(result.commands.len(), 2); // DONE 1 + NEW
        assert_eq!(result.rejected.len(), 1); // DONE abc
    }
}
