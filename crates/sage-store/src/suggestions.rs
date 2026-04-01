use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sage_types::{FeedbackAction, Suggestion};

use super::Store;

impl Store {
    /// 检查 12 小时内是否有相同 (event_source, prompt) 的建议
    pub fn has_recent_suggestion(&self, event_source: &str, prompt: &str) -> bool {
        let Some(conn) = self.conn_or_warn() else { return false; };
        let threshold = (chrono::Local::now() - chrono::Duration::hours(12)).to_rfc3339();
        conn.query_row(
            "SELECT 1 FROM suggestions WHERE event_source = ?1 AND prompt = ?2 AND created_at > ?3 LIMIT 1",
            rusqlite::params![event_source, prompt, threshold],
            |_| Ok(()),
        ).is_ok()
    }

    /// 按事件标题去重（12 小时内，prompt 包含标题关键词）
    /// 搜索 prompt 而非 response，因为 prompt 总是包含英文标题，但 response 可能是中文
    pub fn has_recent_suggestion_by_title(&self, event_source: &str, title: &str) -> bool {
        let Some(conn) = self.conn_or_warn() else { return false; };
        let threshold = (chrono::Local::now() - chrono::Duration::hours(12)).to_rfc3339();
        // 用标题前 20 字符做模糊匹配（避免时间戳等变化导致的不匹配）
        let key: String = title.chars().take(20).collect();
        let escaped = key.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        conn.query_row(
            "SELECT 1 FROM suggestions WHERE event_source = ?1 AND prompt LIKE ?2 ESCAPE '\\' AND created_at > ?3 LIMIT 1",
            rusqlite::params![event_source, pattern, threshold],
            |_| Ok(()),
        ).is_ok()
    }

    /// 插入建议记录，返回自增 id
    /// 对 heartbeat 类型：同天同 prompt 前缀只保留一条（upsert）
    pub fn record_suggestion(
        &self,
        event_source: &str,
        prompt: &str,
        response: &str,
    ) -> Result<i64> {
        let conn = self.conn()?;

        let now = chrono::Local::now().to_rfc3339();

        // heartbeat 类型：同天同源只保留一条，重复生成时更新内容
        // 用 LIKE 匹配 prompt 中的标题关键词（prompt 包含动态时间，不能精确匹配）
        if event_source == "heartbeat" {
            // 提取标题：prompt 格式为 "...处理定时任务：{title}\n..." 或 "...{title}：\n..."
            let stable_key = extract_stable_key(prompt);
            let existing_id: Option<i64> = conn.query_row(
                "SELECT id FROM suggestions WHERE event_source = 'heartbeat' AND prompt LIKE ?1 ESCAPE '\\' AND created_at >= ?2 ORDER BY id DESC LIMIT 1",
                rusqlite::params![format!("%{stable_key}%"), crate::today_start()],
                |row| row.get(0),
            ).ok();
            if let Some(id) = existing_id {
                conn.execute(
                    "UPDATE suggestions SET response = ?1, created_at = ?2 WHERE id = ?3",
                    rusqlite::params![response, now, id],
                ).context("更新 heartbeat suggestion 失败")?;
                return Ok(id);
            }
        } else {
            // 非 heartbeat：12 小时去重
            let threshold = (chrono::Local::now() - chrono::Duration::hours(12)).to_rfc3339();
            let existing_id: Option<i64> = conn.query_row(
                "SELECT id FROM suggestions WHERE event_source = ?1 AND prompt = ?2 AND created_at > ?3 ORDER BY id DESC LIMIT 1",
                rusqlite::params![event_source, prompt, threshold],
                |row| row.get(0),
            ).ok();
            if let Some(id) = existing_id {
                return Ok(id);
            }
        }

        conn.execute(
            "INSERT INTO suggestions (event_source, prompt, response, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![event_source, prompt, response, now],
        ).context("记录 suggestion 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 按时间倒序获取最近的建议
    pub fn get_recent_suggestions(&self, limit: usize) -> Result<Vec<Suggestion>> {
        let conn = self.conn()?;
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
        let conn = self.conn()?;
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
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM feedback WHERE suggestion_id = ?1",
            rusqlite::params![suggestion_id],
        )
        .context("删除关联 feedback 失败")?;
        tx.execute(
            "DELETE FROM suggestions WHERE id = ?1",
            rusqlite::params![suggestion_id],
        )
        .context("删除 suggestion 失败")?;
        tx.commit()?;
        Ok(())
    }

    /// 更新 suggestion 的 response 内容
    pub fn update_suggestion_response(&self, suggestion_id: i64, response: &str) -> Result<()> {
        let conn = self.conn()?;
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
        let conn = self.conn()?;
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
        let conn = self.conn()?;
        let pattern = format!("%{action_type}%");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback WHERE action LIKE ?1",
                rusqlite::params![pattern],
                |row| row.get(0),
            )
            .context("统计 feedback 失败")?;
        Ok(count as usize)
    }

    /// 统计某个 event_source 下特定 action 类型的反馈数量
    pub fn count_feedback_by_source_and_type(
        &self,
        event_source: &str,
        action_type: &str,
    ) -> Result<usize> {
        let conn = self.conn()?;
        let pattern = format!("%{action_type}%");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback f
                 JOIN suggestions s ON f.suggestion_id = s.id
                 WHERE s.event_source = ?1 AND f.action LIKE ?2",
                rusqlite::params![event_source, pattern],
                |row| row.get(0),
            )
            .context("统计 feedback by source 失败")?;
        Ok(count as usize)
    }

    /// 读取最近 N 条 suggestions 及其 feedback，返回 (event_source, response, feedback_action) 三元组
    pub fn get_suggestions_with_feedback(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let conn = self.conn()?;
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

/// 从 heartbeat prompt 中提取稳定的去重关键词（去掉动态时间部分）
/// prompt 格式："当前时间：2026-03-25 10:58...处理定时任务：Meeting Update\n..."
/// 或 "当前时间：...生成今日 Morning Brief：..."
fn extract_stable_key(prompt: &str) -> String {
    // 去掉第一行（包含动态时间的 time_header），取后面的内容前 60 字符
    let after_time = prompt
        .find('\n')
        .map(|i| &prompt[i + 1..])
        .unwrap_or(prompt);
    let key: String = after_time.chars().take(60).collect();
    // 转义 SQL LIKE 特殊字符
    key.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}
