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
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_events_normal_input() {
        let raw = "SUBJECT:Meeting||START:2026-03-03 10:00||END:2026-03-03 11:00||LOCATION:Room A|||";
        let result = parse_events(raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Meeting");
    }
}
