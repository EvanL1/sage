use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sage_types::{FeedbackAction, Suggestion};

use super::Store;

impl Store {
    /// 检查 12 小时内是否有相同 (event_source, prompt) 的建议
    pub fn has_recent_suggestion(&self, event_source: &str, prompt: &str) -> bool {
        let conn = self.conn.lock().ok();
        let Some(conn) = conn else { return false };
        let threshold = (chrono::Local::now() - chrono::Duration::hours(12)).to_rfc3339();
        conn.query_row(
            "SELECT 1 FROM suggestions WHERE event_source = ?1 AND prompt = ?2 AND created_at > ?3 LIMIT 1",
            rusqlite::params![event_source, prompt, threshold],
            |_| Ok(()),
        ).is_ok()
    }

    /// 插入建议记录，返回自增 id
    pub fn record_suggestion(
        &self,
        event_source: &str,
        prompt: &str,
        response: &str,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;

        // Store 级别去重：同源 + 同 prompt 在 12 小时内不重复创建
        let threshold = (chrono::Local::now() - chrono::Duration::hours(12)).to_rfc3339();
        let existing_id: Option<i64> = conn.query_row(
            "SELECT id FROM suggestions WHERE event_source = ?1 AND prompt = ?2 AND created_at > ?3 ORDER BY id DESC LIMIT 1",
            rusqlite::params![event_source, prompt, threshold],
            |row| row.get(0),
        ).ok();
        if let Some(id) = existing_id {
            return Ok(id);
        }

        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO suggestions (event_source, prompt, response, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![event_source, prompt, response, now],
        ).context("记录 suggestion 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 按时间倒序获取最近的建议
    pub fn get_recent_suggestions(&self, limit: usize) -> Result<Vec<Suggestion>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.event_source, s.prompt, s.response, s.created_at,
                    (SELECT f.action FROM feedback f WHERE f.suggestion_id = s.id ORDER BY f.id DESC LIMIT 1)
             FROM suggestions s WHERE s.event_source NOT LIKE '\\_%' ESCAPE '\\' ORDER BY s.created_at DESC LIMIT ?1",
        ).context("准备 get_recent_suggestions 查询失败")?;

        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                let id: i64 = row.get(0)?;
                let event_source: String = row.get(1)?;
                let prompt: String = row.get(2)?;
                let response: String = row.get(3)?;
                let created_at: String = row.get(4)?;
                let feedback_json: Option<String> = row.get(5)?;
                Ok((
                    id,
                    event_source,
                    prompt,
                    response,
                    created_at,
                    feedback_json,
                ))
            })
            .context("执行 get_recent_suggestions 查询失败")?;

        let mut suggestions = Vec::new();
        for row in rows {
            let (id, event_source, prompt, response, created_at, feedback_json) = row?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&chrono::Local))
                .unwrap_or_else(|_| chrono::Local::now());
            let feedback =
                feedback_json.and_then(|json| serde_json::from_str::<FeedbackAction>(&json).ok());
            suggestions.push(Suggestion {
                id,
                event_source,
                prompt,
                response,
                timestamp,
                feedback,
            });
        }
        Ok(suggestions)
    }

    /// 获取最近一条 questioner 生成的每日问题（event_source='questioner', prompt='daily-question'）
    pub fn get_daily_question(&self) -> Result<Option<Suggestion>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let result = conn.query_row(
            "SELECT s.id, s.event_source, s.prompt, s.response, s.created_at,
                    (SELECT f.action FROM feedback f WHERE f.suggestion_id = s.id ORDER BY f.id DESC LIMIT 1)
             FROM suggestions s
             WHERE s.event_source = 'questioner' AND s.prompt = 'daily-question'
             ORDER BY s.created_at DESC
             LIMIT 1",
            [],
            |row| {
                let id: i64 = row.get(0)?;
                let event_source: String = row.get(1)?;
                let prompt: String = row.get(2)?;
                let response: String = row.get(3)?;
                let created_at: String = row.get(4)?;
                let feedback_json: Option<String> = row.get(5)?;
                Ok((id, event_source, prompt, response, created_at, feedback_json))
            },
        ).optional().context("查询每日问题失败")?;

        match result {
            None => Ok(None),
            Some((id, event_source, prompt, response, created_at, feedback_json)) => {
                let timestamp = chrono::DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&chrono::Local))
                    .unwrap_or_else(|_| chrono::Local::now());
                let feedback = feedback_json
                    .and_then(|json| serde_json::from_str::<FeedbackAction>(&json).ok());
                Ok(Some(Suggestion {
                    id,
                    event_source,
                    prompt,
                    response,
                    timestamp,
                    feedback,
                }))
            }
        }
    }

    /// 删除指定 suggestion 及其关联 feedback
    pub fn delete_suggestion(&self, suggestion_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM feedback WHERE suggestion_id = ?1",
            rusqlite::params![suggestion_id],
        )
        .context("删除关联 feedback 失败")?;
        conn.execute(
            "DELETE FROM suggestions WHERE id = ?1",
            rusqlite::params![suggestion_id],
        )
        .context("删除 suggestion 失败")?;
        Ok(())
    }

    /// 更新 suggestion 的 response 内容
    pub fn update_suggestion_response(&self, suggestion_id: i64, response: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let affected = conn
            .execute(
                "UPDATE suggestions SET response = ?1 WHERE id = ?2",
                rusqlite::params![response, suggestion_id],
            )
            .context("更新 suggestion 失败")?;
        if affected == 0 {
            anyhow::bail!("Suggestion {suggestion_id} not found");
        }
        Ok(())
    }

    /// 记录反馈
    pub fn record_feedback(&self, suggestion_id: i64, action: &FeedbackAction) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let action_json = serde_json::to_string(action).context("序列化 FeedbackAction 失败")?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO feedback (suggestion_id, action, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![suggestion_id, action_json, now],
        )
        .context("记录 feedback 失败")?;
        Ok(())
    }

    /// 统计特定 action 类型的反馈数量（按 event_source 关联）
    pub fn count_feedback_by_type(&self, action_type: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{action_type}%");
        let count: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback WHERE action LIKE ?1",
                rusqlite::params![pattern],
                |row| row.get(0),
            )
            .context("统计 feedback 失败")?;
        Ok(count)
    }

    /// 统计某个 event_source 下特定 action 类型的反馈数量
    pub fn count_feedback_by_source_and_type(
        &self,
        event_source: &str,
        action_type: &str,
    ) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{action_type}%");
        let count: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback f
                 JOIN suggestions s ON f.suggestion_id = s.id
                 WHERE s.event_source = ?1 AND f.action LIKE ?2",
                rusqlite::params![event_source, pattern],
                |row| row.get(0),
            )
            .context("统计 feedback by source 失败")?;
        Ok(count)
    }

    /// 读取最近 N 条 suggestions 及其 feedback，返回 (event_source, response, feedback_action) 三元组
    pub fn get_suggestions_with_feedback(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT s.event_source, s.response, f.action
                 FROM suggestions s
                 LEFT JOIN feedback f ON f.suggestion_id = s.id
                   AND f.id = (SELECT MAX(f2.id) FROM feedback f2 WHERE f2.suggestion_id = s.id)
                 ORDER BY s.created_at DESC LIMIT ?1",
            )
            .context("准备 get_suggestions_with_feedback 查询失败")?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                let event_source: String = row.get(0)?;
                let response: String = row.get(1)?;
                let action: Option<String> = row.get(2)?;
                Ok((event_source, response, action))
            })
            .context("执行 get_suggestions_with_feedback 查询失败")?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}
