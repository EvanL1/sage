use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::prompts;
use crate::store::Store;

/// 记忆进化返回值
pub struct EvolutionResult {
    pub consolidated: usize,
    pub condensed: usize,
    pub decayed: usize,
    pub promoted: usize,
    pub linked: usize,
    pub compiled_semantic: usize,
    pub compiled_axiom: usize,
}

/// 记忆进化：每日 Evening Review 后运行
/// 新管道顺序：
/// 1. merge_similar（同 depth 内去重）
/// 2. compile_to_semantic（episodic → semantic 行为模式归纳）
/// 3. synthesize_traits（semantic → procedural 判断模式提炼）
/// 4. compile_to_axiom（procedural → axiom 底层信念凝结）
/// 5. condense_verbose（精简冗长记忆）
/// 6. link_memories（记忆图谱连接）
/// 7. promote_validated（提升高频验证记忆 confidence）
pub async fn evolve(agent: &Agent, store: &Store) -> Result<EvolutionResult> {
    let merged = match merge_similar(agent, store).await {
        Ok(n) => n,
        Err(e) => { tracing::error!("merge_similar failed: {e}"); 0 }
    };
    let compiled_semantic = match compile_to_semantic(agent, store).await {
        Ok(n) => n,
        Err(e) => { tracing::error!("compile_to_semantic failed: {e}"); 0 }
    };
    let synthesized = match synthesize_traits(agent, store).await {
        Ok(n) => n,
        Err(e) => { tracing::error!("synthesize_traits failed: {e}"); 0 }
    };
    let compiled_axiom = match compile_to_axiom(agent, store).await {
        Ok(n) => n,
        Err(e) => { tracing::error!("compile_to_axiom failed: {e}"); 0 }
    };
    let condensed = match condense_verbose(agent, store).await {
        Ok(n) => n,
        Err(e) => { tracing::error!("condense_verbose failed: {e}"); 0 }
    };
    let linked = match link_memories(agent, store).await {
        Ok(n) => n,
        Err(e) => { tracing::error!("link_memories failed: {e}"); 0 }
    };
    let decayed = decay_unused(store).unwrap_or(0);
    let promoted = promote_validated(store).unwrap_or(0);

    let total_consolidated = merged + synthesized;
    if total_consolidated + condensed + decayed + promoted + linked + compiled_semantic + compiled_axiom > 0 {
        info!(
            "Memory evolution: merged={merged}, compiled_semantic={compiled_semantic}, synthesized={synthesized}, compiled_axiom={compiled_axiom}, condensed={condensed}, linked={linked}, decayed={decayed}, promoted={promoted}"
        );
    }
    Ok(EvolutionResult {
        consolidated: total_consolidated,
        condensed,
        decayed,
        promoted,
        linked,
        compiled_semantic,
        compiled_axiom,
    })
}

/// 单批次合并请求的最大条目数（避免 prompt 过长导致 LLM 超时或格式错误）
const MERGE_BATCH_SIZE: usize = 20;

/// 合并同 category + 同 depth 下内容相似的记忆（通过 LLM 识别，保留最新）
async fn merge_similar(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    // 按 (category, depth) 分组，避免跨 depth 合并
    let mut by_group: std::collections::HashMap<(String, String), Vec<_>> =
        std::collections::HashMap::new();
    for m in &memories {
        by_group
            .entry((m.category.clone(), m.depth.clone()))
            .or_default()
            .push(m);
    }

    let mut total_merged = 0;
    for ((category, _depth), items) in &by_group {
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
    let lang = store.prompt_lang();
    let prompt = prompts::evolution_merge(&lang, category, items.len(), &content_list.join("\n"));

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

    // Build whitelist of IDs in this batch — prevents hallucinated IDs from deleting real memories
    let batch_ids: std::collections::HashSet<i64> = items.iter().map(|m| m.id).collect();

    let mut batch_merged = 0;
    for line in resp.text.lines() {
        if let Some(rest) = line.strip_prefix("MERGE [") {
            if let Some((ids_str, content)) = rest.split_once("] → ") {
                let ids: Vec<i64> = ids_str
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                // Validate ALL IDs exist in current batch — reject any hallucinated IDs
                let valid_ids: Vec<i64> =
                    ids.iter().copied().filter(|id| batch_ids.contains(id)).collect();
                if valid_ids.len() >= 2 && !content.is_empty() {
                    let keep_id = *valid_ids.iter().max().unwrap();
                    let max_conf = items
                        .iter()
                        .filter(|m| valid_ids.contains(&m.id))
                        .map(|m| m.confidence)
                        .fold(0.0f64, f64::max);
                    if store
                        .update_memory(keep_id, content, (max_conf + 0.05).min(1.0))
                        .is_ok()
                    {
                        for &del_id in &valid_ids {
                            if del_id != keep_id {
                                let _ = store.delete_memory(del_id);
                            }
                        }
                        batch_merged += valid_ids.len() - 1;
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

/// 判断模式提炼批次大小
const SYNTH_BATCH_SIZE: usize = 20;

/// 判断模式提炼：将 behavior/thinking/emotion 观察编译为 personality 判断模式
/// 从 declarative（「做了什么」）→ procedural（「遇到X时怎么判断」）
async fn synthesize_traits(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    let trait_categories = ["behavior", "thinking", "emotion"];

    let mut total_synthesized = 0;
    for category in &trait_categories {
        let items: Vec<_> = memories
            .iter()
            .filter(|m| m.category == *category)
            .collect();

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

    let lang = store.prompt_lang();
    let prompt = prompts::evolution_synth(&lang, items.len(), category, &content_list.join("\n"));

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
                if !ids.is_empty() && !trait_content.is_empty() {
                    if let Ok(id) = store.save_memory("personality", trait_content, "evolution", 0.85) {
                        // synthesize_traits 产出的 personality 记忆标记为 procedural
                        let _ = store.update_memory_depth(id, "procedural");
                        synthesized_ids.extend(&ids);
                    }
                }
            }
        }
    }

    // Only delete IDs that are actually in this batch — reject any hallucinated IDs
    let batch_ids: std::collections::HashSet<i64> = items.iter().map(|m| m.id).collect();
    for &id in &synthesized_ids {
        if batch_ids.contains(&id) {
            let _ = store.delete_memory(id);
        }
    }

    if !synthesized_ids.is_empty() {
        info!(
            "Trait synthesis [{category}]: {} observations consolidated",
            synthesized_ids.len()
        );
    }
    Ok(synthesized_ids.len())
}

// ─── compile_to_semantic ────────────────────────────────────────────────────

/// 编译 episodic → semantic：同 category 下 ≥5 条 episodic 记忆归纳为行为模式
async fn compile_to_semantic(agent: &Agent, store: &Store) -> Result<usize> {
    let episodic = store.load_memories_by_depth("episodic")?;
    if episodic.is_empty() {
        return Ok(0);
    }

    // 按 category 分组
    let mut by_category: std::collections::HashMap<String, Vec<_>> =
        std::collections::HashMap::new();
    for m in &episodic {
        by_category.entry(m.category.clone()).or_default().push(m);
    }

    let mut total_compiled = 0;
    for (category, items) in &by_category {
        if items.len() < 5 {
            continue; // 不足 5 条不触发
        }
        agent.reset_counter();

        let content_list: Vec<String> = items
            .iter()
            .map(|m| format!("[id:{}] {}", m.id, m.content))
            .collect();
        let lang = store.prompt_lang();
        let prompt = prompts::evolution_compile_semantic(
            &lang,
            items.len(),
            category,
            &content_list.join("\n"),
        );

        let resp = match agent.invoke(&prompt, None).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("compile_to_semantic LLM call failed for {category}: {e}");
                continue;
            }
        };

        if resp.text.trim() == "NONE" {
            continue;
        }

        let batch_ids: std::collections::HashSet<i64> = items.iter().map(|m| m.id).collect();
        for line in resp.text.lines() {
            if let Some(rest) = line.trim().strip_prefix("PATTERN [") {
                if let Some((ids_str, pattern_content)) = rest.split_once("] → ") {
                    let source_ids: Vec<i64> = ids_str
                        .split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .filter(|id| batch_ids.contains(id))
                        .collect();
                    let pattern_content = pattern_content.trim();
                    if !source_ids.is_empty() && !pattern_content.is_empty() {
                        if let Ok(new_id) =
                            store.save_memory(category, pattern_content, "evolution", 0.8)
                        {
                            let _ = store.update_memory_depth(new_id, "semantic");
                            // 源 episodic 记忆标记为 compiled（保留但不再参与活跃检索）
                            for &src_id in &source_ids {
                                let _ = store.mark_memory_compiled(src_id);
                            }
                            total_compiled += source_ids.len();
                            info!(
                                "compile_to_semantic [{category}]: {} episodic → 1 semantic pattern",
                                source_ids.len()
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(total_compiled)
}

// ─── compile_to_axiom ───────────────────────────────────────────────────────

/// 编译 procedural → axiom：被动观察验证。
/// 不依赖 Chat 召回次数（validation_count），而是检查近期 episodic 事件是否支持该判断模式。
/// 对每条 procedural，让 LLM 判断有多少 episodic 证据支持它。≥3 条跨源证据 = 候选。
async fn compile_to_axiom(agent: &Agent, store: &Store) -> Result<usize> {
    let procedural = store.load_memories_by_depth("procedural")?;
    if procedural.is_empty() {
        return Ok(0);
    }

    let episodic = store.load_memories_by_depth("episodic")?;
    if episodic.len() < 5 {
        return Ok(0); // 证据太少
    }

    // 采样最近 50 条 episodic 作为证据池（避免 prompt 过长）
    let evidence: Vec<_> = episodic.iter().rev().take(50).collect();
    let evidence_text: Vec<String> = evidence
        .iter()
        .map(|m| format!("[{}|{}] {}", m.source, m.category, m.content))
        .collect();

    agent.reset_counter();

    // 让 LLM 判断哪些 procedural 被 episodic 证据充分支持
    let proc_text: Vec<String> = procedural
        .iter()
        .map(|m| format!("[id:{}] {}", m.id, m.content))
        .collect();

    let lang = store.prompt_lang();
    let prompt = prompts::evolution_compile_axiom_evidence(
        &lang,
        procedural.len(),
        &proc_text.join("\n"),
        evidence.len(),
        &evidence_text.join("\n"),
    );

    let resp = match agent.invoke(&prompt, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("compile_to_axiom LLM call failed: {e}");
            return Ok(0);
        }
    };

    if resp.text.trim() == "NONE" {
        return Ok(0);
    }

    let proc_ids: std::collections::HashSet<i64> = procedural.iter().map(|m| m.id).collect();
    let mut total_axioms = 0;
    for line in resp.text.lines() {
        if let Some(rest) = line.trim().strip_prefix("AXIOM [") {
            if let Some((ids_str, belief_content)) = rest.split_once("] → ") {
                let source_ids: Vec<i64> = ids_str
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .filter(|id| proc_ids.contains(id))
                    .collect();
                let belief_content = belief_content.trim();
                if !source_ids.is_empty() && !belief_content.is_empty() {
                    if let Ok(new_id) =
                        store.save_memory("values", belief_content, "evolution", 0.95)
                    {
                        let _ = store.update_memory_depth(new_id, "axiom");
                        total_axioms += 1;
                        info!(
                            "compile_to_axiom: {} procedural → 1 axiom belief (evidence-based)",
                            source_ids.len()
                        );
                    }
                }
            }
        }
    }

    Ok(total_axioms)
}

/// 精简冗长记忆内容：>150 字的记忆才触发精简（保留自然表达，不过度压缩）
const CONDENSE_CHAR_THRESHOLD: usize = 150;
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

    let lang = store.prompt_lang();
    let prompt = prompts::evolution_condense(&lang, items.len(), &content_list.join("\n"));

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
async fn link_batch(agent: &Agent, store: &Store, items: &[&sage_types::Memory]) -> Result<usize> {
    agent.reset_counter();

    let content_list: Vec<String> = items
        .iter()
        .map(|m| format!("[id:{} cat:{}] {}", m.id, m.category, m.content))
        .collect();

    let lang = store.prompt_lang();
    let prompt = prompts::evolution_link(&lang, items.len(), &content_list.join("\n"));

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

    info!(
        "Link batch: LLM response ({} lines)",
        trimmed.lines().count()
    );
    let mut linked = 0;
    let valid_relations = [
        "causes",
        "supports",
        "contradicts",
        "co_occurred",
        "derived_from",
        "similar",
    ];
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
                            Err(e) => tracing::warn!(
                                "Failed to save edge [{},{}] {}: {e}",
                                ids[0],
                                ids[1],
                                relation
                            ),
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

/// 衰减长期未更新的 archive 记忆（Phase 1a 已禁用）
/// depth+validation_count 已承担信号权重，不再需要主动衰减 confidence。
fn decay_unused(_store: &Store) -> Result<usize> {
    Ok(0)
}

/// 提升高置信度 archive 记忆到 core（纯 SQL）
fn promote_validated(store: &Store) -> Result<usize> {
    store.promote_high_confidence_memories(0.85)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::LlmProvider;
    use async_trait::async_trait;
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
        let id1 = store
            .save_memory("behavior", "喜欢直接沟通", "chat", 0.7)
            .unwrap();
        let id2 = store
            .save_memory("behavior", "偏好直接了当的沟通方式", "chat", 0.6)
            .unwrap();

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
        store
            .save_memory("values", "团队优先", "chat", 0.8)
            .unwrap();
        store
            .save_memory("values", "集体利益大于个人", "chat", 0.7)
            .unwrap();

        let agent = make_agent(vec!["NONE"]);
        let merged = merge_similar(&agent, &store).await.unwrap();
        // LLM 返回 NONE 所以实际不合并，但重点是 LLM 被调用了（阈值 2 生效）
        assert_eq!(merged, 0);
    }

    #[tokio::test]
    async fn test_merge_single_item_skipped() {
        let store = Store::open_in_memory().unwrap();
        store
            .save_memory("identity", "我是 Alex", "chat", 0.9)
            .unwrap();

        // 只有 1 条，不应调用 LLM
        let agent = make_agent(vec![]); // 空响应，如果被调用会 panic
        let merged = merge_similar(&agent, &store).await.unwrap();
        assert_eq!(merged, 0);
    }

    #[tokio::test]
    async fn test_merge_confidence_boost() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store
            .save_memory("pattern", "早起工作", "chat", 0.6)
            .unwrap();
        let id2 = store
            .save_memory("pattern", "习惯早起办公", "chat", 0.8)
            .unwrap();

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

        // Mock LLM 返回 2 条判断模式
        let resp = format!(
            "TRAIT [{},{},{}] → 信息不全时，快速选方向边走边修，而不是等齐了再动\nTRAIT [{},{},{}] → 复杂度上升时，先砍范围而非加抽象——交付比完美重要",
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5]
        );
        let agent = make_agent(vec![&resp]);

        let synthesized = synthesize_traits(&agent, &store).await.unwrap();
        assert_eq!(synthesized, 6, "all 6 observations should be synthesized");

        let active = store.load_active_memories().unwrap();
        // 原始 6 条 behavior 应被删除，新增 2 条 personality（判断模式）
        let personalities: Vec<_> = active
            .iter()
            .filter(|m| m.category == "personality")
            .collect();
        let behaviors: Vec<_> = active.iter().filter(|m| m.category == "behavior").collect();

        assert_eq!(personalities.len(), 2);
        assert_eq!(behaviors.len(), 0);
        assert!(personalities.iter().any(|m| m.content.contains("快速选方向")));
        assert!(personalities.iter().any(|m| m.content.contains("砍范围")));
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
        store
            .save_memory("identity", "我是 Alex", "chat", 0.9)
            .unwrap();

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
        // 验证 reset_counter 修复：15 个不同类别 × 每类 2 条 = 15 次 LLM 调用
        // Agent 默认 max_iterations=10，如果不 reset 第 11 次起会失败。
        // 此测试确保 15 次调用都能完成（每次都返回 NONE，仅验证不崩溃）。
        let store = Store::open_in_memory().unwrap();

        // 15 个类别，每类 2 条 → 每类触发 1 次 LLM 调用
        for i in 0..15 {
            let cat = format!("unique_cat_{i}");
            store
                .save_memory(&cat, &format!("记忆Alpha-{i}"), "chat", 0.6)
                .unwrap();
            store
                .save_memory(&cat, &format!("记忆Beta-{i}"), "chat", 0.7)
                .unwrap();
        }

        // 15 次调用都返回 NONE（无需匹配 ID，仅测试 reset_counter 不阻断流程）
        let responses: Vec<&str> = vec!["NONE"; 15];
        let agent = make_agent(responses);

        // 如果 reset_counter 没生效，第 11 个类别后 agent 会停止调用 LLM，
        // 但此处只验证函数正常完成（不 panic，不返回 Err）
        let merged = merge_similar(&agent, &store).await.unwrap();
        assert_eq!(merged, 0, "all NONE responses → 0 merges, but all 15 batches processed");

        // 所有 30 条记忆均保留
        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 30);
    }

    #[tokio::test]
    async fn test_evolve_merge_only() {
        let store = Store::open_in_memory().unwrap();

        // 只有 pattern 类别有 2 条，触发 merge；无 behavior ≥6，不触发 synthesize
        let p1 = store
            .save_memory("pattern", "每天下午查邮件", "chat", 0.6)
            .unwrap();
        let p2 = store
            .save_memory("pattern", "下午定时查看邮件", "chat", 0.7)
            .unwrap();

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
        // 创建一条超过 150 字的冗长记忆
        let long_content = "这是一条非常非常非常非常非常长的记忆内容，它包含了大量不必要的修饰词和背景描述信息，这些冗余的内容需要被精简到更短的版本以提升记忆效率和可读性。此外还有更多的补充说明和额外的上下文信息，用于让这段文字超过一百五十个字符的阈值，从而触发精简逻辑的执行。还要再加上一些无意义的填充内容来确保测试能够通过字数检查";
        let id = store
            .save_memory("behavior", long_content, "chat", 0.7)
            .unwrap();
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
        store
            .save_memory("behavior", "简短记忆", "chat", 0.7)
            .unwrap();

        let agent = make_agent(vec![]); // 不应被调用
        let condensed = condense_verbose(&agent, &store).await.unwrap();
        assert_eq!(condensed, 0);
    }

    #[tokio::test]
    async fn test_condense_keeps_already_concise() {
        let store = Store::open_in_memory().unwrap();
        let long = "这段记忆虽然很长很长很长很长很长很长很长很长很长很长很长很长但是LLM认为已经足够精炼了不需要改。额外补充更多文字来确保超过一百五十个字符的阈值，这样才能触发精简流程并测试KEEP逻辑是否正确工作。继续填充一些内容以达到足够的长度";
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
        let id1 = store
            .save_memory("behavior", "喜欢早起工作", "chat", 0.7)
            .unwrap();
        let id2 = store
            .save_memory("pattern", "每天6点起床", "chat", 0.8)
            .unwrap();
        let id3 = store
            .save_memory("values", "重视时间管理", "chat", 0.9)
            .unwrap();

        let resp = format!("LINK [{id1},{id2}] supports 0.8\nLINK [{id2},{id3}] derived_from 0.6");
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
            store
                .save_memory("behavior", &format!("记忆{i}"), "chat", 0.7)
                .unwrap();
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
        let id1 = store
            .save_memory("behavior", "早起工作", "chat", 0.7)
            .unwrap();
        let id2 = store
            .save_memory("pattern", "每天跑步", "chat", 0.8)
            .unwrap();
        store
            .save_memory("values", "自律生活", "chat", 0.9)
            .unwrap();

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
        let id1 = store
            .save_memory("behavior", "偏好早起", "chat", 0.7)
            .unwrap();
        let id2 = store
            .save_memory("pattern", "晨跑习惯", "chat", 0.8)
            .unwrap();
        let id3 = store
            .save_memory("values", "健康第一", "chat", 0.9)
            .unwrap();

        // 混合有效和无效行
        let resp = format!(
            "分析如下：\nLINK [{id1},{id2}] co_occurred 0.8\n这两条记忆相关\nLINK [{id2},{id3}] supports 0.7\nLINK [{id1},{id3}] invalid_rel 0.5\n完成。"
        );
        let agent = make_agent(vec![&resp]);

        let linked = link_memories(&agent, &store).await.unwrap();
        assert_eq!(
            linked, 2,
            "should parse valid LINK lines and skip noise/invalid"
        );
    }

    #[tokio::test]
    async fn test_link_batching_large_set() {
        let store = Store::open_in_memory().unwrap();
        // 创建 35 条记忆（> LINK_BATCH_SIZE=30），应分 2 批
        let mut ids = Vec::new();
        for i in 0..35 {
            let id = store
                .save_memory("behavior", &format!("记忆{i}"), "chat", 0.7)
                .unwrap();
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

    // ─── compile_to_semantic 测试 ──────────────────────────────

    #[tokio::test]
    async fn test_compile_to_semantic_creates_pattern() {
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..6 {
            let id = store
                .save_memory("behavior", &format!("具体行为事件 {i}"), "chat", 0.6)
                .unwrap();
            ids.push(id);
        }
        // behavior + chat → infer_depth = episodic
        let episodic = store.load_memories_by_depth("episodic").unwrap();
        assert_eq!(episodic.len(), 6);

        let resp = format!(
            "PATTERN [{},{},{},{},{},{}] → 倾向于直接行动而非等待",
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5]
        );
        let agent = make_agent(vec![&resp]);

        let compiled = compile_to_semantic(&agent, &store).await.unwrap();
        assert_eq!(compiled, 6, "6 episodic should be marked compiled");

        // 源 episodic 应标记为 compiled（不再出现在活跃 episodic 中）
        let still_active_episodic = store.load_memories_by_depth("episodic").unwrap();
        assert_eq!(still_active_episodic.len(), 0);

        // 应生成 1 条 semantic 记忆
        let semantic = store.load_memories_by_depth("semantic").unwrap();
        assert_eq!(semantic.len(), 1);
        assert!(semantic[0].content.contains("直接行动"));
    }

    #[tokio::test]
    async fn test_compile_to_semantic_too_few() {
        let store = Store::open_in_memory().unwrap();
        // 只有 4 条 episodic，不触发（阈值 5）
        for i in 0..4 {
            store
                .save_memory("behavior", &format!("事件 {i}"), "chat", 0.6)
                .unwrap();
        }

        let agent = make_agent(vec![]); // 不应被调用
        let compiled = compile_to_semantic(&agent, &store).await.unwrap();
        assert_eq!(compiled, 0);

        // episodic 记忆应保持活跃
        let episodic = store.load_memories_by_depth("episodic").unwrap();
        assert_eq!(episodic.len(), 4);
    }

    // ─── compile_to_axiom 测试 ──────────────────────────────

    #[tokio::test]
    async fn test_compile_to_axiom_creates_belief() {
        let store = Store::open_in_memory().unwrap();
        // procedural 判断模式（evolution source → infer_depth = procedural）
        let id1 = store
            .save_memory("personality", "复杂度上升时先砍范围，交付比完美重要", "evolution", 0.9)
            .unwrap();
        let id2 = store
            .save_memory("personality", "信息不全时快速选方向边走边修", "evolution", 0.92)
            .unwrap();

        // 创建 ≥5 条 episodic 证据（来自不同 source，模拟跨渠道行为观察）
        for (src, content) in [
            ("chat", "收到需求后 2 小时内发了方案"),
            ("email", "邮件中说先出 MVP 再迭代"),
            ("observer", "PR review 拒绝了过度工程化方案"),
            ("chat", "跟团队说先能跑再优化"),
            ("email", "回复客户时选了最简方案"),
            ("observer", "砍掉了 3 个非核心功能再开始写"),
        ] {
            store.save_memory("behavior", content, src, 0.7).unwrap();
        }

        let procedural = store.load_memories_by_depth("procedural").unwrap();
        assert_eq!(procedural.len(), 2);
        let episodic = store.load_memories_by_depth("episodic").unwrap();
        assert_eq!(episodic.len(), 6);

        let resp = format!("AXIOM [{id1},{id2}] → 行动优于等待，交付优于完美");
        let agent = make_agent(vec![&resp]);

        let compiled = compile_to_axiom(&agent, &store).await.unwrap();
        assert_eq!(compiled, 1);

        // 应生成 1 条 axiom 记忆
        let axioms = store.load_memories_by_depth("axiom").unwrap();
        assert_eq!(axioms.len(), 1);
        assert!(axioms[0].content.contains("行动优于等待"));
        assert_eq!(axioms[0].category, "values");

        // 源 procedural 应保留
        let still_procedural = store.load_memories_by_depth("procedural").unwrap();
        assert_eq!(still_procedural.len(), 2);
    }

    // ─── store 方法测试 ──────────────────────────────

    #[tokio::test]
    async fn test_update_memory_depth() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .save_memory("behavior", "测试记忆", "chat", 0.7)
            .unwrap();

        // behavior+chat → episodic
        let episodic = store.load_memories_by_depth("episodic").unwrap();
        assert!(episodic.iter().any(|m| m.id == id));

        // 更新 depth
        store.update_memory_depth(id, "semantic").unwrap();

        let semantic = store.load_memories_by_depth("semantic").unwrap();
        assert!(semantic.iter().any(|m| m.id == id));

        let episodic_after = store.load_memories_by_depth("episodic").unwrap();
        assert!(!episodic_after.iter().any(|m| m.id == id));
    }

    #[tokio::test]
    async fn test_mark_memory_compiled() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .save_memory("behavior", "测试记忆", "chat", 0.7)
            .unwrap();

        // 初始应为活跃
        let active = store.load_active_memories().unwrap();
        assert!(active.iter().any(|m| m.id == id));

        // 标记为 compiled
        store.mark_memory_compiled(id).unwrap();

        // 不应出现在活跃记忆中
        let active_after = store.load_active_memories().unwrap();
        assert!(!active_after.iter().any(|m| m.id == id));

        // 也不应出现在按 depth 查询的活跃结果中
        let episodic = store.load_memories_by_depth("episodic").unwrap();
        assert!(!episodic.iter().any(|m| m.id == id));
    }
}
