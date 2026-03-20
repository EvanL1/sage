use anyhow::{Context, Result};

use super::{Store, TaskSignal};

impl Store {
    pub fn create_task(
        &self,
        content: &str,
        source: &str,
        source_id: Option<i64>,
        priority: Option<&str>,
        due_date: Option<&str>,
        description: Option<&str>,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT INTO tasks (content, source, source_id, priority, due_date, description) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![content, source, source_id, priority.unwrap_or("normal"), due_date, description],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Returns (id, content, status, priority, due_date, source, created_at, updated_at, outcome, verification, description)
    #[allow(clippy::type_complexity)]
    pub fn list_tasks(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> Result<
        Vec<(
            i64,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        )>,
    > {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(s) =
            status
        {
            ("SELECT id, content, status, priority, due_date, source, created_at, updated_at, outcome, verification, description FROM tasks \
              WHERE status = ?1 ORDER BY \
              CASE WHEN due_date IS NOT NULL THEN 0 ELSE 1 END, due_date ASC, created_at DESC LIMIT ?2",
             vec![Box::new(s.to_string()), Box::new(limit as i64)])
        } else {
            ("SELECT id, content, status, priority, due_date, source, created_at, updated_at, outcome, verification, description FROM tasks \
              ORDER BY CASE status WHEN 'open' THEN 0 WHEN 'done' THEN 1 ELSE 2 END, \
              CASE WHEN due_date IS NOT NULL THEN 0 ELSE 1 END, due_date ASC, created_at DESC LIMIT ?1",
             vec![Box::new(limit as i64)])
        };
        let mut stmt = conn.prepare(sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn update_task_verification(&self, task_id: i64, verification: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE tasks SET verification = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![verification, task_id],
        )?;
        Ok(())
    }

    pub fn update_task_status(&self, task_id: i64, status: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE tasks SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![status, task_id],
        )?;
        Ok(())
    }

    pub fn update_task_with_outcome(
        &self,
        task_id: i64,
        status: &str,
        outcome: Option<&str>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE tasks SET status = ?1, outcome = ?2, updated_at = datetime('now') WHERE id = ?3",
            rusqlite::params![status, outcome, task_id],
        )?;
        Ok(())
    }

    pub fn update_task_due_date(&self, task_id: i64, due_date: Option<&str>) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE tasks SET due_date = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![due_date, task_id],
        )?;
        Ok(())
    }

    pub fn update_task(
        &self,
        task_id: i64,
        content: &str,
        priority: Option<&str>,
        due_date: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE tasks SET content = ?1, priority = ?2, due_date = ?3, description = ?4, updated_at = datetime('now') WHERE id = ?5",
            rusqlite::params![content, priority.unwrap_or("normal"), due_date, description, task_id],
        )?;
        Ok(())
    }

    /// Returns (content, status, priority, due_date, description, outcome)
    pub fn get_task(
        &self,
        task_id: i64,
    ) -> Result<Option<(String, String, String, Option<String>, Option<String>, Option<String>)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT content, status, priority, due_date, description, outcome FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![task_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn delete_task(&self, task_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM tasks WHERE id = ?1",
            rusqlite::params![task_id],
        )?;
        Ok(())
    }

    pub fn save_task_signal(
        &self,
        signal_type: &str,
        task_id: Option<i64>,
        title: &str,
        evidence: &str,
        suggested_outcome: Option<&str>,
    ) -> Result<i64> {
        self.save_task_signal_inner(signal_type, task_id, title, evidence, suggested_outcome, None)
    }

    pub fn get_pending_signals(&self) -> Result<Vec<TaskSignal>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, signal_type, task_id, title, evidence, suggested_outcome, status, created_at, importance
             FROM task_signals WHERE status = 'pending' ORDER BY importance DESC, created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TaskSignal {
                id: row.get(0)?,
                signal_type: row.get(1)?,
                task_id: row.get(2)?,
                title: row.get(3)?,
                evidence: row.get(4)?,
                suggested_outcome: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                importance: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn update_signal_status(&self, signal_id: i64, status: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE task_signals SET status = ?1 WHERE id = ?2",
            rusqlite::params![status, signal_id],
        )?;
        Ok(())
    }

    /// Get recently dismissed signals (to prevent re-suggesting)
    pub fn get_recent_dismissed_signals(&self, limit: usize) -> Result<Vec<TaskSignal>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, signal_type, task_id, title, evidence, suggested_outcome, status, created_at, importance
             FROM task_signals WHERE status = 'dismissed' ORDER BY created_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(TaskSignal {
                id: row.get(0)?,
                signal_type: row.get(1)?,
                task_id: row.get(2)?,
                title: row.get(3)?,
                evidence: row.get(4)?,
                suggested_outcome: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                importance: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Auto-dismiss signals older than 3 days
    pub fn dismiss_old_signals(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let n = conn.execute(
            "UPDATE task_signals SET status = 'dismissed'
             WHERE status = 'pending' AND created_at < datetime('now', '-3 days')",
            [],
        )?;
        Ok(n)
    }

    /// 保存携带重要性分数的任务信号
    pub fn save_task_signal_with_importance(
        &self,
        signal_type: &str,
        task_id: Option<i64>,
        title: &str,
        evidence: &str,
        suggested_outcome: Option<&str>,
        importance: f32,
    ) -> Result<i64> {
        self.save_task_signal_inner(signal_type, task_id, title, evidence, suggested_outcome, Some(importance))
    }

    /// 内部统一实现：去重 + 插入
    fn save_task_signal_inner(
        &self,
        signal_type: &str,
        task_id: Option<i64>,
        title: &str,
        evidence: &str,
        suggested_outcome: Option<&str>,
        importance: Option<f32>,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        // 去重：同 task_id + signal_type 若已有 pending 信号则跳过
        if let Some(tid) = task_id {
            let exists: bool = conn.query_row(
                "SELECT COUNT(*) FROM task_signals WHERE task_id = ?1 AND signal_type = ?2 AND status = 'pending'",
                rusqlite::params![tid, signal_type],
                |row| row.get::<_, i64>(0),
            ).unwrap_or(0) > 0;
            if exists {
                return Ok(-1);
            }
        }
        // 去重：new_task 类型按 title 内容去重
        if signal_type == "new_task" {
            let exists: bool = conn.query_row(
                "SELECT COUNT(*) FROM task_signals WHERE signal_type = 'new_task' AND title = ?1 AND status = 'pending'",
                rusqlite::params![title],
                |row| row.get::<_, i64>(0),
            ).unwrap_or(0) > 0;
            if exists {
                return Ok(-1);
            }
        }
        let imp = importance.unwrap_or(0.5);
        conn.execute(
            "INSERT INTO task_signals (signal_type, task_id, title, evidence, suggested_outcome, status, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
            rusqlite::params![signal_type, task_id, title, evidence, suggested_outcome, imp],
        )
        .context("保存 task_signal 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 返回窗口内 (accepted_count, total_count)
    pub fn get_signal_accept_rate(&self, window_days: u32) -> Result<(usize, usize)> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let window = format!("-{} days", window_days);
        let total: usize = conn.query_row(
            "SELECT COUNT(*) FROM task_signals WHERE status IN ('accepted', 'dismissed') AND created_at > datetime('now', ?1)",
            rusqlite::params![window],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) as usize;
        let accepted: usize = conn.query_row(
            "SELECT COUNT(*) FROM task_signals WHERE status = 'accepted' AND created_at > datetime('now', ?1)",
            rusqlite::params![window],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) as usize;
        Ok((accepted, total))
    }

    /// 读取重要性阈值，默认 0.65
    pub fn get_importance_threshold(&self) -> Result<f32> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let result: Option<String> = conn
            .query_row(
                "SELECT value FROM kv_store WHERE key = 'importance_threshold'",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(result
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.65))
    }

    /// 写入重要性阈值
    pub fn set_importance_threshold(&self, value: f32) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO kv_store (key, value, updated_at) VALUES ('importance_threshold', ?1, datetime('now'))",
            rusqlite::params![value.to_string()],
        )?;
        Ok(())
    }
}
