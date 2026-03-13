use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::store::Store;

/// 观察者：读取 raw observations，加上语义上下文（频率、关联），输出 observer_note
/// 只做「看见」，不做「理解」。不归档 observations（留给 Coach）。
pub async fn annotate(agent: &Agent, store: &Store) -> Result<bool> {
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
    let history_text = if history.is_empty() {
        "（无历史记录）".to_string()
    } else {
        history
            .iter()
            .map(|(cat, obs)| format!("- [{}] {}", cat, obs))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        "## 待标注的原始记录\n{obs_text}\n\n\
         ## 近期历史（用于判断频率和关联）\n{history_text}\n\n\
         请为每条原始记录输出一行语义标注。规则：\n\
         1. 每条原始记录对应一行输出\n\
         2. 格式：原始内容 ← 语义上下文\n\
         3. 语义上下文举例：本周第N次、今天第N封同类邮件、在X分钟内触发Y次、\
            与[某事]时间接近可能有关联、首次出现\n\
         4. 只输出标注行，不要编号、不要解释"
    );

    let system = "你是 Sage 的观察者。你只描述「发生了什么」，不评价、不分析模式、不给建议。\
                  你的工作是为原始事件添加频率和上下文信息，让后续分析者能看到更完整的画面。";

    let resp = agent.invoke(&prompt, Some(system)).await?;

    let mut saved = 0;
    for line in resp.text.lines() {
        let line = line.trim().trim_start_matches('-').trim();
        if !line.is_empty() {
            if let Err(e) = store.save_memory_with_visibility("observer_note", line, "observer", 0.6, "subconscious") {
                tracing::error!("Observer: failed to save note: {e}");
            } else {
                saved += 1;
            }
        }
    }

    info!("Observer: saved {saved} annotated notes");
    Ok(saved > 0)
}
