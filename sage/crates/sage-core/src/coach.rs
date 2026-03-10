use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::skills;
use crate::store::Store;

/// 学习教练：读取未处理 observations → Claude 发现模式 → 保存 coach_insight → 归档
/// 不再写 sage.md，改用 store.save_coach_insight() 持久化洞察
pub async fn learn(agent: &Agent, store: &Store) -> Result<bool> {
    let observations = store.load_unprocessed_observations(50)?;
    if observations.is_empty() {
        info!("Coach: no unprocessed observations, skipping");
        return Ok(false);
    }

    info!("Coach: analyzing {} observations", observations.len());

    let obs_text = observations
        .iter()
        .map(|o| format!("- [{}] **{}**: {}", o.created_at, o.category, o.observation))
        .collect::<Vec<_>>()
        .join("\n");

    // 从 SQLite 读取最近的教练洞察，替代原来读 sage.md
    let existing_insights = store.search_memories("coach_insight", 10)?;
    let existing_text = if existing_insights.is_empty() {
        "（空，首次学习）".to_string()
    } else {
        existing_insights
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        "你是 Sage 的学习教练。分析以下原始观察记录，从中发现用户的行为模式、偏好和习惯。\n\n\
         ## 最近观察（未处理）\n{obs_text}\n\n\
         ## 当前认知（历史洞察）\n{existing_text}\n\n\
         请输出你新发现的核心洞察（每条一行，简洁）。规则：\n\
         1. 只输出新发现或需要更新的认知，不要重复已有内容\n\
         2. 每条认知以「行为模式：」「决策倾向：」「沟通偏好：」等前缀开头\n\
         3. 每条简洁一行，不要写长段落\n\
         4. 只输出洞察内容，不要其他解释",
    );

    let observe_guide = skills::load_section("sage-cognitive", "## Phase 1: OBSERVE");
    let system = format!(
        "{observe_guide}\n\n\
         ## 输出要求\n\
         纯文本列表，每行一条洞察，以「行为模式：」「决策倾向：」「沟通偏好：」等前缀开头。"
    );
    let resp = agent.invoke(&prompt, Some(&system)).await?;

    // 将洞察保存到 SQLite memories 表（替代原来写 sage.md）
    let content = resp.text.trim();
    if !content.is_empty() {
        // 每行作为一条独立的 coach_insight 保存
        for line in content.lines() {
            let line = line.trim().trim_start_matches('-').trim();
            if !line.is_empty() {
                if let Err(e) = store.save_coach_insight(line) {
                    tracing::error!("Coach: failed to save insight: {e}");
                }
            }
        }
        info!("Coach: {} insight lines saved to SQLite", content.lines().count());
    }

    // 归档已处理的 observations
    let ids: Vec<i64> = observations.iter().map(|o| o.id).collect();
    store.mark_observations_processed(&ids)?;
    info!("Coach: {} observations archived", ids.len());

    Ok(true)
}
