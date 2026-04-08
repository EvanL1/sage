use anyhow::{Context, Result};
use sage_types::Memory;

use super::Store;

/// UTF-8 安全截断：在 max_bytes 处找最近的字符边界
pub(super) fn utf8_safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn format_memory_lines(items: &[Memory]) -> String {
    items
        .iter()
        .map(|m| format!("- [{}] {}", m.category, m.content))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Store {
    /// 按认知深度分层构建 LLM 上下文（Lost in the Middle U 型注意力优化）
    ///
    /// 布局：axiom（开头高注意力）→ procedural → semantic → episodic（结尾高注意力）
    ///
    /// - `topic_hint`：当前事件的主题关键词，用于 semantic/episodic 层按相关性加载（而非纯时间）
    /// - 如果 depth 字段全为 episodic（旧数据未迁移），自动回退到 tier-based 查询。
    pub fn get_memory_context(&self, max_bytes: usize, topic_hint: Option<&str>) -> Result<String> {
        // 分段获取 conn 避免死锁：search_memories_by_depth 内部也会获取 conn
        let (has_deep, axiom_items, proc_items) = {
            let conn = self.conn()?;
            let has_deep: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE status = 'active' AND depth != 'episodic' LIMIT 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0) > 0;
            if !has_deep {
                return self.get_memory_context_tier_fallback(&conn, max_bytes);
            }
            let axiom_items = Self::query_memories_by(&conn, "depth = 'axiom' AND status = 'active'", 20)?;
            let proc_items = Self::query_memories_by(&conn, "depth = 'procedural' AND status = 'active'", 15)?;
            (has_deep, axiom_items, proc_items)
        }; // conn 释放

        let mut sections = Vec::new();

        if !axiom_items.is_empty() {
            let lines = format_memory_lines(&axiom_items);
            sections.push(format!("## 信念（始终有效）\n{lines}"));
        }

        if !proc_items.is_empty() {
            let lines = format_memory_lines(&proc_items);
            sections.push(format!("## 判断模式\n{lines}"));
        }

        // 3. 行为模式（semantic）— conn 已释放，search_memories_by_depth 可安全获取
        let sem_items = if let Some(hint) = topic_hint {
            self.search_memories_by_depth(hint, "semantic", 10)?
        } else {
            let conn = self.conn()?;
            Self::query_memories_by(&conn, "depth = 'semantic' AND status = 'active'", 10)?
        };
        if !sem_items.is_empty() {
            let section_label = if topic_hint.is_some() { "## 相关模式" } else { "## 行为模式" };
            let lines = format_memory_lines(&sem_items);
            sections.push(format!("{section_label}\n{lines}"));
        }

        // 4. 近期事件（episodic）— 有 topic_hint 时按相关性加载；过滤已过期
        let epi_items = if let Some(hint) = topic_hint {
            self.search_memories_by_depth(hint, "episodic", 5)?
                .into_iter()
                .filter(|m| {
                    m.valid_until.as_deref().map_or(true, |vu| {
                        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                        vu >= now.as_str()
                    })
                })
                .collect::<Vec<_>>()
        } else {
            let conn = self.conn()?;
            let sql = "SELECT id, category, content, source, confidence, visibility, \
                       created_at, updated_at, about_person, last_accessed_at, depth, \
                       valid_until, validation_count, derived_from, evolution_note, derivable \
                       FROM memories \
                       WHERE depth = 'episodic' AND status = 'active' \
                         AND (valid_until IS NULL OR valid_until > datetime('now')) \
                       ORDER BY updated_at DESC LIMIT 5";
            let mut stmt = conn.prepare(sql).context("准备 episodic 查询失败")?;
            let rows = stmt.query_map([], Self::row_to_memory)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if !epi_items.is_empty() {
            let section_label = if topic_hint.is_some() { "## 相关事件" } else { "## 近期事件" };
            let lines = format_memory_lines(&epi_items);
            sections.push(format!("{section_label}\n{lines}"));
        }

        let full = sections.join("\n\n");
        if full.len() <= max_bytes {
            Ok(full)
        } else {
            Ok(utf8_safe_truncate(&full, max_bytes).to_string())
        }
    }

    /// Fallback：旧数据（depth 全为 episodic）时用 tier 分层
    fn get_memory_context_tier_fallback(
        &self,
        conn: &rusqlite::Connection,
        max_bytes: usize,
    ) -> Result<String> {
        let mut sections = Vec::new();

        let core_items = Self::query_memories_by(conn, "tier = 'core' AND status = 'active'", 50)?;
        if !core_items.is_empty() {
            sections.push(format!("## 核心认知\n{}", format_memory_lines(&core_items)));
        }

        let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let working_where = format!(
            "tier = 'working' AND status = 'active' AND (expires_at IS NULL OR expires_at > '{now_str}')"
        );
        let working_items = Self::query_memories_by(conn, &working_where, 20)?;
        if !working_items.is_empty() {
            sections.push(format!(
                "## 当前任务与决策\n{}",
                format_memory_lines(&working_items)
            ));
        }

        let archive_items =
            Self::query_memories_by(conn, "tier = 'archive' AND status = 'active'", 30)?;
        if !archive_items.is_empty() {
            sections.push(format!("## 行为洞察\n{}", format_memory_lines(&archive_items)));
        }

        let full = sections.join("\n\n");
        if full.len() <= max_bytes {
            Ok(full)
        } else {
            Ok(utf8_safe_truncate(&full, max_bytes).to_string())
        }
    }

    /// 将 SQLite 记忆同步到 Claude Code 的 MEMORY.md
    pub fn sync_to_claude_memory(&self, memory_dir: &std::path::Path) -> Result<()> {
        let memory_file = memory_dir.join("MEMORY.md");

        let sync_block = self.generate_sync_block()?;

        let content = if memory_file.exists() {
            let existing = std::fs::read_to_string(&memory_file).context("读取 MEMORY.md 失败")?;
            Self::replace_sync_section(&existing, &sync_block)
        } else {
            std::fs::create_dir_all(memory_dir)?;
            format!("# Project Memory\n\n{sync_block}\n")
        };

        std::fs::write(&memory_file, &content).context("写入 MEMORY.md 失败")?;
        tracing::info!("已同步 Sage 记忆到 {:?}", memory_file);
        Ok(())
    }

    /// 生成同步区块内容
    fn generate_sync_block(&self) -> Result<String> {
        let all_memories = self.load_memories()?;
        // 去重：按内容去重，保留最新的（load_memories 已按 updated_at DESC）
        let mut seen = std::collections::HashSet::new();
        let memories: Vec<_> = all_memories
            .into_iter()
            .filter(|m| seen.insert(m.content.clone()))
            .collect();
        let mut lines: Vec<String> = vec![
            "<!-- SAGE_SYNC_START -->".into(),
            "## Sage Shared Memory".into(),
            String::new(),
            "> Auto-synced from Sage SQLite. Do NOT edit manually — changes will be overwritten."
                .into(),
            String::new(),
        ];

        let category_order = [
            ("identity", "Identity"),
            ("personality", "Personality"),
            ("values", "Values"),
            ("behavior", "Behavior Patterns"),
            ("thinking", "Thinking Style"),
            ("emotion", "Emotional Cues"),
            ("growth", "Growth Areas"),
            ("decision", "Recent Decisions"),
            ("pattern", "Observed Patterns"),
            ("coach_insight", "Coach Insights"),
        ];

        let known_cats: std::collections::HashSet<&str> =
            category_order.iter().map(|(c, _)| *c).collect();

        for (cat, label) in &category_order {
            let items: Vec<_> = memories
                .iter()
                .filter(|m| m.category == *cat)
                .take(10)
                .collect();
            if items.is_empty() {
                continue;
            }
            lines.push(format!("### {label}"));
            for m in &items {
                let content: String = m.content.chars().take(200).collect();
                lines.push(format!("- {content}"));
            }
            lines.push(String::new());
        }

        let mut extra: Vec<_> = memories
            .iter()
            .filter(|m| !known_cats.contains(m.category.as_str()))
            .take(15)
            .collect();
        if !extra.is_empty() {
            extra.sort_by(|a, b| a.category.cmp(&b.category));
            lines.push("### Other".into());
            for m in &extra {
                let content: String = m.content.chars().take(200).collect();
                lines.push(format!("- [{}] {content}", m.category));
            }
            lines.push(String::new());
        }

        let session_count = self.count_distinct_sessions().unwrap_or(0);
        lines.push(format!(
            "_Sage stats: {} memories, {} chat sessions_",
            memories.len(),
            session_count
        ));
        lines.push("<!-- SAGE_SYNC_END -->".into());

        Ok(lines.join("\n"))
    }

    /// 替换 MEMORY.md 中 SAGE_SYNC 标记之间的内容
    pub(super) fn replace_sync_section(existing: &str, new_block: &str) -> String {
        const START: &str = "<!-- SAGE_SYNC_START -->";
        const END: &str = "<!-- SAGE_SYNC_END -->";

        if let (Some(start_pos), Some(end_pos)) = (existing.find(START), existing.find(END)) {
            let before = &existing[..start_pos];
            let after = &existing[end_pos + END.len()..];
            format!("{before}{new_block}{after}")
        } else {
            let trimmed = existing.trim_end();
            format!("{trimmed}\n\n{new_block}\n")
        }
    }

    /// 保存行为模式记忆（category="pattern"），返回新记录 id
    pub fn append_pattern(&self, category: &str, observation: &str) -> Result<i64> {
        let content = format!("[{category}] {observation}");
        self.save_memory_with_visibility("pattern", &content, "router", 0.6, "public")
    }

    /// 保存决策记忆（category="decision"），返回新记录 id
    pub fn append_decision(&self, context: &str, decision: &str) -> Result<i64> {
        let content = format!("**Context**: {context}\n**Decision**: {decision}");
        self.save_memory_with_visibility("decision", &content, "router", 0.7, "public")
    }

    /// 查询今天已完成的心跳动作标题
    pub fn get_today_handled_actions(&self) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut stmt = conn.prepare(
            "SELECT content FROM memories WHERE category = 'decision' AND source = 'router' AND created_at >= ?1"
        ).context("准备查询今日动作失败")?;

        let titles: Vec<String> = stmt
            .query_map(rusqlite::params![today], |row| {
                let content: String = row.get(0)?;
                Ok(content
                    .lines()
                    .next()
                    .and_then(|line| line.strip_prefix("**Context**: "))
                    .unwrap_or("")
                    .to_string())
            })?
            .filter_map(|r| r.ok())
            .filter(|t| !t.is_empty())
            .collect();

        Ok(titles)
    }

    /// 检查自上次 evolution 以来是否有新的非 episodic 记忆
    pub fn has_memories_since_last_evolution(&self) -> Result<bool> {
        let conn = self.conn()?;
        // 上次 evolution 时间：最近一条 coach_insight 或 observer_note 的 created_at 之前的最后 evolution
        // 简化：用 today 的第一条 coach_insight 时间作为基准（Coach 在 Evolution 之前跑）
        let last_evolution: Option<String> = conn.query_row(
            "SELECT MAX(created_at) FROM memories WHERE category = 'coach_insight' AND created_at >= ?1",
            rusqlite::params![crate::today_start()],
            |row| row.get(0),
        ).ok().flatten();

        let Some(since) = last_evolution else {
            return Ok(true); // 今天没跑过 coach → 有新东西
        };

        // 检查 since 之后是否有新的非 episodic 记忆（排除 coach_insight/observer_note 本身）
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE depth <> 'episodic' AND category NOT IN ('coach_insight', 'observer_note') AND created_at > ?1",
            rusqlite::params![since],
            |row| row.get(0),
        ).unwrap_or(0);
        Ok(count > 0)
    }

    /// 获取今天的 observer_notes 内容列表
    pub fn get_today_observer_notes(&self) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT content FROM memories WHERE category = 'observer_note' AND created_at >= ?1 ORDER BY created_at DESC LIMIT 30"
        )?;
        let rows = stmt.query_map(rusqlite::params![crate::today_start()], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 获取今天的 coach_insights 内容列表
    pub fn get_today_coach_insights(&self) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT content FROM memories WHERE category = 'coach_insight' AND created_at >= ?1 ORDER BY created_at DESC LIMIT 15"
        )?;
        let rows = stmt.query_map(rusqlite::params![crate::today_start()], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 保存教练洞察（category="coach_insight"），返回新记录 id
    pub fn save_coach_insight(&self, insight: &str) -> Result<i64> {
        self.save_memory_with_visibility("coach_insight", insight, "coach", 0.8, "subconscious")
    }

    /// 获取消息社交图数据（person ↔ person）
    pub fn get_message_graph_data(&self) -> Result<Vec<(String, String, i64, i64)>> {
        // Resolve owner name before acquiring conn lock
        let owner_name = self
            .load_profile()
            .ok()
            .flatten()
            .map(|p| p.identity.name)
            .unwrap_or_default();
        let owner_pattern = if owner_name.is_empty() {
            "___NOMATCH___".to_string()
        } else {
            format!("%{}%", owner_name)
        };

        let conn = self.conn()?;

        let mut stmt = conn.prepare(
            "SELECT a.sender, b.sender,
                    COUNT(DISTINCT a.channel) as shared_ch,
                    (a.msg_count + b.msg_count) as total_msgs
             FROM (SELECT sender, channel, source, COUNT(*) as msg_count
                   FROM messages
                   WHERE sender NOT IN ('Unknown', '你', 'Oops', 'unknown', 'me', '我')
                     AND sender NOT LIKE ?1
                   GROUP BY sender, channel, source) a
             JOIN (SELECT sender, channel, source, COUNT(*) as msg_count
                   FROM messages
                   WHERE sender NOT IN ('Unknown', '你', 'Oops', 'unknown', 'me', '我')
                     AND sender NOT LIKE ?1
                   GROUP BY sender, channel, source) b
             ON a.channel = b.channel AND a.source = b.source AND a.sender < b.sender
             GROUP BY a.sender, b.sender
             ORDER BY shared_ch DESC, total_msgs DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![owner_pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
