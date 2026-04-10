use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sage_types::Memory;

use super::Store;

// ─── 检索加权辅助函数（Phase 1a） ──────────────────────────────

/// depth 层级对搜索得分的提升倍数（认知越深，优先级越高）
fn depth_boost(depth: &str) -> f64 {
    match depth {
        "axiom" => 3.0,
        "procedural" => 2.0,
        "semantic" => 1.0,
        "episodic" => 0.3,
        _ => 0.5,
    }
}

/// 解析 ISO 8601 / RFC 3339 日期字符串，返回距今天数（失败返回 0.0）
fn days_since(updated_at: &str) -> f64 {
    use chrono::{DateTime, TimeZone};
    let now = chrono::Local::now();
    // 尝试 RFC 3339（带时区），再尝试 SQLite 格式（无时区，视为本地时间）
    if let Ok(dt) = DateTime::parse_from_rfc3339(updated_at) {
        let secs = (now.timestamp() - dt.timestamp()).max(0) as f64;
        return secs / 86400.0;
    }
    if let Ok(naive) =
        chrono::NaiveDateTime::parse_from_str(updated_at, "%Y-%m-%d %H:%M:%S")
    {
        let dt = chrono::Local
            .from_local_datetime(&naive)
            .single()
            .unwrap_or_else(|| chrono::Local::now());
        let secs = (now.timestamp() - dt.timestamp()).max(0) as f64;
        return secs / 86400.0;
    }
    0.0
}

/// 指数衰减时效因子：e^(-α×days)，α=0.03 → 半衰期 ~23 天
/// 比对数衰减更平滑，day 0 到 day 30 之间有合理区分度
fn recency_factor(updated_at: &str) -> f64 {
    let days = days_since(updated_at);
    (-0.03 * days).exp()
}

/// 综合加权得分：base_score × depth_boost × confidence × recency_factor × (1 + 0.1 × validation_count)
fn weighted_score(m: &Memory, base_score: f64) -> f64 {
    base_score
        * depth_boost(&m.depth)
        * m.confidence
        * recency_factor(&m.updated_at)
        * (1.0 + 0.1 * m.validation_count as f64)
}

/// 检查记忆是否在有效期内（valid_until 为 None 或 > now）
fn is_memory_valid(m: &Memory, now_rfc3339: &str) -> bool {
    match &m.valid_until {
        None => true,
        Some(vu) => vu.as_str() >= &now_rfc3339[..19.min(now_rfc3339.len())],
    }
}

impl Store {
    /// 根据 category 推断记忆层级
    pub(super) fn infer_tier(category: &str) -> &'static str {
        match category {
            "identity" | "personality" | "values" => "core",
            "task" | "decision" | "session" | "reminder" | "observer_note" => "working",
            _ => "archive",
        }
    }

    /// 根据 category 和 source 推断认知深度
    /// 推断认知深度。
    /// 所有记忆默认 episodic，只有 Evolution 编译链（compile_to_semantic/procedural/axiom）
    /// 才能提升 depth。唯一例外：evolution source 的 personality 是编译产物。
    /// 根据 category + source + **内容语义** 推断记忆深度。
    /// depth 由内容决定，不能仅靠 category 静态映射。
    pub(super) fn infer_depth(category: &str, source: &str, content: &str) -> &'static str {
        // evolution 产出已在 evolution 内部设定正确 depth
        if source == "evolution" {
            return "procedural";
        }

        // ── 1. 明确 episodic 的 category（原始事件/通信记录）──
        match category {
            "observer_note" | "communication" | "session" | "report_insight" => {
                return "episodic";
            }
            _ => {}
        }

        // ── 2. 内容中包含具体日期/时间 → episodic（具体事件，不是规律）──
        if Self::has_temporal_marker(content) {
            return "episodic";
        }

        // ── 3. 按内容特征分层 ──
        match category {
            // fact 总是 episodic
            "fact" => "episodic",
            // decision 是具体判断
            "decision" | "recent_decisions" => "procedural",
            // 核心认知类：短抽象原则 → axiom，否则 semantic
            "identity" | "values" => {
                if content.chars().count() <= 40 && !Self::has_specific_reference(content) {
                    "axiom"
                } else {
                    "semantic"
                }
            }
            // 人格/行为/思维模式 → semantic（规律）
            "personality" | "behavior_patterns" | "behavior" | "thinking_style"
            | "thinking" | "emotional_cues" | "emotion" | "growth_areas" | "growth"
            | "strategy_insight" | "coach_insight" | "pattern" => "semantic",
            // 其余 → semantic（宁可高估也不要全部丢进 episodic）
            _ => "semantic",
        }
    }

    /// 内容中是否包含具体时间标记（日期、周次、时间戳等）
    fn has_temporal_marker(content: &str) -> bool {
        // 2026-03-19, 03-19, 2026/03/19
        if content.contains("202") && content.chars().any(|c| c == '-' || c == '/') {
            // 检查是否有 YYYY-MM-DD 或 MM-DD 格式的日期
            for word in content.split(|c: char| c.is_whitespace() || c == '，' || c == '。' || c == '、') {
                let w = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '/');
                if w.len() >= 5 {
                    let parts: Vec<&str> = w.split(|c| c == '-' || c == '/').collect();
                    if parts.len() >= 2 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
                        return true;
                    }
                }
            }
        }
        // W13, W2 等周次标记
        for word in content.split_whitespace() {
            if word.starts_with('W') && word.len() <= 4
                && word[1..].chars().all(|c| c.is_ascii_digit())
            {
                return true;
            }
        }
        false
    }

    /// 内容中是否包含具体人/事引用（特定事件而非抽象原则）
    fn has_specific_reference(content: &str) -> bool {
        // 包含具体人名动作（如 "向Bob汇报"、"和Sam讨论"）
        let markers = ["汇报", "邮件", "会议纪要", "PULSE", "项目"];
        markers.iter().any(|m| content.contains(m))
    }

    /// working 层默认 TTL（天）
    pub(super) fn default_ttl_days(category: &str) -> Option<i64> {
        match category {
            "task" | "reminder" => Some(7),
            "decision" => Some(14),
            "session" | "observer_note" => Some(3),
            _ => None,
        }
    }

    /// 从行中构建 Memory（SELECT 列顺序: id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable）
    /// embedding 字段默认 None（普通查询不 SELECT embedding，节省带宽）
    pub(super) fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
        Ok(Memory {
            id: row.get(0)?,
            category: row.get(1)?,
            content: row.get(2)?,
            source: row.get(3)?,
            confidence: row.get(4)?,
            visibility: row
                .get::<_, String>(5)
                .unwrap_or_else(|_| "public".to_string()),
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            about_person: row.get(8).ok(),
            last_accessed_at: row.get(9).ok(),
            depth: row.get(10).unwrap_or_else(|_| "episodic".to_string()),
            valid_until: row.get(11).ok(),
            validation_count: row.get(12).unwrap_or(0),
            embedding: None,
            derived_from: row.get("derived_from").ok(),
            evolution_note: row.get("evolution_note").ok(),
            derivable: row.get("derivable").unwrap_or(0),
        })
    }

    /// 按完整 SQL 查询记忆（供 pipeline actions 使用，SQL 必须返回标准 memory 列集合）
    pub fn query_memories_by_raw(&self, sql: &str) -> Result<Vec<sage_types::Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(sql).context("准备 query_memories_by_raw 查询失败")?;
        let rows = stmt.query_map([], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按条件查询记忆（内部方法）
    pub(super) fn query_memories_by(
        conn: &rusqlite::Connection,
        where_clause: &str,
        limit: usize,
    ) -> Result<Vec<sage_types::Memory>> {
        let sql = format!(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories WHERE {where_clause}
             ORDER BY confidence DESC, updated_at DESC LIMIT ?1"
        );
        let mut stmt = conn.prepare(&sql).context("查询记忆失败")?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 保存记忆（自动设置 tier / expires_at，字符相似度去重）
    pub fn save_memory(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64> {
        self.save_memory_inner(category, content, source, confidence, "public", None)
    }

    /// 在同 category 中找精确匹配的已有记忆
    fn find_exact_in_category(&self, category: &str, content: &str) -> Result<Option<i64>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let id: Option<i64> = conn
            .query_row(
                "SELECT id FROM memories WHERE category = ?1 AND content = ?2 AND status = 'active' LIMIT 1",
                rusqlite::params![category, content],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    /// 在同 category（可选 about_person）中找高度相似的已有记忆（LCS >60%）
    fn find_similar(
        &self,
        category: &str,
        content: &str,
        about_person: Option<&str>,
    ) -> Result<Option<i64>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        // 动态 SQL：有 about_person 时增加 person 过滤
        let (sql, person_filter) = match about_person {
            Some(p) => (
                "SELECT id, content FROM memories WHERE category = ?1 AND about_person = ?2 AND status = 'active'".to_string(),
                Some(p.to_string()),
            ),
            None => (
                "SELECT id, content FROM memories WHERE category = ?1 AND status = 'active'".to_string(),
                None,
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(i64, String)> = match person_filter {
            Some(ref p) => stmt
                .query_map(rusqlite::params![category, p], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect(),
            None => stmt
                .query_map(rusqlite::params![category], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect(),
        };
        drop(stmt);
        for (id, existing) in rows {
            if crate::similarity::text_similarity(content, &existing) > 0.6 {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// 对已存在的相似记忆执行 UPDATE（保留更长/更新的版本，取最高置信度）
    fn dedup_update(
        conn: &rusqlite::Connection,
        id: i64,
        content: &str,
        confidence: f64,
    ) -> Result<()> {
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET content = CASE WHEN length(?1) >= length(content) THEN ?1 ELSE content END, \
             confidence = MAX(confidence, ?2), updated_at = ?3 WHERE id = ?4",
            rusqlite::params![content, confidence, now, id],
        )?;
        Ok(())
    }

    /// 计算 expires_at（working tier 有 TTL，其余为 None）
    fn compute_expires_at(category: &str) -> Option<String> {
        Self::default_ttl_days(category).map(|days| {
            (chrono::Local::now() + chrono::Duration::days(days))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
    }

    /// 核心保存逻辑：归一化 → 相似去重 → INSERT（about_person 为 None 时不写入该列）
    fn save_memory_inner(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
        visibility: &str,
        about_person: Option<&str>,
    ) -> Result<i64> {
        self.save_memory_inner_with_derivable(category, content, source, confidence, visibility, about_person, 0)
    }

    /// 核心保存逻辑（带 derivable 标记）：归一化 → 相似去重 → INSERT
    fn save_memory_inner_with_derivable(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
        visibility: &str,
        about_person: Option<&str>,
        derivable: i64,
    ) -> Result<i64> {
        let content = crate::time_normalizer::normalize_time_refs(content);
        let content = content.as_str();

        // 短内容（≤15字）精确匹配去重；长内容相似度匹配去重
        if content.chars().count() <= 15 {
            if let Some(id) = self.find_exact_in_category(category, content)? {
                let conn = self
                    .conn
                    .lock()
                    .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
                let now = chrono::Local::now().to_rfc3339();
                conn.execute(
                    "UPDATE memories SET confidence = MAX(confidence, ?1), updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![confidence, now, id],
                )?;
                return Ok(id);
            }
        } else if let Some(id) = self.find_similar(category, content, about_person)? {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
            Self::dedup_update(&conn, id, content, confidence)?;
            return Ok(id);
        }

        let tier = Self::infer_tier(category);
        let depth = Self::infer_depth(category, source, content);
        let expires_at = Self::compute_expires_at(category);
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        if let Some(person) = about_person {
            conn.execute(
                "INSERT INTO memories (category, content, source, confidence, tier, status, expires_at, visibility, about_person, depth, valid_until, validation_count, derivable, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9, NULL, 0, ?10, ?11, ?11)",
                rusqlite::params![category, content, source, confidence, tier, expires_at, visibility, person, depth, derivable, now],
            )
            .context("保存 memory（关于某人）失败")?;
        } else {
            conn.execute(
                "INSERT INTO memories (category, content, source, confidence, tier, status, expires_at, visibility, depth, valid_until, validation_count, derivable, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, NULL, 0, ?9, ?10, ?10)",
                rusqlite::params![category, content, source, confidence, tier, expires_at, visibility, depth, derivable, now],
            )
            .context("保存 memory 失败")?;
        }
        Ok(conn.last_insert_rowid())
    }

    /// 保存带可见性的记忆（observer 阶段时 derivable=1，表示可从原始数据重新推导）
    pub fn save_memory_with_visibility_derivable(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
        visibility: &str,
        derivable: bool,
    ) -> Result<i64> {
        self.save_memory_inner_with_derivable(
            category, content, source, confidence, visibility, None, if derivable { 1 } else { 0 },
        )
    }

    /// 保存带可见性的记忆
    pub fn save_memory_with_visibility(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
        visibility: &str,
    ) -> Result<i64> {
        self.save_memory_inner(category, content, source, confidence, visibility, None)
    }

    /// 保存关于某人的记忆（自动解析别名）
    pub fn save_memory_about_person(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
        visibility: &str,
        about_person: &str,
    ) -> Result<i64> {
        let resolved = self.resolve_person_alias(about_person)?;
        self.save_memory_inner(category, content, source, confidence, visibility, Some(&resolved))
    }

    /// 查询别名，返回 canonical name（无别名则返回原名）
    pub fn resolve_person_alias(&self, name: &str) -> Result<String> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let result = conn.query_row(
            "SELECT canonical FROM person_aliases WHERE alias = ?1",
            rusqlite::params![name],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(canonical) => Ok(canonical),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(name.to_string()),
            Err(e) => Err(e.into()),
        }
    }

    /// 清理过期 working 记忆：标记为 expired
    pub fn expire_stale_memories(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let count = conn
            .execute(
                "UPDATE memories SET status = 'expired'
             WHERE tier = 'working' AND status = 'active'
             AND expires_at IS NOT NULL AND expires_at < ?1",
                rusqlite::params![now],
            )
            .context("清理过期记忆失败")?;
        Ok(count)
    }

    /// 将 working 任务标记为完成
    pub fn complete_task(&self, memory_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET status = 'done', updated_at = ?2
             WHERE id = ?1 AND tier = 'working'",
            rusqlite::params![memory_id, now],
        )
        .context("标记任务完成失败")?;
        Ok(())
    }

    /// 按 source 删除记忆（用于 session ingestion 的 upsert 场景）
    pub fn delete_memory_by_source(&self, source: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let deleted = conn
            .execute(
                "DELETE FROM memories WHERE source = ?1",
                rusqlite::params![source],
            )
            .context("按 source 删除 memory 失败")?;
        Ok(deleted)
    }

    /// 清除所有非活跃记忆（硬删除 archived / compiled / expired 行）
    pub fn purge_archived_memories(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let deleted = conn
            .execute(
                "DELETE FROM memories WHERE status IN ('archived', 'compiled', 'expired')",
                [],
            )
            .context("清除非活跃 memory 失败")?;
        Ok(deleted)
    }

    /// 归档过期 episodic 记忆（超过 days 天且 confidence < max_conf）
    pub fn archive_stale_episodics(&self, days: i64, max_conf: f64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let archived = conn
            .execute(
                "UPDATE memories SET status = 'archived', evolution_note = 'episodic expired', updated_at = datetime('now') \
                 WHERE status = 'active' AND depth = 'episodic' AND confidence < ?1 \
                 AND updated_at < datetime('now', ?2)",
                rusqlite::params![max_conf, format!("-{days} days")],
            )
            .context("归档过期 episodic 失败")?;
        Ok(archived)
    }

    /// 更新记忆的内容和置信度
    pub fn update_memory(&self, id: i64, content: &str, confidence: f64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET content = ?1, confidence = ?2, updated_at = ?4 WHERE id = ?3",
            rusqlite::params![content, confidence, id, now],
        )
        .context("更新 memory 失败")?;
        Ok(())
    }

    /// 更新记忆内容（仅 content + updated_at），供 MemoryIntegrator 合并记忆时调用
    pub fn update_memory_content(&self, id: i64, content: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let affected = conn
            .execute(
                "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![content, now, id],
            )
            .context("更新 memory content 失败")?;
        if affected == 0 {
            anyhow::bail!("Memory {id} not found");
        }
        Ok(())
    }

    /// 批量更新 last_accessed_at（Chat 召回或 Dashboard 展示时调用）
    pub fn touch_memories(&self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let placeholders: Vec<String> = (0..ids.len()).map(|i| format!("?{}", i + 2)).collect();
        let sql = format!(
            "UPDATE memories SET last_accessed_at = ?1 WHERE id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];
        for id in ids {
            params.push(Box::new(*id));
        }
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let count = stmt.execute(refs.as_slice())?;
        Ok(count)
    }

    /// 增加记忆的验证次数（每次被 Chat 检索注入上下文时调用）
    pub fn increment_validation(&self, memory_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET validation_count = validation_count + 1, last_accessed_at = ?1 WHERE id = ?2",
            rusqlite::params![now, memory_id],
        )
        .context("更新 validation_count 失败")?;
        Ok(())
    }

    /// 加载所有记忆（按置信度和更新时间排序）
    pub fn load_memories(&self) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable FROM memories WHERE status = 'active' ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 load_memories 查询失败")?;
        let rows = stmt.query_map([], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// FTS5 关键词搜索记忆，按 BM25 相关度 + 置信度排序
    pub fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.load_memories();
        }
        let pattern = format!("%{trimmed}%");
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories
             WHERE content LIKE ?1 OR category LIKE ?1
             ORDER BY confidence DESC
             LIMIT ?2",
        ).context("准备 search_memories 查询失败")?;

        let rows = stmt
            .query_map(
                rusqlite::params![pattern, limit as i64],
                Self::row_to_memory,
            )
            .context("执行 search_memories 查询失败")?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// FTS 检索 + 图谱扩展：先找 seed 记忆，再通过边拉取一跳邻居，合并去重排序
    /// 排序使用加权公式：base_score × depth_boost × confidence × recency_factor × (1 + 0.1 × validation_count)
    pub fn search_memories_with_graph(
        &self,
        query: &str,
        seed_limit: usize,
        total_limit: usize,
    ) -> Result<Vec<Memory>> {
        let now_str = chrono::Local::now().to_rfc3339();
        let seeds: Vec<Memory> = self
            .search_memories(query, seed_limit)?
            .into_iter()
            .filter(|m| is_memory_valid(m, &now_str))
            .collect();
        if seeds.is_empty() {
            return Ok(seeds);
        }

        // seed 记忆的得分 = confidence，图谱邻居的得分 = activation
        let mut score_map: std::collections::HashMap<i64, (Memory, f64)> =
            std::collections::HashMap::new();
        for m in &seeds {
            let score = weighted_score(m, m.confidence);
            score_map.insert(m.id, (m.clone(), score));
        }

        // 对每个 seed 拉一跳图谱邻居（过滤已过期）
        for seed in &seeds {
            if let Ok(neighbors) = self.get_connected_memories(seed.id, 1) {
                for (mem, activation) in neighbors {
                    if !is_memory_valid(&mem, &now_str) {
                        continue;
                    }
                    let score = weighted_score(&mem, activation);
                    let entry = score_map.entry(mem.id).or_insert((mem.clone(), 0.0));
                    if score > entry.1 {
                        entry.1 = score;
                    }
                }
            }
        }

        let mut results: Vec<(Memory, f64)> = score_map.into_values().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(total_limit);
        Ok(results.into_iter().map(|(m, _)| m).collect())
    }

    /// 搜索公开记忆（Digital Twin 使用）
    pub fn search_public_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.get_memories_by_visibility("public");
        }
        let pattern = format!("%{trimmed}%");
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories
             WHERE visibility = 'public' AND (content LIKE ?1 OR category LIKE ?1)
             ORDER BY confidence DESC
             LIMIT ?2",
        ).context("准备 search_public_memories 查询失败")?;
        let rows = stmt.query_map(
            rusqlite::params![pattern, limit as i64],
            Self::row_to_memory,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按可见性层级加载记忆
    pub fn get_memories_by_visibility(&self, visibility: &str) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories WHERE visibility = ?1
             ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 get_memories_by_visibility 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![visibility], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 更新记忆可见性
    pub fn update_memory_visibility(&self, id: i64, visibility: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET visibility = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![visibility, now, id],
        )
        .context("更新 memory visibility 失败")?;
        Ok(())
    }

    /// 统计各可见性层级的记忆数量
    pub fn count_memories_by_visibility(&self) -> Result<Vec<(String, i64)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT visibility, COUNT(*) FROM memories GROUP BY visibility ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取关于某人的所有记忆
    pub fn get_memories_about_person(&self, name: &str) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories WHERE about_person = ?1
             ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 get_memories_about_person 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![name], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取所有已知人名（去重排序）
    pub fn get_known_persons(&self) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT about_person FROM memories WHERE about_person IS NOT NULL ORDER BY about_person",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 合并两个人：将 source 的所有记忆转移到 target 名下，记录别名，然后去重
    pub fn merge_persons(&self, target: &str, source: &str) -> Result<u64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        // 将 source 的所有记忆归到 target
        let moved = conn.execute(
            "UPDATE memories SET about_person = ?1 WHERE about_person = ?2",
            rusqlite::params![target, source],
        ).context("合并人物记忆失败")? as u64;
        // 记录别名：下次 LLM 再提取出 source 名字时自动映射到 target
        conn.execute(
            "INSERT OR REPLACE INTO person_aliases (alias, canonical) VALUES (?1, ?2)",
            rusqlite::params![source, target],
        )?;
        // 将指向 source 的旧别名也重定向到 target（传递合并）
        conn.execute(
            "UPDATE person_aliases SET canonical = ?1 WHERE canonical = ?2",
            rusqlite::params![target, source],
        )?;
        // 合并后去重：同 category + 相似内容只保留 confidence 最高的
        let mut stmt = conn.prepare(
            "SELECT id, category, content, confidence FROM memories
             WHERE about_person = ?1 AND status = 'active'
             ORDER BY category, confidence DESC",
        )?;
        let rows: Vec<(i64, String, String, f64)> = stmt
            .query_map(rusqlite::params![target], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen: Vec<(String, String)> = Vec::new();
        let mut to_archive: Vec<i64> = Vec::new();
        for (id, cat, content, _conf) in &rows {
            let dominated = seen.iter().any(|(sc, st)| {
                sc == cat && crate::similarity::text_similarity(st, content) > 0.6
            });
            if dominated {
                to_archive.push(*id);
            } else {
                seen.push((cat.clone(), content.clone()));
            }
        }
        for id in &to_archive {
            conn.execute(
                "UPDATE memories SET status = 'archived', evolution_note = '人物合并去重' WHERE id = ?1",
                rusqlite::params![id],
            )?;
        }
        Ok(moved)
    }

    /// 删除记忆
    pub fn delete_memory(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
            .context("删除 memory 失败")?;
        Ok(())
    }

    /// 获取某个日期之后创建的 memories（用于报告上下文收集）
    pub fn get_memories_since(&self, since: &str) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable FROM memories WHERE created_at >= ?1 ORDER BY created_at DESC",
        ).context("准备 get_memories_since 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![since], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取某个日期之后的 session 类 memories
    pub fn get_session_summaries_since(&self, since: &str) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable FROM memories WHERE category = 'session' AND created_at >= ?1 ORDER BY created_at DESC",
        ).context("准备 get_session_summaries_since 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![since], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取某个日期之后的 coach insights 内容列表
    pub fn get_coach_insights_since(&self, since: &str) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT content FROM memories WHERE category = 'coach_insight' AND created_at >= ?1 ORDER BY created_at DESC",
        ).context("准备 get_coach_insights_since 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![since], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取指定 category 的所有活跃记忆
    pub fn get_memories_by_category(&self, category: &str) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
             FROM memories WHERE category = ?1 AND status = 'active' ORDER BY created_at DESC",
        ).context("准备 get_memories_by_category 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![category], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 加载所有活跃记忆（status='active'）
    pub fn load_active_memories(&self) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories WHERE status = 'active'
             ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 load_active_memories 查询失败")?;
        let rows = stmt.query_map([], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 核心人格/认知记忆类别
    const CORE_CATEGORIES: &'static [&'static str] = &[
        "identity",
        "personality",
        "values",
        "behavior_patterns",
        "thinking_style",
        "emotional_cues",
        "growth_areas",
        "coach_insight",
        "strategy_insight",
        "decision",
        "recent_decisions",
    ];

    /// 加载核心人格记忆（排除通信观察等噪音）
    pub fn load_core_memories(&self) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let placeholders: String = Self::CORE_CATEGORIES
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories WHERE status = 'active' AND category IN ({placeholders})
             ORDER BY confidence DESC, updated_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = Self::CORE_CATEGORIES
            .iter()
            .map(|c| c as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 衰减长期未更新的 archive 记忆（Phase 1a 已禁用）
    /// 不再按时间衰减置信度——depth+validation_count 已承担信号权重，
    /// 人工衰减会误删低频但真实的长期记忆（axiom/procedural）。
    pub fn decay_stale_archive_memories(
        &self,
        _stale_days: i64,
        _decay_amount: f64,
        _expire_threshold: f64,
    ) -> Result<usize> {
        Ok(0)
    }

    /// 分层指数衰减：按 depth 使用不同的 α，axiom 永不自动衰减
    /// - episodic:   α=0.05（半衰期 ~14 天），低于 0.3 时归档
    /// - semantic:   α=0.02（半衰期 ~35 天），低于 0.3 时归档
    /// - procedural: α=0.01（半衰期 ~69 天），低于 0.3 时归档
    /// - axiom:      永不自动衰减
    /// 生产默认值：(30, 90, 180)；测试可传入 (0, 0, 0) 让所有无访问记录的记忆立即衰减
    pub fn decay_memories_by_depth_with_thresholds(
        &self,
        episodic_days: i64,
        semantic_days: i64,
        procedural_days: i64,
    ) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let mut total = 0;

        // 每层用不同的 α：episodic 衰减快，procedural 衰减慢
        let alpha_map: &[(&str, i64, f64)] = &[
            ("episodic", episodic_days, 0.05),    // 半衰期 ~14 天
            ("semantic", semantic_days, 0.02),     // 半衰期 ~35 天
            ("procedural", procedural_days, 0.01), // 半衰期 ~69 天
            // axiom 不在列表中，永不衰减
        ];
        for (depth, days, alpha) in alpha_map {
            let threshold = format!("-{days} days");
            // 查询符合条件的记忆，在 Rust 端计算指数衰减
            let mut stmt = conn.prepare(
                "SELECT id, confidence,
                        julianday('now') - julianday(COALESCE(last_accessed_at, created_at)) AS inactive_days
                 FROM memories
                 WHERE status = 'active' AND depth = ?1
                   AND (last_accessed_at IS NULL OR last_accessed_at < datetime('now', ?2))",
            ).context("准备分层衰减查询失败")?;
            let targets: Vec<(i64, f64, f64)> = stmt
                .query_map(rusqlite::params![depth, threshold], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?
                .filter_map(|r| r.ok())
                .collect();

            for (id, confidence, inactive_days) in &targets {
                let new_conf = confidence * (-alpha * inactive_days).exp();
                if new_conf < 0.3 {
                    conn.execute(
                        "UPDATE memories SET confidence = 0.3, status = 'archived', updated_at = ?1 WHERE id = ?2",
                        rusqlite::params![now, id],
                    ).context("归档衰减记忆失败")?;
                } else {
                    conn.execute(
                        "UPDATE memories SET confidence = ?1, updated_at = ?2 WHERE id = ?3",
                        rusqlite::params![new_conf, now, id],
                    ).context("更新衰减记忆失败")?;
                }
                total += 1;
            }
        }
        Ok(total)
    }

    /// 分层衰减（生产用，阈值：episodic=30天, semantic=90天, procedural=180天）
    pub fn decay_memories_by_depth(&self) -> Result<usize> {
        self.decay_memories_by_depth_with_thresholds(30, 90, 180)
    }

    /// 提升高置信度 archive 记忆到 core（限定特定行为/模式类别）
    pub fn promote_high_confidence_memories(&self, min_confidence: f64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let promoted = conn
            .execute(
                "UPDATE memories SET tier = 'core', updated_at = ?1
             WHERE tier = 'archive' AND status = 'active' AND confidence >= ?2
             AND category IN ('behavior', 'thinking', 'pattern', 'growth', 'emotion')
             AND updated_at != created_at",
                rusqlite::params![now, min_confidence],
            )
            .context("提升记忆到 core 失败")?;
        Ok(promoted)
    }

    /// 更新记忆的认知深度（depth 字段）
    pub fn update_memory_depth(&self, id: i64, depth: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE memories SET depth = ?1 WHERE id = ?2",
            rusqlite::params![depth, id],
        )
        .context("更新 memory depth 失败")?;
        Ok(())
    }

    /// 标记记忆为已编译（status = 'compiled'）——源 episodic 保留但不再参与检索
    pub fn mark_memory_compiled(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET status = 'compiled', updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        )
        .context("标记 memory compiled 失败")?;
        Ok(())
    }

    /// 按 depth 加载活跃记忆
    pub fn load_memories_by_depth(&self, depth: &str) -> Result<Vec<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories WHERE depth = ?1 AND status = 'active'
             ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 load_memories_by_depth 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![depth], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按 depth + 关键词搜索记忆（主题感知加载）
    ///
    /// 先按关键词过滤，不足时用最近记忆补齐（不超过 limit）。
    pub fn search_memories_by_depth(&self, query: &str, depth: &str, limit: usize) -> Result<Vec<Memory>> {
        // 从 query 中提取 2 字符以上的词（最多取 5 个关键词）
        let keywords: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric() && !matches!(c, '\u{4e00}'..='\u{9fff}'))
            .filter(|w| w.chars().count() >= 2)
            .take(5)
            .map(|w| w.replace('\'', "''"))
            .collect();

        if keywords.is_empty() {
            return self.load_memories_by_depth_limited(depth, limit);
        }

        // 构建 LIKE 条件（逐词 OR）
        let where_clause = keywords
            .iter()
            .map(|k| format!("content LIKE '%{k}%'"))
            .collect::<Vec<_>>()
            .join(" OR ");

        let sql = format!(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
             about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
             FROM memories WHERE depth = ?1 AND status = 'active' AND ({where_clause}) \
             ORDER BY confidence DESC, updated_at DESC LIMIT ?2"
        );

        let mut results: Vec<Memory> = {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
            let mut stmt = conn.prepare(&sql).context("准备 search_memories_by_depth 查询失败")?;
            let rows = stmt.query_map(rusqlite::params![depth, limit as i64], Self::row_to_memory)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // 关键词命中不足时，用按时间倒序的记忆补齐
        if results.len() < limit {
            let remaining = limit - results.len();
            let existing_ids: std::collections::HashSet<i64> = results.iter().map(|m| m.id).collect();
            let extra = self.load_memories_by_depth_limited(depth, remaining + existing_ids.len())?;
            for m in extra {
                if !existing_ids.contains(&m.id) && results.len() < limit {
                    results.push(m);
                }
            }
        }
        Ok(results)
    }

    /// 按 depth 加载有限数量的活跃记忆（内部辅助）
    fn load_memories_by_depth_limited(&self, depth: &str, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
             about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
             FROM memories WHERE depth = ?1 AND status = 'active' \
             ORDER BY confidence DESC, updated_at DESC LIMIT ?2",
        ).context("准备 load_memories_by_depth_limited 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![depth, limit as i64], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 加载最旧的 episodic 批次（evolution 小批量轮转）
    pub fn load_oldest_episodic_batch(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
             about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
             FROM memories WHERE depth = 'episodic' AND status = 'active' \
             ORDER BY last_accessed_at ASC NULLS FIRST, created_at ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 加载最旧的冗长记忆批次（content > 50 字符，evolution 小批量轮转）
    pub fn load_oldest_verbose_batch(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
             about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
             FROM memories WHERE status = 'active' AND length(content) > 50 \
             ORDER BY last_accessed_at ASC NULLS FIRST, created_at ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 更新 last_accessed_at 为当前时间（标记已处理，用于轮转）
    pub fn touch_memory(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET last_accessed_at = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        )?;
        Ok(())
    }

    /// 记忆总数（活跃状态）
    pub fn count_memories(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE status = 'active'",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// 统计自指定时间戳以来新增的活跃记忆数（autoDream 数据门控用）
    pub fn count_memories_since(&self, since: &str) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE created_at >= ?1 AND status = 'active'",
            rusqlite::params![since],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    /// 记忆驱逐：当 active 记忆超过 cap 时，按 weighted_score 淘汰末尾批次。
    /// 只淘汰 tier='working' AND depth IN ('episodic','semantic')，不碰 core/procedural/axiom。
    pub fn evict_low_quality_memories(&self, cap: usize, evict_count: usize) -> Result<usize> {
        if self.count_memories()? <= cap {
            return Ok(0);
        }
        let memories = {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
            let mut stmt = conn.prepare(
                "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
                 about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
                 FROM memories \
                 WHERE tier = 'working' AND depth IN ('episodic','semantic') AND status = 'active'",
            ).context("准备驱逐查询失败")?;
            let rows = stmt.query_map([], Self::row_to_memory)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut scored: Vec<(i64, f64)> = memories
            .iter()
            .map(|m| (m.id, weighted_score(m, m.confidence)))
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let to_evict: Vec<i64> = scored.into_iter().take(evict_count).map(|(id, _)| id).collect();
        if to_evict.is_empty() {
            return Ok(0);
        }

        let placeholders: String = to_evict.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "UPDATE memories SET status = 'archived' WHERE id IN ({placeholders})"
        );
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let params: Vec<&dyn rusqlite::ToSql> = to_evict.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let count = conn.execute(&sql, params.as_slice()).context("驱逐记忆失败")?;
        Ok(count)
    }

    /// 检查是否存在与给定内容高度相似的记忆（LIKE 模糊匹配，用于导入前去重）
    pub fn has_similar_memory(&self, content: &str) -> Result<bool> {
        if content.trim().is_empty() {
            return Ok(true); // 空内容视为重复，不插入
        }
        // 取前 80 个字符做相似度查询（足够判断语义重叠）
        let mut end = content.len().min(80);
        while end < content.len() && !content.is_char_boundary(end) { end += 1; }
        let prefix = &content[..end];
        let pattern = format!("%{prefix}%");
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE status = 'active' AND content LIKE ?1",
                rusqlite::params![pattern],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(count > 0)
    }

    /// 加载最近 24 小时内的 observer_note 记忆（供 Coach 使用）
    pub fn load_observer_notes_recent(&self) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT content FROM memories
             WHERE category = 'observer_note'
               AND created_at >= datetime('now', '-24 hours')
             ORDER BY created_at ASC
             LIMIT 100",
            )
            .context("查询 observer_notes 失败")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ─── Phase 1b: Embedding 向量搜索 ──────────────────────────────

    /// 保存记忆的 embedding（f32 数组 → little-endian 字节）
    pub fn save_embedding(&self, memory_id: i64, embedding: &[f32]) -> Result<()> {
        let bytes = embed_to_bytes(embedding);
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE memories SET embedding = ?1 WHERE id = ?2",
            rusqlite::params![bytes, memory_id],
        )
        .context("保存 embedding 失败")?;
        Ok(())
    }

    /// 向量相似度搜索：暴力遍历所有有 embedding 的 active 记忆，返回 (Memory, cosine_score)
    pub fn search_memories_by_vector(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(Memory, f32)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;

        // 步骤 1：取出所有有 embedding 的活跃记忆 id + embedding bytes
        let mut stmt = conn
            .prepare(
                "SELECT id, embedding FROM memories
                 WHERE status = 'active' AND embedding IS NOT NULL",
            )
            .context("准备向量搜索查询失败")?;

        let id_scores: Vec<(i64, f32)> = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let bytes: Vec<u8> = row.get(1)?;
                Ok((id, bytes))
            })?
            .filter_map(|r| r.ok())
            .map(|(id, bytes)| {
                let vec = bytes_to_embed(&bytes);
                let score = cosine_similarity(query_embedding, &vec);
                (id, score)
            })
            .collect();

        // 步骤 2：按相似度降序排列，取 top-limit
        let mut sorted = id_scores;
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(limit);

        if sorted.is_empty() {
            return Ok(vec![]);
        }

        // 步骤 3：批量取完整 Memory 信息
        let placeholders: String = (1..=sorted.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at,
                    about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
             FROM memories WHERE id IN ({placeholders})"
        );
        let mut mem_stmt = conn.prepare(&sql).context("准备向量搜索结果查询失败")?;
        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            sorted.iter().map(|(id, _)| -> Box<dyn rusqlite::types::ToSql> { Box::new(*id) }).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mem_map: std::collections::HashMap<i64, Memory> = mem_stmt
            .query_map(refs.as_slice(), Self::row_to_memory)?
            .filter_map(|r| r.ok())
            .map(|m| (m.id, m))
            .collect();

        // 步骤 4：按原来排序重组，保留相似度分数
        let results = sorted
            .into_iter()
            .filter_map(|(id, score)| mem_map.get(&id).cloned().map(|m| (m, score)))
            .collect();
        Ok(results)
    }

    /// 混合搜索：LIKE 关键词 + 向量相似度 + 图谱扩展，按 depth 加权综合排序
    pub fn search_memories_hybrid(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let now_str = chrono::Local::now().to_rfc3339();

        // 步骤 1：LIKE 搜索（扩大 seed 范围）
        let fts_results = self.search_memories(query, limit * 2)?;
        let mut score_map: std::collections::HashMap<i64, (Memory, f64)> =
            std::collections::HashMap::new();
        for m in fts_results {
            if !is_memory_valid(&m, &now_str) {
                continue;
            }
            let score = weighted_score(&m, m.confidence);
            score_map.insert(m.id, (m, score));
        }

        // 步骤 2：向量搜索（可选）
        if let Some(qvec) = query_embedding {
            let vec_results = self.search_memories_by_vector(qvec, limit * 2)?;
            for (m, cosine) in vec_results {
                if !is_memory_valid(&m, &now_str) {
                    continue;
                }
                let score = weighted_score(&m, cosine as f64);
                let entry = score_map.entry(m.id).or_insert((m.clone(), 0.0));
                if score > entry.1 {
                    entry.1 = score;
                }
            }
        }

        // 步骤 3：图谱扩展（1-hop spreading activation）
        let seeds: Vec<i64> = score_map.keys().copied().collect();
        for seed_id in seeds {
            if let Some((seed_mem, _)) = score_map.get(&seed_id) {
                let seed_mem = seed_mem.clone();
                if let Ok(neighbors) = self.get_connected_memories(seed_mem.id, 1) {
                    for (mem, activation) in neighbors {
                        if !is_memory_valid(&mem, &now_str) {
                            continue;
                        }
                        let score = weighted_score(&mem, activation);
                        let entry = score_map.entry(mem.id).or_insert((mem.clone(), 0.0));
                        if score > entry.1 {
                            entry.1 = score;
                        }
                    }
                }
            }
        }

        // 步骤 4：排序截断
        let mut results: Vec<(Memory, f64)> = score_map.into_values().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results.into_iter().map(|(m, _)| m).collect())
    }

    /// 按 ID 列表批量获取记忆（用于 derived_from 证据链接）
    pub fn get_memories_by_ids(&self, ids: &[i64]) -> Result<Vec<Memory>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let placeholders = ids.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
             about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
             FROM memories WHERE id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql).context("准备 get_memories_by_ids 查询失败")?;
        let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter()
            .map(|id| id as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按 ID 获取单条记忆（无论 status，用于 provenance 溯源查询）
    pub fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mem = conn
            .query_row(
                "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
                 about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
                 FROM memories WHERE id = ?1",
                rusqlite::params![id],
                Self::row_to_memory,
            )
            .optional()
            .context("查询 memory by id 失败")?;
        Ok(mem)
    }

    /// 验证记忆：增加 validation_count，微调 confidence（+0.02，封顶 1.0）
    pub fn verify_memory(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET validation_count = validation_count + 1, \
             confidence = MIN(1.0, confidence + 0.02), \
             last_accessed_at = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        ).context("验证 memory 失败")?;
        Ok(())
    }

    /// 挑战记忆：降低 confidence（-0.05，最低 0.3），追加挑战记录到 evolution_note
    pub fn challenge_memory(&self, id: i64, counter_evidence: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET confidence = MAX(0.3, confidence - 0.05), \
             last_accessed_at = ?1, \
             evolution_note = COALESCE(evolution_note || char(10), '') || 'CHALLENGE: ' || ?2 \
             WHERE id = ?3",
            rusqlite::params![now, counter_evidence, id],
        ).context("挑战 memory 失败")?;
        Ok(())
    }

    /// 设置 evolution_note（覆盖写）
    pub fn set_evolution_note(&self, id: i64, note: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE memories SET evolution_note = ?1 WHERE id = ?2",
            rusqlite::params![note, id],
        ).context("设置 evolution_note 失败")?;
        Ok(())
    }

    /// 按多个 depth 加载活跃记忆（verifier/integrator 使用）
    pub fn load_memories_by_depths(&self, depths: &[&str]) -> Result<Vec<Memory>> {
        if depths.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let placeholders = depths.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, \
             about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable \
             FROM memories WHERE depth IN ({placeholders}) AND status = 'active' \
             ORDER BY confidence DESC, updated_at DESC"
        );
        let mut stmt = conn.prepare(&sql).context("准备 load_memories_by_depths 查询失败")?;
        let params: Vec<&dyn rusqlite::types::ToSql> = depths.iter()
            .map(|d| d as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按 category 加载最近 N 条 observations（verifier/integrator 使用）
    pub fn load_recent_observations_by_category(&self, category: &str, limit: usize) -> Result<Vec<crate::ObservationRow>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, observation, raw_data, created_at FROM observations \
             WHERE category = ?1 ORDER BY created_at DESC LIMIT ?2",
        ).context("准备 load_recent_observations_by_category 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![category, limit as i64], |row| {
            Ok(crate::ObservationRow {
                id: row.get(0)?,
                category: row.get(1)?,
                observation: row.get(2)?,
                raw_data: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取 depth 分布统计（各 depth 的活跃记忆数量）
    pub fn get_depth_distribution(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT depth, COUNT(*) FROM memories WHERE status = 'active' GROUP BY depth \
             ORDER BY CASE depth WHEN 'episodic' THEN 1 WHEN 'semantic' THEN 2 \
             WHEN 'procedural' THEN 3 WHEN 'axiom' THEN 4 ELSE 5 END",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 软删除记忆（归档而非硬删除），并记录原因
    pub fn archive_memory(&self, id: i64, note: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET status = 'archived', evolution_note = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![note, now, id],
        )
        .context("归档 memory 失败")?;
        Ok(())
    }

    /// 保存新记忆并记录溯源（由 Evolution COMPILE 使用）
    pub fn save_memory_with_provenance(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
        derived_from_ids: &[i64],
        note: &str,
    ) -> Result<i64> {
        let id = self.save_memory(category, content, source, confidence)?;
        if id > 0 {
            let derived_json = serde_json::to_string(derived_from_ids).unwrap_or_default();
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
            conn.execute(
                "UPDATE memories SET derived_from = ?1, evolution_note = ?2 WHERE id = ?3",
                rusqlite::params![derived_json, note, id],
            )
            .context("更新 provenance 失败")?;
        }
        Ok(id)
    }
}

// ─── Phase 1b: 向量工具函数 ──────────────────────────────────────

/// 将 f32 数组序列化为 little-endian 字节
pub fn embed_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// 将 little-endian 字节反序列化为 f32 数组
pub fn bytes_to_embed(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// 余弦相似度（0.0 ~ 1.0，向量维度不匹配或零向量返回 0.0）
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a < 1e-8 || norm_b < 1e-8 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
