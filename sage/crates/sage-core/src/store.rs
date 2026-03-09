use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use sage_types::{ChatMessage, FeedbackAction, Memory, ProviderConfig, Suggestion, UserProfile};

/// 未处理的 observation 行（含 id，供学习教练归档用）
#[derive(Debug, Clone)]
pub struct ObservationRow {
    pub id: i64,
    pub category: String,
    pub observation: String,
    pub raw_data: Option<String>,
    pub created_at: String,
}

/// SQLite 存储层，线程安全
pub struct Store {
    conn: Mutex<Connection>,
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

    /// 用 user_version pragma 做增量迁移
    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS profile (
                    id INTEGER PRIMARY KEY,
                    data TEXT NOT NULL,
                    sop_version INTEGER DEFAULT 0,
                    updated_at TEXT
                );
                CREATE TABLE IF NOT EXISTS suggestions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_source TEXT,
                    prompt TEXT,
                    response TEXT,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS feedback (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    suggestion_id INTEGER REFERENCES suggestions(id),
                    action TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS observations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    category TEXT,
                    observation TEXT,
                    raw_data TEXT,
                    created_at TEXT NOT NULL
                );
                PRAGMA user_version = 1;",
            )
            .context("数据库迁移 v1 失败")?;
        }

        if version < 2 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS provider_config (
                    provider_id TEXT PRIMARY KEY,
                    api_key TEXT,
                    model TEXT,
                    base_url TEXT,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    updated_at TEXT
                );
                PRAGMA user_version = 2;",
            )
            .context("数据库迁移 v2 失败")?;
        }

        if version < 3 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS chat_messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS memories (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    category TEXT NOT NULL,
                    content TEXT NOT NULL,
                    source TEXT NOT NULL DEFAULT 'chat',
                    confidence REAL DEFAULT 0.5,
                    created_at TEXT DEFAULT (datetime('now')),
                    updated_at TEXT DEFAULT (datetime('now'))
                );
                PRAGMA user_version = 3;",
            )
            .context("数据库迁移 v3 失败")?;
        }

        if version < 4 {
            conn.execute_batch(
                "ALTER TABLE observations ADD COLUMN processed_at TEXT;
                 PRAGMA user_version = 4;",
            )
            .context("数据库迁移 v4 失败")?;
        }

        if version < 5 {
            conn.execute_batch(
                "ALTER TABLE memories ADD COLUMN embedding BLOB;
                 PRAGMA user_version = 5;",
            )
            .context("数据库迁移 v5（embedding 列）失败")?;
        }

        Ok(())
    }

    // ─── Profile 方法 ──────────────────────────────

    /// 保存用户 profile（upsert id=1）
    pub fn save_profile(&self, profile: &UserProfile) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let data = serde_json::to_string(profile).context("序列化 UserProfile 失败")?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO profile (id, data, sop_version, updated_at) VALUES (1, ?1, ?2, ?3)",
            rusqlite::params![data, profile.sop_version, now],
        ).context("保存 profile 失败")?;
        Ok(())
    }

    /// 读取 id=1 的 profile
    pub fn load_profile(&self) -> Result<Option<UserProfile>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare("SELECT data FROM profile WHERE id = 1")
            .context("准备 load_profile 查询失败")?;
        let mut rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .context("执行 load_profile 查询失败")?;

        match rows.next() {
            Some(Ok(data)) => {
                let profile: UserProfile =
                    serde_json::from_str(&data).context("反序列化 UserProfile 失败")?;
                Ok(Some(profile))
            }
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// 从 profile 表读 sop_version
    pub fn get_sop_version(&self) -> Result<u32> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let version: Option<u32> = conn
            .query_row(
                "SELECT sop_version FROM profile WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(version.unwrap_or(0))
    }

    // ─── Suggestion 历史方法 ──────────────────────────

    /// 检查 12 小时内是否有相同 (event_source, prompt) 的建议
    pub fn has_recent_suggestion(&self, event_source: &str, prompt: &str) -> bool {
        let conn = self.conn.lock().ok();
        let Some(conn) = conn else { return false };
        let threshold = (chrono::Local::now() - chrono::Duration::hours(12)).to_rfc3339();
        conn.query_row(
            "SELECT 1 FROM suggestions WHERE event_source = ?1 AND prompt = ?2 AND created_at > ?3 LIMIT 1",
            rusqlite::params![event_source, prompt, threshold],
            |_| Ok(()),
        ).is_ok()
    }

    /// 插入建议记录，返回自增 id
    pub fn record_suggestion(
        &self,
        event_source: &str,
        prompt: &str,
        response: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;

        // Store 级别去重：同源 + 同 prompt 在 12 小时内不重复创建
        let threshold = (chrono::Local::now() - chrono::Duration::hours(12)).to_rfc3339();
        let existing_id: Option<i64> = conn.query_row(
            "SELECT id FROM suggestions WHERE event_source = ?1 AND prompt = ?2 AND created_at > ?3 ORDER BY id DESC LIMIT 1",
            rusqlite::params![event_source, prompt, threshold],
            |row| row.get(0),
        ).ok();
        if let Some(id) = existing_id {
            return Ok(id);
        }

        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO suggestions (event_source, prompt, response, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![event_source, prompt, response, now],
        ).context("记录 suggestion 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 按时间倒序获取最近的建议
    pub fn get_recent_suggestions(&self, limit: usize) -> Result<Vec<Suggestion>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.event_source, s.prompt, s.response, s.created_at,
                    (SELECT f.action FROM feedback f WHERE f.suggestion_id = s.id ORDER BY f.id DESC LIMIT 1)
             FROM suggestions s ORDER BY s.created_at DESC LIMIT ?1",
        ).context("准备 get_recent_suggestions 查询失败")?;

        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                let id: i64 = row.get(0)?;
                let event_source: String = row.get(1)?;
                let prompt: String = row.get(2)?;
                let response: String = row.get(3)?;
                let created_at: String = row.get(4)?;
                let feedback_json: Option<String> = row.get(5)?;
                Ok((id, event_source, prompt, response, created_at, feedback_json))
            })
            .context("执行 get_recent_suggestions 查询失败")?;

        let mut suggestions = Vec::new();
        for row in rows {
            let (id, event_source, prompt, response, created_at, feedback_json) = row?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&chrono::Local))
                .unwrap_or_else(|_| chrono::Local::now());
            let feedback = feedback_json
                .and_then(|json| serde_json::from_str::<FeedbackAction>(&json).ok());
            suggestions.push(Suggestion {
                id,
                event_source,
                prompt,
                response,
                timestamp,
                feedback,
            });
        }
        Ok(suggestions)
    }

    // ─── Feedback 方法 ──────────────────────────────

    /// 记录反馈
    pub fn record_feedback(
        &self,
        suggestion_id: i64,
        action: &FeedbackAction,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let action_json = serde_json::to_string(action).context("序列化 FeedbackAction 失败")?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO feedback (suggestion_id, action, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![suggestion_id, action_json, now],
        )
        .context("记录 feedback 失败")?;
        Ok(())
    }

    /// 统计特定 action 类型的反馈数量（按 event_source 关联）
    pub fn count_feedback_by_type(&self, action_type: &str) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{action_type}%");
        let count: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback WHERE action LIKE ?1",
                rusqlite::params![pattern],
                |row| row.get(0),
            )
            .context("统计 feedback 失败")?;
        Ok(count)
    }

    /// 统计某个 event_source 下特定 action 类型的反馈数量
    pub fn count_feedback_by_source_and_type(
        &self,
        event_source: &str,
        action_type: &str,
    ) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{action_type}%");
        let count: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback f
                 JOIN suggestions s ON f.suggestion_id = s.id
                 WHERE s.event_source = ?1 AND f.action LIKE ?2",
                rusqlite::params![event_source, pattern],
                |row| row.get(0),
            )
            .context("统计 feedback by source 失败")?;
        Ok(count)
    }

    // ─── ProviderConfig 方法 ──────────────────────────────

    /// 保存或更新 provider 配置（upsert by provider_id）
    pub fn save_provider_config(&self, config: &ProviderConfig) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let enabled: i32 = if config.enabled { 1 } else { 0 };
        conn.execute(
            "INSERT OR REPLACE INTO provider_config
             (provider_id, api_key, model, base_url, enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                config.provider_id,
                config.api_key,
                config.model,
                config.base_url,
                enabled,
                now,
            ],
        )
        .context("保存 provider_config 失败")?;
        Ok(())
    }

    /// 加载所有 provider 配置
    pub fn load_provider_configs(&self) -> Result<Vec<ProviderConfig>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare("SELECT provider_id, api_key, model, base_url, enabled FROM provider_config")
            .context("准备 load_provider_configs 查询失败")?;
        let rows = stmt
            .query_map([], |row| {
                let enabled_int: i32 = row.get(4)?;
                Ok(ProviderConfig {
                    provider_id: row.get(0)?,
                    api_key: row.get(1)?,
                    model: row.get(2)?,
                    base_url: row.get(3)?,
                    enabled: enabled_int != 0,
                })
            })
            .context("执行 load_provider_configs 查询失败")?;
        let mut configs = Vec::new();
        for row in rows {
            configs.push(row?);
        }
        Ok(configs)
    }

    /// 删除指定 provider 配置
    pub fn delete_provider_config(&self, id: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM provider_config WHERE provider_id = ?1",
            rusqlite::params![id],
        )
        .context("删除 provider_config 失败")?;
        Ok(())
    }

    // ─── Observation 方法 ──────────────────────────────

    /// 记录观察
    pub fn record_observation(
        &self,
        category: &str,
        observation: &str,
        raw_data: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO observations (category, observation, raw_data, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![category, observation, raw_data, now],
        ).context("记录 observation 失败")?;
        Ok(())
    }

    /// 读取未处理的 observations（学习教练用），返回带 id 的完整行
    pub fn load_unprocessed_observations(&self, limit: usize) -> Result<Vec<ObservationRow>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "UPDATE observations SET processed_at = ?1 WHERE id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql).context("准备 mark_observations_processed 失败")?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];
        for id in ids {
            params.push(Box::new(*id));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        stmt.execute(param_refs.as_slice())
            .context("标记 observations 已处理失败")?;
        Ok(())
    }

    /// 读取最近 N 条 observations，返回 (category, observation) 对
    pub fn load_recent_observations(&self, limit: usize) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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

    /// 读取最近 N 条 suggestions 及其 feedback，返回 (event_source, response, feedback_action) 三元组
    pub fn get_suggestions_with_feedback(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT s.event_source, s.response, f.action
                 FROM suggestions s
                 LEFT JOIN feedback f ON f.suggestion_id = s.id
                   AND f.id = (SELECT MAX(f2.id) FROM feedback f2 WHERE f2.suggestion_id = s.id)
                 ORDER BY s.created_at DESC LIMIT ?1",
            )
            .context("准备 get_suggestions_with_feedback 查询失败")?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                let event_source: String = row.get(0)?;
                let response: String = row.get(1)?;
                let action: Option<String> = row.get(2)?;
                Ok((event_source, response, action))
            })
            .context("执行 get_suggestions_with_feedback 查询失败")?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // ─── ChatMessage 方法 ──────────────────────────────

    /// 保存聊天消息
    pub fn save_chat_message(&self, role: &str, content: &str, session_id: &str) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT INTO chat_messages (role, content, session_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![role, content, session_id],
        )
        .context("保存 chat_message 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 加载某个 session 的消息
    pub fn load_session_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, role, content, session_id, created_at
             FROM chat_messages WHERE session_id = ?1
             ORDER BY id DESC LIMIT ?2",
        ).context("准备 get_recent_messages_for_prompt 查询失败")?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
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
        ).context("准备 list_sessions 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let preview: Option<String> = row.get(1)?;
            Ok(sage_types::ChatSession {
                session_id: row.get(0)?,
                preview: preview
                    .map(|s| {
                        let truncated: String = s.chars().take(60).collect();
                        if truncated.len() < s.len() { format!("{truncated}…") } else { s }
                    })
                    .unwrap_or_default(),
                message_count: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ─── 统一记忆上下文方法（替代 memory.rs Markdown 文件） ──────────────────────────────

    /// 从 memories 表构建 LLM 上下文字符串，按 category 分组，截断到 max_bytes（UTF-8 安全）
    pub fn get_memory_context(&self, max_bytes: usize) -> Result<String> {
        let memories = self.load_memories()?;
        if memories.is_empty() {
            return Ok(String::new());
        }

        // 按 category 分组，定义显示顺序
        let categories = ["core", "pattern", "decision", "coach_insight"];
        let mut sections = Vec::new();

        for cat in &categories {
            let items: Vec<&sage_types::Memory> =
                memories.iter().filter(|m| m.category == *cat).collect();
            if !items.is_empty() {
                let header = match *cat {
                    "core" => "## 核心认知",
                    "pattern" => "## 行为模式",
                    "decision" => "## 近期决策",
                    "coach_insight" => "## 教练洞察",
                    _ => "## 其他",
                };
                let lines: Vec<String> = items
                    .iter()
                    .map(|m| format!("- {}", m.content))
                    .collect();
                sections.push(format!("{}\n{}", header, lines.join("\n")));
            }
        }

        // 处理未列举的 category
        let known: std::collections::HashSet<&str> =
            categories.iter().copied().collect();
        let mut extra_cats: Vec<&str> = memories
            .iter()
            .map(|m| m.category.as_str())
            .filter(|c| !known.contains(c))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        extra_cats.sort();
        for cat in extra_cats {
            let items: Vec<&sage_types::Memory> =
                memories.iter().filter(|m| m.category == cat).collect();
            let lines: Vec<String> = items
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect();
            sections.push(format!("## {}\n{}", cat, lines.join("\n")));
        }

        let full = sections.join("\n\n");
        // UTF-8 安全截断
        if full.len() <= max_bytes {
            Ok(full)
        } else {
            Ok(utf8_safe_truncate(&full, max_bytes).to_string())
        }
    }

    /// 保存行为模式记忆（category="pattern"），返回新记录 id
    pub fn append_pattern(&self, category: &str, observation: &str) -> Result<i64> {
        let content = format!("[{category}] {observation}");
        self.save_memory("pattern", &content, "router", 0.6)
    }

    /// 保存决策记忆（category="decision"），返回新记录 id
    pub fn append_decision(&self, context: &str, decision: &str) -> Result<i64> {
        let content = format!("**Context**: {context}\n**Decision**: {decision}");
        self.save_memory("decision", &content, "router", 0.7)
    }

    /// 保存教练洞察（category="coach_insight"），返回新记录 id
    pub fn save_coach_insight(&self, insight: &str) -> Result<i64> {
        self.save_memory("coach_insight", insight, "coach", 0.8)
    }

    // ─── Memory 方法 ──────────────────────────────

    /// 保存记忆
    pub fn save_memory(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT INTO memories (category, content, source, confidence) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![category, content, source, confidence],
        )
        .context("保存 memory 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 更新记忆的内容和置信度
    pub fn update_memory(&self, id: i64, content: &str, confidence: f64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE memories SET content = ?1, confidence = ?2, updated_at = datetime('now') WHERE id = ?3",
            rusqlite::params![content, confidence, id],
        )
        .context("更新 memory 失败")?;
        Ok(())
    }

    /// 加载所有记忆（按置信度和更新时间排序）
    pub fn load_memories(&self) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, created_at, updated_at FROM memories ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 load_memories 查询失败")?;
        let rows = stmt.query_map([], |row| {
            Ok(Memory {
                id: row.get(0)?,
                category: row.get(1)?,
                content: row.get(2)?,
                source: row.get(3)?,
                confidence: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 统计不同的对话 session 数量
    pub fn count_distinct_sessions(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT session_id) FROM chat_messages",
            [],
            |row| row.get(0),
        ).context("统计 session 数量失败")?;
        Ok(count as usize)
    }

    /// FTS5 关键词搜索记忆，按 BM25 相关度 + 置信度排序
    pub fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.load_memories();
        }
        let pattern = format!("%{trimmed}%");
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, created_at, updated_at
             FROM memories
             WHERE content LIKE ?1 OR category LIKE ?1
             ORDER BY confidence DESC
             LIMIT ?2",
        ).context("准备 search_memories 查询失败")?;

        let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                category: row.get(1)?,
                content: row.get(2)?,
                source: row.get(3)?,
                confidence: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        }).context("执行 search_memories 查询失败")?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 删除记忆
    pub fn delete_memory(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
            .context("删除 memory 失败")?;
        Ok(())
    }
}

/// UTF-8 安全截断：在 max_bytes 处找最近的字符边界
fn utf8_safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // 从 max_bytes 开始向前找有效的字符边界
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_types::*;

    fn make_test_profile() -> UserProfile {
        UserProfile {
            identity: UserIdentity {
                name: "Evan".into(),
                role: "Team Lead".into(),
                reporting_line: vec!["Evan".into(), "Shawn".into()],
                primary_language: "zh".into(),
                secondary_language: "en".into(),
            },
            sop_version: 1,
            negative_rules: vec!["不要发重复邮件".into()],
            ..Default::default()
        }
    }

    #[test]
    fn test_open_in_memory() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.load_profile().unwrap().is_none());
    }

    #[test]
    fn test_save_and_load_profile() {
        let store = Store::open_in_memory().unwrap();
        let profile = make_test_profile();
        store.save_profile(&profile).unwrap();

        let loaded = store.load_profile().unwrap().unwrap();
        assert_eq!(loaded.identity.name, "Evan");
        assert_eq!(loaded.sop_version, 1);
        assert_eq!(loaded.negative_rules, vec!["不要发重复邮件"]);
    }

    #[test]
    fn test_profile_upsert() {
        let store = Store::open_in_memory().unwrap();
        let mut profile = make_test_profile();
        store.save_profile(&profile).unwrap();

        profile.sop_version = 2;
        profile.negative_rules.push("不要在晚上发通知".into());
        store.save_profile(&profile).unwrap();

        let loaded = store.load_profile().unwrap().unwrap();
        assert_eq!(loaded.sop_version, 2);
        assert_eq!(loaded.negative_rules.len(), 2);
    }

    #[test]
    fn test_get_sop_version() {
        let store = Store::open_in_memory().unwrap();
        // 无 profile 时返回 0
        assert_eq!(store.get_sop_version().unwrap(), 0);

        let profile = make_test_profile();
        store.save_profile(&profile).unwrap();
        assert_eq!(store.get_sop_version().unwrap(), 1);
    }

    #[test]
    fn test_record_and_get_suggestions() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store
            .record_suggestion("email", "处理这封邮件", "建议回复确认")
            .unwrap();
        let id2 = store
            .record_suggestion("calendar", "明天的会议", "准备议题")
            .unwrap();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);

        let suggestions = store.get_recent_suggestions(10).unwrap();
        assert_eq!(suggestions.len(), 2);
        // 按时间倒序，最新的在前
        assert_eq!(suggestions[0].event_source, "calendar");
        assert_eq!(suggestions[1].event_source, "email");
    }

    #[test]
    fn test_record_feedback() {
        let store = Store::open_in_memory().unwrap();
        let sid = store
            .record_suggestion("email", "test", "response")
            .unwrap();

        store
            .record_feedback(sid, &FeedbackAction::Useful)
            .unwrap();
        store
            .record_feedback(sid, &FeedbackAction::NotUseful)
            .unwrap();
        store
            .record_feedback(
                sid,
                &FeedbackAction::NeverDoThis("不要总结邮件".into()),
            )
            .unwrap();
        store
            .record_feedback(
                sid,
                &FeedbackAction::Correction("应该直接转发".into()),
            )
            .unwrap();

        let count = store.count_feedback_by_type("NotUseful").unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_count_feedback_by_source_and_type() {
        let store = Store::open_in_memory().unwrap();
        let s1 = store
            .record_suggestion("email", "p1", "r1")
            .unwrap();
        let s2 = store
            .record_suggestion("email", "p2", "r2")
            .unwrap();
        let s3 = store
            .record_suggestion("calendar", "p3", "r3")
            .unwrap();

        store
            .record_feedback(s1, &FeedbackAction::NotUseful)
            .unwrap();
        store
            .record_feedback(s2, &FeedbackAction::NotUseful)
            .unwrap();
        store
            .record_feedback(s3, &FeedbackAction::NotUseful)
            .unwrap();

        let email_count = store
            .count_feedback_by_source_and_type("email", "NotUseful")
            .unwrap();
        assert_eq!(email_count, 2);

        let cal_count = store
            .count_feedback_by_source_and_type("calendar", "NotUseful")
            .unwrap();
        assert_eq!(cal_count, 1);
    }

    #[test]
    fn test_suggestion_with_feedback() {
        let store = Store::open_in_memory().unwrap();
        let sid = store
            .record_suggestion("email", "test prompt", "test response")
            .unwrap();
        store
            .record_feedback(sid, &FeedbackAction::Useful)
            .unwrap();

        let suggestions = store.get_recent_suggestions(10).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert!(matches!(
            suggestions[0].feedback,
            Some(FeedbackAction::Useful)
        ));
    }

    #[test]
    fn test_record_observation() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_observation("pattern", "每天下午3点查邮件", Some("{\"count\": 5}"))
            .unwrap();
        store
            .record_observation("habit", "偏好直接沟通", None)
            .unwrap();
        // 验证不报错即可，观察记录主要是写入
    }

    #[test]
    fn test_load_recent_observations() {
        let store = Store::open_in_memory().unwrap();

        // 空时返回空 vec
        let empty = store.load_recent_observations(10).unwrap();
        assert!(empty.is_empty());

        store
            .record_observation("pattern", "每天下午3点查邮件", None)
            .unwrap();
        store
            .record_observation("habit", "偏好直接沟通", None)
            .unwrap();
        store
            .record_observation("pattern", "喜欢用类比解释概念", None)
            .unwrap();

        let all = store.load_recent_observations(10).unwrap();
        assert_eq!(all.len(), 3);
        // 最新的在前
        assert_eq!(all[0].0, "pattern");
        assert_eq!(all[0].1, "喜欢用类比解释概念");

        // limit 限制
        let limited = store.load_recent_observations(2).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_get_suggestions_with_feedback() {
        let store = Store::open_in_memory().unwrap();

        // 空时返回空 vec
        let empty = store.get_suggestions_with_feedback(10).unwrap();
        assert!(empty.is_empty());

        let s1 = store
            .record_suggestion("email", "prompt1", "response1")
            .unwrap();
        let _s2 = store
            .record_suggestion("calendar", "prompt2", "response2")
            .unwrap();

        // s1 有 feedback，s2 没有
        store
            .record_feedback(s1, &FeedbackAction::Useful)
            .unwrap();

        let results = store.get_suggestions_with_feedback(10).unwrap();
        assert_eq!(results.len(), 2);
        // 最新的在前 (s2)
        assert_eq!(results[0].0, "calendar");
        assert_eq!(results[0].1, "response2");
        assert!(results[0].2.is_none());
        // s1 有 feedback
        assert_eq!(results[1].0, "email");
        assert!(results[1].2.is_some());
    }

    // ─── ProviderConfig 测试 ──────────────────────────

    #[test]
    fn test_save_and_load_provider_config() {
        let store = Store::open_in_memory().unwrap();
        let config = ProviderConfig {
            provider_id: "anthropic-api".into(),
            api_key: Some("sk-test-123".into()),
            model: Some("claude-sonnet-4-20250514".into()),
            base_url: None,
            enabled: true,
        };
        store.save_provider_config(&config).unwrap();

        let configs = store.load_provider_configs().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].provider_id, "anthropic-api");
        assert_eq!(configs[0].api_key, Some("sk-test-123".into()));
        assert!(configs[0].enabled);
    }

    #[test]
    fn test_provider_config_upsert() {
        let store = Store::open_in_memory().unwrap();
        let mut config = ProviderConfig {
            provider_id: "openai-api".into(),
            api_key: Some("sk-old".into()),
            model: None,
            base_url: None,
            enabled: true,
        };
        store.save_provider_config(&config).unwrap();

        config.api_key = Some("sk-new".into());
        config.model = Some("gpt-4o".into());
        store.save_provider_config(&config).unwrap();

        let configs = store.load_provider_configs().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].api_key, Some("sk-new".into()));
        assert_eq!(configs[0].model, Some("gpt-4o".into()));
    }

    #[test]
    fn test_delete_provider_config() {
        let store = Store::open_in_memory().unwrap();
        let config = ProviderConfig {
            provider_id: "deepseek-api".into(),
            api_key: Some("ds-key".into()),
            model: None,
            base_url: None,
            enabled: true,
        };
        store.save_provider_config(&config).unwrap();
        assert_eq!(store.load_provider_configs().unwrap().len(), 1);

        store.delete_provider_config("deepseek-api").unwrap();
        assert_eq!(store.load_provider_configs().unwrap().len(), 0);
    }

    #[test]
    fn test_load_empty_provider_configs() {
        let store = Store::open_in_memory().unwrap();
        let configs = store.load_provider_configs().unwrap();
        assert!(configs.is_empty());
    }

    // ─── Observation 归档测试 ──────────────────────────

    #[test]
    fn test_load_unprocessed_observations() {
        let store = Store::open_in_memory().unwrap();

        // 空时返回空 vec
        let empty = store.load_unprocessed_observations(10).unwrap();
        assert!(empty.is_empty());

        store.record_observation("normal", "[email] 新邮件", None).unwrap();
        store.record_observation("urgent", "紧急会议", Some("raw")).unwrap();
        store.record_observation("scheduled", "Morning Brief", None).unwrap();

        let all = store.load_unprocessed_observations(10).unwrap();
        assert_eq!(all.len(), 3);
        // ASC 排序，最早的在前
        assert_eq!(all[0].category, "normal");
        assert_eq!(all[2].category, "scheduled");
        assert!(all[0].id > 0);
    }

    #[test]
    fn test_mark_observations_processed() {
        let store = Store::open_in_memory().unwrap();
        store.record_observation("a", "obs1", None).unwrap();
        store.record_observation("b", "obs2", None).unwrap();
        store.record_observation("c", "obs3", None).unwrap();

        let all = store.load_unprocessed_observations(10).unwrap();
        assert_eq!(all.len(), 3);

        // 归档前两条
        store.mark_observations_processed(&[all[0].id, all[1].id]).unwrap();

        // 只剩 1 条未处理
        let remaining = store.load_unprocessed_observations(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].observation, "obs3");
    }

    #[test]
    fn test_mark_empty_ids() {
        let store = Store::open_in_memory().unwrap();
        // 空 ids 不报错
        store.mark_observations_processed(&[]).unwrap();
    }

    // ─── FTS5 记忆搜索测试 ──────────────────────────

    #[test]
    fn test_search_memories() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("values", "重视团队成长胜过个人表现", "chat", 0.8).unwrap();
        store.save_memory("thinking", "用系统思考分析问题", "chat", 0.7).unwrap();
        store.save_memory("behavior", "每天下午三点查邮件", "chat", 0.6).unwrap();

        let results = store.search_memories("团队", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("团队"));

        let results = store.search_memories("邮件", 5).unwrap();
        assert_eq!(results.len(), 1);

        // 按 category 也能搜
        let results = store.search_memories("values", 5).unwrap();
        assert_eq!(results.len(), 1);

        // 空查询退化为全量加载
        let all = store.search_memories("", 10).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_search_memories_update() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("growth", "想学习 Rust 异步编程", "chat", 0.7).unwrap();

        store.update_memory(id, "已掌握 Rust 异步编程基础", 0.9).unwrap();
        let results = store.search_memories("异步编程", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("已掌握"));
    }

    #[test]
    fn test_search_memories_delete() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("emotion", "会议压力大", "chat", 0.5).unwrap();

        store.delete_memory(id).unwrap();
        let results = store.search_memories("压力", 5).unwrap();
        assert!(results.is_empty());
    }

    // ─── 新增：统一记忆上下文方法测试（TDD RED → GREEN） ──────────────────────────────

    #[test]
    fn test_get_memory_context_empty() {
        // 空数据库时应返回空字符串
        let store = Store::open_in_memory().unwrap();
        let ctx = store.get_memory_context(2000).unwrap();
        assert!(ctx.is_empty(), "空数据库应返回空字符串，但得到: {ctx:?}");
    }

    #[test]
    fn test_get_memory_context_includes_all_categories() {
        let store = Store::open_in_memory().unwrap();

        // 写入各类记忆
        store.save_memory("core", "重视团队成长", "test", 0.9).unwrap();
        store.save_memory("pattern", "每天下午查邮件", "test", 0.7).unwrap();
        store.save_memory("decision", "选择 Rust 作为核心语言", "test", 0.8).unwrap();
        store.save_memory("coach_insight", "Evan 决策偏系统思考", "test", 0.8).unwrap();

        let ctx = store.get_memory_context(10000).unwrap();

        // 验证各分类都包含
        assert!(ctx.contains("## 核心认知"), "应包含核心认知分类");
        assert!(ctx.contains("## 行为模式"), "应包含行为模式分类");
        assert!(ctx.contains("## 近期决策"), "应包含近期决策分类");
        assert!(ctx.contains("## 教练洞察"), "应包含教练洞察分类");

        // 验证内容存在
        assert!(ctx.contains("重视团队成长"));
        assert!(ctx.contains("每天下午查邮件"));
        assert!(ctx.contains("选择 Rust 作为核心语言"));
        assert!(ctx.contains("Evan 决策偏系统思考"));
    }

    #[test]
    fn test_get_memory_context_truncation() {
        let store = Store::open_in_memory().unwrap();

        // 写入较长内容
        let long_content = "A".repeat(500);
        store.save_memory("core", &long_content, "test", 0.9).unwrap();
        store.save_memory("pattern", &long_content, "test", 0.7).unwrap();
        store.save_memory("decision", &long_content, "test", 0.8).unwrap();

        // 截断到 300 字节
        let ctx = store.get_memory_context(300).unwrap();
        assert!(
            ctx.len() <= 300,
            "截断后字节数 {} 应 ≤ 300",
            ctx.len()
        );

        // 验证结果是有效 UTF-8（不应在字符中间截断）
        assert!(std::str::from_utf8(ctx.as_bytes()).is_ok(), "截断结果应为有效 UTF-8");
    }

    #[test]
    fn test_get_memory_context_utf8_safe_truncation() {
        let store = Store::open_in_memory().unwrap();

        // 中文字符每个 3 字节，测试 UTF-8 安全截断
        let chinese_content = "重视团队成长胜过个人表现，这是核心价值观。";
        store.save_memory("core", chinese_content, "test", 0.9).unwrap();

        // 设定一个可能切在中文字符中间的 max_bytes
        let ctx = store.get_memory_context(20).unwrap();
        // 验证是有效 UTF-8
        assert!(
            std::str::from_utf8(ctx.as_bytes()).is_ok(),
            "中文截断后应仍为有效 UTF-8，得到字节数: {}",
            ctx.len()
        );
        assert!(ctx.len() <= 20, "截断后字节数 {} 应 ≤ 20", ctx.len());
    }

    #[test]
    fn test_append_pattern_stores_and_searchable() {
        let store = Store::open_in_memory().unwrap();

        let id = store.append_pattern("behavior", "每天下午三点查邮件").unwrap();
        assert!(id > 0, "append_pattern 应返回正整数 id");

        // 验证可搜索
        let results = store.search_memories("下午三点", 5).unwrap();
        assert_eq!(results.len(), 1, "应找到 1 条 pattern");
        assert_eq!(results[0].category, "pattern");
        assert!(results[0].content.contains("每天下午三点查邮件"));
        assert!(results[0].content.contains("behavior"));
    }

    #[test]
    fn test_append_decision_stores_and_searchable() {
        let store = Store::open_in_memory().unwrap();

        let id = store.append_decision("架构选型", "选择 Rust 实现 VoltageEMS 核心").unwrap();
        assert!(id > 0, "append_decision 应返回正整数 id");

        // 验证可搜索
        let results = store.search_memories("Rust", 5).unwrap();
        assert_eq!(results.len(), 1, "应找到 1 条 decision");
        assert_eq!(results[0].category, "decision");
        assert!(results[0].content.contains("Context"));
        assert!(results[0].content.contains("Decision"));
        assert!(results[0].content.contains("架构选型"));
    }

    #[test]
    fn test_save_coach_insight_stores_and_searchable() {
        let store = Store::open_in_memory().unwrap();

        let id = store.save_coach_insight("Evan 在压力下倾向于系统化思考而非直觉决策").unwrap();
        assert!(id > 0, "save_coach_insight 应返回正整数 id");

        // 验证可搜索
        let results = store.search_memories("系统化思考", 5).unwrap();
        assert_eq!(results.len(), 1, "应找到 1 条 coach_insight");
        assert_eq!(results[0].category, "coach_insight");
        assert_eq!(results[0].source, "coach");
    }

    #[test]
    fn test_append_pattern_appears_in_context() {
        let store = Store::open_in_memory().unwrap();

        store.append_pattern("communication", "偏好直接沟通").unwrap();
        store.append_decision("工具选型", "使用 Claude Code").unwrap();
        store.save_coach_insight("主动学习型用户").unwrap();

        let ctx = store.get_memory_context(10000).unwrap();

        // 三类记忆都应出现在上下文中
        assert!(ctx.contains("偏好直接沟通"), "pattern 应出现在上下文");
        assert!(ctx.contains("工具选型"), "decision 应出现在上下文");
        assert!(ctx.contains("主动学习型用户"), "coach_insight 应出现在上下文");
    }

    // ─── Task 7: 历史窗口化测试 ──────────────────────────

    #[test]
    fn test_get_recent_messages_for_prompt() {
        // 插入 50 条消息，取最近 20 条，验证数量正确
        let store = Store::open_in_memory().unwrap();
        let sid = "test-session-window";

        for i in 0..50 {
            let role = if i % 2 == 0 { "user" } else { "sage" };
            store.save_chat_message(role, &format!("消息 {}", i), sid).unwrap();
        }

        let messages = store.get_recent_messages_for_prompt(sid, 20).unwrap();
        assert_eq!(messages.len(), 20, "应返回 20 条消息");

        // 最后一条应是第 49 条（最新的消息 id=50）
        assert!(
            messages.last().unwrap().content.contains("49"),
            "最后一条应是最新消息（索引49）"
        );
        // 第一条应是第 30 条（20条窗口从索引 30 开始）
        assert!(
            messages.first().unwrap().content.contains("30"),
            "第一条应是窗口开始处的消息（索引30）"
        );
    }

    #[test]
    fn test_recent_messages_preserves_order() {
        // 返回的消息应按时间正序排列（id 递增）
        let store = Store::open_in_memory().unwrap();
        let sid = "test-session-order";

        for i in 0..5 {
            store.save_chat_message("user", &format!("消息 {}", i), sid).unwrap();
        }

        let messages = store.get_recent_messages_for_prompt(sid, 10).unwrap();
        assert_eq!(messages.len(), 5);

        // 验证 id 递增（时间正序）
        let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
        let mut sorted_ids = ids.clone();
        sorted_ids.sort();
        assert_eq!(ids, sorted_ids, "消息应按 id 正序返回（时间正序）");

        // 验证内容顺序
        assert!(messages[0].content.contains("0"), "第一条应是最早的消息");
        assert!(messages[4].content.contains("4"), "最后一条应是最新的消息");
    }

    #[test]
    fn test_recent_messages_limit_less_than_total() {
        // 消息总数少于 limit 时，返回全部
        let store = Store::open_in_memory().unwrap();
        let sid = "test-session-small";

        for i in 0..5 {
            store.save_chat_message("user", &format!("msg {}", i), sid).unwrap();
        }

        let messages = store.get_recent_messages_for_prompt(sid, 20).unwrap();
        assert_eq!(messages.len(), 5, "消息总数少于 limit 时应返回全部");
    }
}
