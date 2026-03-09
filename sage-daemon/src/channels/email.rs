use anyhow::Result;
use async_trait::async_trait;

use crate::applescript;
use crate::channel::{Event, EventType, InputChannel};

pub struct EmailChannel;

#[async_trait]
impl InputChannel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        let script = r#"
            tell application "System Events"
                if not (exists process "Microsoft Outlook") then return ""
            end tell
            tell application "Microsoft Outlook"
                set unreadMsgs to messages of inbox whose is read is false
                set msgCount to count of unreadMsgs
                if msgCount is 0 then return ""
                set maxCount to msgCount
                if maxCount > 5 then set maxCount to 5
                set output to ""
                repeat with i from 1 to maxCount
                    set msg to item i of unreadMsgs
                    set output to output & "SUBJECT:" & subject of msg & "||FROM:" & (address of sender of msg) & "||DATE:" & (time sent of msg as string) & "|||"
                end repeat
                return output
            end tell
        "#;

        let raw = applescript::run(script).await?;
        Ok(parse_emails(&raw))
    }
}

fn parse_emails(raw: &str) -> Vec<Event> {
    raw.split("|||")
        .filter(|s| !s.trim().is_empty())
        .filter_map(|entry| {
            let mut subject = String::new();
            let mut from = String::new();

            for field in entry.split("||") {
                if let Some(val) = field.strip_prefix("SUBJECT:") {
                    subject = val.to_string();
                } else if let Some(val) = field.strip_prefix("FROM:") {
                    from = val.to_string();
                }
            }

            if subject.is_empty() {
                return None;
            }

            Some(Event {
                source: "email".into(),
                event_type: EventType::NewEmail,
                title: subject,
                body: format!("From: {from}"),
                metadata: [("from".into(), from)].into_iter().collect(),
                timestamp: chrono::Local::now(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_emails_empty_string() {
        let result = parse_emails("");
        assert!(result.is_empty(), "空字符串应返回空列表");
    }

    #[test]
    fn test_parse_emails_whitespace_only() {
        let result = parse_emails("   \n  ");
        assert!(result.is_empty(), "纯空白字符串应返回空列表");
    }

    #[test]
    fn test_parse_emails_normal_input() {
        let raw = "FROM:sender@example.com||SUBJECT:Hello World||DATE:2026-03-03|||FROM:sender2@example.com||SUBJECT:Second Email||DATE:2026-03-03|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 2);

        assert_eq!(result[0].title, "Hello World");
        assert_eq!(result[0].source, "email");
        assert_eq!(result[0].metadata.get("from").map(|s| s.as_str()), Some("sender@example.com"));
        assert!(result[0].body.contains("sender@example.com"));

        assert_eq!(result[1].title, "Second Email");
        assert_eq!(result[1].metadata.get("from").map(|s| s.as_str()), Some("sender2@example.com"));
    }

    #[test]
    fn test_parse_emails_missing_subject_filtered_out() {
        // 缺少 SUBJECT 字段，应被过滤掉
        let raw = "FROM:sender@example.com||DATE:2026-03-03|||";
        let result = parse_emails(raw);
        assert!(result.is_empty(), "缺少 SUBJECT 的条目应被过滤");
    }

    #[test]
    fn test_parse_emails_missing_from_field() {
        // 缺少 FROM 字段，但有 SUBJECT，应该正常解析
        let raw = "SUBJECT:No Sender Email||DATE:2026-03-03|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "No Sender Email");
        assert_eq!(result[0].metadata.get("from").map(|s| s.as_str()), Some(""));
    }

    #[test]
    fn test_parse_emails_mixed_valid_and_invalid() {
        // 一条有效、一条缺 SUBJECT
        let raw = "SUBJECT:Valid||FROM:a@b.com|||FROM:no-subject@b.com|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Valid");
    }
}
