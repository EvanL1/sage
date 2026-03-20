use anyhow::{Context, Result};
use sage_types::ChatMessage;

use super::Store;

impl Store {
    /// 保存聊天消息
    pub fn save_chat_message(&self, role: &str, content: &str, session_id: &str) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO chat_messages (role, content, session_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![role, content, session_id, now],
        )
        .context("保存 chat_message 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 加载某个 session 的消息
    pub fn load_session_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, role, content, session_id, created_at FROM chat_messages WHERE session_id = ?1 ORDER BY id",
        ).context("准备 load_session_messages 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![session_id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                session_id: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取某 session 最近 N 条消息，用于 prompt 构建（窗口化，避免历史过长）
    /// 按 id 倒序取 limit 条，再正序返回，保证时间顺序正确
    pub fn get_recent_messages_for_prompt(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ChatMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, role, content, session_id, created_at
             FROM chat_messages WHERE session_id = ?1
             ORDER BY id DESC LIMIT ?2",
            )
            .context("准备 get_recent_messages_for_prompt 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![session_id, limit as i64], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                session_id: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut messages: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        // 倒序取回后，按 id 正序排列（时间正序）
        messages.reverse();
        Ok(messages)
    }

    /// 加载最近 N 条消息（跨 session，按时间正序返回）
    pub fn load_recent_messages(&self, limit: usize) -> Result<Vec<ChatMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, role, content, session_id, created_at FROM chat_messages ORDER BY id DESC LIMIT ?1",
        ).context("准备 load_recent_messages 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                session_id: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut messages: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        messages.reverse(); // 按时间正序
        Ok(messages)
    }

    /// 列出所有聊天 session 概览（按最新消息时间倒序）
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<sage_types::ChatSession>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT
                session_id,
                MIN(CASE WHEN role = 'user' THEN content END) AS first_user_msg,
                COUNT(*) AS msg_count,
                MIN(created_at) AS created_at,
                MAX(created_at) AS updated_at
             FROM chat_messages
             GROUP BY session_id
             ORDER BY MAX(created_at) DESC
             LIMIT ?1",
            )
            .context("准备 list_sessions 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let preview: Option<String> = row.get(1)?;
            Ok(sage_types::ChatSession {
                session_id: row.get(0)?,
                preview: preview
                    .map(|s| {
                        let truncated: String = s.chars().take(500).collect();
                        if truncated.len() < s.len() {
                            format!("{truncated}…")
                        } else {
                            s
                        }
                    })
                    .unwrap_or_default(),
                message_count: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let deleted = conn
            .execute(
                "DELETE FROM chat_messages WHERE session_id = ?1",
                rusqlite::params![session_id],
            )
            .context("删除 session 失败")?;
        Ok(deleted)
    }

    /// 统计不同的对话 session 数量
    pub fn count_distinct_sessions(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT session_id) FROM chat_messages",
                [],
                |row| row.get(0),
            )
            .context("统计 session 数量失败")?;
        Ok(count as usize)
    }
}
