//! calibrator — 校准模式反思
//!
//! 当同类报告纠正数 >= 3 时，调用 LLM 分析共同模式，
//! 提炼自我约束规则存入 memories 表（category="calibration"）。

use crate::agent::Agent;
use crate::pipeline::PipelineContext;
use crate::prompts;
use crate::store::Store;
use anyhow::Result;
use tracing::{info, warn};

const PATTERN_THRESHOLD: usize = 3;

/// 检查各类报告的纠正积累，达到阈值时触发 LLM 模式反思
/// 返回 true 如果有新规则生成
pub async fn reflect_patterns(agent: &Agent, store: &Store, _ctx: &mut PipelineContext) -> Result<bool> {
    let mut generated = false;

    for report_type in &["morning", "evening", "weekly"] {
        let corrections = store.get_corrections_for_pattern(report_type)?;
        if corrections.len() < PATTERN_THRESHOLD {
            continue;
        }

        // 检查是否已有此类 calibration 记忆（避免重复反思）
        let existing = store.get_memories_by_category("calibration")?;
        let type_tag = format!("[{report_type}]");
        let last_correction_time = corrections
            .last()
            .map(|c| c.created_at.as_str())
            .unwrap_or("");
        let already_reflected = existing
            .iter()
            .any(|m| m.content.contains(&type_tag) && m.created_at.as_str() > last_correction_time);
        if already_reflected {
            continue;
        }

        let lang = store.prompt_lang();
        let corrections_text: Vec<String> = corrections
            .iter()
            .map(|c| {
                if lang == "en" {
                    format!("- Wrong: \"{}\" → Correct: \"{}\"", c.wrong_claim, c.correct_fact)
                } else {
                    format!("- 错误：「{}」→ 正确：「{}」", c.wrong_claim, c.correct_fact)
                }
            })
            .collect();

        let prompt = prompts::calibrator_reflect(
            &lang,
            report_type,
            corrections.len(),
            &corrections_text.join("\n"),
        );

        match agent.invoke(&prompt, None).await {
            Ok(resp) => {
                let rules: Vec<&str> = resp
                    .text
                    .lines()
                    .filter(|l| {
                        let t = l.trim_start();
                        t.starts_with("规则：")
                            || t.starts_with("规则:")
                            || t.starts_with("Rule:")
                            || t.starts_with("RULE:")
                    })
                    .collect();
                for rule in &rules {
                    let content = format!("[{report_type}] {}", rule.trim());
                    if let Err(e) = store.save_memory("calibration", &content, "calibrator", 0.75) {
                        warn!("保存校准规则失败: {e}");
                    }
                }
                if !rules.is_empty() {
                    info!(
                        "Calibrator: {} 报告生成 {} 条新校准规则",
                        report_type,
                        rules.len()
                    );
                    generated = true;
                }
            }
            Err(e) => warn!("Calibrator LLM 调用失败: {e}"),
        }
    }

    Ok(generated)
}
