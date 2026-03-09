use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time;
use tracing::{error, info, warn};

use crate::agent::Agent;
use crate::channel::{Event, EventType, InputChannel};
use crate::channels::calendar::CalendarChannel;
use crate::channels::email::EmailChannel;
use crate::channels::hooks::HooksChannel;
use crate::channels::wechat::WechatChannel;
use crate::config::Config;
use crate::heartbeat;
use crate::memory::Memory;
use crate::router::Router;

/// 每个事件最多重试次数（超过后当日不再尝试）
const MAX_RETRIES: u8 = 2;

/// 合并去重状态，用单一 Mutex 包装避免双锁死锁
struct EventState {
    handled: HashSet<String>,
    retries: HashMap<String, u8>,
    last_reset: String,
}

impl EventState {
    fn new() -> Self {
        Self {
            handled: HashSet::new(),
            retries: HashMap::new(),
            last_reset: String::new(),
        }
    }
}

pub struct Daemon {
    config: Config,
    router: Router,
    email: Option<EmailChannel>,
    calendar: Option<CalendarChannel>,
    hooks: Option<HooksChannel>,
    wechat: Option<WechatChannel>,
    heartbeat_interval: Duration,
    /// 合并的事件状态（已处理 key + 重试计数），单锁消除死锁风险
    event_state: Mutex<EventState>,
}

impl Daemon {
    pub fn new(config: Config) -> Result<Self> {
        let memory_dir = Config::expand_path(&config.memory.base_dir);
        let memory = Memory::new(memory_dir)?;
        let agent = Agent::new(config.agent.clone());
        let router = Router::new(agent, memory);

        let email = config
            .channels
            .email
            .enabled
            .then_some(EmailChannel);
        let calendar = config
            .channels
            .calendar
            .enabled
            .then_some(CalendarChannel);
        let hooks = config
            .channels
            .hooks
            .enabled
            .then(|| HooksChannel::new(Config::expand_path(&config.channels.hooks.watch_dir)));

        let wechat = config.channels.wechat.enabled.then(|| {
            WechatChannel::new(Config::expand_path(&config.channels.wechat.events_file))
        });

        let heartbeat_interval = Duration::from_secs(config.daemon.heartbeat_interval_secs);

        Ok(Self {
            config,
            router,
            email,
            calendar,
            hooks,
            wechat,
            heartbeat_interval,
            event_state: Mutex::new(EventState::new()),
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Daemon event loop started");

        // 启动通知
        if let Err(e) = crate::applescript::notify(
            "Sage Daemon 已启动",
            &format!(
                "配置: ~/.sage/config.toml | 心跳: {}s",
                self.heartbeat_interval.as_secs()
            ),
        )
        .await
        {
            error!("Startup notification failed: {e}");
        }

        let mut ticker = time::interval(self.heartbeat_interval);

        loop {
            ticker.tick().await;
            if let Err(e) = self.tick().await {
                error!("Tick failed: {e}");
            }
        }
    }

    pub async fn heartbeat_once(&self) -> Result<()> {
        self.tick().await
    }

    async fn tick(&self) -> Result<()> {
        // 1. 收集所有通道事件
        let mut all_events = Vec::new();

        if let Some(ref email) = self.email {
            match email.poll().await {
                Ok(events) => all_events.extend(events),
                Err(e) => error!("Email poll failed: {e}"),
            }
        }

        if let Some(ref calendar) = self.calendar {
            match calendar.poll().await {
                Ok(events) => all_events.extend(events),
                Err(e) => error!("Calendar poll failed: {e}"),
            }
        }

        if let Some(ref hooks) = self.hooks {
            match hooks.poll().await {
                Ok(events) => all_events.extend(events),
                Err(e) => error!("Hooks poll failed: {e}"),
            }
        }

        if let Some(ref wechat) = self.wechat {
            match wechat.poll().await {
                Ok(events) => all_events.extend(events),
                Err(e) => error!("Wechat poll failed: {e}"),
            }
        }

        // 2. 心跳调度 — 将时间感知任务转为事件
        let actions = heartbeat::evaluate(&self.config);
        for action in actions {
            if let Some(event) = self.action_to_event(&action, &all_events) {
                all_events.push(event);
            }
        }

        // 3. 去重：过滤已成功或超过重试上限的事件
        let deduped = self.dedup_events(all_events);
        info!("Tick: {} events to process", deduped.len());

        // 4. 路由事件（遇到网络错误时 early abort）
        for event in deduped {
            let key = format!("{}:{}", event.source, event.title);
            match self.router.route(event).await {
                Ok(()) => {
                    self.event_state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .handled
                        .insert(key);
                }
                Err(e) => {
                    let is_transient = is_transient_error(&e);
                    {
                        let mut state = self
                            .event_state
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        let count = state.retries.entry(key.clone()).or_insert(0);
                        *count += 1;
                        if *count >= MAX_RETRIES {
                            state.handled.insert(key.clone());
                            error!(
                                "Route failed, giving up after {MAX_RETRIES} retries: {key}: {e}"
                            );
                        } else {
                            warn!("Route failed ({count}/{MAX_RETRIES}), will retry: {key}: {e}");
                        }
                    }
                    // 网络类错误：跳过本 tick 剩余事件
                    if is_transient {
                        warn!("Transient error, skipping remaining events this tick");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// 将 heartbeat Action 转为可路由的 Event
    fn action_to_event(&self, action: &heartbeat::Action, events: &[Event]) -> Option<Event> {
        let now = chrono::Local::now();
        let make = |title: &str, body: String| Event {
            source: "heartbeat".into(),
            event_type: EventType::ScheduledTask,
            title: title.into(),
            body,
            metadata: Default::default(),
            timestamp: now,
        };

        match action {
            heartbeat::Action::MorningBrief => {
                let email_count = events.iter().filter(|e| e.source == "email").count();
                let meeting_count = events.iter().filter(|e| e.source == "calendar").count();
                Some(make(
                    "Morning Brief",
                    format!("{email_count} unread emails, {meeting_count} meetings today"),
                ))
            }
            heartbeat::Action::EveningReview => {
                Some(make("Evening Review", "总结今日工作，更新 memory".into()))
            }
            heartbeat::Action::WeeklyReport => {
                Some(make("Weekly Report", "生成本周工作周报草稿".into()))
            }
            heartbeat::Action::WeekStart => {
                Some(make("Week Start", "检查本周日程并提醒重点事项".into()))
            }
            heartbeat::Action::UrgentEmailCheck | heartbeat::Action::UpcomingMeetingCheck => {
                // 这些由 channel poll 天然覆盖，不需要额外事件
                None
            }
            heartbeat::Action::Idle => None,
        }
    }

    /// 对事件去重：过滤已成功处理或超过重试上限的事件
    fn dedup_events(&self, events: Vec<Event>) -> Vec<Event> {
        let mut state = self
            .event_state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        // 每日重置（在同一锁内操作，无需第二把锁）
        if state.last_reset != today {
            state.handled.clear();
            state.retries.clear();
            state.last_reset = today;
        }

        events
            .into_iter()
            .filter(|event| {
                // Background 事件已经在 hooks 层去重了，直接放行
                if matches!(event.event_type, EventType::PatternObserved) {
                    return true;
                }
                let key = format!("{}:{}", event.source, event.title);
                // 已成功处理 → 过滤
                if state.handled.contains(&key) {
                    return false;
                }
                // 超过重试上限 → 过滤
                if state.retries.get(&key).copied().unwrap_or(0) >= MAX_RETRIES {
                    return false;
                }
                true
            })
            .collect()
    }
}

/// 判断是否为网络/连接类瞬态错误（值得下次 tick 重试）
fn is_transient_error(e: &anyhow::Error) -> bool {
    let msg = e.to_string().to_lowercase();
    msg.contains("connection refused")
        || msg.contains("connectionrefused")
        || msg.contains("timed out")
        || msg.contains("unable to connect")
        || msg.contains("network")
        || msg.contains("cli timed out")
}
