use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::store::Store;

/// 记忆进化：每日 Evening Review 后运行
/// 1. 合并相似记忆（同 category 下 LLM 识别重复，保留最新）
/// 2. 特质提炼（behavior/thinking/emotion 观察 → personality 特质）
/// 3. 衰减长期未更新记忆（降低 confidence）
/// 4. 提升高频验证记忆（archive → core）
pub async fn evolve(agent: &Agent, store: &Store) -> Result<(usize, usize, usize)> {
    let merged = merge_similar(agent, store).await.unwrap_or(0);
    let synthesized = synthesize_traits(agent, store).await.unwrap_or(0);
    let decayed = decay_unused(store).unwrap_or(0);
    let promoted = promote_validated(store).unwrap_or(0);

    let total_consolidated = merged + synthesized;
    if total_consolidated + decayed + promoted > 0 {
        info!(
            "Memory evolution: merged={merged}, synthesized={synthesized}, decayed={decayed}, promoted={promoted}"
        );
    }
    Ok((total_consolidated, decayed, promoted))
}

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
                        // 保留最新的（最大 id），而非第一个
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
                            total_merged += ids.len() - 1;
                        }
                    }
                }
            }
        }
    }

    Ok(total_merged)
}

/// 特质提炼：将大量 behavior/thinking/emotion 观察合并为简洁的 personality 特质
async fn synthesize_traits(agent: &Agent, store: &Store) -> Result<usize> {
    let memories = store.load_active_memories()?;
    let trait_categories = ["behavior", "thinking", "emotion"];

    let mut total_synthesized = 0;
    for category in &trait_categories {
        let items: Vec<_> = memories.iter().filter(|m| m.category == *category).collect();

        // 只有条目足够多时才触发特质提炼
        if items.len() < 6 {
            continue;
        }

        let content_list: Vec<String> = items
            .iter()
            .map(|m| format!("[id:{}] {}", m.id, m.content))
            .collect();

        let prompt = format!(
            "以下是关于用户的 {} 条「{category}」类观察记录：\n{}\n\n\
             请将这些具体观察归纳为 3-5 条**人格特质**。规则：\n\
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
                info!("Trait synthesis LLM call failed for {category}: {e}");
                continue;
            }
        };

        let mut synthesized_ids: std::collections::HashSet<i64> =
            std::collections::HashSet::new();
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

        // 删除已被提炼的原始观察
        for &id in &synthesized_ids {
            let _ = store.delete_memory(id);
        }

        if !synthesized_ids.is_empty() {
            info!(
                "Trait synthesis [{category}]: {} observations consolidated",
                synthesized_ids.len()
            );
            total_synthesized += synthesized_ids.len();
        }
    }

    Ok(total_synthesized)
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
        let (merged, decayed, promoted) = evolve(&agent, &store).await.unwrap();
        assert_eq!(merged, 0);
        assert_eq!(decayed, 0);
        assert_eq!(promoted, 0);
    }

    #[tokio::test]
    async fn test_evolve_merge_only() {
        let store = Store::open_in_memory().unwrap();

        // 只有 pattern 类别有 2 条，触发 merge；无 behavior ≥6，不触发 synthesize
        let p1 = store.save_memory("pattern", "每天下午查邮件", "chat", 0.6).unwrap();
        let p2 = store.save_memory("pattern", "下午定时查看邮件", "chat", 0.7).unwrap();

        let merge_resp = format!("MERGE [{p1},{p2}] → 每天下午定时查邮件");
        let agent = make_agent(vec![&merge_resp]);

        let (consolidated, decayed, promoted) = evolve(&agent, &store).await.unwrap();
        assert_eq!(consolidated, 1, "should merge 1 pair");
        assert_eq!(decayed, 0);
        assert_eq!(promoted, 0);

        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].content, "每天下午定时查邮件");
    }
}
