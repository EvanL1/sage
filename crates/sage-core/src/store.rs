use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

mod bridge;
mod context;
mod graph;
mod memories;
mod messages;
mod migrations;
mod observations;
mod profile;
mod providers;
mod questions;
mod reflective;
mod reports;
mod sessions;
mod suggestions;
mod tasks;

#[cfg(test)]
mod tests;

/// 未处理的 observation 行（含 id，供学习教练归档用）
#[derive(Debug, Clone)]
pub struct ObservationRow {
    pub id: i64,
    pub category: String,
    pub observation: String,
    pub raw_data: Option<String>,
    pub created_at: String,
}

/// 浏览器行为记录
#[derive(Debug, Clone)]
pub struct BrowserBehaviorRow {
    pub id: i64,
    pub source: String,
    pub event_type: String,
    pub metadata: Option<String>,
    pub created_at: String,
}

/// 任务智能信号
#[derive(Debug, Clone)]
pub struct TaskSignal {
    pub id: i64,
    pub signal_type: String,
    pub task_id: Option<i64>,
    pub title: String,
    pub evidence: String,
    pub suggested_outcome: Option<String>,
    pub status: String,
    pub created_at: String,
    pub importance: f32,
}

/// 反思信号行（Mirror Layer 检测到的反思/脆弱/矛盾时刻）
#[derive(Debug, Clone)]
pub struct ReflectiveSignalRow {
    pub id: i64,
    pub timestamp: String,
    pub source: String,
    pub signal_type: String,
    pub raw_text: String,
    pub context: Option<String>,
    pub baseline_divergence: f64,
    pub armor_pattern: Option<String>,
    pub intensity: f64,
    pub resolved: bool,
    pub resolution_text: Option<String>,
    pub created_at: String,
}

/// SQLite 存储层，线程安全
pub struct Store {
    pub(crate) conn: Mutex<Connection>,
}

impl Store {
    /// 打开/创建 SQLite 数据库，自动运行 schema 迁移
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("打开 SQLite 数据库失败")?;
        // 设置 WAL 模式和 busy_timeout，支持 daemon 和 desktop 并发读写
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
            .context("设置 SQLite PRAGMA 失败")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    /// 内存数据库，用于测试
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("创建内存数据库失败")?;
        conn.execute_batch("PRAGMA busy_timeout = 5000;")
            .context("设置 SQLite PRAGMA 失败")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }
}
