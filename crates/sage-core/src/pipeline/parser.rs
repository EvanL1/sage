//! 通用 LLM 输出解析器 — XML 提取 + JSON 反序列化 + 行命令解析

use anyhow::{anyhow, Result};
use tracing::warn;

// ─── XML block 提取 ─────────────────────────────────────────────────────────

/// 从 LLM 响应中提取 `<output>...</output>` 内容。
/// 如果没有 `<output>` 标签，回退到整个文本（兼容旧 prompt）。
pub fn extract_output_block(text: &str) -> &str {
    if let Some(start) = text.find("<output>") {
        let content_start = start + "<output>".len();
        if let Some(end) = text[content_start..].find("</output>") {
            return text[content_start..content_start + end].trim();
        }
    }
    // 回退：无 <output> 标签时用整个文本
    text.trim()
}

// ─── JSON 模式解析 ──────────────────────────────────────────────────────────

/// 去除 markdown fence 后反序列化 JSON。
/// 支持 ```json ... ``` 和裸 JSON。
pub fn parse_json_fenced<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let stripped = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(stripped)
        .map_err(|e| anyhow!("JSON 解析失败: {e}\n原始响应: {}", &stripped[..stripped.len().min(200)]))
}

// ─── 行命令模式解析 ─────────────────────────────────────────────────────────

/// 解析结果：成功解析的命令 + 被拒绝的行（行内容, 拒绝原因）
pub struct ParseResult<T> {
    pub commands: Vec<T>,
    pub rejected: Vec<(String, String)>,
}

/// 逐行解析 LLM 输出为 typed commands。
/// - `parser` 对每行返回 `Ok(Some(cmd))` 表示成功解析，`Ok(None)` 表示跳过（叙述文本），
///   `Err(reason)` 表示看起来像命令但格式有误。
/// - 自动先调用 `extract_output_block` 提取 `<output>` 块。
pub fn parse_commands<T, F>(text: &str, parser: F) -> ParseResult<T>
where
    F: Fn(&str) -> Result<Option<T>>,
{
    let block = extract_output_block(text);
    let mut commands = Vec::new();
    let mut rejected = Vec::new();

    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() || line == "NONE" {
            continue;
        }
        match parser(line) {
            Ok(Some(cmd)) => commands.push(cmd),
            Ok(None) => {} // 叙述文本，正常跳过
            Err(e) => {
                let truncated = if line.len() > 100 { &line[..100] } else { line };
                warn!("rejected command '{}': {}", truncated, e);
                rejected.push((line.to_string(), e.to_string()));
            }
        }
    }

    ParseResult { commands, rejected }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_output_block_basic() {
        let text = "<thinking>\nsome analysis\n</thinking>\n<output>\nDEDUP [1, 2]\nCONDENSE [3] → shorter\n</output>";
        assert_eq!(extract_output_block(text), "DEDUP [1, 2]\nCONDENSE [3] → shorter");
    }

    #[test]
    fn extract_output_block_no_tags() {
        let text = "DEDUP [1, 2]\nCONDENSE [3] → shorter";
        assert_eq!(extract_output_block(text), text);
    }

    #[test]
    fn extract_output_block_with_preamble() {
        let text = "Here is my analysis:\n<output>\nDEDUP [1]\n</output>\nDone.";
        assert_eq!(extract_output_block(text), "DEDUP [1]");
    }

    #[test]
    fn parse_json_fenced_bare() {
        let text = r#"{"name": "test", "value": 42}"#;
        let v: serde_json::Value = parse_json_fenced(text).unwrap();
        assert_eq!(v["name"], "test");
    }

    #[test]
    fn parse_json_fenced_with_fence() {
        let text = "```json\n{\"name\": \"test\"}\n```";
        let v: serde_json::Value = parse_json_fenced(text).unwrap();
        assert_eq!(v["name"], "test");
    }

    #[test]
    fn parse_json_fenced_invalid() {
        let text = "not json at all";
        assert!(parse_json_fenced::<serde_json::Value>(text).is_err());
    }

    #[test]
    fn parse_commands_with_output_block() {
        let text = "<thinking>\nanalysis here\n</thinking>\n<output>\nCMD 1\nCMD 2\nskip this\n</output>";
        let result = parse_commands(text, |line| {
            if line.starts_with("CMD") {
                Ok(Some(line.to_string()))
            } else {
                Ok(None)
            }
        });
        assert_eq!(result.commands.len(), 2);
        assert!(result.rejected.is_empty());
    }

    #[test]
    fn parse_commands_collects_rejected() {
        let text = "<output>\nGOOD 1\nBAD line\n</output>";
        let result = parse_commands(text, |line| {
            if line.starts_with("GOOD") {
                Ok(Some(line.to_string()))
            } else if line.starts_with("BAD") {
                Err(anyhow!("invalid format"))
            } else {
                Ok(None)
            }
        });
        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.rejected.len(), 1);
        assert!(result.rejected[0].1.contains("invalid format"));
    }

    #[test]
    fn parse_commands_none_is_skipped() {
        let text = "NONE";
        let result = parse_commands::<String, _>(text, |_| Ok(Some("x".into())));
        assert!(result.commands.is_empty());
    }
}
