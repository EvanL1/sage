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

    // ─── 自定义管线阶段 ─────────────────────────────

    /// 自定义阶段定义
    pub fn create_custom_stage(
        &self,
        name: &str,
        description: &str,
        prompt: &str,
        insert_after: &str,
        output_format: &str,
        available_actions: &str,
        allowed_inputs: &str,
        max_actions: i32,
        pre_condition: &str,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO custom_stages (name, description, prompt, insert_after, output_format, available_actions, allowed_inputs, max_actions, pre_condition) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![name, description, prompt, insert_after, output_format, available_actions, allowed_inputs, max_actions, pre_condition],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_custom_stages(&self) -> Result<Vec<CustomStage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, description, prompt, insert_after, enabled, output_format, available_actions, allowed_inputs, max_actions, pre_condition, is_preset, archive_observations FROM custom_stages ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CustomStage {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    prompt: row.get(3)?,
                    insert_after: row.get(4)?,
                    enabled: row.get::<_, i32>(5)? != 0,
                    output_format: row.get(6)?,
                    available_actions: row.get(7)?,
                    allowed_inputs: row.get(8)?,
                    max_actions: row.get(9)?,
                    pre_condition: row.get(10)?,
                    is_preset: row.get::<_, i32>(11).unwrap_or(0) != 0,
                    archive_observations: row.get::<_, i32>(12).unwrap_or(0) != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn delete_custom_stage(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        // 预设 stage 不可删除
        let is_preset: i32 = conn.query_row(
            "SELECT is_preset FROM custom_stages WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).unwrap_or(0);
        if is_preset != 0 {
            return Err(anyhow::anyhow!("预设阶段不可删除"));
        }
        conn.execute("DELETE FROM custom_stages WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    /// 执行条件查询（pre_condition hook 用）：SELECT 返回的第一行第一列 > 0 则通过
    pub fn execute_condition_query(&self, sql: &str) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let result: i64 = conn.query_row(sql, [], |row| row.get(0)).unwrap_or(0);
        Ok(result > 0)
    }

    pub fn toggle_custom_stage(&self, id: i64, enabled: bool) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE custom_stages SET enabled = ?1 WHERE id = ?2",
            rusqlite::params![enabled as i32, id],
        )?;
        Ok(())
    }
}

/// 自定义管线阶段
#[derive(Debug, Clone)]
pub struct CustomStage {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub insert_after: String,
    pub enabled: bool,
    pub output_format: String,
    /// 逗号分隔的可用 action 名
    pub available_actions: String,
    /// 逗号分隔的允许输入源
    pub allowed_inputs: String,
    /// 每次执行最多产出的 ACTION 数
    pub max_actions: i32,
    /// 前置条件（SQL 查询，返回 >0 才执行），空=无条件
    pub pre_condition: String,
    /// 是否为内置预设（不可删除）
    pub is_preset: bool,
    /// 执行后归档已消费的 observations
    pub archive_observations: bool,
}

/// 种子 7 个内置预设 stage（migration v46 调用）
pub fn seed_preset_stages(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    let presets: &[(&str, &str, &str, &str, &str, &str, &str, i32, &str, bool)] = &[
        // (name, desc, prompt, insert_after, output_format, actions, inputs, max_actions, pre_cond, archive_obs)
        (
            "observer",
            "观察标注：语义维度分解 + 事件标注",
            concat!(
                "你是 Sage 的观察者。你的工作分两步：先分解维度，再逐事件标注。\n\n",
                "## Step 1: 语义维度分解\n",
                "先浏览全部原始事件，识别 3-5 个「语义维度」——即今天事件中值得关注的主题轴。\n",
                "例如：工作压力、社交模式、健康信号、学习投入、情绪波动。\n",
                "用一行列出维度（不需要 ACTION），格式：DIMENSIONS: 维度1, 维度2, ...\n\n",
                "## Step 2: 逐事件标注\n",
                "对每条事件，描述「发生了什么」并推断「为什么可能发生」——不评价、不建议。\n",
                "标注时确保覆盖 Step 1 中识别的所有维度，不要遗漏任何维度相关的事件。\n\n",
                "## 今日原始事件\n{context}\n\n",
                "## 规则\n",
                "- 从用户视角推断意图，用试探性语言（「可能因为」「似乎为了」）\n",
                "- 无法推断时说「意图不明」\n",
                "- 高频/异常时段活动标注 [high-arousal]\n",
                "- 每条事件输出一个 ACTION save_memory_visible\n",
                "- confidence: 有情绪信号=0.8, 普通=0.6\n",
                "- 输出 NONE 如果没有值得标注的事件",
            ),
            "", "每条事件一行 ACTION，不要输出多余解释",
            "save_memory_visible", "raw_observations,calibration_rules", 30, "", false,
        ),
        (
            "coach",
            "行为教练：从观察笔记中提炼行为模式和偏好",
            concat!(
                "你是 Sage 的学习教练。分析下面的观察记录，发现用户的行为模式、偏好和习惯。\n\n",
                "## 观察记录\n{context}\n\n",
                "## 规则\n",
                "- 只输出新发现的洞察，不重复已有的\n",
                "- 每条洞察用 ACTION save_memory_visible 输出\n",
                "- category 固定为 coach_insight，confidence: 0.7，visibility: subconscious\n",
                "- 洞察类型：行为模式 / 决策倾向 / 沟通风格 / 时间偏好 / 价值取向\n",
                "- 不要输出泛泛而谈的洞察，要具体到可观察的行为\n",
                "- 输出 NONE 如果没有新发现",
            ),
            "observer", "每条洞察一行 ACTION，不要输出多余解释",
            "save_memory_visible", "observer_notes,raw_observations,calibration_rules", 20, "", true,
        ),
        (
            "mirror",
            "认知镜像：选择一个最值得注意的模式，温和地反馈给用户",
            concat!(
                "你是 Sage 的认知镜像。从下面的行为洞察中，选择**一个**最值得注意的模式，\n",
                "写一段温和、不带评判的观察（1-2 句话）。\n",
                "风格：像一个细心的朋友轻轻指出他们注意到的事情。\n\n",
                "## 近期洞察\n{context}\n\n",
                "## 规则\n",
                "- 先输出你的反思文本（1-2 句话）\n",
                "- 然后用 ACTION record_suggestion_dedup 记录\n",
                "- 然后用 ACTION notify_user 发送通知\n",
                "- 输出 NONE 如果洞察不足或无值得反馈的内容",
            ),
            "coach", "",
            "record_suggestion_dedup,notify_user", "coach_insights,calibration_rules", 3, "", false,
        ),
        (
            "questioner",
            "苏格拉底提问：生成一个触及价值观/动机/盲点的深层问题",
            concat!(
                "你是 Sage 的提问者。基于下面的洞察和记忆，生成**一个**苏格拉底式的深层问题。\n\n",
                "## 上下文\n{context}\n\n",
                "## 规则\n",
                "- 问题应触及价值观、动机或盲点\n",
                "- 优先选择信息增益最大的问题\n",
                "- 避免从已有行为中容易推断答案的问题\n",
                "- 先输出问题文本\n",
                "- 然后用 ACTION record_suggestion_dedup 记录\n",
                "- 然后用 ACTION save_open_question 保存\n",
                "- 输出 NONE 如果上下文不足",
            ),
            "mirror", "",
            "record_suggestion_dedup,save_open_question", "coach_insights,memories", 5, "", false,
        ),
        (
            "calibrator",
            "校准器：从用户纠正中提炼自约束规则",
            concat!(
                "你是 Sage 的校准器。分析下面用户对报告的纠正记录，提炼出共性模式和根因。\n\n",
                "## 纠正记录\n{context}\n\n",
                "## 规则\n",
                "- 输出 1-2 条具体的自约束规则，每条 ≤50 字\n",
                "- 用 ACTION save_calibration_rule | 规则内容 | confidence:0.75\n",
                "- 输出 NONE 如果纠正记录不足以提炼规则",
            ),
            "questioner", "每条规则一行 ACTION",
            "save_calibration_rule", "corrections", 10, "", false,
        ),
        (
            "strategist",
            "战略分析师：识别跨领域的结构性趋势和轨迹信号",
            concat!(
                "你是一个完全超脱的战略分析师。从月球看地球——没有情绪、没有偏见，只有结构和轨迹。\n\n",
                "## 数据\n{context}\n\n",
                "## 规则\n",
                "- 识别 2-3 个结构性观察或轨迹信号\n",
                "- 不要重复 Coach 已发现的内容，关注价值观和行为之间的一致性/分歧\n",
                "- 完全中立的学术语调\n",
                "- 每条用 ACTION save_memory_visible 保存，category=strategy_insight，visibility=subconscious\n",
                "- 最后用一个 ACTION record_suggestion_dedup 汇总\n",
                "- 输出 NONE 如果数据不足",
            ),
            "", "最多 3 条，少即是多",
            "save_memory_visible,record_suggestion_dedup", "coach_insights,memories", 10, "", false,
        ),
        (
            "person_observer",
            "人物认知：从今日事件中提取关于特定人物的行为观察",
            concat!(
                "从今日事件中提取关于**其他人**（非用户本人）的行为观察。\n\n",
                "## 今日事件\n{context}\n\n",
                "## 规则\n",
                "- 只提取关于其他人的观察，不包括用户自己\n",
                "- 每人最多一条观察，≤30 字，关注行为模式、能力、协作风格、角色\n",
                "- 不要编造——只从已有证据推断\n",
                "- 同一人在多封邮件/消息中出现时合并为一条\n",
                "- 用 ACTION save_person_memory 保存，最多 8 条\n",
                "- 输出 NONE 如果没有值得记录的内容",
            ),
            "coach", "",
            "save_person_memory", "emails,messages,observer_notes,coach_insights", 30, "", false,
        ),
        (
            "mirror_weekly",
            "周度认知镜像：汇总本周反思信号，生成反映性周报",
            concat!(
                "你是 Sage 的周度认知镜像。回顾本周的行为信号和反思记录，\n",
                "写一份温和、有洞察力的周度回顾（3-5 段落）。\n",
                "风格：像一个理解你的朋友在周末和你聊这一周发生的事。\n\n",
                "## 本周信号\n{context}\n\n",
                "## 规则\n",
                "- 先用 ACTION save_report 保存完整周报\n",
                "- 再用 ACTION record_suggestion_dedup 记录建议（source=mirror, key=weekly-mirror）\n",
                "- 最后用 ACTION notify_user 通知用户查看\n",
                "- 如果信号不足，输出 NONE",
            ),
            "", // insert_after（周度管线自行排序）
            "先输出 ACTION save_report，再输出 ACTION record_suggestion_dedup，最后 ACTION notify_user",
            "save_report,record_suggestion_dedup,notify_user",
            "weekly_signals,coach_insights,memories",
            3,
            "SELECT COUNT(*) = 0 FROM suggestions WHERE source = 'mirror' AND dedup_key = 'weekly-mirror' AND created_at > datetime('now', '-6 days')",
            false,
        ),
        // ── Evolution Presets ──────────────────────────────────────────────
        (
            "evolution_merge",
            "记忆合并：识别并归档重复的表层记忆",
            concat!(
                "你是 Sage 的记忆进化系统。审查下面的表层记忆，找出语义重复的条目。\n\n",
                "## 待审查记忆\n{context}\n\n",
                "## 规则\n",
                "- 对重复记忆用 ACTION dedup_memory 归档（保留最完整的那条）\n",
                "- 对可合并的多条记忆用 ACTION compile_memories 合并为一条更精炼的\n",
                "- 不确定时不操作，输出 NONE\n",
                "- 注意：id 必须来自上面的列表，不要编造",
            ),
            "observer", // insert_after
            "每行一个 ACTION，不输出多余解释",
            "dedup_memory,compile_memories",
            "similar_memories",
            30,
            "",
            false,
        ),
        (
            "evolution_synth",
            "特质提炼：从表层记忆中提炼稳定的行为特质",
            concat!(
                "你是 Sage 的特质提炼器。从下面的表层观察中，识别反复出现的行为模式，\n",
                "将它们提炼为更抽象的特质描述。\n\n",
                "## 表层记忆\n{context}\n\n",
                "## 规则\n",
                "- 只有出现 3 次以上的模式才值得提炼\n",
                "- 用 ACTION compile_memories 将多条观察合并为一条特质\n",
                "- 新特质的 category 用 personality，confidence 用 0.85\n",
                "- 输出 NONE 如果没有新发现",
            ),
            "evolution_merge", // insert_after
            "每行一个 ACTION",
            "compile_memories,promote_memory",
            "similar_memories,memories",
            20,
            "",
            false,
        ),
        (
            "evolution_condense",
            "精简冗长：将超过 50 字的记忆压缩到 30 字以内",
            concat!(
                "你是 Sage 的记忆精简器。将下面每条冗长的记忆压缩到 30 字以内，\n",
                "保留核心信息，去掉修饰语和冗余细节。\n\n",
                "## 冗长记忆\n{context}\n\n",
                "## 规则\n",
                "- 用 ACTION condense_memory 更新每条记忆\n",
                "- 压缩后长度必须 ≤ 30 字\n",
                "- 不要改变原始含义\n",
                "- 已经简洁的记忆（≤30 字）跳过",
            ),
            "evolution_synth", // insert_after
            "每行一个 ACTION condense_memory",
            "condense_memory",
            "verbose_memories",
            15,
            "",
            false,
        ),
        (
            "evolution_link",
            "记忆关联：识别记忆之间的语义关系",
            concat!(
                "你是 Sage 的记忆图谱构建器。分析下面的记忆，找出它们之间的关系。\n\n",
                "## 记忆列表\n{context}\n\n",
                "## 规则\n",
                "- 用 ACTION link_memories 创建关系边\n",
                "- relation 必须是: causes, supports, contradicts, co_occurred, derived_from, similar\n",
                "- weight 范围 0.1-1.0（越相关越高）\n",
                "- 只创建有实际意义的关系，不要强行关联\n",
                "- 最多 20 条关系",
            ),
            "evolution_condense", // insert_after
            "每行一个 ACTION link_memories",
            "link_memories",
            "memories",
            20,
            "",
            false,
        ),
        (
            "evolution_decay",
            "记忆衰减：归档长期未访问的过期记忆",
            concat!(
                "你是 Sage 的记忆衰减器。审查下面长期未访问的记忆，\n",
                "判断哪些已经过时或不再相关。\n\n",
                "## 长期未访问记忆\n{context}\n\n",
                "## 规则\n",
                "- 用 ACTION decay_memory 归档确实过时的记忆\n",
                "- 用户的核心价值观、重要关系等即使久未访问也不应衰减\n",
                "- 事件性记忆（episodic）超过 60 天未访问可以衰减\n",
                "- 不确定时保留，输出 NONE",
            ),
            "evolution_link", // insert_after
            "每行一个 ACTION decay_memory",
            "decay_memory",
            "memories",
            10,
            "SELECT COUNT(*) FROM memories WHERE last_accessed_at < datetime('now', '-30 days') AND depth = 'episodic'",
            false,
        ),
        (
            "evolution_promote",
            "记忆晋升：将经过验证的记忆提升到更高层级",
            concat!(
                "你是 Sage 的记忆评估器。审查下面的记忆，判断哪些已经足够稳定，\n",
                "可以从当前层级提升到更高层级。\n\n",
                "## 待评估记忆\n{context}\n\n",
                "## 层级说明\n",
                "- episodic（事件性）→ semantic（语义性）：同一模式出现 3+ 次\n",
                "- semantic → procedural（程序性）：稳定的行为习惯\n",
                "- procedural → axiom（公理性）：核心价值观（极少使用）\n\n",
                "## 规则\n",
                "- 用 ACTION promote_memory 提升层级\n",
                "- 只提升确实已验证的记忆\n",
                "- axiom 层级需要极高确信度（被反复验证的核心信念）",
            ),
            "evolution_decay", // insert_after
            "每行一个 ACTION promote_memory",
            "promote_memory",
            "memories",
            10,
            "",
            false,
        ),
        // ── Meta Presets ───────────────────────────────────────────────────
        (
            "meta_params",
            "管线参数进化：根据执行统计调整 stage 参数",
            concat!(
                "你是 Sage 的管线优化器。根据过去 14 天的执行统计，判断是否需要调整参数。\n\n",
                "## 执行统计\n{context}\n\n",
                "## 规则\n",
                "- 用 ACTION set_pipeline_override 调整参数\n",
                "- 可调整 key: enabled (true/false), max_iterations (数字)\n",
                "- 不允许禁用 evolution 或 meta 相关 stage\n",
                "- 只在统计数据明确支持时才调整\n",
                "- 大多数情况输出 NONE",
            ),
            "", // insert_after
            "每行一个 ACTION，慎重调整",
            "set_pipeline_override",
            "pipeline_stats",
            5,
            "",
            false,
        ),
        (
            "meta_prompts",
            "Prompt 自进化：将校准规则烘焙到 prompt 中",
            concat!(
                "你是 Sage 的 Prompt 工程师。下面是用户反馈产生的校准规则，\n",
                "判断是否需要将某些规则永久写入对应的 prompt 文件。\n\n",
                "## 校准规则\n{context}\n\n",
                "## 规则\n",
                "- 只有被反复确认的规则才值得烘焙\n",
                "- 用 ACTION rewrite_prompt 更新 prompt 文件\n",
                "- prompt_name 可选: observer_user, coach_user, mirror_user, task_intelligence_user\n",
                "- 大多数情况输出 NONE",
            ),
            "meta_params", // insert_after
            "每行一个 ACTION",
            "rewrite_prompt,save_memory",
            "calibration_rules",
            5,
            "SELECT COUNT(*) FROM memories WHERE category IN ('calibration', 'calibration_task') AND created_at > datetime('now', '-30 days')",
            false,
        ),
        (
            "meta_ui",
            "UI 页面进化：根据记忆和观察生成个性化页面",
            concat!(
                "你是 Sage 的 UI 设计师。根据用户的记忆和观察数据，\n",
                "生成一个有价值的个性化洞察页面（markdown 格式）。\n\n",
                "## 用户数据\n{context}\n\n",
                "## 规则\n",
                "- 用 ACTION save_custom_page 保存页面\n",
                "- 标题格式: [auto] 页面主题\n",
                "- 内容用 markdown，2-4 段落\n",
                "- 只在有足够数据时才生成\n",
                "- 大多数情况输出 NONE",
            ),
            "meta_prompts", // insert_after
            "ACTION save_custom_page | [auto] 标题 | markdown 内容",
            "save_custom_page",
            "memories,raw_observations",
            3,
            "SELECT COUNT(*) > 10 FROM memories",
            false,
        ),
    ];

    for &(name, desc, prompt, after, fmt, acts, inputs, max, pre, archive) in presets {
        conn.execute(
            "INSERT OR IGNORE INTO custom_stages \
             (name, description, prompt, insert_after, output_format, \
              available_actions, allowed_inputs, max_actions, pre_condition, \
              is_preset, archive_observations) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10)",
            rusqlite::params![name, desc, prompt, after, fmt, acts, inputs, max, pre, archive as i32],
        )?;
    }
    Ok(())
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
