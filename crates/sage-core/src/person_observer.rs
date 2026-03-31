use anyhow::Result;
use tracing::{info, warn};

use crate::pipeline::{actions, invoker, ConstrainedInvoker, PipelineContext};
use crate::prompts;
use crate::store::Store;

/// 从今日事件中提取人物认知，每天 Evening Review 后调用一次
pub async fn extract_persons(invoker: &dyn ConstrainedInvoker, store: &Store, _ctx: &mut PipelineContext) -> Result<bool> {
    // 优先读原始数据（保留人名），observer_notes 作为补充
    let emails = store.get_today_email_summaries(20).unwrap_or_default();
    let messages = store.get_today_message_summaries(30).unwrap_or_default();
    let notes = store.get_today_observer_notes()?;
    let insights = store.get_today_coach_insights()?;

    if emails.is_empty() && messages.is_empty() && notes.is_empty() && insights.is_empty() {
        return Ok(false);
    }

    let mut events = String::new();
    if !emails.is_empty() {
        events.push_str("### 邮件\n");
        for e in &emails {
            events.push_str(&format!("- {e}\n"));
        }
    }
    if !messages.is_empty() {
        events.push_str("### 消息\n");
        for m in &messages {
            events.push_str(&format!("- {m}\n"));
        }
    }
    if !notes.is_empty() {
        events.push_str("### 观察记录\n");
        for n in notes.iter().take(15) {
            events.push_str(&format!("- {n}\n"));
        }
    }
    if !insights.is_empty() {
        events.push_str("### 行为洞察\n");
        for i in insights.iter().take(10) {
            events.push_str(&format!("- {i}\n"));
        }
    }

    let lang = store.prompt_lang();
    let prompt = prompts::person_extract(&lang, &events);

    if prompt.trim().is_empty() {
        return Ok(false);
    }

    invoker.reset_counter();
    let text = invoker::invoke_text(invoker, &prompt, None).await?;

    if text.trim() == "NONE" {
        info!("PersonObserver: no person insights today");
        return Ok(false);
    }

    // rate limit：每次运行最多保存 30 条人物观察
    const MAX_PERSON_OBSERVATIONS: usize = 30;
    let mut count = 0;
    for line in text.lines() {
        if count >= MAX_PERSON_OBSERVATIONS {
            warn!("PersonObserver: rate limit reached ({MAX_PERSON_OBSERVATIONS}), skipping remaining");
            break;
        }
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("PERSON [") {
            if let Some(bracket_end) = rest.find(']') {
                let name = rest[..bracket_end].trim();
                let observation = rest[bracket_end + 1..].trim();
                if name.is_empty() || observation.is_empty() { continue; }
                // 约束层验证：人物观察内容合法性
                let action_line = format!("save_person_memory | {name} | behavior | {observation} | confidence:0.6 | visibility:private");
                let parts: Vec<&str> = action_line.splitn(6, '|').map(|s| s.trim()).collect();
                if let Some(reason) = actions::validate_action_params("save_person_memory", &parts) {
                    warn!("PersonObserver: BLOCKED invalid person memory for {name}: {reason}");
                    continue;
                }
                match store.save_memory_about_person(
                    "behavior",
                    observation,
                    "person_observer",
                    0.6,
                    "private",
                    name,
                ) {
                    Ok(_) => count += 1,
                    Err(e) => warn!("PersonObserver: 保存人物观察失败 {name}: {e}"),
                }
            }
        }
    }

    if count > 0 {
        info!("PersonObserver: extracted {count} person insights");
    }
    Ok(count > 0)
}
