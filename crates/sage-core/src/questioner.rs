use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::pipeline::{PipelineContext, QuestionerOutput};
use crate::prompts;
use crate::skills;
use crate::store::Store;

/// 发问者：基于行为模式和近期决策，生成苏格拉底式深度问题
/// 支持问题追踪：新问题存入 open_questions，到期问题重新浮现
pub async fn ask(agent: &Agent, store: &Store, ctx: &mut PipelineContext) -> Result<bool> {
    // 1. 先检查是否有到期需要重新浮现的问题
    let due = store.get_due_questions(1)?;
    if let Some((q_id, question_text, ask_count)) = due.into_iter().next() {
        // 每日最多生成一次
        if store.has_recent_suggestion("questioner", "daily-question") {
            return Ok(false);
        }

        // 重新浮现：以变体形式再次提出
        let lang = store.prompt_lang();
        let prompt = prompts::questioner_resurface(&lang, ask_count.max(0) as u32, &question_text);
        let resp = agent.invoke(&prompt, None).await?;
        store.record_suggestion("questioner", "daily-question", &resp.text)?;
        store.bump_question_ask(q_id)?;
        info!("Questioner: resurfaced question #{q_id} (ask #{ask_count})");
        ctx.questioner = Some(QuestionerOutput { question: resp.text, is_resurface: true });
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
    let lang = store.prompt_lang();
    let decisions_text = if decisions.is_empty() {
        if lang == "en" { "(none yet)".to_string() } else { "（暂无记录）".to_string() }
    } else {
        decisions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = prompts::questioner_new(&lang, &insights_text, &decisions_text);
    let question_guide = skills::load_section("sage-cognitive", "## Phase 3: QUESTION");
    let system = format!(
        "{question_guide}\n\n{}",
        prompts::questioner_system_suffix(&lang)
    );
    let resp = agent.invoke(&prompt, Some(&system)).await?;

    // 存入 suggestions 并同时追踪到 open_questions
    let suggestion_id = store.record_suggestion("questioner", "daily-question", &resp.text)?;
    store.save_open_question(&resp.text, Some(suggestion_id))?;
    info!("Questioner: generated daily question and tracked in open_questions");

    // 写入上下文
    ctx.questioner = Some(QuestionerOutput { question: resp.text, is_resurface: false });

    Ok(true)
}
