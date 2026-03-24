use anyhow::Result;
use async_trait::async_trait;
use sage_types::{Event, EventType};

use crate::applescript;
use crate::channel::InputChannel;

pub struct CalendarChannel;

#[async_trait]
impl InputChannel for CalendarChannel {
    fn name(&self) -> &str {
        "calendar"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        let script = r#"
            tell application "System Events"
                if not (exists process "Microsoft Outlook") then return ""
            end tell
            tell application "Microsoft Outlook"
                set todayStart to current date
                set time of todayStart to 0
                set todayEnd to todayStart + (1 * days)
                set allCalendars to every calendar
                set output to ""
                repeat with cal in allCalendars
                    try
                        set calEvents to (every calendar event of cal whose start time >= todayStart and start time < todayEnd)
                        repeat with evt in calEvents
                            set evtSubject to subject of evt
                            set evtStart to start time of evt as string
                            set evtEnd to end time of evt as string
                            set evtLocation to ""
                            try
                                set evtLocation to location of evt
                            end try
                            set attendeeList to ""
                            try
                                set reqAttendees to required attendees of evt
                                repeat with att in reqAttendees
                                    set attendeeList to attendeeList & (email address of att) & ","
                                end repeat
                            end try
                            set evtOrganizer to ""
                            try
                                set evtOrganizer to address of organizer of evt
                            end try
                            set evtBody to ""
                            try
                                set evtBody to plain text content of evt
                                if (count of evtBody) > 500 then
                                    set evtBody to text 1 thru 500 of evtBody
                                end if
                            end try
                            set output to output & "SUBJECT:" & evtSubject & "||START:" & evtStart & "||END:" & evtEnd & "||LOCATION:" & evtLocation & "||ATTENDEES:" & attendeeList & "||ORGANIZER:" & evtOrganizer & "||BODY:" & evtBody & "|||"
                        end repeat
                    end try
                end repeat
                return output
            end tell
        "#;

        let raw = applescript::run(script).await?;
        Ok(parse_events(&raw))
    }
}

fn parse_events(raw: &str) -> Vec<Event> {
    raw.split("|||")
        .filter(|s| !s.trim().is_empty())
        .filter_map(|entry| {
            let mut subject = String::new();
            let mut start = String::new();
            let mut end_time = String::new();
            let mut location = String::new();
            let mut attendees = String::new();
            let mut organizer = String::new();
            let mut description = String::new();

            for field in entry.split("||") {
                if let Some(val) = field.strip_prefix("SUBJECT:") {
                    subject = val.to_string();
                } else if let Some(val) = field.strip_prefix("START:") {
                    start = val.to_string();
                } else if let Some(val) = field.strip_prefix("END:") {
                    end_time = val.to_string();
                } else if let Some(val) = field.strip_prefix("LOCATION:") {
                    location = val.to_string();
                } else if let Some(val) = field.strip_prefix("ATTENDEES:") {
                    attendees = val.trim_end_matches(',').to_string();
                } else if let Some(val) = field.strip_prefix("ORGANIZER:") {
                    organizer = val.to_string();
                } else if let Some(val) = field.strip_prefix("BODY:") {
                    description = val.to_string();
                }
            }

            if subject.is_empty() {
                return None;
            }

            let body = format!(
                "{start} @ {location}\nOrganizer: {organizer}\nAttendees: {attendees}\n{description}"
            );

            Some(Event {
                source: "calendar".into(),
                event_type: EventType::UpcomingMeeting,
                title: subject,
                body,
                metadata: [
                    ("start".into(), start),
                    ("end".into(), end_time),
                    ("location".into(), location),
                    ("organizer".into(), organizer),
                    ("attendees".into(), attendees),
                ]
                .into_iter()
                .collect(),
                timestamp: chrono::Local::now(),
            })
        })
        .collect()
}

/// 扫描今日所有日历事件，根据 source 配置选择 Outlook/Apple/both
/// source: "outlook"（默认）, "apple", "both"
pub async fn scan_today_events(source: &str) -> Result<String> {
    let mut parts = Vec::new();

    if source == "outlook" || source == "both" {
        if let Ok(raw) = scan_outlook_events().await {
            if !raw.is_empty() {
                parts.push(raw);
            }
        }
    }

    if source == "apple" || source == "both" {
        if let Ok(raw) = scan_apple_events().await {
            if !raw.is_empty() {
                parts.push(raw);
            }
        }
    }

    // 默认 fallback 到 outlook
    if parts.is_empty() && source != "outlook" && source != "apple" && source != "both" {
        if let Ok(raw) = scan_outlook_events().await {
            if !raw.is_empty() {
                parts.push(raw);
            }
        }
    }

    let combined = parts.join("|||");
    if combined.is_empty() {
        return Ok(String::new());
    }

    Ok(format_calendar_digest(&combined))
}

/// 扫描 Microsoft Outlook 今日事件
async fn scan_outlook_events() -> Result<String> {
    let script = r#"
        tell application "System Events"
            if not (exists process "Microsoft Outlook") then return "__NOT_RUNNING__"
        end tell
        tell application "Microsoft Outlook"
            set todayStart to current date
            set time of todayStart to 0
            set todayEnd to todayStart + (1 * days)
            set allCalendars to every calendar
            set output to ""
            repeat with cal in allCalendars
                try
                    set calEvents to (every calendar event of cal whose start time >= todayStart and start time < todayEnd)
                    repeat with evt in calEvents
                        set evtSubject to subject of evt
                        set evtStart to start time of evt as string
                        set evtEnd to end time of evt as string
                        set evtLocation to ""
                        try
                            set evtLocation to location of evt
                        end try
                        set attendeeList to ""
                        try
                            set reqAttendees to required attendees of evt
                            repeat with att in reqAttendees
                                set attendeeList to attendeeList & (email address of att) & ","
                            end repeat
                        end try
                        set evtOrganizer to ""
                        try
                            set evtOrganizer to address of organizer of evt
                        end try
                        set output to output & "SUBJECT:" & evtSubject & "||START:" & evtStart & "||END:" & evtEnd & "||LOCATION:" & evtLocation & "||ATTENDEES:" & attendeeList & "||ORGANIZER:" & evtOrganizer & "|||"
                    end repeat
                end try
            end repeat
            if output is "" then return ""
            return output
        end tell
    "#;

    let raw = applescript::run(script).await?;
    if raw == "__NOT_RUNNING__" || raw.is_empty() {
        return Ok(String::new());
    }
    Ok(raw)
}

/// 扫描 macOS Calendar.app (Apple Calendar) 今日事件
async fn scan_apple_events() -> Result<String> {
    let script = r#"
        tell application "System Events"
            if not (exists process "Calendar") then return "__NOT_RUNNING__"
        end tell
        tell application "Calendar"
            set todayStart to current date
            set time of todayStart to 0
            set todayEnd to todayStart + (1 * days)
            set output to ""
            repeat with cal in calendars
                try
                    set calEvents to (every event of cal whose start date >= todayStart and start date < todayEnd)
                    repeat with evt in calEvents
                        set evtSubject to summary of evt
                        set evtStart to start date of evt as string
                        set evtEnd to end date of evt as string
                        set evtLocation to ""
                        try
                            set evtLocation to location of evt
                        end try
                        set attendeeList to ""
                        try
                            set attList to attendees of evt
                            repeat with att in attList
                                try
                                    set attendeeList to attendeeList & (display name of att) & ","
                                end try
                            end repeat
                        end try
                        set evtOrganizer to ""
                        try
                            set org to organizer of evt
                            set evtOrganizer to display name of org
                        end try
                        set output to output & "SUBJECT:" & evtSubject & "||START:" & evtStart & "||END:" & evtEnd & "||LOCATION:" & evtLocation & "||ATTENDEES:" & attendeeList & "||ORGANIZER:" & evtOrganizer & "|||"
                    end repeat
                end try
            end repeat
            if output is "" then return ""
            return output
        end tell
    "#;

    let raw = applescript::run(script).await?;
    if raw == "__NOT_RUNNING__" || raw.is_empty() {
        return Ok(String::new());
    }
    Ok(raw)
}

/// 格式化日历事件为 Markdown 摘要，按时效分类
fn format_calendar_digest(raw: &str) -> String {
    let entries: Vec<&str> = raw.split("|||").filter(|s| !s.trim().is_empty()).collect();
    if entries.is_empty() {
        return String::new();
    }

    let now = chrono::Local::now();
    let now_str = now.format("%H:%M").to_string();

    struct CalEvent {
        subject: String,
        start: String,
        end_time: String,
        location: String,
        attendees: String,
        organizer: String,
        status: &'static str, // "past" | "now" | "upcoming"
    }

    let mut events = Vec::new();
    for entry in &entries {
        let mut subject = String::new();
        let mut start = String::new();
        let mut end_time = String::new();
        let mut location = String::new();
        let mut attendees = String::new();
        let mut organizer = String::new();

        for field in entry.split("||") {
            if let Some(val) = field.strip_prefix("SUBJECT:") {
                subject = val.to_string();
            } else if let Some(val) = field.strip_prefix("START:") {
                start = val.to_string();
            } else if let Some(val) = field.strip_prefix("END:") {
                end_time = val.to_string();
            } else if let Some(val) = field.strip_prefix("LOCATION:") {
                location = val.to_string();
            } else if let Some(val) = field.strip_prefix("ATTENDEES:") {
                attendees = val.trim_end_matches(',').to_string();
            } else if let Some(val) = field.strip_prefix("ORGANIZER:") {
                organizer = val.to_string();
            }
        }

        if subject.is_empty() {
            continue;
        }

        // 解析时间判断状态：尝试从字符串中提取 HH:MM
        let status = match (extract_time(&start), extract_time(&end_time)) {
            (Some(s), Some(e)) => {
                if e <= now_str {
                    "past"
                } else if s <= now_str {
                    "now"
                } else {
                    "upcoming"
                }
            }
            _ => "upcoming", // 解析失败默认为即将开始
        };

        events.push(CalEvent {
            subject,
            start,
            end_time,
            location,
            attendees,
            organizer,
            status,
        });
    }

    let upcoming: Vec<_> = events.iter().filter(|e| e.status == "upcoming").collect();
    let in_progress: Vec<_> = events.iter().filter(|e| e.status == "now").collect();
    let past: Vec<_> = events.iter().filter(|e| e.status == "past").collect();

    let format_event = |e: &CalEvent| -> String {
        let mut detail = format!("- **{}** ({} — {})", e.subject, e.start, e.end_time);
        if !e.location.is_empty() {
            detail.push_str(&format!("\n  Location: {}", e.location));
        }
        if !e.organizer.is_empty() {
            detail.push_str(&format!("\n  Organizer: {}", e.organizer));
        }
        if !e.attendees.is_empty() {
            detail.push_str(&format!("\n  Attendees: {}", e.attendees));
        }
        detail
    };

    let mut sections = Vec::new();
    sections.push(format!(
        "当前时间 {}，今日共 {} 个会议：",
        now_str,
        events.len()
    ));

    if !in_progress.is_empty() {
        let lines: Vec<_> = in_progress.iter().map(|e| format_event(e)).collect();
        sections.push(format!("### 正在进行\n{}", lines.join("\n")));
    }
    if !upcoming.is_empty() {
        let lines: Vec<_> = upcoming.iter().map(|e| format_event(e)).collect();
        sections.push(format!("### 即将开始（需准备）\n{}", lines.join("\n")));
    }
    if !past.is_empty() {
        let lines: Vec<_> = past.iter().map(|e| format_event(e)).collect();
        sections.push(format!("### 已结束\n{}", lines.join("\n")));
    }

    sections.join("\n")
}

/// 从日期时间字符串中提取 HH:MM（支持多种格式）
fn extract_time(datetime_str: &str) -> Option<String> {
    // 找第一个 "数字:两位数字" 模式
    let bytes = datetime_str.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' && i >= 1 && i + 2 < bytes.len() {
            // 往前找1-2位数字
            let h_start = if i >= 2 && bytes[i - 2].is_ascii_digit() {
                i - 2
            } else if bytes[i - 1].is_ascii_digit() {
                i - 1
            } else {
                continue;
            };
            // 往后找2位数字
            if !bytes[i + 1].is_ascii_digit() || !bytes[i + 2].is_ascii_digit() {
                continue;
            }
            let h: u32 = datetime_str[h_start..i].parse().ok()?;
            let m: u32 = datetime_str[i + 1..i + 3].parse().ok()?;
            if h > 23 || m > 59 {
                continue;
            }
            // 处理中文上午/下午
            let h = if datetime_str.contains("下午") && h < 12 {
                h + 12
            } else if datetime_str.contains("上午") && h == 12 {
                0
            } else {
                h
            };
            return Some(format!("{:02}:{:02}", h, m));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_events_empty_string() {
        let result = parse_events("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_events_normal_input() {
        let raw =
            "SUBJECT:Meeting||START:2026-03-03 10:00||END:2026-03-03 11:00||LOCATION:Room A|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Meeting");
    }

    #[test]
    fn test_parse_events_with_attendees() {
        let raw = "SUBJECT:Sprint Review||START:2026-03-10 10:00||END:2026-03-10 11:00||LOCATION:Room A||ATTENDEES:alice@example.com,bob@example.com,||ORGANIZER:alex@example.com|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Sprint Review");
        assert!(result[0]
            .metadata
            .get("attendees")
            .unwrap()
            .contains("alice@example.com"));
        assert_eq!(
            result[0].metadata.get("organizer").unwrap(),
            "alex@example.com"
        );
    }

    #[test]
    fn test_format_calendar_digest() {
        let raw = "SUBJECT:Team Standup||START:09:00||END:09:30||LOCATION:Teams||ATTENDEES:a@example.com,b@example.com,||ORGANIZER:alex@example.com|||SUBJECT:1:1 with Jordan||START:14:00||END:14:30||LOCATION:||ATTENDEES:jordan@example.com,||ORGANIZER:jordan@example.com|||";
        let digest = format_calendar_digest(raw);
        assert!(digest.contains("2 个会议"));
        assert!(digest.contains("Team Standup"));
        assert!(digest.contains("1:1 with Jordan"));
        assert!(digest.contains("a@example.com"));
    }

    #[test]
    fn test_format_calendar_digest_empty() {
        assert!(format_calendar_digest("").is_empty());
    }
}
