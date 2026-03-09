# Smart Reports — 定时自动产出 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 让 Sage daemon 的 4 个定时产出（Morning Brief, Evening Review, Weekly Report, Week Start）从泛泛的占位内容变成数据驱动的高质量报告，展示在 Desktop App Dashboard 上。

**Architecture:** 新增 `context_gatherer.rs` 模块在 LLM 调用前收集上下文（本周 sessions、memories、项目/团队文件）。新增 `reports` SQLite 表存储结构化报告（与 suggestions 分离）。Dashboard 增加报告卡片展示区。

**Tech Stack:** Rust (sage-core), SQLite, React/TypeScript (Tauri Desktop)

---

## Task 1: reports 表 — SQLite schema + CRUD

**Files:**
- Modify: `crates/sage-core/src/store.rs`
- Test: `crates/sage-core/src/store.rs` (内联测试)

**Step 1: 写测试**

```rust
#[test]
fn test_save_and_load_report() {
    let store = make_test_store();
    store.save_report("weekly", "本周报告内容").unwrap();
    store.save_report("weekly", "更新的周报").unwrap();
    store.save_report("morning", "早间 brief").unwrap();

    let latest = store.get_latest_report("weekly").unwrap();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().content, "更新的周报");

    let all = store.get_reports("weekly", 10).unwrap();
    assert_eq!(all.len(), 2);
}
```

**Step 2: 运行测试确认失败**

```bash
cargo test -p sage-core -- test_save_and_load_report
```
Expected: FAIL — `save_report` method not found

**Step 3: 实现**

在 `store.rs` 的 `ensure_schema()` 中追加建表：

```sql
CREATE TABLE IF NOT EXISTS reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    report_type TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_reports_type_date ON reports(report_type, created_at DESC);
```

在 `sage-types/src/lib.rs` 中添加 `Report` 结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: i64,
    pub report_type: String,
    pub content: String,
    pub created_at: String,
}
```

在 `store.rs` 中实现：

```rust
pub fn save_report(&self, report_type: &str, content: &str) -> Result<i64> {
    let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    let now = chrono::Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO reports (report_type, content, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![report_type, content, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_latest_report(&self, report_type: &str) -> Result<Option<Report>> {
    let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    conn.query_row(
        "SELECT id, report_type, content, created_at FROM reports WHERE report_type = ?1 ORDER BY created_at DESC LIMIT 1",
        rusqlite::params![report_type],
        |row| Ok(Report { id: row.get(0)?, report_type: row.get(1)?, content: row.get(2)?, created_at: row.get(3)? }),
    ).optional().map_err(Into::into)
}

pub fn get_reports(&self, report_type: &str, limit: usize) -> Result<Vec<Report>> {
    // 标准 query_map 模式
}
```

**Step 4: 运行测试确认通过**

```bash
cargo test -p sage-core -- test_save_and_load_report
```
Expected: PASS

**Step 5: Commit**

```bash
git add crates/sage-types/src/lib.rs crates/sage-core/src/store.rs
git commit -m "feat: reports table — save/load structured daemon reports"
```

---

## Task 2: context_gatherer — 为每种报告收集上下文

**Files:**
- Create: `crates/sage-core/src/context_gatherer.rs`
- Modify: `crates/sage-core/src/lib.rs` (注册模块)
- Modify: `crates/sage-core/src/store.rs` (新增时间范围查询)
- Test: `crates/sage-core/src/context_gatherer.rs` (内联测试)

**Step 1: store.rs 新增时间范围查询**

```rust
/// 获取某个日期之后创建的 memories（用于报告上下文收集）
pub fn get_memories_since(&self, since: &str) -> Result<Vec<Memory>> {
    // SELECT * FROM memories WHERE created_at >= ?1 ORDER BY created_at DESC
}

/// 获取某个日期之后的 observations 数量
pub fn count_observations_since(&self, since: &str) -> Result<usize> {
    // SELECT COUNT(*) FROM observations WHERE created_at >= ?1
}

/// 获取某个日期之后的 session 类 memories
pub fn get_session_summaries_since(&self, since: &str) -> Result<Vec<Memory>> {
    // SELECT * FROM memories WHERE category = 'session' AND created_at >= ?1
}

/// 获取某个日期之后的 coach insights
pub fn get_coach_insights_since(&self, since: &str) -> Result<Vec<String>> {
    // SELECT content FROM coach_insights WHERE created_at >= ?1
}
```

**Step 2: 写 context_gatherer.rs 测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gather_morning_brief_returns_structured_context() {
        let store = make_test_store();
        // 插入一些测试 memories
        store.save_memory("identity", "Evan is a team lead", "chat", 0.9).unwrap();
        store.save_memory("decision", "chose Rust for EMS", "chat", 0.8).unwrap();

        let ctx = gather(ReportType::MorningBrief, &store);
        assert!(ctx.contains("决策"));  // 应包含近期决策
    }

    #[test]
    fn test_gather_weekly_includes_sessions() {
        let store = make_test_store();
        store.save_memory("session", "[session] fix bugs — 50 msgs", "claude-code", 0.8).unwrap();

        let ctx = gather(ReportType::WeeklyReport, &store);
        assert!(ctx.contains("session"));
    }
}
```

**Step 3: 实现 context_gatherer.rs**

```rust
use crate::store::Store;

pub enum ReportType {
    MorningBrief,
    EveningReview,
    WeeklyReport,
    WeekStart,
}

/// 为指定报告类型收集上下文，返回格式化的 Markdown 文本块
pub fn gather(report_type: ReportType, store: &Store) -> String {
    match report_type {
        ReportType::MorningBrief => gather_morning(store),
        ReportType::EveningReview => gather_evening(store),
        ReportType::WeeklyReport => gather_weekly(store),
        ReportType::WeekStart => gather_week_start(store),
    }
}
```

每个 gather 函数的数据源：

| 函数 | 数据源 |
|------|--------|
| `gather_morning` | 近期决策 memories + 上次 evening review report |
| `gather_evening` | 今日 session memories + 今日 observations 数量 + 今日 coach insights |
| `gather_weekly` | 本周 session memories + 本周所有 memories + 本周 coach insights + `.context/projects.md` + `.context/team.md` |
| `gather_week_start` | 上周 weekly report + `.context/projects.md` |

**读文件策略**：`gather_weekly` 和 `gather_week_start` 读 `.context/projects.md` 和 `.context/team.md`。路径从环境变量 `SAGE_PROJECT_DIR` 获取，默认 `~/dev/digital-twin`。文件不存在时静默跳过。

**Step 4: 运行测试**

```bash
cargo test -p sage-core -- context_gatherer
```
Expected: PASS

**Step 5: Commit**

```bash
git add crates/sage-core/src/context_gatherer.rs crates/sage-core/src/lib.rs crates/sage-core/src/store.rs
git commit -m "feat: context_gatherer — collect rich data for each report type"
```

---

## Task 3: router.rs — 注入上下文 + 保存报告

**Files:**
- Modify: `crates/sage-core/src/router.rs`

**Step 1: 修改 handle_scheduled 注入 context_gatherer 输出**

在 `handle_scheduled` 中，根据 `event.title` 确定 `ReportType`，调用 `context_gatherer::gather()` 获取上下文，注入到 prompt 中：

```rust
async fn handle_scheduled(&self, event: Event) -> Result<()> {
    let system = self.full_system_prompt();

    // 收集上下文
    let report_type = match event.title.as_str() {
        "Morning Brief" => Some(context_gatherer::ReportType::MorningBrief),
        "Evening Review" => Some(context_gatherer::ReportType::EveningReview),
        "Weekly Report" => Some(context_gatherer::ReportType::WeeklyReport),
        "Week Start" => Some(context_gatherer::ReportType::WeekStart),
        _ => None,
    };

    let context = report_type.as_ref()
        .map(|rt| context_gatherer::gather(rt, &self.store))
        .unwrap_or_default();

    let prompt = match event.title.as_str() {
        "Morning Brief" => format!(
            "现在是早间 briefing 时间。\n\n## 可用数据\n{context}\n\n请生成今日 Morning Brief：\n1. 今日重点关注事项\n2. 待决策/待跟进事项\n3. 建议优先级排序\n\n用 Markdown 格式，简洁有结构。"
        ),
        // ... 其他类型同理
    };

    // ... 现有的去重/LLM 调用/通知逻辑 ...

    // 额外：保存到 reports 表
    if let Some(rt) = &report_type {
        let type_str = match rt {
            context_gatherer::ReportType::MorningBrief => "morning",
            context_gatherer::ReportType::EveningReview => "evening",
            context_gatherer::ReportType::WeeklyReport => "weekly",
            context_gatherer::ReportType::WeekStart => "week_start",
        };
        let _ = self.store.save_report(type_str, &resp.text);
    }

    Ok(())
}
```

**Step 2: 验证编译**

```bash
cargo check -p sage-core
```

**Step 3: Commit**

```bash
git add crates/sage-core/src/router.rs
git commit -m "feat: inject gathered context into scheduled report prompts"
```

---

## Task 4: Tauri command — get_reports

**Files:**
- Modify: `apps/sage-desktop/src-tauri/src/commands.rs`
- Modify: `apps/sage-desktop/src-tauri/src/main.rs`

**Step 1: 添加 Tauri 命令**

```rust
#[tauri::command]
pub fn get_reports(
    state: tauri::State<'_, crate::AppState>,
    report_type: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<sage_types::Report>, String> {
    let limit = limit.unwrap_or(10);
    match report_type {
        Some(rt) => state.store.get_reports(&rt, limit).map_err(|e| e.to_string()),
        None => state.store.get_all_reports(limit).map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub fn get_latest_reports(
    state: tauri::State<'_, crate::AppState>,
) -> Result<std::collections::HashMap<String, sage_types::Report>, String> {
    // 返回每种类型的最新一条报告
    let types = ["morning", "evening", "weekly", "week_start"];
    let mut map = std::collections::HashMap::new();
    for t in types {
        if let Ok(Some(r)) = state.store.get_latest_report(t) {
            map.insert(t.to_string(), r);
        }
    }
    Ok(map)
}
```

**Step 2: 注册到 invoke_handler**

在 `main.rs` 的 `generate_handler!` 宏中添加 `commands::get_reports` 和 `commands::get_latest_reports`。

**Step 3: 验证编译**

```bash
cargo check -p sage-desktop
```

**Step 4: Commit**

```bash
git add apps/sage-desktop/src-tauri/src/commands.rs apps/sage-desktop/src-tauri/src/main.rs
git commit -m "feat: Tauri commands for report retrieval"
```

---

## Task 5: Dashboard UI — 报告展示

**Files:**
- Modify: `apps/sage-desktop/src/pages/Dashboard.tsx`
- Modify: `apps/sage-desktop/src/App.css`

**Step 1: Dashboard 新增报告区域**

在 Dashboard 的 stat cards 下方、suggestions 上方，添加一个报告区域：

```tsx
interface Report {
  id: number;
  report_type: string;
  content: string;
  created_at: string;
}

// 在 Dashboard 组件中新增 state
const [reports, setReports] = useState<Record<string, Report>>({});

// useEffect 加载最新报告
useEffect(() => {
  invoke<Record<string, Report>>("get_latest_reports")
    .then(setReports)
    .catch(console.error);
}, []);
```

报告展示为可折叠卡片：

```tsx
const REPORT_LABELS: Record<string, string> = {
  morning: "Morning Brief",
  evening: "Evening Review",
  weekly: "Weekly Report",
  week_start: "Week Start",
};

{Object.entries(reports).length > 0 && (
  <div className="reports-section">
    <span className="card-title">Reports</span>
    {Object.entries(reports).map(([type, report]) => (
      <details key={type} className="report-card" open={type === "weekly" || type === "morning"}>
        <summary className="report-header">
          <span className="report-type">{REPORT_LABELS[type] ?? type}</span>
          <span className="report-time">{formatTime(report.created_at)}</span>
        </summary>
        <div className="report-body">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{report.content}</ReactMarkdown>
        </div>
      </details>
    ))}
  </div>
)}
```

**Step 2: CSS 样式**

```css
.reports-section { margin-bottom: 24px; }
.report-card {
  background: var(--card-bg);
  border-radius: 12px;
  margin-bottom: 8px;
  overflow: hidden;
}
.report-header {
  padding: 12px 16px;
  cursor: pointer;
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.report-type { font-weight: 600; }
.report-time { opacity: 0.5; font-size: 0.85em; }
.report-body { padding: 0 16px 16px; }
```

**Step 3: 验证编译 + 手动测试**

```bash
cd apps/sage-desktop && npm run build
cargo build -p sage-desktop
```

**Step 4: Commit**

```bash
git add apps/sage-desktop/src/pages/Dashboard.tsx apps/sage-desktop/src/App.css
git commit -m "feat: Dashboard report cards — display latest scheduled reports"
```

---

## Task 6: 端到端验证

**Step 1: 手动触发一次报告生成**

在 `commands.rs` 中添加一个临时/永久的 `trigger_report` 命令，或者通过 `sage-ingest` 扩展，让我们能在开发时手动触发报告生成（不用等到周五下午）。

最简方案：在 `store.rs` 中直接插入一条测试报告：

```rust
#[tauri::command]
pub fn trigger_test_report(
    state: tauri::State<'_, crate::AppState>,
    report_type: String,
) -> Result<String, String> {
    // 收集上下文
    let ctx = sage_core::context_gatherer::gather(
        match report_type.as_str() {
            "morning" => sage_core::context_gatherer::ReportType::MorningBrief,
            "evening" => sage_core::context_gatherer::ReportType::EveningReview,
            "weekly" => sage_core::context_gatherer::ReportType::WeeklyReport,
            "week_start" => sage_core::context_gatherer::ReportType::WeekStart,
            _ => return Err("unknown type".into()),
        },
        &state.store,
    );

    // 不调 LLM，直接保存 context 作为报告预览
    state.store.save_report(&report_type, &format!("## Context Preview\n\n{ctx}"))
        .map_err(|e| e.to_string())?;
    Ok(format!("Test report '{}' generated", report_type))
}
```

**Step 2: 在 Desktop App 测试**

1. 打开 App → Dashboard
2. 调用 trigger_test_report 验证报告卡片显示
3. 检查报告内容包含 sessions/memories/projects 数据

**Step 3: 最终 commit**

```bash
git add -A
git commit -m "feat: smart reports — end-to-end scheduled report generation with context"
```

---

## 依赖关系

```
Task 1 (reports 表)
    ↓
Task 2 (context_gatherer) ← 依赖 Task 1 的时间范围查询
    ↓
Task 3 (router 注入) ← 依赖 Task 2
    ↓
Task 4 (Tauri 命令) ← 依赖 Task 1
    ↓
Task 5 (Dashboard UI) ← 依赖 Task 4
    ↓
Task 6 (端到端验证) ← 依赖全部
```

Task 1 和 Task 2 可串行快速完成（纯 Rust，有测试）。
Task 4 和 Task 5 可在 Task 1 完成后并行开发。
Task 3 依赖 Task 2 但改动小。
