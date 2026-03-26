//! Pipeline 自我进化存储：运行日志 + 运行时覆盖

use anyhow::Result;

use super::Store;

/// 管线 stage 执行结果
#[derive(Debug, Clone)]
pub struct PipelineRun {
    pub stage: String,
    pub pipeline: String,
    pub outcome: String,   // "ok", "empty", "error"
    pub elapsed_ms: i64,
    pub created_at: String,
}

/// 运行时覆盖条目
#[derive(Debug, Clone)]
pub struct PipelineOverride {
    pub stage: String,
    pub key: String,
    pub value: String,
    pub reason: String,
}

impl Store {
    /// 记录一次 stage 执行
    pub fn log_pipeline_run(
        &self,
        stage: &str,
        pipeline: &str,
        outcome: &str,
        elapsed_ms: i64,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT INTO pipeline_runs (stage, pipeline, outcome, elapsed_ms) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![stage, pipeline, outcome, elapsed_ms],
        )?;
        Ok(())
    }

    /// 获取某个 stage 最近 N 次执行记录
    pub fn get_pipeline_runs(&self, stage: &str, limit: usize) -> Result<Vec<PipelineRun>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT stage, pipeline, outcome, elapsed_ms, created_at
             FROM pipeline_runs WHERE stage = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![stage, limit], |row| {
                Ok(PipelineRun {
                    stage: row.get(0)?,
                    pipeline: row.get(1)?,
                    outcome: row.get(2)?,
                    elapsed_ms: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// 获取所有 stage 最近 N 天的执行摘要
    pub fn get_pipeline_summary(&self, days: u32) -> Result<Vec<(String, usize, usize, usize)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let window = format!("-{days} days");
        let mut stmt = conn.prepare(
            "SELECT stage,
                    SUM(CASE WHEN outcome = 'ok' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN outcome = 'empty' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN outcome = 'error' THEN 1 ELSE 0 END)
             FROM pipeline_runs
             WHERE created_at > datetime('now', ?1)
             GROUP BY stage",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![window], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, i64>(2)? as usize,
                    row.get::<_, i64>(3)? as usize,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// 写入/更新运行时覆盖
    pub fn set_pipeline_override(
        &self,
        stage: &str,
        key: &str,
        value: &str,
        reason: &str,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT INTO pipeline_overrides (stage, key, value, reason)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(stage, key) DO UPDATE SET value = ?3, reason = ?4",
            rusqlite::params![stage, key, value, reason],
        )?;
        Ok(())
    }

    /// 读取某 stage 的所有运行时覆盖
    pub fn get_pipeline_overrides(&self, stage: &str) -> Result<Vec<PipelineOverride>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT stage, key, value, COALESCE(reason, '') FROM pipeline_overrides WHERE stage = ?1",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![stage], |row| {
                Ok(PipelineOverride {
                    stage: row.get(0)?,
                    key: row.get(1)?,
                    value: row.get(2)?,
                    reason: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// 读取所有运行时覆盖
    pub fn get_all_pipeline_overrides(&self) -> Result<Vec<PipelineOverride>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT stage, key, value, COALESCE(reason, '') FROM pipeline_overrides",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PipelineOverride {
                    stage: row.get(0)?,
                    key: row.get(1)?,
                    value: row.get(2)?,
                    reason: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// 删除一条覆盖
    pub fn delete_pipeline_override(&self, stage: &str, key: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM pipeline_overrides WHERE stage = ?1 AND key = ?2",
            rusqlite::params![stage, key],
        )?;
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
    fn log_and_query_pipeline_run() {
        let s = store();
        s.log_pipeline_run("observer", "evening", "ok", 1200).unwrap();
        s.log_pipeline_run("observer", "evening", "empty", 50).unwrap();
        s.log_pipeline_run("observer", "evening", "error", 100).unwrap();
        let runs = s.get_pipeline_runs("observer", 10).unwrap();
        assert_eq!(runs.len(), 3);
        // 验证三种 outcome 都被记录
        let outcomes: Vec<&str> = runs.iter().map(|r| r.outcome.as_str()).collect();
        assert!(outcomes.contains(&"ok"));
        assert!(outcomes.contains(&"empty"));
        assert!(outcomes.contains(&"error"));
    }

    #[test]
    fn pipeline_summary_aggregates() {
        let s = store();
        s.log_pipeline_run("coach", "evening", "ok", 500).unwrap();
        s.log_pipeline_run("coach", "evening", "ok", 600).unwrap();
        s.log_pipeline_run("coach", "evening", "empty", 10).unwrap();
        let summary = s.get_pipeline_summary(30).unwrap();
        let coach = summary.iter().find(|r| r.0 == "coach").unwrap();
        assert_eq!(coach.1, 2); // ok
        assert_eq!(coach.2, 1); // empty
        assert_eq!(coach.3, 0); // error
    }

    #[test]
    fn override_upsert_and_query() {
        let s = store();
        s.set_pipeline_override("evolution", "max_iterations", "30", "too many timeouts").unwrap();
        let overrides = s.get_pipeline_overrides("evolution").unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].value, "30");
        // upsert: 覆盖旧值
        s.set_pipeline_override("evolution", "max_iterations", "20", "adjusted down").unwrap();
        let overrides = s.get_pipeline_overrides("evolution").unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].value, "20");
        assert_eq!(overrides[0].reason, "adjusted down");
    }

    #[test]
    fn override_delete() {
        let s = store();
        s.set_pipeline_override("observer", "enabled", "false", "test").unwrap();
        s.delete_pipeline_override("observer", "enabled").unwrap();
        let overrides = s.get_pipeline_overrides("observer").unwrap();
        assert!(overrides.is_empty());
    }

    #[test]
    fn get_all_overrides() {
        let s = store();
        s.set_pipeline_override("observer", "max_iterations", "5", "").unwrap();
        s.set_pipeline_override("evolution", "max_iterations", "30", "").unwrap();
        let all = s.get_all_pipeline_overrides().unwrap();
        assert_eq!(all.len(), 2);
    }
}
