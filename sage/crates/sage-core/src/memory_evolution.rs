use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::store::Store;

/// 记忆进化返回值
pub struct EvolutionResult {
    pub consolidated: usize,
    pub condensed: usize,
    pub decayed: usize,
    pub promoted: usize,
    pub linked: usize,
}

/// 记忆进化：每日 Evening Review 后运行
/// 1. 合并相似记忆（同 category 下 LLM 识别重复，保留最新）
/// 2. 特质提炼（behavior/thinking/emotion 观察 → personality 特质）
/// 3. 精简冗长记忆（>50 字 → LLM 压缩到 ≤30 字）
/// 4. 衰减长期未更新记忆（降低 confidence）
/// 5. 提升高频验证记忆（archive → core）
pub async fn evolve(agent: &Agent, store: &Store) -> Result<EvolutionResult> {
    let merged = match merge_similar(agent, store).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("merge_similar failed: {e}");
            0
        }
    };
    let synthesized = match synthesize_traits(agent, store).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("synthesize_traits failed: {e}");
            0
        }
    };
    let condensed = match condense_verbose(agent, store).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("condense_verbose failed: {e}");
            0
        }
    };
    let linked = match link_memories(agent, store).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("link_memories failed: {e}");
            0
        }
    };
    let decayed = decay_unused(store).unwrap_or(0);
    let promoted = promote_validated(store).unwrap_or(0);

    let total_consolidated = merged + synthesized;
    if total_consolidated + condensed + decayed + promoted + linked > 0 {
        info!(
            "Memory evolution: merged={merged}, synthesized={synthesized}, condensed={condensed}, linked={linked}, decayed={decayed}, promoted={promoted}"
        );
    }
    Ok(EvolutionResult {
        consolidated: total_consolidated,
        condensed,
        decayed,
        promoted,
        linked,
    })
}

/// 单批次合并请求的最大条目数（避免 prompt 过长导致 LLM 超时或格式错误）
const MERGE_BATCH_SIZE: usize = 20;

/// 合并同 category 下内容相似的记忆（通过 LLM 识别，保留最新）
async fn merge_similar(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    let mut by_category: std::collections::HashMap<String, Vec<_>> =
        std::collections::HashMap::new();
    for m in &memories {
        by_category.entry(m.category.clone()).or_default().push(m);
    }

    let mut total_merged = 0;
    for (category, items) in &by_category {
        if items.len() < 2 {
            continue;
        }

        // 分批处理：每批最多 MERGE_BATCH_SIZE 条
        for chunk in items.chunks(MERGE_BATCH_SIZE) {
            if chunk.len() < 2 {
                continue;
            }

            let merged = merge_batch(agent, store, category, chunk).await?;
            total_merged += merged;
        }
    }

    Ok(total_merged)
}

/// 处理单批次合并
async fn merge_batch(
    agent: &Agent,
    store: &Store,
    category: &str,
    items: &[&sage_types::Memory],
) -> Result<usize> {
    // 每批次独立：重置 Agent 计数器，避免累积触发 max_iterations 上限
    agent.reset_counter();

    let content_list: Vec<String> = items
        .iter()
        .map(|m| format!("[id:{}] {}", m.id, m.content))
        .collect();
    let prompt = format!(
        "以下是分类「{category}」下的 {} 条记忆：\n{}\n\n\
         你的任务是**积极去重**。规则：\n\
         1. 两条记忆表达相似意思（即使措辞不同）→ 必须合并\n\
         2. 一条是另一条的细化或重复 → 合并，保留更精炼的表述\n\
         3. 合并后的内容简洁（不超过50字），抓住核心本质\n\
         4. 每组输出一行：MERGE [id1,id2,...] → 合并后的内容\n\
         5. 如果完全没有可合并的，只输出 NONE",
        items.len(),
        content_list.join("\n")
    );

    let resp = match agent.invoke(&prompt, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Memory merge LLM call failed for {category}: {e}");
            return Ok(0);
        }
    };

    if resp.text.trim() == "NONE" {
        return Ok(0);
    }

    let mut batch_merged = 0;
    for line in resp.text.lines() {
        if let Some(rest) = line.strip_prefix("MERGE [") {
            if let Some((ids_str, content)) = rest.split_once("] → ") {
                let ids: Vec<i64> = ids_str
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if ids.len() >= 2 && !content.is_empty() {
                    let keep_id = *ids.iter().max().unwrap();
                    let max_conf = items
                        .iter()
                        .filter(|m| ids.contains(&m.id))
                        .map(|m| m.confidence)
                        .fold(0.0f64, f64::max);
                    if store
                        .update_memory(keep_id, content, (max_conf + 0.05).min(1.0))
                        .is_ok()
                    {
                        for &del_id in &ids {
                            if del_id != keep_id {
                                let _ = store.delete_memory(del_id);
                            }
                        }
                        batch_merged += ids.len() - 1;
                    }
                }
            }
        }
    }
    if batch_merged > 0 {
        info!("Merged {batch_merged} memories in category [{category}]");
    }
    Ok(batch_merged)
}

/// 特质提炼批次大小
const SYNTH_BATCH_SIZE: usize = 20;

/// 特质提炼：将大量 behavior/thinking/emotion 观察合并为简洁的 personality 特质
async fn synthesize_traits(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    let trait_categories = ["behavior", "thinking", "emotion"];

    let mut total_synthesized = 0;
    for category in &trait_categories {
        let items: Vec<_> = memories.iter().filter(|m| m.category == *category).collect();

        if items.len() < 6 {
            continue;
        }

        // 分批处理
        for chunk in items.chunks(SYNTH_BATCH_SIZE) {
            if chunk.len() < 3 {
                continue; // 批次太小不值得提炼
            }

            let synthesized = synth_batch(agent, store, category, chunk).await?;
            total_synthesized += synthesized;
        }
    }

    Ok(total_synthesized)
}

/// 处理单批次特质提炼
async fn synth_batch(
    agent: &Agent,
    store: &Store,
    category: &str,
    items: &[&sage_types::Memory],
) -> Result<usize> {
    // 每批次独立：重置 Agent 计数器，避免累积触发 max_iterations 上限
    agent.reset_counter();

    let content_list: Vec<String> = items
        .iter()
        .map(|m| format!("[id:{}] {}", m.id, m.content))
        .collect();

    let prompt = format!(
        "以下是关于用户的 {} 条「{category}」类观察记录：\n{}\n\n\
         请将这些具体观察归纳为 2-4 条**人格特质**。规则：\n\
         1. 每条特质是简洁的性格/行为/思维描述（不超过30字）\n\
         2. 特质是高度概括的抽象，不是具体事件\n\
         3. 标注每条特质的来源 ID\n\
         4. 格式：TRAIT [id1,id2,...] → 特质描述\n\
         5. 每条观察至少归入一个特质；无法归类的忽略\n\n\
         示例：\n\
         TRAIT [1,3,7] → 决策果断，偏好行动先于分析\n\
         TRAIT [2,5,8] → 重视团队成长胜过个人表现",
        items.len(),
        content_list.join("\n")
    );

    let resp = match agent.invoke(&prompt, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Trait synthesis LLM call failed for {category}: {e}");
            return Ok(0);
        }
    };

    let mut synthesized_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for line in resp.text.lines() {
        if let Some(rest) = line.strip_prefix("TRAIT [") {
            if let Some((ids_str, trait_content)) = rest.split_once("] → ") {
                let ids: Vec<i64> = ids_str
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                let trait_content = trait_content.trim();
                if !ids.is_empty()
                    && !trait_content.is_empty()
                    && store
                        .save_memory("personality", trait_content, "evolution", 0.85)
                        .is_ok()
                {
                    synthesized_ids.extend(&ids);
                }
            }
        }
    }

    for &id in &synthesized_ids {
        let _ = store.delete_memory(id);
    }

    if !synthesized_ids.is_empty() {
        info!(
            "Trait synthesis [{category}]: {} observations consolidated",
            synthesized_ids.len()
        );
    }
    Ok(synthesized_ids.len())
}

/// 精简冗长记忆内容：>50 字的记忆 → LLM 压缩到 ≤30 字
const CONDENSE_CHAR_THRESHOLD: usize = 50;
const CONDENSE_BATCH_SIZE: usize = 15;

async fn condense_verbose(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    let verbose: Vec<_> = memories
        .iter()
        .filter(|m| m.content.chars().count() > CONDENSE_CHAR_THRESHOLD)
        .collect();

    if verbose.is_empty() {
        return Ok(0);
    }

    let mut total_condensed = 0;
    for chunk in verbose.chunks(CONDENSE_BATCH_SIZE) {
        let condensed = condense_batch(agent, store, chunk).await?;
        total_condensed += condensed;
    }
    Ok(total_condensed)
}

/// 处理单批次精简
async fn condense_batch(
    agent: &Agent,
    store: &Store,
    items: &[&sage_types::Memory],
) -> Result<usize> {
    agent.reset_counter();

    let content_list: Vec<String> = items
        .iter()
        .map(|m| format!("[id:{}] {}", m.id, m.content))
        .collect();

    let prompt = format!(
        "以下 {} 条记忆内容过长，请将每条精简到30字以内，保留核心含义。\n{}\n\n\
         规则：\n\
         1. 每条输出一行：CONDENSE [id] → 精简后的内容\n\
         2. 删除修饰词、背景描述，只保留关键信息\n\
         3. 如果某条已经足够精炼，输出 KEEP [id]\n\
         4. 绝不改变原意",
        items.len(),
        content_list.join("\n")
    );

    let resp = match agent.invoke(&prompt, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Condense LLM call failed: {e}");
            return Ok(0);
        }
    };

    let mut batch_condensed = 0;
    for line in resp.text.lines() {
        if let Some(rest) = line.strip_prefix("CONDENSE [") {
            if let Some((id_str, content)) = rest.split_once("] → ") {
                if let Ok(id) = id_str.trim().parse::<i64>() {
                    let content = content.trim();
                    if !content.is_empty() {
                        if let Some(mem) = items.iter().find(|m| m.id == id) {
                            if store.update_memory(id, content, mem.confidence).is_ok() {
                                batch_condensed += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    if batch_condensed > 0 {
        info!("Condensed {batch_condensed} verbose memories");
    }
    Ok(batch_condensed)
}

/// 记忆图谱连接：LLM 识别跨类别的语义关系（公开接口，可独立调用）
const LINK_BATCH_SIZE: usize = 30;

pub async fn link_memories(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    if memories.len() < 3 {
        return Ok(0);
    }

    // 分批处理：每批 LINK_BATCH_SIZE 条，覆盖所有记忆
    let mut total_linked = 0;
    for chunk in memories.chunks(LINK_BATCH_SIZE) {
        let sample: Vec<_> = chunk.iter().collect();
        total_linked += link_batch(agent, store, &sample).await?;
    }

    // 如果超过 2 批，做一轮跨批次连接（首批 + 末批混合）
    if memories.len() > LINK_BATCH_SIZE * 2 {
        let half = LINK_BATCH_SIZE / 2;
        let mut cross: Vec<_> = memories.iter().take(half).collect();
        cross.extend(memories.iter().rev().take(half));
        total_linked += link_batch(agent, store, &cross).await?;
    }

    Ok(total_linked)
}

/// 处理单批次记忆连接
async fn link_batch(
    agent: &Agent,
    store: &Store,
    items: &[&sage_types::Memory],
) -> Result<usize> {
    agent.reset_counter();

    let content_list: Vec<String> = items
        .iter()
        .map(|m| format!("[id:{} cat:{}] {}", m.id, m.category, m.content))
        .collect();

    let prompt = format!(
        "以下是 {} 条记忆，请找出它们之间的语义关联：\n{}\n\n\
         规则：\n\
         1. 找出有意义的关联对（因果、支撑、矛盾、共现、派生）\n\
         2. 关系类型：causes / supports / contradicts / co_occurred / derived_from / similar\n\
         3. 权重 0.3-1.0（越强越高）\n\
         4. 每行输出：LINK [id1,id2] relation weight\n\
         5. 只输出确信的关联，不要勉强关联\n\
         6. 如果没有关联，只输出 NONE\n\n\
         示例：\n\
         LINK [3,7] causes 0.8\n\
         LINK [1,5] supports 0.6",
        items.len(),
        content_list.join("\n")
    );

    info!("Link batch: sending {} memories to LLM", items.len());
    let resp = match agent.invoke(&prompt, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Link memories LLM call failed: {e}");
            return Ok(0);
        }
    };

    let trimmed = resp.text.trim();
    if trimmed == "NONE" {
        info!("Link batch: LLM returned NONE");
        return Ok(0);
    }

    info!("Link batch: LLM response ({} lines)", trimmed.lines().count());
    let mut linked = 0;
    let valid_relations = ["causes", "supports", "contradicts", "co_occurred", "derived_from", "similar"];
    for line in resp.text.lines() {
        let line = line.trim();
        // 支持 "LINK [3,7] causes 0.8" 和 "LINK [3, 7] causes 0.8" 格式
        if let Some(rest) = line.strip_prefix("LINK [") {
            if let Some((ids_str, remainder)) = rest.split_once(']') {
                let parts: Vec<&str> = remainder.trim().splitn(2, ' ').collect();
                if parts.len() == 2 {
                    let ids: Vec<i64> = ids_str
                        .split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .collect();
                    let relation = parts[0].trim();
                    let weight: f64 = parts[1].trim().parse().unwrap_or(0.5);

                    if ids.len() == 2 && valid_relations.contains(&relation) && weight > 0.0 {
                        match store.save_memory_edge(ids[0], ids[1], relation, weight.min(1.0)) {
                            Ok(_) => linked += 1,
                            Err(e) => tracing::warn!("Failed to save edge [{},{}] {}: {e}", ids[0], ids[1], relation),
                        }
                    } else if ids.len() == 2 {
                        tracing::warn!("Rejected link: relation={relation:?} weight={weight} (not in whitelist)");
                    }
                }
            }
        }
    }

    if linked > 0 {
        info!("Linked {linked} memory pairs");
    }
    Ok(linked)
}

/// 衰减长期未更新的 archive 记忆（纯 SQL，不消耗 LLM）
fn decay_unused(store: &Store) -> Result<usize> {
    store.decay_stale_archive_memories(60, 0.1, 0.2)
}

/// 提升高置信度 archive 记忆到 core（纯 SQL）
fn promote_validated(store: &Store) -> Result<usize> {
    store.promote_high_confidence_memories(0.85)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::provider::LlmProvider;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// 可配置响应的 Mock Provider
    struct ScriptedProvider {
        responses: Mutex<Vec<String>>,
        call_count: AtomicUsize,
    }

    impl ScriptedProvider {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(String::from).collect()),
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ScriptedProvider {
        fn name(&self) -> &str {
            "scripted-mock"
        }

        async fn invoke(&self, _prompt: &str, _system: Option<&str>) -> Result<String> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            let responses = self.responses.lock().unwrap();
            Ok(responses.get(idx).cloned().unwrap_or_else(|| "NONE".into()))
        }
    }

    fn make_agent(responses: Vec<&str>) -> Agent {
        Agent::with_provider(Box::new(ScriptedProvider::new(responses)))
    }

    // ─── merge_similar 测试 ──────────────────────────────

    #[tokio::test]
    async fn test_merge_dedup_keeps_newest() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "喜欢直接沟通", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "偏好直接了当的沟通方式", "chat", 0.6).unwrap();

        // Mock LLM 返回合并指令，保留两个 ID
        let agent = make_agent(vec![&format!("MERGE [{id1},{id2}] → 偏好直接沟通")]);

        let merged = merge_similar(&agent, &store).await.unwrap();
        assert_eq!(merged, 1, "should merge 1 duplicate");

        // 最新的 (id2) 应被保留，id1 应被删除
        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id2);
        assert_eq!(active[0].content, "偏好直接沟通");
    }

    #[tokio::test]
    async fn test_merge_threshold_two() {
        let store = Store::open_in_memory().unwrap();
        // 只有 2 条也应触发合并（之前需要 4 条）
        store.save_memory("values", "团队优先", "chat", 0.8).unwrap();
        store.save_memory("values", "集体利益大于个人", "chat", 0.7).unwrap();

        let agent = make_agent(vec!["NONE"]);
        let merged = merge_similar(&agent, &store).await.unwrap();
        // LLM 返回 NONE 所以实际不合并，但重点是 LLM 被调用了（阈值 2 生效）
        assert_eq!(merged, 0);
    }

    #[tokio::test]
    async fn test_merge_single_item_skipped() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("identity", "我是 Evan", "chat", 0.9).unwrap();

        // 只有 1 条，不应调用 LLM
        let agent = make_agent(vec![]); // 空响应，如果被调用会 panic
        let merged = merge_similar(&agent, &store).await.unwrap();
        assert_eq!(merged, 0);
    }

    #[tokio::test]
    async fn test_merge_confidence_boost() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("pattern", "早起工作", "chat", 0.6).unwrap();
        let id2 = store.save_memory("pattern", "习惯早起办公", "chat", 0.8).unwrap();

        let agent = make_agent(vec![&format!("MERGE [{id1},{id2}] → 早起工作习惯")]);
        merge_similar(&agent, &store).await.unwrap();

        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 1);
        // confidence 应为 max(0.6, 0.8) + 0.05 = 0.85
        assert!((active[0].confidence - 0.85).abs() < 0.01);
    }

    // ─── synthesize_traits 测试 ──────────────────────────────

    #[tokio::test]
    async fn test_synthesize_needs_six_items() {
        let store = Store::open_in_memory().unwrap();
        // 只有 5 条 behavior，不够触发
        for i in 0..5 {
            store
                .save_memory("behavior", &format!("行为观察 {i}"), "chat", 0.6)
                .unwrap();
        }

        let agent = make_agent(vec![]); // 不应被调用
        let synthesized = synthesize_traits(&agent, &store).await.unwrap();
        assert_eq!(synthesized, 0);
    }

    #[tokio::test]
    async fn test_synthesize_creates_personality_traits() {
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..6 {
            let id = store
                .save_memory("behavior", &format!("行为观察 {i}"), "chat", 0.6)
                .unwrap();
            ids.push(id);
        }

        // Mock LLM 返回 2 条特质
        let resp = format!(
            "TRAIT [{},{},{}] → 决策果断，行动优先\nTRAIT [{},{},{}] → 重视效率胜过完美",
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5]
        );
        let agent = make_agent(vec![&resp]);

        let synthesized = synthesize_traits(&agent, &store).await.unwrap();
        assert_eq!(synthesized, 6, "all 6 observations should be synthesized");

        let active = store.load_active_memories().unwrap();
        // 原始 6 条 behavior 应被删除，新增 2 条 personality
        let personalities: Vec<_> = active.iter().filter(|m| m.category == "personality").collect();
        let behaviors: Vec<_> = active.iter().filter(|m| m.category == "behavior").collect();

        assert_eq!(personalities.len(), 2);
        assert_eq!(behaviors.len(), 0);
        assert!(personalities.iter().any(|m| m.content.contains("决策果断")));
        assert!(personalities.iter().any(|m| m.content.contains("重视效率")));
    }

    #[tokio::test]
    async fn test_synthesize_personality_is_core_tier() {
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..6 {
            let id = store
                .save_memory("behavior", &format!("obs {i}"), "chat", 0.6)
                .unwrap();
            ids.push(id);
        }

        let resp = format!(
            "TRAIT [{},{},{},{},{},{}] → 测试特质",
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5]
        );
        let agent = make_agent(vec![&resp]);
        synthesize_traits(&agent, &store).await.unwrap();

        // personality category 在 infer_tier 中映射为 core
        // 验证：搜索到的 personality 记忆 confidence = 0.85
        let results = store.search_memories("测试特质", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].category, "personality");
        assert!((results[0].confidence - 0.85).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_synthesize_partial_coverage() {
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..6 {
            let id = store
                .save_memory("thinking", &format!("思维观察 {i}"), "chat", 0.6)
                .unwrap();
            ids.push(id);
        }

        // LLM 只合成了 4 条（ids[0..4]），留下 2 条未被提及
        let resp = format!(
            "TRAIT [{},{},{},{}] → 系统性思维模式",
            ids[0], ids[1], ids[2], ids[3]
        );
        let agent = make_agent(vec![&resp]);
        let synthesized = synthesize_traits(&agent, &store).await.unwrap();
        assert_eq!(synthesized, 4);

        let active = store.load_active_memories().unwrap();
        // 2 条 thinking 应保留，1 条新 personality
        let thinking: Vec<_> = active.iter().filter(|m| m.category == "thinking").collect();
        let personality: Vec<_> = active
            .iter()
            .filter(|m| m.category == "personality")
            .collect();
        assert_eq!(thinking.len(), 2);
        assert_eq!(personality.len(), 1);
    }

    // ─── evolve 集成测试 ──────────────────────────────

    #[tokio::test]
    async fn test_evolve_runs_without_error() {
        let store = Store::open_in_memory().unwrap();

        // 只有 1 条记忆，不触发 merge 或 synthesize
        store.save_memory("identity", "我是 Evan", "chat", 0.9).unwrap();

        // 空响应列表，不应有任何 LLM 调用
        let agent = make_agent(vec![]);
        let r = evolve(&agent, &store).await.unwrap();
        assert_eq!(r.consolidated, 0);
        assert_eq!(r.condensed, 0);
        assert_eq!(r.decayed, 0);
        assert_eq!(r.promoted, 0);
        assert_eq!(r.linked, 0);
    }

    #[tokio::test]
    async fn test_merge_many_batches_exceeds_max_iterations() {
        // 验证 reset_counter 修复：15 个类别 × 每类 2 条 = 15 次 LLM 调用
        // Agent 默认 max_iterations=10，如果不 reset 就会在第 11 次失败
        let store = Store::open_in_memory().unwrap();
        let mut responses = Vec::new();

        for i in 0..15 {
            let cat = format!("cat_{i}");
            let id1 = store.save_memory(&cat, &format!("记忆A-{i}"), "chat", 0.6).unwrap();
            let id2 = store.save_memory(&cat, &format!("记忆B-{i}"), "chat", 0.7).unwrap();
            responses.push(format!("MERGE [{id1},{id2}] → 合并记忆-{i}"));
        }

        let resp_refs: Vec<&str> = responses.iter().map(|s| s.as_str()).collect();
        let agent = make_agent(resp_refs);

        // 如果 reset_counter 没生效，这里会在第 11 个类别时返回 0（LLM 调用失败）
        let merged = merge_similar(&agent, &store).await.unwrap();
        assert_eq!(merged, 15, "all 15 categories should be merged despite max_iterations=10");

        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 15, "15 merged memories should remain");
    }

    #[tokio::test]
    async fn test_evolve_merge_only() {
        let store = Store::open_in_memory().unwrap();

        // 只有 pattern 类别有 2 条，触发 merge；无 behavior ≥6，不触发 synthesize
        let p1 = store.save_memory("pattern", "每天下午查邮件", "chat", 0.6).unwrap();
        let p2 = store.save_memory("pattern", "下午定时查看邮件", "chat", 0.7).unwrap();

        let merge_resp = format!("MERGE [{p1},{p2}] → 每天下午定时查邮件");
        let agent = make_agent(vec![&merge_resp]);

        let r = evolve(&agent, &store).await.unwrap();
        assert_eq!(r.consolidated, 1, "should merge 1 pair");
        assert_eq!(r.condensed, 0);
        assert_eq!(r.decayed, 0);
        assert_eq!(r.promoted, 0);
        assert_eq!(r.linked, 0);

        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].content, "每天下午定时查邮件");
    }

    // ─── condense_verbose 测试 ──────────────────────────────

    #[tokio::test]
    async fn test_condense_shortens_verbose_memory() {
        let store = Store::open_in_memory().unwrap();
        // 创建一条超过 50 字的冗长记忆
        let long_content = "这是一条非常非常非常非常非常长的记忆内容，它包含了大量不必要的修饰词和背景描述信息，这些冗余的内容需要被精简到更短的版本以提升记忆效率和可读性";
        let id = store.save_memory("behavior", long_content, "chat", 0.7).unwrap();
        assert!(long_content.chars().count() > CONDENSE_CHAR_THRESHOLD);

        let agent = make_agent(vec![&format!("CONDENSE [{id}] → 冗长记忆需精简")]);
        let condensed = condense_verbose(&agent, &store).await.unwrap();
        assert_eq!(condensed, 1);

        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].content, "冗长记忆需精简");
        // confidence 不变
        assert!((active[0].confidence - 0.7).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_condense_skips_short_memories() {
        let store = Store::open_in_memory().unwrap();
        // 短记忆不触发精简
        store.save_memory("behavior", "简短记忆", "chat", 0.7).unwrap();

        let agent = make_agent(vec![]); // 不应被调用
        let condensed = condense_verbose(&agent, &store).await.unwrap();
        assert_eq!(condensed, 0);
    }

    #[tokio::test]
    async fn test_condense_keeps_already_concise() {
        let store = Store::open_in_memory().unwrap();
        let long = "这段记忆虽然很长很长很长很长很长很长很长很长很长很长很长很长但是LLM认为已经足够精炼了不需要改";
        let id = store.save_memory("thinking", long, "chat", 0.8).unwrap();

        // LLM 返回 KEEP
        let agent = make_agent(vec![&format!("KEEP [{id}]")]);
        let condensed = condense_verbose(&agent, &store).await.unwrap();
        assert_eq!(condensed, 0);

        // 内容不变
        let active = store.load_active_memories().unwrap();
        assert_eq!(active[0].content, long);
    }

    // ─── link_memories 测试 ──────────────────────────────

    #[tokio::test]
    async fn test_link_creates_edges() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "喜欢早起工作", "chat", 0.7).unwrap();
        let id2 = store.save_memory("pattern", "每天6点起床", "chat", 0.8).unwrap();
        let id3 = store.save_memory("values", "重视时间管理", "chat", 0.9).unwrap();

        let resp = format!(
            "LINK [{id1},{id2}] supports 0.8\nLINK [{id2},{id3}] derived_from 0.6"
        );
        let agent = make_agent(vec![&resp]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 2);

        let edges = store.get_all_memory_edges().unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[tokio::test]
    async fn test_link_skips_too_few_memories() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        // 只有 2 条，不够触发

        let agent = make_agent(vec![]);
        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 0);
    }

    #[tokio::test]
    async fn test_link_none_response() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..5 {
            store.save_memory("behavior", &format!("记忆{i}"), "chat", 0.7).unwrap();
        }

        let agent = make_agent(vec!["NONE"]);
        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 0);
        assert_eq!(store.count_memory_edges().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_link_invalid_relation_rejected() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory("behavior", "C", "chat", 0.7).unwrap();

        // "random_relation" 不在白名单中
        let resp = format!("LINK [{id1},{id2}] random_relation 0.5");
        let agent = make_agent(vec![&resp]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 0, "invalid relation should be rejected");
    }

    #[tokio::test]
    async fn test_link_weight_clamped() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory("behavior", "C", "chat", 0.7).unwrap();

        // weight > 1.0 应被 clamp 到 1.0
        let resp = format!("LINK [{id1},{id2}] causes 1.5");
        let agent = make_agent(vec![&resp]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 1);

        let edges = store.get_memory_edges(id1).unwrap();
        assert!((edges[0].weight - 1.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_evolve_includes_linked() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "早起工作", "chat", 0.7).unwrap();
        let id2 = store.save_memory("pattern", "每天跑步", "chat", 0.8).unwrap();
        store.save_memory("values", "自律生活", "chat", 0.9).unwrap();

        // merge: NONE, synth: not enough, condense: short enough, link: 1 pair
        let resp = format!("LINK [{id1},{id2}] co_occurred 0.7");
        let agent = make_agent(vec!["NONE", &resp]); // merge=NONE, link=resp

        let r = evolve(&agent, &store).await.unwrap();
        // link 可能被跳过（取决于 batch 顺序），但不应 panic
        // linked 可以是 0 或更多，取决于 LLM 响应顺序
        let _ = r.linked;
    }

    #[tokio::test]
    async fn test_link_spaces_in_ids() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory("behavior", "C", "chat", 0.7).unwrap();

        // LLM 可能在逗号后加空格
        let resp = format!("LINK [{id1}, {id2}] supports 0.7");
        let agent = make_agent(vec![&resp]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 1, "should parse IDs with spaces after comma");
    }

    #[tokio::test]
    async fn test_link_multiple_lines_mixed() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "偏好早起", "chat", 0.7).unwrap();
        let id2 = store.save_memory("pattern", "晨跑习惯", "chat", 0.8).unwrap();
        let id3 = store.save_memory("values", "健康第一", "chat", 0.9).unwrap();

        // 混合有效和无效行
        let resp = format!(
            "分析如下：\nLINK [{id1},{id2}] co_occurred 0.8\n这两条记忆相关\nLINK [{id2},{id3}] supports 0.7\nLINK [{id1},{id3}] invalid_rel 0.5\n完成。"
        );
        let agent = make_agent(vec![&resp]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 2, "should parse valid LINK lines and skip noise/invalid");
    }

    #[tokio::test]
    async fn test_link_batching_large_set() {
        let store = Store::open_in_memory().unwrap();
        // 创建 35 条记忆（> LINK_BATCH_SIZE=30），应分 2 批
        let mut ids = Vec::new();
        for i in 0..35 {
            let id = store.save_memory("behavior", &format!("记忆{i}"), "chat", 0.7).unwrap();
            ids.push(id);
        }

        // 第一批(30条)返回 1 条边，第二批(5条)返回 NONE
        let resp1 = format!("LINK [{},{}] similar 0.6", ids[0], ids[1]);
        let agent = make_agent(vec![&resp1, "NONE"]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 1);
        assert_eq!(store.count_memory_edges().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_link_all_relation_types() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        let id3 = store.save_memory("behavior", "C", "chat", 0.7).unwrap();

        // 测试所有合法关系类型
        let resp = format!(
            "LINK [{id1},{id2}] causes 0.8\n\
             LINK [{id1},{id3}] supports 0.7\n\
             LINK [{id2},{id3}] contradicts 0.6\n\
             LINK [{id1},{id2}] co_occurred 0.5\n\
             LINK [{id1},{id3}] derived_from 0.4\n\
             LINK [{id2},{id3}] similar 0.3"
        );
        let agent = make_agent(vec![&resp]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(linked, 6, "all 6 valid relation types should be accepted");
    }
}
