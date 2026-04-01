use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

mod bridge;
mod context;
mod email;
mod graph;
pub mod memories;
mod messages;
mod migrations;
mod observations;
mod pages;
mod profile;
mod providers;
mod questions;
mod reflective;
mod reports;
mod sessions;
pub mod similarity;
mod suggestions;
pub mod time_normalizer;
mod tasks;
pub mod pipeline;

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

/// 返回今天零点的 ISO 8601 字符串，用于"今天起始时间"过滤
pub(crate) fn today_start() -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    format!("{today}T00:00:00")
}

/// SQLite 存储层，线程安全
pub struct Store {
    pub(crate) conn: Mutex<Connection>,
}

impl Store {
    /// 获取数据库锁，失败时返回标准错误
    fn conn(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
        self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))
    }

    /// 获取数据库锁，失败时打印警告并返回 None（用于返回 bool 的方法）
    fn conn_or_warn(&self) -> Option<std::sync::MutexGuard<'_, rusqlite::Connection>> {
        match self.conn.lock() {
            Ok(g) => Some(g),
            Err(e) => { tracing::warn!("Store mutex poisoned: {e}"); None }
        }
    }

    /// 打开/创建 SQLite 数据库，自动运行 schema 迁移
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("打开 SQLite 数据库失败")?;
        // 设置 WAL 模式、busy_timeout、启用外键约束
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;")
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
