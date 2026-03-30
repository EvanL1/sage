//! 解析 Claude Code 的 JSONL session 文件，提取有价值的结构化信息。
//!
//! Claude Code 每次会话生成一个 `.jsonl` 文件，每行是一个 JSON 对象。
//! 本模块读取这些文件，提炼出用户意图、工具使用情况和修改的文件列表。

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::{debug, info};

use crate::store::Store;

/// 单次 Claude Code 会话的提炼摘要
#[derive(Debug, Clone, Default)]
pub struct SessionSummary {
    /// 会话 ID（来自第一条消息的 sessionId 字段）
    pub session_id: String,
    /// 会话开始时间（ISO 8601）
    pub started_at: String,
    /// 会话结束时间（ISO 8601）
    pub ended_at: String,
    /// 用户发送的所有消息文本
    pub user_messages: Vec<String>,
    /// Write / Edit 工具操作过的文件路径（已去重）
    pub files_modified: Vec<String>,
    /// Bash 工具执行的命令（截断到 100 字符）
    pub commands_run: Vec<String>,
    /// 工具名 → 调用次数
    pub tools_used: HashMap<String, usize>,
    /// user + assistant 消息总数
    pub message_count: usize,
    /// 基于前 3 条用户消息自动生成的一句话预览提示
    pub summary_hint: String,
    /// 是否由 cron/scheduled trigger 自动触发（检测到 queue-operation enqueue）
    pub is_automated: bool,
}

/// 逐行解析 JSONL session 文件，返回结构化的 `SessionSummary`。
///
/// - 无法解析的行静默跳过（记录 debug 日志）
/// - `progress` / `system` / `file-history-snapshot` 类型跳过
/// - `files_modified` 自动去重
pub fn analyze_session(jsonl_path: &Path) -> Result<SessionSummary> {
    let file = std::fs::File::open(jsonl_path)?;
    let reader = BufReader::new(file);

    let mut summary = SessionSummary::default();
    // 用于 files_modified 去重
    let mut files_seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (line_no, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                debug!("session_analyzer: 读取第 {} 行失败: {}", line_no + 1, e);
                continue;
            }
        };

        // 跳过空行
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 解析 JSON，失败则静默跳过
        let val: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                debug!(
                    "session_analyzer: 第 {} 行 JSON 解析失败: {}",
                    line_no + 1,
                    e
                );
                continue;
            }
        };

        // 读取消息类型，跳过不关心的类型
        let msg_type = match val["type"].as_str() {
            Some(t) => t,
            None => continue,
        };
        if matches!(msg_type, "progress" | "system" | "file-history-snapshot") {
            continue;
        }

        // 更新时间戳（第一次写入 started_at，之后持续覆盖 ended_at）
        if let Some(ts) = val["timestamp"].as_str() {
            if summary.started_at.is_empty() {
                summary.started_at = ts.to_string();
            }
            summary.ended_at = ts.to_string();
        }

        // 填充 session_id（只取一次）
        if summary.session_id.is_empty() {
            if let Some(id) = val["sessionId"].as_str() {
                summary.session_id = id.to_string();
            }
        }

        match msg_type {
            "user" => {
                summary.message_count += 1;
                // user 消息的 content 通常是字符串
                let text = extract_user_text(&val);
                if !text.is_empty() {
                    summary.user_messages.push(text);
                }
            }
            "assistant" => {
                summary.message_count += 1;
                // assistant 消息的 content 是数组，遍历所有 tool_use 元素
                if let Some(content_arr) = val["message"]["content"].as_array() {
                    for item in content_arr {
                        process_content_item(
                            item,
                            &mut summary.tools_used,
                            &mut summary.commands_run,
                            &mut files_seen,
                            &mut summary.files_modified,
                        );
                    }
                }
            }
            "queue-operation" => {
                // cron/scheduled trigger 会有 enqueue 操作
                if val["operation"].as_str() == Some("enqueue") {
                    summary.is_automated = true;
                }
            }
            other => {
                debug!("session_analyzer: 未知消息类型 '{}', 跳过", other);
            }
        }
    }

    // 生成 summary_hint：取前 3 条用户消息，每条截断到 50 字符
    summary.summary_hint = build_summary_hint(&summary.user_messages);

    Ok(summary)
}

/// 从 user 消息中提取文本内容。
/// content 可能是字符串，也可能是 `[{type:"text", text:"..."}]` 数组。
fn extract_user_text(val: &Value) -> String {
    let content = &val["message"]["content"];

    if let Some(s) = content.as_str() {
        return s.trim().to_string();
    }

    if let Some(arr) = content.as_array() {
        let parts: Vec<&str> = arr
            .iter()
            .filter_map(|item| {
                if item["type"].as_str() == Some("text") {
                    item["text"].as_str()
                } else {
                    None
                }
            })
            .collect();
        return parts.join(" ").trim().to_string();
    }

    String::new()
}

/// 处理 assistant content 数组中的单个元素。
/// 根据 type 字段分别处理 tool_use（统计工具 + 提取命令/文件）。
fn process_content_item(
    item: &Value,
    tools_used: &mut HashMap<String, usize>,
    commands_run: &mut Vec<String>,
    files_seen: &mut std::collections::HashSet<String>,
    files_modified: &mut Vec<String>,
) {
    let item_type = match item["type"].as_str() {
        Some(t) => t,
        None => return,
    };

    if item_type != "tool_use" {
        // text / thinking 等类型不做处理
        return;
    }

    let tool_name = match item["name"].as_str() {
        Some(n) => n,
        None => return,
    };

    // 统计工具使用次数
    *tools_used.entry(tool_name.to_string()).or_insert(0) += 1;

    let input = &item["input"];

    match tool_name {
        // Bash 工具：提取 command 字段，截断到 100 字符
        "Bash" => {
            if let Some(cmd) = input["command"].as_str() {
                let truncated = truncate_str(cmd.trim(), 100);
                commands_run.push(truncated.to_string());
            }
        }
        // Write / Edit 工具：提取 file_path 字段，去重后加入列表
        "Write" | "Edit" => {
            if let Some(fp) = input["file_path"].as_str() {
                let path = fp.trim().to_string();
                if !path.is_empty() && files_seen.insert(path.clone()) {
                    files_modified.push(path);
                }
            }
        }
        _ => {
            // 其他工具（Read, Grep, Glob 等）只统计次数，不提取细节
        }
    }
}

/// 取前 3 条用户消息，每条截断到 50 字符，拼接为预览提示。
fn build_summary_hint(user_messages: &[String]) -> String {
    if user_messages.is_empty() {
        return "(无用户消息)".to_string();
    }

    user_messages
        .iter()
        .take(3)
        .map(|msg| truncate_str(msg, 50).to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// 将字符串截断到指定字节长度，保证 UTF-8 边界安全。
fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    // 按字符数截断，找到对应的字节边界
    let byte_pos = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    &s[..byte_pos]
}

// ─── Session Ingestion Pipeline ───────────────────────────────────────────────

/// 扫描 `~/.claude/projects/` 下的 JSONL session 文件，分析并保存到 Store。
///
/// - `claude_dir`: Claude Code 数据目录（通常为 `~/.claude`）
/// - `hours`: 只处理最近 N 小时内修改过的文件
/// - 返回新 ingest 的 session 数量
///
/// 每个 session 以 `source = "claude-session:{file_stem}"` 存储，
/// 重复调用时会 upsert（删旧插新），保证内容是最新的。
pub fn ingest_sessions(claude_dir: &Path, store: &Store, hours: i64) -> Result<usize> {
    let projects_dir = claude_dir.join("projects");
    if !projects_dir.exists() {
        debug!("Claude projects dir not found: {}", projects_dir.display());
        return Ok(0);
    }

    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(hours as u64 * 3600);
    let mut ingested = 0;

    // 遍历每个项目目录
    for project_entry in std::fs::read_dir(&projects_dir)?.flatten() {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }

        let project_name = project_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(prettify_project_name)
            .unwrap_or_else(|| "unknown".into());

        // 遍历项目目录下的 .jsonl 文件
        for entry in std::fs::read_dir(&project_path)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            // 只处理最近 N 小时内修改过的文件
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(mtime) = meta.modified() {
                    if mtime < cutoff {
                        continue;
                    }
                }
            }

            let file_stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let source_key = format!("claude-session:{file_stem}");

            match analyze_session(&path) {
                Ok(summary) if summary.message_count > 0 && !summary.is_automated => {
                    let content = format_session_summary(&summary, &project_name);
                    // Upsert: 删旧 → 插新
                    let _ = store.delete_memory_by_source(&source_key);
                    store.save_memory_with_visibility(
                        "session",
                        &content,
                        &source_key,
                        0.8,
                        "public",
                    )?;
                    ingested += 1;
                    debug!(
                        "Ingested session {} ({} msgs, {} files)",
                        file_stem,
                        summary.message_count,
                        summary.files_modified.len()
                    );
                }
                Ok(summary) if summary.is_automated => {
                    // 自动触发的 session 不 ingest，清理之前可能已存的旧记忆
                    let _ = store.delete_memory_by_source(&source_key);
                }
                Ok(_) => {} // 空 session，跳过
                Err(e) => {
                    debug!("Failed to analyze {}: {}", path.display(), e);
                }
            }
        }
    }

    if ingested > 0 {
        info!("Ingested {ingested} Claude Code sessions");
    }
    Ok(ingested)
}

/// 返回默认的 Claude Code 数据目录
pub fn default_claude_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(".claude")
}

/// 将 SessionSummary 格式化为适合存储的单行摘要
fn format_session_summary(s: &SessionSummary, project: &str) -> String {
    let mut parts = Vec::new();

    // [项目名] 用户消息预览（自动触发加 [cron] 前缀）
    let prefix = if s.is_automated {
        format!("[cron:{project}]")
    } else {
        format!("[{project}]")
    };
    parts.push(format!("{prefix} {}", s.summary_hint));

    // 消息统计
    parts.push(format!("{} msgs", s.message_count));

    // 修改的文件（最多显示 5 个，取短路径）
    if !s.files_modified.is_empty() {
        let file_list: Vec<String> = s
            .files_modified
            .iter()
            .take(5)
            .map(|f| shorten_path(f))
            .collect();
        let more = if s.files_modified.len() > 5 {
            format!(" +{}", s.files_modified.len() - 5)
        } else {
            String::new()
        };
        parts.push(format!("files: {}{more}", file_list.join(", ")));
    }

    // 工具使用 Top 5
    if !s.tools_used.is_empty() {
        let mut tools: Vec<_> = s.tools_used.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));
        let top: Vec<String> = tools
            .into_iter()
            .take(5)
            .map(|(k, v)| format!("{k}×{v}"))
            .collect();
        parts.push(format!("tools: {}", top.join(" ")));
    }

    // 时间范围
    if !s.started_at.is_empty() && !s.ended_at.is_empty() {
        // 只取时间部分 (HH:MM)
        let start = extract_time(&s.started_at);
        let end = extract_time(&s.ended_at);
        if !start.is_empty() {
            parts.push(format!("{start}~{end}"));
        }
    }

    parts.join(" — ")
}

/// 从 ISO 8601 时间戳中提取 HH:MM
fn extract_time(ts: &str) -> String {
    // "2024-01-01T08:30:00Z" or "2024-01-01T08:30:00+08:00"
    if let Some(t_pos) = ts.find('T') {
        let time_part = &ts[t_pos + 1..];
        // 取 HH:MM
        if time_part.len() >= 5 {
            return time_part[..5].to_string();
        }
    }
    String::new()
}

/// 缩短文件路径：取最后 2 个组件
fn shorten_path(path: &str) -> String {
    let parts: Vec<&str> = path.rsplit('/').take(2).collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

/// 将 Claude Code 项目目录名转换为可读的项目名
///
/// 格式：`-Users-dev-digital-twin` → `digital-twin`
fn prettify_project_name(dir_name: &str) -> String {
    let stripped = dir_name.trim_start_matches('-');
    let parts: Vec<&str> = stripped.split('-').collect();

    // 找到 "dev" 后面的部分作为项目名
    if let Some(dev_pos) = parts.iter().position(|&p| p == "dev") {
        let project_parts = &parts[dev_pos + 1..];
        if !project_parts.is_empty() {
            return project_parts.join("-");
        }
    }

    // fallback: 取最后一个部分
    parts.last().unwrap_or(&"unknown").to_string()
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// 生成一条 JSONL 行（user 消息）
    fn user_line(session_id: &str, ts: &str, text: &str) -> String {
        serde_json::json!({
            "type": "user",
            "timestamp": ts,
            "sessionId": session_id,
            "uuid": "u1",
            "message": {
                "role": "user",
                "content": text
            }
        })
        .to_string()
    }

    /// 生成一条 JSONL 行（assistant 消息，包含 tool_use）
    fn assistant_tool_line(session_id: &str, ts: &str, tool: &str, input: Value) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": ts,
            "sessionId": session_id,
            "uuid": "a1",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "name": tool,
                        "input": input
                    }
                ]
            }
        })
        .to_string()
    }

    /// 写内容到临时文件并返回
    fn write_temp(lines: &[String]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        f
    }

    // ── 测试 1：提取用户消息 ──────────────────────────────────────────────────

    #[test]
    fn test_analyze_user_messages() {
        let lines = vec![
            user_line("sess-1", "2024-01-01T08:00:00Z", "帮我写一个 Rust 函数"),
            user_line("sess-1", "2024-01-01T08:01:00Z", "再加个单元测试"),
            user_line("sess-1", "2024-01-01T08:02:00Z", "好的，谢谢"),
        ];
        let f = write_temp(&lines);
        let summary = analyze_session(f.path()).unwrap();

        assert_eq!(summary.user_messages.len(), 3);
        assert_eq!(summary.user_messages[0], "帮我写一个 Rust 函数");
        assert_eq!(summary.user_messages[1], "再加个单元测试");
        assert_eq!(summary.session_id, "sess-1");
        assert_eq!(summary.started_at, "2024-01-01T08:00:00Z");
        assert_eq!(summary.ended_at, "2024-01-01T08:02:00Z");
        assert_eq!(summary.message_count, 3);
        // summary_hint 包含前 3 条
        assert!(summary.summary_hint.contains("帮我写一个 Rust 函数"));
        assert!(summary.summary_hint.contains("再加个单元测试"));
    }

    // ── 测试 2：统计工具使用次数 ──────────────────────────────────────────────

    #[test]
    fn test_analyze_tool_usage() {
        let lines = vec![
            user_line("sess-2", "2024-01-01T09:00:00Z", "执行一些命令"),
            assistant_tool_line(
                "sess-2",
                "2024-01-01T09:00:01Z",
                "Bash",
                serde_json::json!({"command": "cargo build"}),
            ),
            assistant_tool_line(
                "sess-2",
                "2024-01-01T09:00:02Z",
                "Bash",
                serde_json::json!({"command": "cargo test"}),
            ),
            assistant_tool_line(
                "sess-2",
                "2024-01-01T09:00:03Z",
                "Read",
                serde_json::json!({"file_path": "src/main.rs"}),
            ),
        ];
        let f = write_temp(&lines);
        let summary = analyze_session(f.path()).unwrap();

        assert_eq!(*summary.tools_used.get("Bash").unwrap(), 2);
        assert_eq!(*summary.tools_used.get("Read").unwrap(), 1);
        assert_eq!(summary.commands_run.len(), 2);
        assert!(summary.commands_run.contains(&"cargo build".to_string()));
        assert!(summary.commands_run.contains(&"cargo test".to_string()));
        // message_count: 1 user + 3 assistant
        assert_eq!(summary.message_count, 4);
    }

    // ── 测试 3：提取修改的文件（去重） ────────────────────────────────────────

    #[test]
    fn test_analyze_files_modified() {
        let lines = vec![
            user_line("sess-3", "2024-01-01T10:00:00Z", "修改几个文件"),
            assistant_tool_line(
                "sess-3",
                "2024-01-01T10:00:01Z",
                "Write",
                serde_json::json!({"file_path": "src/lib.rs", "content": "..."}),
            ),
            assistant_tool_line(
                "sess-3",
                "2024-01-01T10:00:02Z",
                "Edit",
                serde_json::json!({"file_path": "src/main.rs", "old_string": "a", "new_string": "b"}),
            ),
            // 重复写入同一文件，应被去重
            assistant_tool_line(
                "sess-3",
                "2024-01-01T10:00:03Z",
                "Write",
                serde_json::json!({"file_path": "src/lib.rs", "content": "...updated..."}),
            ),
        ];
        let f = write_temp(&lines);
        let summary = analyze_session(f.path()).unwrap();

        // 去重后只有 2 个文件
        assert_eq!(summary.files_modified.len(), 2);
        assert!(summary.files_modified.contains(&"src/lib.rs".to_string()));
        assert!(summary.files_modified.contains(&"src/main.rs".to_string()));
        // Write 被调用 2 次，Edit 被调用 1 次
        assert_eq!(*summary.tools_used.get("Write").unwrap(), 2);
        assert_eq!(*summary.tools_used.get("Edit").unwrap(), 1);
    }

    // ── 测试 4：空文件不报错 ──────────────────────────────────────────────────

    #[test]
    fn test_analyze_empty_file() {
        let f = write_temp(&[]);
        let summary = analyze_session(f.path()).unwrap();

        assert!(summary.session_id.is_empty());
        assert!(summary.user_messages.is_empty());
        assert!(summary.files_modified.is_empty());
        assert!(summary.commands_run.is_empty());
        assert!(summary.tools_used.is_empty());
        assert_eq!(summary.message_count, 0);
        assert_eq!(summary.summary_hint, "(无用户消息)");
    }

    // ── 辅助测试：progress / system 行被跳过 ─────────────────────────────────

    #[test]
    fn test_skip_non_message_types() {
        let lines = vec![
            // progress 行，应跳过
            serde_json::json!({
                "type": "progress",
                "timestamp": "2024-01-01T11:00:00Z",
                "sessionId": "sess-4"
            })
            .to_string(),
            // system 行，应跳过
            serde_json::json!({
                "type": "system",
                "timestamp": "2024-01-01T11:00:01Z",
                "sessionId": "sess-4"
            })
            .to_string(),
            // 真正的 user 消息
            user_line("sess-4", "2024-01-01T11:00:02Z", "只有这一条"),
        ];
        let f = write_temp(&lines);
        let summary = analyze_session(f.path()).unwrap();

        assert_eq!(summary.user_messages.len(), 1);
        assert_eq!(summary.message_count, 1);
    }

    // ── 辅助测试：truncate_str 的 UTF-8 安全截断 ─────────────────────────────

    #[test]
    fn test_truncate_str_multibyte() {
        // 中文每个字符 3 字节，截断时不能切断多字节序列
        let s = "你好世界，这是一段很长的中文文本用于测试截断功能是否正确处理多字节字符";
        let result = truncate_str(s, 10);
        // 结果必须是合法 UTF-8，且长度不超过 10 个字符
        assert!(result.chars().count() <= 10);
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }

    // ── 测试 prettify_project_name ────────────────────────────────────────────

    #[test]
    fn test_prettify_project_name() {
        assert_eq!(
            prettify_project_name("-Users-dev-digital-twin"),
            "digital-twin"
        );
        assert_eq!(prettify_project_name("-Users-dev-ProjectX"), "ProjectX");
        assert_eq!(
            prettify_project_name("-Users-dev-my-cool-project"),
            "my-cool-project"
        );
        // fallback: 没有 "dev" 时取最后一个部分
        assert_eq!(prettify_project_name("-some-path-project"), "project");
    }

    // ── 测试 format_session_summary ───────────────────────────────────────────

    #[test]
    fn test_format_session_summary() {
        let summary = SessionSummary {
            session_id: "sess-fmt".into(),
            started_at: "2024-01-01T08:00:00Z".into(),
            ended_at: "2024-01-01T09:30:00Z".into(),
            user_messages: vec!["fix a bug".into(), "add tests".into()],
            files_modified: vec!["src/lib.rs".into(), "~/dev/project/src/main.rs".into()],
            commands_run: vec!["cargo test".into()],
            tools_used: {
                let mut m = HashMap::new();
                m.insert("Edit".into(), 5);
                m.insert("Read".into(), 3);
                m.insert("Bash".into(), 1);
                m
            },
            message_count: 10,
            summary_hint: "fix a bug | add tests".into(),
            is_automated: false,
        };
        let formatted = format_session_summary(&summary, "digital-twin");
        assert!(formatted.contains("[digital-twin]"));
        assert!(formatted.contains("10 msgs"));
        assert!(formatted.contains("08:00~09:30"));
        assert!(formatted.contains("files:"));
        assert!(formatted.contains("tools:"));
    }

    #[test]
    fn test_format_session_summary_cron() {
        let summary = SessionSummary {
            message_count: 5,
            summary_hint: "auto task".into(),
            is_automated: true,
            ..Default::default()
        };
        let formatted = format_session_summary(&summary, "qu");
        assert!(formatted.contains("[cron:qu]"));
        assert!(!formatted.contains("[qu]"));
    }

    // ── 测试 extract_time ─────────────────────────────────────────────────────

    #[test]
    fn test_extract_time() {
        assert_eq!(extract_time("2024-01-01T08:30:00Z"), "08:30");
        assert_eq!(extract_time("2024-01-01T14:05:00+08:00"), "14:05");
        assert_eq!(extract_time("invalid"), "");
    }

    // ── 测试 shorten_path ─────────────────────────────────────────────────────

    #[test]
    fn test_shorten_path() {
        assert_eq!(shorten_path("~/dev/project/src/main.rs"), "src/main.rs");
        assert_eq!(shorten_path("src/lib.rs"), "src/lib.rs");
        assert_eq!(shorten_path("file.rs"), "file.rs");
    }

    // ── 测试 ingest_sessions（集成测试） ──────────────────────────────────────

    #[test]
    fn test_ingest_sessions_with_temp_dir() {
        let store = Store::open_in_memory().unwrap();

        // 创建模拟的 Claude Code 目录结构
        let tmp = tempfile::tempdir().unwrap();
        let projects_dir = tmp.path().join("projects");
        let project_dir = projects_dir.join("-Users-dev-test-project");
        std::fs::create_dir_all(&project_dir).unwrap();

        // 写入一个 session JSONL 文件
        let session_file = project_dir.join("sess-test-123.jsonl");
        let lines = vec![
            user_line("sess-test-123", "2024-01-01T08:00:00Z", "实现一个新功能"),
            assistant_tool_line(
                "sess-test-123",
                "2024-01-01T08:01:00Z",
                "Edit",
                serde_json::json!({"file_path": "src/lib.rs", "old_string": "a", "new_string": "b"}),
            ),
            user_line("sess-test-123", "2024-01-01T08:02:00Z", "加个测试"),
        ];
        {
            let mut f = std::fs::File::create(&session_file).unwrap();
            for line in &lines {
                writeln!(f, "{}", line).unwrap();
            }
        }

        // 执行 ingestion
        let count = ingest_sessions(tmp.path(), &store, 24 * 365).unwrap();
        assert_eq!(count, 1);

        // 验证 session memory 已写入 store
        let sessions = store
            .get_session_summaries_since("2000-01-01T00:00:00+00:00")
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].content.contains("test-project"));
        assert!(sessions[0].content.contains("3 msgs"));
        assert!(sessions[0].source.contains("claude-session:"));

        // 再次 ingest 不应重复（upsert 逻辑）
        let count2 = ingest_sessions(tmp.path(), &store, 24 * 365).unwrap();
        assert_eq!(count2, 1); // upsert: 旧的被删再插
        let sessions2 = store
            .get_session_summaries_since("2000-01-01T00:00:00+00:00")
            .unwrap();
        assert_eq!(sessions2.len(), 1); // 仍然只有 1 条
    }

    // ── 测试 ingest_sessions 空目录不报错 ─────────────────────────────────────

    #[test]
    fn test_ingest_sessions_empty_dir() {
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let count = ingest_sessions(tmp.path(), &store, 24).unwrap();
        assert_eq!(count, 0);
    }

    // ── 测试 ingest_sessions 跳过空 session ──────────────────────────────────

    #[test]
    fn test_ingest_sessions_skips_empty_session() {
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let projects_dir = tmp.path().join("projects");
        let project_dir = projects_dir.join("-Users-dev-empty");
        std::fs::create_dir_all(&project_dir).unwrap();

        // 写入空 session 文件
        std::fs::write(project_dir.join("empty-sess.jsonl"), "").unwrap();

        let count = ingest_sessions(tmp.path(), &store, 24 * 365).unwrap();
        assert_eq!(count, 0);
    }
}
