use anyhow::Result;
use tracing::{info, warn};

use crate::pipeline::{actions, invoker, ConstrainedInvoker, PipelineContext};
use crate::prompts;
use crate::skills;
use crate::store::Store;

/// 战略家：站在月球看地球 — 完全理性、客观、超然的宏观分析
/// 运行频率：每周（Weekly Report 之后）
/// 输入：已合成的 coach_insight + decision + 历史 strategy_insight
/// 输出：strategy_insight 记忆（subconscious 可见性）
pub async fn strategize(invoker: &dyn ConstrainedInvoker, store: &Store, _ctx: &mut PipelineContext) -> Result<bool> {
    // 去重：7 天内已运行则跳过
    if store.has_recent_suggestion("strategist", "weekly-strategy") {
        info!("Strategist: already ran this week, skipping");
        return Ok(false);
    }

    // 收集输入：Coach 洞察（近 2 周）+ 决策记录 + 历史战略洞察
    let coach_insights = store.search_memories("coach_insight", 20)?;
    let decisions = store.search_memories("decision", 10)?;
    let past_strategies = store.search_memories("strategy_insight", 5)?;

    if coach_insights.is_empty() && decisions.is_empty() {
        info!("Strategist: insufficient data (no insights or decisions), skipping");
        return Ok(false);
    }

    info!(
        "Strategist: analyzing {} coach insights + {} decisions",
        coach_insights.len(),
        decisions.len()
    );

    let lang = store.prompt_lang();

    let insights_text = if coach_insights.is_empty() {
        if lang == "en" { "(no recent insights)".to_string() } else { "（无近期洞察）".to_string() }
    } else {
        coach_insights
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let decisions_text = if decisions.is_empty() {
        if lang == "en" { "(no recent decisions)".to_string() } else { "（无近期决策）".to_string() }
    } else {
        decisions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let past_text = if past_strategies.is_empty() {
        if lang == "en" { "(first strategy analysis)".to_string() } else { "（首次战略分析）".to_string() }
    } else {
        past_strategies
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let prompt = prompts::strategist_user(&lang, &insights_text, &decisions_text, &past_text);
    let strategy_guide = skills::load_section("sage-cognitive", "## Phase 5: STRATEGIZE");
    let system = format!(
        "{strategy_guide}\n\n{}",
        prompts::strategist_system_suffix(&lang)
    );

    let content = invoker::invoke_text(invoker, &prompt, Some(&system)).await?;
    // rate limit：每次运行最多保存 20 条战略洞察
    const MAX_STRATEGIES: usize = 20;
    let mut saved = 0;
    if !content.is_empty() {
        for line in content.lines() {
            if saved >= MAX_STRATEGIES {
                warn!("Strategist: rate limit reached ({MAX_STRATEGIES}), skipping remaining insights");
                break;
            }
            let line = line.trim().trim_start_matches('-').trim();
            if line.is_empty() { continue; }
            // 约束层验证：战略洞察内容合法性
            let action_line = format!("save_memory_visible | strategy_insight | {line} | confidence:0.85 | visibility:subconscious");
            let parts: Vec<&str> = action_line.splitn(6, '|').map(|s| s.trim()).collect();
            if let Some(reason) = actions::validate_action_params("save_memory_visible", &parts) {
                warn!("Strategist: BLOCKED invalid strategy insight: {reason}");
                continue;
            }
            if let Err(e) = store.save_memory_with_visibility(
                "strategy_insight",
                line,
                "strategist",
                0.85,
                "subconscious",
            ) {
                tracing::error!("Strategist: failed to save insight: {e}");
            } else {
                saved += 1;
            }
        }
        info!("Strategist: {saved} strategic insights saved");
    }

    // 记录去重标记
    store.record_suggestion("strategist", "weekly-strategy", &content)?;

    Ok(saved > 0)
}
