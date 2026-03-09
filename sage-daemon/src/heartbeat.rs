use chrono::{Datelike, Local, Timelike};
use tracing::info;

use crate::config::Config;

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

/// 心跳调度器：根据时间和配置决定需要执行哪些动作
pub fn evaluate(config: &Config) -> Vec<Action> {
    let now = Local::now();
    let hour = now.hour();
    let weekday = now.weekday();
    let mut actions = Vec::new();

    // 早间 brief (8:00-8:59)
    if hour == 8 {
        actions.push(Action::MorningBrief);
    }

    // 晚间回顾 (18:00-18:59)
    if hour == 18 {
        actions.push(Action::EveningReview);
    }

    // 工作时间内持续检查邮件
    if config.channels.email.enabled && (8..19).contains(&hour) {
        actions.push(Action::UrgentEmailCheck);
    }

    // 工作时间内检查即将到来的会议
    if config.channels.calendar.enabled && (8..19).contains(&hour) {
        actions.push(Action::UpcomingMeetingCheck);
    }

    // 周五下午生成周报
    if weekday == chrono::Weekday::Fri && hour >= 16 {
        actions.push(Action::WeeklyReport);
    }

    // 周一早上提醒本周重点
    if weekday == chrono::Weekday::Mon && hour == 8 {
        actions.push(Action::WeekStart);
    }

    if actions.is_empty() {
        actions.push(Action::Idle);
    }

    info!("Heartbeat: {} actions for {:02}:{:02} {:?}",
        actions.len(), hour, now.minute(), weekday);
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
            },
            channels: ChannelsConfig {
                email: PollChannelConfig { enabled: email_enabled, poll_interval_secs: 300 },
                calendar: PollChannelConfig { enabled: calendar_enabled, poll_interval_secs: 300 },
                wechat: WechatConfig { enabled: false, events_file: "/tmp/wechat.json".into() },
                hooks: HooksConfig { enabled: false, watch_dir: "/tmp/hooks".into() },
                notification: ToggleConfig { enabled: false },
            },
        }
    }

    #[test]
    fn test_evaluate_smoke_test_returns_at_least_one_action() {
        // smoke test：evaluate 不应 panic，且始终返回至少一个 Action
        let config = make_config(true, true);
        let actions = evaluate(&config);
        assert!(!actions.is_empty(), "evaluate 应至少返回一个 Action");
    }

    #[test]
    fn test_evaluate_channels_disabled_still_returns_action() {
        // 禁用所有渠道时，evaluate 仍应正常返回（不 panic）
        let config = make_config(false, false);
        let actions = evaluate(&config);
        assert!(!actions.is_empty(), "禁用渠道时 evaluate 也应至少返回一个 Action");
    }
}
