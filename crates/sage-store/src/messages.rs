use anyhow::{Context, Result};
use sage_types::Message;

use super::Store;

impl Store {
    fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<Message> {
        Ok(Message {
            id: row.get(0)?,
            sender: row.get(1)?,
            channel: row.get(2)?,
            content: row.get(3)?,
            source: row.get(4)?,
            message_type: row.get(5)?,
            timestamp: row.get(6)?,
            created_at: row.get(7)?,
            direction: row.get::<_, String>(8).unwrap_or_else(|_| "received".into()),
            action_state: row.get::<_, String>(9).unwrap_or_else(|_| "pending".into()),
            resolved_at: row.get::<_, Option<String>>(10).unwrap_or(None),
        })
    }

    /// 保存通讯消息
    pub fn save_message(
        &self,
        sender: &str,
        channel: &str,
        content: Option<&str>,
        source: &str,
        message_type: &str,
        timestamp: &str,
    ) -> Result<i64> {
        self.save_message_with_direction(
            sender,
            channel,
            content,
            source,
            message_type,
            timestamp,
            "received",
        )
    }

    pub fn save_message_with_direction(
        &self,
        sender: &str,
        channel: &str,
        content: Option<&str>,
        source: &str,
        message_type: &str,
        timestamp: &str,
        direction: &str,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO messages (sender, channel, content, source, message_type, timestamp, direction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![sender, channel, content, source, message_type, timestamp, direction],
        )
        .context("保存 message 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 按频道查询消息
    pub fn get_messages_by_channel(&self, channel: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at, direction, action_state, resolved_at
             FROM messages WHERE channel = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![channel, limit as i64],
            Self::row_to_message,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按来源查询消息
    pub fn get_messages_by_source(&self, source: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at, direction, action_state, resolved_at
             FROM messages WHERE source = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![source, limit as i64],
            Self::row_to_message,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 搜索消息内容
    pub fn search_messages(&self, query: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{query}%");
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at, direction, action_state, resolved_at
             FROM messages
             WHERE content LIKE ?1 OR sender LIKE ?1 OR channel LIKE ?1
             ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![pattern, limit as i64],
            Self::row_to_message,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 今日消息统计（按来源分组）
    pub fn get_today_message_stats(&self) -> Result<Vec<(String, i64)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) FROM messages
             WHERE created_at >= ?1 GROUP BY source ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![today], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取所有频道列表（含 source 和消息数）
    pub fn get_message_channels(&self) -> Result<Vec<(String, String, i64)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT channel, source, COUNT(*) as cnt FROM messages
             GROUP BY channel, source ORDER BY cnt DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 消息总数
    pub fn count_messages(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// 删除指定消息
    pub fn delete_message(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM messages WHERE id = ?1", rusqlite::params![id])
            .context("删除 message 失败")?;
        Ok(())
    }

    /// 更新消息的 action_state
    pub fn update_message_action_state(&self, id: i64, state: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let resolved_at = if state == "resolved" || state == "expired" {
            Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string())
        } else {
            None
        };
        conn.execute(
            "UPDATE messages SET action_state = ?1, resolved_at = ?2 WHERE id = ?3",
            rusqlite::params![state, resolved_at, id],
        )
        .context("更新 message action_state 失败")?;
        Ok(())
    }

    /// 获取所有超过 N 小时且状态为 pending 的已接收消息
    pub fn get_pending_messages_older_than(&self, hours: i64) -> Result<Vec<Message>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let cutoff = (chrono::Local::now() - chrono::Duration::hours(hours))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at, direction, action_state, resolved_at
             FROM messages
             WHERE direction = 'received' AND action_state = 'pending' AND created_at < ?1
             ORDER BY timestamp DESC LIMIT 50",
        )?;
        let rows = stmt.query_map(rusqlite::params![cutoff], Self::row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取最近 N 小时内已发送的消息（用于回复链检测）
    pub fn get_recent_sent_messages(&self, hours: i64) -> Result<Vec<Message>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let cutoff = (chrono::Local::now() - chrono::Duration::hours(hours))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at, direction, action_state, resolved_at
             FROM messages
             WHERE direction = 'sent' AND created_at > ?1
             ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![cutoff], Self::row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 批量将指定 ID 的消息标记为 resolved
    pub fn resolve_messages(&self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut count = 0;
        for id in ids {
            count += conn.execute(
                "UPDATE messages SET action_state = 'resolved', resolved_at = ?1 WHERE id = ?2 AND action_state = 'pending'",
                rusqlite::params![now, id],
            )?;
        }
        Ok(count)
    }

    /// 统计处于 pending 状态的已接收消息数量
    pub fn count_pending_messages(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE direction = 'received' AND action_state = 'pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}
