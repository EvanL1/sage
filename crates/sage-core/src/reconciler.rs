use anyhow::Result;
use tracing::{info, warn};

use crate::agent::Agent;
use crate::prompts;
use crate::store::Store;

/// 认知调和（增量）：新记忆写入后，检查是否与现有 decisions/strategy_insights 矛盾。
pub async fn reconcile(agent: &Agent, store: &Store, new_content: &str) -> Result<usize> {
    agent.reset_counter();

    let all_memories = store.load_active_memories()?;
    let decisions: Vec<_> = all_memories
        .iter()
        .filter(|m| m.category == "decision" || m.category == "recent_decision")
        .take(20)
        .collect();
    let insights: Vec<_> = all_memories
        .iter()
        .filter(|m| m.category == "strategy_insight")
        .take(10)
        .collect();

    let existing: Vec<_> = decisions
        .iter()
        .chain(insights.iter())
        .copied()
        .filter(|m| m.content != new_content && !m.content.starts_with("[REVISED]"))
        .collect();

    if existing.is_empty() {
        return Ok(0);
    }

    let items_text = format_items(&existing);
    let lang = store.prompt_lang();
    let prompt = prompts::reconciler_incremental(&lang, new_content, &items_text);
    let system = prompts::reconciler_system(&lang);
    let resp = agent.invoke(&prompt, Some(system)).await?;
    apply_revisions(store, &resp.text, &existing)
}

/// 认知调和（全量扫描）：检查所有 decisions/strategy_insights 之间的内部矛盾、
/// 基于错误前提的推导、已过时的结论。由 Settings UI 手动触发。
pub async fn reconcile_full(agent: &Agent, store: &Store) -> Result<usize> {
    agent.reset_counter();

    let all_memories = store.load_active_memories()?;

    let mut all: Vec<_> = all_memories
        .iter()
        .filter(|m| {
            matches!(
                m.category.as_str(),
                "decision" | "recent_decision" | "strategy_insight" | "coach_insight"
            ) && !m.content.starts_with("[REVISED]")
        })
        .collect();

    all.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

    if all.len() < 2 {
        info!("Reconciler full scan: not enough memories to reconcile");
        return Ok(0);
    }

    info!(
        "Reconciler full scan: checking {} memories for contradictions",
        all.len()
    );

    let items_text = format_items(&all);
    let lang = store.prompt_lang();
    let prompt = prompts::reconciler_full(&lang, &items_text);
    let system = prompts::reconciler_system(&lang);
    let resp = agent.invoke(&prompt, Some(system)).await?;
    apply_revisions(store, &resp.text, &all)
}

fn format_items(items: &[&sage_types::Memory]) -> String {
    items
        .iter()
        .map(|m| format!("[id={}] [{}] {}", m.id, m.category, m.content))
        .collect::<Vec<_>>()
        .join("\n")
}

fn apply_revisions(
    store: &Store,
    llm_output: &str,
    candidates: &[&sage_types::Memory],
) -> Result<usize> {
    let text = llm_output.trim();

    if text.starts_with("NONE") || text.is_empty() {
        info!("Reconciler: no contradictions found");
        return Ok(0);
    }

    let mut revised = 0;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("REVISE ") {
            if let Some((id_str, reason)) = rest.split_once(':') {
                let id_str = id_str.trim();
                let reason = reason.trim();
                if let Ok(id) = id_str.parse::<i64>() {
                    if let Some(mem) = candidates.iter().find(|m| m.id == id) {
                        let annotated = format!("[REVISED] {}\n— Reason: {}", mem.content, reason);
                        if let Err(e) = store.update_memory(id, &annotated, 0.05) {
                            warn!("Reconciler: failed to revise memory {id}: {e}");
                        } else {
                            info!("Reconciler: revised memory id={id}, reason: {reason}");
                            revised += 1;
                        }
                    } else {
                        warn!("Reconciler: id={id} not in candidate list, skipping");
                    }
                }
            }
        }
    }

    if revised > 0 {
        info!("Reconciler: {revised} memories revised with corrections");
    }

    Ok(revised)
}
