use anyhow::Result;
use async_trait::async_trait;
use sage_types::{Event, EventType};
use tracing::info;

use crate::applescript;
use crate::channel::InputChannel;

pub struct EmailChannel;

#[async_trait]
impl InputChannel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        // 遍历所有"收件箱"/"Inbox"文件夹以覆盖多账户
        // sender address 用 try 包裹防止 -1700 错误
        let script = r#"
            tell application "System Events"
                if not (exists process "Microsoft Outlook") then return "__NOT_RUNNING__"
            end tell
            tell application "Microsoft Outlook"
                set allFolders to every mail folder
                set output to ""
                repeat with f in allFolders
                    try
                        set folderName to name of f
                        if folderName is "收件箱" or folderName is "Inbox" then
                            set unreadMsgs to (messages of f whose is read is false)
                            set msgCount to count of unreadMsgs
                            if msgCount > 0 then
                                set maxCount to msgCount
                                if maxCount > 5 then set maxCount to 5
                                repeat with i from 1 to maxCount
                                    set msg to item i of unreadMsgs
                                    set senderAddr to ""
                                    try
                                        set senderAddr to address of sender of msg
                                    end try
                                    set bodyPreview to ""
                                    try
                                        set bodyText to plain text content of msg
                                        if (count of bodyText) > 500 then
                                            set bodyPreview to text 1 thru 500 of bodyText
                                        else
                                            set bodyPreview to bodyText
                                        end if
                                    end try
                                    set output to output & "SUBJECT:" & subject of msg & "||FROM:" & senderAddr & "||DATE:" & (time sent of msg as string) & "||BODY:" & bodyPreview & "|||"
                                end repeat
                            end if
                        end if
                    end try
                end repeat
                if output is "" then return ""
                return output
            end tell
        "#;

        let raw = applescript::run(script).await?;

        if raw == "__NOT_RUNNING__" {
            info!("Email poll: Outlook not running, skipping");
            return Ok(Vec::new());
        }

        let emails = parse_emails(&raw);
        if !emails.is_empty() {
            info!("Email poll: {} unread emails found", emails.len());
        }
        Ok(emails)
    }
}

fn parse_emails(raw: &str) -> Vec<Event> {
    raw.split("|||")
        .filter(|s| !s.trim().is_empty())
        .filter_map(|entry| {
            let mut subject = String::new();
            let mut from = String::new();
            let mut body_preview = String::new();

            for field in entry.split("||") {
                if let Some(val) = field.strip_prefix("SUBJECT:") {
                    subject = val.to_string();
                } else if let Some(val) = field.strip_prefix("FROM:") {
                    from = val.to_string();
                } else if let Some(val) = field.strip_prefix("BODY:") {
                    body_preview = val.trim().to_string();
                }
            }

            if subject.is_empty() {
                return None;
            }

            let body = if body_preview.is_empty() {
                format!("From: {from}")
            } else {
                format!("From: {from}\n\n{body_preview}")
            };

            Some(Event {
                source: "email".into(),
                event_type: EventType::NewEmail,
                title: subject,
                body,
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
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_emails_normal_input() {
        let raw = "FROM:sender@example.com||SUBJECT:Hello World||DATE:2026-03-03|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Hello World");
        assert_eq!(result[0].body, "From: sender@example.com");
    }

    #[test]
    fn test_parse_emails_with_body() {
        let raw = "SUBJECT:Meeting||FROM:bob@test.com||DATE:2026-03-10||BODY:Hi team, let's meet at 3pm to discuss the project.|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Meeting");
        assert!(result[0].body.contains("Hi team, let's meet at 3pm"));
        assert!(result[0].body.starts_with("From: bob@test.com"));
    }

    #[test]
    fn test_parse_emails_missing_subject_filtered_out() {
        let raw = "FROM:sender@example.com||DATE:2026-03-03|||";
        let result = parse_emails(raw);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_emails_multiple_inboxes() {
        let raw = "SUBJECT:Mail A||FROM:a@test.com||DATE:2026-03-04|||SUBJECT:Mail B||FROM:||DATE:2026-03-04|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title, "Mail A");
        assert_eq!(result[1].title, "Mail B");
        // sender 为空时 metadata 仍应存在
        assert_eq!(result[1].metadata.get("from").unwrap(), "");
    }
}
