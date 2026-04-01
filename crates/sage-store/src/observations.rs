use anyhow::{Context, Result};

use super::{ObservationRow, Store};

impl Store {
    /// 记录观察
    /// 检查是否已存在同标题的 feed observation
    pub fn has_feed_observation(&self, title: &str) -> bool {
        let Some(conn) = self.conn_or_warn() else { return false; };
        conn.query_row(
            "SELECT 1 FROM observations WHERE category = 'feed' AND observation = ?1 LIMIT 1",
            rusqlite::params![title],
            |_| Ok(()),
        ).is_ok()
    }

    pub fn record_observation(
        &self,
        category: &str,
        observation: &str,
        raw_data: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO observations (category, observation, raw_data, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![category, observation, raw_data, now],
        ).context("记录 observation 失败")?;
        Ok(())
    }

    /// 读取未处理的 observations（学习教练用），返回带 id 的完整行
    pub fn load_unprocessed_observations(&self, limit: usize) -> Result<Vec<ObservationRow>> {
        let conn = self.conn()?;
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
        let conn = self.conn()?;
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
        let conn = self.conn()?;
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
        let conn = self.conn()?;
        // UNION: 最新 limit 条 + 所有有 action 的条目（归档/已学习），避免旧归档被挤出
        let mut stmt = conn
            .prepare(
                "SELECT id, category, observation, raw_data, created_at FROM (
                    SELECT id, category, observation, raw_data, created_at
                    FROM observations WHERE category = 'feed' AND id IN (
                        SELECT id FROM observations WHERE category = 'feed' ORDER BY created_at DESC LIMIT ?1
                    )
                  UNION
                    SELECT o.id, o.category, o.observation, o.raw_data, o.created_at
                    FROM observations o INNER JOIN feed_actions fa ON o.id = fa.observation_id
                    WHERE o.category = 'feed'
                 ) ORDER BY created_at DESC",
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

    /// 保存 Feed 每日简报到缓存
    pub fn save_feed_digest(&self, date: &str, content: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO feed_digests (date, content) VALUES (?1, ?2)
             ON CONFLICT(date) DO UPDATE SET content = excluded.content, created_at = datetime('now')",
            rusqlite::params![date, content],
        )
        .context("保存 feed digest 失败")?;
        Ok(())
    }

    /// 读取指定日期的 Feed 简报缓存
    pub fn get_feed_digest_for_date(&self, date: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let result = conn.query_row(
            "SELECT content FROM feed_digests WHERE date = ?1",
            rusqlite::params![date],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(content) => Ok(Some(content)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 归档 feed 条目
    pub fn archive_feed_item(&self, observation_id: i64, category: Option<&str>) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO feed_actions (observation_id, action, category)
             VALUES (?1, 'archived', ?2)
             ON CONFLICT(observation_id) DO UPDATE SET action = 'archived', category = COALESCE(excluded.category, feed_actions.category)",
            rusqlite::params![observation_id, category],
        ).context("归档 feed 条目失败")?;
        Ok(())
    }

    /// 取消归档
    pub fn unarchive_feed_item(&self, observation_id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM feed_actions WHERE observation_id = ?1",
            rusqlite::params![observation_id],
        ).context("取消归档失败")?;
        Ok(())
    }

    /// 标记 feed 条目为已学习
    pub fn mark_feed_learned(&self, observation_id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO feed_actions (observation_id, action)
             VALUES (?1, 'learned')
             ON CONFLICT(observation_id) DO UPDATE SET action = 'learned'",
            rusqlite::params![observation_id],
        ).context("标记已学习失败")?;
        Ok(())
    }

    /// 标记 feed 条目为学习中
    pub fn mark_feed_learning(&self, observation_id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO feed_actions (observation_id, action)
             VALUES (?1, 'learning')
             ON CONFLICT(observation_id) DO UPDATE SET action = 'learning'",
            rusqlite::params![observation_id],
        ).context("标记学习中失败")?;
        Ok(())
    }

    /// 获取所有 feed actions（归档/学习状态）
    pub fn get_feed_actions(&self) -> Result<std::collections::HashMap<i64, (String, Option<String>)>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT observation_id, action, category FROM feed_actions"
        ).context("查询 feed_actions 失败")?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
        })?;
        for row in rows {
            let (id, action, category) = row?;
            map.insert(id, (action, category));
        }
        Ok(map)
    }

    /// 获取已归档的 observation IDs（用于 digest 排除）
    pub fn get_archived_feed_ids(&self) -> Result<std::collections::HashSet<i64>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT observation_id FROM feed_actions WHERE action IN ('archived', 'learned')"
        )?;
        let ids = stmt.query_map([], |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    /// 更新 feed 条目的分类
    pub fn set_feed_category(&self, observation_id: i64, category: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO feed_actions (observation_id, action, category)
             VALUES (?1, 'archived', ?2)
             ON CONFLICT(observation_id) DO UPDATE SET category = excluded.category",
            rusqlite::params![observation_id, category],
        ).context("设置分类失败")?;
        Ok(())
    }

    /// 获取某个日期之后的 observations 数量
    pub fn count_observations_since(&self, since: &str) -> Result<usize> {
        let conn = self.conn()?;
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
