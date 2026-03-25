use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sage_types::Report;

use super::Store;

impl Store {
    /// 保存报告（同一天同类型只保留一条，重复生成时更新内容）
    pub fn save_report(&self, report_type: &str, content: &str) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let today = &now[..10]; // "2026-03-25"

        // 查找今天是否已有同类型报告
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM reports WHERE report_type = ?1 AND created_at >= ?2 ORDER BY id DESC LIMIT 1",
                rusqlite::params![report_type, format!("{today}T00:00:00")],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            conn.execute(
                "UPDATE reports SET content = ?1, created_at = ?2 WHERE id = ?3",
                rusqlite::params![content, now, id],
            )
            .context("更新 report 失败")?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO reports (report_type, content, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![report_type, content, now],
            )
            .context("保存 report 失败")?;
            Ok(conn.last_insert_rowid())
        }
    }

    /// 获取指定类型的最新一条报告
    pub fn get_latest_report(&self, report_type: &str) -> Result<Option<Report>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.query_row(
            "SELECT id, report_type, content, created_at FROM reports WHERE report_type = ?1 ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![report_type],
            |row| Ok(Report {
                id: row.get(0)?,
                report_type: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            }),
        )
        .optional()
        .map_err(Into::into)
    }

    /// 获取指定类型的最近 N 条报告（按时间倒序）
    pub fn get_reports(&self, report_type: &str, limit: usize) -> Result<Vec<Report>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, report_type, content, created_at FROM reports WHERE report_type = ?1 ORDER BY created_at DESC LIMIT ?2",
        ).context("准备 get_reports 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![report_type, limit as i64], |row| {
            Ok(Report {
                id: row.get(0)?,
                report_type: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取所有类型的最近 N 条报告（按时间倒序）
    pub fn get_all_reports(&self, limit: usize) -> Result<Vec<Report>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, report_type, content, created_at FROM reports ORDER BY created_at DESC LIMIT ?1",
        ).context("准备 get_all_reports 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(Report {
                id: row.get(0)?,
                report_type: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn save_correction(
        &self,
        report_type: &str,
        wrong_claim: &str,
        correct_fact: &str,
        context_hint: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        conn.execute(
            "UPDATE report_corrections SET superseded_at = datetime('now') WHERE report_type = ?1 AND wrong_claim = ?2 AND superseded_at IS NULL",
            rusqlite::params![report_type, wrong_claim],
        )?;
        conn.execute(
            "INSERT INTO report_corrections (report_type, wrong_claim, correct_fact, context_hint) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![report_type, wrong_claim, correct_fact, context_hint],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_active_corrections(
        &self,
        report_type: &str,
        limit: usize,
    ) -> Result<Vec<sage_types::ReportCorrection>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, report_type, wrong_claim, correct_fact, context_hint, confidence, applied_count, created_at, superseded_at
             FROM report_corrections WHERE report_type = ?1 AND superseded_at IS NULL
             ORDER BY confidence DESC, created_at DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![report_type, limit as i64], |row| {
            Ok(sage_types::ReportCorrection {
                id: row.get(0)?,
                report_type: row.get(1)?,
                wrong_claim: row.get(2)?,
                correct_fact: row.get(3)?,
                context_hint: row.get(4)?,
                confidence: row.get(5)?,
                applied_count: row.get(6)?,
                created_at: row.get(7)?,
                superseded_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn increment_correction_applied(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        conn.execute(
            "UPDATE report_corrections SET applied_count = applied_count + 1, confidence = MIN(1.0, confidence + 0.05) WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub fn get_corrections_for_pattern(
        &self,
        report_type: &str,
    ) -> Result<Vec<sage_types::ReportCorrection>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        let cutoff = (chrono::Local::now() - chrono::Duration::days(30)).to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, report_type, wrong_claim, correct_fact, context_hint, confidence, applied_count, created_at, superseded_at
             FROM report_corrections WHERE report_type = ?1 AND created_at > ?2 AND applied_count >= 1
             ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![report_type, cutoff], |row| {
            Ok(sage_types::ReportCorrection {
                id: row.get(0)?,
                report_type: row.get(1)?,
                wrong_claim: row.get(2)?,
                correct_fact: row.get(3)?,
                context_hint: row.get(4)?,
                confidence: row.get(5)?,
                applied_count: row.get(6)?,
                created_at: row.get(7)?,
                superseded_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_correction(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        conn.execute(
            "DELETE FROM report_corrections WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub fn get_all_corrections(&self) -> Result<Vec<sage_types::ReportCorrection>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, report_type, wrong_claim, correct_fact, context_hint, confidence, applied_count, created_at, superseded_at
             FROM report_corrections WHERE superseded_at IS NULL ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(sage_types::ReportCorrection {
                id: row.get(0)?,
                report_type: row.get(1)?,
                wrong_claim: row.get(2)?,
                correct_fact: row.get(3)?,
                context_hint: row.get(4)?,
                confidence: row.get(5)?,
                applied_count: row.get(6)?,
                created_at: row.get(7)?,
                superseded_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
