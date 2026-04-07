# Sage 认知管线架构文档

更新日期：2026-04-01

---

## 1. 总览

Sage 的认知管线是一个 **DAG（有向无环图）执行引擎**，每晚自动运行，将原始事件（邮件、消息、浏览行为）转化为结构化认知记忆。

```
数据源 → Daemon tick → Router → CognitivePipeline → Store (SQLite)
                                      ↓
                Evening 管线（16 preset stages，DAG 并行）
                Weekly 管线（mirror_weekly）
```

**核心设计原则**：LLM 只能通过 `ConstrainedInvoker` trait 调用，Store 写入只能通过 ACTION 约束系统执行。模块拿不到 raw Agent，编译期强制约束。

---

## 2. ConstrainedInvoker Trait

**文件**：`crates/sage-core/src/pipeline/invoker.rs`（141 行）

编译期强制约束：所有认知模块只接受此 trait，拿不到 raw Agent。

| 方法 | 用途 |
|------|------|
| `invoke(prompt, system)` | 标准 LLM 调用（300s 超时） |
| `invoke_long(prompt, system)` | 长超时调用（600s），用于 evolution |
| `write_action(action_line, allowed)` | 受约束的单条 Store 写入 |
| `write_actions_from_text(text, allowed, max)` | 批量解析 ACTION 行并执行（带 rate limit） |
| `reset_counter()` | 重置 LLM 调用计数器（批次处理场景） |
| `clone_boxed()` | 克隆为 Box（用于 tokio::spawn） |
| `as_provider()` | 获取底层 LlmProvider（仅 MemoryIntegrator 用） |

**唯一实现**：`HarnessedAgent { agent: Agent, store: Arc<Store>, caller: String }`

**便捷函数**（定义在 invoker.rs，替代旧 harness.rs）：

| 函数 | 行为 |
|------|------|
| `invoke_text` | invoke → 提取 `<output>` 块 → 返回纯文本 |
| `invoke_text_long` | invoke_long → 提取 `<output>` 块 |
| `invoke_raw` | invoke → 不提取，原样返回 trimmed |
| `invoke_json<T>` | invoke → 提取 + JSON 反序列化 |
| `invoke_commands<T, F>` | invoke → 提取 + 逐行 typed 解析 |

---

## 3. ACTION 约束系统

**文件**：`crates/sage-core/src/pipeline/actions.rs`（836 行）

三层硬约束：**授权白名单** → **参数验证** → **Rate Limit**

### 3.1 ACTION 类型（22 种）

#### 基础 ACTION（10 种）

| Action | 格式 | 关键验证 |
|--------|------|---------|
| `create_task` | `content \| priority:normal/P0/P1/P2 \| due:YYYY-MM-DD` | content 非空；priority 枚举；due 长度=10 |
| `save_memory` | `category \| 内容 \| confidence:0.0-1.0` | content 非空；confidence 范围 |
| `save_memory_visible` | `category \| 内容 \| confidence \| visibility:public/subconscious/private` | content 非空；visibility 枚举 |
| `save_memory_integrated` | 同 save_memory_visible | 同上（TODO: 未来升级为 MemoryIntegrator 去重仲裁） |
| `send_notification` | `标题 \| 内容` | 标题非空 |
| `notify_user` | `标题 \| 内容` | 标题非空 |
| `save_observation` | `观察内容` | 内容非空 |
| `record_suggestion_dedup` | `source_key \| dedup_key \| 内容` | 三段均非空；24h 内 source+key 去重 |
| `save_open_question` | `问题内容 \| suggestion_id:N` | 内容非空 |
| `bump_question` | `question_id:N` | id 可解析为 i64 |

#### 人物 & 报告 ACTION（3 种）

| Action | 格式 | 关键验证 |
|--------|------|---------|
| `save_person_memory` | `人名 \| category \| 内容 \| confidence \| visibility` | 人名+内容非空 |
| `save_report` | `report_type \| 内容` | type+content 非空 |
| `save_calibration_rule` | `规则内容 \| confidence` | 内容非空；同步写入 negative_rules |

#### Evolution ACTION（6 种）

| Action | 格式 | 关键验证 |
|--------|------|---------|
| `dedup_memory` | `memory_id \| reason` | id 为 i64 |
| `compile_memories` | `source_ids \| content \| category \| confidence` | ids+content 非空 |
| `condense_memory` | `memory_id \| new_content` | id 为 i64；content 非空 |
| `link_memories` | `id1 \| id2 \| relation \| weight:0.0-1.0` | id 为 i64；relation 白名单；weight 范围 |
| `promote_memory` | `memory_id \| new_depth` | id 为 i64；depth 枚举 |
| `decay_memory` | `memory_id \| reason` | id 为 i64 |

#### Meta ACTION（3 种）

| Action | 格式 | 关键验证 |
|--------|------|---------|
| `set_pipeline_override` | `stage_name \| key \| value \| reason` | 禁止禁用 evolution 核心阶段 |
| `rewrite_prompt` | `prompt_name \| new_content` | name+content 非空；content <10KB |
| `save_custom_page` | `title \| content` | title+content 非空；content <10KB |

### 3.2 执行函数

| 函数 | 输入 | 场景 |
|------|------|------|
| `execute_actions(text, actions, store, stage, max)` | LLM 原始输出 | UserDefinedStage 主执行路径 |
| `execute_single_action(line, whitelist, store, caller)` | 不含 "ACTION " 前缀的行 | 各模块单条写入 |
| `execute_constrained_actions(lines, whitelist, store, caller, max)` | 预处理的 action 行切片 | 批量写入 |

### 3.3 load_filtered_context 数据源（12 种）

| 来源键 | 查询方法 | 上限 | 使用者 |
|--------|---------|------|--------|
| `raw_observations` | `load_unprocessed_observations(50)` | 50 | observer |
| `observer_notes` | `get_today_observer_notes()` | 15 | coach |
| `coach_insights` | `get_today_coach_insights()` | 10 | mirror, questioner, strategist, person_observer |
| `emails` | `get_today_email_summaries(15)` | 15 | person_observer |
| `messages` | `get_today_message_summaries(20)` | 20 | person_observer |
| `memories` | `get_memories_since(3天前)` | 15 | questioner, strategist, evolution_link/decay/promote, meta_ui |
| `corrections` | `get_corrections_for_pattern(type)` | 全量 | calibrator |
| `calibration_rules` | `get_memories_by_category("calibration")` | 全量 | observer, coach, mirror, meta_prompts |
| `recent_observations` | `load_recent_observations(200)` | 100 | — |
| `weekly_signals` | `get_recent_suggestions(20)` | 20 | mirror_weekly |
| `similar_memories` | `load_memories_by_depth("episodic")` | 50 | evolution_merge, evolution_synth |
| `verbose_memories` | `load_memories().filter(len>50)` | 50 | evolution_condense |
| `pipeline_stats` | `get_pipeline_summary(14)` | 全量 | meta_params |

---

## 4. Pipeline 执行引擎

**文件**：`crates/sage-core/src/pipeline.rs`（590 行）

### 4.1 核心类型

```rust
CognitivePipeline {
    stages: HashMap<String, Arc<dyn CognitiveStage>>,  // 注册的 stage 实例
    adj / rev_adj: HashMap<String, Vec<String>>,         // DAG 邻接表
    all_nodes: Vec<String>,                              // 拓扑排序后的节点列表
    stage_configs: HashMap<String, StageConfig>,         // per-stage 配置
    core_stages: Vec<String>,                            // evening 前两个 stage
}
```

### 4.2 DAG 执行策略（Wave-based Kahn 算法）

```
1. 计算入度 → 入度为 0 的节点入 ready 队列
2. 每波取出所有 ready nodes
3. 分类：
   - ctx_writers（串行组）：observer, coach, mirror, questioner, evolution_*, meta_*
   - 其余（并行组）：tokio::spawn 真并行，各自空 ctx
4. 串行组顺序执行，传递 PipelineContext
5. 并行组并发执行，各自拿空 PipelineContext
6. 完成后更新入度，推入新 ready nodes
```

### 4.3 超时策略

| Stage 类型 | 默认超时 | 可覆盖 |
|-----------|---------|--------|
| 普通 stage | 120s | `stage_configs[name].timeout_secs` |
| evolution_* | 300s | 同上 |
| 通过 pipeline_overrides 覆盖 | — | `max_iterations` |

### 4.4 结果分类

| StageStatus | 条件 |
|------------|------|
| `Ok` | StageOutput::Bool(true) 或 Evolution 有变更 |
| `Empty` | StageOutput::Bool(false) 或 Evolution 无变更 |
| `Degraded` | 错误含"已达上限"（LLM budget 耗尽） |
| `Skipped` | stage 被 disable 或 pre_condition 不满足 |
| `Error` | 超时或其他异常 |

---

## 5. UserDefinedStage — 统一执行引擎

**文件**：`crates/sage-core/src/pipeline/stages.rs`（131 行）

所有 16 个 preset stage 和用户自定义 stage 共用此引擎：

```
═══ PRE-HOOK（硬约束）═══
  pre_condition SQL → 返回 >0 才继续
  load_filtered_context(allowed_inputs) → 无数据则跳过

═══ LLM 执行（软约束在 prompt 中）═══
  prompt_template.replace("{context}") + output_format + action_docs
  reset_counter() → invoke_text()

═══ POST-HOOK（硬约束）═══
  execute_actions(白名单 + rate limit)
  非 ACTION 行 → record_observation("custom_{stage}")
  archive_observations（若开启）→ mark_observations_processed
```

---

## 6. 16 个预设 Stage

**种子文件**：`crates/sage-store/src/pipeline.rs` → `seed_preset_stages()`

### Evening 管线（DAG 依赖链）

```
observer ──→ coach ──→ mirror ──→ questioner ──→ calibrator
               │                                    ↑（无直接依赖）
               └──→ person_observer
                                            strategist（无依赖，可并行）

evolution_merge → evolution_synth → evolution_condense → evolution_link → evolution_decay → evolution_promote

meta_params → meta_prompts → meta_ui
```

### 认知阶段（7 个）

| Stage | 输入源 | 可用 ACTION | 上限 | 归档 obs | 核心行为 |
|-------|--------|------------|------|---------|---------|
| **observer** | raw_observations, calibration_rules | save_memory_visible | 30 | ✗ | 语义维度分解 + 逐事件标注 |
| **coach** | observer_notes, raw_observations, calibration_rules | save_memory_visible | 20 | ✓ | 从观察中提炼行为模式 |
| **mirror** | coach_insights, calibration_rules | record_suggestion_dedup, notify_user | 3 | ✗ | 选一个最值得注意的模式反馈 |
| **questioner** | coach_insights, memories | record_suggestion_dedup, save_open_question | 5 | ✗ | 生成苏格拉底式深层问题 |
| **calibrator** | corrections | save_calibration_rule | 10 | ✗ | 从纠正记录提炼自约束规则 |
| **strategist** | coach_insights, memories | save_memory_visible, record_suggestion_dedup | 10 | ✗ | 跨领域结构性趋势识别 |
| **person_observer** | emails, messages, observer_notes, coach_insights | save_person_memory | 30 | ✗ | 提取他人行为观察 |

### 周度阶段（1 个）

| Stage | 输入源 | 可用 ACTION | 上限 | pre_condition |
|-------|--------|------------|------|-------------|
| **mirror_weekly** | weekly_signals, coach_insights, memories | save_report, record_suggestion_dedup, notify_user | 3 | 本周未生成过 weekly-mirror |

### Evolution 阶段（6 个）

| Stage | 输入源 | 可用 ACTION | 上限 | pre_condition |
|-------|--------|------------|------|-------------|
| **evolution_merge** | similar_memories | dedup_memory, compile_memories | 30 | — |
| **evolution_synth** | similar_memories, memories | compile_memories, promote_memory | 20 | — |
| **evolution_condense** | verbose_memories | condense_memory | 15 | — |
| **evolution_link** | memories | link_memories | 20 | — |
| **evolution_decay** | memories | decay_memory | 10 | episodic 记忆 30 天未访问 |
| **evolution_promote** | memories | promote_memory | 10 | — |

### Meta 阶段（3 个） — 管线自我进化

| Stage | 输入源 | 可用 ACTION | 上限 | pre_condition |
|-------|--------|------------|------|-------------|
| **meta_params** | pipeline_stats | set_pipeline_override | 5 | — |
| **meta_prompts** | calibration_rules | rewrite_prompt, save_memory | 5 | 30 天内有校准记忆 |
| **meta_ui** | memories, raw_observations | save_custom_page | 3 | 记忆 >10 条 |

---

## 7. Pipeline 存储层

**文件**：`crates/sage-store/src/pipeline.rs`（731 行）

### 7.1 执行日志（pipeline_runs 表）

| 方法 | 用途 |
|------|------|
| `log_pipeline_run(stage, pipeline, outcome, elapsed_ms)` | 记录每次 stage 执行 |
| `get_pipeline_runs(stage, limit)` | 查询某 stage 最近 N 次记录 |
| `get_pipeline_summary(days)` | 汇总各 stage 的 ok/empty/error 计数 |

### 7.2 运行时覆盖（pipeline_overrides 表）

| 方法 | 用途 |
|------|------|
| `set_pipeline_override(stage, key, value, reason)` | 写入/更新覆盖 |
| `get_pipeline_overrides(stage)` | 读取某 stage 的覆盖 |
| `get_all_pipeline_overrides()` | 读取全部覆盖 |
| `delete_pipeline_override(stage, key)` | 删除一条覆盖 |

### 7.3 自定义 Stage CRUD（custom_stages 表）

| 方法 | 用途 |
|------|------|
| `create_custom_stage(...)` | 创建/更新自定义阶段（INSERT OR REPLACE） |
| `list_custom_stages()` | 列出所有阶段（含 preset + custom） |
| `delete_custom_stage(id)` | 删除（禁止删 preset） |
| `toggle_custom_stage(id, enabled)` | 启用/禁用 |
| `execute_condition_query(sql)` | 执行 pre_condition SQL |
| `seed_preset_stages(conn)` | 种子 16 个内置 preset（INSERT OR IGNORE） |

### 7.4 CustomStage 结构

```rust
CustomStage {
    id, name, description, prompt,
    insert_after,          // DAG 依赖（空=无依赖）
    enabled,               // 是否启用
    output_format,         // 输出格式说明（注入 prompt）
    available_actions,     // 逗号分隔的 ACTION 白名单
    allowed_inputs,        // 逗号分隔的数据源白名单
    max_actions,           // Rate limit
    pre_condition,         // SQL 前置检查
    is_preset,             // 是否内置预设（不可删除）
    archive_observations,  // 执行后归档 observations
}
```

---

## 8. 配置层

**文件**：`crates/sage-core/src/config.rs`（459 行）

```toml
# config.toml 示例
[pipeline]
evening = ["observer", "coach", "mirror", "questioner", "calibrator",
           "strategist", "person_observer",
           "evolution_merge", "evolution_synth", "evolution_condense",
           "evolution_link", "evolution_decay", "evolution_promote",
           "meta_params", "meta_prompts", "meta_ui"]
weekly = ["mirror_weekly"]

# 可选 DAG 边定义（覆盖 evening 隐含链式依赖）
[[pipeline.edges]]
from = "observer"
to = "coach"

# per-stage 配置
[pipeline.stages.evolution_merge]
max_iterations = 20
timeout_secs = 300
```

**拓扑排序**：`topo_sort(seed_order, edges)` — BFS Kahn 算法，seed_order 作为同优先级 tiebreaker。

---

## 9. Daemon 集成

**文件**：`crates/sage-core/src/daemon.rs`（947 行）

Pipeline 对象**每次调用时重建**，不缓存在 Daemon 上：

| 触发方式 | 调用路径 |
|---------|---------|
| Evening 自动触发 | `tick()` → 检测时间窗口 → `build_pipeline()` → `pipeline.run_evening()` |
| Weekly 自动触发 | `tick()` → 检测时间窗口 → `build_pipeline()` → `pipeline.run_weekly()` |
| 手动 Evolution | `trigger_memory_evolution()` → `build_pipeline()` → 逐 stage 运行 + 进度回调 |
| 手动单 Stage | `run_pipeline_preset(name)` → `build_pipeline()` → `pipeline.run(name, &[preset])` |

---

## 10. 旧 Harness 残留

**文件**：`crates/sage-core/src/pipeline/harness.rs`（69 行）— 待废弃

旧式基于 `&Agent` 的 LLM 调用，仅剩 3 处使用：

| 位置 | 调用 |
|------|------|
| `channels/feed.rs:109` | `harness::invoke_text(agent, ...)` |
| `channels/feed.rs:288` | `harness::invoke_text(agent, ...)` |
| `daemon.rs:713` | `harness::invoke_raw(agent, ...)` |

当 feed.rs 和 daemon.rs 的 feed digest 迁移到 `ConstrainedInvoker` 后，harness.rs 可删除。

---

## 11. LLM 输出解析

**文件**：`crates/sage-core/src/pipeline/parser.rs`（154 行）

| 函数 | 行为 |
|------|------|
| `extract_output_block(text)` | 提取 `<output>...</output>` 块，无标签则返回原文 |
| `parse_json_fenced<T>(text)` | 剥离 markdown code fence → JSON 反序列化 |
| `parse_commands<T, F>(text, parser)` | 逐行 typed 解析 → `ParseResult { commands, rejected }` |

---

## 12. 文件行数汇总

| 文件 | 行数 | 职责 |
|------|------|------|
| `pipeline.rs` | 590 | DAG 引擎 + build_pipeline + 测试 |
| `pipeline/actions.rs` | 836 | ACTION 文档+验证+执行+数据源过滤 |
| `pipeline/invoker.rs` | 141 | ConstrainedInvoker trait + HarnessedAgent |
| `pipeline/stages.rs` | 131 | UserDefinedStage 统一执行引擎 |
| `pipeline/parser.rs` | 154 | LLM 输出解析 |
| `pipeline/harness.rs` | 69 | 旧式 harness（待废弃） |
| `sage-store/pipeline.rs` | 731 | 执行日志+覆盖+自定义 stage CRUD+种子 |
| **合计** | **2652** | |

---

## 13. 架构决策记录

### 为什么全走 Preset UserDefinedStage？

早期版本中 observer/coach/mirror/questioner 有独立的内置 `bool_stage!` 实现。重构后全部由 preset stage 取代，走 `UserDefinedStage` 统一路径。好处：

1. **单一执行引擎**：所有 stage 共用同一套约束检查、ACTION 执行、超时控制
2. **Prompt 可进化**：meta_prompts stage 可以在运行时重写任何 preset 的 prompt
3. **用户可扩展**：自定义 stage 和 preset stage 使用完全相同的机制，`insert_after` 字段控制 DAG 插入点

### 为什么 Pipeline 每次重建？

`build_pipeline()` 不缓存在 Daemon 上，因为：

1. `custom_stages` 表可能被用户/meta_params 随时修改
2. `pipeline_overrides` 表的值需要实时反映
3. Pipeline 构建成本极低（只是 HashMap 组装，无 I/O）

### ACTION 白名单 vs 黑名单

选择白名单（每个 stage 显式声明可用 ACTION）而非黑名单，因为：

1. LLM 幻觉可能产生任意 ACTION 名称
2. 白名单失败是沉默跳过（安全），黑名单失败是意外执行（危险）
3. 新增 ACTION 类型时，旧 stage 不会自动获得新权限
