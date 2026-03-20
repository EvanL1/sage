use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::prompts;
use crate::skills;
use crate::store::Store;

/// 学习教练：读取 observer_notes（降级读 raw observations）→ 发现模式 → 保存 coach_insight → 归档
pub async fn learn(agent: &Agent, store: &Store) -> Result<bool> {
    let observations = store.load_unprocessed_observations(50)?;
    if observations.is_empty() {
        info!("Coach: no unprocessed observations, skipping");
        return Ok(false);
    }

    info!("Coach: analyzing {} observations", observations.len());

    // 优先使用 Observer 标注过的 notes；降级使用 raw observations
    let observer_notes = store.load_observer_notes_recent()?;
    let obs_text = if !observer_notes.is_empty() {
        info!(
            "Coach: using {} observer notes (enriched)",
            observer_notes.len()
        );
        observer_notes
            .iter()
            .map(|n| format!("- {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        info!("Coach: no observer notes, falling back to raw observations");
        observations
            .iter()
            .map(|o| format!("- [{}] **{}**: {}", o.created_at, o.category, o.observation))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let lang = store.prompt_lang();
    let existing_insights = store.search_memories("coach_insight", 10)?;
    let existing_text = if existing_insights.is_empty() {
        if lang == "en" { "(empty, first session)".to_string() } else { "（空，首次学习）".to_string() }
    } else {
        existing_insights
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = prompts::coach_user(&lang, &obs_text, &existing_text);
    let observe_guide = skills::load_section("sage-cognitive", "## Phase 1: OBSERVE");
    let system = format!("{observe_guide}\n\n{}", prompts::coach_system_suffix(&lang));
    let resp = agent.invoke(&prompt, Some(&system)).await?;

    let content = resp.text.trim();
    if !content.is_empty() {
        for line in content.lines() {
            let line = line.trim().trim_start_matches('-').trim();
            if !line.is_empty() {
                if let Err(e) = store.save_coach_insight(line) {
                    tracing::error!("Coach: failed to save insight: {e}");
                }
            }
        }
        info!(
            "Coach: {} insight lines saved to SQLite",
            content.lines().count()
        );
    }

    // 归档已处理的 observations
    let ids: Vec<i64> = observations.iter().map(|o| o.id).collect();
    store.mark_observations_processed(&ids)?;
    info!("Coach: {} observations archived", ids.len());

    Ok(true)
}
