use anyhow::{Context, Result};

use super::{ReflectiveSignalRow, Store};

impl Store {
    /// 保存反思信号 / Save a reflective signal
    #[allow(clippy::too_many_arguments)]
    pub fn save_reflective_signal(
        &self,
        source: &str,
        signal_type: &str,
        raw_text: &str,
        context: Option<&str>,
        baseline_divergence: f64,
        armor_pattern: Option<&str>,
        intensity: f64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO reflective_signals
                (timestamp, source, signal_type, raw_text, context,
                 baseline_divergence, armor_pattern, intensity, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                now,
                source,
                signal_type,
                raw_text,
                context,
                baseline_divergence,
                armor_pattern,
                intensity,
                now,
            ],
        )
        .context("保存 reflective_signal 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取未解决的反思信号 / Get unresolved reflective signals
    pub fn get_unresolved_signals(&self, limit: usize) -> Result<Vec<ReflectiveSignalRow>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, timestamp, source, signal_type, raw_text, context,
                        baseline_divergence, armor_pattern, intensity,
                        resolved, resolution_text, created_at
                 FROM reflective_signals WHERE resolved = 0
                 ORDER BY created_at DESC LIMIT ?1",
            )
            .context("准备 get_unresolved_signals 查询失败")?;
        let rows = stmt
            .query_map(rusqlite::params![limit], Self::map_signal_row)
            .context("执行 get_unresolved_signals 查询失败")?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取指定日期之后的所有反思信号 / Get signals since a date
    pub fn get_signals_since(&self, since: &str) -> Result<Vec<ReflectiveSignalRow>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, timestamp, source, signal_type, raw_text, context,
                        baseline_divergence, armor_pattern, intensity,
                        resolved, resolution_text, created_at
                 FROM reflective_signals WHERE created_at >= ?1
                 ORDER BY created_at ASC",
            )
            .context("准备 get_signals_since 查询失败")?;
        let rows = stmt
            .query_map(rusqlite::params![since], Self::map_signal_row)
            .context("执行 get_signals_since 查询失败")?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 标记信号为已解决 / Resolve a signal
    pub fn resolve_signal(&self, id: i64, resolution_text: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE reflective_signals SET resolved = 1, resolution_text = ?1 WHERE id = ?2",
            rusqlite::params![resolution_text, id],
        )
        .context("resolve_signal 失败")?;
        Ok(())
    }

    /// 按类型统计信号数 / Count signals by type
    pub fn count_signals_by_type(&self) -> Result<Vec<(String, usize)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT signal_type, COUNT(*) FROM reflective_signals
                 GROUP BY signal_type ORDER BY COUNT(*) DESC",
            )
            .context("准备 count_signals_by_type 查询失败")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?)))
            .context("执行 count_signals_by_type 查询失败")?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn map_signal_row(row: &rusqlite::Row) -> rusqlite::Result<ReflectiveSignalRow> {
        Ok(ReflectiveSignalRow {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            source: row.get(2)?,
            signal_type: row.get(3)?,
            raw_text: row.get(4)?,
            context: row.get(5)?,
            baseline_divergence: row.get(6)?,
            armor_pattern: row.get(7)?,
            intensity: row.get(8)?,
            resolved: row.get::<_, i32>(9)? != 0,
            resolution_text: row.get(10)?,
            created_at: row.get(11)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Store {
        Store::open_in_memory().expect("创建测试数据库失败")
    }

    #[test]
    fn save_and_load_signal() {
        let store = test_store();
        let id = store
            .save_reflective_signal(
                "chat",
                "uncertainty",
                "如果我能更勇敢一点",
                Some("讨论职业选择"),
                0.6,
                Some("decisive_action"),
                0.7,
            )
            .unwrap();
        assert!(id > 0);

        let signals = store.get_unresolved_signals(10).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].signal_type, "uncertainty");
        assert_eq!(signals[0].raw_text, "如果我能更勇敢一点");
        assert!(!signals[0].resolved);
    }

    #[test]
    fn resolve_signal() {
        let store = test_store();
        let id = store
            .save_reflective_signal("chat", "vulnerability", "说实话我很焦虑", None, 0.8, None, 0.9)
            .unwrap();

        store.resolve_signal(id, "转化为行动计划").unwrap();

        let unresolved = store.get_unresolved_signals(10).unwrap();
        assert!(unresolved.is_empty());
    }

    #[test]
    fn get_signals_since() {
        let store = test_store();
        store
            .save_reflective_signal("chat", "blocked_state", "一直在等审批", None, 0.5, None, 0.6)
            .unwrap();

        let signals = store.get_signals_since("2000-01-01").unwrap();
        assert_eq!(signals.len(), 1);

        let empty = store.get_signals_since("2099-01-01").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn count_by_type() {
        let store = test_store();
        store
            .save_reflective_signal("chat", "uncertainty", "也许吧", None, 0.3, None, 0.4)
            .unwrap();
        store
            .save_reflective_signal("chat", "uncertainty", "可能是", None, 0.3, None, 0.5)
            .unwrap();
        store
            .save_reflective_signal("note", "vulnerability", "承认很难", None, 0.7, None, 0.8)
            .unwrap();

        let counts = store.count_signals_by_type().unwrap();
        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0], ("uncertainty".to_string(), 2));
        assert_eq!(counts[1], ("vulnerability".to_string(), 1));
    }

    #[test]
    fn multiple_signals_ordering() {
        let store = test_store();
        for i in 0..5 {
            store
                .save_reflective_signal(
                    "chat",
                    "self_analysis",
                    &format!("我发现自己的模式 {i}"),
                    None,
                    0.5,
                    None,
                    0.5 + i as f64 * 0.1,
                )
                .unwrap();
        }

        // limit = 3, DESC order → newest first
        let signals = store.get_unresolved_signals(3).unwrap();
        assert_eq!(signals.len(), 3);
        assert!(signals[0].id > signals[1].id);
    }

    #[test]
    fn resolved_excluded_from_unresolved() {
        let store = test_store();
        let id1 = store
            .save_reflective_signal("chat", "contradiction", "其实我也不确定", None, 0.4, None, 0.5)
            .unwrap();
        store
            .save_reflective_signal("chat", "blocked_state", "卡住了", None, 0.6, None, 0.7)
            .unwrap();

        store.resolve_signal(id1, "想通了").unwrap();

        let unresolved = store.get_unresolved_signals(10).unwrap();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].signal_type, "blocked_state");
    }
}
