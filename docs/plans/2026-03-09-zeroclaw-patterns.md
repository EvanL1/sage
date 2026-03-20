# ZeroClaw 架构借鉴实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 ZeroClaw 的三个核心架构模式（记忆统一、Provider 弹性、Agent 护栏）引入 Sage

**Architecture:** 三条并行工作流 —— (A) 记忆系统统一到 SQLite，淘汰 markdown 文件 (B) Provider 层加入重试+回退+hint路由 (C) Agent 循环加护栏。所有改动在 sage-core 层完成，daemon 和 desktop 只做适配调用。

**Tech Stack:** Rust, SQLite (rusqlite), tokio, serde, chrono

---

## 并行分组

```
工作流 A（记忆统一）     工作流 B（Provider 弹性）     工作流 C（Agent 护栏）
  Task 1: Store 扩展       Task 4: ReliableProvider      Task 6: Agent Loop 限制
  Task 2: Router 迁移      Task 5: RouterProvider         Task 7: 历史压缩
  Task 3: Coach/Mirror/                                    Task 8: 凭证脱敏
          Questioner 迁移
```

A、B、C 之间无依赖，可完全并行。每个工作流内部需串行。

---

## 工作流 A：记忆统一

### 现状
- `memory.rs`（Memory struct）：读写 `~/.sage/memory/` 下的 markdown 文件
  - `MEMORY.md` — 核心记忆
  - `patterns.md` — 行为模式（Coach append）
  - `decisions.md` — 决策记录（Router append）
  - `sage.md` — Coach 分析输出
- `store.rs`（Store struct）：SQLite memories 表（Desktop chat 用）

### 目标
统一到 SQLite `memories` 表，用 `category` 区分原有文件角色：
- `MEMORY.md` → category = "core"
- `patterns.md` → category = "pattern"
- `decisions.md` → category = "decision"
- `sage.md` → category = "coach_insight"

### Task 1: Store 扩展 — 新增记忆上下文方法

**Files:**
- Modify: `crates/sage-core/src/store.rs`
- Test: 同文件 `#[cfg(test)]` 模块

**Step 1: 写失败测试**

```rust
#[test]
fn test_get_memory_context() {
    let store = Store::open_in_memory().unwrap();
    store.save_memory("core", "我是 Alex，Team Lead", "system", 1.0).unwrap();
    store.save_memory("pattern", "每周五下午会复盘", "coach", 0.8).unwrap();
    store.save_memory("decision", "选择 Rust 重写 EMS", "chat", 0.9).unwrap();
    store.save_memory("coach_insight", "Alex 倾向系统思考", "coach", 0.7).unwrap();

    let ctx = store.get_memory_context(2000).unwrap();
    assert!(ctx.contains("Alex"));
    assert!(ctx.contains("Rust"));
    assert!(ctx.contains("系统思考"));
    assert!(ctx.len() <= 2000);
}

#[test]
fn test_append_pattern() {
    let store = Store::open_in_memory().unwrap();
    store.append_pattern("工作", "每天 9 点开始处理邮件").unwrap();
    let results = store.search_memories("邮件", 5).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].category, "pattern");
}

#[test]
fn test_append_decision() {
    let store = Store::open_in_memory().unwrap();
    store.append_decision("技术选型", "选择 SQLite 统一记忆存储").unwrap();
    let results = store.search_memories("SQLite", 5).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].category, "decision");
}

#[test]
fn test_save_coach_insight() {
    let store = Store::open_in_memory().unwrap();
    store.save_coach_insight("Alex 重视团队自主性").unwrap();
    let results = store.search_memories("自主", 5).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].category, "coach_insight");
}
```

**Step 2: 运行测试确认失败**

Run: `cargo test -p sage-core -- store::tests::test_get_memory_context store::tests::test_append_pattern store::tests::test_append_decision store::tests::test_save_coach_insight -v`
Expected: FAIL — 方法不存在

**Step 3: 实现**

在 `store.rs` 的 `impl Store` 块中添加：

```rust
/// 构建 LLM 上下文字符串（替代 memory.as_context()）
/// 按类别组织，截断到 max_bytes
pub fn get_memory_context(&self, max_bytes: usize) -> Result<String> {
    let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁: {e}"))?;
    let mut stmt = conn.prepare(
        "SELECT category, content FROM memories ORDER BY confidence DESC, updated_at DESC"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut sections: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for row in rows {
        let (cat, content) = row?;
        sections.entry(cat).or_default().push(content);
    }

    let mut output = String::new();
    let order = ["core", "pattern", "decision", "coach_insight"];
    let labels = ["核心记忆", "行为模式", "决策记录", "教练洞察"];
    for (cat, label) in order.iter().zip(labels.iter()) {
        if let Some(items) = sections.get(*cat) {
            output.push_str(&format!("\n## {label}\n"));
            for item in items {
                output.push_str(&format!("- {item}\n"));
            }
        }
    }
    // 其余自定义 category
    for (cat, items) in &sections {
        if !order.contains(&cat.as_str()) {
            output.push_str(&format!("\n## {cat}\n"));
            for item in items {
                output.push_str(&format!("- {item}\n"));
            }
        }
    }
    if output.len() > max_bytes {
        // 按 UTF-8 字符边界安全截断
        let mut end = max_bytes;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
    }
    Ok(output)
}

/// 记录行为模式（替代 memory.record_pattern）
pub fn append_pattern(&self, category: &str, observation: &str) -> Result<i64> {
    let content = format!("[{category}] {observation}");
    self.save_memory("pattern", &content, "daemon", 0.6)
}

/// 记录决策（替代 memory.record_decision）
pub fn append_decision(&self, context: &str, decision: &str) -> Result<i64> {
    let content = format!("[{context}] {decision}");
    self.save_memory("decision", &content, "daemon", 0.7)
}

/// 保存 Coach 分析输出（替代覆写 sage.md）
pub fn save_coach_insight(&self, insight: &str) -> Result<i64> {
    self.save_memory("coach_insight", insight, "coach", 0.8)
}
```

**Step 4: 运行测试确认通过**

Run: `cargo test -p sage-core -- store::tests::test_get_memory_context store::tests::test_append_pattern store::tests::test_append_decision store::tests::test_save_coach_insight -v`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/sage-core/src/store.rs
git commit -m "feat(store): add memory context methods for unified memory system"
```

---

### Task 2: Router 迁移 — 从 Memory 切到 Store

**Files:**
- Modify: `crates/sage-core/src/router.rs`
- Modify: `crates/sage-core/src/daemon.rs`（Router 构造）

**Step 1: 读 router.rs 当前代码**

关键位置：
- `Router` struct 有 `memory: Memory` 字段
- `full_system_prompt()` 调用 `self.memory.as_context()`
- `handle_immediate()` 调用 `self.memory.record_decision()`
- `handle_scheduled()` 调用 `self.memory.record_decision()`
- `handle_background()` 调用 `self.memory.record_pattern()`
- `handle_normal()` 调用 `self.memory.record_pattern()`

**Step 2: 修改 Router**

```rust
// 1. 将 memory: Memory 替换为 store: Arc<Store>
// 2. full_system_prompt() 中：
//    self.memory.as_context() → self.store.get_memory_context(2000)?
// 3. handle_immediate/scheduled 中：
//    self.memory.record_decision() → self.store.append_decision()
// 4. handle_background/normal 中：
//    self.memory.record_pattern() → self.store.append_pattern()
```

**Step 3: 修改 daemon.rs 中 Router 构造**

移除 `Memory::new()` 调用，Router 直接使用已有的 `Arc<Store>`

**Step 4: 构建确认编译通过**

Run: `cargo build -p sage-core`
Expected: 编译通过

**Step 5: 运行全量测试**

Run: `cargo test --workspace`
Expected: 全部通过

**Step 6: Commit**

```bash
git add crates/sage-core/src/router.rs crates/sage-core/src/daemon.rs
git commit -m "refactor(router): migrate from markdown memory to SQLite store"
```

---

### Task 3: Coach/Mirror/Questioner 迁移

**Files:**
- Modify: `crates/sage-core/src/coach.rs`
- Modify: `crates/sage-core/src/mirror.rs`
- Modify: `crates/sage-core/src/questioner.rs`

**Step 1: Coach 迁移**

当前：`learn(agent, store, memory)` — 读 observations → LLM → 覆写 `sage.md`

改为：`learn(agent, store)` — 读 observations → LLM → `store.save_coach_insight(insight)`

移除 `memory` 参数，删除 `memory.write_file("sage.md", ...)` 调用。

**Step 2: Mirror 迁移**

当前：`reflect(agent, store, memory)` — 读 `sage.md` → LLM → 通知

改为：`reflect(agent, store)` — 读 `store.search_memories("coach_insight", 5)` → LLM → 通知

移除 `memory` 参数和 `memory.read_file("sage.md")` 调用。

**Step 3: Questioner 迁移**

当前：`ask(agent, store, memory)` — 读 `sage.md` + `decisions.md` → LLM → 保存

改为：`ask(agent, store)` — 读 coach_insight + decision 记忆 → LLM → 保存

移除 `memory` 参数。

**Step 4: 更新 daemon.rs/router.rs 中的调用签名**

所有 `run_coach()`、`run_mirror()`、`run_questioner()` 调用去掉 memory 参数。

**Step 5: 标记 memory.rs 为 deprecated**

在 `memory.rs` 顶部加 `#[deprecated(note = "Use Store methods instead")]`
保留文件，不删除（向后兼容现有 `~/.sage/memory/` 目录）。

**Step 6: 构建 + 测试**

Run: `cargo test --workspace`
Expected: 全部通过

**Step 7: Commit**

```bash
git add crates/sage-core/src/coach.rs crates/sage-core/src/mirror.rs \
       crates/sage-core/src/questioner.rs crates/sage-core/src/router.rs \
       crates/sage-core/src/daemon.rs crates/sage-core/src/memory.rs
git commit -m "refactor(memory): unify coach/mirror/questioner to use SQLite store"
```

---

## 工作流 B：Provider 弹性

### Task 4: ReliableProvider — 重试 + 回退

**Files:**
- Create: `crates/sage-core/src/reliable_provider.rs`
- Modify: `crates/sage-core/src/lib.rs`（pub mod）
- Modify: `crates/sage-core/src/agent.rs`（使用 ReliableProvider 包装）

**Step 1: 写失败测试**

```rust
// 在 reliable_provider.rs 底部
#[cfg(test)]
mod tests {
    use super::*;

    struct FailNProvider { fail_count: Mutex<usize>, max_fails: usize }
    #[async_trait::async_trait]
    impl LlmProvider for FailNProvider {
        fn name(&self) -> &str { "fail-n" }
        async fn invoke(&self, prompt: &str, _sys: Option<&str>) -> Result<String> {
            let mut count = self.fail_count.lock().unwrap();
            if *count < self.max_fails {
                *count += 1;
                Err(anyhow::anyhow!("server error 500"))
            } else {
                Ok(format!("ok: {prompt}"))
            }
        }
    }

    struct AlwaysFailProvider;
    #[async_trait::async_trait]
    impl LlmProvider for AlwaysFailProvider {
        fn name(&self) -> &str { "always-fail" }
        async fn invoke(&self, _p: &str, _s: Option<&str>) -> Result<String> {
            Err(anyhow::anyhow!("permanent failure"))
        }
    }

    #[tokio::test]
    async fn test_retry_then_succeed() {
        let provider = FailNProvider {
            fail_count: Mutex::new(0), max_fails: 2,
        };
        let reliable = ReliableProvider::new(Box::new(provider))
            .max_retries(3)
            .base_delay(Duration::from_millis(10));
        let result = reliable.invoke("test", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fallback_on_exhaustion() {
        let primary = AlwaysFailProvider;
        let fallback = FailNProvider {
            fail_count: Mutex::new(0), max_fails: 0,
        };
        let reliable = ReliableProvider::new(Box::new(primary))
            .max_retries(1)
            .base_delay(Duration::from_millis(10))
            .fallback(Box::new(fallback));
        let result = reliable.invoke("test", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_all_fail() {
        let reliable = ReliableProvider::new(Box::new(AlwaysFailProvider))
            .max_retries(2)
            .base_delay(Duration::from_millis(10));
        let result = reliable.invoke("test", None).await;
        assert!(result.is_err());
    }
}
```

**Step 2: 运行确认失败**

Run: `cargo test -p sage-core -- reliable_provider::tests -v`
Expected: FAIL — 模块不存在

**Step 3: 实现 ReliableProvider**

```rust
use crate::provider::LlmProvider;
use anyhow::Result;
use std::sync::Mutex;
use std::time::Duration;

pub struct ReliableProvider {
    primary: Box<dyn LlmProvider>,
    fallback: Option<Box<dyn LlmProvider>>,
    max_retries: usize,
    base_delay: Duration,
}

impl ReliableProvider {
    pub fn new(primary: Box<dyn LlmProvider>) -> Self {
        Self {
            primary,
            fallback: None,
            max_retries: 3,
            base_delay: Duration::from_secs(1),
        }
    }

    pub fn max_retries(mut self, n: usize) -> Self {
        self.max_retries = n;
        self
    }

    pub fn base_delay(mut self, d: Duration) -> Self {
        self.base_delay = d;
        self
    }

    pub fn fallback(mut self, f: Box<dyn LlmProvider>) -> Self {
        self.fallback = Some(f);
        self
    }
}

#[async_trait::async_trait]
impl LlmProvider for ReliableProvider {
    fn name(&self) -> &str { self.primary.name() }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            match self.primary.invoke(prompt, system_prompt).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_err = Some(e);
                    if attempt < self.max_retries {
                        let delay = self.base_delay * 2u32.pow(attempt as u32);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        // Primary exhausted → try fallback
        if let Some(ref fb) = self.fallback {
            return fb.invoke(prompt, system_prompt).await;
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("all retries exhausted")))
    }
}
```

**Step 4: 注册模块**

在 `lib.rs` 添加 `pub mod reliable_provider;`

**Step 5: 运行测试**

Run: `cargo test -p sage-core -- reliable_provider::tests -v`
Expected: PASS

**Step 6: 集成到 Agent**

修改 `agent.rs` 中 `Agent::with_provider()`，包装 `ReliableProvider`：

```rust
// agent.rs 中加 discovery 流程自动包装
pub fn with_reliable_provider(
    primary: Box<dyn LlmProvider>,
    fallback: Option<Box<dyn LlmProvider>>,
) -> Self {
    let mut reliable = ReliableProvider::new(primary);
    if let Some(fb) = fallback {
        reliable = reliable.fallback(fb);
    }
    Self {
        config: AgentConfig::default(),
        provider: Box::new(reliable),
    }
}
```

**Step 7: Commit**

```bash
git add crates/sage-core/src/reliable_provider.rs crates/sage-core/src/lib.rs \
       crates/sage-core/src/agent.rs
git commit -m "feat(provider): add ReliableProvider with retry and fallback"
```

---

### Task 5: RouterProvider — Hint 路由

**Files:**
- Create: `crates/sage-core/src/router_provider.rs`
- Modify: `crates/sage-core/src/lib.rs`
- Modify: `crates/sage-core/src/config.rs`（添加 model_routes 配置）

**Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider { model_name: String }
    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> &str { &self.model_name }
        async fn invoke(&self, _p: &str, _s: Option<&str>) -> Result<String> {
            Ok(format!("from:{}", self.model_name))
        }
    }

    #[tokio::test]
    async fn test_hint_routing() {
        let mut rp = HintRouter::new(Box::new(MockProvider { model_name: "default".into() }));
        rp.add_route("reasoning", Box::new(MockProvider { model_name: "opus".into() }));
        rp.add_route("fast", Box::new(MockProvider { model_name: "haiku".into() }));

        let r1 = rp.invoke("hint:reasoning\nsolve this", None).await.unwrap();
        assert!(r1.contains("opus"));

        let r2 = rp.invoke("hint:fast\nsummarize", None).await.unwrap();
        assert!(r2.contains("haiku"));

        let r3 = rp.invoke("no hint here", None).await.unwrap();
        assert!(r3.contains("default"));
    }

    #[tokio::test]
    async fn test_strip_hint_from_prompt() {
        let rp = HintRouter::new(Box::new(MockProvider { model_name: "default".into() }));
        // 默认 provider 收到的 prompt 不应包含 hint: 前缀
        let r = rp.invoke("hint:fast\nreal prompt", None).await.unwrap();
        assert!(r.contains("default"));
    }
}
```

**Step 2: 实现 HintRouter**

```rust
use crate::provider::LlmProvider;
use anyhow::Result;
use std::collections::HashMap;

pub struct HintRouter {
    default: Box<dyn LlmProvider>,
    routes: HashMap<String, Box<dyn LlmProvider>>,
}

impl HintRouter {
    pub fn new(default: Box<dyn LlmProvider>) -> Self {
        Self { default, routes: HashMap::new() }
    }

    pub fn add_route(&mut self, hint: &str, provider: Box<dyn LlmProvider>) {
        self.routes.insert(hint.to_string(), provider);
    }

    fn parse_hint(prompt: &str) -> (Option<&str>, &str) {
        if let Some(rest) = prompt.strip_prefix("hint:") {
            if let Some(newline_pos) = rest.find('\n') {
                let hint = rest[..newline_pos].trim();
                let real_prompt = rest[newline_pos + 1..].trim_start();
                (Some(hint), real_prompt)
            } else {
                (Some(rest.trim()), "")
            }
        } else {
            (None, prompt)
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for HintRouter {
    fn name(&self) -> &str { "hint-router" }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let (hint, real_prompt) = Self::parse_hint(prompt);
        let provider = match hint {
            Some(h) => self.routes.get(h).unwrap_or(&self.default),
            None => &self.default,
        };
        provider.invoke(real_prompt, system_prompt).await
    }
}
```

**Step 3: 运行测试**

Run: `cargo test -p sage-core -- router_provider::tests -v`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/sage-core/src/router_provider.rs crates/sage-core/src/lib.rs
git commit -m "feat(provider): add HintRouter for task-based model routing"
```

---

## 工作流 C：Agent 护栏

### Task 6: Agent Loop 迭代限制

**Files:**
- Modify: `crates/sage-core/src/agent.rs`

**Step 1: 写失败测试**

```rust
#[test]
fn test_agent_config_defaults() {
    let config = AgentConfig::default();
    assert_eq!(config.max_iterations, 10);
    assert_eq!(config.max_budget_usd, 1.0);
}
```

**Step 2: 添加 max_iterations 字段**

在 `sage-types` 的 `AgentConfig` 或 `agent.rs` 的 config 中添加：

```rust
// 如果在 config.rs 的 AgentConfig：
pub max_iterations: usize,  // 默认 10
```

修改 `Default` impl 设置默认值。

**Step 3: 在 daemon.rs 的事件循环中检查**

目前 daemon 每 tick 逐个处理 events，没有无限循环风险。
但 Agent::invoke 可能被循环调用。在 Agent 中加计数器：

```rust
pub struct Agent {
    config: AgentConfig,
    provider: Box<dyn LlmProvider>,
    invocation_count: AtomicUsize,  // 当前 tick 内调用计数
}

impl Agent {
    pub fn reset_counter(&self) { self.invocation_count.store(0, Ordering::Relaxed); }

    pub async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<AgentResponse> {
        let count = self.invocation_count.fetch_add(1, Ordering::Relaxed);
        if count >= self.config.max_iterations {
            return Err(anyhow::anyhow!("max iterations ({}) reached", self.config.max_iterations));
        }
        let text = self.provider.invoke(prompt, system_prompt).await?;
        Ok(AgentResponse { text })
    }
}
```

**Step 4: 运行测试**

Run: `cargo test -p sage-core -v`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/sage-core/src/agent.rs crates/sage-core/src/config.rs
git commit -m "feat(agent): add max_iterations guard to prevent runaway loops"
```

---

### Task 7: 历史压缩

**Files:**
- Modify: `crates/sage-core/src/store.rs`（添加 compress_session_history）
- Modify: `apps/sage-desktop/src-tauri/src/commands.rs`（chat 命令中调用）

**Step 1: 写失败测试**

```rust
#[test]
fn test_compress_history() {
    let store = Store::open_in_memory().unwrap();
    let sid = "test-session";
    // 插入 25 条消息
    for i in 0..25 {
        store.save_chat_message("user", &format!("msg {i}"), sid).unwrap();
        store.save_chat_message("sage", &format!("reply {i}"), sid).unwrap();
    }
    // 50 条消息
    let all = store.load_session_messages(sid).unwrap();
    assert_eq!(all.len(), 50);

    // 压缩：保留最近 20 条
    let recent = store.get_recent_messages_for_prompt(sid, 20).unwrap();
    assert_eq!(recent.len(), 20);
    // 最新的消息在列表中
    assert!(recent.last().unwrap().content.contains("reply 24"));
}
```

**Step 2: 实现**

```rust
/// 获取最近 N 条消息用于 prompt 构建
/// 超出的旧消息不删除，只是不加载到 prompt
pub fn get_recent_messages_for_prompt(&self, session_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
    let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁: {e}"))?;
    let mut stmt = conn.prepare(
        "SELECT id, role, content, session_id, created_at
         FROM chat_messages
         WHERE session_id = ?1
         ORDER BY id DESC
         LIMIT ?2"
    )?;
    let rows = stmt.query_map(rusqlite::params![session_id, limit as i64], |row| {
        Ok(ChatMessage {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            session_id: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut msgs: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
    msgs.reverse(); // 恢复时间顺序
    Ok(msgs)
}
```

**Step 3: 在 chat 命令中使用**

将 `commands.rs` 中 `load_session_messages(session_id)` 替换为 `get_recent_messages_for_prompt(session_id, 20)`

**Step 4: 运行测试**

Run: `cargo test -p sage-core -- store::tests::test_compress_history -v`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/sage-core/src/store.rs apps/sage-desktop/src-tauri/src/commands.rs
git commit -m "feat(chat): add history windowing to prevent unbounded context growth"
```

---

### Task 8: 凭证脱敏

**Files:**
- Create: `crates/sage-core/src/scrub.rs`
- Modify: `crates/sage-core/src/lib.rs`

**Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_api_key() {
        let input = "use key sk-ant-abc123xyz to call API";
        let scrubbed = scrub_credentials(input);
        assert!(!scrubbed.contains("sk-ant-abc123xyz"));
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc";
        let scrubbed = scrub_credentials(input);
        assert!(!scrubbed.contains("eyJ"));
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn test_no_false_positive() {
        let input = "normal text without any secrets";
        let scrubbed = scrub_credentials(input);
        assert_eq!(input, scrubbed);
    }
}
```

**Step 2: 实现**

```rust
use regex::RegexSet;
use std::sync::LazyLock;

static CREDENTIAL_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
    vec![
        regex::Regex::new(r"sk-[a-zA-Z0-9_-]{20,}").unwrap(),          // Anthropic/OpenAI keys
        regex::Regex::new(r"Bearer\s+[a-zA-Z0-9._-]{20,}").unwrap(),   // Bearer tokens
        regex::Regex::new(r"api[_-]?key[=:]\s*['\"]?[a-zA-Z0-9_-]{16,}").unwrap(), // generic api_key=
        regex::Regex::new(r"password[=:]\s*['\"]?[^\s'\"]{8,}").unwrap(), // password=
    ]
});

pub fn scrub_credentials(input: &str) -> String {
    let mut result = input.to_string();
    for pat in CREDENTIAL_PATTERNS.iter() {
        result = pat.replace_all(&result, "[REDACTED]").to_string();
    }
    result
}
```

**Step 3: 在 Agent::invoke 中调用**

```rust
// agent.rs invoke() 中，在存储/日志记录前脱敏
let scrubbed = scrub::scrub_credentials(&text);
// 日志用 scrubbed，返回原文给用户
```

**Step 4: 添加 regex 依赖**

在 `crates/sage-core/Cargo.toml` 中添加 `regex = "1"`（若尚未有）

**Step 5: 运行测试**

Run: `cargo test -p sage-core -- scrub::tests -v`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/sage-core/src/scrub.rs crates/sage-core/src/lib.rs \
       crates/sage-core/src/agent.rs crates/sage-core/Cargo.toml
git commit -m "feat(security): add credential scrubbing for LLM responses"
```

---

## 验证

### 最终集成测试

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### 验收标准

- [ ] `memory.rs` 标记 deprecated，所有消费者切到 Store
- [ ] Router/Coach/Mirror/Questioner 不再引用 markdown 文件
- [ ] ReliableProvider：3 次重试 + 自动回退到备用 provider
- [ ] HintRouter：`hint:reasoning` / `hint:fast` 可路由不同模型
- [ ] Agent 调用次数上限：默认 10 次/tick
- [ ] Chat 历史窗口：默认保留最近 20 条
- [ ] 凭证脱敏：sk-*, Bearer *, api_key=* 在日志中被替换
- [ ] 全量测试通过，clippy 零警告
