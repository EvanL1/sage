//! Read emails from locally-running Microsoft Outlook via AppleScript.
//! No IMAP/OAuth needed — reads directly from the Outlook desktop app.

use anyhow::Result;
use sage_types::EmailMessage;
use tracing::{info, warn};

use crate::applescript;

/// Check if Outlook is running
pub async fn is_outlook_running() -> bool {
    let script = r#"
        tell application "System Events"
            if exists process "Microsoft Outlook" then return "YES"
            return "NO"
        end tell
    "#;
    applescript::run(script)
        .await
        .map(|s| s.trim() == "YES")
        .unwrap_or(false)
}

/// Fetch recent emails from Outlook inbox via AppleScript.
/// Returns EmailMessage structs with source_id set to the provided value.
pub async fn fetch_outlook_emails(source_id: i64, limit: usize) -> Result<Vec<EmailMessage>> {
    let limit = limit.min(100); // Cap to prevent AppleScript timeout

    // IMPORTANT: Chinese macOS quirks:
    // - `plain text content` may return CSS/HTML fragments for HTML-only emails
    // - Use strip_html in Rust to clean up
    let half = limit / 2; // split limit between inbox and sent
    let script = format!(
        r#"
        tell application "System Events"
            if not (exists process "Microsoft Outlook") then return "__NOT_RUNNING__"
        end tell
        tell application "Microsoft Outlook"
            set output to ""
            set cutoff to current date
            set time of cutoff to 0
            set day of cutoff to (day of cutoff) - 7

            set accts to every exchange account
            repeat with acct in accts
                -- INBOX
                try
                    set inboxMsgs to (every message of (inbox of acct) whose time received >= cutoff)
                    set processed to 0
                    repeat with msg in inboxMsgs
                        if processed >= {half} then exit repeat
                        try
                            set msgSubject to subject of msg
                            set msgFrom to ""
                            try
                                set msgFrom to address of sender of msg
                            end try
                            -- Fallback: extract From from raw headers if sender is empty
                            if msgFrom is "" then
                                try
                                    set rawH to headers of msg
                                    set pList to paragraphs of rawH
                                    repeat with ln in pList
                                        if ln starts with "From:" then
                                            set msgFrom to text 6 thru -1 of ln
                                            exit repeat
                                        end if
                                    end repeat
                                end try
                            end if
                            set msgTo to ""
                            try
                                repeat with r in (to recipients of msg)
                                    set msgTo to msgTo & (email address of r) & ","
                                end repeat
                            end try
                            set msgDate to time received of msg as string
                            set msgId to id of msg as string
                            set readFlag to "0"
                            if is read of msg then set readFlag to "1"
                            set msgBody to ""
                            try
                                set msgBody to plain text content of msg
                                if msgBody is missing value then set msgBody to ""
                                if (count of msgBody) > 2000 then set msgBody to text 1 thru 2000 of msgBody
                            end try
                            set output to output & "ID:" & msgId & "||FROM:" & msgFrom & "||TO:" & msgTo & "||SUBJECT:" & msgSubject & "||DATE:" & msgDate & "||READ:" & readFlag & "||DIR:received||BODY:" & msgBody & "|||"
                            set processed to processed + 1
                        end try
                    end repeat
                end try

                -- SENT
                try
                    set sentMsgs to (every message of (sent items of acct) whose time sent >= cutoff)
                    set processed to 0
                    repeat with msg in sentMsgs
                        if processed >= {half} then exit repeat
                        try
                            set msgSubject to subject of msg
                            set msgFrom to ""
                            try
                                set msgFrom to address of sender of msg
                            end try
                            if msgFrom is "" then
                                try
                                    set rawH to headers of msg
                                    set pList to paragraphs of rawH
                                    repeat with ln in pList
                                        if ln starts with "From:" then
                                            set msgFrom to text 6 thru -1 of ln
                                            exit repeat
                                        end if
                                    end repeat
                                end try
                            end if
                            set msgTo to ""
                            try
                                repeat with r in (to recipients of msg)
                                    set msgTo to msgTo & (email address of r) & ","
                                end repeat
                            end try
                            set msgDate to time sent of msg as string
                            set msgId to id of msg as string
                            set readFlag to "1"
                            set msgBody to ""
                            try
                                set msgBody to plain text content of msg
                                if msgBody is missing value then set msgBody to ""
                                if (count of msgBody) > 2000 then set msgBody to text 1 thru 2000 of msgBody
                            end try
                            set output to output & "ID:" & msgId & "||FROM:" & msgFrom & "||TO:" & msgTo & "||SUBJECT:" & msgSubject & "||DATE:" & msgDate & "||READ:" & readFlag & "||DIR:sent||BODY:" & msgBody & "|||"
                            set processed to processed + 1
                        end try
                    end repeat
                end try
            end repeat
            return output
        end tell
    "#
    );

    let raw = applescript::run(&script).await?;
    if raw.trim() == "__NOT_RUNNING__" {
        warn!("Outlook is not running");
        return Ok(Vec::new());
    }
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    let emails = parse_outlook_emails(&raw, source_id);
    if !emails.is_empty() {
        info!("Fetched {} emails from local Outlook", emails.len());
    }
    Ok(emails)
}

fn parse_outlook_emails(raw: &str, source_id: i64) -> Vec<EmailMessage> {
    use crate::channels::email::strip_html_public;

    raw.split("|||")
        .filter(|s| !s.trim().is_empty())
        .filter_map(|entry| {
            let mut uid = String::new();
            let mut from_addr = String::new();
            let mut to_addr = String::new();
            let mut subject = String::new();
            let mut date = String::new();
            let mut is_read = false;
            let mut body_text = String::new();
            let mut direction = "received".to_string();

            for field in entry.split("||") {
                if let Some(val) = field.strip_prefix("ID:") {
                    uid = val.trim().to_string();
                } else if let Some(val) = field.strip_prefix("FROM:") {
                    from_addr = val.trim().to_string();
                } else if let Some(val) = field.strip_prefix("TO:") {
                    to_addr = val.trim_end_matches(',').trim().to_string();
                } else if let Some(val) = field.strip_prefix("SUBJECT:") {
                    subject = val.trim().to_string();
                } else if let Some(val) = field.strip_prefix("DATE:") {
                    date = val.trim().to_string();
                } else if let Some(val) = field.strip_prefix("READ:") {
                    is_read = val.trim() == "1";
                } else if let Some(val) = field.strip_prefix("DIR:") {
                    direction = val.trim().to_string();
                } else if let Some(val) = field.strip_prefix("BODY:") {
                    body_text = strip_html_public(val.trim());
                }
            }

            if subject.is_empty() && body_text.is_empty() {
                return None;
            }

            let folder = if direction == "sent" { "Sent" } else { "INBOX" };
            Some(EmailMessage {
                id: 0,
                source_id,
                uid,
                folder: folder.to_string(),
                from_addr,
                to_addr,
                subject,
                body_text,
                body_html: None,
                is_read,
                date,
                fetched_at: chrono::Local::now().to_rfc3339(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_outlook_emails_empty() {
        let result = parse_outlook_emails("", 1);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_outlook_emails_basic() {
        let raw = "ID:42||FROM:alice@example.com||TO:bob@example.com||SUBJECT:Hello||DATE:2026-03-22 10:00||READ:0||BODY:Test body|||";
        let result = parse_outlook_emails(raw, 1);
        assert_eq!(result.len(), 1);
        let msg = &result[0];
        assert_eq!(msg.uid, "42");
        assert_eq!(msg.from_addr, "alice@example.com");
        assert_eq!(msg.to_addr, "bob@example.com");
        assert_eq!(msg.subject, "Hello");
        assert!(!msg.is_read);
        assert_eq!(msg.source_id, 1);
        assert!(msg.body_text.contains("Test body"));
    }

    #[test]
    fn test_parse_outlook_emails_read_flag() {
        let raw = "ID:7||FROM:x@x.com||TO:y@y.com||SUBJECT:Meeting||DATE:2026-03-22||READ:1||BODY:|||";
        let result = parse_outlook_emails(raw, 2);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_read);
    }

    #[test]
    fn test_parse_outlook_emails_strips_html_body() {
        let raw = "ID:5||FROM:a@b.com||TO:c@d.com||SUBJECT:HTML Email||DATE:2026-03-22||READ:0||BODY:<p>Hello <b>world</b></p>|||";
        let result = parse_outlook_emails(raw, 1);
        assert_eq!(result.len(), 1);
        assert!(!result[0].body_text.contains("<p>"));
        assert!(result[0].body_text.contains("Hello"));
        assert!(result[0].body_text.contains("world"));
    }

    #[test]
    fn test_parse_outlook_emails_skips_empty_entry() {
        let raw = "ID:1||FROM:||TO:||SUBJECT:||DATE:||READ:0||BODY:|||";
        let result = parse_outlook_emails(raw, 1);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_outlook_emails_multiple() {
        let raw = "ID:1||FROM:a@a.com||TO:b@b.com||SUBJECT:First||DATE:2026-03-22||READ:0||BODY:body1|||ID:2||FROM:c@c.com||TO:d@d.com||SUBJECT:Second||DATE:2026-03-22||READ:1||BODY:body2|||";
        let result = parse_outlook_emails(raw, 3);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].subject, "First");
        assert_eq!(result[1].subject, "Second");
    }
}
