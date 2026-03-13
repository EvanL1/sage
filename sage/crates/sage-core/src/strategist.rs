use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::skills;
use crate::store::Store;

/// 战略家：站在月球看地球 — 完全理性、客观、超然的宏观分析
/// 运行频率：每周（Weekly Report 之后）
/// 输入：已合成的 coach_insight + decision + 历史 strategy_insight
/// 输出：strategy_insight 记忆（subconscious 可见性）
pub async fn strategize(agent: &Agent, store: &Store) -> Result<bool> {
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

    let insights_text = if coach_insights.is_empty() {
        "（无近期洞察）".to_string()
    } else {
        coach_insights
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let decisions_text = if decisions.is_empty() {
        "（无近期决策）".to_string()
    } else {
        decisions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let past_text = if past_strategies.is_empty() {
        "（首次战略分析）".to_string()
    } else {
        past_strategies
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        "你是一个完全超然的战略分析者。你站在月球上看地球——没有情感、没有偏见，只有结构和轨迹。\n\n\
         你的任务：从以下已积累的数据中，识别 2-3 个结构性观察。\n\n\
         ## 近期行为模式（Coach 观察）\n{insights_text}\n\n\
         ## 近期决策记录\n{decisions_text}\n\n\
         ## 历史战略洞察\n{past_text}\n\n\
         请输出 2-3 条结构性观察，每条一行。规则：\n\
         1. 不要重复 Coach 已发现的模式，要看到 Coach 看不到的东西\n\
         2. 关注「趋势」和「轨迹」，而非单次事件\n\
         3. 关注「价值观-行为」的一致性或偏离\n\
         4. 语气完全中性，像写学术论文的观察段落\n\
         5. 每条以「结构观察：」或「轨迹信号：」前缀开头\n\
         6. 如果有历史战略洞察，评估其是否仍然成立\n\
         7. 只输出观察内容，不要其他解释"
    );

    let strategy_guide = skills::load_section("sage-cognitive", "## Phase 5: STRATEGIZE");
    let system = format!(
        "{strategy_guide}\n\n\
         ## 输出要求\n\
         纯文本列表，每行一条观察，以「结构观察：」或「轨迹信号：」前缀开头。\n\
         最多 3 条。少即是多。"
    );

    let resp = agent.invoke(&prompt, Some(&system)).await?;

    let content = resp.text.trim();
    let mut saved = 0;
    if !content.is_empty() {
        for line in content.lines() {
            let line = line.trim().trim_start_matches('-').trim();
            if !line.is_empty() {
                if let Err(e) =
                    store.save_memory_with_visibility("strategy_insight", line, "strategist", 0.85, "subconscious")
                {
                    tracing::error!("Strategist: failed to save insight: {e}");
                } else {
                    saved += 1;
                }
            }
        }
        info!("Strategist: {saved} strategic insights saved");
    }

    // 记录去重标记
    store.record_suggestion("strategist", "weekly-strategy", &resp.text)?;

    Ok(saved > 0)
}
