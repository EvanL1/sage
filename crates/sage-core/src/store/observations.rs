use anyhow::{Context, Result};

use super::{ObservationRow, Store};

impl Store {
    /// 记录观察
    pub fn record_observation(
        &self,
        category: &str,
        observation: &str,
        raw_data: Option<&str>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO observations (category, observation, raw_data, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![category, observation, raw_data, now],
        ).context("记录 observation 失败")?;
        Ok(())
    }

    /// 读取未处理的 observations（学习教练用），返回带 id 的完整行
    pub fn load_unprocessed_observations(&self, limit: usize) -> Result<Vec<ObservationRow>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, category, observation, raw_data, created_at
                 FROM observations WHERE processed_at IS NULL
                 ORDER BY created_at ASC LIMIT ?1",
            )
            .context("准备 load_unprocessed_observations 查询失败")?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(ObservationRow {
                    id: row.get(0)?,
                    category: row.get(1)?,
                    observation: row.get(2)?,
                    raw_data: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .context("执行 load_unprocessed_observations 查询失败")?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 标记 observations 为已处理（归档）
    pub fn mark_observations_processed(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "UPDATE observations SET processed_at = ?1 WHERE id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn
            .prepare(&sql)
            .context("准备 mark_observations_processed 失败")?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];
        for id in ids {
            params.push(Box::new(*id));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        stmt.execute(param_refs.as_slice())
            .context("标记 observations 已处理失败")?;
        Ok(())
    }

    /// 读取最近 N 条 observations，返回 (category, observation) 对
    pub fn load_recent_observations(&self, limit: usize) -> Result<Vec<(String, String)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT category, observation FROM observations ORDER BY created_at DESC LIMIT ?1",
            )
            .context("准备 load_recent_observations 查询失败")?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                let category: String = row.get(0)?;
                let observation: String = row.get(1)?;
                Ok((category, observation))
            })
            .context("执行 load_recent_observations 查询失败")?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// 读取最近的 feed observations
    pub fn load_feed_observations(&self, limit: usize) -> Result<Vec<ObservationRow>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, category, observation, raw_data, created_at
                 FROM observations WHERE category = 'feed'
                 ORDER BY created_at DESC LIMIT ?1",
            )
            .context("准备 load_feed_observations 查询失败")?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(ObservationRow {
                    id: row.get(0)?,
                    category: row.get(1)?,
                    observation: row.get(2)?,
                    raw_data: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .context("执行 load_feed_observations 查询失败")?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取某个日期之后的 observations 数量
    pub fn count_observations_since(&self, since: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM observations WHERE created_at >= ?1",
                rusqlite::params![since],
                |row| row.get(0),
            )
            .context("统计 observations 数量失败")?;
        Ok(count as usize)
    }
}
