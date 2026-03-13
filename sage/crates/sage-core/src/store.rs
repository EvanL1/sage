use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

use sage_types::{ChatMessage, FeedbackAction, KnowledgeEdge, Memory, MemoryEdge, Message, ProviderConfig, Report, Suggestion, UserProfile};

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

        if version < 6 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS reports (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    report_type TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_reports_type_date ON reports(report_type, created_at DESC);
                PRAGMA user_version = 6;",
            )
            .context("数据库迁移 v6（reports 表）失败")?;
        }

        if version < 7 {
            conn.execute_batch(
                "ALTER TABLE memories ADD COLUMN tier TEXT NOT NULL DEFAULT 'archive';
                 ALTER TABLE memories ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
                 ALTER TABLE memories ADD COLUMN expires_at TEXT;
                 CREATE INDEX IF NOT EXISTS idx_memories_tier_status ON memories(tier, status);
                 PRAGMA user_version = 7;",
            )
            .context("数据库迁移 v7（记忆分层）失败")?;

            // 按 category 初始化 tier
            conn.execute_batch(
                "UPDATE memories SET tier = 'core' WHERE category IN ('identity', 'personality', 'values');
                 UPDATE memories SET tier = 'working' WHERE category IN ('task', 'decision', 'session');
                 UPDATE memories SET tier = 'archive' WHERE tier = 'archive';",
            )
            .context("初始化记忆层级失败")?;
        }

        if version < 8 {
            conn.execute_batch(
                "ALTER TABLE provider_config ADD COLUMN priority INTEGER;
                 PRAGMA user_version = 8;",
            )
            .context("数据库迁移 v8（provider priority）失败")?;
        }

        if version < 9 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS open_questions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    question_text TEXT NOT NULL,
                    source_suggestion_id INTEGER REFERENCES suggestions(id),
                    status TEXT NOT NULL DEFAULT 'open',
                    ask_count INTEGER NOT NULL DEFAULT 1,
                    next_ask_at TEXT,
                    created_at TEXT NOT NULL,
                    answered_at TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_open_questions_status
                    ON open_questions(status, next_ask_at);
                PRAGMA user_version = 9;",
            )
            .context("数据库迁移 v9（open_questions 表）失败")?;
        }

        if version < 10 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS browser_behaviors (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    metadata TEXT,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_browser_behaviors_source
                    ON browser_behaviors(source, created_at DESC);
                PRAGMA user_version = 10;",
            )
            .context("数据库迁移 v10（browser_behaviors 表）失败")?;
        }

        if version < 11 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS memory_edges (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    from_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                    to_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                    relation TEXT NOT NULL,
                    weight REAL NOT NULL DEFAULT 0.5,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_memory_edges_from ON memory_edges(from_id);
                CREATE INDEX IF NOT EXISTS idx_memory_edges_to ON memory_edges(to_id);
                CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_edges_pair
                    ON memory_edges(from_id, to_id, relation);
                PRAGMA user_version = 11;",
            )
            .context("数据库迁移 v11（memory_edges 图谱表）失败")?;
        }

        if version < 12 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS memory_tags (
                    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                    tag TEXT NOT NULL,
                    PRIMARY KEY (memory_id, tag)
                );
                CREATE INDEX IF NOT EXISTS idx_memory_tags_tag ON memory_tags(tag);
                PRAGMA user_version = 12;",
            )
            .context("数据库迁移 v12（memory_tags 标签表）失败")?;
        }

        if version < 13 {
            conn.execute_batch(
                "ALTER TABLE memory_edges ADD COLUMN last_activated_at TEXT DEFAULT NULL;
                 CREATE INDEX IF NOT EXISTS idx_memory_edges_activated ON memory_edges(last_activated_at);
                 PRAGMA user_version = 13;",
            )
            .context("数据库迁移 v13（memory_edges.last_activated_at）失败")?;
        }

        if version < 14 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    sender TEXT NOT NULL,
                    channel TEXT NOT NULL,
                    content TEXT,
                    source TEXT NOT NULL,
                    message_type TEXT NOT NULL DEFAULT 'text',
                    timestamp TEXT NOT NULL,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel, timestamp DESC);
                CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender, timestamp DESC);
                CREATE INDEX IF NOT EXISTS idx_messages_source ON messages(source, created_at DESC);
                PRAGMA user_version = 14;",
            )
            .context("数据库迁移 v14（messages 通讯消息表）失败")?;
        }

        if version < 15 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS knowledge_edges (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    from_type TEXT NOT NULL DEFAULT 'memory',
                    from_id INTEGER NOT NULL,
                    to_type TEXT NOT NULL DEFAULT 'memory',
                    to_id INTEGER NOT NULL,
                    relation TEXT NOT NULL,
                    weight REAL NOT NULL DEFAULT 0.5,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_knowledge_edges_from ON knowledge_edges(from_type, from_id);
                CREATE INDEX IF NOT EXISTS idx_knowledge_edges_to ON knowledge_edges(to_type, to_id);
                CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_edges_pair
                    ON knowledge_edges(from_type, from_id, to_type, to_id, relation);
                INSERT OR IGNORE INTO knowledge_edges (from_type, from_id, to_type, to_id, relation, weight, created_at)
                    SELECT 'memory', from_id, 'memory', to_id, relation, weight, created_at
                    FROM memory_edges;
                PRAGMA user_version = 15;",
            )
            .context("数据库迁移 v15（knowledge_edges 通用知识图谱表）失败")?;
        }

        // ── v16: memories 三层可见性 ──
        if version < 16 {
            conn.execute_batch(
                "ALTER TABLE memories ADD COLUMN visibility TEXT NOT NULL DEFAULT 'public';
                CREATE INDEX IF NOT EXISTS idx_memories_visibility ON memories(visibility);
                -- 回填：chat/emotion → private，coach/mirror/questioner/observer → subconscious
                UPDATE memories SET visibility = 'private'
                    WHERE source IN ('chat', 'user_input')
                       OR category IN ('emotion', 'task', 'reminder');
                UPDATE memories SET visibility = 'subconscious'
                    WHERE source IN ('coach', 'mirror', 'questioner', 'observer')
                       OR category IN ('coach_insight', 'observer_note', 'mirror_reflection', 'questioner_probe');
                PRAGMA user_version = 16;",
            )
            .context("数据库迁移 v16（memories visibility 三层可见性）失败")?;
        }

        if version < 17 {
            conn.execute_batch(
                "-- 删除重复消息：保留每组(sender, channel, source, timestamp)中 id 最小的
                DELETE FROM messages WHERE id NOT IN (
                    SELECT MIN(id) FROM messages GROUP BY sender, channel, source, timestamp
                );
                -- 添加唯一约束防止未来重复
                CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_dedup
                    ON messages(sender, channel, source, timestamp);
                PRAGMA user_version = 17;",
            )
            .context("数据库迁移 v17（messages 去重 + UNIQUE 约束）失败")?;
        }

        // 补偿：messages 在 v14 插入，但已跳到 v15 的 DB 需要补偿创建
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender TEXT NOT NULL,
                channel TEXT NOT NULL,
                content TEXT,
                source TEXT NOT NULL,
                message_type TEXT NOT NULL DEFAULT 'text',
                timestamp TEXT NOT NULL,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_source ON messages(source, created_at DESC);",
        )
        .context("补偿创建 messages 表失败")?;

        // 补偿：browser_behaviors 在 v10 插入，但旧 DB 可能已跳过该版本
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS browser_behaviors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                event_type TEXT NOT NULL,
                metadata TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_browser_behaviors_source
                ON browser_behaviors(source, created_at DESC);",
        )
        .context("补偿创建 browser_behaviors 表失败")?;

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

    /// 获取最近一条 questioner 生成的每日问题（event_source='questioner', prompt='daily-question'）
    pub fn get_daily_question(&self) -> Result<Option<Suggestion>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let result = conn.query_row(
            "SELECT s.id, s.event_source, s.prompt, s.response, s.created_at,
                    (SELECT f.action FROM feedback f WHERE f.suggestion_id = s.id ORDER BY f.id DESC LIMIT 1)
             FROM suggestions s
             WHERE s.event_source = 'questioner' AND s.prompt = 'daily-question'
             ORDER BY s.created_at DESC
             LIMIT 1",
            [],
            |row| {
                let id: i64 = row.get(0)?;
                let event_source: String = row.get(1)?;
                let prompt: String = row.get(2)?;
                let response: String = row.get(3)?;
                let created_at: String = row.get(4)?;
                let feedback_json: Option<String> = row.get(5)?;
                Ok((id, event_source, prompt, response, created_at, feedback_json))
            },
        ).optional().context("查询每日问题失败")?;

        match result {
            None => Ok(None),
            Some((id, event_source, prompt, response, created_at, feedback_json)) => {
                let timestamp = chrono::DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&chrono::Local))
                    .unwrap_or_else(|_| chrono::Local::now());
                let feedback = feedback_json
                    .and_then(|json| serde_json::from_str::<FeedbackAction>(&json).ok());
                Ok(Some(Suggestion {
                    id,
                    event_source,
                    prompt,
                    response,
                    timestamp,
                    feedback,
                }))
            }
        }
    }

    /// 删除指定 suggestion 及其关联 feedback
    pub fn delete_suggestion(&self, suggestion_id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM feedback WHERE suggestion_id = ?1",
            rusqlite::params![suggestion_id],
        ).context("删除关联 feedback 失败")?;
        conn.execute(
            "DELETE FROM suggestions WHERE id = ?1",
            rusqlite::params![suggestion_id],
        ).context("删除 suggestion 失败")?;
        Ok(())
    }

    /// 更新 suggestion 的 response 内容
    pub fn update_suggestion_response(&self, suggestion_id: i64, response: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let affected = conn.execute(
            "UPDATE suggestions SET response = ?1 WHERE id = ?2",
            rusqlite::params![response, suggestion_id],
        ).context("更新 suggestion 失败")?;
        if affected == 0 {
            anyhow::bail!("Suggestion {suggestion_id} not found");
        }
        Ok(())
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
        let priority: Option<i32> = config.priority.map(|p| p as i32);
        conn.execute(
            "INSERT OR REPLACE INTO provider_config
             (provider_id, api_key, model, base_url, enabled, priority, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                config.provider_id,
                config.api_key,
                config.model,
                config.base_url,
                enabled,
                priority,
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
            .prepare("SELECT provider_id, api_key, model, base_url, enabled, priority FROM provider_config")
            .context("准备 load_provider_configs 查询失败")?;
        let rows = stmt
            .query_map([], |row| {
                let enabled_int: i32 = row.get(4)?;
                let priority_int: Option<i32> = row.get(5)?;
                Ok(ProviderConfig {
                    provider_id: row.get(0)?,
                    api_key: row.get(1)?,
                    model: row.get(2)?,
                    base_url: row.get(3)?,
                    enabled: enabled_int != 0,
                    priority: priority_int.map(|p| p as u8),
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
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO chat_messages (role, content, session_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![role, content, session_id, now],
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

    /// 分层构建 LLM 上下文：core 全量 → working 活跃 → archive 按相关性
    pub fn get_memory_context(&self, max_bytes: usize) -> Result<String> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut sections = Vec::new();

        // 1. Core 层：全量注入（identity/personality/values）
        let core_items = Self::query_memories_by(
            &conn,
            "tier = 'core' AND status = 'active'",
            50,
        )?;
        if !core_items.is_empty() {
            let lines: Vec<String> = core_items.iter().map(|m| format!("- [{}] {}", m.category, m.content)).collect();
            sections.push(format!("## 核心认知\n{}", lines.join("\n")));
        }

        // 2. Working 层：活跃任务/决策（未过期 + status=active）
        let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let working_where = format!(
            "tier = 'working' AND status = 'active' AND (expires_at IS NULL OR expires_at > '{now_str}')"
        );
        let working_items = Self::query_memories_by(
            &conn,
            &working_where,
            20,
        )?;
        if !working_items.is_empty() {
            let lines: Vec<String> = working_items.iter().map(|m| format!("- [{}] {}", m.category, m.content)).collect();
            sections.push(format!("## 当前任务与决策\n{}", lines.join("\n")));
        }

        // 3. Archive 层：最近 + 高 confidence（行为模式、洞察）
        let archive_items = Self::query_memories_by(
            &conn,
            "tier = 'archive' AND status = 'active'",
            30,
        )?;
        if !archive_items.is_empty() {
            let lines: Vec<String> = archive_items.iter().map(|m| format!("- [{}] {}", m.category, m.content)).collect();
            sections.push(format!("## 行为洞察\n{}", lines.join("\n")));
        }

        let full = sections.join("\n\n");
        if full.len() <= max_bytes {
            Ok(full)
        } else {
            Ok(utf8_safe_truncate(&full, max_bytes).to_string())
        }
    }

    /// 按条件查询记忆（内部方法）
    fn query_memories_by(
        conn: &rusqlite::Connection,
        where_clause: &str,
        limit: usize,
    ) -> Result<Vec<sage_types::Memory>> {
        let sql = format!(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at
             FROM memories WHERE {where_clause}
             ORDER BY confidence DESC, updated_at DESC LIMIT ?1"
        );
        let mut stmt = conn.prepare(&sql).context("查询记忆失败")?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ─── Claude Code 共享记忆同步 ──────────────────────────────

    /// 将 SQLite 记忆同步到 Claude Code 的 MEMORY.md
    ///
    /// 在 `<!-- SAGE_SYNC_START -->` / `<!-- SAGE_SYNC_END -->` 标记之间写入，
    /// 保留手动维护的内容不被覆盖。如果标记不存在则追加。
    pub fn sync_to_claude_memory(&self, memory_dir: &std::path::Path) -> Result<()> {
        let memory_file = memory_dir.join("MEMORY.md");

        // 生成 Sage 同步区块
        let sync_block = self.generate_sync_block()?;

        let content = if memory_file.exists() {
            let existing = std::fs::read_to_string(&memory_file)
                .context("读取 MEMORY.md 失败")?;
            Self::replace_sync_section(&existing, &sync_block)
        } else {
            // 文件不存在，创建新的
            std::fs::create_dir_all(memory_dir)?;
            format!("# Project Memory\n\n{sync_block}\n")
        };

        std::fs::write(&memory_file, &content).context("写入 MEMORY.md 失败")?;
        tracing::info!("已同步 Sage 记忆到 {:?}", memory_file);
        Ok(())
    }

    /// 生成同步区块内容
    fn generate_sync_block(&self) -> Result<String> {
        let all_memories = self.load_memories()?;
        // 去重：按内容去重，保留最新的（load_memories 已按 updated_at DESC）
        let mut seen = std::collections::HashSet::new();
        let memories: Vec<_> = all_memories
            .into_iter()
            .filter(|m| seen.insert(m.content.clone()))
            .collect();
        let mut lines: Vec<String> = vec![
            "<!-- SAGE_SYNC_START -->".into(),
            "## Sage Shared Memory".into(),
            String::new(),
            "> Auto-synced from Sage SQLite. Do NOT edit manually — changes will be overwritten.".into(),
            String::new(),
        ];

        // 按 category 分组输出（按价值排序）
        let category_order = [
            ("identity", "Identity"),
            ("personality", "Personality"),
            ("values", "Values"),
            ("behavior", "Behavior Patterns"),
            ("thinking", "Thinking Style"),
            ("emotion", "Emotional Cues"),
            ("growth", "Growth Areas"),
            ("decision", "Recent Decisions"),
            ("pattern", "Observed Patterns"),
            ("coach_insight", "Coach Insights"),
        ];

        let known_cats: std::collections::HashSet<&str> =
            category_order.iter().map(|(c, _)| *c).collect();

        for (cat, label) in &category_order {
            let items: Vec<_> = memories
                .iter()
                .filter(|m| m.category == *cat)
                .take(10) // 每类最多 10 条，防止 MEMORY.md 膨胀
                .collect();
            if items.is_empty() {
                continue;
            }
            lines.push(format!("### {label}"));
            for m in &items {
                // 截断过长内容，保持 MEMORY.md 简洁
                let content: String = m.content.chars().take(200).collect();
                lines.push(format!("- {content}"));
            }
            lines.push(String::new());
        }

        // 未列举的 category（最多 15 条）
        let mut extra: Vec<_> = memories
            .iter()
            .filter(|m| !known_cats.contains(m.category.as_str()))
            .take(15)
            .collect();
        if !extra.is_empty() {
            extra.sort_by(|a, b| a.category.cmp(&b.category));
            lines.push("### Other".into());
            for m in &extra {
                let content: String = m.content.chars().take(200).collect();
                lines.push(format!("- [{}] {content}", m.category));
            }
            lines.push(String::new());
        }

        // 统计摘要
        let session_count = self.count_distinct_sessions().unwrap_or(0);
        lines.push(format!(
            "_Sage stats: {} memories, {} chat sessions_",
            memories.len(),
            session_count
        ));
        lines.push("<!-- SAGE_SYNC_END -->".into());

        Ok(lines.join("\n"))
    }

    /// 替换 MEMORY.md 中 SAGE_SYNC 标记之间的内容
    fn replace_sync_section(existing: &str, new_block: &str) -> String {
        const START: &str = "<!-- SAGE_SYNC_START -->";
        const END: &str = "<!-- SAGE_SYNC_END -->";

        if let (Some(start_pos), Some(end_pos)) = (existing.find(START), existing.find(END)) {
            let before = &existing[..start_pos];
            let after = &existing[end_pos + END.len()..];
            format!("{before}{new_block}{after}")
        } else {
            // 标记不存在，追加到末尾
            let trimmed = existing.trim_end();
            format!("{trimmed}\n\n{new_block}\n")
        }
    }

    /// 保存行为模式记忆（category="pattern"），返回新记录 id
    pub fn append_pattern(&self, category: &str, observation: &str) -> Result<i64> {
        let content = format!("[{category}] {observation}");
        self.save_memory_with_visibility("pattern", &content, "router", 0.6, "public")
    }

    /// 保存决策记忆（category="decision"），返回新记录 id
    pub fn append_decision(&self, context: &str, decision: &str) -> Result<i64> {
        let content = format!("**Context**: {context}\n**Decision**: {decision}");
        self.save_memory_with_visibility("decision", &content, "router", 0.7, "public")
    }

    /// 查询今天已完成的心跳动作标题（用于 daemon 重启后恢复去重状态）
    pub fn get_today_handled_actions(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut stmt = conn.prepare(
            "SELECT content FROM memories WHERE category = 'decision' AND source = 'router' AND created_at >= ?1"
        ).context("准备查询今日动作失败")?;

        let titles: Vec<String> = stmt.query_map(rusqlite::params![today], |row| {
            let content: String = row.get(0)?;
            // content 格式: "**Context**: Morning Brief\n**Decision**: ..."
            // 提取 Context 值作为 title
            Ok(content
                .lines()
                .next()
                .and_then(|line| line.strip_prefix("**Context**: "))
                .unwrap_or("")
                .to_string())
        })?.filter_map(|r| r.ok())
        .filter(|t| !t.is_empty())
        .collect();

        Ok(titles)
    }

    /// 保存教练洞察（category="coach_insight"），返回新记录 id
    pub fn save_coach_insight(&self, insight: &str) -> Result<i64> {
        self.save_memory_with_visibility("coach_insight", insight, "coach", 0.8, "subconscious")
    }

    // ─── Memory 方法 ──────────────────────────────

    /// 根据 category 推断记忆层级
    fn infer_tier(category: &str) -> &'static str {
        match category {
            "identity" | "personality" | "values" => "core",
            "task" | "decision" | "session" | "reminder" | "observer_note" => "working",
            _ => "archive",
        }
    }

    /// working 层默认 TTL（天）
    fn default_ttl_days(category: &str) -> Option<i64> {
        match category {
            "task" | "reminder" => Some(7),
            "decision" => Some(14),
            "session" | "observer_note" => Some(3),
            _ => None,
        }
    }

    /// 保存记忆（自动设置 tier / expires_at）
    pub fn save_memory(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
    ) -> Result<i64> {
        let tier = Self::infer_tier(category);
        let expires_at = Self::default_ttl_days(category).map(|days| {
            (chrono::Local::now() + chrono::Duration::days(days))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        });
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO memories (category, content, source, confidence, tier, status, expires_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?7)",
            rusqlite::params![category, content, source, confidence, tier, expires_at, now],
        )
        .context("保存 memory 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 清理过期 working 记忆：标记为 expired
    pub fn expire_stale_memories(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let count = conn.execute(
            "UPDATE memories SET status = 'expired'
             WHERE tier = 'working' AND status = 'active'
             AND expires_at IS NOT NULL AND expires_at < ?1",
            rusqlite::params![now],
        ).context("清理过期记忆失败")?;
        Ok(count)
    }

    /// 将 working 任务标记为完成
    pub fn complete_task(&self, memory_id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET status = 'done', updated_at = ?2
             WHERE id = ?1 AND tier = 'working'",
            rusqlite::params![memory_id, now],
        ).context("标记任务完成失败")?;
        Ok(())
    }

    /// 按 source 删除记忆（用于 session ingestion 的 upsert 场景）
    pub fn delete_memory_by_source(&self, source: &str) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let deleted = conn.execute(
            "DELETE FROM memories WHERE source = ?1",
            rusqlite::params![source],
        ).context("按 source 删除 memory 失败")?;
        Ok(deleted)
    }

    /// 更新记忆的内容和置信度
    pub fn update_memory(&self, id: i64, content: &str, confidence: f64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET content = ?1, confidence = ?2, updated_at = ?4 WHERE id = ?3",
            rusqlite::params![content, confidence, id, now],
        )
        .context("更新 memory 失败")?;
        Ok(())
    }

    /// 从行中构建 Memory（SELECT 列顺序: id, category, content, source, confidence, visibility, created_at, updated_at）
    fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
        Ok(Memory {
            id: row.get(0)?,
            category: row.get(1)?,
            content: row.get(2)?,
            source: row.get(3)?,
            confidence: row.get(4)?,
            visibility: row.get::<_, String>(5).unwrap_or_else(|_| "public".to_string()),
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    /// 加载所有记忆（按置信度和更新时间排序）
    pub fn load_memories(&self) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at FROM memories ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 load_memories 查询失败")?;
        let rows = stmt.query_map([], Self::row_to_memory)?;
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
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at
             FROM memories
             WHERE content LIKE ?1 OR category LIKE ?1
             ORDER BY confidence DESC
             LIMIT ?2",
        ).context("准备 search_memories 查询失败")?;

        let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], Self::row_to_memory)
            .context("执行 search_memories 查询失败")?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// FTS 检索 + 图谱扩展：先找 seed 记忆，再通过边拉取一跳邻居，合并去重排序
    pub fn search_memories_with_graph(
        &self,
        query: &str,
        seed_limit: usize,
        total_limit: usize,
    ) -> Result<Vec<Memory>> {
        let seeds = self.search_memories(query, seed_limit)?;
        if seeds.is_empty() {
            return Ok(seeds);
        }

        // seed 记忆的得分 = confidence，图谱邻居的得分 = activation
        let mut score_map: std::collections::HashMap<i64, (Memory, f64)> =
            std::collections::HashMap::new();
        for m in &seeds {
            score_map.insert(m.id, (m.clone(), m.confidence));
        }

        // 对每个 seed 拉一跳图谱邻居
        for seed in &seeds {
            if let Ok(neighbors) = self.get_connected_memories(seed.id, 1) {
                for (mem, activation) in neighbors {
                    let entry = score_map.entry(mem.id).or_insert((mem.clone(), 0.0));
                    if activation > entry.1 {
                        entry.1 = activation;
                    }
                }
            }
        }

        let mut results: Vec<(Memory, f64)> = score_map.into_values().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(total_limit);
        Ok(results.into_iter().map(|(m, _)| m).collect())
    }

    /// 删除记忆
    pub fn delete_memory(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
            .context("删除 memory 失败")?;
        Ok(())
    }

    // ─── Visibility 方法 ──────────────────────────────

    /// 保存带可见性的记忆
    pub fn save_memory_with_visibility(
        &self,
        category: &str,
        content: &str,
        source: &str,
        confidence: f64,
        visibility: &str,
    ) -> Result<i64> {
        let tier = Self::infer_tier(category);
        let expires_at = Self::default_ttl_days(category).map(|days| {
            (chrono::Local::now() + chrono::Duration::days(days))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        });
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO memories (category, content, source, confidence, tier, status, expires_at, visibility, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?8)",
            rusqlite::params![category, content, source, confidence, tier, expires_at, visibility, now],
        )
        .context("保存 memory（带 visibility）失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 搜索公开记忆（Digital Evan 使用）
    pub fn search_public_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.get_memories_by_visibility("public");
        }
        let pattern = format!("%{trimmed}%");
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at
             FROM memories
             WHERE visibility = 'public' AND (content LIKE ?1 OR category LIKE ?1)
             ORDER BY confidence DESC
             LIMIT ?2",
        ).context("准备 search_public_memories 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按可见性层级加载记忆
    pub fn get_memories_by_visibility(&self, visibility: &str) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at
             FROM memories WHERE visibility = ?1
             ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 get_memories_by_visibility 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![visibility], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 更新记忆可见性
    pub fn update_memory_visibility(&self, id: i64, visibility: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET visibility = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![visibility, now, id],
        )
        .context("更新 memory visibility 失败")?;
        Ok(())
    }

    /// 统计各可见性层级的记忆数量
    pub fn count_memories_by_visibility(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT visibility, COUNT(*) FROM memories GROUP BY visibility ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ─── Report 方法 ──────────────────────────────

    /// 保存报告，返回自增 id
    pub fn save_report(&self, report_type: &str, content: &str) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO reports (report_type, content, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![report_type, content, now],
        )
        .context("保存 report 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取指定类型的最新一条报告
    pub fn get_latest_report(&self, report_type: &str) -> Result<Option<Report>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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

    /// 获取某个日期之后创建的 memories（用于报告上下文收集）
    pub fn get_memories_since(&self, since: &str) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at FROM memories WHERE created_at >= ?1 ORDER BY created_at DESC",
        ).context("准备 get_memories_since 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![since], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取某个日期之后的 observations 数量
    pub fn count_observations_since(&self, since: &str) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM observations WHERE created_at >= ?1",
            rusqlite::params![since],
            |row| row.get(0),
        ).context("统计 observations 数量失败")?;
        Ok(count as usize)
    }

    /// 获取某个日期之后的 session 类 memories
    pub fn get_session_summaries_since(&self, since: &str) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at FROM memories WHERE category = 'session' AND created_at >= ?1 ORDER BY created_at DESC",
        ).context("准备 get_session_summaries_since 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![since], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取某个日期之后的 coach insights 内容列表
    pub fn get_coach_insights_since(&self, since: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT content FROM memories WHERE category = 'coach_insight' AND created_at >= ?1 ORDER BY created_at DESC",
        ).context("准备 get_coach_insights_since 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![since], |row| {
            row.get::<_, String>(0)
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ─── Memory Evolution 方法 ──────────────────────────────

    /// 加载所有活跃记忆（status='active'）
    pub fn load_active_memories(&self) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, content, source, confidence, visibility, created_at, updated_at
             FROM memories WHERE status = 'active'
             ORDER BY confidence DESC, updated_at DESC",
        ).context("准备 load_active_memories 查询失败")?;
        let rows = stmt.query_map([], Self::row_to_memory)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 衰减长期未更新的 archive 记忆（纯 SQL）
    /// - stale_days: 多少天未更新算"过期"
    /// - decay_amount: 每次衰减的 confidence 量
    /// - expire_threshold: confidence 低于此值则标记为 expired
    pub fn decay_stale_archive_memories(
        &self,
        stale_days: i64,
        decay_amount: f64,
        expire_threshold: f64,
    ) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let cutoff = (chrono::Local::now() - chrono::Duration::days(stale_days))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let now = chrono::Local::now().to_rfc3339();

        // 衰减
        let decayed = conn.execute(
            "UPDATE memories SET confidence = MAX(0.1, confidence - ?1), updated_at = ?2
             WHERE tier = 'archive' AND status = 'active'
             AND updated_at < ?3 AND confidence > ?4",
            rusqlite::params![decay_amount, now, cutoff, expire_threshold],
        ).context("衰减记忆失败")?;

        // 低于阈值的标记为 expired
        conn.execute(
            "UPDATE memories SET status = 'expired'
             WHERE tier = 'archive' AND status = 'active' AND confidence <= ?1",
            rusqlite::params![expire_threshold],
        ).context("过期低置信度记忆失败")?;

        Ok(decayed)
    }

    /// 提升高置信度 archive 记忆到 core（限定特定行为/模式类别）
    pub fn promote_high_confidence_memories(&self, min_confidence: f64) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let promoted = conn.execute(
            "UPDATE memories SET tier = 'core', updated_at = ?1
             WHERE tier = 'archive' AND status = 'active' AND confidence >= ?2
             AND category IN ('behavior', 'thinking', 'pattern', 'growth', 'emotion')
             AND updated_at != created_at",
            rusqlite::params![now, min_confidence],
        ).context("提升记忆到 core 失败")?;
        Ok(promoted)
    }

    // ─── Open Questions 方法（Questioner 实体化） ──────────────────────────────

    /// 保存开放问题
    pub fn save_open_question(
        &self,
        question_text: &str,
        source_suggestion_id: Option<i64>,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let next_ask = (chrono::Local::now() + chrono::Duration::days(3))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        conn.execute(
            "INSERT INTO open_questions (question_text, source_suggestion_id, status, ask_count, next_ask_at, created_at)
             VALUES (?1, ?2, 'open', 1, ?3, ?4)",
            rusqlite::params![question_text, source_suggestion_id, next_ask, now],
        ).context("保存 open_question 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取到期需要重新提问的开放问题
    pub fn get_due_questions(&self, limit: usize) -> Result<Vec<(i64, String, i32)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let mut stmt = conn.prepare(
            "SELECT id, question_text, ask_count FROM open_questions
             WHERE status = 'open' AND next_ask_at <= ?1 AND ask_count < 4
             ORDER BY next_ask_at ASC LIMIT ?2",
        ).context("查询 due questions 失败")?;
        let rows = stmt.query_map(rusqlite::params![now, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 标记问题为已回答
    pub fn answer_question(&self, question_id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "UPDATE open_questions SET status = 'answered', answered_at = ?1 WHERE id = ?2",
            rusqlite::params![now, question_id],
        ).context("标记问题已回答失败")?;
        Ok(())
    }

    /// 增加问题提问次数，更新下次提问时间（间隔递增：3d→7d→14d）
    /// 超过 3 次自动归档
    pub fn bump_question_ask(&self, question_id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let ask_count: i32 = conn
            .query_row(
                "SELECT ask_count FROM open_questions WHERE id = ?1",
                rusqlite::params![question_id],
                |row| row.get(0),
            )
            .context("查询 ask_count 失败")?;

        let interval_days = match ask_count {
            1 => 3,
            2 => 7,
            _ => 14,
        };
        let next_ask = (chrono::Local::now() + chrono::Duration::days(interval_days))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        conn.execute(
            "UPDATE open_questions SET ask_count = ask_count + 1, next_ask_at = ?1 WHERE id = ?2",
            rusqlite::params![next_ask, question_id],
        ).context("更新问题提问次数失败")?;

        // 超过 3 次归档
        if ask_count + 1 >= 4 {
            conn.execute(
                "UPDATE open_questions SET status = 'archived' WHERE id = ?1",
                rusqlite::params![question_id],
            ).context("归档超限问题失败")?;
        }

        Ok(())
    }

    /// 搜索开放问题（用于 Chat 中匹配用户是否在回答某个问题）
    /// 加载最近 24 小时内的 observer_note 记忆（供 Coach 使用）
    pub fn load_observer_notes_recent(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT content FROM memories
             WHERE category = 'observer_note'
               AND created_at >= datetime('now', '-24 hours')
             ORDER BY created_at ASC
             LIMIT 100",
        ).context("查询 observer_notes 失败")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ─── Memory Graph 方法（记忆图谱） ──────────────────────────────

    /// 添加记忆之间的边（连接），存在则更新权重
    pub fn save_memory_edge(
        &self,
        from_id: i64,
        to_id: i64,
        relation: &str,
        weight: f64,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO memory_edges (from_id, to_id, relation, weight, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(from_id, to_id, relation) DO UPDATE SET weight = ?4",
            rusqlite::params![from_id, to_id, relation, weight, now],
        )
        .context("保存 memory_edge 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// Hebbian 共现强化：同一 chat turn 被召回的记忆两两加强连接
    pub fn strengthen_edges(&self, memory_ids: &[i64]) -> Result<usize> {
        if memory_ids.len() < 2 {
            return Ok(0);
        }
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let mut strengthened = 0;

        for i in 0..memory_ids.len() {
            for j in (i + 1)..memory_ids.len() {
                let (a, b) = (memory_ids[i], memory_ids[j]);
                // 查现有边（任意方向）
                let existing: Option<(i64, f64)> = conn.query_row(
                    "SELECT id, weight FROM memory_edges
                     WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)
                     LIMIT 1",
                    rusqlite::params![a, b],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                ).optional()?;

                match existing {
                    Some((edge_id, w)) => {
                        let new_w = (w + 0.05).min(1.0);
                        conn.execute(
                            "UPDATE memory_edges SET weight = ?1, last_activated_at = ?2 WHERE id = ?3",
                            rusqlite::params![new_w, now, edge_id],
                        )?;
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO memory_edges (from_id, to_id, relation, weight, created_at, last_activated_at)
                             VALUES (?1, ?2, 'co_occurred', 0.3, ?3, ?3)",
                            rusqlite::params![a, b, now],
                        )?;
                    }
                }
                strengthened += 1;
            }
        }
        Ok(strengthened)
    }

    /// 衰减长期未激活的边：weight *= decay_factor，低于阈值则删除
    pub fn decay_cold_edges(&self, cold_days: u32, decay_factor: f64, min_weight: f64) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let threshold = format!("-{cold_days} days");

        // 衰减旧边（last_activated_at 为 NULL 或超过 cold_days 天）
        conn.execute(
            "UPDATE memory_edges SET weight = weight * ?1
             WHERE last_activated_at IS NULL OR last_activated_at < datetime('now', ?2)",
            rusqlite::params![decay_factor, threshold],
        )?;

        // 清理过轻的边
        let deleted = conn.execute(
            "DELETE FROM memory_edges WHERE weight < ?1",
            rusqlite::params![min_weight],
        )?;

        Ok(deleted)
    }

    /// 获取指定记忆的所有相邻边（双向）
    pub fn get_memory_edges(&self, memory_id: i64) -> Result<Vec<MemoryEdge>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, from_id, to_id, relation, weight, created_at
             FROM memory_edges
             WHERE from_id = ?1 OR to_id = ?1
             ORDER BY weight DESC",
        ).context("准备 get_memory_edges 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![memory_id], |row| {
            Ok(MemoryEdge {
                id: row.get(0)?,
                from_id: row.get(1)?,
                to_id: row.get(2)?,
                relation: row.get(3)?,
                weight: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取所有边（用于图谱可视化）
    pub fn get_all_memory_edges(&self) -> Result<Vec<MemoryEdge>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT e.id, e.from_id, e.to_id, e.relation, e.weight, e.created_at
             FROM memory_edges e
             INNER JOIN memories m1 ON e.from_id = m1.id
             INNER JOIN memories m2 ON e.to_id = m2.id
             WHERE m1.status = 'active' AND m2.status = 'active'
             ORDER BY e.weight DESC",
        ).context("准备 get_all_memory_edges 查询失败")?;
        let rows = stmt.query_map([], |row| {
            Ok(MemoryEdge {
                id: row.get(0)?,
                from_id: row.get(1)?,
                to_id: row.get(2)?,
                relation: row.get(3)?,
                weight: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 删除指定的边
    pub fn delete_memory_edge(&self, edge_id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM memory_edges WHERE id = ?1", rusqlite::params![edge_id])
            .context("删除 memory_edge 失败")?;
        Ok(())
    }

    /// 图谱遍历：从起始记忆出发，获取 N 跳内的相关记忆（spreading activation）
    pub fn get_connected_memories(&self, start_id: i64, max_hops: usize) -> Result<Vec<(Memory, f64)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;

        // BFS 遍历：收集 max_hops 内的所有连接节点及其衰减权重
        let mut visited: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
        visited.insert(start_id, 1.0);
        let mut frontier = vec![(start_id, 1.0)];

        for _hop in 0..max_hops {
            let mut next_frontier = Vec::new();
            for (node_id, activation) in &frontier {
                let mut stmt = conn.prepare(
                    "SELECT from_id, to_id, weight FROM memory_edges
                     WHERE from_id = ?1 OR to_id = ?1",
                ).context("图谱遍历查询失败")?;
                let edges: Vec<(i64, i64, f64)> = stmt.query_map(
                    rusqlite::params![node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?.filter_map(|r| r.ok()).collect();

                for (from, to, weight) in edges {
                    let neighbor = if from == *node_id { to } else { from };
                    let new_activation = activation * weight * 0.7; // 衰减因子 0.7
                    if new_activation > 0.1 {
                        let entry = visited.entry(neighbor).or_insert(0.0);
                        if new_activation > *entry {
                            *entry = new_activation;
                            next_frontier.push((neighbor, new_activation));
                        }
                    }
                }
            }
            frontier = next_frontier;
        }

        // 排除起始节点，查询记忆详情
        visited.remove(&start_id);
        let mut results = Vec::new();
        for (mem_id, activation) in &visited {
            let mem = conn.query_row(
                "SELECT id, category, content, source, confidence, visibility, created_at, updated_at
                 FROM memories WHERE id = ?1 AND status = 'active'",
                rusqlite::params![mem_id],
                Self::row_to_memory,
            ).optional()?;
            if let Some(m) = mem {
                results.push((m, *activation));
            }
        }
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }

    /// 统计图谱边数
    pub fn count_memory_edges(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_edges", [], |row| row.get(0),
        ).context("统计 memory_edges 失败")?;
        Ok(count as usize)
    }

    // ─── Knowledge Edges 方法（通用知识图谱） ──────────────────────────────

    /// 保存知识图谱边（不同类型节点之间的连接），存在则更新权重
    pub fn save_knowledge_edge(
        &self,
        from_type: &str,
        from_id: i64,
        to_type: &str,
        to_id: i64,
        relation: &str,
        weight: f64,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO knowledge_edges (from_type, from_id, to_type, to_id, relation, weight, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(from_type, from_id, to_type, to_id, relation) DO UPDATE SET weight = ?6",
            rusqlite::params![from_type, from_id, to_type, to_id, relation, weight, now],
        )
        .context("保存 knowledge_edge 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取指定节点的所有连接（双向）
    pub fn get_knowledge_edges(&self, node_type: &str, node_id: i64) -> Result<Vec<KnowledgeEdge>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, from_type, from_id, to_type, to_id, relation, weight, created_at
             FROM knowledge_edges
             WHERE (from_type = ?1 AND from_id = ?2) OR (to_type = ?1 AND to_id = ?2)
             ORDER BY weight DESC",
        ).context("准备 get_knowledge_edges 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![node_type, node_id], |row| {
            Ok(KnowledgeEdge {
                id: row.get(0)?,
                from_type: row.get(1)?,
                from_id: row.get(2)?,
                to_type: row.get(3)?,
                to_id: row.get(4)?,
                relation: row.get(5)?,
                weight: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取两种类型之间的所有连接
    pub fn get_knowledge_edges_between_types(
        &self,
        from_type: &str,
        to_type: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeEdge>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, from_type, from_id, to_type, to_id, relation, weight, created_at
             FROM knowledge_edges
             WHERE from_type = ?1 AND to_type = ?2
             ORDER BY weight DESC LIMIT ?3",
        ).context("准备 get_knowledge_edges_between_types 查询失败")?;
        let rows = stmt.query_map(rusqlite::params![from_type, to_type, limit as i64], |row| {
            Ok(KnowledgeEdge {
                id: row.get(0)?,
                from_type: row.get(1)?,
                from_id: row.get(2)?,
                to_type: row.get(3)?,
                to_id: row.get(4)?,
                relation: row.get(5)?,
                weight: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 获取所有知识图谱边（用于图谱可视化）
    pub fn get_all_knowledge_edges(&self) -> Result<Vec<KnowledgeEdge>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, from_type, from_id, to_type, to_id, relation, weight, created_at
             FROM knowledge_edges
             ORDER BY weight DESC",
        ).context("准备 get_all_knowledge_edges 查询失败")?;
        let rows = stmt.query_map([], |row| {
            Ok(KnowledgeEdge {
                id: row.get(0)?,
                from_type: row.get(1)?,
                from_id: row.get(2)?,
                to_type: row.get(3)?,
                to_id: row.get(4)?,
                relation: row.get(5)?,
                weight: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 删除指定的知识图谱边
    pub fn delete_knowledge_edge(&self, edge_id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM knowledge_edges WHERE id = ?1", rusqlite::params![edge_id])
            .context("删除 knowledge_edge 失败")?;
        Ok(())
    }

    /// 统计知识图谱边数
    pub fn count_knowledge_edges(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge_edges", [], |row| row.get(0),
        ).context("统计 knowledge_edges 失败")?;
        Ok(count as usize)
    }

    // ─── Tag 方法 ──────────────────────────────

    /// 给记忆添加标签（忽略重复）
    pub fn add_tag(&self, memory_id: i64, tag: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
            rusqlite::params![memory_id, tag.trim().to_lowercase()],
        ).context("添加标签失败")?;
        Ok(())
    }

    /// 批量给记忆添加标签
    pub fn add_tags(&self, memory_id: i64, tags: &[&str]) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
        )?;
        for tag in tags {
            let t = tag.trim().to_lowercase();
            if !t.is_empty() {
                stmt.execute(rusqlite::params![memory_id, t])?;
            }
        }
        Ok(())
    }

    /// 获取某条记忆的所有标签
    pub fn get_tags(&self, memory_id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag",
        )?;
        let tags: Vec<String> = stmt.query_map([memory_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tags)
    }

    /// 获取所有标签及其记忆数量（按数量降序）
    pub fn get_all_tags(&self) -> Result<Vec<(String, usize)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT t.tag, COUNT(*) as cnt FROM memory_tags t
             JOIN memories m ON t.memory_id = m.id
             WHERE m.status = 'active'
             GROUP BY t.tag ORDER BY cnt DESC",
        )?;
        let tags: Vec<(String, usize)> = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
        })?.filter_map(|r| r.ok()).collect();
        Ok(tags)
    }

    /// 获取带有指定标签的所有记忆 ID
    pub fn get_memories_by_tag(&self, tag: &str) -> Result<Vec<i64>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT memory_id FROM memory_tags WHERE tag = ?1",
        )?;
        let ids: Vec<i64> = stmt.query_map([tag.trim().to_lowercase()], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    /// 删除记忆的某个标签
    pub fn remove_tag(&self, memory_id: i64, tag: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM memory_tags WHERE memory_id = ?1 AND tag = ?2",
            rusqlite::params![memory_id, tag.trim().to_lowercase()],
        )?;
        Ok(())
    }

    pub fn search_open_questions(&self, query: &str) -> Result<Vec<(i64, String)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{}%", query.trim());
        let mut stmt = conn.prepare(
            "SELECT id, question_text FROM open_questions
             WHERE status = 'open' AND question_text LIKE ?1
             LIMIT 5",
        ).context("搜索 open_questions 失败")?;
        let rows = stmt.query_map(rusqlite::params![pattern], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ─── Browser Bridge 方法 ──────────────────────────────

    /// 保存浏览器行为事件
    pub fn save_browser_behavior(&self, source: &str, event_type: &str, metadata: &str) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT INTO browser_behaviors (source, event_type, metadata) VALUES (?1, ?2, ?3)",
            rusqlite::params![source, event_type, metadata],
        ).context("保存浏览器行为失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 获取最近 N 条浏览器行为事件
    pub fn get_browser_behaviors(&self, limit: usize) -> Result<Vec<BrowserBehaviorRow>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, source, event_type, metadata, created_at
             FROM browser_behaviors ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(BrowserBehaviorRow {
                id: row.get(0)?,
                source: row.get(1)?,
                event_type: row.get(2)?,
                metadata: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 获取指定时间之后的浏览器行为（用于报告生成）
    pub fn get_browser_behaviors_since(&self, since: &str) -> Result<Vec<BrowserBehaviorRow>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, source, event_type, metadata, created_at
             FROM browser_behaviors WHERE created_at >= ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![since], |row| {
            Ok(BrowserBehaviorRow {
                id: row.get(0)?,
                source: row.get(1)?,
                event_type: row.get(2)?,
                metadata: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 浏览器行为事件总数
    pub fn count_browser_behaviors(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM browser_behaviors",
            [],
            |row| row.get(0),
        ).unwrap_or(0);
        Ok(count)
    }

    // ─── Message 方法（通讯消息）──────────────────────────────

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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO messages (sender, channel, content, source, message_type, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![sender, channel, content, source, message_type, timestamp],
        )
        .context("保存 message 失败")?;
        Ok(conn.last_insert_rowid())
    }

    /// 按频道查询消息
    pub fn get_messages_by_channel(&self, channel: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at
             FROM messages WHERE channel = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![channel, limit as i64], Self::row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按来源查询消息
    pub fn get_messages_by_source(&self, source: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at
             FROM messages WHERE source = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![source, limit as i64], Self::row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 搜索消息内容
    pub fn search_messages(&self, query: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let pattern = format!("%{query}%");
        let mut stmt = conn.prepare(
            "SELECT id, sender, channel, content, source, message_type, timestamp, created_at
             FROM messages
             WHERE content LIKE ?1 OR sender LIKE ?1 OR channel LIKE ?1
             ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], Self::row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 今日消息统计（按来源分组）
    pub fn get_today_message_stats(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT channel, source, COUNT(*) as cnt FROM messages
             GROUP BY channel, source ORDER BY cnt DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 消息总数
    pub fn count_messages(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages", [], |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// 删除指定消息
    pub fn delete_message(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute("DELETE FROM messages WHERE id = ?1", rusqlite::params![id])
            .context("删除 message 失败")?;
        Ok(())
    }

    /// 行映射辅助
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
        })
    }

    /// 记忆总数（活跃状态）
    pub fn count_memories(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE status = 'active'",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
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
            priority: None,
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
            priority: None,
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
            priority: None,
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

        // 写入各层记忆
        store.save_memory("identity", "重视团队成长", "test", 0.9).unwrap();   // → core
        store.save_memory("behavior", "每天下午查邮件", "test", 0.7).unwrap(); // → archive
        store.save_memory("task", "选择 Rust 作为核心语言", "test", 0.8).unwrap(); // → working
        store.save_memory("coach_insight", "Evan 决策偏系统思考", "test", 0.8).unwrap(); // → archive

        let ctx = store.get_memory_context(10000).unwrap();

        // 验证分层标题
        assert!(ctx.contains("## 核心认知"), "应包含核心认知");
        assert!(ctx.contains("## 当前任务与决策"), "应包含当前任务与决策");
        assert!(ctx.contains("## 行为洞察"), "应包含行为洞察");

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
    fn test_get_today_handled_actions() {
        let store = Store::open_in_memory().unwrap();

        store.append_decision("Morning Brief", "今日日程...").unwrap();
        store.append_decision("Email Check", "2封未读邮件").unwrap();

        let actions = store.get_today_handled_actions().unwrap();
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&"Morning Brief".to_string()));
        assert!(actions.contains(&"Email Check".to_string()));
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

    // ─── Shared Memory Sync Tests ──────────────────────────

    #[test]
    fn test_sync_to_claude_memory_creates_file() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("identity", "Software team lead", "test", 0.9).unwrap();
        store.save_memory("values", "Team growth over individual performance", "test", 0.8).unwrap();

        let dir = tempfile::tempdir().unwrap();
        store.sync_to_claude_memory(dir.path()).unwrap();

        let content = std::fs::read_to_string(dir.path().join("MEMORY.md")).unwrap();
        assert!(content.contains("SAGE_SYNC_START"));
        assert!(content.contains("SAGE_SYNC_END"));
        assert!(content.contains("Software team lead"));
        assert!(content.contains("Team growth"));
        assert!(content.contains("Sage Shared Memory"));
    }

    #[test]
    fn test_sync_preserves_existing_content() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("identity", "Test user", "test", 0.9).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let memory_file = dir.path().join("MEMORY.md");
        std::fs::write(&memory_file, "# My Project\n\nManual notes here.\n").unwrap();

        store.sync_to_claude_memory(dir.path()).unwrap();

        let content = std::fs::read_to_string(&memory_file).unwrap();
        assert!(content.contains("# My Project"), "manual content preserved");
        assert!(content.contains("Manual notes here"), "manual content preserved");
        assert!(content.contains("Test user"), "sync content added");
    }

    #[test]
    fn test_sync_replaces_existing_sync_section() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("identity", "Updated info", "test", 0.9).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let memory_file = dir.path().join("MEMORY.md");
        std::fs::write(
            &memory_file,
            "# Project\n\n<!-- SAGE_SYNC_START -->\nold data\n<!-- SAGE_SYNC_END -->\n\n# Footer\n",
        ).unwrap();

        store.sync_to_claude_memory(dir.path()).unwrap();

        let content = std::fs::read_to_string(&memory_file).unwrap();
        assert!(!content.contains("old data"), "old sync section replaced");
        assert!(content.contains("Updated info"), "new sync content present");
        assert!(content.contains("# Footer"), "content after sync preserved");
    }

    #[test]
    fn test_replace_sync_section_static() {
        let existing = "# Header\n\nSome content.\n\n<!-- SAGE_SYNC_START -->\nold\n<!-- SAGE_SYNC_END -->\n\nMore content.\n";
        let result = Store::replace_sync_section(existing, "<!-- SAGE_SYNC_START -->\nnew\n<!-- SAGE_SYNC_END -->");
        assert!(result.contains("# Header"));
        assert!(result.contains("new"));
        assert!(!result.contains("old"));
        assert!(result.contains("More content."));
    }

    // ─── Report 测试 ──────────────────────────────

    #[test]
    fn test_save_and_load_report() {
        let store = Store::open_in_memory().unwrap();
        store.save_report("weekly", "本周报告内容").unwrap();
        store.save_report("weekly", "更新的周报").unwrap();
        store.save_report("morning", "早间 brief").unwrap();

        let latest = store.get_latest_report("weekly").unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().content, "更新的周报");

        let all = store.get_reports("weekly", 10).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_get_latest_report_empty() {
        let store = Store::open_in_memory().unwrap();
        let result = store.get_latest_report("morning").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_all_reports() {
        let store = Store::open_in_memory().unwrap();
        store.save_report("morning", "早间报告").unwrap();
        store.save_report("evening", "晚间回顾").unwrap();
        store.save_report("weekly", "周报").unwrap();

        let all = store.get_all_reports(10).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_get_memories_since() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("decision", "chose Rust", "chat", 0.8).unwrap();
        store.save_memory("identity", "Evan is a team lead", "chat", 0.9).unwrap();

        // since 设为过去很久以前，应该能取到全部
        let memories = store.get_memories_since("2000-01-01T00:00:00+00:00").unwrap();
        assert_eq!(memories.len(), 2);

        // since 设为未来，应该取不到
        let empty = store.get_memories_since("2099-01-01T00:00:00+00:00").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_count_observations_since() {
        let store = Store::open_in_memory().unwrap();
        store.record_observation("pattern", "obs1", None).unwrap();
        store.record_observation("habit", "obs2", None).unwrap();

        let count = store.count_observations_since("2000-01-01T00:00:00+00:00").unwrap();
        assert_eq!(count, 2);

        let zero = store.count_observations_since("2099-01-01T00:00:00+00:00").unwrap();
        assert_eq!(zero, 0);
    }

    #[test]
    fn test_get_session_summaries_since() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("session", "[session] fix bugs — 50 msgs", "claude-code", 0.8).unwrap();
        store.save_memory("decision", "chose async", "chat", 0.7).unwrap();

        let sessions = store.get_session_summaries_since("2000-01-01T00:00:00+00:00").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].category, "session");
    }

    #[test]
    fn test_get_coach_insights_since() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("coach_insight", "Evan 偏系统思考", "coach", 0.8).unwrap();
        store.save_memory("coach_insight", "喜欢类比解释", "coach", 0.7).unwrap();
        store.save_memory("decision", "not a coach insight", "chat", 0.5).unwrap();

        let insights = store.get_coach_insights_since("2000-01-01T00:00:00+00:00").unwrap();
        assert_eq!(insights.len(), 2);
        assert!(insights.iter().all(|s| !s.is_empty()));
    }

    // ─── Memory Evolution 测试 ──────────────────────────────

    #[test]
    fn test_load_active_memories() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("behavior", "active memory", "test", 0.8).unwrap();
        store.save_memory("behavior", "another active", "test", 0.6).unwrap();

        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 2);
        // 按 confidence DESC 排序
        assert!(active[0].confidence >= active[1].confidence);
    }

    #[test]
    fn test_load_active_excludes_expired() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("behavior", "active one", "test", 0.8).unwrap();

        // 手动将一条记忆标记为 expired
        let id = store.save_memory("behavior", "will expire", "test", 0.3).unwrap();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE memories SET status = 'expired' WHERE id = ?1",
            rusqlite::params![id],
        ).unwrap();
        drop(conn);

        let active = store.load_active_memories().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].content, "active one");
    }

    #[test]
    fn test_decay_stale_archive_memories() {
        let store = Store::open_in_memory().unwrap();

        // 创建一条 archive 记忆，手动设置 updated_at 为 90 天前
        let id = store.save_memory("pattern", "old pattern", "test", 0.5).unwrap();
        let old_date = (chrono::Local::now() - chrono::Duration::days(90))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE memories SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![old_date, id],
        ).unwrap();
        drop(conn);

        // 衰减 60 天未更新的，衰减 0.1，阈值 0.2
        let decayed = store.decay_stale_archive_memories(60, 0.1, 0.2).unwrap();
        assert_eq!(decayed, 1);

        // 验证 confidence 降低了
        let memories = store.load_memories().unwrap();
        let m = memories.iter().find(|m| m.id == id).unwrap();
        assert!((m.confidence - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_decay_expires_low_confidence() {
        let store = Store::open_in_memory().unwrap();

        // 创建一条低 confidence 的 archive 记忆
        let id = store.save_memory("pattern", "weak pattern", "test", 0.2).unwrap();
        let old_date = (chrono::Local::now() - chrono::Duration::days(90))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE memories SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![old_date, id],
        ).unwrap();
        drop(conn);

        // 衰减后 confidence 0.2 - 0.1 = 0.1，但 0.2 <= threshold，所以应该被 expired
        store.decay_stale_archive_memories(60, 0.1, 0.2).unwrap();

        // 检查 status 变为 expired
        let active = store.load_active_memories().unwrap();
        assert!(active.iter().all(|m| m.id != id));
    }

    #[test]
    fn test_promote_high_confidence_memories() {
        let store = Store::open_in_memory().unwrap();

        // 创建一条高 confidence 的 behavior 记忆（archive 层）
        let id = store.save_memory("behavior", "consistent pattern", "coach", 0.9).unwrap();

        // 需要 updated_at != created_at 才会提升
        store.update_memory(id, "consistent pattern (confirmed)", 0.9).unwrap();

        let promoted = store.promote_high_confidence_memories(0.85).unwrap();
        assert_eq!(promoted, 1);

        // 验证 tier 变为 core
        let conn = store.conn.lock().unwrap();
        let tier: String = conn.query_row(
            "SELECT tier FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(tier, "core");
    }

    #[test]
    fn test_promote_ignores_unconfirmed() {
        let store = Store::open_in_memory().unwrap();

        // 高 confidence 但 updated_at == created_at（从未被更新确认过）
        store.save_memory("behavior", "new observation", "coach", 0.9).unwrap();

        let promoted = store.promote_high_confidence_memories(0.85).unwrap();
        assert_eq!(promoted, 0);
    }

    #[test]
    fn test_promote_ignores_wrong_categories() {
        let store = Store::open_in_memory().unwrap();

        // 高 confidence 的 identity 记忆（已是 core，不该再提升）
        let id = store.save_memory("identity", "I am Evan", "user", 0.95).unwrap();
        store.update_memory(id, "I am Evan (confirmed)", 0.95).unwrap();

        // identity 已是 core 层，promote 的 WHERE 筛选 tier='archive'
        let promoted = store.promote_high_confidence_memories(0.85).unwrap();
        assert_eq!(promoted, 0);
    }

    // ─── Open Questions 测试 ──────────────────────────────

    #[test]
    fn test_save_and_search_open_question() {
        let store = Store::open_in_memory().unwrap();

        let id = store.save_open_question("你为什么选择这个方向？", None).unwrap();
        assert!(id > 0);

        let results = store.search_open_questions("方向").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id);
        assert!(results[0].1.contains("方向"));
    }

    #[test]
    fn test_open_question_with_suggestion_link() {
        let store = Store::open_in_memory().unwrap();

        let suggestion_id = store.record_suggestion("questioner", "daily-question", "test q").unwrap();
        let q_id = store.save_open_question("test question", Some(suggestion_id)).unwrap();
        assert!(q_id > 0);
    }

    #[test]
    fn test_get_due_questions_respects_time() {
        let store = Store::open_in_memory().unwrap();

        // 新问题的 next_ask_at 是 3 天后，不应该 due
        store.save_open_question("future question", None).unwrap();

        let due = store.get_due_questions(10).unwrap();
        assert!(due.is_empty(), "新问题不应该立即到期");
    }

    #[test]
    fn test_get_due_questions_returns_past_due() {
        let store = Store::open_in_memory().unwrap();

        let id = store.save_open_question("overdue question", None).unwrap();

        // 手动设置 next_ask_at 为过去
        let past = (chrono::Local::now() - chrono::Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE open_questions SET next_ask_at = ?1 WHERE id = ?2",
            rusqlite::params![past, id],
        ).unwrap();
        drop(conn);

        let due = store.get_due_questions(10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, id);
    }

    #[test]
    fn test_answer_question() {
        let store = Store::open_in_memory().unwrap();

        let id = store.save_open_question("will be answered", None).unwrap();
        store.answer_question(id).unwrap();

        // 已回答的不应出现在搜索中（status != 'open'）
        let results = store.search_open_questions("answered").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_bump_question_ask_increments() {
        let store = Store::open_in_memory().unwrap();

        let id = store.save_open_question("bump test", None).unwrap();

        // bump 增加 ask_count
        store.bump_question_ask(id).unwrap();

        let conn = store.conn.lock().unwrap();
        let (count, status): (i32, String) = conn.query_row(
            "SELECT ask_count, status FROM open_questions WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        drop(conn);
        assert_eq!(count, 2);
        assert_eq!(status, "open");
    }

    #[test]
    fn test_bump_question_archives_after_max() {
        let store = Store::open_in_memory().unwrap();

        let id = store.save_open_question("will archive", None).unwrap();

        // 手动设置 ask_count = 3（再 bump 一次就是 4，应该归档）
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE open_questions SET ask_count = 3 WHERE id = ?1",
            rusqlite::params![id],
        ).unwrap();
        drop(conn);

        store.bump_question_ask(id).unwrap();

        let conn = store.conn.lock().unwrap();
        let status: String = conn.query_row(
            "SELECT status FROM open_questions WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(status, "archived");
    }

    #[test]
    fn test_due_questions_excludes_answered_and_archived() {
        let store = Store::open_in_memory().unwrap();

        let id1 = store.save_open_question("answered q", None).unwrap();
        let id2 = store.save_open_question("archived q", None).unwrap();
        let id3 = store.save_open_question("open q", None).unwrap();

        store.answer_question(id1).unwrap();

        // 手动归档 id2
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE open_questions SET status = 'archived' WHERE id = ?1",
            rusqlite::params![id2],
        ).unwrap();
        // 手动设置所有 next_ask_at 为过去
        let past = (chrono::Local::now() - chrono::Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        conn.execute(
            "UPDATE open_questions SET next_ask_at = ?1",
            rusqlite::params![past],
        ).unwrap();
        drop(conn);

        let due = store.get_due_questions(10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, id3);
    }

    #[test]
    fn test_chat_auto_answer_flow() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_open_question("你对团队协作有什么看法？", None).unwrap();

        // 模拟用户在 Chat 中讨论了"团队协作"
        let matches = store.search_open_questions("团队协作").unwrap();
        assert!(!matches.is_empty());
        assert_eq!(matches[0].0, id);

        // 标记为已回答
        store.answer_question(matches[0].0).unwrap();

        // 确认不再出现在搜索结果中（只搜 status='open'）
        let matches_after = store.search_open_questions("团队协作").unwrap();
        assert!(matches_after.is_empty());
    }

    #[test]
    fn test_observer_note_tier_and_ttl() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("observer_note", "邮件频率增加 ← 本周第3封", "observer", 0.6).unwrap();

        // observer_note 应该在 working tier，TTL 3 天
        let conn = store.conn.lock().unwrap();
        let (tier, expires_at): (String, Option<String>) = conn.query_row(
            "SELECT tier, expires_at FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(tier, "working");
        assert!(expires_at.is_some(), "observer_note should have expires_at");
    }

    #[test]
    fn test_load_observer_notes_recent_empty() {
        let store = Store::open_in_memory().unwrap();
        let notes = store.load_observer_notes_recent().unwrap();
        assert!(notes.is_empty());
    }

    #[test]
    fn test_load_observer_notes_recent_returns_today() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("observer_note", "note1 ← 首次出现", "observer", 0.6).unwrap();
        store.save_memory("observer_note", "note2 ← 本周第2次", "observer", 0.6).unwrap();
        // 其他 category 不应返回
        store.save_memory("behavior", "some behavior", "chat", 0.8).unwrap();

        let notes = store.load_observer_notes_recent().unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("note1"));
        assert!(notes[1].contains("note2"));
    }

    // ─── Memory Graph 测试 ──────────────────────────────

    #[test]
    fn test_save_and_get_memory_edge() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "喜欢直接沟通", "chat", 0.7).unwrap();
        let id2 = store.save_memory("values", "重视团队协作", "chat", 0.8).unwrap();

        let edge_id = store.save_memory_edge(id1, id2, "supports", 0.6).unwrap();
        assert!(edge_id > 0);

        let edges = store.get_memory_edges(id1).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_id, id1);
        assert_eq!(edges[0].to_id, id2);
        assert_eq!(edges[0].relation, "supports");
        assert!((edges[0].weight - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_edge_bidirectional_query() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

        store.save_memory_edge(id1, id2, "causes", 0.8).unwrap();

        // 从 id2 方向也能查到
        let edges = store.get_memory_edges(id2).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_id, id1);
    }

    #[test]
    fn test_edge_upsert_updates_weight() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

        store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();
        // 同 pair+relation 再次保存应更新权重
        store.save_memory_edge(id1, id2, "similar", 0.9).unwrap();

        let edges = store.get_memory_edges(id1).unwrap();
        assert_eq!(edges.len(), 1);
        assert!((edges[0].weight - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_edge_different_relations() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

        // 同一对可以有不同关系类型
        store.save_memory_edge(id1, id2, "supports", 0.5).unwrap();
        store.save_memory_edge(id1, id2, "co_occurred", 0.3).unwrap();

        let edges = store.get_memory_edges(id1).unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_get_all_memory_edges() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        let id3 = store.save_memory("values", "C", "chat", 0.8).unwrap();

        store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();
        store.save_memory_edge(id2, id3, "supports", 0.7).unwrap();

        let all_edges = store.get_all_memory_edges().unwrap();
        assert_eq!(all_edges.len(), 2);
    }

    #[test]
    fn test_delete_memory_edge() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

        let edge_id = store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();
        store.delete_memory_edge(edge_id).unwrap();

        let edges = store.get_memory_edges(id1).unwrap();
        assert_eq!(edges.len(), 0);
    }

    #[test]
    fn test_count_memory_edges() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.count_memory_edges().unwrap(), 0);

        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();

        assert_eq!(store.count_memory_edges().unwrap(), 1);
    }

    #[test]
    fn test_connected_memories_traversal() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A - 起点", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B - 一跳", "chat", 0.7).unwrap();
        let id3 = store.save_memory("values", "C - 两跳", "chat", 0.8).unwrap();

        store.save_memory_edge(id1, id2, "supports", 0.8).unwrap();
        store.save_memory_edge(id2, id3, "causes", 0.9).unwrap();

        // 1 跳：只应找到 B
        let hop1 = store.get_connected_memories(id1, 1).unwrap();
        assert_eq!(hop1.len(), 1);
        assert_eq!(hop1[0].0.id, id2);

        // 2 跳：应找到 B 和 C
        let hop2 = store.get_connected_memories(id1, 2).unwrap();
        assert_eq!(hop2.len(), 2);
        // B 的激活度 > C 的激活度（距离更近）
        assert!(hop2[0].1 > hop2[1].1);
    }

    #[test]
    fn test_connected_memories_empty() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "孤立节点", "chat", 0.7).unwrap();

        let result = store.get_connected_memories(id1, 3).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_coach_reads_observer_notes_over_raw_obs() {
        // 验证 load_observer_notes_recent 返回的数据可被 Coach 使用
        let store = Store::open_in_memory().unwrap();
        // 模拟 Observer 已存入标注
        store.save_memory("observer_note", "Morning Brief ← 今天第2次触发", "observer", 0.6).unwrap();
        // 同时有 raw observations
        store.record_observation("scheduled", "Morning Brief", None).unwrap();

        let notes = store.load_observer_notes_recent().unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("今天第2次"));

        // Coach 应该优先使用 observer_notes
        let raw = store.load_unprocessed_observations(50).unwrap();
        assert_eq!(raw.len(), 1); // raw 仍在，待 Coach 归档
    }

    // ─── Browser Bridge 测试 ──────────────────────────────

    #[test]
    fn test_save_browser_behavior() {
        let store = Store::open_in_memory().unwrap();
        store.save_browser_behavior("chatgpt", "conversation_start", r#"{"topic":"rust"}"#).unwrap();
        store.save_browser_behavior("claude", "memory_created", r#"{"count":3}"#).unwrap();
        let rows = store.get_browser_behaviors(10).unwrap();
        assert_eq!(rows.len(), 2);
        // 最新在前
        assert_eq!(rows[0].source, "claude");
        assert_eq!(rows[1].source, "chatgpt");
    }

    #[test]
    fn test_count_memories() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("identity", "test memory", "chat", 0.9).unwrap();
        store.save_memory("values", "another one", "import", 0.8).unwrap();
        let count = store.count_memories().unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_browser_behavior_empty() {
        let store = Store::open_in_memory().unwrap();
        let rows = store.get_browser_behaviors(10).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_save_memory_with_browser_source() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("behavior", "uses chatgpt for brainstorming", "browser:chatgpt", 0.7).unwrap();
        assert!(id > 0);
        let mems = store.search_memories("brainstorming", 10).unwrap();
        assert_eq!(mems.len(), 1);
        assert_eq!(mems[0].source, "browser:chatgpt");
    }

    // ─── Tag 相关测试 ──────────────────────────────

    #[test]
    fn test_add_and_get_tags() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("behavior", "早起跑步", "chat", 0.8).unwrap();

        store.add_tag(id, "health").unwrap();
        store.add_tag(id, "Routine").unwrap(); // 大写应被规范化

        let tags = store.get_tags(id).unwrap();
        assert_eq!(tags, vec!["health", "routine"]); // 按字母排序，小写
    }

    #[test]
    fn test_add_tag_idempotent() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("values", "注重团队", "chat", 0.9).unwrap();

        store.add_tag(id, "leadership").unwrap();
        store.add_tag(id, "leadership").unwrap(); // 重复添加应无报错
        store.add_tag(id, " Leadership ").unwrap(); // trim + lowercase 后重复

        let tags = store.get_tags(id).unwrap();
        assert_eq!(tags.len(), 1);
    }

    #[test]
    fn test_add_tags_batch() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("thinking", "系统思维", "chat", 0.7).unwrap();

        store.add_tags(id, &["work", "cognition", ""]).unwrap(); // 空标签应被跳过

        let tags = store.get_tags(id).unwrap();
        assert_eq!(tags, vec!["cognition", "work"]);
    }

    #[test]
    fn test_remove_tag() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("emotion", "对加班敏感", "chat", 0.6).unwrap();

        store.add_tags(id, &["stress", "work"]).unwrap();
        store.remove_tag(id, "stress").unwrap();

        let tags = store.get_tags(id).unwrap();
        assert_eq!(tags, vec!["work"]);
    }

    #[test]
    fn test_get_all_tags_counts() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "跑步", "chat", 0.8).unwrap();
        let id2 = store.save_memory("behavior", "冥想", "chat", 0.7).unwrap();
        let id3 = store.save_memory("values", "健康优先", "chat", 0.9).unwrap();

        store.add_tag(id1, "health").unwrap();
        store.add_tag(id2, "health").unwrap();
        store.add_tag(id3, "health").unwrap();
        store.add_tag(id1, "morning").unwrap();
        store.add_tag(id2, "mindfulness").unwrap();

        let all = store.get_all_tags().unwrap();
        assert_eq!(all[0], ("health".to_string(), 3)); // 最多的排第一
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_get_memories_by_tag() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "写日记", "chat", 0.8).unwrap();
        let id2 = store.save_memory("growth", "学 Rust", "chat", 0.9).unwrap();

        store.add_tag(id1, "daily").unwrap();
        store.add_tag(id2, "daily").unwrap();
        store.add_tag(id2, "coding").unwrap();

        let daily = store.get_memories_by_tag("daily").unwrap();
        assert_eq!(daily.len(), 2);
        assert!(daily.contains(&id1));
        assert!(daily.contains(&id2));

        let coding = store.get_memories_by_tag("coding").unwrap();
        assert_eq!(coding, vec![id2]);
    }

    #[test]
    fn test_tags_cascade_on_memory_delete() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("task", "买菜", "chat", 1.0).unwrap();
        store.add_tags(id, &["errand", "daily"]).unwrap();

        store.delete_memory(id).unwrap();

        // 标签应随记忆删除级联清除
        let tags = store.get_tags(id).unwrap();
        assert!(tags.is_empty());
    }

    // ─── Edge Cascade + 图谱鲁棒性测试 ──────────────────────────────

    #[test]
    fn test_edges_cascade_on_memory_delete() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        let id3 = store.save_memory("values", "C", "chat", 0.8).unwrap();

        store.save_memory_edge(id1, id2, "supports", 0.8).unwrap();
        store.save_memory_edge(id2, id3, "causes", 0.9).unwrap();
        assert_eq!(store.count_memory_edges().unwrap(), 2);

        // 删除 id2：应级联删除涉及 id2 的两条边
        store.delete_memory(id2).unwrap();
        assert_eq!(store.count_memory_edges().unwrap(), 0);
    }

    #[test]
    fn test_connected_memories_with_cycle() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.8).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.8).unwrap();
        let c = store.save_memory("behavior", "C", "chat", 0.8).unwrap();

        // 创建环：A → B → C → A
        store.save_memory_edge(a, b, "causes", 0.9).unwrap();
        store.save_memory_edge(b, c, "supports", 0.8).unwrap();
        store.save_memory_edge(c, a, "derived_from", 0.7).unwrap();

        // BFS 应正常返回，不死循环
        let result = store.get_connected_memories(a, 5).unwrap();
        assert_eq!(result.len(), 2); // B 和 C
    }

    #[test]
    fn test_edge_reverse_direction_separate() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

        // A→B 和 B→A 同关系类型是不同的边（UNIQUE 约束是 from_id+to_id+relation）
        store.save_memory_edge(id1, id2, "supports", 0.5).unwrap();
        store.save_memory_edge(id2, id1, "supports", 0.8).unwrap();

        let all = store.get_all_memory_edges().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_connected_memories_activation_decay() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "起点", "chat", 0.9).unwrap();
        let b = store.save_memory("behavior", "一跳", "chat", 0.8).unwrap();
        let c = store.save_memory("values", "两跳", "chat", 0.7).unwrap();
        let d = store.save_memory("values", "三跳", "chat", 0.6).unwrap();

        store.save_memory_edge(a, b, "supports", 1.0).unwrap();
        store.save_memory_edge(b, c, "causes", 1.0).unwrap();
        store.save_memory_edge(c, d, "supports", 1.0).unwrap();

        let result = store.get_connected_memories(a, 3).unwrap();
        // 衰减因子 0.7: B=0.7, C=0.49, D=0.343
        // D 的激活度 0.343 > 阈值 0.1，应该存在
        assert_eq!(result.len(), 3);
        // 按激活度降序：B > C > D
        assert_eq!(result[0].0.id, b);
        assert_eq!(result[1].0.id, c);
        assert_eq!(result[2].0.id, d);
        // 验证衰减值
        assert!((result[0].1 - 0.7).abs() < 0.01);
        assert!((result[1].1 - 0.49).abs() < 0.01);
        assert!((result[2].1 - 0.343).abs() < 0.01);
    }

    // ─── search_memories_with_graph 测试 ──────────────────────────────

    #[test]
    fn test_search_with_graph_returns_neighbors() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "每天早起跑步", "chat", 0.8).unwrap();
        let b = store.save_memory("values", "重视健康", "chat", 0.7).unwrap();
        // B 通过边连接 A，但 B 的 content 不包含"跑步"
        store.save_memory_edge(a, b, "supports", 0.9).unwrap();

        let results = store.search_memories_with_graph("跑步", 5, 10).unwrap();
        let ids: Vec<i64> = results.iter().map(|m| m.id).collect();
        assert!(ids.contains(&a), "seed should be found");
        assert!(ids.contains(&b), "graph neighbor should be included");
    }

    #[test]
    fn test_search_with_graph_deduplicates() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "喜欢写代码", "chat", 0.9).unwrap();
        let b = store.save_memory("behavior", "写代码很快乐", "chat", 0.8).unwrap();
        // B 同时是 FTS 结果和图谱邻居
        store.save_memory_edge(a, b, "similar", 0.8).unwrap();

        let results = store.search_memories_with_graph("代码", 5, 10).unwrap();
        let b_count = results.iter().filter(|m| m.id == b).count();
        assert_eq!(b_count, 1, "should deduplicate");
    }

    #[test]
    fn test_search_with_graph_respects_total_limit() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..10 {
            store.save_memory("behavior", &format!("测试记忆{i}"), "chat", 0.5 + i as f64 * 0.05).unwrap();
        }
        let results = store.search_memories_with_graph("测试", 10, 3).unwrap();
        assert_eq!(results.len(), 3, "should respect total_limit");
    }

    // ─── strengthen_edges 测试 ──────────────────────────────

    #[test]
    fn test_strengthen_creates_new_edge() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        assert_eq!(store.count_memory_edges().unwrap(), 0);

        let n = store.strengthen_edges(&[a, b]).unwrap();
        assert_eq!(n, 1);

        let edges = store.get_memory_edges(a).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].relation, "co_occurred");
        assert!((edges[0].weight - 0.3).abs() < 0.01);
    }

    #[test]
    fn test_strengthen_boosts_existing_edge() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory_edge(a, b, "supports", 0.5).unwrap();

        store.strengthen_edges(&[a, b]).unwrap();

        let edges = store.get_memory_edges(a).unwrap();
        assert_eq!(edges.len(), 1);
        assert!((edges[0].weight - 0.55).abs() < 0.01, "should be 0.5 + 0.05");
    }

    #[test]
    fn test_strengthen_caps_at_1() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory_edge(a, b, "causes", 0.98).unwrap();

        store.strengthen_edges(&[a, b]).unwrap();

        let edges = store.get_memory_edges(a).unwrap();
        assert!((edges[0].weight - 1.0).abs() < 0.01, "should cap at 1.0");
    }

    #[test]
    fn test_strengthen_single_id_noop() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let n = store.strengthen_edges(&[a]).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn test_strengthen_multiple_pairs() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        let c = store.save_memory("values", "C", "chat", 0.8).unwrap();

        // 3 条记忆 → C(3,2) = 3 对
        let n = store.strengthen_edges(&[a, b, c]).unwrap();
        assert_eq!(n, 3);
        assert_eq!(store.count_memory_edges().unwrap(), 3);
    }

    // ─── decay_cold_edges 测试 ──────────────────────────────

    #[test]
    fn test_decay_cold_edges_decreases_weight() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory_edge(a, b, "supports", 0.8).unwrap();
        // last_activated_at 默认 NULL → 被视为冷边

        store.decay_cold_edges(0, 0.5, 0.1).unwrap(); // cold_days=0 → 所有边都衰减

        let edges = store.get_memory_edges(a).unwrap();
        assert_eq!(edges.len(), 1);
        assert!((edges[0].weight - 0.4).abs() < 0.01, "0.8 * 0.5 = 0.4");
    }

    #[test]
    fn test_decay_cold_edges_deletes_below_threshold() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        store.save_memory_edge(a, b, "similar", 0.15).unwrap();

        store.decay_cold_edges(0, 0.5, 0.1).unwrap(); // 0.15 * 0.5 = 0.075 < 0.1

        assert_eq!(store.count_memory_edges().unwrap(), 0, "should delete edge below threshold");
    }

    #[test]
    fn test_decay_skips_recently_activated() {
        let store = Store::open_in_memory().unwrap();
        let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
        let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
        // strengthen_edges 设置 last_activated_at 为 now
        store.strengthen_edges(&[a, b]).unwrap();

        // cold_days=30 → 刚激活的边不应衰减
        store.decay_cold_edges(30, 0.5, 0.1).unwrap();

        let edges = store.get_memory_edges(a).unwrap();
        assert!((edges[0].weight - 0.3).abs() < 0.01, "recently activated edge should not decay");
    }

    // ── Knowledge Edges 测试 ──

    #[test]
    fn test_save_and_get_knowledge_edge() {
        let store = Store::open_in_memory().unwrap();
        let mem_id = store.save_memory("behavior", "likes running", "chat", 0.7).unwrap();

        // memory → message 的连接（message id 用 999 模拟，不需要真实存在）
        let edge_id = store.save_knowledge_edge("memory", mem_id, "message", 999, "references", 0.8).unwrap();
        assert!(edge_id > 0);

        let edges = store.get_knowledge_edges("memory", mem_id).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_type, "memory");
        assert_eq!(edges[0].to_type, "message");
        assert_eq!(edges[0].relation, "references");
    }

    #[test]
    fn test_knowledge_edge_upsert() {
        let store = Store::open_in_memory().unwrap();
        store.save_knowledge_edge("memory", 1, "message", 2, "references", 0.5).unwrap();
        store.save_knowledge_edge("memory", 1, "message", 2, "references", 0.9).unwrap();

        let edges = store.get_knowledge_edges("memory", 1).unwrap();
        assert_eq!(edges.len(), 1);
        assert!((edges[0].weight - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_knowledge_edge_different_relations() {
        let store = Store::open_in_memory().unwrap();
        store.save_knowledge_edge("memory", 1, "message", 2, "references", 0.5).unwrap();
        store.save_knowledge_edge("memory", 1, "message", 2, "triggers", 0.7).unwrap();

        let edges = store.get_knowledge_edges("memory", 1).unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_knowledge_edge_bidirectional_query() {
        let store = Store::open_in_memory().unwrap();
        store.save_knowledge_edge("memory", 1, "message", 2, "references", 0.8).unwrap();

        // 从 message 侧也能查到
        let edges = store.get_knowledge_edges("message", 2).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_type, "memory");
    }

    #[test]
    fn test_get_knowledge_edges_between_types() {
        let store = Store::open_in_memory().unwrap();
        store.save_knowledge_edge("memory", 1, "message", 10, "references", 0.8).unwrap();
        store.save_knowledge_edge("memory", 2, "message", 11, "triggers", 0.6).unwrap();
        store.save_knowledge_edge("memory", 3, "observation", 20, "supports", 0.7).unwrap();

        let mem_msg = store.get_knowledge_edges_between_types("memory", "message", 10).unwrap();
        assert_eq!(mem_msg.len(), 2);

        let mem_obs = store.get_knowledge_edges_between_types("memory", "observation", 10).unwrap();
        assert_eq!(mem_obs.len(), 1);
    }

    #[test]
    fn test_get_all_knowledge_edges() {
        let store = Store::open_in_memory().unwrap();
        store.save_knowledge_edge("memory", 1, "message", 2, "references", 0.8).unwrap();
        store.save_knowledge_edge("memory", 3, "question", 4, "answers", 0.9).unwrap();

        let all = store.get_all_knowledge_edges().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_delete_knowledge_edge() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_knowledge_edge("memory", 1, "message", 2, "references", 0.5).unwrap();
        store.delete_knowledge_edge(id).unwrap();

        assert_eq!(store.count_knowledge_edges().unwrap(), 0);
    }

    #[test]
    fn test_count_knowledge_edges() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.count_knowledge_edges().unwrap(), 0);

        store.save_knowledge_edge("memory", 1, "message", 2, "references", 0.5).unwrap();
        store.save_knowledge_edge("observation", 3, "memory", 4, "triggers", 0.7).unwrap();
        assert_eq!(store.count_knowledge_edges().unwrap(), 2);
    }

    #[test]
    fn test_knowledge_edge_cross_type_graph() {
        let store = Store::open_in_memory().unwrap();
        // 构建：memory1 → message1 → memory2 的路径
        store.save_knowledge_edge("memory", 1, "message", 10, "references", 0.9).unwrap();
        store.save_knowledge_edge("message", 10, "memory", 2, "triggers", 0.8).unwrap();

        // 从 message 10 查询应能看到两条边
        let edges = store.get_knowledge_edges("message", 10).unwrap();
        assert_eq!(edges.len(), 2);
    }

    // ─── Message 测试 ──────────────────────────────

    #[test]
    fn test_save_and_get_message() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_message(
            "Alice", "#general", Some("Hello world"), "teams", "text", "2026-03-12T10:00:00",
        ).unwrap();
        assert!(id > 0);

        let msgs = store.get_messages_by_channel("#general", 10).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].sender, "Alice");
        assert_eq!(msgs[0].content, Some("Hello world".to_string()));
        assert_eq!(msgs[0].source, "teams");
        assert_eq!(msgs[0].message_type, "text");
    }

    #[test]
    fn test_save_message_without_content() {
        let store = Store::open_in_memory().unwrap();
        store.save_message(
            "Bob", "#dev", None, "teams", "file", "2026-03-12T11:00:00",
        ).unwrap();

        let msgs = store.get_messages_by_channel("#dev", 10).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].content.is_none());
        assert_eq!(msgs[0].message_type, "file");
    }

    #[test]
    fn test_get_messages_by_source() {
        let store = Store::open_in_memory().unwrap();
        store.save_message("A", "#ch1", Some("hi"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.save_message("B", "inbox", Some("email"), "email", "text", "2026-03-12T10:01:00").unwrap();
        store.save_message("C", "#ch2", Some("yo"), "teams", "text", "2026-03-12T10:02:00").unwrap();

        let teams = store.get_messages_by_source("teams", 10).unwrap();
        assert_eq!(teams.len(), 2);

        let email = store.get_messages_by_source("email", 10).unwrap();
        assert_eq!(email.len(), 1);
        assert_eq!(email[0].sender, "B");
    }

    #[test]
    fn test_search_messages() {
        let store = Store::open_in_memory().unwrap();
        store.save_message("Alice", "#general", Some("meeting with Bob"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.save_message("Carol", "#dev", Some("code review done"), "teams", "text", "2026-03-12T10:01:00").unwrap();

        let found = store.search_messages("meeting", 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].sender, "Alice");

        // 搜索 sender
        let found_sender = store.search_messages("Carol", 10).unwrap();
        assert_eq!(found_sender.len(), 1);
    }

    #[test]
    fn test_count_messages() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.count_messages().unwrap(), 0);

        store.save_message("A", "#ch", Some("x"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.save_message("B", "#ch", Some("y"), "slack", "text", "2026-03-12T10:01:00").unwrap();
        assert_eq!(store.count_messages().unwrap(), 2);
    }

    #[test]
    fn test_delete_message() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_message("A", "#ch", Some("tmp"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.delete_message(id).unwrap();
        assert_eq!(store.count_messages().unwrap(), 0);
    }

    #[test]
    fn test_today_message_stats() {
        let store = Store::open_in_memory().unwrap();
        store.save_message("A", "#ch1", Some("a"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.save_message("B", "#ch2", Some("b"), "teams", "text", "2026-03-12T10:01:00").unwrap();
        store.save_message("C", "inbox", Some("c"), "email", "text", "2026-03-12T10:02:00").unwrap();

        let stats = store.get_today_message_stats().unwrap();
        // 应有 teams 和 email 两组
        assert!(stats.len() >= 2);
        // teams 数量最多
        assert_eq!(stats[0].0, "teams");
        assert_eq!(stats[0].1, 2);
    }

    #[test]
    fn test_get_message_channels() {
        let store = Store::open_in_memory().unwrap();
        store.save_message("A", "#general", Some("hi"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.save_message("B", "#general", Some("hey"), "teams", "text", "2026-03-12T10:01:00").unwrap();
        store.save_message("C", "#dev", Some("PR"), "teams", "text", "2026-03-12T10:02:00").unwrap();
        store.save_message("D", "inbox", Some("email"), "email", "text", "2026-03-12T10:03:00").unwrap();

        let channels = store.get_message_channels().unwrap();
        assert_eq!(channels.len(), 3);
        // #general has 2 messages — should be first (ordered by count DESC)
        assert_eq!(channels[0].0, "#general");
        assert_eq!(channels[0].1, "teams");
        assert_eq!(channels[0].2, 2);
    }

    #[test]
    fn test_save_message_dedup_by_timestamp() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_message("Alice", "#general", Some("hello"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        // 相同 sender+channel+source+timestamp → 应被忽略
        let id2 = store.save_message("Alice", "#general", Some("hello"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        // 不同时间戳 → 应正常插入
        let id3 = store.save_message("Alice", "#general", Some("world"), "teams", "text", "2026-03-12T10:01:00").unwrap();

        assert!(id1 > 0);
        // INSERT OR IGNORE 返回 last_insert_rowid 但实际没插入
        assert_eq!(store.count_messages().unwrap(), 2);
        assert_ne!(id1, id3);
        let _ = id2; // 静默使用
    }

    // ─── Visibility 测试 ──────────────────────────────

    #[test]
    fn test_save_memory_default_visibility_is_public() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory("identity", "test content", "chat", 0.8).unwrap();
        let mems = store.load_memories().unwrap();
        assert_eq!(mems.len(), 1);
        assert_eq!(mems[0].visibility, "public");
    }

    #[test]
    fn test_save_memory_with_visibility() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory_with_visibility("emotion", "feeling tired", "chat", 0.7, "private").unwrap();
        assert!(id > 0);
        let mems = store.load_memories().unwrap();
        assert_eq!(mems[0].visibility, "private");
    }

    #[test]
    fn test_search_public_memories_filters_correctly() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory_with_visibility("identity", "public fact", "onboarding", 0.9, "public").unwrap();
        store.save_memory_with_visibility("emotion", "private feeling", "chat", 0.8, "private").unwrap();
        store.save_memory_with_visibility("coach_insight", "subconscious pattern", "coach", 0.7, "subconscious").unwrap();

        let public = store.search_public_memories("", 100).unwrap();
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].content, "public fact");

        let public_search = store.search_public_memories("fact", 100).unwrap();
        assert_eq!(public_search.len(), 1);

        // private 和 subconscious 不会出现在 public 搜索中
        let no_match = store.search_public_memories("feeling", 100).unwrap();
        assert!(no_match.is_empty());
    }

    #[test]
    fn test_get_memories_by_visibility() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory_with_visibility("identity", "a", "chat", 0.9, "public").unwrap();
        store.save_memory_with_visibility("emotion", "b", "chat", 0.8, "private").unwrap();
        store.save_memory_with_visibility("emotion", "c", "chat", 0.7, "private").unwrap();
        store.save_memory_with_visibility("pattern", "d", "coach", 0.6, "subconscious").unwrap();

        assert_eq!(store.get_memories_by_visibility("public").unwrap().len(), 1);
        assert_eq!(store.get_memories_by_visibility("private").unwrap().len(), 2);
        assert_eq!(store.get_memories_by_visibility("subconscious").unwrap().len(), 1);
    }

    #[test]
    fn test_update_memory_visibility() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_memory("identity", "test", "chat", 0.8).unwrap();
        assert_eq!(store.load_memories().unwrap()[0].visibility, "public");

        store.update_memory_visibility(id, "private").unwrap();
        assert_eq!(store.load_memories().unwrap()[0].visibility, "private");

        store.update_memory_visibility(id, "subconscious").unwrap();
        assert_eq!(store.load_memories().unwrap()[0].visibility, "subconscious");
    }

    #[test]
    fn test_count_memories_by_visibility() {
        let store = Store::open_in_memory().unwrap();
        store.save_memory_with_visibility("a", "x", "s", 0.9, "public").unwrap();
        store.save_memory_with_visibility("b", "y", "s", 0.8, "public").unwrap();
        store.save_memory_with_visibility("c", "z", "s", 0.7, "private").unwrap();
        store.save_memory_with_visibility("d", "w", "s", 0.6, "subconscious").unwrap();

        let counts = store.count_memories_by_visibility().unwrap();
        // counts 按 COUNT DESC 排序，public=2 应排第一
        assert_eq!(counts[0], ("public".to_string(), 2));
        assert!(counts.iter().any(|(v, c)| v == "private" && *c == 1));
        assert!(counts.iter().any(|(v, c)| v == "subconscious" && *c == 1));
    }

    #[test]
    fn test_migration_backfill_visibility() {
        let store = Store::open_in_memory().unwrap();
        // chat source → private (by v16 backfill for new DBs, save_memory default is public)
        // 但 open_in_memory 跑完迁移时表是空的，backfill 不会影响后续插入
        // 验证新插入 + search 的 visibility 字段正确传递
        store.save_memory_with_visibility("coach_insight", "pattern X", "coach", 0.8, "subconscious").unwrap();
        let search = store.search_memories("pattern", 10).unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].visibility, "subconscious");
    }

    #[test]
    fn test_graph_search_preserves_visibility() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save_memory_with_visibility("identity", "node A", "chat", 0.9, "private").unwrap();
        let id2 = store.save_memory_with_visibility("identity", "node B", "onboarding", 0.8, "public").unwrap();
        store.save_memory_edge(id1, id2, "similar", 0.9).unwrap();

        let results = store.search_memories_with_graph("node", 10, 20).unwrap();
        assert!(results.len() >= 2);
        // 验证 visibility 字段在图谱搜索结果中保留
        for m in &results {
            assert!(m.visibility == "private" || m.visibility == "public");
        }
    }
}
