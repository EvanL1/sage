use anyhow::Result;
use async_trait::async_trait;

use crate::applescript;
use crate::channel::{Event, EventType, InputChannel};

pub struct CalendarChannel;

#[async_trait]
impl InputChannel for CalendarChannel {
    fn name(&self) -> &str {
        "calendar"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        // 中文 macOS 兼容：不用 default calendar，遍历所有日历
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
                            set output to output & "SUBJECT:" & evtSubject & "||START:" & evtStart & "||END:" & evtEnd & "||LOCATION:" & evtLocation & "|||"
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
            let mut location = String::new();

            for field in entry.split("||") {
                if let Some(val) = field.strip_prefix("SUBJECT:") {
                    subject = val.to_string();
                } else if let Some(val) = field.strip_prefix("START:") {
                    start = val.to_string();
                } else if let Some(val) = field.strip_prefix("LOCATION:") {
                    location = val.to_string();
                }
            }

            if subject.is_empty() {
                return None;
            }

            Some(Event {
                source: "calendar".into(),
                event_type: EventType::UpcomingMeeting,
                title: subject,
                body: format!("{start} @ {location}"),
                metadata: [("start".into(), start), ("location".into(), location)]
                    .into_iter()
                    .collect(),
                timestamp: chrono::Local::now(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_events_empty_string() {
        let result = parse_events("");
        assert!(result.is_empty(), "空字符串应返回空列表");
    }

    #[test]
    fn test_parse_events_whitespace_only() {
        let result = parse_events("   \n  ");
        assert!(result.is_empty(), "纯空白字符串应返回空列表");
    }

    #[test]
    fn test_parse_events_normal_input() {
        let raw = "SUBJECT:Meeting||START:2026-03-03 10:00||END:2026-03-03 11:00||LOCATION:Room A|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);

        let evt = &result[0];
        assert_eq!(evt.title, "Meeting");
        assert_eq!(evt.source, "calendar");
        assert_eq!(evt.metadata.get("start").map(|s| s.as_str()), Some("2026-03-03 10:00"));
        assert_eq!(evt.metadata.get("location").map(|s| s.as_str()), Some("Room A"));
        assert!(evt.body.contains("2026-03-03 10:00"));
        assert!(evt.body.contains("Room A"));
    }

    #[test]
    fn test_parse_events_multiple_entries() {
        let raw = "SUBJECT:Standup||START:2026-03-03 09:00||END:2026-03-03 09:30||LOCATION:Online|||SUBJECT:Review||START:2026-03-03 14:00||END:2026-03-03 15:00||LOCATION:Room B|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title, "Standup");
        assert_eq!(result[1].title, "Review");
    }

    #[test]
    fn test_parse_events_missing_subject_filtered() {
        // 缺少 SUBJECT，应被过滤掉
        let raw = "START:2026-03-03 10:00||END:2026-03-03 11:00||LOCATION:Room A|||";
        let result = parse_events(raw);
        assert!(result.is_empty(), "缺少 SUBJECT 的事件应被过滤");
    }

    #[test]
    fn test_parse_events_missing_location_defaults_to_empty() {
        // 缺少 LOCATION 字段，metadata 中 location 为空字符串
        let raw = "SUBJECT:No Location Meeting||START:2026-03-03 10:00||END:2026-03-03 11:00|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "No Location Meeting");
        assert_eq!(result[0].metadata.get("location").map(|s| s.as_str()), Some(""));
    }

    #[test]
    fn test_parse_events_mixed_valid_and_invalid() {
        let raw = "SUBJECT:Valid Meeting||START:2026-03-03 10:00||END:2026-03-03 11:00||LOCATION:Room C|||START:2026-03-03 12:00||END:2026-03-03 13:00||LOCATION:Room D|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Valid Meeting");
    }
}
