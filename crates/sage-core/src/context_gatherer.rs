//! context_gatherer — 为每种定时报告收集数据上下文
//!
//! 在 LLM 调用前，根据报告类型从 SQLite 记忆系统收集结构化数据，
//! 返回格式化的 Markdown 文本块供 prompt 注入使用。

use crate::store::Store;

pub enum ReportType {
    MorningBrief,
    EveningReview,
    WeeklyReport,
    WeekStart,
}

/// 为指定报告类型收集上下文，返回格式化的 Markdown 文本块
/// calendar_source: "outlook" / "apple" / "both"
/// lang: "zh" / "en"
pub async fn gather(
    report_type: &ReportType,
    store: &Store,
    calendar_source: &str,
    lang: &str,
) -> String {
    match report_type {
        ReportType::MorningBrief => gather_morning(store, calendar_source, lang).await,
        ReportType::EveningReview => gather_evening(store, calendar_source, lang).await,
        ReportType::WeeklyReport => gather_weekly(store, lang),
        ReportType::WeekStart => gather_week_start(store, lang),
    }
}

/// Pick Chinese or English string based on lang
fn sec<'a>(lang: &str, zh: &'a str, en: &'a str) -> &'a str {
    match lang {
        "en" => en,
        _ => zh,
    }
}

/// Morning Brief：邮件摘要 + 工作 session + Claude 记忆 + 决策 + 晚间回顾
async fn gather_morning(store: &Store, calendar_source: &str, lang: &str) -> String {

    let mut sections = Vec::new();

    // 今日日历事件（会议安排）— 这是唯一可信的会议来源
    let cal_header = sec(
        lang,
        "## 今日日程（日历数据，可信来源）",
        "## Today's Schedule (calendar data, trusted source)",
    );
    match crate::channels::calendar::scan_today_events(calendar_source).await {
        Ok(digest) if !digest.is_empty() => {
            sections.push(format!("{cal_header}\n{digest}"));
        }
        Ok(_) => {
            let no_mtg = sec(lang, "今天没有会议。", "No meetings today.");
            sections.push(format!("{cal_header}\n{no_mtg}"));
        }
        Err(e) => {
            tracing::warn!("扫描日历失败: {e}");
            let fallback_header = sec(lang, "## 今日日程", "## Today's Schedule");
            let fallback_msg = sec(
                lang,
                "日历扫描失败，无法确认今日会议。",
                "Calendar scan failed, unable to confirm today's meetings.",
            );
            sections.push(format!("{fallback_header}\n{fallback_msg}"));
        }
    }

    // 从 store 读取已入库的近期邮件（daemon tick 已 poll 并存入 messages 表）
    // 优先用 store 数据：信息已确认入库，不依赖二次 AppleScript 调用
    let email_msgs = store
        .get_messages_by_source("email", 15)
        .unwrap_or_default();
    if !email_msgs.is_empty() {
        // 只取最近 24 小时的
        let cutoff = (chrono::Local::now() - chrono::Duration::hours(24)).to_rfc3339();
        let recent: Vec<_> = email_msgs
            .iter()
            .filter(|m| m.timestamp > cutoff || m.created_at > cutoff)
            .filter(|m| m.action_state != "resolved" && m.action_state != "expired")
            .collect();
        if !recent.is_empty() {
            let lines: Vec<String> = recent
                .iter()
                .map(|m| {
                    let subject = &m.channel; // channel 字段存的是邮件 subject
                    let sender = &m.sender;
                    let ts = &m.timestamp; // 邮件原始时间戳
                    let preview: String = m
                        .content
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(300)
                        .collect();
                    if preview.is_empty() {
                        format!("- [{ts}] **{subject}** — {sender}")
                    } else {
                        format!("- [{ts}] **{subject}** — {sender}\n  > {preview}")
                    }
                })
                .collect();
            let email_header = if lang == "en" {
                format!("## Recent Emails ({} stored)", recent.len())
            } else {
                format!("## 近期邮件（{}封，已入库）", recent.len())
            };
            sections.push(format!("{email_header}\n{}", lines.join("\n")));
        }
    }
    // 备选：如果 store 无邮件数据，尝试 IMAP 直扫
    let email_marker = sec(lang, "近期邮件", "Recent Emails");
    if !sections.iter().any(|s| s.contains(email_marker)) {
        let fallback_email_header = sec(lang, "## 近期邮件（IMAP 直扫）", "## Recent Emails (IMAP scan)");
        let email_sources = store.get_message_sources_by_type("imap").unwrap_or_default();
        match crate::channels::email::scan_recent_emails(&email_sources, 14).await {
            Ok(digest) if !digest.is_empty() => {
                sections.push(format!("{fallback_email_header}\n{digest}"));
            }
            Err(e) => tracing::debug!("扫描邮件失败: {e}"),
            _ => {}
        }
    }

    // 昨日/近期 Claude Code 工作 session（LLM 可从中提取今日待办）
    let since_1d = days_ago(1);
    let sessions = store
        .get_session_summaries_since(&since_1d)
        .unwrap_or_default();
    if !sessions.is_empty() {
        let lines: Vec<String> = sessions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let sess_header = sec(
            lang,
            "## 近期工作 Sessions（从中提取今日待办）",
            "## Recent Work Sessions (extract today's to-dos from these)",
        );
        sections.push(format!("{sess_header}\n{}", lines.join("\n")));
    }

    // 读取 Claude Code MEMORY.md（含项目进展和待办）
    if let Some(content) = read_claude_memory() {
        let mem_header = sec(lang, "## Claude Code 记忆", "## Claude Code Memories");
        sections.push(format!("{mem_header}\n{content}"));
    }

    // 从记忆中查询项目相关信息
    let project_memories = store
        .search_memories("project status priority", 10)
        .unwrap_or_default();
    if !project_memories.is_empty() {
        let lines: Vec<String> = project_memories
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let proj_header = sec(lang, "## 项目相关记忆", "## Project Memories");
        sections.push(format!("{proj_header}\n{}", lines.join("\n")));
    }

    // 近 7 天的决策记忆
    let since_7d = days_ago(7);
    let memories = store.get_memories_since(&since_7d).unwrap_or_default();
    let decisions: Vec<_> = memories
        .iter()
        .filter(|m| m.category == "decision")
        .collect();
    if !decisions.is_empty() {
        let lines: Vec<String> = decisions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let dec_header = sec(lang, "## 近期决策", "## Recent Decisions");
        sections.push(format!("{dec_header}\n{}", lines.join("\n")));
    }

    // 近 7 天的 coach insights
    let insights = store
        .get_coach_insights_since(&since_7d)
        .unwrap_or_default();
    if !insights.is_empty() {
        let lines: Vec<String> = insights.iter().map(|s| format!("- {s}")).collect();
        let coach_header = sec(lang, "## 教练洞察", "## Coach Insights");
        sections.push(format!("{coach_header}\n{}", lines.join("\n")));
    }

    // 上次 evening review 报告（仅 3 天内有效，避免注入过时回顾）
    if let Ok(Some(report)) = store.get_latest_report("evening") {
        let three_days_ago = (chrono::Local::now() - chrono::Duration::days(3)).to_rfc3339();
        if report.created_at > three_days_ago {
            let eve_header = sec(lang, "## 昨日晚间回顾", "## Yesterday's Evening Review");
            sections.push(format!("{eve_header}\n{}", report.content));
        }
    }

    // Feed 信息洞察（过去 24h 高分项）
    if let Some(feed_section) = build_feed_section(store, 24, true, lang) {
        sections.push(feed_section);
    }

    inject_corrections(store, "morning", &mut sections, lang);

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

/// Evening Review：今日 session memories + 今日 observations 数量 + 今日 coach insights
async fn gather_evening(store: &Store, calendar_source: &str, lang: &str) -> String {

    let mut sections = Vec::new();

    // 今日日历事件（回顾会议情况）
    match crate::channels::calendar::scan_today_events(calendar_source).await {
        Ok(digest) if !digest.is_empty() => {
            let mtg_header = sec(lang, "## 今日会议", "## Today's Meetings");
            sections.push(format!("{mtg_header}\n{digest}"));
        }
        Err(e) => tracing::debug!("扫描日历失败: {e}"),
        _ => {}
    }

    let since = days_ago(1);

    // 今日 session summaries
    let sessions = store
        .get_session_summaries_since(&since)
        .unwrap_or_default();
    if !sessions.is_empty() {
        let lines: Vec<String> = sessions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let sess_header = sec(lang, "## 今日工作 Sessions", "## Today's Work Sessions");
        sections.push(format!("{sess_header}\n{}", lines.join("\n")));
    }

    // 今日 observations 数量
    let obs_count = store.count_observations_since(&since).unwrap_or(0);
    let stats_header = sec(lang, "## 今日活动统计", "## Today's Activity Stats");
    let obs_label = if lang == "en" {
        format!("- Behavioral observations: {obs_count} entries")
    } else {
        format!("- 行为观察记录：{obs_count} 条")
    };
    sections.push(format!("{stats_header}\n{obs_label}"));

    // 今日 coach insights
    let insights = store.get_coach_insights_since(&since).unwrap_or_default();
    if !insights.is_empty() {
        let lines: Vec<String> = insights.iter().map(|s| format!("- {s}")).collect();
        let coach_header = sec(lang, "## 今日教练洞察", "## Today's Coach Insights");
        sections.push(format!("{coach_header}\n{}", lines.join("\n")));
    }

    // 今日浏览器行为（Teams 消息 + 页面访问 + 活动模式）
    let behaviors = store
        .get_browser_behaviors_since(&since)
        .unwrap_or_default();
    if !behaviors.is_empty() {
        let mut teams_msgs = Vec::new();
        let mut page_visits = Vec::new();
        let mut patterns = Vec::new();

        for b in &behaviors {
            let meta: serde_json::Value = b
                .metadata
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();

            match b.event_type.as_str() {
                "message_received" if b.source == "teams" => {
                    let sender = meta["sender"].as_str().unwrap_or("?");
                    let channel = meta["channel"].as_str().unwrap_or("?");
                    teams_msgs.push(format!("- {sender} @ {channel}"));
                }
                "page_visit" => {
                    let domain = meta["domain"].as_str().unwrap_or("?");
                    let dur = meta["duration_seconds"].as_i64().unwrap_or(0);
                    if dur >= 30 {
                        page_visits.push(format!("- {domain} ({dur}s)"));
                    }
                }
                "activity_pattern" => {
                    let pattern = meta["pattern"].as_str().unwrap_or("?");
                    let domain = meta["domain"].as_str().unwrap_or("");
                    if !domain.is_empty() {
                        patterns.push(format!("- {pattern}: {domain}"));
                    } else {
                        patterns.push(format!("- {pattern}"));
                    }
                }
                _ => {}
            }
        }

        let browser_title = sec(lang, "## 今日浏览器活动", "## Today's Browser Activity");
        let mut browser_section = format!("{browser_title}\n");

        if !teams_msgs.is_empty() {
            // 去重统计：按 sender 分组计数
            let mut sender_counts = std::collections::HashMap::new();
            for b in &behaviors {
                if b.event_type == "message_received" && b.source == "teams" {
                    let meta: serde_json::Value = b
                        .metadata
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_default();
                    let sender = meta["sender"].as_str().unwrap_or("?").to_string();
                    *sender_counts.entry(sender).or_insert(0usize) += 1;
                }
            }
            let mut counts: Vec<_> = sender_counts.into_iter().collect();
            counts.sort_by(|a, b| b.1.cmp(&a.1));
            let msg_label = if lang == "en" {
                |(s, c): &(String, usize)| format!("- {s}: {c} messages")
            } else {
                |(s, c): &(String, usize)| format!("- {s}：{c} 条消息")
            };
            let summary: Vec<String> = counts.iter().take(10).map(msg_label).collect();
            let teams_header = if lang == "en" {
                format!("### Teams Messages ({} total)", teams_msgs.len())
            } else {
                format!("### Teams 通讯（共 {} 条）", teams_msgs.len())
            };
            browser_section.push_str(&format!("{teams_header}\n{}\n", summary.join("\n")));
        }

        if !page_visits.is_empty() {
            // 去重：按 domain 聚合总时长
            let mut domain_totals = std::collections::HashMap::new();
            for b in &behaviors {
                if b.event_type == "page_visit" {
                    let meta: serde_json::Value = b
                        .metadata
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_default();
                    let domain = meta["domain"].as_str().unwrap_or("?").to_string();
                    let dur = meta["duration_seconds"].as_i64().unwrap_or(0);
                    *domain_totals.entry(domain).or_insert(0i64) += dur;
                }
            }
            let mut totals: Vec<_> = domain_totals.into_iter().collect();
            totals.sort_by(|a, b| b.1.cmp(&a.1));
            let site_label = if lang == "en" {
                |(d, s): &(String, i64)| format!("- {d}: {} min", s / 60)
            } else {
                |(d, s): &(String, i64)| format!("- {d}：{} 分钟", s / 60)
            };
            let top: Vec<String> = totals.iter().take(10).map(site_label).collect();
            let top10_header = sec(lang, "### 网站访问 Top 10", "### Top 10 Websites");
            browser_section.push_str(&format!("{top10_header}\n{}\n", top.join("\n")));
        }

        if !patterns.is_empty() {
            let pat_header = sec(lang, "### 活动模式", "### Activity Patterns");
            browser_section.push_str(&format!("{pat_header}\n{}\n", patterns.join("\n")));
        }

        sections.push(browser_section);
    }

    // Feed 信息洞察（过去 24h，简洁版）
    if let Some(feed_section) = build_feed_section(store, 24, false, lang) {
        sections.push(feed_section);
    }

    inject_corrections(store, "evening", &mut sections, lang);

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

/// Weekly Report：本周所有 memories + sessions + coach insights + 项目/团队文件
fn gather_weekly(store: &Store, lang: &str) -> String {

    let mut sections = Vec::new();

    let since = days_ago(7);

    // 本周 session summaries
    let sessions = store
        .get_session_summaries_since(&since)
        .unwrap_or_default();
    if !sessions.is_empty() {
        let lines: Vec<String> = sessions
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let sess_header = sec(lang, "## 本周工作 Sessions", "## This Week's Work Sessions");
        sections.push(format!("{sess_header}\n{}", lines.join("\n")));
    }

    // 本周所有记忆（按 category 分组）
    let memories = store.get_memories_since(&since).unwrap_or_default();
    if !memories.is_empty() {
        let decision_lines: Vec<_> = memories
            .iter()
            .filter(|m| m.category == "decision")
            .map(|m| format!("- {}", m.content))
            .collect();
        if !decision_lines.is_empty() {
            let dec_header = sec(lang, "## 本周决策", "## This Week's Decisions");
            sections.push(format!("{dec_header}\n{}", decision_lines.join("\n")));
        }

        let insight_lines: Vec<_> = memories
            .iter()
            .filter(|m| m.category == "coach_insight")
            .map(|m| format!("- {}", m.content))
            .collect();
        if !insight_lines.is_empty() {
            let ins_header = sec(lang, "## 本周教练洞察", "## This Week's Coach Insights");
            sections.push(format!("{ins_header}\n{}", insight_lines.join("\n")));
        }
    }

    // 从记忆中查询项目和团队相关信息
    let project_memories = store
        .search_memories("project status progress", 8)
        .unwrap_or_default();
    if !project_memories.is_empty() {
        let lines: Vec<String> = project_memories
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let proj_header = sec(lang, "## 项目相关记忆", "## Project Memories");
        sections.push(format!("{proj_header}\n{}", lines.join("\n")));
    }
    let team_memories = store
        .search_memories("team member colleague", 8)
        .unwrap_or_default();
    if !team_memories.is_empty() {
        let lines: Vec<String> = team_memories
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let team_header = sec(lang, "## 团队相关记忆", "## Team Memories");
        sections.push(format!("{team_header}\n{}", lines.join("\n")));
    }

    inject_corrections(store, "weekly", &mut sections, lang);

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

/// Week Start：上周 weekly report + .context/projects.md
fn gather_week_start(store: &Store, lang: &str) -> String {
    let mut sections = Vec::new();

    // 上次 weekly report
    if let Ok(Some(report)) = store.get_latest_report("weekly") {
        let weekly_header = sec(lang, "## 上周周报", "## Last Week's Report");
        sections.push(format!("{weekly_header}\n{}", report.content));
    }

    // 从记忆中查询项目相关信息
    let project_memories = store
        .search_memories("project plan priority", 8)
        .unwrap_or_default();
    if !project_memories.is_empty() {
        let lines: Vec<String> = project_memories
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect();
        let proj_header = sec(lang, "## 项目相关记忆", "## Project Memories");
        sections.push(format!("{proj_header}\n{}", lines.join("\n")));
    }

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
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
                let project_name = entry
                    .file_name()
                    .to_string_lossy()
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

/// 从最近 `hours` 小时的 feed observations 中构建 Feed 信息洞察 section。
/// `detailed=true`：含 summary + idea；`detailed=false`：只列标题和 score。
/// score < 3 的条目不展示；返回 None 表示无内容。
fn build_feed_section(store: &Store, hours: i64, detailed: bool, lang: &str) -> Option<String> {
    let feed_obs = store.load_feed_observations(30).unwrap_or_default();
    let cutoff = (chrono::Local::now() - chrono::Duration::hours(hours)).to_rfc3339();

    let mut lines = Vec::new();
    for obs in feed_obs.iter().filter(|o| o.created_at >= cutoff) {
        let raw = obs.raw_data.as_deref().unwrap_or("");
        let parts: Vec<&str> = raw.lines().collect();

        // 兼容旧格式（url\ninsight）和新格式（url\nscore\ninsight\nsummary\nidea）
        let (score, url, insight, summary, idea) = if parts.len() >= 5 {
            let score: u8 = parts[1].parse().unwrap_or(0);
            (score, parts[0], parts[2], parts[3], parts[4])
        } else if parts.len() == 2 {
            (3u8, parts[0], parts[1], "", "")
        } else {
            (0u8, "", "", "", "")
        };

        if score < 3 {
            continue;
        }

        let mut line = format!("- **{}**（score: {}）", obs.observation, score);
        if detailed {
            if !insight.is_empty() {
                line.push_str(&format!("\n  > {insight}"));
            }
            if !summary.is_empty() {
                line.push_str(&format!("\n  > 摘要: {summary}"));
            }
            if !idea.is_empty() {
                line.push_str(&format!("\n  > 💡 {idea}"));
            }
        }
        if !url.is_empty() {
            line.push_str(&format!("\n  > {url}"));
        }
        lines.push(line);
    }

    if lines.is_empty() {
        return None;
    }
    let title = if detailed {
        sec(lang, "## Feed 信息洞察（score ≥ 3 的高分项）", "## Feed Insights (score ≥ 3)")
    } else {
        sec(lang, "## Feed 今日洞察（score ≥ 3）", "## Today's Feed Insights (score ≥ 3)")
    };
    Some(format!("{title}\n{}", lines.join("\n")))
}

/// 返回 N 天前的 RFC3339 时间戳字符串
fn days_ago(days: i64) -> String {
    (chrono::Local::now() - chrono::Duration::days(days)).to_rfc3339()
}

/// 注入历史校准记录到报告上下文
fn inject_corrections(store: &Store, report_type: &str, sections: &mut Vec<String>, lang: &str) {
    let corrections = store
        .get_active_corrections(report_type, 10)
        .unwrap_or_default();
    if corrections.is_empty() {
        return;
    }
    let lines: Vec<String> = corrections
        .iter()
        .map(|c| {
            let hint = if c.context_hint.is_empty() {
                String::new()
            } else {
                format!("[{}] ", c.context_hint)
            };
            if lang == "en" {
                format!(
                    "- {hint}Wrong: \"{}\" → Correct: \"{}\" (corrected {} times)",
                    c.wrong_claim, c.correct_fact, c.applied_count
                )
            } else {
                format!(
                    "- {hint}错误：「{}」→ 正确：「{}」（已校准{}次）",
                    c.wrong_claim, c.correct_fact, c.applied_count
                )
            }
        })
        .collect();
    for c in &corrections {
        let _ = store.increment_correction_applied(c.id);
    }
    let cal_header = sec(
        lang,
        "## 历史校准（你之前在此类报告中犯过这些错误，请避免重复）",
        "## Historical Calibrations (errors you made before in this report type — avoid repeating)",
    );
    sections.push(format!("{cal_header}\n{}", lines.join("\n")));
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
        store
            .save_memory("decision", "chose Rust for EMS", "chat", 0.8)
            .unwrap();
        store
            .save_memory("coach_insight", "Alex 偏系统思考", "coach", 0.8)
            .unwrap();

        let ctx = gather(&ReportType::MorningBrief, &store, "outlook", "zh").await;
        assert!(
            ctx.contains("决策") || ctx.contains("chose Rust"),
            "应包含近期决策内容，实际: {ctx}"
        );
    }

    #[tokio::test]
    async fn test_gather_weekly_includes_sessions() {
        let store = make_test_store();
        store
            .save_memory(
                "session",
                "[session] fix bugs — 50 msgs",
                "claude-code",
                0.8,
            )
            .unwrap();

        let ctx = gather(&ReportType::WeeklyReport, &store, "outlook", "zh").await;
        assert!(
            ctx.contains("session") || ctx.contains("Session"),
            "应包含 session 信息，实际: {ctx}"
        );
    }

    #[tokio::test]
    async fn test_gather_evening_review_has_stats() {
        let store = make_test_store();
        store
            .record_observation("pattern", "focused work", None)
            .unwrap();

        let ctx = gather(&ReportType::EveningReview, &store, "outlook", "zh").await;
        assert!(
            ctx.contains("统计") || ctx.contains("活动") || ctx.contains("Activity"),
            "应包含活动统计，实际: {ctx}"
        );
    }

    #[tokio::test]
    async fn test_gather_week_start_includes_last_weekly_report() {
        let store = make_test_store();
        store
            .save_report("weekly", "上周完成了 ProjectY 模块开发")
            .unwrap();

        let ctx = gather(&ReportType::WeekStart, &store, "outlook", "zh").await;
        assert!(ctx.contains("上周"), "应包含上周周报，实际: {ctx}");
    }

    #[tokio::test]
    async fn test_gather_empty_store_returns_empty_or_partial() {
        let store = make_test_store();
        // 空 store 时各类型不崩溃
        let _ = gather(&ReportType::MorningBrief, &store, "outlook", "zh").await;
        let _ = gather(&ReportType::EveningReview, &store, "outlook", "en").await;
        let _ = gather(&ReportType::WeeklyReport, &store, "outlook", "zh").await;
        let _ = gather(&ReportType::WeekStart, &store, "outlook", "en").await;
    }
}
