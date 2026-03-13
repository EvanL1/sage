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
                                        if (count of bodyText) > 2000 then
                                            set bodyPreview to text 1 thru 2000 of bodyText
                                        else
                                            set bodyPreview to bodyText
                                        end if
                                    end try
                                    set msgPriority to "normal"
                                    try
                                        set p to priority of msg
                                        if p is priority high then set msgPriority to "high"
                                        if p is priority low then set msgPriority to "low"
                                    end try
                                    set output to output & "SUBJECT:" & subject of msg & "||FROM:" & senderAddr & "||DATE:" & (time sent of msg as string) & "||PRIORITY:" & msgPriority & "||BODY:" & bodyPreview & "|||"
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

/// 扫描最近 N 小时内收到的所有邮件（不管已读未读），用于 Morning Brief 上下文
///
/// 返回格式化的摘要文本（非 Event），供 context_gatherer 直接注入 prompt。
pub async fn scan_recent_emails(hours: u32) -> Result<String> {
    // AppleScript 中用 current date 减去秒数作为时间截止点
    let seconds = hours * 3600;
    let script = format!(
        r#"
        tell application "System Events"
            if not (exists process "Microsoft Outlook") then return "__NOT_RUNNING__"
        end tell
        tell application "Microsoft Outlook"
            set cutoff to (current date) - {seconds}
            set allFolders to every mail folder
            set output to ""
            set totalCount to 0
            repeat with f in allFolders
                try
                    set folderName to name of f
                    if folderName is "收件箱" or folderName is "Inbox" then
                        set recentMsgs to (messages of f whose time sent > cutoff)
                        set msgCount to count of recentMsgs
                        if msgCount > 0 then
                            set maxCount to msgCount
                            if maxCount > 15 then set maxCount to 15
                            repeat with i from 1 to maxCount
                                set msg to item i of recentMsgs
                                set senderAddr to ""
                                try
                                    set senderAddr to address of sender of msg
                                end try
                                set bodyPreview to ""
                                try
                                    set bodyText to plain text content of msg
                                    if (count of bodyText) > 2000 then
                                        set bodyPreview to text 1 thru 2000 of bodyText
                                    else
                                        set bodyPreview to bodyText
                                    end if
                                end try
                                set readStatus to "unread"
                                if is read of msg then set readStatus to "read"
                                set output to output & "SUBJECT:" & subject of msg & "||FROM:" & senderAddr & "||DATE:" & (time sent of msg as string) & "||STATUS:" & readStatus & "||BODY:" & bodyPreview & "|||"
                                set totalCount to totalCount + 1
                            end repeat
                        end if
                    end if
                end try
            end repeat
            if output is "" then return ""
            return output
        end tell
        "#
    );

    let raw = applescript::run(&script).await?;

    if raw == "__NOT_RUNNING__" || raw.is_empty() {
        return Ok(String::new());
    }

    let summary = format_email_digest(&raw);
    Ok(summary)
}

/// 将扫描结果格式化为简洁摘要（供 Morning Brief prompt 注入）
fn format_email_digest(raw: &str) -> String {
    let entries: Vec<&str> = raw.split("|||").filter(|s| !s.trim().is_empty()).collect();
    if entries.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    for entry in &entries {
        let mut subject = String::new();
        let mut from = String::new();
        let mut status = String::new();
        let mut body_preview = String::new();

        for field in entry.split("||") {
            if let Some(val) = field.strip_prefix("SUBJECT:") {
                subject = val.to_string();
            } else if let Some(val) = field.strip_prefix("FROM:") {
                from = val.to_string();
            } else if let Some(val) = field.strip_prefix("STATUS:") {
                status = val.to_string();
            } else if let Some(val) = field.strip_prefix("BODY:") {
                body_preview = val.trim().to_string();
            }
        }

        if subject.is_empty() {
            continue;
        }

        let tag = if status == "unread" { "[未读]" } else { "[已读]" };
        let clean = strip_html(&body_preview);
        let preview: String = clean.chars().take(150).collect();
        let preview_suffix = if clean.chars().count() > 150 { "..." } else { "" };

        if preview.is_empty() {
            lines.push(format!("- {tag} **{subject}** — {from}"));
        } else {
            lines.push(format!("- {tag} **{subject}** — {from}\n  > {preview}{preview_suffix}"));
        }
    }

    format!("共 {} 封邮件：\n{}", entries.len(), lines.join("\n"))
}

fn parse_emails(raw: &str) -> Vec<Event> {
    raw.split("|||")
        .filter(|s| !s.trim().is_empty())
        .filter_map(|entry| {
            let mut subject = String::new();
            let mut from = String::new();
            let mut priority = String::from("normal");
            let mut body_preview = String::new();
            let mut date_raw = String::new();

            for field in entry.split("||") {
                if let Some(val) = field.strip_prefix("SUBJECT:") {
                    subject = val.to_string();
                } else if let Some(val) = field.strip_prefix("FROM:") {
                    from = val.to_string();
                } else if let Some(val) = field.strip_prefix("PRIORITY:") {
                    priority = val.to_string();
                } else if let Some(val) = field.strip_prefix("BODY:") {
                    body_preview = val.trim().to_string();
                } else if let Some(val) = field.strip_prefix("DATE:") {
                    date_raw = val.trim().to_string();
                }
            }

            if subject.is_empty() {
                return None;
            }

            let clean_body = strip_html(&body_preview);
            let body = if clean_body.is_empty() {
                format!("From: {from}")
            } else {
                // 截取前 500 字符作为预览，避免超长邮件占满 Dashboard
                let preview: String = clean_body.chars().take(500).collect();
                let suffix = if clean_body.chars().count() > 500 { "..." } else { "" };
                format!("From: {from}\n\n{preview}{suffix}")
            };

            Some(Event {
                source: "email".into(),
                event_type: EventType::NewEmail,
                title: subject,
                body,
                metadata: [
                    ("from".into(), from),
                    ("priority".into(), priority),
                    ("date".into(), date_raw),
                ]
                .into_iter()
                .collect(),
                timestamp: chrono::Local::now(),
            })
        })
        .collect()
}

/// 剥离 HTML 标签、`<style>` 块、CSS 残留，返回干净纯文本
fn strip_html(input: &str) -> String {
    // 1. 移除 <style>...</style> 块（含内容）— 用 find 而非逐字节遍历，保证 UTF-8 安全
    let mut s = String::with_capacity(input.len());
    let lower = input.to_lowercase();
    let mut pos = 0;
    while pos < input.len() {
        if let Some(style_start) = lower[pos..].find("<style") {
            // 拷贝 <style 之前的内容
            s.push_str(&input[pos..pos + style_start]);
            let after_tag = pos + style_start;
            // 找 </style> 结束
            if let Some(end_offset) = lower[after_tag..].find("</style") {
                let close_start = after_tag + end_offset;
                // 跳过 </style> 标签本身（到 > 为止）
                if let Some(gt) = input[close_start..].find('>') {
                    pos = close_start + gt + 1;
                } else {
                    pos = input.len();
                }
            } else {
                // 没有闭合标签，跳过到末尾
                pos = input.len();
            }
        } else {
            s.push_str(&input[pos..]);
            break;
        }
    }

    // 2. 移除所有 <...> 标签
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    // 3. 基础 HTML 实体解码
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"");

    // 4. 压缩连续空白行和空格
    let mut cleaned = String::with_capacity(result.len());
    let mut blank_count = 0u32;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                cleaned.push('\n');
            }
        } else {
            blank_count = 0;
            if !cleaned.is_empty() && !cleaned.ends_with('\n') {
                cleaned.push('\n');
            }
            cleaned.push_str(trimmed);
        }
    }

    cleaned.trim().to_string()
}

/// 判断邮件是否应升级为紧急
/// 基于：优先级标记、发件人域名、主题关键词
pub fn should_upgrade_to_urgent(
    event: &Event,
    vip_domains: &[String],
    urgent_keywords: &[String],
) -> bool {
    // 1. Outlook priority = high
    if event
        .metadata
        .get("priority")
        .map(|p| p == "high")
        .unwrap_or(false)
    {
        return true;
    }
    // 2. 发件人域名匹配 VIP 列表
    let from = event
        .metadata
        .get("from")
        .map(|s| s.as_str())
        .unwrap_or("");
    for domain in vip_domains {
        if from.contains(domain.as_str()) {
            return true;
        }
    }
    // 3. 主题包含紧急关键词
    let title_lower = event.title.to_lowercase();
    for kw in urgent_keywords {
        if title_lower.contains(&kw.to_lowercase()) {
            return true;
        }
    }
    false
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
        // body truncation limit is 2000 chars (enforced in AppleScript; parse_emails handles any length)
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

    #[test]
    fn test_format_email_digest_mixed_status() {
        let raw = "SUBJECT:Weekly Update||FROM:boss@co.com||DATE:2026-03-10||STATUS:read||BODY:Please review the Q1 numbers.|||SUBJECT:Urgent: Server Down||FROM:ops@co.com||DATE:2026-03-10||STATUS:unread||BODY:Production is offline.|||";
        let digest = format_email_digest(raw);
        assert!(digest.contains("共 2 封邮件"));
        assert!(digest.contains("[已读]"));
        assert!(digest.contains("[未读]"));
        assert!(digest.contains("Weekly Update"));
        assert!(digest.contains("Server Down"));
        assert!(digest.contains("Q1 numbers"));
    }

    #[test]
    fn test_format_email_digest_empty() {
        assert!(format_email_digest("").is_empty());
    }

    #[test]
    fn test_parse_emails_with_priority() {
        let raw = "SUBJECT:Urgent Fix||FROM:boss@voltageenergy.com||DATE:2026-03-10||PRIORITY:high||BODY:Need fix ASAP|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].metadata.get("priority").unwrap(), "high");
    }

    #[test]
    fn test_should_upgrade_to_urgent_by_priority() {
        let event = Event {
            source: "email".into(),
            event_type: sage_types::EventType::NewEmail,
            title: "Normal subject".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "someone@random.com".into()),
                ("priority".into(), "high".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(should_upgrade_to_urgent(&event, &[], &[]));
    }

    #[test]
    fn test_should_upgrade_to_urgent_by_domain() {
        let event = Event {
            source: "email".into(),
            event_type: sage_types::EventType::NewEmail,
            title: "Regular update".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "bob@voltageenergy.com".into()),
                ("priority".into(), "normal".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(should_upgrade_to_urgent(
            &event,
            &["voltageenergy.com".into()],
            &[]
        ));
    }

    #[test]
    fn test_should_upgrade_to_urgent_by_keyword() {
        let event = Event {
            source: "email".into(),
            event_type: sage_types::EventType::NewEmail,
            title: "URGENT: Server down".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "ops@random.com".into()),
                ("priority".into(), "normal".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(should_upgrade_to_urgent(
            &event,
            &[],
            &["urgent".into()]
        ));
    }

    #[test]
    fn test_should_not_upgrade_normal_email() {
        let event = Event {
            source: "email".into(),
            event_type: sage_types::EventType::NewEmail,
            title: "Weekly newsletter".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "news@random.com".into()),
                ("priority".into(), "normal".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(!should_upgrade_to_urgent(
            &event,
            &["voltageenergy.com".into()],
            &["urgent".into()]
        ));
    }

    #[test]
    fn test_strip_html_basic_tags() {
        assert_eq!(strip_html("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn test_strip_html_style_block() {
        let input = "Before<style type=\"text/css\">.foo{color:red}</style>After";
        assert_eq!(strip_html(input), "BeforeAfter");
    }

    #[test]
    fn test_strip_html_css_fragments() {
        let input = "<text-decoration:none; color:#2A3C42> some text <font-size:14px> more";
        let result = strip_html(input);
        assert_eq!(result, "some text  more");
    }

    #[test]
    fn test_strip_html_entities() {
        assert_eq!(strip_html("A &amp; B &lt; C"), "A & B < C");
    }

    #[test]
    fn test_strip_html_collapses_blank_lines() {
        let input = "Line 1\n\n\n\nLine 2";
        let result = strip_html(input);
        assert_eq!(result, "Line 1\nLine 2");
    }

    #[test]
    fn test_strip_html_empty_input() {
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn test_parse_emails_strips_html_from_body() {
        let raw = "SUBJECT:Newsletter||FROM:news@co.com||DATE:2026-03-10||BODY:<p>Hello <b>world</b></p>|||";
        let result = parse_emails(raw);
        assert_eq!(result.len(), 1);
        assert!(result[0].body.contains("Hello world"));
        assert!(!result[0].body.contains("<p>"));
    }
}
