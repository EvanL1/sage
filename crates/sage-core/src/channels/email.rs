use anyhow::Result;
use async_trait::async_trait;
use sage_types::{Event, EventType};
use serde_json::Value;
use tokio::process::Command;
use tracing::{info, warn};

use crate::channel::InputChannel;

pub struct EmailChannel {
    binary: String,
}

impl EmailChannel {
    pub fn new() -> Self {
        let binary = find_himalaya_binary();
        Self { binary }
    }
}

fn find_himalaya_binary() -> String {
    let candidates = [
        "himalaya",
        "/opt/homebrew/bin/himalaya",
        "/usr/local/bin/himalaya",
    ];
    for candidate in &candidates {
        if std::process::Command::new("which")
            .arg(candidate)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
            || std::path::Path::new(candidate).exists()
        {
            return candidate.to_string();
        }
    }
    "himalaya".to_string()
}

async fn run_himalaya(binary: &str, args: &[&str]) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Command::new(binary)
            .args(args)
            .env_remove("CLAUDECODE")
            .output(),
    )
    .await??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("himalaya {args:?} failed: {stderr}");
    }
    Ok(stdout)
}

fn parse_himalaya_list(json: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn extract_addr(from: &Value) -> String {
    if let Some(obj) = from.as_object() {
        let addr = obj
            .get("addr")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !name.is_empty() {
            return format!("{name} <{addr}>");
        }
        return addr.to_string();
    }
    from.as_str().unwrap_or_default().to_string()
}

fn is_unread(flags: &Value) -> bool {
    flags
        .as_array()
        .map(|arr| {
            arr.iter()
                .all(|f| f.as_str().map(|s| s != "seen").unwrap_or(true))
        })
        .unwrap_or(true)
}

fn build_event(msg: &Value, body: String) -> Option<Event> {
    let subject = msg
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if subject.is_empty() {
        return None;
    }

    let from_val = msg.get("from").cloned().unwrap_or(Value::Null);
    let from = extract_addr(&from_val);
    let date = msg
        .get("date")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let clean_body = strip_html(&body);
    let preview: String = clean_body.chars().take(500).collect();
    let suffix = if clean_body.chars().count() > 500 {
        "..."
    } else {
        ""
    };
    let body_text = if preview.is_empty() {
        format!("From: {from}")
    } else {
        format!("From: {from}\n\n{preview}{suffix}")
    };

    Some(Event {
        source: "email".into(),
        event_type: EventType::NewEmail,
        title: subject,
        body: body_text,
        metadata: [
            ("from".into(), from),
            ("priority".into(), "normal".into()),
            ("date".into(), date),
            ("direction".into(), "received".into()),
        ]
        .into_iter()
        .collect(),
        timestamp: chrono::Local::now(),
    })
}

#[async_trait]
impl InputChannel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        let list_json = run_himalaya(&self.binary, &["list", "-o", "json", "-s", "20"]).await;

        let list_json = match list_json {
            Ok(j) if !j.is_empty() => j,
            Ok(_) => {
                warn!("himalaya list returned empty output — is himalaya installed?");
                return Ok(Vec::new());
            }
            Err(e) => {
                warn!("himalaya not available: {e}");
                return Ok(Vec::new());
            }
        };

        let messages = parse_himalaya_list(&list_json);
        if messages.is_empty() {
            return Ok(Vec::new());
        }

        let mut events = Vec::new();
        for msg in &messages {
            let id = match msg.get("id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            let flags = msg.get("flags").cloned().unwrap_or(Value::Null);
            if !is_unread(&flags) {
                continue;
            }

            let body = run_himalaya(&self.binary, &["read", &id, "-o", "plain"])
                .await
                .unwrap_or_default();

            if let Some(event) = build_event(msg, body) {
                events.push(event);
            }
        }

        if !events.is_empty() {
            info!("Email poll: {} unread emails found", events.len());
        }
        Ok(events)
    }
}

/// Scan recent N hours of emails for Morning Brief context injection.
pub async fn scan_recent_emails(_hours: u32) -> Result<String> {
    let channel = EmailChannel::new();
    let list_json = run_himalaya(&channel.binary, &["list", "-o", "json", "-s", "30"]).await?;

    if list_json.is_empty() {
        return Ok(String::new());
    }

    let messages = parse_himalaya_list(&list_json);
    if messages.is_empty() {
        return Ok(String::new());
    }

    let mut lines = Vec::new();
    for msg in &messages {
        let id = match msg.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let subject = msg
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if subject.is_empty() {
            continue;
        }
        let from_val = msg.get("from").cloned().unwrap_or(Value::Null);
        let from = extract_addr(&from_val);
        let flags = msg.get("flags").cloned().unwrap_or(Value::Null);
        let tag = if is_unread(&flags) { "[未读]" } else { "[已读]" };

        let body = run_himalaya(&channel.binary, &["read", &id, "-o", "plain"])
            .await
            .unwrap_or_default();
        let clean = strip_html(&body);
        let preview: String = clean.chars().take(150).collect();
        let suffix = if clean.chars().count() > 150 {
            "..."
        } else {
            ""
        };

        if preview.is_empty() {
            lines.push(format!("- {tag} **{subject}** — {from}"));
        } else {
            lines.push(format!(
                "- {tag} **{subject}** — {from}\n  > {preview}{suffix}"
            ));
        }
    }

    if lines.is_empty() {
        return Ok(String::new());
    }

    Ok(format!("共 {} 封邮件：\n{}", lines.len(), lines.join("\n")))
}

/// Strip HTML tags, style blocks, and entities from text.
fn strip_html(input: &str) -> String {
    // 1. Remove <style>...</style> blocks
    let mut s = String::with_capacity(input.len());
    let lower = input.to_lowercase();
    let mut pos = 0;
    while pos < input.len() {
        if let Some(style_start) = lower[pos..].find("<style") {
            s.push_str(&input[pos..pos + style_start]);
            let after_tag = pos + style_start;
            if let Some(end_offset) = lower[after_tag..].find("</style") {
                let close_start = after_tag + end_offset;
                if let Some(gt) = input[close_start..].find('>') {
                    pos = close_start + gt + 1;
                } else {
                    pos = input.len();
                }
            } else {
                pos = input.len();
            }
        } else {
            s.push_str(&input[pos..]);
            break;
        }
    }

    // 2. Remove all <...> tags
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

    // 3. Decode basic HTML entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"");

    // 4. Collapse excess blank lines
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

/// Determine if an email event should be escalated to urgent.
pub fn should_upgrade_to_urgent(
    event: &Event,
    vip_domains: &[String],
    urgent_keywords: &[String],
) -> bool {
    if event
        .metadata
        .get("priority")
        .map(|p| p == "high")
        .unwrap_or(false)
    {
        return true;
    }
    let from = event.metadata.get("from").map(|s| s.as_str()).unwrap_or("");
    for domain in vip_domains {
        if from.contains(domain.as_str()) {
            return true;
        }
    }
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
    use serde_json::json;

    // --- himalaya JSON parsing ---

    #[test]
    fn test_parse_himalaya_list_empty_array() {
        let result = parse_himalaya_list("[]");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_himalaya_list_malformed_json() {
        let result = parse_himalaya_list("not json");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_himalaya_list_valid() {
        let json = r#"[{"id":"1","subject":"Hello","from":{"name":"John","addr":"john@example.com"},"date":"2026-03-21T10:00:00+08:00","flags":[]}]"#;
        let result = parse_himalaya_list(json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["subject"], "Hello");
    }

    #[test]
    fn test_extract_addr_object() {
        let v = json!({"name": "John", "addr": "john@example.com"});
        assert_eq!(extract_addr(&v), "John <john@example.com>");
    }

    #[test]
    fn test_extract_addr_no_name() {
        let v = json!({"name": "", "addr": "john@example.com"});
        assert_eq!(extract_addr(&v), "john@example.com");
    }

    #[test]
    fn test_extract_addr_string_fallback() {
        let v = Value::String("raw@addr.com".into());
        assert_eq!(extract_addr(&v), "raw@addr.com");
    }

    #[test]
    fn test_is_unread_no_seen_flag() {
        let flags = json!(["flagged"]);
        assert!(is_unread(&flags));
    }

    #[test]
    fn test_is_unread_with_seen_flag() {
        let flags = json!(["seen", "flagged"]);
        assert!(!is_unread(&flags));
    }

    #[test]
    fn test_is_unread_empty_flags() {
        let flags = json!([]);
        assert!(is_unread(&flags));
    }

    #[test]
    fn test_build_event_basic() {
        let msg = json!({
            "id": "42",
            "subject": "Test Subject",
            "from": {"name": "Alice", "addr": "alice@example.com"},
            "date": "2026-03-21T10:00:00+08:00",
            "flags": []
        });
        let event = build_event(&msg, "Hello body".into()).unwrap();
        assert_eq!(event.title, "Test Subject");
        assert_eq!(event.metadata.get("from").unwrap(), "Alice <alice@example.com>");
        assert!(event.body.contains("Hello body"));
    }

    #[test]
    fn test_build_event_no_subject_returns_none() {
        let msg = json!({"id": "1", "subject": "", "from": {"addr": "x@x.com"}, "flags": []});
        assert!(build_event(&msg, String::new()).is_none());
    }

    // --- strip_html (keep existing coverage) ---

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

    // --- should_upgrade_to_urgent ---

    #[test]
    fn test_should_upgrade_to_urgent_by_priority() {
        let event = Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
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
            event_type: EventType::NewEmail,
            title: "Regular update".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "sam@example.com".into()),
                ("priority".into(), "normal".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(should_upgrade_to_urgent(
            &event,
            &["example.com".into()],
            &[]
        ));
    }

    #[test]
    fn test_should_upgrade_to_urgent_by_keyword() {
        let event = Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
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
        assert!(should_upgrade_to_urgent(&event, &[], &["urgent".into()]));
    }

    #[test]
    fn test_should_not_upgrade_normal_email() {
        let event = Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
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
            &["example.com".into()],
            &["urgent".into()]
        ));
    }
}
