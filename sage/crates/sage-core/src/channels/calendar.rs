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

/// 扫描今日所有日历事件，返回格式化文本供 Morning Brief 注入
pub async fn scan_today_events() -> Result<String> {
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

    Ok(format_calendar_digest(&raw))
}

/// 格式化日历事件为 Markdown 摘要
fn format_calendar_digest(raw: &str) -> String {
    let entries: Vec<&str> = raw.split("|||").filter(|s| !s.trim().is_empty()).collect();
    if entries.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
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

        let mut detail = format!("- **{subject}** ({start} — {end_time})");
        if !location.is_empty() {
            detail.push_str(&format!("\n  Location: {location}"));
        }
        if !organizer.is_empty() {
            detail.push_str(&format!("\n  Organizer: {organizer}"));
        }
        if !attendees.is_empty() {
            detail.push_str(&format!("\n  Attendees: {attendees}"));
        }
        lines.push(detail);
    }

    format!("今日 {} 个会议：\n{}", lines.len(), lines.join("\n"))
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
        let raw = "SUBJECT:Meeting||START:2026-03-03 10:00||END:2026-03-03 11:00||LOCATION:Room A|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Meeting");
    }

    #[test]
    fn test_parse_events_with_attendees() {
        let raw = "SUBJECT:Sprint Review||START:2026-03-10 10:00||END:2026-03-10 11:00||LOCATION:Room A||ATTENDEES:alice@co.com,bob@co.com,||ORGANIZER:evan@co.com|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Sprint Review");
        assert!(result[0]
            .metadata
            .get("attendees")
            .unwrap()
            .contains("alice@co.com"));
        assert_eq!(result[0].metadata.get("organizer").unwrap(), "evan@co.com");
    }

    #[test]
    fn test_format_calendar_digest() {
        let raw = "SUBJECT:Team Standup||START:09:00||END:09:30||LOCATION:Teams||ATTENDEES:a@co.com,b@co.com,||ORGANIZER:evan@co.com|||SUBJECT:1:1 with Shawn||START:14:00||END:14:30||LOCATION:||ATTENDEES:shawn@co.com,||ORGANIZER:shawn@co.com|||";
        let digest = format_calendar_digest(raw);
        assert!(digest.contains("今日 2 个会议"));
        assert!(digest.contains("Team Standup"));
        assert!(digest.contains("1:1 with Shawn"));
        assert!(digest.contains("a@co.com"));
    }

    #[test]
    fn test_format_calendar_digest_empty() {
        assert!(format_calendar_digest("").is_empty());
    }
}
