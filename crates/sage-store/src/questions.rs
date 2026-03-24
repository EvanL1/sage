use anyhow::{Context, Result};

use super::Store;

impl Store {
    /// 保存开放问题
    pub fn save_open_question(
        &self,
        question_text: &str,
        source_suggestion_id: Option<i64>,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let next_ask = (chrono::Local::now() + chrono::Duration::days(3))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        conn.execute(
            "INSERT INTO open_questions (question_text, source_suggestion_id, status, ask_count, next_ask_at, created_at)
             VALUES (?1, ?2, 'open', 1, ?3, ?4)",
            rusqlite::params![question_text, source_suggestion_id, next_ask, now],
        ).context("保存 open_question 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取到期需要重新提问的开放问题
    pub fn get_due_questions(&self, limit: usize) -> Result<Vec<(i64, String, i32)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut stmt = conn
            .prepare(
                "SELECT id, question_text, ask_count FROM open_questions
             WHERE status = 'open' AND next_ask_at <= ?1 AND ask_count < 4
             ORDER BY next_ask_at ASC LIMIT ?2",
            )
            .context("查询 due questions 失败")?;
        let rows = stmt.query_map(rusqlite::params![now, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 标记问题为已回答
    pub fn answer_question(&self, question_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE open_questions SET status = 'answered', answered_at = ?1 WHERE id = ?2",
            rusqlite::params![now, question_id],
        )
        .context("标记问题已回答失败")?;
        Ok(())
    }

    /// 增加问题提问次数，更新下次提问时间（间隔递增：3d→7d→14d）
    /// 超过 3 次自动归档
    pub fn bump_question_ask(&self, question_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let ask_count: i32 = conn
            .query_row(
                "SELECT ask_count FROM open_questions WHERE id = ?1",
                rusqlite::params![question_id],
                |row| row.get(0),
            )
            .context("查询 ask_count 失败")?;

        let interval_days = match ask_count {
            1 => 3,
            2 => 7,
            _ => 14,
        };
        let next_ask = (chrono::Local::now() + chrono::Duration::days(interval_days))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        conn.execute(
            "UPDATE open_questions SET ask_count = ask_count + 1, next_ask_at = ?1 WHERE id = ?2",
            rusqlite::params![next_ask, question_id],
        )
        .context("更新问题提问次数失败")?;

        // 超过 3 次归档
        if ask_count + 1 >= 4 {
            conn.execute(
                "UPDATE open_questions SET status = 'archived' WHERE id = ?1",
                rusqlite::params![question_id],
            )
            .context("归档超限问题失败")?;
        }

        Ok(())
    }

    /// 搜索开放问题（用于 Chat 中匹配用户是否在回答某个问题）
    pub fn search_open_questions(&self, query: &str) -> Result<Vec<(i64, String)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{}%", query.trim());
        let mut stmt = conn
            .prepare(
                "SELECT id, question_text FROM open_questions
             WHERE status = 'open' AND question_text LIKE ?1
             LIMIT 5",
            )
            .context("搜索 open_questions 失败")?;
        let rows = stmt.query_map(rusqlite::params![pattern], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
