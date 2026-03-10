use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::applescript;
use crate::skills;
use crate::store::Store;

/// 镜子角色：从 SQLite coach_insight 记忆中挑选一个行为模式，温和地反映给用户（每天最多一次）
/// 不再读 sage.md，改用 store.search_memories("coach_insight", 5) 获取最近洞察
pub async fn reflect(agent: &Agent, store: &Store) -> Result<bool> {
    // 读取最近的教练洞察（替代原来读 sage.md）
    let insights = store.search_memories("coach_insight", 5)?;
    if insights.is_empty() {
        info!("Mirror: no coach_insight records found, skipping");
        return Ok(false);
    }

    let insights_text = insights
        .iter()
        .map(|m| format!("- {}", m.content))
        .collect::<Vec<_>>()
        .join("\n");

    // 内容过短则跳过
    if insights_text.len() < 20 {
        info!("Mirror: insight content too short, skipping");
        return Ok(false);
    }

    // 去重：今天已经反映过则跳过
    if store.has_recent_suggestion("mirror", "daily-reflection") {
        info!("Mirror: already reflected today, skipping");
        return Ok(false);
    }

    info!("Mirror: generating reflection from {} coach insights", insights.len());

    let prompt = format!(
        "以下是关于用户行为模式的记录：\n\n{insights_text}\n\n\
         请从中挑选**一个**最值得关注的模式，用温和、非评判的语气写一句观察（1-2句中文）。\n\
         风格：像一位细心的朋友，轻轻说出你注意到的事情。\n\
         示例：「我注意到你这周做了3次类似的决定，似乎在某个方向上越来越确定。」\n\
         只输出那1-2句话，不要其他解释。"
    );

    let reflect_guide = skills::load_section("sage-cognitive", "## Phase 2: REFLECT");
    let system = format!(
        "{reflect_guide}\n\n\
         ## 输出要求\n\
         只输出 1-2 句中文观察，不要其他解释。"
    );
    let resp = agent.invoke(&prompt, Some(&system)).await?;

    let reflection = resp.text.trim().to_string();
    if reflection.is_empty() {
        info!("Mirror: empty response from agent, skipping");
        return Ok(false);
    }

    // 记录本次反映（用于去重）
    store.record_suggestion("mirror", "daily-reflection", &reflection)?;
    info!("Mirror: reflection recorded");

    // 通知用户
    applescript::notify("Sage 观察", &reflection).await?;

    Ok(true)
}
