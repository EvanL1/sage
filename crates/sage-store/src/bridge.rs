use anyhow::{Context, Result};

use super::{BrowserBehaviorRow, Store};

/// 将 rusqlite row 转换为 BrowserBehaviorRow
fn row_to_browser_behavior(row: &rusqlite::Row<'_>) -> rusqlite::Result<BrowserBehaviorRow> {
    Ok(BrowserBehaviorRow {
        id: row.get(0)?,
        source: row.get(1)?,
        event_type: row.get(2)?,
        metadata: row.get(3)?,
        created_at: row.get(4)?,
    })
}

impl Store {
    /// 保存浏览器行为事件
    pub fn save_browser_behavior(
        &self,
        source: &str,
        event_type: &str,
        metadata: &str,
    ) -> Result<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO browser_behaviors (source, event_type, metadata) VALUES (?1, ?2, ?3)",
            rusqlite::params![source, event_type, metadata],
        )
        .context("保存浏览器行为失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取最近 N 条浏览器行为事件
    pub fn get_browser_behaviors(&self, limit: usize) -> Result<Vec<BrowserBehaviorRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, source, event_type, metadata, created_at
             FROM browser_behaviors ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], row_to_browser_behavior)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 获取指定时间之后的浏览器行为（用于报告生成）
    pub fn get_browser_behaviors_since(&self, since: &str) -> Result<Vec<BrowserBehaviorRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, source, event_type, metadata, created_at
             FROM browser_behaviors WHERE created_at >= ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![since], row_to_browser_behavior)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 浏览器行为事件总数
    pub fn count_browser_behaviors(&self) -> Result<usize> {
        let conn = self.conn()?;
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM browser_behaviors", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        Ok(count)
    }
}
