use anyhow::{Context, Result};

use super::{Store, TaskSignal};
use crate::similarity::text_similarity;

/// 语义去重阈值：相似度超过此值视为重复
const TASK_DEDUP_THRESHOLD: f64 = 0.6;

/// 去除 LLM 从上下文复制的 [id=XX] 标记，支持 [id=126] 和 [id=126/130/131] 格式
pub(crate) fn strip_id_markers(s: &str) -> String {
    let mut result = s.to_string();
    // 循环去除所有 [id=...] 模式（含前后空格）
    while let Some(start) = result.find("[id=") {
        if let Some(end) = result[start..].find(']') {
            let before = if start > 0 && result.as_bytes()[start - 1] == b' ' {
                start - 1
            } else {
                start
            };
            let after = start + end + 1;
            // 去掉 ] 后面紧跟的空格
            let after = if after < result.len() && result.as_bytes()[after] == b' ' {
                after + 1
            } else {
                after
            };
            result = format!("{}{}", &result[..before], &result[after..]);
        } else {
            break;
        }
    }
    result.trim().to_string()
}

impl Store {
    /// 创建任务，自动与 open/done/cancelled 任务语义去重。
    /// 返回 Ok(-1) 表示检测到重复，跳过创建。
    pub fn create_task(
        &self,
        content: &str,
        source: &str,
        source_id: Option<i64>,
        priority: Option<&str>,
        due_date: Option<&str>,
        description: Option<&str>,
    ) -> Result<i64> {
        // 先清理 [id=XX] 标记
        let clean_content = strip_id_markers(content);
        let content = if clean_content.is_empty() { content } else { &clean_content };

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;

        // 语义去重：与 open/done/cancelled 任务比较（deleted 的已从 DB 移除，不参与）
        let mut stmt = conn.prepare(
            "SELECT content FROM tasks WHERE status IN ('open', 'done', 'cancelled')",
        )?;
        let existing: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        for existing_content in &existing {
            let clean_existing = strip_id_markers(existing_content);
            if text_similarity(&clean_existing, content) > TASK_DEDUP_THRESHOLD {
                tracing::debug!(
                    "Task dedup: skip '{}' (similar to '{}')",
                    content.chars().take(40).collect::<String>(),
                    clean_existing.chars().take(40).collect::<String>(),
                );
                return Ok(-1);
            }
        }

        let normalized_due = due_date.map(|d| sage_types::normalize_timestamp(d));
        conn.execute(
            "INSERT INTO tasks (content, source, source_id, priority, due_date, description) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![content, source, source_id, priority.unwrap_or("normal"), normalized_due, description],
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
        let normalized = due_date.map(|d| sage_types::normalize_timestamp(d));
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE tasks SET due_date = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![normalized, task_id],
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

    /// Get recently accepted signals (to prevent LLM re-suggesting same topics)
    pub fn get_recent_accepted_signals(&self, limit: usize) -> Result<Vec<TaskSignal>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, signal_type, task_id, title, evidence, suggested_outcome, status, created_at, importance
             FROM task_signals WHERE status = 'accepted' AND created_at > datetime('now', '-7 days')
             ORDER BY created_at DESC LIMIT ?1"
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
        // 去重：new_task 类型按语义去重（pending signals + 现有 open/done/cancelled tasks）
        if signal_type == "new_task" {
            // 用 suggested_outcome（纯任务内容）做去重，避免 title 的 "Suggested new task: " 前缀稀释相似度
            let dedup_text = suggested_outcome.unwrap_or(title);
            let clean_text = strip_id_markers(dedup_text);
            // 1. 与 pending 的 new_task signals 语义比较（优先用 suggested_outcome）
            let mut sig_stmt = conn.prepare(
                "SELECT COALESCE(suggested_outcome, title) FROM task_signals WHERE signal_type = 'new_task' AND status = 'pending'",
            )?;
            let sig_texts: Vec<String> = sig_stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            for t in &sig_texts {
                if text_similarity(&strip_id_markers(t), &clean_text) > TASK_DEDUP_THRESHOLD {
                    return Ok(-1);
                }
            }

            // 2. 与现有 open/done/cancelled 任务语义比较
            let mut task_stmt = conn.prepare(
                "SELECT content FROM tasks WHERE status IN ('open', 'done', 'cancelled')",
            )?;
            let task_contents: Vec<String> = task_stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            for c in &task_contents {
                if text_similarity(&strip_id_markers(c), &clean_text) > TASK_DEDUP_THRESHOLD {
                    return Ok(-1);
                }
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

    /// 通用 KV 读写（用于 evolution 进度等轻量状态）
    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO kv_store (key, value, updated_at) VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare("SELECT value FROM kv_store WHERE key = ?1")?;
        let result = stmt.query_row(rusqlite::params![key], |row| row.get::<_, String>(0));
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn kv_delete(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM kv_store WHERE key = ?1", rusqlite::params![key])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    #[test]
    fn dedup_blocks_similar_open_task() {
        let s = store();
        let id1 = s.create_task("完成项目报告", "test", None, None, None, None).unwrap();
        assert!(id1 > 0);
        // 语义相似的任务应被去重
        let id2 = s.create_task("完成项目的报告", "test", None, None, None, None).unwrap();
        assert_eq!(id2, -1);
    }

    #[test]
    fn dedup_blocks_similar_done_task() {
        let s = store();
        let id = s.create_task("发送周报邮件", "test", None, None, None, None).unwrap();
        s.update_task_status(id, "done").unwrap();
        // done 的任务也参与去重
        let id2 = s.create_task("发送周报的邮件", "test", None, None, None, None).unwrap();
        assert_eq!(id2, -1);
    }

    #[test]
    fn dedup_blocks_similar_cancelled_task() {
        let s = store();
        let id = s.create_task("准备会议材料", "test", None, None, None, None).unwrap();
        s.update_task_status(id, "cancelled").unwrap();
        // cancelled 的任务也参与去重
        let id2 = s.create_task("准备会议的材料", "test", None, None, None, None).unwrap();
        assert_eq!(id2, -1);
    }

    #[test]
    fn dedup_allows_after_delete() {
        let s = store();
        let id = s.create_task("买咖啡豆", "test", None, None, None, None).unwrap();
        s.delete_task(id).unwrap();
        // deleted 的任务不参与去重，可以重新创建
        let id2 = s.create_task("买咖啡豆", "test", None, None, None, None).unwrap();
        assert!(id2 > 0);
    }

    #[test]
    fn dedup_allows_different_content() {
        let s = store();
        s.create_task("写代码", "test", None, None, None, None).unwrap();
        // 完全不同的任务应该正常创建
        let id2 = s.create_task("去超市买菜", "test", None, None, None, None).unwrap();
        assert!(id2 > 0);
    }

    #[test]
    fn signal_dedup_semantic_with_existing_tasks() {
        let s = store();
        s.create_task("整理项目文档", "test", None, None, None, None).unwrap();
        // new_task signal 与现有 task 语义相似，应被去重
        let sig_id = s.save_task_signal("new_task", None, "整理项目的文档", "test evidence", None).unwrap();
        assert_eq!(sig_id, -1);
    }

    #[test]
    fn signal_dedup_semantic_with_pending_signals() {
        let s = store();
        let sig1 = s.save_task_signal("new_task", None, "检查服务器状态", "evidence", None).unwrap();
        assert!(sig1 > 0);
        // 语义相似的 signal 应被去重
        let sig2 = s.save_task_signal("new_task", None, "检查服务器的状态", "evidence", None).unwrap();
        assert_eq!(sig2, -1);
    }

    // --- strip_id_markers ---

    #[test]
    fn strip_single_id() {
        assert_eq!(strip_id_markers("[id=130] codex会议结论未整理"), "codex会议结论未整理");
    }

    #[test]
    fn strip_multi_id() {
        assert_eq!(
            strip_id_markers("[id=126/130/131]三任务重叠"),
            "三任务重叠"
        );
    }

    #[test]
    fn strip_mid_text_id() {
        assert_eq!(
            strip_id_markers("codex会议结论[id=126]已整理"),
            "codex会议结论已整理"
        );
    }

    #[test]
    fn strip_no_id() {
        assert_eq!(strip_id_markers("正常任务内容"), "正常任务内容");
    }

    #[test]
    fn dedup_ignores_id_markers() {
        let s = store();
        let id1 = s.create_task("[id=126] codex会议结论整理", "ai_signal", None, None, None, None).unwrap();
        assert!(id1 > 0);
        // 同一任务带不同 [id=XX] 应被去重
        let id2 = s.create_task("[id=130] codex会议结论整理", "ai_signal", None, None, None, None).unwrap();
        assert_eq!(id2, -1);
    }

    #[test]
    fn stored_content_has_no_id_markers() {
        let s = store();
        let id = s.create_task("[id=126] codex会议结论整理", "ai_signal", None, None, None, None).unwrap();
        assert!(id > 0);
        let tasks = s.list_tasks(Some("open"), 10).unwrap();
        let content = &tasks[0].1;
        assert!(!content.contains("[id="), "stored content should not contain [id=]: {content}");
    }

    #[test]
    fn signal_dedup_uses_suggested_outcome_not_title() {
        let s = store();
        // 创建一个已有任务
        s.create_task("codex会议结论整理", "manual", None, None, None, None).unwrap();
        // 用装饰过的 title + 纯内容 suggested_outcome 保存信号
        // 以前 bug: title "Suggested new task: codex会议结论" 与 "codex会议结论整理" 相似度低，不会去重
        // 修复后: 用 suggested_outcome "codex会议结论整理" 做比较，应命中去重
        let sig_id = s.save_task_signal(
            "new_task", None,
            "Suggested new task: codex会议结论",
            "evidence",
            Some("codex会议结论整理"),
        ).unwrap();
        assert_eq!(sig_id, -1, "should dedup against existing task using suggested_outcome");
    }

    #[test]
    fn get_recent_accepted_signals_returns_accepted() {
        let s = store();
        let sig_id = s.save_task_signal("new_task", None, "测试任务", "evidence", Some("测试任务内容")).unwrap();
        assert!(sig_id > 0);
        // 标记为 accepted
        s.update_signal_status(sig_id, "accepted").unwrap();
        let accepted = s.get_recent_accepted_signals(10).unwrap();
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].id, sig_id);
        assert_eq!(accepted[0].status, "accepted");
    }
}
