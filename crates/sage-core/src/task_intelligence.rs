use anyhow::Result;
use tracing::{info, warn};

use crate::agent::Agent;
use crate::prompts;
use crate::store::Store;

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

    let system = "You are a task detection assistant. Given a single event and the user's current \
        open tasks, determine if this event requires a new task. \
        Output format: `NEW | <task title> | <evidence>` for each new task (max 2). \
        If nothing actionable, output `NONE`.";

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

    let resp = agent.invoke(&prompt, Some(system)).await?;
    let count = parse_and_save_new_task_signals(&resp.text, store, importance)?;

    // Record dedup marker (source=_async_task with underscore prefix → hidden from History page)
    // Task signals go to tasks table only, not the suggestion feed
    let _ = store.record_suggestion("_async_task", title, &resp.text);

    if count > 0 {
        info!("suggest_from_event: {} new signals from '{}'", count, title);
    }
    Ok(count)
}

fn parse_and_save_new_task_signals(response: &str, store: &Store, importance: f32) -> Result<usize> {
    let mut count = 0usize;
    for line in response.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("NEW | ") {
            let parts: Vec<&str> = rest.splitn(2, " | ").collect();
            if parts.is_empty() {
                continue;
            }
            let task_content = truncate(parts[0].trim(), 120);
            let evidence = parts
                .get(1)
                .map(|s| truncate(s.trim(), 120))
                .unwrap_or_default();
            let title = format!("Suggested new task: {}", truncate(&task_content, 50));
            let id = store.save_task_signal_with_importance(
                "new_task", None, &title, &evidence, Some(&task_content), importance,
            )?;
            if id > 0 {
                // 自动创建任务 + 接受信号
                let task_id = store.create_task(&task_content, "ai_signal", None, None, None, Some(&evidence));
                if task_id.is_ok() {
                    let _ = store.update_signal_status(id, "accepted");
                }
                count += 1;
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

    // 已有 pending + 已拒绝 signals，告诉 LLM 避免重复
    let pending = store.get_pending_signals().unwrap_or_default();
    let dismissed = store.get_recent_dismissed_signals(30).unwrap_or_default();
    let mut dedup_items: Vec<String> = Vec::new();
    for s in &pending {
        dedup_items.push(format!("[pending:{}] {}", s.signal_type, s.title));
    }
    for s in &dismissed {
        dedup_items.push(format!("[dismissed:{}] {}", s.signal_type, s.title));
    }
    let pending_section = if dedup_items.is_empty() {
        String::new()
    } else {
        format!(
            "\nALREADY SUGGESTED (do NOT repeat anything similar to these):\n{}\n",
            dedup_items.join("\n")
        )
    };

    let lang = store.prompt_lang();
    let prompt = prompts::task_intelligence_user_template(&lang)
        .replace("{tasks_text}", &tasks_text)
        .replace("{actions_text}", &actions_text)
        .replace("{pending_section}", &pending_section)
        .replace("{done_section}", &done_section);
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
    let (accepted, total) = store.get_signal_accept_rate(30)?;
    let current = store.get_importance_threshold()?;
    if let Some(new_threshold) = calibrate_threshold(accepted, total, current) {
        store.set_importance_threshold(new_threshold)?;
        info!("Importance threshold calibrated: {current:.2} → {new_threshold:.2}");
    }

    if new_count > 0 {
        info!("Task intelligence: {} new signals detected", new_count);
    }

    Ok(new_count)
}

fn parse_and_save_signals(response: &str, store: &Store) -> Result<usize> {
    let mut count = 0usize;

    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() || line == "NONE" {
            continue;
        }

        if let Some(rest) = line.strip_prefix("DONE ") {
            let parts: Vec<&str> = rest.splitn(3, " | ").collect();
            if parts.len() < 2 {
                warn!("Task intelligence: malformed DONE line: {line}");
                continue;
            }
            let task_id: i64 = match parts[0].trim().parse() {
                Ok(id) => id,
                Err(_) => {
                    warn!("Task intelligence: invalid task_id in DONE: {}", parts[0]);
                    continue;
                }
            };
            let evidence = truncate(parts[1].trim(), 120);
            let suggested = parts.get(2).map(|s| truncate(s.trim(), 120));
            let title = format!("Task looks completed: {}", truncate(parts[1].trim(), 50));
            let id = store.save_task_signal(
                "completion",
                Some(task_id),
                &title,
                &evidence,
                suggested.as_deref(),
            )?;
            if id > 0 {
                count += 1;
            }
        } else if let Some(rest) = line.strip_prefix("CANCEL ") {
            let parts: Vec<&str> = rest.splitn(3, " | ").collect();
            if parts.len() < 2 {
                warn!("Task intelligence: malformed CANCEL line: {line}");
                continue;
            }
            let task_id: i64 = match parts[0].trim().parse() {
                Ok(id) => id,
                Err(_) => {
                    warn!("Task intelligence: invalid task_id in CANCEL: {}", parts[0]);
                    continue;
                }
            };
            let evidence = truncate(parts[1].trim(), 120);
            let suggested = parts.get(2).map(|s| truncate(s.trim(), 120));
            let title = format!("Task may be irrelevant: {}", truncate(parts[1].trim(), 50));
            let id = store.save_task_signal(
                "cancellation",
                Some(task_id),
                &title,
                &evidence,
                suggested.as_deref(),
            )?;
            if id > 0 {
                count += 1;
            }
        } else if let Some(rest) = line.strip_prefix("NEW | ") {
            let parts: Vec<&str> = rest.splitn(2, " | ").collect();
            if parts.is_empty() {
                continue;
            }
            let task_content = truncate(parts[0].trim(), 120);
            let evidence = parts
                .get(1)
                .map(|s| truncate(s.trim(), 120))
                .unwrap_or_default();
            let title = format!("Suggested new task: {}", truncate(&task_content, 50));
            let id =
                store.save_task_signal("new_task", None, &title, &evidence, Some(&task_content))?;
            if id > 0 {
                // 自动创建任务 + 接受信号
                let task_id = store.create_task(&task_content, "ai_signal", None, None, None, Some(&evidence));
                if task_id.is_ok() {
                    let _ = store.update_signal_status(id, "accepted");
                }
                count += 1;
            }
        }
    }

    Ok(count)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}…", &s[..end])
    }
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
}
