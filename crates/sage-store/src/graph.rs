use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sage_types::{KnowledgeEdge, MemoryEdge};

use super::Store;

/// 将数据库行转换为 KnowledgeEdge（列顺序：id, from_type, from_id, to_type, to_id, relation, weight, created_at）
fn row_to_knowledge_edge(row: &rusqlite::Row) -> rusqlite::Result<KnowledgeEdge> {
    Ok(KnowledgeEdge {
        id: row.get(0)?,
        from_type: row.get(1)?,
        from_id: row.get(2)?,
        to_type: row.get(3)?,
        to_id: row.get(4)?,
        relation: row.get(5)?,
        weight: row.get(6)?,
        created_at: row.get(7)?,
    })
}

impl Store {
    /// 添加记忆之间的边（连接），存在则更新权重
    pub fn save_memory_edge(
        &self,
        from_id: i64,
        to_id: i64,
        relation: &str,
        weight: f64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO memory_edges (from_id, to_id, relation, weight, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(from_id, to_id, relation) DO UPDATE SET weight = ?4",
            rusqlite::params![from_id, to_id, relation, weight, now],
        )
        .context("保存 memory_edge 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// Hebbian 共现强化：同一 chat turn 被召回的记忆两两加强连接
    pub fn strengthen_edges(&self, memory_ids: &[i64]) -> Result<usize> {
        if memory_ids.len() < 2 {
            return Ok(0);
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let mut strengthened = 0;

        for i in 0..memory_ids.len() {
            for j in (i + 1)..memory_ids.len() {
                let (a, b) = (memory_ids[i], memory_ids[j]);
                let existing: Option<(i64, f64)> = conn
                    .query_row(
                        "SELECT id, weight FROM memory_edges
                     WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)
                     LIMIT 1",
                        rusqlite::params![a, b],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;

                match existing {
                    Some((edge_id, w)) => {
                        let new_w = (w + 0.05).min(1.0);
                        conn.execute(
                            "UPDATE memory_edges SET weight = ?1, last_activated_at = ?2 WHERE id = ?3",
                            rusqlite::params![new_w, now, edge_id],
                        )?;
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO memory_edges (from_id, to_id, relation, weight, created_at, last_activated_at)
                             VALUES (?1, ?2, 'co_occurred', 0.3, ?3, ?3)",
                            rusqlite::params![a, b, now],
                        )?;
                    }
                }
                strengthened += 1;
            }
        }
        Ok(strengthened)
    }

    /// 指数衰减长期未激活的边：weight × e^(-α × inactive_days)，低于阈值则删除
    /// α 控制衰减速率（默认 0.03 → 半衰期 ~23 天），比线性 ×0.9 更符合遗忘曲线
    pub fn decay_cold_edges(
        &self,
        cold_days: u32,
        alpha: f64,
        min_weight: f64,
    ) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let threshold = format!("-{cold_days} days");

        // 查询所有冷边及其不活跃天数，在 Rust 端计算 e^(-α×days)
        let mut stmt = conn.prepare(
            "SELECT id, weight,
                    julianday('now') - julianday(COALESCE(last_activated_at, created_at)) AS inactive_days
             FROM memory_edges
             WHERE last_activated_at IS NULL OR last_activated_at < datetime('now', ?1)",
        )?;
        let cold_edges: Vec<(i64, f64, f64)> = stmt
            .query_map(rusqlite::params![threshold], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut deleted = 0usize;
        for (id, weight, inactive_days) in &cold_edges {
            let new_weight = weight * (-alpha * inactive_days).exp();
            if new_weight >= min_weight {
                conn.execute(
                    "UPDATE memory_edges SET weight = ?1 WHERE id = ?2",
                    rusqlite::params![new_weight, id],
                )?;
            } else {
                conn.execute(
                    "DELETE FROM memory_edges WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                deleted += 1;
            }
        }

        Ok(deleted)
    }

    /// 获取指定记忆的所有相邻边（双向）
    pub fn get_memory_edges(&self, memory_id: i64) -> Result<Vec<MemoryEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, from_id, to_id, relation, weight, created_at
             FROM memory_edges
             WHERE from_id = ?1 OR to_id = ?1
             ORDER BY weight DESC",
            )
            .context("准备 get_memory_edges 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![memory_id], |row| {
            Ok(MemoryEdge {
                id: row.get(0)?,
                from_id: row.get(1)?,
                to_id: row.get(2)?,
                relation: row.get(3)?,
                weight: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取所有边（用于图谱可视化）
    pub fn get_all_memory_edges(&self) -> Result<Vec<MemoryEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.from_id, e.to_id, e.relation, e.weight, e.created_at
             FROM memory_edges e
             INNER JOIN memories m1 ON e.from_id = m1.id
             INNER JOIN memories m2 ON e.to_id = m2.id
             WHERE m1.status = 'active' AND m2.status = 'active'
             ORDER BY e.weight DESC",
            )
            .context("准备 get_all_memory_edges 查询失败")?;
        let rows = stmt.query_map([], |row| {
            Ok(MemoryEdge {
                id: row.get(0)?,
                from_id: row.get(1)?,
                to_id: row.get(2)?,
                relation: row.get(3)?,
                weight: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 删除指定的边
    pub fn delete_memory_edge(&self, edge_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM memory_edges WHERE id = ?1",
            rusqlite::params![edge_id],
        )
        .context("删除 memory_edge 失败")?;
        Ok(())
    }

    /// 图谱遍历：从起始记忆出发，获取 N 跳内的相关记忆（spreading activation）
    pub fn get_connected_memories(
        &self,
        start_id: i64,
        max_hops: usize,
    ) -> Result<Vec<(sage_types::Memory, f64)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;

        // BFS 遍历：收集 max_hops 内的所有连接节点及其衰减权重
        let mut visited: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
        visited.insert(start_id, 1.0);
        let mut frontier = vec![(start_id, 1.0)];

        for _hop in 0..max_hops {
            let mut next_frontier = Vec::new();
            for (node_id, activation) in &frontier {
                let mut stmt = conn
                    .prepare(
                        "SELECT from_id, to_id, weight FROM memory_edges
                     WHERE from_id = ?1 OR to_id = ?1",
                    )
                    .context("图谱遍历查询失败")?;
                let edges: Vec<(i64, i64, f64)> = stmt
                    .query_map(rusqlite::params![node_id], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();

                for (from, to, weight) in edges {
                    let neighbor = if from == *node_id { to } else { from };
                    let new_activation = activation * weight * 0.7; // 衰减因子 0.7
                    if new_activation > 0.1 {
                        let entry = visited.entry(neighbor).or_insert(0.0);
                        if new_activation > *entry {
                            *entry = new_activation;
                            next_frontier.push((neighbor, new_activation));
                        }
                    }
                }
            }
            frontier = next_frontier;
        }

        // 排除起始节点，查询记忆详情
        visited.remove(&start_id);
        let mut results = Vec::new();
        for (mem_id, activation) in &visited {
            let mem = conn.query_row(
                "SELECT id, category, content, source, confidence, visibility, created_at, updated_at, about_person, last_accessed_at, depth, valid_until, validation_count, derived_from, evolution_note, derivable
                 FROM memories WHERE id = ?1 AND status = 'active'",
                rusqlite::params![mem_id],
                Self::row_to_memory,
            ).optional()?;
            if let Some(m) = mem {
                results.push((m, *activation));
            }
        }
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }

    /// 统计图谱边数
    pub fn count_memory_edges(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_edges", [], |row| row.get(0))
            .context("统计 memory_edges 失败")?;
        Ok(count as usize)
    }

    /// 给记忆添加标签（忽略重复）
    pub fn add_tag(&self, memory_id: i64, tag: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
            rusqlite::params![memory_id, tag.trim().to_lowercase()],
        )
        .context("添加标签失败")?;
        Ok(())
    }

    /// 批量给记忆添加标签
    pub fn add_tags(&self, memory_id: i64, tags: &[&str]) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt =
            conn.prepare("INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)")?;
        for tag in tags {
            let t = tag.trim().to_lowercase();
            if !t.is_empty() {
                stmt.execute(rusqlite::params![memory_id, t])?;
            }
        }
        Ok(())
    }

    /// 获取某条记忆的所有标签
    pub fn get_tags(&self, memory_id: i64) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt =
            conn.prepare("SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag")?;
        let tags: Vec<String> = stmt
            .query_map([memory_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tags)
    }

    /// 获取所有标签及其记忆数量（按数量降序）
    pub fn get_all_tags(&self) -> Result<Vec<(String, usize)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT t.tag, COUNT(*) as cnt FROM memory_tags t
             JOIN memories m ON t.memory_id = m.id
             WHERE m.status = 'active'
             GROUP BY t.tag ORDER BY cnt DESC",
        )?;
        let tags: Vec<(String, usize)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tags)
    }

    /// 获取带有指定标签的所有记忆 ID
    pub fn get_memories_by_tag(&self, tag: &str) -> Result<Vec<i64>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare("SELECT memory_id FROM memory_tags WHERE tag = ?1")?;
        let ids: Vec<i64> = stmt
            .query_map([tag.trim().to_lowercase()], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    /// 删除记忆的某个标签
    pub fn remove_tag(&self, memory_id: i64, tag: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM memory_tags WHERE memory_id = ?1 AND tag = ?2",
            rusqlite::params![memory_id, tag.trim().to_lowercase()],
        )?;
        Ok(())
    }

    /// 保存知识图谱边（不同类型节点之间的连接），存在则更新权重
    pub fn save_knowledge_edge(
        &self,
        from_type: &str,
        from_id: i64,
        to_type: &str,
        to_id: i64,
        relation: &str,
        weight: f64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO knowledge_edges (from_type, from_id, to_type, to_id, relation, weight, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(from_type, from_id, to_type, to_id, relation) DO UPDATE SET weight = ?6",
            rusqlite::params![from_type, from_id, to_type, to_id, relation, weight, now],
        )
        .context("保存 knowledge_edge 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取指定节点的所有连接（双向）
    pub fn get_knowledge_edges(&self, node_type: &str, node_id: i64) -> Result<Vec<KnowledgeEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, from_type, from_id, to_type, to_id, relation, weight, created_at
             FROM knowledge_edges
             WHERE (from_type = ?1 AND from_id = ?2) OR (to_type = ?1 AND to_id = ?2)
             ORDER BY weight DESC",
            )
            .context("准备 get_knowledge_edges 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![node_type, node_id], row_to_knowledge_edge)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取两种类型之间的所有连接
    pub fn get_knowledge_edges_between_types(
        &self,
        from_type: &str,
        to_type: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, from_type, from_id, to_type, to_id, relation, weight, created_at
             FROM knowledge_edges
             WHERE from_type = ?1 AND to_type = ?2
             ORDER BY weight DESC LIMIT ?3",
            )
            .context("准备 get_knowledge_edges_between_types 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![from_type, to_type, limit as i64], row_to_knowledge_edge)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取所有知识图谱边（用于图谱可视化）
    pub fn get_all_knowledge_edges(&self) -> Result<Vec<KnowledgeEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, from_type, from_id, to_type, to_id, relation, weight, created_at
             FROM knowledge_edges
             ORDER BY weight DESC",
            )
            .context("准备 get_all_knowledge_edges 查询失败")?;
        let rows = stmt.query_map([], row_to_knowledge_edge)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 删除指定的知识图谱边
    pub fn delete_knowledge_edge(&self, edge_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM knowledge_edges WHERE id = ?1",
            rusqlite::params![edge_id],
        )
        .context("删除 knowledge_edge 失败")?;
        Ok(())
    }

    /// 统计知识图谱边数
    pub fn count_knowledge_edges(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM knowledge_edges", [], |row| row.get(0))
            .context("统计 knowledge_edges 失败")?;
        Ok(count as usize)
    }
}
