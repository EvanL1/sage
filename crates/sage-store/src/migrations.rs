use anyhow::{Context, Result};

use super::Store;

impl Store {
    /// 用 user_version pragma 做增量迁移
    pub(super) fn migrate(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
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

        if version < 18 {
            conn.execute_batch(
                "ALTER TABLE memories ADD COLUMN about_person TEXT DEFAULT NULL;
                 CREATE INDEX IF NOT EXISTS idx_memories_about_person ON memories(about_person);
                 PRAGMA user_version = 18;",
            )
            .context("数据库迁移 v18（memories.about_person）失败")?;
        }

        if version < 19 {
            conn.execute_batch(
                "ALTER TABLE memories ADD COLUMN last_accessed_at TEXT DEFAULT NULL;
                 CREATE INDEX IF NOT EXISTS idx_memories_last_accessed ON memories(last_accessed_at);
                 PRAGMA user_version = 19;",
            )
            .context("数据库迁移 v19（memories.last_accessed_at）失败")?;
        }

        if version < 20 {
            conn.execute_batch(
                "ALTER TABLE messages ADD COLUMN direction TEXT NOT NULL DEFAULT 'received';
                 CREATE INDEX IF NOT EXISTS idx_messages_direction ON messages(direction);
                 PRAGMA user_version = 20;",
            )
            .context("数据库迁移 v20（messages.direction 收/发标记）失败")?;
        }

        if version < 21 {
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS report_corrections (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    report_type TEXT NOT NULL,
                    wrong_claim TEXT NOT NULL,
                    correct_fact TEXT NOT NULL,
                    context_hint TEXT DEFAULT '',
                    confidence REAL NOT NULL DEFAULT 0.6,
                    applied_count INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    superseded_at TEXT DEFAULT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_corrections_type
                    ON report_corrections(report_type, superseded_at, created_at DESC);
                PRAGMA user_version = 21;
            ",
            )?;
        }

        if version < 22 {
            conn.execute_batch("
                CREATE TABLE IF NOT EXISTS tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    content TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'open',
                    priority TEXT NOT NULL DEFAULT 'normal',
                    due_date TEXT DEFAULT NULL,
                    source TEXT NOT NULL DEFAULT 'manual',
                    source_id INTEGER DEFAULT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status, due_date, created_at DESC);
                PRAGMA user_version = 22;
            ")?;
        }

        // v23: tasks 表补列（priority + due_date）— 修复 v22 先跑无这两列的情况
        if version < 23 {
            // 检查 priority 列是否存在
            let has_priority: bool = conn.prepare("SELECT priority FROM tasks LIMIT 0").is_ok();
            if !has_priority {
                conn.execute_batch(
                    "
                    ALTER TABLE tasks ADD COLUMN priority TEXT NOT NULL DEFAULT 'normal';
                    ALTER TABLE tasks ADD COLUMN due_date TEXT DEFAULT NULL;
                ",
                )?;
            }
            conn.execute_batch(
                "
                DROP INDEX IF EXISTS idx_tasks_status;
                CREATE INDEX idx_tasks_status ON tasks(status, due_date, created_at DESC);
                PRAGMA user_version = 23;
            ",
            )?;
        }

        // v24: tasks.outcome — 完成/取消时记录结果说明
        if version < 24 {
            conn.execute_batch(
                "
                ALTER TABLE tasks ADD COLUMN outcome TEXT DEFAULT NULL;
                PRAGMA user_version = 24;
            ",
            )
            .context("数据库迁移 v24（tasks.outcome）失败")?;
        }

        // v25: tasks.verification — LLM 生成的验收标准 JSON
        if version < 25 {
            conn.execute_batch(
                "
                ALTER TABLE tasks ADD COLUMN verification TEXT DEFAULT NULL;
                PRAGMA user_version = 25;
            ",
            )
            .context("数据库迁移 v25（tasks.verification）失败")?;
        }

        // v26: task_signals — 任务智能信号（完成/取消/新增任务建议）
        if version < 26 {
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS task_signals (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    signal_type TEXT NOT NULL,
                    task_id INTEGER,
                    title TEXT NOT NULL,
                    evidence TEXT NOT NULL,
                    suggested_outcome TEXT,
                    status TEXT NOT NULL DEFAULT 'pending',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_task_signals_status
                    ON task_signals(status, created_at DESC);
                PRAGMA user_version = 26;
            ",
            )
            .context("数据库迁移 v26（task_signals 任务信号表）失败")?;
        }

        // v27: tasks.description — 任务描述字段
        if version < 27 {
            conn.execute_batch(
                "
                ALTER TABLE tasks ADD COLUMN description TEXT DEFAULT NULL;
                PRAGMA user_version = 27;
            ",
            )
            .context("数据库迁移 v27（tasks.description）失败")?;
        }

        // v28: task_signals.importance + kv_store
        if version < 28 {
            conn.execute_batch(
                "
                ALTER TABLE task_signals ADD COLUMN importance REAL NOT NULL DEFAULT 0.5;
                CREATE INDEX IF NOT EXISTS idx_task_signals_importance
                    ON task_signals(importance, status);
                CREATE TABLE IF NOT EXISTS kv_store (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                PRAGMA user_version = 28;
            ",
            )
            .context("数据库迁移 v28（task_signals.importance + kv_store）失败")?;
        }

        // v29: memories 认知深度系统
        if version < 29 {
            conn.execute_batch("
                ALTER TABLE memories ADD COLUMN depth TEXT NOT NULL DEFAULT 'episodic';
                ALTER TABLE memories ADD COLUMN valid_until TEXT DEFAULT NULL;
                ALTER TABLE memories ADD COLUMN validation_count INTEGER NOT NULL DEFAULT 0;
                CREATE INDEX IF NOT EXISTS idx_memories_depth ON memories(depth);
                CREATE INDEX IF NOT EXISTS idx_memories_valid_until ON memories(valid_until);
                -- 所有记忆默认 episodic，只有 evolution 产出的标记为 procedural
                UPDATE memories SET depth = 'procedural' WHERE source = 'evolution';
                PRAGMA user_version = 29;
            ").context("数据库迁移 v29（memories 认知深度）失败")?;
        }

        // v30: memories embedding 列（Phase 1b 语义向量搜索）
        // 注：v5 已添加 embedding BLOB，此处为版本号对齐
        if version < 30 {
            conn.execute_batch(
                "PRAGMA user_version = 30;",
            )
            .context("数据库迁移 v30（embedding 版本对齐）失败")?;
        }

        // v31: Mirror Layer — reflective_signals 反思信号表
        if version < 31 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS reflective_signals (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    source TEXT NOT NULL,
                    signal_type TEXT NOT NULL,
                    raw_text TEXT NOT NULL,
                    context TEXT,
                    baseline_divergence REAL NOT NULL DEFAULT 0.0,
                    armor_pattern TEXT,
                    intensity REAL NOT NULL DEFAULT 0.5,
                    resolved INTEGER NOT NULL DEFAULT 0,
                    resolution_text TEXT,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_reflective_type_resolved
                    ON reflective_signals(signal_type, resolved);
                CREATE INDEX IF NOT EXISTS idx_reflective_created
                    ON reflective_signals(created_at DESC);
                PRAGMA user_version = 31;",
            )
            .context("数据库迁移 v31（reflective_signals 反思信号）失败")?;
        }

        // v32: Custom Pages — 用户自定义动态页面
        if version < 32 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS custom_pages (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    title       TEXT NOT NULL,
                    markdown    TEXT NOT NULL,
                    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_custom_pages_created ON custom_pages(created_at DESC);
                PRAGMA user_version = 32;",
            )
            .context("数据库迁移 v32（custom_pages 自定义页面）失败")?;
        }

        // v33: message_sources 通用消息源 + emails 缓存
        if version < 33 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS message_sources (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    label       TEXT NOT NULL,
                    source_type TEXT NOT NULL,
                    config      TEXT NOT NULL DEFAULT '{}',
                    enabled     INTEGER NOT NULL DEFAULT 1,
                    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS emails (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    source_id   INTEGER NOT NULL REFERENCES message_sources(id) ON DELETE CASCADE,
                    uid         TEXT NOT NULL,
                    folder      TEXT NOT NULL DEFAULT 'INBOX',
                    from_addr   TEXT NOT NULL DEFAULT '',
                    to_addr     TEXT NOT NULL DEFAULT '',
                    subject     TEXT NOT NULL DEFAULT '',
                    body_text   TEXT NOT NULL DEFAULT '',
                    body_html   TEXT DEFAULT NULL,
                    is_read     INTEGER NOT NULL DEFAULT 0,
                    date        TEXT NOT NULL DEFAULT '',
                    fetched_at  TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_emails_uid
                    ON emails(source_id, uid, folder);
                CREATE INDEX IF NOT EXISTS idx_emails_source_date
                    ON emails(source_id, date DESC);
                PRAGMA user_version = 33;",
            )
            .context("数据库迁移 v33（message_sources + emails 表）失败")?;
        }

        // v34: emails.dismissed — soft delete for Outlook re-fetch protection
        if version < 34 {
            conn.execute_batch(
                "ALTER TABLE emails ADD COLUMN dismissed INTEGER NOT NULL DEFAULT 0;
                 PRAGMA user_version = 34;",
            )
            .context("数据库迁移 v34（emails.dismissed）失败")?;
        }

        // v35: messages.action_state + resolved_at — 消息待处理状态追踪
        if version < 35 {
            conn.execute_batch(
                "ALTER TABLE messages ADD COLUMN action_state TEXT NOT NULL DEFAULT 'pending';
                 ALTER TABLE messages ADD COLUMN resolved_at TEXT;
                 CREATE INDEX IF NOT EXISTS idx_messages_action_state ON messages(action_state, timestamp DESC);
                 PRAGMA user_version = 35;",
            )
            .context("数据库迁移 v35（messages.action_state + resolved_at）失败")?;
        }

        if version < 36 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS feed_digests (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    date TEXT NOT NULL UNIQUE,
                    content TEXT NOT NULL,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_feed_digests_date ON feed_digests(date DESC);
                PRAGMA user_version = 36;",
            )
            .context("数据库迁移 v36（feed_digests 表）失败")?;
        }

        if version < 37 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS feed_actions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    observation_id INTEGER NOT NULL UNIQUE,
                    action TEXT NOT NULL DEFAULT 'archived',
                    category TEXT,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_feed_actions_obs ON feed_actions(observation_id);
                CREATE INDEX IF NOT EXISTS idx_feed_actions_action ON feed_actions(action);
                PRAGMA user_version = 37;",
            )
            .context("数据库迁移 v37（feed_actions 表）失败")?;
        }

        if version < 38 {
            conn.execute_batch(
                "ALTER TABLE memories ADD COLUMN derived_from TEXT;
                 ALTER TABLE memories ADD COLUMN evolution_note TEXT;
                 PRAGMA user_version = 38;",
            )
            .context("数据库迁移 v38（memory provenance）失败")?;
        }

        if version < 39 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS pipeline_runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    stage TEXT NOT NULL,
                    pipeline TEXT NOT NULL DEFAULT 'evening',
                    outcome TEXT NOT NULL,
                    elapsed_ms INTEGER,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_pipeline_runs_stage
                    ON pipeline_runs(stage, created_at DESC);
                CREATE TABLE IF NOT EXISTS pipeline_overrides (
                    stage TEXT NOT NULL,
                    key TEXT NOT NULL,
                    value TEXT NOT NULL,
                    reason TEXT,
                    created_at TEXT DEFAULT (datetime('now')),
                    PRIMARY KEY (stage, key)
                );
                PRAGMA user_version = 39;",
            )
            .context("数据库迁移 v39（pipeline self-evolution）失败")?;
        }

        // ── v40: 标准化所有时间戳为 ISO 8601 ──
        if version < 40 {
            let mut total_fixed = 0usize;

            // messages.timestamp
            let mut stmt = conn.prepare(
                "SELECT id, timestamp FROM messages WHERE timestamp NOT LIKE '____-__-__%'"
            )?;
            let rows: Vec<(i64, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            drop(stmt);
            for (id, raw_ts) in &rows {
                let normalized = sage_types::normalize_timestamp(raw_ts);
                if &normalized != raw_ts {
                    conn.execute(
                        "UPDATE messages SET timestamp = ?1 WHERE id = ?2",
                        rusqlite::params![normalized, id],
                    )?;
                    total_fixed += 1;
                }
            }

            // emails.date
            let mut stmt = conn.prepare(
                "SELECT id, date FROM emails WHERE date NOT LIKE '____-__-__%'"
            )?;
            let rows: Vec<(i64, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            drop(stmt);
            for (id, raw_ts) in &rows {
                let normalized = sage_types::normalize_timestamp(raw_ts);
                if &normalized != raw_ts {
                    conn.execute(
                        "UPDATE emails SET date = ?1 WHERE id = ?2",
                        rusqlite::params![normalized, id],
                    )?;
                    total_fixed += 1;
                }
            }

            conn.execute_batch("PRAGMA user_version = 40;")?;
            if total_fixed > 0 {
                tracing::info!("Migration v40: normalized {total_fixed} timestamps (messages + emails)");
            }
        }

        // ── v41: 补充修复 emails.date（v40 可能漏掉）──
        if version < 41 {
            let mut stmt = conn.prepare(
                "SELECT id, date FROM emails WHERE date NOT LIKE '____-__-__%'"
            )?;
            let rows: Vec<(i64, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            drop(stmt);
            let mut fixed = 0usize;
            for (id, raw_ts) in &rows {
                let normalized = sage_types::normalize_timestamp(raw_ts);
                if &normalized != raw_ts {
                    conn.execute(
                        "UPDATE emails SET date = ?1 WHERE id = ?2",
                        rusqlite::params![normalized, id],
                    )?;
                    fixed += 1;
                }
            }
            conn.execute_batch("PRAGMA user_version = 41;")?;
            if fixed > 0 {
                tracing::info!("Migration v41: normalized {fixed} email timestamps");
            }
        }

        // ── v42: 自定义管线阶段 ──
        if version < 42 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS custom_stages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    description TEXT NOT NULL DEFAULT '',
                    prompt TEXT NOT NULL,
                    insert_after TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                PRAGMA user_version = 42;",
            )
            .context("数据库迁移 v42（custom pipeline stages）失败")?;
        }

        // ── v43: custom_stages 增加 output_format 列 ──
        if version < 43 {
            conn.execute_batch(
                "ALTER TABLE custom_stages ADD COLUMN output_format TEXT NOT NULL DEFAULT '';
                 PRAGMA user_version = 43;",
            )
            .context("数据库迁移 v43（custom_stages output_format）失败")?;
        }

        // ── v44: custom_stages 增加 available_actions 列 ──
        if version < 44 {
            conn.execute_batch(
                "ALTER TABLE custom_stages ADD COLUMN available_actions TEXT NOT NULL DEFAULT '';
                 PRAGMA user_version = 44;",
            )
            .context("数据库迁移 v44（custom_stages available_actions）失败")?;
        }

        // ── v45: custom_stages 硬约束字段 ──
        if version < 45 {
            conn.execute_batch(
                "ALTER TABLE custom_stages ADD COLUMN allowed_inputs TEXT NOT NULL DEFAULT 'observer_notes,coach_insights';
                 ALTER TABLE custom_stages ADD COLUMN max_actions INTEGER NOT NULL DEFAULT 5;
                 ALTER TABLE custom_stages ADD COLUMN pre_condition TEXT NOT NULL DEFAULT '';
                 PRAGMA user_version = 45;",
            )
            .context("数据库迁移 v45（hard constraints）失败")?;
        }

        // ── v46: custom_stages 预设支持 + 种子数据 ──
        if version < 46 {
            conn.execute_batch(
                "ALTER TABLE custom_stages ADD COLUMN is_preset INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE custom_stages ADD COLUMN archive_observations INTEGER NOT NULL DEFAULT 0;",
            )
            .context("数据库迁移 v46（preset columns）失败")?;

            crate::pipeline::seed_preset_stages(&conn)?;

            conn.execute_batch("PRAGMA user_version = 46;")?;
        }

        // ── v47: 重新种子预设（新增 mirror_weekly + calibrator 改用 save_calibration_rule）──
        if version < 47 {
            // INSERT OR IGNORE 保证幂等（已有预设不覆盖）
            crate::pipeline::seed_preset_stages(&conn)?;
            conn.execute_batch("PRAGMA user_version = 47;")?;
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
}
