use anyhow::Result;
use tracing::{info, warn};

use crate::agent::Agent;
use crate::prompts;
use crate::store::Store;

/// 从今日事件中提取人物认知，每天 Evening Review 后调用一次
pub async fn extract_persons(agent: &Agent, store: &Store) -> Result<bool> {
    // 收集今日素材：observer_notes + coach_insights
    let notes = store.get_today_observer_notes()?;
    let insights = store.get_today_coach_insights()?;

    if notes.is_empty() && insights.is_empty() {
        return Ok(false);
    }

    let mut events = String::new();
    if !notes.is_empty() {
        events.push_str("### 观察记录\n");
        for n in notes.iter().take(20) {
            events.push_str(&format!("- {}\n", n));
        }
    }
    if !insights.is_empty() {
        events.push_str("### 行为洞察\n");
        for i in insights.iter().take(10) {
            events.push_str(&format!("- {}\n", i));
        }
    }

    let lang = store.prompt_lang();
    let prompt = prompts::person_extract(&lang, &events);

    agent.reset_counter();
    let resp = agent.invoke(&prompt, None).await?;

    if resp.text.trim() == "NONE" {
        info!("PersonObserver: no person insights today");
        return Ok(false);
    }

    let mut count = 0;
    for line in resp.text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("PERSON [") {
            if let Some(bracket_end) = rest.find(']') {
                let name = rest[..bracket_end].trim();
                let observation = rest[bracket_end + 1..].trim();
                if !name.is_empty() && !observation.is_empty() {
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
    }

    if count > 0 {
        info!("PersonObserver: extracted {count} person insights");
    }
    Ok(count > 0)
}
