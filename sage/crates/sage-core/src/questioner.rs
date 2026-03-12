use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::skills;
use crate::store::Store;

/// 发问者：基于行为模式和近期决策，生成苏格拉底式深度问题
/// 支持问题追踪：新问题存入 open_questions，到期问题重新浮现
pub async fn ask(agent: &Agent, store: &Store) -> Result<bool> {
    // 1. 先检查是否有到期需要重新浮现的问题
    let due = store.get_due_questions(1)?;
    if let Some((q_id, question_text, ask_count)) = due.into_iter().next() {
        // 每日最多生成一次
        if store.has_recent_suggestion("questioner", "daily-question") {
            return Ok(false);
        }

        // 重新浮现：以变体形式再次提出
        let prompt = format!(
            "以下是一个之前提出但尚未被回答的深度问题（第 {ask_count} 次提出）：\n\
             \"{question_text}\"\n\n\
             请用不同的角度或措辞重新表述这个问题，保持核心追问方向不变。\n\
             只输出一个问题，不要编号、不要解释。"
        );

        let resp = agent.invoke(&prompt, None).await?;
        store.record_suggestion("questioner", "daily-question", &resp.text)?;
        store.bump_question_ask(q_id)?;
        info!("Questioner: resurfaced question #{q_id} (ask #{ask_count})");
        return Ok(true);
    }

    // 2. 没有到期问题，生成新问题
    let insights = store.search_memories("coach_insight", 10)?;
    if insights.is_empty() {
        info!("Questioner: no coach_insight records found, skipping");
        return Ok(false);
    }

    if store.has_recent_suggestion("questioner", "daily-question") {
        info!("Questioner: daily question already generated, skipping");
        return Ok(false);
    }

    let insights_text = insights
        .iter()
        .map(|m| format!("- {}", m.content))
        .collect::<Vec<_>>()
        .join("\n");

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

    let question_guide = skills::load_section("sage-cognitive", "## Phase 3: QUESTION");
    let system = format!(
        "{question_guide}\n\n\
         ## 输出要求\n\
         只输出一个问题，不要编号、不要解释、不要引导语。"
    );
    let resp = agent.invoke(&prompt, Some(&system)).await?;

    // 存入 suggestions 并同时追踪到 open_questions
    let suggestion_id = store.record_suggestion("questioner", "daily-question", &resp.text)?;
    store.save_open_question(&resp.text, Some(suggestion_id))?;
    info!("Questioner: generated daily question and tracked in open_questions");

    Ok(true)
}
