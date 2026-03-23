//! 消息过时性检测：pending → resolved / expired
//!
//! Resolution signals:
//! - Reply detection: user sent to same channel after receiving
//! - TTL expiration: message exceeded natural lifetime
//! - LLM classification: ambiguous cases

use std::sync::Arc;
use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::store::Store;

/// Result of a staleness check pass
#[derive(Debug, Default)]
pub struct StalenessResult {
    pub checked: usize,
    pub resolved: usize,
    pub expired: usize,
    pub unchanged: usize,
}

/// Run staleness detection on pending messages
pub async fn check_staleness(agent: &Agent, store: &Arc<Store>) -> Result<StalenessResult> {
    // 获取超过 4 小时的 pending 消息
    let pending = store.get_pending_messages_older_than(4)?;
    if pending.is_empty() {
        return Ok(StalenessResult::default());
    }

    info!("Staleness: checking {} pending messages", pending.len());

    // 获取最近 7 天发出的消息，用于回复检测
    let sent = store.get_recent_sent_messages(168)?;

    let mut result = StalenessResult { checked: pending.len(), ..Default::default() };
    let mut resolved_ids = Vec::new();
    let mut expired_ids = Vec::new();
    let mut ambiguous = Vec::new();

    for msg in &pending {
        // Signal 1: 回复检测 — 用户是否向同一频道发送了消息？
        let has_reply = sent.iter().any(|s| {
            s.channel == msg.channel && s.timestamp > msg.timestamp
        });

        if has_reply {
            resolved_ids.push(msg.id);
            continue;
        }

        // Signal 2: TTL 过期检测
        let age_hours = message_age_hours(msg);
        let ttl = estimate_ttl(msg);

        if age_hours > ttl {
            expired_ids.push(msg.id);
            continue;
        }

        // Signal 3: 超过 24 小时的模糊消息，收集用于 LLM 分类
        if age_hours > 24.0 {
            ambiguous.push(msg);
        }
    }

    // 批量 LLM 分类模糊消息（每次最多 10 条）
    if !ambiguous.is_empty() {
        let batch: Vec<_> = ambiguous.into_iter().take(10).collect();
        let classifications = classify_messages(agent, store, &batch).await?;

        for (msg, classification) in batch.iter().zip(classifications.iter()) {
            match classification.as_str() {
                "resolved" => resolved_ids.push(msg.id),
                "expired" => expired_ids.push(msg.id),
                _ => result.unchanged += 1,
            }
        }
    }

    // 应用状态变更
    result.resolved = store.resolve_messages(&resolved_ids)?;

    for id in &expired_ids {
        store.update_message_action_state(*id, "expired")?;
    }
    result.expired = expired_ids.len();

    result.unchanged = result.checked - result.resolved - result.expired;

    info!(
        "Staleness: {} checked, {} resolved, {} expired, {} unchanged",
        result.checked, result.resolved, result.expired, result.unchanged
    );

    Ok(result)
}

/// 计算消息的已存活小时数
fn message_age_hours(msg: &sage_types::Message) -> f64 {
    use chrono::{Local, NaiveDateTime};

    let created = msg.created_at.as_str();
    if let Ok(dt) = NaiveDateTime::parse_from_str(created, "%Y-%m-%d %H:%M:%S") {
        let now = Local::now().naive_local();
        let duration = now - dt;
        return duration.num_minutes() as f64 / 60.0;
    }

    // 解析失败则视为非常旧的消息
    999.0
}

/// 根据消息类型估算 TTL（小时）
fn estimate_ttl(msg: &sage_types::Message) -> f64 {
    // 会议相关：TTL 到会议时间后 1 天
    if msg.message_type == "meeting" || msg.channel.to_lowercase().contains("calendar") {
        return 48.0;
    }

    // 邮件：7 天
    if msg.source == "email" {
        return 168.0;
    }

    // 即时消息：3 天
    if msg.source == "teams" || msg.source == "wechat" || msg.source == "slack" {
        return 72.0;
    }

    // 默认：5 天
    120.0
}

/// 使用 LLM 对模糊消息进行分类
async fn classify_messages(
    agent: &Agent,
    store: &Arc<Store>,
    messages: &[&sage_types::Message],
) -> Result<Vec<String>> {
    if messages.is_empty() {
        return Ok(Vec::new());
    }

    let lang = store.prompt_lang();

    let msg_text = messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let content = m.content.as_deref().unwrap_or("(no content)");
            let preview = if content.len() > 200 { &content[..200] } else { content };
            format!(
                "[{}] From: {} | Channel: {} | Source: {} | Age: {:.0}h\n{}",
                i + 1,
                m.sender,
                m.channel,
                m.source,
                message_age_hours(m),
                preview
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let system = if lang == "en" {
        "You classify messages as: 'resolved' (likely already handled), 'expired' (no longer actionable), or 'pending' (still needs attention). Output one classification per line, just the word."
    } else {
        "你对消息进行分类：'resolved'（可能已处理）、'expired'（不再可操作）、'pending'（仍需关注）。每行输出一个分类词。"
    };

    let prompt = if lang == "en" {
        format!("Classify each message below. Consider: is this something that still needs a response or action? Or has enough time passed that it's no longer relevant?\n\n{msg_text}")
    } else {
        format!("对以下每条消息进行分类。考虑：这是否仍需要回复或处理？还是已经过了足够长的时间，不再相关？\n\n{msg_text}")
    };

    let resp = agent.invoke(&prompt, Some(system)).await?;

    let mut result: Vec<String> = resp
        .text
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .map(|l| {
            if l.contains("resolved") {
                "resolved".to_string()
            } else if l.contains("expired") {
                "expired".to_string()
            } else {
                "pending".to_string()
            }
        })
        .collect();

    // LLM 返回行数不足时补 "pending"
    while result.len() < messages.len() {
        result.push("pending".to_string());
    }
    result.truncate(messages.len());

    Ok(result)
}
