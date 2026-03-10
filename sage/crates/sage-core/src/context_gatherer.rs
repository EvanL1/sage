//! context_gatherer — 为每种定时报告收集数据上下文
//!
//! 在 LLM 调用前，根据报告类型从 SQLite 和文件系统收集结构化数据，
//! 返回格式化的 Markdown 文本块供 prompt 注入使用。

use crate::session_analyzer;
use crate::store::Store;

pub enum ReportType {
    MorningBrief,
    EveningReview,
    WeeklyReport,
    WeekStart,
}

/// 为指定报告类型收集上下文，返回格式化的 Markdown 文本块
pub async fn gather(report_type: &ReportType, store: &Store) -> String {
    match report_type {
        ReportType::MorningBrief => gather_morning(store).await,
        ReportType::EveningReview => gather_evening(store).await,
        ReportType::WeeklyReport => gather_weekly(store),
        ReportType::WeekStart => gather_week_start(store),
    }
}

/// Morning Brief：邮件摘要 + 工作 session + Claude 记忆 + 决策 + 晚间回顾
async fn gather_morning(store: &Store) -> String {
    // 先 ingest 最近 24h 的 Claude Code session
    ingest_recent_sessions(store, 24);

    let mut sections = Vec::new();

    // 今日日历事件（会议安排）
    match crate::channels::calendar::scan_today_events().await {
        Ok(digest) if !digest.is_empty() => {
            sections.push(format!("## 今日日程\n{digest}"));
        }
        Err(e) => tracing::debug!("扫描日历失败: {e}"),
        _ => {}
    }

    // 扫描下班到上班之间所有邮件（不管已读未读，最多 14 小时）
    match crate::channels::email::scan_recent_emails(14).await {
        Ok(digest) if !digest.is_empty() => {
            sections.push(format!("## 近期邮件（含已读未细看的）\n{digest}"));
        }
        Err(e) => tracing::debug!("扫描邮件失败: {e}"),
        _ => {}
    }

    // 昨日/近期 Claude Code 工作 session（LLM 可从中提取今日待办）
    let since_1d = days_ago(1);
    let sessions = store.get_session_summaries_since(&since_1d).unwrap_or_default();
    if !sessions.is_empty() {
        let lines: Vec<String> = sessions.iter().map(|m| format!("- {}", m.content)).collect();
        sections.push(format!("## 近期工作 Sessions（从中提取今日待办）\n{}", lines.join("\n")));
    }

    // 读取 Claude Code MEMORY.md（含项目进展和待办）
    if let Some(content) = read_claude_memory() {
        sections.push(format!("## Claude Code 记忆\n{content}"));
    }

    // 读 .context/projects.md（项目状态和优先级）
    if let Some(content) = read_context_file("projects.md") {
        sections.push(format!("## 项目状态\n{content}"));
    }

    // 近 7 天的决策记忆
    let since_7d = days_ago(7);
    let memories = store.get_memories_since(&since_7d).unwrap_or_default();
    let decisions: Vec<_> = memories.iter().filter(|m| m.category == "decision").collect();
    if !decisions.is_empty() {
        let lines: Vec<String> = decisions.iter().map(|m| format!("- {}", m.content)).collect();
        sections.push(format!("## 近期决策\n{}", lines.join("\n")));
    }

    // 近 7 天的 coach insights
    let insights = store.get_coach_insights_since(&since_7d).unwrap_or_default();
    if !insights.is_empty() {
        let lines: Vec<String> = insights.iter().map(|s| format!("- {s}")).collect();
        sections.push(format!("## 教练洞察\n{}", lines.join("\n")));
    }

    // 上次 evening review 报告
    if let Ok(Some(report)) = store.get_latest_report("evening") {
        sections.push(format!("## 昨日晚间回顾\n{}", report.content));
    }

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

/// Evening Review：今日 session memories + 今日 observations 数量 + 今日 coach insights
async fn gather_evening(store: &Store) -> String {
    // 在收集上下文前，先从 Claude Code JSONL 文件中 ingest 最新的 session 数据
    ingest_recent_sessions(store, 24);

    let mut sections = Vec::new();

    // 今日日历事件（回顾会议情况）
    match crate::channels::calendar::scan_today_events().await {
        Ok(digest) if !digest.is_empty() => {
            sections.push(format!("## 今日会议\n{digest}"));
        }
        Err(e) => tracing::debug!("扫描日历失败: {e}"),
        _ => {}
    }

    let since = days_ago(1);

    // 今日 session summaries
    let sessions = store.get_session_summaries_since(&since).unwrap_or_default();
    if !sessions.is_empty() {
        let lines: Vec<String> = sessions.iter().map(|m| format!("- {}", m.content)).collect();
        sections.push(format!("## 今日工作 Sessions\n{}", lines.join("\n")));
    }

    // 今日 observations 数量
    let obs_count = store.count_observations_since(&since).unwrap_or(0);
    sections.push(format!("## 今日活动统计\n- 行为观察记录：{obs_count} 条"));

    // 今日 coach insights
    let insights = store.get_coach_insights_since(&since).unwrap_or_default();
    if !insights.is_empty() {
        let lines: Vec<String> = insights.iter().map(|s| format!("- {s}")).collect();
        sections.push(format!("## 今日教练洞察\n{}", lines.join("\n")));
    }

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

/// Weekly Report：本周所有 memories + sessions + coach insights + 项目/团队文件
fn gather_weekly(store: &Store) -> String {
    // 在收集上下文前，先 ingest 最新的 session 数据（覆盖一周）
    ingest_recent_sessions(store, 24 * 7);

    let mut sections = Vec::new();

    let since = days_ago(7);

    // 本周 session summaries
    let sessions = store.get_session_summaries_since(&since).unwrap_or_default();
    if !sessions.is_empty() {
        let lines: Vec<String> = sessions.iter().map(|m| format!("- {}", m.content)).collect();
        sections.push(format!("## 本周工作 Sessions\n{}", lines.join("\n")));
    }

    // 本周所有记忆（按 category 分组）
    let memories = store.get_memories_since(&since).unwrap_or_default();
    if !memories.is_empty() {
        let decision_lines: Vec<_> = memories.iter()
            .filter(|m| m.category == "decision")
            .map(|m| format!("- {}", m.content))
            .collect();
        if !decision_lines.is_empty() {
            sections.push(format!("## 本周决策\n{}", decision_lines.join("\n")));
        }

        let insight_lines: Vec<_> = memories.iter()
            .filter(|m| m.category == "coach_insight")
            .map(|m| format!("- {}", m.content))
            .collect();
        if !insight_lines.is_empty() {
            sections.push(format!("## 本周教练洞察\n{}", insight_lines.join("\n")));
        }
    }

    // 读 .context/projects.md
    if let Some(content) = read_context_file("projects.md") {
        sections.push(format!("## 项目状态\n{content}"));
    }

    // 读 .context/team.md
    if let Some(content) = read_context_file("team.md") {
        sections.push(format!("## 团队信息\n{content}"));
    }

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

/// Week Start：上周 weekly report + .context/projects.md
fn gather_week_start(store: &Store) -> String {
    let mut sections = Vec::new();

    // 上次 weekly report
    if let Ok(Some(report)) = store.get_latest_report("weekly") {
        sections.push(format!("## 上周周报\n{}", report.content));
    }

    // 读 .context/projects.md
    if let Some(content) = read_context_file("projects.md") {
        sections.push(format!("## 项目状态\n{content}"));
    }

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

/// 从 Claude Code 的 JSONL session 文件中 ingest 最新数据到 Store
fn ingest_recent_sessions(store: &Store, hours: i64) {
    let claude_dir = session_analyzer::default_claude_dir();
    if let Err(e) = session_analyzer::ingest_sessions(&claude_dir, store, hours) {
        tracing::warn!("Failed to ingest Claude Code sessions: {e}");
    }
}

/// 读取 Claude Code 各项目的 MEMORY.md，合并后截断到 3000 字
fn read_claude_memory() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let projects_dir = std::path::Path::new(&home).join(".claude/projects");

    let mut parts = Vec::new();

    // 全局 MEMORY.md
    let global = std::path::Path::new(&home).join(".claude/MEMORY.md");
    if let Ok(content) = std::fs::read_to_string(&global) {
        if !content.trim().is_empty() {
            parts.push(format!("### Global\n{content}"));
        }
    }

    // 各项目 MEMORY.md
    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let mem_path = entry.path().join("memory/MEMORY.md");
            if let Ok(content) = std::fs::read_to_string(&mem_path) {
                if content.trim().is_empty() {
                    continue;
                }
                let project_name = entry.file_name().to_string_lossy()
                    .replace('-', "/")
                    .trim_start_matches('/')
                    .to_string();
                // 每个项目截断到 800 字
                let truncated: String = content.chars().take(800).collect();
                parts.push(format!("### {project_name}\n{truncated}"));
            }
        }
    }

    if parts.is_empty() {
        return None;
    }

    // 合并后总体截断到 3000 字
    let combined = parts.join("\n\n");
    let result: String = combined.chars().take(3000).collect();
    Some(result)
}

/// 读取 .context/<filename>，路径从 SAGE_PROJECT_DIR 获取，默认 ~/dev/digital-twin
fn read_context_file(filename: &str) -> Option<String> {
    let project_dir = std::env::var("SAGE_PROJECT_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/dev/digital-twin")
    });
    let path = std::path::Path::new(&project_dir).join(".context").join(filename);
    std::fs::read_to_string(&path).ok()
}

/// 返回 N 天前的 RFC3339 时间戳字符串
fn days_ago(days: i64) -> String {
    (chrono::Local::now() - chrono::Duration::days(days)).to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_store() -> Store {
        Store::open_in_memory().unwrap()
    }

    #[tokio::test]
    async fn test_gather_morning_brief_returns_structured_context() {
        let store = make_test_store();
        store.save_memory("decision", "chose Rust for EMS", "chat", 0.8).unwrap();
        store.save_memory("coach_insight", "Evan 偏系统思考", "coach", 0.8).unwrap();

        let ctx = gather(&ReportType::MorningBrief, &store).await;
        assert!(ctx.contains("决策") || ctx.contains("chose Rust"), "应包含近期决策内容，实际: {ctx}");
    }

    #[tokio::test]
    async fn test_gather_weekly_includes_sessions() {
        let store = make_test_store();
        store.save_memory("session", "[session] fix bugs — 50 msgs", "claude-code", 0.8).unwrap();

        let ctx = gather(&ReportType::WeeklyReport, &store).await;
        assert!(ctx.contains("session") || ctx.contains("Session"), "应包含 session 信息，实际: {ctx}");
    }

    #[tokio::test]
    async fn test_gather_evening_review_has_stats() {
        let store = make_test_store();
        store.record_observation("pattern", "focused work", None).unwrap();

        let ctx = gather(&ReportType::EveningReview, &store).await;
        assert!(ctx.contains("统计") || ctx.contains("活动"), "应包含活动统计，实际: {ctx}");
    }

    #[tokio::test]
    async fn test_gather_week_start_includes_last_weekly_report() {
        let store = make_test_store();
        store.save_report("weekly", "上周完成了 PULSE 模块开发").unwrap();

        let ctx = gather(&ReportType::WeekStart, &store).await;
        assert!(ctx.contains("上周"), "应包含上周周报，实际: {ctx}");
    }

    #[tokio::test]
    async fn test_gather_empty_store_returns_empty_or_partial() {
        let store = make_test_store();
        // 空 store 时各类型不崩溃
        let _ = gather(&ReportType::MorningBrief, &store).await;
        let _ = gather(&ReportType::EveningReview, &store).await;
        let _ = gather(&ReportType::WeeklyReport, &store).await;
        let _ = gather(&ReportType::WeekStart, &store).await;
    }
}
