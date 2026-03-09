use chrono::{Datelike, Local, Timelike};
use tracing::info;

use crate::config::Config;
use sage_types::WorkSchedule;

#[derive(Debug)]
pub enum Action {
    MorningBrief,
    EveningReview,
    UrgentEmailCheck,
    UpcomingMeetingCheck,
    WeeklyReport,
    WeekStart,
    Idle,
}

/// 心跳调度器：根据时间、配置和用户日程决定需要执行哪些动作
pub fn evaluate(config: &Config, schedule: Option<&WorkSchedule>) -> Vec<Action> {
    let now = Local::now();
    let hour = now.hour();
    let weekday = now.weekday();
    let mut actions = Vec::new();

    let brief_hour = schedule.map(|s| s.morning_brief_hour).unwrap_or(8);
    let review_hour = schedule.map(|s| s.evening_review_hour).unwrap_or(18);
    let work_start = schedule.map(|s| s.work_start_hour).unwrap_or(8);
    let work_end = schedule.map(|s| s.work_end_hour).unwrap_or(19);
    let report_day = schedule
        .map(|s| s.weekly_report_day.to_chrono())
        .unwrap_or(chrono::Weekday::Fri);
    let report_hour = schedule.map(|s| s.weekly_report_hour).unwrap_or(16);

    // 早间 brief
    if hour == brief_hour {
        actions.push(Action::MorningBrief);
    }

    // 晚间回顾
    if hour == review_hour {
        actions.push(Action::EveningReview);
    }

    // 工作时间内持续检查邮件
    if config.channels.email.enabled && (work_start..work_end).contains(&hour) {
        actions.push(Action::UrgentEmailCheck);
    }

    // 工作时间内检查即将到来的会议
    if config.channels.calendar.enabled && (work_start..work_end).contains(&hour) {
        actions.push(Action::UpcomingMeetingCheck);
    }

    // 周报日下午生成周报
    if weekday == report_day && hour >= report_hour {
        actions.push(Action::WeeklyReport);
    }

    // 周一早上提醒本周重点
    if weekday == chrono::Weekday::Mon && hour == brief_hour {
        actions.push(Action::WeekStart);
    }

    if actions.is_empty() {
        actions.push(Action::Idle);
    }

    info!(
        "Heartbeat: {} actions for {:02}:{:02} {:?}",
        actions.len(),
        hour,
        now.minute(),
        weekday
    );
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(email_enabled: bool, calendar_enabled: bool) -> Config {
        use crate::config::{
            AgentConfig, ChannelsConfig, DaemonConfig, HooksConfig, MemoryConfig,
            PollChannelConfig, ToggleConfig, WechatConfig,
        };
        Config {
            daemon: DaemonConfig {
                heartbeat_interval_secs: 60,
                log_level: "info".into(),
                pid_file: "/tmp/sage.pid".into(),
            },
            memory: MemoryConfig {
                base_dir: "/tmp".into(),
                heartbeat_file: "heartbeat.json".into(),
            },
            agent: AgentConfig {
                provider: "claude".into(),
                claude_binary: "claude".into(),
                codex_binary: String::new(),
                gemini_binary: String::new(),
                default_model: "claude-sonnet-4-6".into(),
                project_dir: "/tmp".into(),
                max_budget_usd: 1.0,
                permission_mode: "default".into(),
                max_iterations: 10,
            },
            channels: ChannelsConfig {
                email: PollChannelConfig {
                    enabled: email_enabled,
                    poll_interval_secs: 300,
                },
                calendar: PollChannelConfig {
                    enabled: calendar_enabled,
                    poll_interval_secs: 300,
                },
                wechat: WechatConfig {
                    enabled: false,
                    events_file: "/tmp/wechat.json".into(),
                },
                hooks: HooksConfig {
                    enabled: false,
                    watch_dir: "/tmp/hooks".into(),
                },
                notification: ToggleConfig { enabled: false },
            },
        }
    }

    #[test]
    fn test_evaluate_returns_at_least_one_action() {
        let config = make_config(true, true);
        let actions = evaluate(&config, None);
        assert!(!actions.is_empty());
    }

    #[test]
    fn test_evaluate_channels_disabled_still_returns_action() {
        let config = make_config(false, false);
        let actions = evaluate(&config, None);
        assert!(!actions.is_empty());
    }
}
