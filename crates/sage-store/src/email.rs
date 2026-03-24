use anyhow::{Context, Result};
use sage_types::{EmailMessage, MessageSource};

use super::Store;

impl Store {
    // ─── Message Sources CRUD ──────────────────────────────

    /// 保存消息源（INSERT 或按 id UPDATE）
    pub fn save_message_source(&self, source: &MessageSource) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        if source.id > 0 {
            conn.execute(
                "UPDATE message_sources SET label=?1, source_type=?2, config=?3, enabled=?4
                 WHERE id=?5",
                rusqlite::params![
                    source.label,
                    source.source_type,
                    source.config,
                    source.enabled,
                    source.id,
                ],
            )
            .context("更新 message_source 失败")?;
            Ok(source.id)
        } else {
            conn.execute(
                "INSERT INTO message_sources (label, source_type, config, enabled) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![source.label, source.source_type, source.config, source.enabled],
            )
            .context("插入 message_source 失败")?;
            Ok(conn.last_insert_rowid())
        }
    }

    /// 获取所有消息源
    pub fn get_message_sources(&self) -> Result<Vec<MessageSource>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, label, source_type, config, enabled, created_at
             FROM message_sources ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MessageSource {
                id: row.get(0)?,
                label: row.get(1)?,
                source_type: row.get(2)?,
                config: row.get(3)?,
                enabled: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取指定类型的消息源
    pub fn get_message_sources_by_type(&self, source_type: &str) -> Result<Vec<MessageSource>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, label, source_type, config, enabled, created_at
             FROM message_sources WHERE source_type = ?1 AND enabled = 1",
        )?;
        let rows = stmt.query_map(rusqlite::params![source_type], |row| {
            Ok(MessageSource {
                id: row.get(0)?,
                label: row.get(1)?,
                source_type: row.get(2)?,
                config: row.get(3)?,
                enabled: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取单个消息源
    pub fn get_message_source(&self, id: i64) -> Result<Option<MessageSource>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, label, source_type, config, enabled, created_at
             FROM message_sources WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], |row| {
            Ok(MessageSource {
                id: row.get(0)?,
                label: row.get(1)?,
                source_type: row.get(2)?,
                config: row.get(3)?,
                enabled: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// 删除消息源（FK ON DELETE CASCADE 自动清理关联 emails）
    pub fn delete_message_source(&self, id: i64) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let tx = conn.transaction().context("开启事务失败")?;
        tx.execute("DELETE FROM emails WHERE source_id = ?1", rusqlite::params![id])
            .context("删除关联 emails 失败")?;
        tx.execute("DELETE FROM message_sources WHERE id = ?1", rusqlite::params![id])
            .context("删除 message_source 失败")?;
        tx.commit().context("提交事务失败")
    }

    // ─── Emails Cache CRUD ──────────────────────────────

    /// 缓存邮件（INSERT OR IGNORE 防重复）
    pub fn save_email(&self, msg: &EmailMessage) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO emails
             (source_id, uid, folder, from_addr, to_addr, subject, body_text, body_html, is_read, date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                msg.source_id,
                msg.uid,
                msg.folder,
                msg.from_addr,
                msg.to_addr,
                msg.subject,
                msg.body_text,
                msg.body_html,
                msg.is_read as i32,
                msg.date,
            ],
        )
        .context("保存 email 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 批量保存邮件（单次锁 + RAII 事务）
    pub fn save_emails(&self, msgs: &[EmailMessage]) -> Result<usize> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let tx = conn.transaction().context("开启事务失败")?;
        let mut saved = 0usize;
        for msg in msgs {
            let n = tx.execute(
                "INSERT OR IGNORE INTO emails
                 (source_id, uid, folder, from_addr, to_addr, subject, body_text, body_html, is_read, date)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    msg.source_id, msg.uid, msg.folder, msg.from_addr, msg.to_addr,
                    msg.subject, msg.body_text, msg.body_html, msg.is_read as i32, msg.date,
                ],
            )?;
            saved += n;
        }
        tx.commit().context("提交事务失败")?;
        Ok(saved)
    }

    /// 获取邮件列表
    pub fn get_emails(
        &self,
        source_id: i64,
        folder: &str,
        limit: usize,
    ) -> Result<Vec<EmailMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, uid, folder, from_addr, to_addr, subject,
                    body_text, body_html, is_read, date, fetched_at
             FROM emails WHERE source_id = ?1 AND folder = ?2 AND dismissed = 0
             ORDER BY date DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![source_id, folder, limit as i64],
            Self::row_to_email,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取单封邮件
    pub fn get_email(&self, id: i64) -> Result<Option<EmailMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, uid, folder, from_addr, to_addr, subject,
                    body_text, body_html, is_read, date, fetched_at
             FROM emails WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], Self::row_to_email)?;
        Ok(rows.next().transpose()?)
    }

    /// 根据 source_id + uid + folder 查找邮件
    pub fn get_email_by_uid(&self, source_id: i64, uid: &str, folder: &str) -> Result<Option<EmailMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, uid, folder, from_addr, to_addr, subject,
                    body_text, body_html, is_read, date, fetched_at
             FROM emails WHERE source_id = ?1 AND uid = ?2 AND folder = ?3",
        )?;
        let mut rows = stmt.query_map(
            rusqlite::params![source_id, uid, folder],
            Self::row_to_email,
        )?;
        Ok(rows.next().transpose()?)
    }

    /// 软删除邮件（标记 dismissed，不真删，防止重复拉取）
    pub fn dismiss_email(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE emails SET dismissed = 1 WHERE id = ?1",
            rusqlite::params![id],
        )
        .context("dismiss email 失败")?;
        Ok(())
    }

    /// 标记邮件已读
    pub fn mark_email_read(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE emails SET is_read = 1 WHERE id = ?1",
            rusqlite::params![id],
        )
        .context("标记邮件已读失败")?;
        Ok(())
    }

    /// 搜索邮件
    pub fn search_emails(&self, query: &str, limit: usize) -> Result<Vec<EmailMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let escaped = query.replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let mut stmt = conn.prepare(
            "SELECT id, source_id, uid, folder, from_addr, to_addr, subject,
                    body_text, body_html, is_read, date, fetched_at
             FROM emails
             WHERE dismissed = 0 AND (subject LIKE ?1 ESCAPE '\\' OR from_addr LIKE ?1 ESCAPE '\\' OR body_text LIKE ?1 ESCAPE '\\')
             ORDER BY date DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![pattern, limit as i64],
            Self::row_to_email,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 未读邮件数
    pub fn count_unread_emails(&self, source_id: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM emails WHERE source_id = ?1 AND is_read = 0 AND dismissed = 0",
            rusqlite::params![source_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    fn row_to_email(row: &rusqlite::Row) -> rusqlite::Result<EmailMessage> {
        Ok(EmailMessage {
            id: row.get(0)?,
            source_id: row.get(1)?,
            uid: row.get(2)?,
            folder: row.get(3)?,
            from_addr: row.get(4)?,
            to_addr: row.get(5)?,
            subject: row.get(6)?,
            body_text: row.get(7)?,
            body_html: row.get(8)?,
            is_read: row.get::<_, i32>(9)? != 0,
            date: row.get(10)?,
            fetched_at: row.get(11)?,
        })
    }
}
