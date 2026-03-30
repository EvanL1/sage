use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::agent::Agent;
use crate::memory_integrator::{is_ephemeral_content, IncomingMemory, MemoryIntegrator};
use crate::pipeline::{harness, ObserverOutput, PipelineContext};
use crate::prompts;
use crate::store::Store;

/// 根据情绪信号决定初始置信度
/// 包含 [high-arousal] 或 [stress] 标记的记忆更重要，给予 0.8（普通为 0.6）
fn emotion_confidence(content: &str) -> f64 {
    if content.contains("[high-arousal]") || content.contains("[stress]") {
        0.8
    } else {
        0.6
    }
}

/// 观察者：读取 raw observations，加上语义上下文（频率、关联），输出 observer_note
/// 只做「看见」，不做「理解」。不归档 observations（留给 Coach）。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_arousal_confidence_is_0_8() {
        // 包含 [high-arousal] 或 [stress] 标记的记忆置信度应为 0.8
        assert!((emotion_confidence("用户在 [high-arousal] 凌晨 2 点提交了代码") - 0.8).abs() < 0.001);
        assert!((emotion_confidence("任务 [stress] 已逾期未处理") - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_normal_content_confidence_is_0_6() {
        // 普通内容（无情绪标记）保持默认 0.6
        assert!((emotion_confidence("上午参加了团队站会") - 0.6).abs() < 0.001);
        assert!((emotion_confidence("[excited] 完成了新功能") - 0.6).abs() < 0.001);
    }
}
pub async fn annotate(agent: &Agent, store: &Arc<Store>, ctx: &mut PipelineContext) -> Result<bool> {
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

    let output_block = harness::invoke_text(agent, &prompt, Some(system)).await?;
    let entries: Vec<IncomingMemory> = output_block
        .lines()
        .map(|l| l.trim().trim_start_matches('-').trim())
        .filter(|l| !l.is_empty())
        .map(|l| IncomingMemory {
            content: l.to_string(),
            category: "observer_note".to_string(),
            source: "observer".to_string(),
            // 高唤醒/压力标记的记忆更重要，给予更高初始置信度
            confidence: emotion_confidence(l),
            about_person: None,
            depth: Some("episodic".to_string()),
        })
        .collect();

    if entries.is_empty() {
        info!("Observer: LLM returned no notes");
        return Ok(false);
    }

    // 捕获 notes 用于 stage I/O 契约
    let notes: Vec<String> = entries.iter().map(|e| e.content.clone()).collect();

    let count = entries.len();
    let integrator = MemoryIntegrator::new(Arc::clone(store));
    let had_output = match integrator.integrate(entries, agent.provider()).await {
        Ok(r) => {
            info!(
                "Observer: {count} notes → {} created, {} updated, {} skipped",
                r.created, r.updated, r.skipped
            );
            r.created + r.updated > 0
        }
        Err(e) => {
            tracing::warn!("Observer: integration failed, falling back to simple insert: {e}");
            let mut saved = 0;
            for line in output_block.lines() {
                let line = line.trim().trim_start_matches('-').trim();
                if !line.is_empty() && !is_ephemeral_content(line) {
                    if store
                        .save_memory_with_visibility(
                            "observer_note",
                            line,
                            "observer",
                            emotion_confidence(line),
                            "subconscious",
                        )
                        .is_ok()
                    {
                        saved += 1;
                    }
                }
            }
            saved > 0
        }
    };

    // 写入上下文：下游 Coach 可读取本次 tick 的 notes
    ctx.observer = Some(ObserverOutput { notes });
    Ok(had_output)
}
