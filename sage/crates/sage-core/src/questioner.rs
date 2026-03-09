use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::store::Store;

/// 发问者：基于行为模式和近期决策，生成一个苏格拉底式深度问题
/// 问题静默存储（不发通知），用户在 Dashboard 自行发现
/// 不再读 sage.md + decisions.md，改用 Store 查询 coach_insight 和 decision 类别记忆
pub async fn ask(agent: &Agent, store: &Store) -> Result<bool> {
    // 读取教练洞察（替代原来读 sage.md）
    let insights = store.search_memories("coach_insight", 10)?;
    if insights.is_empty() {
        info!("Questioner: no coach_insight records found, skipping");
        return Ok(false);
    }

    // 每日最多生成一次
    if store.has_recent_suggestion("questioner", "daily-question") {
        info!("Questioner: daily question already generated, skipping");
        return Ok(false);
    }

    let insights_text = insights
        .iter()
        .map(|m| format!("- {}", m.content))
        .collect::<Vec<_>>()
        .join("\n");

    // 近期决策记录（替代原来读 decisions.md）
    let decisions = store.search_memories("decision", 5)?;
    let decisions_text = if decisions.is_empty() {
        "（暂无记录）".to_string()
    } else {
        decisions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        "## 用户行为模式（教练洞察）\n{insights_text}\n\n\
         ## 近期决策记录\n{decisions_text}\n\n\
         请根据以上内容，生成一个苏格拉底式深度问题。要求：\n\
         1. 只输出一个问题，不要编号、不要解释、不要引导语\n\
         2. 问题要具体——指向观察到的某个具体模式或决策倾向，而非泛泛而谈\n\
         3. 触及价值观、动机或盲点，让人需要认真思考才能回答\n\
         4. 语气温暖、非评判，像一个信任的朋友在问",
    );

    let system = "你是苏格拉底式教练，擅长用一个问题打开自我认知的门。";
    let resp = agent.invoke(&prompt, Some(system)).await?;

    // 静默存储，不发通知
    store.record_suggestion("questioner", "daily-question", &resp.text)?;
    info!("Questioner: generated daily question");

    Ok(true)
}
