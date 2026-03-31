# Sage 认知管线架构审查报告

生成日期：2026-03-31

---

## 1. ConstrainedInvoker Trait

**文件：** `crates/sage-core/src/pipeline/invoker.rs`（142 行）

编译期强制约束：所有认知模块只能接受此 trait，拿不到 raw Agent。

| 方法 | 用途 |
|------|------|
| `invoke(prompt, system)` | 标准 LLM 调用（300s） |
| `invoke_long(prompt, system)` | 长超时调用（600s），用于 evolution |
| `write_action(action_line, allowed)` | 受约束的单条 Store 写入 |
| `write_actions_from_text(text, allowed, max)` | 批量解析 ACTION 行并执行（带 rate limit） |
| `reset_counter()` | 重置 LLM 调用计数器 |
| `clone_boxed()` | 克隆为 Box（用于 tokio::spawn） |
| `as_provider()` | 获取底层 LlmProvider（仅 MemoryIntegrator 用） |

**唯一实现**：`HarnessedAgent { agent: Agent, store: Arc<Store>, caller: String }`

**便捷函数**（定义在 invoker.rs，替代旧 harness.rs）：
- `invoke_text` → 提取 `<output>` 块
- `invoke_raw` → 不提取，原样返回
- `invoke_json<T>` → 提取 + JSON 反序列化
- `invoke_commands<T, F>` → 提取 + 逐行 typed 解析

---

## 2. ACTION 约束系统

**文件：** `crates/sage-core/src/pipeline/actions.rs`（459 行）

### 10 种 ACTION 类型

| Action | 格式 | 关键验证 |
|--------|------|---------|
| `create_task` | `content \| priority:normal/P0/P1/P2 \| due:YYYY-MM-DD` | content 非空；priority 枚举；due 长度=10 |
| `save_memory` | `category \| 内容 \| confidence:0.0-1.0` | content 非空；confidence 范围 |
| `save_memory_visible` | `category \| 内容 \| confidence \| visibility:public/subconscious/private` | content 非空；visibility 枚举 |
| `send_notification` | `标题 \| 内容` | 标题非空 |
| `notify_user` | `标题 \| 内容` | 标题非空 |
| `save_observation` | `观察内容` | 内容非空 |
| `record_suggestion_dedup` | `source_key \| dedup_key \| 内容` | 三段均非空 |
| `save_open_question` | `问题内容 \| suggestion_id:N` | 内容非空 |
| `bump_question` | `question_id:N` | id 可解析为 i64 |
| `save_person_memory` | `人名 \| category \| 内容 \| confidence \| visibility` | 人名+内容非空 |

### 三个执行函数

| 函数 | 输入 | 场景 |
|------|------|------|
| `execute_single_action(line, whitelist, store, caller)` | 不含 "ACTION " 前缀的行 | 各模块单条写入 |
| `execute_constrained_actions(lines, whitelist, store, caller, max)` | 预处理的 action 行切片 | 批量写入 |
| `execute_actions(text, actions, store, stage, max)` | LLM 原始输出 | UserDefinedStage |

### load_filtered_context 数据源

| 来源 | 查询 | 上限 |
|------|------|------|
| `observer_notes` | `get_today_observer_notes()` | 15 |
| `coach_insights` | `get_today_coach_insights()` | 10 |
| `emails` | `get_today_email_summaries(15)` | 15 |
| `messages` | `get_today_message_summaries(20)` | 20 |
| `memories` | `get_memories_since(3天前)` | 15 |
| `raw_observations` | `load_unprocessed_observations(50)` | 50 |
| `corrections` | `get_corrections_for_pattern(type)` | 全量 |

---

## 3. Pipeline Stages

### 3.1 内置 Stage（bool_stage! 宏）

| Stage | 委托函数 |
|-------|---------|
| ObserverStage | `observer::annotate` |
| CoachStage | `coach::learn` |
| MirrorStage | `mirror::reflect` |
| QuestionerStage | `questioner::ask` |
| PersonObserverStage | `person_observer::extract_persons` |
| CalibratorStage | `calibrator::reflect_patterns` |
| StrategistStage | `strategist::strategize` |
| MirrorWeeklyStage | `mirror::mirror_weekly` |
| EvolutionStage | `memory_evolution::evolve`（单独实现） |

### 3.2 UserDefinedStage 执行流程

```
PRE-HOOK → pre_condition SQL 检查 → load_filtered_context(allowed_inputs)
    ↓
LLM 调用 → template.replace("{context}") + output_format + action_docs
    ↓
POST-HOOK → execute_actions(白名单 + rate limit) → 非 ACTION 行存 observation
    ↓
联动 → archive_observations → preset_ctx_key 回写 PipelineContext
```

### 3.3 DAG 执行引擎

- **串行组**（ctx 写入者）：observer → coach → mirror → questioner → evolution
- **并行组**（其余 stage）：tokio::spawn 真并行，各自空 ctx
- **超时**：默认 120s，evolution 600s，可通过 stage_configs/pipeline_overrides 覆盖

---

## 4. 预设 Stage（v46 seed）

| name | inputs | actions | max |
|------|--------|---------|-----|
| observer | raw_observations | save_memory_visible | 30 |
| coach | observer_notes, raw_observations | save_memory_visible | 20 |
| mirror | coach_insights | record_suggestion_dedup, notify_user | 3 |
| questioner | coach_insights, memories | record_suggestion_dedup, save_open_question | 5 |
| calibrator | corrections | save_memory | 10 |
| strategist | coach_insights, memories | save_memory_visible, record_suggestion_dedup | 10 |
| person_observer | emails, messages, observer_notes, coach_insights | save_person_memory | 30 |

预设激活时取代同名内置 Stage，走 `UserDefinedStage` 全约束路径。

---

## 5. MetaStage — 管线自我进化

| 阶段 | 触发条件 | 动作 | Rate Limit |
|------|---------|------|-----------|
| evolve_pipeline_params | 每次 evening | DISABLE/ENABLE/INCREASE/DECREASE stage | 5 条/次 |
| evolve_prompts | calibration 记忆 ≥ 3 | LLM 重写 prompt → 写入 ~/.sage/prompts/ | 5 条/次 |
| evolve_ui | 记忆 ≥ 10 或观察 ≥ 5 | LLM 生成 markdown 页面 | 3 页/次 |

---

## 6. LLM 调用迁移状态

### ✅ 完全迁移

| 模块 | LLM 调用 | Store 写入 |
|------|---------|-----------|
| observer/coach/mirror/questioner | ConstrainedInvoker | 预设走 execute_actions |
| staleness | ConstrainedInvoker | 无 LLM 产出写入 |
| router（4 处） | invoker::invoke_* | execute_single_action |
| pipeline/meta | ConstrainedInvoker | execute_single_action + 元数据直调 |
| task_intelligence（3 处） | ConstrainedInvoker | constrained_create_task + execute_single_action |

### ⚠️ 部分迁移

| 模块 | 状态 | 说明 |
|------|------|------|
| calibrator/strategist/person_observer | LLM ✅ Store ⚠️ | 内置版直调 store（预设版走 ACTION） |
| memory_evolution | LLM ✅ Store ⚠️ | 领域命令+rate limit，非 ACTION 格式 |
| memory_integrator | LLM ⚠️ Store ⚠️ | 用 &dyn LlmProvider（非 ConstrainedInvoker），写入有 validate |
| bridge /api/memories | N/A Store ⚠️ | 批量导入直调 store |
| channels/feed（2 处） | LLM ❌ | 旧 harness::invoke_text(agent) |
| daemon.rs feed digest | LLM ❌ | 旧 harness::invoke_raw(agent) |

### 旧 harness.rs 残留（3 处）

| 位置 | 调用 |
|------|------|
| `channels/feed.rs:109` | `harness::invoke_text(agent, ...)` |
| `channels/feed.rs:288` | `harness::invoke_text(agent, ...)` |
| `daemon.rs:702` | `harness::invoke_raw(agent, ...)` |

当 feed.rs 迁移到 ConstrainedInvoker 后，harness.rs 可废弃。

---

## 7. 文件行数

| 文件 | 行数 |
|------|------|
| pipeline/invoker.rs | 142 |
| pipeline/actions.rs | 459 |
| pipeline/stages.rs | 222 |
| pipeline.rs | 622 |
| pipeline/meta.rs | 281 |
| pipeline/parser.rs | 155 |
| pipeline/harness.rs | 164（待废弃） |
| sage-store/pipeline.rs | 510 |
