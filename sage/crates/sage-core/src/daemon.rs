use anyhow::Result;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time;
use tracing::{error, info, warn};
use tokio::sync::Mutex as TokioMutex;

use sage_types::{Event, EventType, WorkSchedule};

use crate::agent::Agent;
use crate::channel::InputChannel;
use crate::channels::calendar::CalendarChannel;
use crate::channels::email::EmailChannel;
use crate::channels::hooks::HooksChannel;
use crate::channels::wechat::WechatChannel;
use crate::config::Config;
use crate::discovery;
use crate::guardian;
use crate::heartbeat;
use crate::profile;
use crate::provider;
use crate::router::Router;
use crate::store::Store;

pub struct Daemon {
    config: Config,
    router: TokioMutex<Router>,
    store: Arc<Store>,
    schedule: Mutex<Option<WorkSchedule>>,
    /// 当前使用的 provider ID，用于检测 provider 变更
    current_provider_id: Mutex<Option<String>>,
    email: Option<EmailChannel>,
    calendar: Option<CalendarChannel>,
    hooks: Option<HooksChannel>,
    wechat: Option<WechatChannel>,
    heartbeat_interval: Duration,
    handled_keys: Mutex<HashSet<String>>,
}

impl Daemon {
    pub fn new(config: Config) -> Result<Self> {
        // 初始化 SQLite Store（统一存储，替代 memory.rs 的 Markdown 文件）
        let data_dir = Config::expand_path("~/.sage/data");
        std::fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("sage.db");
        let store = Arc::new(Store::open(&db_path)?);

        // 动态 provider 发现：优先使用已配置的 provider，回退到 config 中的 CLI
        let (agent, initial_provider_id) = {
            let providers = discovery::discover_providers(&store);
            let saved = store.load_provider_configs().unwrap_or_default();
            match discovery::select_best_provider(&providers, &saved) {
                Some((ref info, ref prov_config)) => {
                    let llm = provider::create_provider_from_config(info, prov_config, &config.agent);
                    info!("Daemon using discovered provider: {}", info.display_name);
                    (Agent::with_provider(llm), Some(info.id.clone()))
                }
                None => {
                    info!("No discovered provider, falling back to config CLI");
                    (Agent::new(config.agent.clone()), None)
                }
            }
        };

        let mut router = Router::new(agent, store.clone());

        // 加载 profile → 动态生成 SOP
        let schedule = match store.load_profile()? {
            Some(p) => {
                let sop = profile::generate_sop(&p);
                router.set_sop(sop);
                info!(
                    "Loaded profile for {}, SOP version {}",
                    p.identity.name, p.sop_version
                );
                Some(p.schedule)
            }
            None => {
                warn!("No profile found, using default SOP");
                None
            }
        };

        let email = config.channels.email.enabled.then_some(EmailChannel);
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

        // 从数据库恢复今天已处理的心跳动作，避免重启后重复触发
        let mut handled_keys = HashSet::new();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        handled_keys.insert(format!("__date:{today}"));
        if let Ok(titles) = store.get_today_handled_actions() {
            for title in &titles {
                let key = format!("heartbeat:{title}");
                info!("恢复已处理动作: {key}");
                handled_keys.insert(key);
            }
        }

        Ok(Self {
            config,
            router: TokioMutex::new(router),
            store,
            schedule: Mutex::new(schedule),
            current_provider_id: Mutex::new(initial_provider_id),
            email,
            calendar,
            hooks,
            wechat,
            heartbeat_interval,
            handled_keys: Mutex::new(handled_keys),
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Daemon event loop started");
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

    /// 直接触发指定类型的定时报告（绕过心跳时间窗口检查）
    pub async fn trigger_report(&self, report_type: &str) -> Result<String> {
        let title = match report_type {
            "morning" => "Morning Brief",
            "evening" => "Evening Review",
            "weekly" => "Weekly Report",
            "week_start" => "Week Start",
            _ => anyhow::bail!("未知报告类型: {report_type}"),
        };
        let event = Event {
            source: "heartbeat".into(),
            event_type: EventType::ScheduledTask,
            title: title.into(),
            body: String::new(),
            metadata: Default::default(),
            timestamp: chrono::Local::now(),
        };
        info!("手动触发报告: {title}");
        self.router.lock().await.route(event).await?;
        // 返回最新生成的报告内容
        if let Ok(Some(report)) = self.store.get_latest_report(report_type) {
            Ok(report.content)
        } else {
            Ok("报告已触发但未在 DB 中找到".into())
        }
    }

    async fn tick(&self) -> Result<()> {
        // 热重载 profile：schedule + SOP（Settings 修改后无需重启 daemon）
        if let Ok(Some(profile)) = self.store.load_profile() {
            let sop = profile::generate_sop(&profile);
            {
                let mut sched = self.schedule.lock().unwrap();
                *sched = Some(profile.schedule);
            }
            self.router.lock().await.set_sop(sop);
        }

        // 热重载 provider（Welcome/Settings 配置 API key 后无需重启 daemon）
        self.maybe_reload_provider().await;

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

        let actions = {
            let schedule_guard = self.schedule.lock().unwrap();
            heartbeat::evaluate(&self.config, schedule_guard.as_ref())
        };
        for action in actions {
            if let Some(event) = self.action_to_event(&action, &all_events) {
                all_events.push(event);
            }
        }

        let deduped = self.filter_new_events(all_events);
        info!("Tick: {} events to process", deduped.len());

        let mut had_evening_review = false;
        for event in deduped {
            if event.title == "Evening Review" {
                had_evening_review = true;
            }
            match self.router.lock().await.route(event.clone()).await {
                Ok(()) => self.mark_event_handled(&event),
                Err(e) => error!("Route failed (will retry next tick): {e}"),
            }
        }

        // 认知觉醒角色链：Evening Review 后依次触发
        // 顺序：Coach（更新 sage.md）→ Mirror（反映模式）→ Questioner（深度问题）
        if had_evening_review {
            let router = self.router.lock().await;

            match router.run_coach().await {
                Ok(true) => info!("Coach: learning completed"),
                Ok(false) => {}
                Err(e) => error!("Coach failed: {e}"),
            }

            match router.run_mirror().await {
                Ok(true) => info!("Mirror: reflection sent"),
                Ok(false) => {}
                Err(e) => error!("Mirror failed: {e}"),
            }

            match router.run_questioner().await {
                Ok(true) => info!("Questioner: daily question generated"),
                Ok(false) => {}
                Err(e) => error!("Questioner failed: {e}"),
            }
        }

        // Guardian：规则检测，无 Claude 调用（免费），每 tick 检查，自带每日去重
        match guardian::check(&self.store).await {
            Ok(true) => info!("Guardian: care alert sent"),
            Ok(false) => {}
            Err(e) => error!("Guardian failed: {e}"),
        }

        Ok(())
    }

    /// 检测 provider 变更并热更新 Agent
    async fn maybe_reload_provider(&self) {
        let providers = discovery::discover_providers(&self.store);
        let saved = self.store.load_provider_configs().unwrap_or_default();
        let best = discovery::select_best_provider(&providers, &saved);

        let new_id = best.as_ref().map(|(info, _)| info.id.clone());
        let current_id = self.current_provider_id.lock().unwrap().clone();

        if new_id == current_id {
            return;
        }

        let agent = match best {
            Some((ref info, ref prov_config)) => {
                let llm = provider::create_provider_from_config(info, prov_config, &self.config.agent);
                info!("Provider hot-reload: {:?} → {}", current_id, info.display_name);
                Agent::with_provider(llm)
            }
            None => {
                info!("Provider hot-reload: {:?} → CLI fallback", current_id);
                Agent::new(self.config.agent.clone())
            }
        };

        self.router.lock().await.set_agent(agent);
        *self.current_provider_id.lock().unwrap() = new_id;
    }

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
            heartbeat::Action::UrgentEmailCheck => {
                let emails: Vec<&Event> = events.iter()
                    .filter(|e| e.source == "email")
                    .collect();
                if emails.is_empty() {
                    return None;
                }
                let digest = emails.iter()
                    .take(5)
                    .map(|e| format!("- {} ({})", e.title, e.body))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(make(
                    "Email Check",
                    format!("当前{}封未读邮件：\n{}", emails.len(), digest),
                ))
            }
            heartbeat::Action::UpcomingMeetingCheck => {
                let meetings: Vec<&Event> = events.iter()
                    .filter(|e| e.source == "calendar")
                    .collect();
                if meetings.is_empty() {
                    return None;
                }
                let digest = meetings.iter()
                    .map(|e| format!("- {} ({})", e.title, e.body))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(make(
                    "Meeting Update",
                    format!("今日{}个会议/事件：\n{}", meetings.len(), digest),
                ))
            }
            heartbeat::Action::Idle => None,
        }
    }

    /// 过滤已处理事件（不插入，路由成功后再标记）
    fn filter_new_events(&self, events: Vec<Event>) -> Vec<Event> {
        let mut handled = self.handled_keys.lock().unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        let reset_key = format!("__date:{today}");
        if !handled.contains(&reset_key) {
            handled.clear();
            handled.insert(reset_key);
        }

        events
            .into_iter()
            .filter(|event| {
                if matches!(event.event_type, EventType::PatternObserved) {
                    return true;
                }
                let key = format!("{}:{}", event.source, event.title);
                !handled.contains(&key)
            })
            .collect()
    }

    /// 路由成功后标记事件为已处理
    fn mark_event_handled(&self, event: &Event) {
        if matches!(event.event_type, EventType::PatternObserved) {
            return;
        }
        let key = format!("{}:{}", event.source, event.title);
        self.handled_keys.lock().unwrap().insert(key);
    }
}
