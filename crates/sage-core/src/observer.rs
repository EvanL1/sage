use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::agent::Agent;
use crate::memory_integrator::{IncomingMemory, MemoryIntegrator};
use crate::prompts;
use crate::store::Store;

/// 观察者：读取 raw observations，加上语义上下文（频率、关联），输出 observer_note
/// 只做「看见」，不做「理解」。不归档 observations（留给 Coach）。
pub async fn annotate(agent: &Agent, store: &Arc<Store>) -> Result<bool> {
    let observations = store.load_unprocessed_observations(50)?;
    if observations.is_empty() {
        info!("Observer: no unprocessed observations, skipping");
        return Ok(false);
    }

    info!("Observer: annotating {} observations", observations.len());

    // 待标注的原始记录
    let obs_text = observations
        .iter()
        .map(|o| format!("- [{}] {}: {}", o.created_at, o.category, o.observation))
        .collect::<Vec<_>>()
        .join("\n");

    // 近 7 天历史（供 LLM 计算频率和关联）
    let history = store.load_recent_observations(200)?;
    let lang = store.prompt_lang();
    let history_text = if history.is_empty() {
        if lang == "en" { "(no history)".to_string() } else { "（无历史记录）".to_string() }
    } else {
        history
            .iter()
            .map(|(cat, obs)| format!("- [{}] {}", cat, obs))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = prompts::observer_user(&lang, &obs_text, &history_text);
    let system = prompts::observer_system(&lang);

    let resp = agent.invoke(&prompt, Some(system)).await?;

    let entries: Vec<IncomingMemory> = resp
        .text
        .lines()
        .map(|l| l.trim().trim_start_matches('-').trim())
        .filter(|l| !l.is_empty())
        .map(|l| IncomingMemory {
            content: l.to_string(),
            category: "observer_note".to_string(),
            source: "observer".to_string(),
            confidence: 0.6,
            about_person: None,
        })
        .collect();

    if entries.is_empty() {
        info!("Observer: LLM returned no notes");
        return Ok(false);
    }

    let count = entries.len();
    let integrator = MemoryIntegrator::new(Arc::clone(store));
    match integrator.integrate(entries, agent.provider()).await {
        Ok(r) => {
            info!(
                "Observer: {count} notes → {} created, {} updated, {} skipped",
                r.created, r.updated, r.skipped
            );
            Ok(r.created + r.updated > 0)
        }
        Err(e) => {
            tracing::warn!("Observer: integration failed, falling back to simple insert: {e}");
            let mut saved = 0;
            for line in resp.text.lines() {
                let line = line.trim().trim_start_matches('-').trim();
                if !line.is_empty() {
                    if store
                        .save_memory_with_visibility(
                            "observer_note",
                            line,
                            "observer",
                            0.6,
                            "subconscious",
                        )
                        .is_ok()
                    {
                        saved += 1;
                    }
                }
            }
            Ok(saved > 0)
        }
    }
}
