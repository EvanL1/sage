use anyhow::Result;
use tracing::{info, warn};

use crate::agent::Agent;
use crate::pipeline::{harness, CoachOutput, PipelineContext};
use crate::prompts;
use crate::skills;
use crate::store::Store;

/// 学习教练：读取 observer_notes（降级读 raw observations）→ 发现模式 → 保存 coach_insight → 归档
///
/// I/O 契约：
/// - 读取 `ctx.observer` 判断上游 Observer 是否在本次 tick 产出了 notes
/// - 降级：ctx.observer 为 None → 从 SQLite 读（可能是旧数据）→ 仍无则用 raw observations
/// - 写入 `ctx.coach` 供 Mirror/Questioner 下游消费
pub async fn learn(agent: &Agent, store: &Store, ctx: &mut PipelineContext) -> Result<bool> {
    let observations = store.load_unprocessed_observations(50)?;
    if observations.is_empty() {
        info!("Coach: no unprocessed observations, skipping");
        return Ok(false);
    }

    info!("Coach: analyzing {} observations", observations.len());

    // I/O 契约：优先读 ctx.observer（本次 tick 的 notes）
    let (obs_text, degraded) = match &ctx.observer {
        Some(o) if !o.notes.is_empty() => {
            info!("Coach: using {} notes from ctx.observer (this tick)", o.notes.len());
            (o.notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n"), false)
        }
        _ => {
            // 降级路径 1：Observer 本次 tick 没产出，从 SQLite 读历史 notes
            let observer_notes = store.load_observer_notes_recent()?;
            if !observer_notes.is_empty() {
                info!("Coach: ctx.observer empty, using {} notes from SQLite", observer_notes.len());
                (observer_notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n"), false)
            } else {
                // 降级路径 2：完全没有 observer_notes，用 raw observations
                info!("Coach: no observer notes anywhere, falling back to raw observations (degraded)");
                (observations.iter()
                    .map(|o| format!("- [{}] **{}**: {}", o.created_at, o.category, o.observation))
                    .collect::<Vec<_>>()
                    .join("\n"), true)
            }
        }
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
    let content = harness::invoke_text(agent, &prompt, Some(&system)).await?;
    // rate limit：每次运行最多保存 20 条洞察
    const MAX_INSIGHTS: usize = 20;
    let mut saved_insights = Vec::new();
    if !content.is_empty() {
        for line in content.lines() {
            if saved_insights.len() >= MAX_INSIGHTS {
                warn!("Coach: rate limit reached ({MAX_INSIGHTS}), skipping remaining insights");
                break;
            }
            let line = line.trim().trim_start_matches('-').trim();
            // 验证内容：非空且不超过 500 字节
            if line.is_empty() || line.len() > 500 {
                continue;
            }
            if let Err(e) = store.save_coach_insight(line) {
                tracing::error!("Coach: failed to save insight: {e}");
            } else {
                saved_insights.push(line.to_string());
            }
        }
        info!("Coach: {} insight lines saved to SQLite", saved_insights.len());
    }

    // 归档已处理的 observations
    let ids: Vec<i64> = observations.iter().map(|o| o.id).collect();
    store.mark_observations_processed(&ids)?;
    info!("Coach: {} observations archived", ids.len());

    // 写入上下文：下游 Mirror/Questioner 可读取
    ctx.coach = Some(CoachOutput {
        insights: saved_insights,
        observations_archived: ids.len(),
        degraded,
    });

    Ok(true)
}
