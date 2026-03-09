//! 解析 Claude Code 的 JSONL session 文件，提取有价值的结构化信息。
//!
//! Claude Code 每次会话生成一个 `.jsonl` 文件，每行是一个 JSON 对象。
//! 本模块读取这些文件，提炼出用户意图、工具使用情况和修改的文件列表。

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::debug;

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
}
