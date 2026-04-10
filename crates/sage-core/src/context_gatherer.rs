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

/// Context token budget 控制：~15000 字符 ≈ ~4000 tokens
/// 前面的 section 优先级高（保留），超预算时从后面截断
const MAX_CONTEXT_CHARS: usize = 15000;

fn budget_trim(sections: &mut Vec<String>) {
    if sections.is_empty() { return; }
    let total: usize = sections.iter().map(|s| s.len() + 2).sum(); // +2 for "\n\n" join
    if total <= MAX_CONTEXT_CHARS { return; }

    let mut budget = MAX_CONTEXT_CHARS;
    let mut keep = 0;
    for s in sections.iter() {
        let cost = s.len() + 2;
        if cost > budget { break; }
        budget -= cost;
        keep += 1;
    }
    // 至少保留第一个 section
    if keep == 0 { keep = 1; }
    let dropped = sections.len() - keep;
    sections.truncate(keep);
    if dropped > 0 {
        tracing::info!("Context budget: dropped {dropped} low-priority sections ({total} chars → ≤{MAX_CONTEXT_CHARS})");
    }
}

/// 将 items 格式化为 `- item` 列表并附加到 sections（列表为空时跳过）
fn push_bullet_section(sections: &mut Vec<String>, header: &str, items: &[impl AsRef<str>]) {
    if items.is_empty() {
        return;
    }
    let lines: Vec<String> = items.iter().map(|s| format!("- {}", s.as_ref())).collect();
    sections.push(format!("{header}\n{}", lines.join("\n")));
}

/// 注入校准修正记录 + 校准规则（每种报告固定在末尾注入这两块）
fn inject_context_calibration(store: &Store, report_type: &str, sections: &mut Vec<String>, lang: &str) {
    inject_corrections(store, report_type, sections, lang);
    inject_calibration_rules(store, report_type, sections, lang);
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
    let sess_contents: Vec<&str> = sessions.iter().map(|m| m.content.as_str()).collect();
    let sess_header = sec(
        lang,
        "## 近期工作 Sessions（从中提取今日待办）",
        "## Recent Work Sessions (extract today's to-dos from these)",
    );
    push_bullet_section(&mut sections, sess_header, &sess_contents);

    // 读取 Claude Code MEMORY.md（含项目进展和待办）
    if let Some(content) = read_claude_memory() {
        let mem_header = sec(lang, "## Claude Code 记忆", "## Claude Code Memories");
        sections.push(format!("{mem_header}\n{content}"));
    }

    // 从记忆中查询项目相关信息
    let project_memories = store
        .search_memories("project status priority", 10)
        .unwrap_or_default();
    let proj_contents: Vec<&str> = project_memories.iter().map(|m| m.content.as_str()).collect();
    let proj_header = sec(lang, "## 项目相关记忆", "## Project Memories");
    push_bullet_section(&mut sections, proj_header, &proj_contents);

    // 近 7 天的决策记忆
    let since_7d = days_ago(7);
    let memories = store.get_memories_since(&since_7d).unwrap_or_default();
    let decisions: Vec<&str> = memories
        .iter()
        .filter(|m| m.category == "decision")
        .map(|m| m.content.as_str())
        .collect();
    let dec_header = sec(lang, "## 近期决策", "## Recent Decisions");
    push_bullet_section(&mut sections, dec_header, &decisions);

    // 近 7 天的 coach insights
    let insights = store
        .get_coach_insights_since(&since_7d)
        .unwrap_or_default();
    let insights_refs: Vec<&str> = insights.iter().map(|s| s.as_str()).collect();
    let coach_header = sec(lang, "## 教练洞察", "## Coach Insights");
    push_bullet_section(&mut sections, coach_header, &insights_refs);

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

    inject_context_calibration(store, "morning", &mut sections, lang);

    budget_trim(&mut sections);
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
    let sess_contents: Vec<&str> = sessions.iter().map(|m| m.content.as_str()).collect();
    let sess_header = sec(lang, "## 今日工作 Sessions", "## Today's Work Sessions");
    push_bullet_section(&mut sections, sess_header, &sess_contents);

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
    let insights_refs: Vec<&str> = insights.iter().map(|s| s.as_str()).collect();
    let coach_header = sec(lang, "## 今日教练洞察", "## Today's Coach Insights");
    push_bullet_section(&mut sections, coach_header, &insights_refs);

    // 今日浏览器行为（Teams 消息 + 页面访问 + 活动模式）
    let behaviors = store
        .get_browser_behaviors_since(&since)
        .unwrap_or_default();
    if !behaviors.is_empty() {
        let mut teams_msgs = Vec::new();
        let mut page_visits = Vec::new();
        let mut patterns = Vec::new();
        // Claude Code session 聚合：session_id → (project, tools, errors, stop_summary)
        let mut cc_sessions: std::collections::HashMap<
            String,
            CcSession,
        > = std::collections::HashMap::new();

        for b in &behaviors {
            let meta: serde_json::Value = b
                .metadata
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();

            // Claude Code hooks 事件
            if b.source == "claude-hooks" {
                let sid = meta["session_id"].as_str().unwrap_or("").to_string();
                if sid.is_empty() { continue; }
                let sess = cc_sessions.entry(sid).or_insert_with(CcSession::default);
                if let Some(cwd) = meta["cwd"].as_str() {
                    sess.project = extract_project_name(cwd);
                }
                match b.event_type.as_str() {
                    "PreToolUse" | "PostToolUse" => {
                        if let Some(tool) = meta["tool_name"].as_str() {
                            *sess.tools.entry(tool.to_string()).or_insert(0) += 1;
                        }
                        // 从 PostToolUse 提取错误
                        if b.event_type == "PostToolUse" {
                            if let Some(stderr) = meta["tool_response"]["stderr"].as_str() {
                                if !stderr.is_empty() && stderr.contains("error") {
                                    let first_line = stderr.lines().next().unwrap_or("");
                                    if first_line.len() > 10 {
                                        sess.errors.push(truncate(first_line, 120));
                                    }
                                }
                            }
                        }
                    }
                    "Stop" => {
                        if let Some(msg) = meta["last_assistant_message"].as_str() {
                            sess.stop_summary = Some(truncate(msg, 200));
                        }
                    }
                    _ => {}
                }
                sess.event_count += 1;
                continue;
            }

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

        // Claude Code 开发活动
        if !cc_sessions.is_empty() {
            let cc_section = build_claude_code_section(&cc_sessions, lang);
            if !cc_section.is_empty() {
                sections.push(cc_section);
            }
        }
    }

    // Feed 信息洞察（过去 24h，简洁版）
    if let Some(feed_section) = build_feed_section(store, 24, false, lang) {
        sections.push(feed_section);
    }

    inject_context_calibration(store, "evening", &mut sections, lang);

    budget_trim(&mut sections);
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
    let sess_contents: Vec<&str> = sessions.iter().map(|m| m.content.as_str()).collect();
    let sess_header = sec(lang, "## 本周工作 Sessions", "## This Week's Work Sessions");
    push_bullet_section(&mut sections, sess_header, &sess_contents);

    // 本周所有记忆（按 category 分组）
    let memories = store.get_memories_since(&since).unwrap_or_default();
    let decisions: Vec<&str> = memories
        .iter()
        .filter(|m| m.category == "decision")
        .map(|m| m.content.as_str())
        .collect();
    let dec_header = sec(lang, "## 本周决策", "## This Week's Decisions");
    push_bullet_section(&mut sections, dec_header, &decisions);

    let insights: Vec<&str> = memories
        .iter()
        .filter(|m| m.category == "coach_insight")
        .map(|m| m.content.as_str())
        .collect();
    let ins_header = sec(lang, "## 本周教练洞察", "## This Week's Coach Insights");
    push_bullet_section(&mut sections, ins_header, &insights);

    // 从记忆中查询项目和团队相关信息
    let project_memories = store
        .search_memories("project status progress", 8)
        .unwrap_or_default();
    let proj_contents: Vec<&str> = project_memories.iter().map(|m| m.content.as_str()).collect();
    let proj_header = sec(lang, "## 项目相关记忆", "## Project Memories");
    push_bullet_section(&mut sections, proj_header, &proj_contents);

    let team_memories = store
        .search_memories("team member colleague", 8)
        .unwrap_or_default();
    let team_contents: Vec<&str> = team_memories.iter().map(|m| m.content.as_str()).collect();
    let team_header = sec(lang, "## 团队相关记忆", "## Team Memories");
    push_bullet_section(&mut sections, team_header, &team_contents);

    inject_context_calibration(store, "weekly", &mut sections, lang);

    budget_trim(&mut sections);
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
    let proj_contents: Vec<&str> = project_memories.iter().map(|m| m.content.as_str()).collect();
    let proj_header = sec(lang, "## 项目相关记忆", "## Project Memories");
    push_bullet_section(&mut sections, proj_header, &proj_contents);

    budget_trim(&mut sections);
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

/// 注入历史校准记录到报告上下文（正向确认 + 负向纠正分开注入）
fn inject_corrections(store: &Store, report_type: &str, sections: &mut Vec<String>, lang: &str) {
    let corrections = store
        .get_active_corrections(report_type, 20)
        .unwrap_or_default();
    if corrections.is_empty() {
        return;
    }
    // 按 context_hint 前缀区分正向/负向反馈
    let (positive, negative): (Vec<_>, Vec<_>) = corrections
        .iter()
        .partition(|c| c.context_hint.starts_with("positive"));

    for c in &corrections {
        let _ = store.increment_correction_applied(c.id);
    }

    // 正向反馈：用户确认准确的内容
    if !positive.is_empty() {
        let lines: Vec<String> = positive
            .iter()
            .map(|c| format!("- 「{}」", c.wrong_claim))
            .collect();
        let header = sec(
            lang,
            "## 用户确认准确（以下内容用户明确认可，请保持这些判断方向）",
            "## User-Confirmed Accurate (user explicitly approved — maintain these judgments)",
        );
        sections.push(format!("{header}\n{}", lines.join("\n")));
    }

    // 负向反馈：用户标记有误的内容
    if !negative.is_empty() {
        let lines: Vec<String> = negative
            .iter()
            .map(|c| {
                if lang == "en" {
                    format!(
                        "- Wrong: \"{}\" → Correct: \"{}\" (corrected {} times)",
                        c.wrong_claim, c.correct_fact, c.applied_count
                    )
                } else {
                    format!(
                        "- 错误：「{}」→ 正确：「{}」（已校准{}次）",
                        c.wrong_claim, c.correct_fact, c.applied_count
                    )
                }
            })
            .collect();
        let header = sec(
            lang,
            "## 历史校准（你之前在此类报告中犯过这些错误，请避免重复）",
            "## Historical Calibrations (errors you made before — avoid repeating)",
        );
        sections.push(format!("{header}\n{}", lines.join("\n")));
    }
}

/// 注入 calibrator/self-reflect 生成的行为规则
fn inject_calibration_rules(store: &Store, report_type: &str, sections: &mut Vec<String>, lang: &str) {
    // 加载通用校准规则 + report 类型专属规则
    let all_rules = store.get_memories_by_category("calibration").unwrap_or_default();
    let tag = format!("[{report_type}]");
    let matching: Vec<&str> = all_rules
        .iter()
        .filter(|m| m.content.contains(&tag) || !m.content.starts_with('['))
        .map(|m| m.content.as_str())
        .collect();
    if matching.is_empty() {
        return;
    }
    let lines: Vec<String> = matching.iter().map(|r| format!("- {r}")).collect();
    let header = sec(
        lang,
        "## 自我校准规则（Sage 从过去错误中学到的，严格遵守）",
        "## Self-Calibration Rules (learned from past mistakes — follow strictly)",
    );
    sections.push(format!("{header}\n{}", lines.join("\n")));
}

// ─── Claude Code session 聚合 ─────────────────────────────────────────

#[derive(Default)]
struct CcSession {
    project: String,
    tools: std::collections::HashMap<String, usize>,
    errors: Vec<String>,
    stop_summary: Option<String>,
    event_count: usize,
}

fn extract_project_name(cwd: &str) -> String {
    // /Users/evan/dev/sage → sage, /Users/evan/.sage → .sage
    cwd.rsplit('/').next().unwrap_or(cwd).to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..s.floor_char_boundary(max)]) }
}

fn build_claude_code_section(
    sessions: &std::collections::HashMap<String, CcSession>,
    lang: &str,
) -> String {
    // 按 event_count 降序，取 top 10
    let mut sorted: Vec<_> = sessions.iter().collect();
    sorted.sort_by(|a, b| b.1.event_count.cmp(&a.1.event_count));
    sorted.truncate(10);

    // 按项目聚合
    let mut by_project: std::collections::HashMap<String, ProjectStats> =
        std::collections::HashMap::new();
    for (_, sess) in &sorted {
        let proj = if sess.project.is_empty() { "unknown" } else { &sess.project };
        let ps = by_project.entry(proj.to_string()).or_insert_with(ProjectStats::default);
        ps.session_count += 1;
        ps.total_events += sess.event_count;
        for (tool, count) in &sess.tools {
            *ps.tools.entry(tool.clone()).or_insert(0) += count;
        }
        ps.errors.extend(sess.errors.iter().cloned());
        if let Some(ref summary) = sess.stop_summary {
            ps.summaries.push(summary.clone());
        }
    }

    let header = sec(lang,
        "## 今日 Claude Code 开发活动",
        "## Today's Claude Code Development",
    );
    let mut lines = vec![header.to_string()];

    let mut projects: Vec<_> = by_project.into_iter().collect();
    projects.sort_by(|a, b| b.1.total_events.cmp(&a.1.total_events));

    for (proj, ps) in &projects {
        let session_label = if lang == "en" { "sessions" } else { "个会话" };
        let event_label = if lang == "en" { "events" } else { "个事件" };
        lines.push(format!("### {proj}（{} {session_label}，{} {event_label}）",
            ps.session_count, ps.total_events));

        // 工具使用统计
        let mut tool_list: Vec<_> = ps.tools.iter().collect();
        tool_list.sort_by(|a, b| b.1.cmp(a.1));
        let top_tools: Vec<String> = tool_list.iter().take(5)
            .map(|(t, c)| format!("{t}×{c}"))
            .collect();
        let tool_label = if lang == "en" { "Tools" } else { "工具" };
        lines.push(format!("- {tool_label}: {}", top_tools.join(", ")));

        // 错误摘要（去重，最多 3 条）
        if !ps.errors.is_empty() {
            let mut unique_errors: Vec<String> = Vec::new();
            for e in &ps.errors {
                if unique_errors.len() >= 3 { break; }
                if !unique_errors.iter().any(|u| u == e) {
                    unique_errors.push(e.clone());
                }
            }
            let err_label = if lang == "en" { "Errors" } else { "错误" };
            for e in &unique_errors {
                lines.push(format!("- {err_label}: {e}"));
            }
        }

        // 最后一条 stop summary
        if let Some(summary) = ps.summaries.last() {
            let sum_label = if lang == "en" { "Last output" } else { "最后输出" };
            lines.push(format!("- {sum_label}: {summary}"));
        }
    }

    lines.join("\n")
}

#[derive(Default)]
struct ProjectStats {
    session_count: usize,
    total_events: usize,
    tools: std::collections::HashMap<String, usize>,
    errors: Vec<String>,
    summaries: Vec<String>,
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

    #[test]
    fn test_build_claude_code_section_aggregates_sessions() {
        let mut sessions = std::collections::HashMap::new();
        let mut sess = CcSession::default();
        sess.project = "sage".to_string();
        sess.event_count = 42;
        sess.tools.insert("Bash".to_string(), 15);
        sess.tools.insert("Read".to_string(), 10);
        sess.tools.insert("Edit".to_string(), 8);
        sess.errors.push("error[E0425]: cannot find function".to_string());
        sess.stop_summary = Some("Fixed build errors in actions.rs".to_string());
        sessions.insert("session-1".to_string(), sess);

        let mut sess2 = CcSession::default();
        sess2.project = "sage".to_string();
        sess2.event_count = 20;
        sess2.tools.insert("Bash".to_string(), 5);
        sessions.insert("session-2".to_string(), sess2);

        let output = build_claude_code_section(&sessions, "zh");
        assert!(output.contains("Claude Code"), "应有标题");
        assert!(output.contains("sage"), "应包含项目名");
        assert!(output.contains("2 个会话"), "应聚合为 2 个会话");
        assert!(output.contains("Bash×20"), "Bash 应合计为 20");
        assert!(output.contains("E0425"), "应包含错误摘要");
        assert!(output.contains("Fixed build"), "应包含 stop summary");
    }

    #[test]
    fn test_extract_project_name() {
        assert_eq!(extract_project_name("/Users/evan/dev/sage"), "sage");
        assert_eq!(extract_project_name("/Users/evan/.sage"), ".sage");
        assert_eq!(extract_project_name(""), "");
    }

    #[test]
    fn test_truncate_handles_multibyte() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world!", 5).len(), 8); // "hello…"
        let chinese = "你好世界测试";
        let t = truncate(chinese, 6);
        assert!(t.ends_with('…'));
    }

    #[tokio::test]
    async fn test_evening_review_includes_claude_code_hooks() {
        let store = make_test_store();
        let meta = serde_json::json!({
            "session_id": "test-session",
            "cwd": "/Users/evan/dev/sage",
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_response": { "stderr": "", "stdout": "ok" }
        });
        store.save_browser_behavior("claude-hooks", "PostToolUse", &meta.to_string()).unwrap();
        store.save_browser_behavior("claude-hooks", "Stop", &serde_json::json!({
            "session_id": "test-session",
            "cwd": "/Users/evan/dev/sage",
            "last_assistant_message": "Done fixing bugs"
        }).to_string()).unwrap();

        let ctx = gather(&ReportType::EveningReview, &store, "outlook", "zh").await;
        assert!(ctx.contains("Claude Code"), "应包含 Claude Code 开发活动，实际: {ctx}");
        assert!(ctx.contains("sage"), "应包含项目名 sage");
    }
}
