use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::store::Store;

/// 记忆进化：每日 Evening Review 后运行
/// 1. 合并相似记忆（同 category 下 LLM 识别重复）
/// 2. 衰减长期未更新记忆（降低 confidence）
/// 3. 提升高频验证记忆（archive → core）
pub async fn evolve(agent: &Agent, store: &Store) -> Result<(usize, usize, usize)> {
    let merged = merge_similar(agent, store).await.unwrap_or(0);
    let decayed = decay_unused(store).unwrap_or(0);
    let promoted = promote_validated(store).unwrap_or(0);

    if merged + decayed + promoted > 0 {
        info!(
            "Memory evolution: merged={merged}, decayed={decayed}, promoted={promoted}"
        );
    }
    Ok((merged, decayed, promoted))
}

/// 合并同 category 下内容相似的记忆（通过 LLM 识别）
async fn merge_similar(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    let mut by_category: std::collections::HashMap<String, Vec<_>> =
        std::collections::HashMap::new();
    for m in &memories {
        by_category.entry(m.category.clone()).or_default().push(m);
    }

    let mut total_merged = 0;
    for (category, items) in &by_category {
        // 只处理条目较多的类别，避免浪费 LLM 调用
        if items.len() < 4 {
            continue;
        }

        let content_list: Vec<String> = items
            .iter()
            .map(|m| format!("[id:{}] {}", m.id, m.content))
            .collect();
        let prompt = format!(
            "以下是分类「{category}」下的 {} 条记忆：\n{}\n\n\
             识别内容重复或高度相似的条目组。\n\
             对每组输出一行：MERGE [id1,id2,...] → 合并后的内容\n\
             如果没有可合并的，只输出 NONE",
            items.len(),
            content_list.join("\n")
        );

        let resp = match agent.invoke(&prompt, None).await {
            Ok(r) => r,
            Err(e) => {
                info!("Memory merge LLM call failed for {category}: {e}");
                continue;
            }
        };

        if resp.text.trim() == "NONE" {
            continue;
        }

        for line in resp.text.lines() {
            if let Some(rest) = line.strip_prefix("MERGE [") {
                if let Some((ids_str, content)) = rest.split_once("] → ") {
                    let ids: Vec<i64> = ids_str
                        .split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .collect();
                    if ids.len() >= 2 && !content.is_empty() {
                        let keep_id = ids[0];
                        let max_conf = items
                            .iter()
                            .filter(|m| ids.contains(&m.id))
                            .map(|m| m.confidence)
                            .fold(0.0f64, f64::max);
                        // 合并：保留第一个，提升置信度，删除其他
                        if store
                            .update_memory(keep_id, content, (max_conf + 0.05).min(1.0))
                            .is_ok()
                        {
                            for &del_id in &ids[1..] {
                                let _ = store.delete_memory(del_id);
                            }
                            total_merged += ids.len() - 1;
                        }
                    }
                }
            }
        }
    }

    Ok(total_merged)
}

/// 衰减长期未更新的 archive 记忆（纯 SQL，不消耗 LLM）
fn decay_unused(store: &Store) -> Result<usize> {
    store.decay_stale_archive_memories(60, 0.1, 0.2)
}

/// 提升高置信度 archive 记忆到 core（纯 SQL）
fn promote_validated(store: &Store) -> Result<usize> {
    store.promote_high_confidence_memories(0.85)
}
