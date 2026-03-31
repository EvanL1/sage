use anyhow::Result;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tokio::time;
use tracing::{error, info, warn};

use sage_types::{Event, EventType, ProjectStatus, WorkSchedule};

use crate::agent::Agent;
use crate::channel::InputChannel;
use crate::channels::calendar::CalendarChannel;
use crate::channels::email::EmailChannel;
use crate::channels::feed::{
    ArxivChannel, GitHubChannel, HackerNewsChannel, RedditChannel, RssChannel,
};
use crate::channels::hooks::HooksChannel;
use crate::channels::wechat::WechatChannel;
use crate::config::Config;
use crate::discovery;
use crate::guardian;
use crate::heartbeat;
use crate::pipeline::HarnessedAgent;
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
    feed_channels: Vec<Box<dyn InputChannel>>,
    heartbeat_interval: Duration,
    handled_keys: Mutex<HashSet<String>>,
    /// Tick 计数器，用于控制每 3 tick 触发一次 task_intelligence
    tick_count: Mutex<u32>,
}

impl Daemon {
    pub fn new(config: Config) -> Result<Self> {
        let data_dir = Config::expand_path("~/.sage/data");
        std::fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("sage.db");
        let store = Arc::new(Store::open(&db_path)?);
        Self::with_store(config, store)
    }

    /// 使用外部提供的 Store 实例构建 Daemon（Desktop 内嵌时共享同一 Store）
    pub fn with_store(config: Config, store: Arc<Store>) -> Result<Self> {
        // 动态 provider 发现：优先使用已配置的 provider，回退到 config 中的 CLI
        let (agent, initial_provider_id) = {
            let providers = discovery::discover_providers(&store);
            let saved = store.load_provider_configs().unwrap_or_default();
            match discovery::select_best_provider(&providers, &saved) {
                Some((ref info, ref prov_config)) => {
                    let llm =
                        provider::create_provider_from_config(info, prov_config, &config.agent);
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
        router.set_calendar_source(config.channels.calendar.source.clone());

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

        let email = config.channels.email.enabled.then(|| {
            let sources = store.get_message_sources_by_type("imap").unwrap_or_default();
            EmailChannel::new(sources)
        });
        let calendar = config.channels.calendar.enabled.then_some(CalendarChannel);
        let hooks = config
            .channels
            .hooks
            .enabled
            .then(|| HooksChannel::new(Config::expand_path(&config.channels.hooks.watch_dir)));

        let wechat =
            config.channels.wechat.enabled.then(|| {
                WechatChannel::new(Config::expand_path(&config.channels.wechat.events_file))
            });

        let heartbeat_interval = Duration::from_secs(config.daemon.heartbeat_interval_secs);

        // 构建 Feed 通道（全部默认 disabled，不影响现有用户）
        let feed_agent = Arc::new({
            let providers = discovery::discover_providers(&store);
            let saved = store.load_provider_configs().unwrap_or_default();
            match discovery::select_best_provider(&providers, &saved) {
                Some((ref info, ref prov_config)) => {
                    let llm =
                        provider::create_provider_from_config(info, prov_config, &config.agent);
                    Agent::with_provider(llm)
                }
                None => Agent::new(config.agent.clone()),
            }
        });
        let mut feed_channels: Vec<Box<dyn InputChannel>> = Vec::new();
        let feed_cfg = &config.channels.feed;
        let interests = feed_cfg.user_interests.join(", ");
        // Personality summary: top identity/personality memories joined, or empty
        let personality = store
            .search_memories("identity personality traits", 5)
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.content)
            .collect::<Vec<_>>()
            .join("; ");
        let project_focus = store
            .load_profile()
            .ok()
            .flatten()
            .map(|profile| {
                let mut parts = Vec::new();
                let active_projects = profile
                    .work_context
                    .projects
                    .iter()
                    .filter(|project| {
                        matches!(
                            project.status,
                            ProjectStatus::Active | ProjectStatus::Planning
                        )
                    })
                    .take(3)
                    .map(|project| format!("{}: {}", project.name, project.description))
                    .collect::<Vec<_>>();
                if !active_projects.is_empty() {
                    parts.push(match store.prompt_lang().as_str() {
                        "en" => format!("Current projects: {}", active_projects.join("; ")),
                        _ => format!("当前项目：{}", active_projects.join("；")),
                    });
                }
                if !profile.work_context.tech_stack.is_empty() {
                    let tech_stack = profile
                        .work_context
                        .tech_stack
                        .iter()
                        .take(8)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    parts.push(match store.prompt_lang().as_str() {
                        "en" => format!("Tech stack: {tech_stack}"),
                        _ => format!("技术栈：{tech_stack}"),
                    });
                }
                parts.join("\n")
            })
            .unwrap_or_default();
        if feed_cfg.reddit.enabled {
            feed_channels.push(Box::new(RedditChannel::new(
                feed_cfg.reddit.clone(),
                Arc::clone(&store),
                interests.clone(),
                personality.clone(),
                project_focus.clone(),
                feed_agent.clone(),
            )));
        }
        if feed_cfg.github.enabled {
            feed_channels.push(Box::new(GitHubChannel::new(
                feed_cfg.github.clone(),
                Arc::clone(&store),
                interests.clone(),
                personality.clone(),
                project_focus.clone(),
                feed_agent.clone(),
            )));
        }
        if feed_cfg.hackernews.enabled {
            feed_channels.push(Box::new(HackerNewsChannel::new(
                feed_cfg.hackernews.clone(),
                Arc::clone(&store),
                interests.clone(),
                personality.clone(),
                project_focus.clone(),
                feed_agent.clone(),
            )));
        }
        if feed_cfg.arxiv.enabled {
            feed_channels.push(Box::new(ArxivChannel::new(
                feed_cfg.arxiv.clone(),
                Arc::clone(&store),
                interests.clone(),
                personality.clone(),
                project_focus.clone(),
                feed_agent.clone(),
            )));
        }
        if feed_cfg.rss.enabled {
            feed_channels.push(Box::new(RssChannel::new(
                feed_cfg.rss.clone(),
                Arc::clone(&store),
                interests.clone(),
                personality.clone(),
                project_focus.clone(),
                feed_agent.clone(),
            )));
        }

        // 从数据库恢复今天已处理的心跳动作，避免重启后重复触发
        let mut handled_keys = HashSet::new();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        handled_keys.insert(format!("__date:{today}"));
        if let Ok(titles) = store.get_today_handled_actions() {
            for title in &titles {
                // key 格式与 filter_new_events 一致: "{source}:{title}:{date}"
                // heartbeat 事件没有 metadata.date，所以 date 为空 → 尾部有冒号
                let key = format!("heartbeat:{title}:");
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
            feed_channels,
            heartbeat_interval,
            handled_keys: Mutex::new(handled_keys),
            tick_count: Mutex::new(0),
        })
    }

    pub fn heartbeat_interval_secs(&self) -> u64 {
        self.heartbeat_interval.as_secs()
    }

    pub async fn run(&self) -> Result<()> {
        info!("Daemon event loop started");

        // 启动 Browser Bridge HTTP 服务器
        let bridge_store = self.store.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::bridge::start_bridge_server(bridge_store, crate::bridge::DEFAULT_PORT).await
            {
                error!("Bridge server failed: {e}");
            }
        });

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

    /// 手动触发记忆进化（通过 pipeline 逐 stage 执行，实时汇报进度）
    pub async fn trigger_memory_evolution(
        &self,
    ) -> Result<crate::pipeline::EvolutionResult> {
        info!("手动触发记忆进化");
        let stage_labels = [
            ("evolution_merge", "去重合并"),
            ("evolution_synth", "特质提炼"),
            ("evolution_condense", "精简冗长"),
            ("evolution_link", "记忆关联"),
            ("evolution_decay", "衰减过期"),
            ("evolution_promote", "晋升验证"),
        ];
        // 短暂 lock 拿到 agent clone + store，立即释放（避免和 daemon tick 死锁）
        let (agent, store) = {
            let router = self.router.lock().await;
            (router.agent().clone(), router.store_arc())
        };
        let pipeline = crate::pipeline::build_pipeline(&self.config.pipeline, &self.store);
        let total = stage_labels.len();
        let mut summaries = Vec::new();
        for (i, (name, label)) in stage_labels.iter().enumerate() {
            let _ = self.store.kv_set("evolution_progress",
                &format!("{}/{} — [{}] {}", i + 1, total, name, label));
            let ctx = pipeline.run(
                &format!("manual_{name}"), &[name.to_string()], &agent, &store,
            ).await;
            summaries.push(format!("{label}: {}", ctx.summary()));
        }
        let _ = self.store.kv_delete("evolution_progress");
        let summary = summaries.join(" | ");
        info!("手动记忆进化完成: {summary}");
        Ok(crate::pipeline::EvolutionResult { summary, ..Default::default() })
    }

    /// 手动触发人物观察（通过 pipeline 执行 person_observer preset）
    pub async fn trigger_person_observer(&self) -> Result<bool> {
        let (agent, store) = {
            let r = self.router.lock().await;
            (r.agent().clone(), r.store_arc())
        };
        let pipeline = crate::pipeline::build_pipeline(&self.config.pipeline, &self.store);
        let ctx = pipeline.run("manual_person_observer", &["person_observer".into()], &agent, &store).await;
        Ok(!ctx.summary().contains("empty"))
    }

    /// 手动触发战略分析（通过 pipeline 执行 strategist preset）
    pub async fn trigger_strategist(&self) -> Result<bool> {
        let (agent, store) = {
            let r = self.router.lock().await;
            (r.agent().clone(), r.store_arc())
        };
        let pipeline = crate::pipeline::build_pipeline(&self.config.pipeline, &self.store);
        let ctx = pipeline.run("manual_strategist", &["strategist".into()], &agent, &store).await;
        Ok(!ctx.summary().contains("empty"))
    }

    /// 手动触发记忆连接（通过 pipeline 执行 evolution_link preset）
    pub async fn trigger_memory_linking(&self) -> Result<usize> {
        let (agent, store) = {
            let r = self.router.lock().await;
            (r.agent().clone(), r.store_arc())
        };
        let pipeline = crate::pipeline::build_pipeline(&self.config.pipeline, &self.store);
        let ctx = pipeline.run("manual_linking", &["evolution_link".into()], &agent, &store).await;
        Ok(ctx.stage_results.len())
    }

    /// 认知调和（增量）：检查新内容是否推翻了旧 decisions/insights
    pub async fn run_reconcile(&self, new_content: &str) -> Result<usize> {
        let router = self.router.lock().await;
        let invoker = HarnessedAgent::new(router.agent().clone(), router.store_arc(), "reconcile");
        crate::reconciler::reconcile(&invoker, router.store(), new_content).await
    }

    /// 认知调和（全量）：扫描所有记忆，找出内部矛盾和过时结论
    pub async fn run_reconcile_full(&self) -> Result<usize> {
        info!("手动触发全量认知调和");
        let router = self.router.lock().await;
        let invoker = HarnessedAgent::new(router.agent().clone(), router.store_arc(), "reconcile_full");
        crate::reconciler::reconcile_full(&invoker, router.store()).await
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
                let mut sched = self.schedule.lock().unwrap_or_else(|e| e.into_inner());
                *sched = Some(profile.schedule);
            }
            self.router.lock().await.set_sop(sop);
        }

        // 热重载 provider（Welcome/Settings 配置 API key 后无需重启 daemon）
        self.maybe_reload_provider().await;

        let mut all_events = Vec::new();

        if let Some(ref email) = self.email {
            match email.poll().await {
                Ok(events) => {
                    // 邮件事件写入 messages 表，供信息流页面浏览
                    // 使用邮件原始 DATE 字段作为时间戳（稳定，用于去重）
                    for ev in &events {
                        let direction = ev
                            .metadata
                            .get("direction")
                            .map(|s| s.as_str())
                            .unwrap_or("received");
                        let raw_sender = ev
                            .metadata
                            .get("from")
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        let sender = if raw_sender.is_empty() && direction == "sent" {
                            self.store.load_profile()
                                .ok()
                                .flatten()
                                .map(|p| p.identity.name)
                                .unwrap_or_else(|| "Me".to_string())
                        } else if raw_sender.is_empty() {
                            "Unknown".to_string()
                        } else {
                            raw_sender.to_string()
                        };
                        let sender = sender.as_str();
                        let subject = &ev.title;
                        // 邮件正文：去签名档/引用链，减少无效上下文
                        let cleaned_body = if ev.body.is_empty() {
                            None
                        } else {
                            let stripped = crate::channels::email_filter::strip_signature_and_quotes(&ev.body);
                            if stripped.is_empty() { None } else { Some(stripped) }
                        };
                        let ts = ev
                            .metadata
                            .get("date")
                            .filter(|d| !d.is_empty())
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| ev.timestamp.to_rfc3339());
                        // 邮件分类：noise → info_only
                        let is_noise = crate::channels::email_filter::classify(
                            sender, subject, cleaned_body.as_deref().unwrap_or(""),
                        ) == "noise";
                        let msg_id = self.store.save_message_with_direction(
                            sender, subject, cleaned_body.as_deref(), "email", "email", &ts, direction,
                        );
                        match msg_id {
                            Ok(id) if is_noise && id > 0 => {
                                let _ = self.store.update_message_action_state(id, "info_only");
                            }
                            Err(e) => error!("保存邮件到 messages 失败: {e}"),
                            _ => {}
                        }
                    }
                    all_events.extend(events);
                }
                Err(e) => error!("Email poll failed: {e}"),
            }
        }

        // Outlook AppleScript 邮件轮询（独立于 IMAP channel）
        if let Ok(outlook_sources) = self.store.get_message_sources_by_type("outlook") {
            for src in &outlook_sources {
                match crate::channels::outlook::fetch_outlook_emails(src.id, 30).await {
                    Ok(emails) => {
                        let _ = self.store.save_emails(&emails);
                        for email in &emails {
                            let importance = crate::channels::email_filter::classify(
                                &email.from_addr, &email.subject, &email.body_text,
                            );
                            if importance == "noise" { continue; }
                            let direction = if email.folder == "Sent" { "sent" } else { "received" };
                            let sender = if direction == "sent" { "我" } else { email.from_addr.as_str() };
                            let _ = self.store.save_message_with_direction(
                                sender, &email.subject, Some(&email.body_text),
                                "email", "email", &email.date, direction,
                            );
                        }
                        if !emails.is_empty() {
                            info!("Outlook poll: {} emails from source {}", emails.len(), src.id);
                        }
                    }
                    Err(e) => error!("Outlook poll failed (source {}): {e}", src.id),
                }
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

        self.poll_feed_channels().await;

        let mut actions = {
            let schedule_guard = self.schedule.lock().unwrap_or_else(|e| e.into_inner());
            heartbeat::evaluate(&self.config, schedule_guard.as_ref())
        };
        self.catchup_missed_reports(&mut actions);
        for action in actions {
            if let Some(event) = self.action_to_event(&action, &all_events) {
                all_events.push(event);
            }
        }

        let deduped = self.filter_new_events(all_events);
        info!("Tick: {} events to process", deduped.len());

        let mut had_morning_brief = false;
        let mut had_evening_review = false;
        let mut had_weekly_report = false;
        for event in deduped {
            if event.title == "Morning Brief" {
                had_morning_brief = true;
            }
            if event.title == "Evening Review" {
                had_evening_review = true;
            }
            if event.title == "Weekly Report" {
                had_weekly_report = true;
            }
            match self.router.lock().await.route(event.clone()).await {
                Ok(()) => self.mark_event_handled(&event),
                Err(e) => error!("Route failed (will retry next tick): {e}"),
            }
        }

        // 晨间任务规划：Morning Brief 后自动提取今日待办
        if had_morning_brief {
            match self.router.lock().await.run_task_planner().await {
                Ok(n) => {
                    if n > 0 {
                        info!("Task planner: {n} tasks created from morning brief");
                    }
                }
                Err(e) => error!("Task planner failed: {e}"),
            }
        }

        // 晚间任务清理：auto-stale 超过 3 天逾期的 open tasks
        if had_evening_review {
            let cutoff = (chrono::Local::now() - chrono::Duration::days(3))
                .format("%Y-%m-%d")
                .to_string();
            if let Ok(overdue) = self.store.list_tasks(Some("open"), 100) {
                for (id, _, _, _, due, _, _, _, _, _, _) in &overdue {
                    if let Some(d) = due {
                        if d.as_str() < cutoff.as_str() {
                            let _ = self.store.update_task_status(*id, "stale");
                        }
                    }
                }
            }
        }

        // 认知管线：config 驱动的 stage 执行（替代硬编码链）
        if had_evening_review || had_weekly_report {
            let router = self.router.lock().await;
            let pipeline = crate::pipeline::build_pipeline(
                &self.config.pipeline,
                &self.store,
            );
            let agent = router.agent();
            let store = router.store_arc();
            if had_evening_review {
                let ctx = pipeline.run_evening(agent, &store).await;
                info!("Evening {}", ctx.summary());
            }
            if had_weekly_report {
                let ctx = pipeline.run_weekly(agent, &store).await;
                info!("Weekly {}", ctx.summary());
            }
        }

        // 记忆清理：标记过期 working 记忆
        if let Ok(n) = self.store.expire_stale_memories() {
            if n > 0 {
                info!("Memory: {n} stale memories expired");
            }
        }

        // Guardian：规则检测，无 Claude 调用（免费），每 tick 检查，自带每日去重
        match guardian::check(&self.store).await {
            Ok(true) => info!("Guardian: care alert sent"),
            Ok(false) => {}
            Err(e) => error!("Guardian failed: {e}"),
        }

        // 周期性任务：每 3 tick 执行
        let run_task_intel = {
            let mut count = self.tick_count.lock().unwrap_or_else(|e| e.into_inner());
            *count = count.wrapping_add(1);
            *count % 3 == 0
        };

        // Task Intelligence + Staleness Check：每 3 tick
        if run_task_intel {
            let router = self.router.lock().await;
            match crate::task_intelligence::detect_task_signals(router.agent(), router.store())
                .await
            {
                Ok(n) if n > 0 => info!("Task intelligence: {n} new signals"),
                Ok(_) => {}
                Err(e) => error!("Task intelligence failed: {e}"),
            }

            let staleness_invoker = HarnessedAgent::new(router.agent().clone(), router.store_arc(), "staleness");
            match crate::staleness::check_staleness(&staleness_invoker, &router.store_arc())
                .await
            {
                Ok(r) if r.resolved + r.expired > 0 => {
                    info!(
                        "Staleness: {} resolved, {} expired out of {} checked",
                        r.resolved, r.expired, r.checked
                    );
                }
                Ok(_) => {}
                Err(e) => error!("Staleness check failed: {e}"),
            }
        }

        Ok(())
    }

    /// 手动触发 Feed 抓取（完成后自动生成每日简报缓存）
    pub async fn trigger_feed_poll(&self) {
        self.poll_feed_channels().await;
        self.generate_and_cache_digest().await;
    }

    /// 生成 Feed 每日简报并写入缓存
    async fn generate_and_cache_digest(&self) {
        let lang = self.store.prompt_lang();
        let items = match self.store.load_feed_observations(30) {
            Ok(items) => items,
            Err(e) => {
                error!("加载 feed observations 失败: {e}");
                return;
            }
        };
        if items.is_empty() {
            return;
        }

        // 构建 digest 输入（排除已归档条目）
        let archived_ids = self.store.get_archived_feed_ids().unwrap_or_default();
        let mut lines = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for row in &items {
            if archived_ids.contains(&row.id) { continue; }
            let obs = &row.observation;
            let title = if let Some(idx) = obs.rfind('|') {
                obs[..idx].trim()
            } else {
                obs.as_str()
            };
            if !seen.insert(title.to_string()) {
                continue;
            }
            let (score, insight) = row
                .raw_data
                .as_deref()
                .map(|s| {
                    // raw_data format: "url\nscore\ninsight\n..."
                    let mut parts = s.splitn(3, '\n');
                    let _url = parts.next().unwrap_or("");
                    let sc = parts.next().unwrap_or("3").trim().parse::<u8>().unwrap_or(3);
                    let ins = parts.next().unwrap_or("").trim().to_string();
                    (sc, ins)
                })
                .unwrap_or((3, String::new()));
            if score >= 3 {
                lines.push(format!("{score} | {title} | {insight}"));
            }
        }
        if lines.is_empty() {
            return;
        }

        let router = self.router.lock().await;
        let agent = router.agent();
        agent.reset_counter();
        let system = crate::prompts::feed_digest_system(&lang);
        let user = crate::prompts::feed_digest_user(&lang, &lines.join("\n"));
        match crate::pipeline::harness::invoke_raw(agent, &user, Some(system)).await {
            Ok(digest) => {
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                if let Err(e) = self.store.save_feed_digest(&today, &digest) {
                    error!("缓存 feed digest 失败: {e}");
                }
                info!("Feed digest 已生成并缓存（{today}）");
            }
            Err(e) => error!("生成 feed digest 失败: {e}"),
        }
    }

    /// 轮询所有 Feed 通道，将结果写入 observations 表（同标题去重）
    async fn poll_feed_channels(&self) {
        for ch in &self.feed_channels {
            match ch.poll().await {
                Ok(events) => {
                    for ev in events {
                        if !self.store.has_feed_observation(&ev.title) {
                            let _ = self
                                .store
                                .record_observation("feed", &ev.title, Some(&ev.body));
                        }
                    }
                }
                Err(e) => error!("Feed {} failed: {e}", ch.name()),
            }
        }
    }

    /// 检测 provider 变更并热更新 Agent
    async fn maybe_reload_provider(&self) {
        let providers = discovery::discover_providers(&self.store);
        let saved = self.store.load_provider_configs().unwrap_or_default();
        let best = discovery::select_best_provider(&providers, &saved);

        let new_id = best.as_ref().map(|(info, _)| info.id.clone());
        let current_id = self
            .current_provider_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        if new_id == current_id {
            return;
        }

        let agent = match best {
            Some((ref info, ref prov_config)) => {
                let llm =
                    provider::create_provider_from_config(info, prov_config, &self.config.agent);
                info!(
                    "Provider hot-reload: {:?} → {}",
                    current_id, info.display_name
                );
                Agent::with_provider(llm)
            }
            None => {
                info!("Provider hot-reload: {:?} → CLI fallback", current_id);
                Agent::new(self.config.agent.clone())
            }
        };

        self.router.lock().await.set_agent(agent);
        *self
            .current_provider_id
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = new_id;
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
                let emails: Vec<&Event> = events.iter().filter(|e| e.source == "email").collect();
                if emails.is_empty() {
                    return None;
                }
                let digest = emails
                    .iter()
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
                let meetings: Vec<&Event> =
                    events.iter().filter(|e| e.source == "calendar").collect();
                if meetings.is_empty() {
                    return None;
                }
                let digest = meetings
                    .iter()
                    .map(|e| format!("- {} ({})", e.title, e.body))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(make(
                    "Meeting Update",
                    format!("今日{}个会议/事件：\n{}", meetings.len(), digest),
                ))
            }
            heartbeat::Action::MirrorWeekly => None, // 不生成事件，在 had_weekly_report 块中直接触发
            heartbeat::Action::Idle => None,
        }
    }

    /// 过滤已处理事件（不插入，路由成功后再标记）
    fn filter_new_events(&self, events: Vec<Event>) -> Vec<Event> {
        let mut handled = self.handled_keys.lock().unwrap_or_else(|e| e.into_inner());
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
                let date = event.metadata.get("date").map(|s| s.as_str()).unwrap_or("");
                let key = format!("{}:{}:{}", event.source, event.title, date);
                if handled.contains(&key) {
                    return false;
                }
                // 同一批次内去重：同一 tick 中多个日历返回同一会议时只保留第一个
                handled.insert(key);
                true
            })
            .collect()
    }

    /// 路由成功后标记事件为已处理
    fn mark_event_handled(&self, event: &Event) {
        if matches!(event.event_type, EventType::PatternObserved) {
            return;
        }
        let date = event.metadata.get("date").map(|s| s.as_str()).unwrap_or("");
        let key = format!("{}:{}:{}", event.source, event.title, date);
        self.handled_keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key);
    }

    /// 补偿检查：今天应生成但错过的报告，一次 tick 最多补一个
    fn catchup_missed_reports(&self, actions: &mut Vec<heartbeat::Action>) {
        use chrono::{Datelike as _, Timelike as _};
        let now = chrono::Local::now();
        let hour = now.hour();
        let today = now.format("%Y-%m-%d").to_string();

        let (brief_hour, review_hour) = {
            let sched = self.schedule.lock().unwrap_or_else(|e| e.into_inner());
            (
                sched.as_ref().map(|s| s.morning_brief_hour).unwrap_or(8),
                sched.as_ref().map(|s| s.evening_review_hour).unwrap_or(18),
            )
        };

        let handled = self.handled_keys.lock().unwrap_or_else(|e| e.into_inner());

        // Morning Brief：错过了 brief_hour，还没到中午，且今天没生成过
        if hour > brief_hour
            && hour < 12
            && !handled.contains("heartbeat:Morning Brief:")
            && !self.has_report_today("morning", &today)
        {
            drop(handled);
            info!("Catchup: missed Morning Brief (brief_hour={brief_hour}), generating now at {hour}:xx");
            actions.push(heartbeat::Action::MorningBrief);
            return;
        }

        // Evening Review：错过了 review_hour，还没到午夜，且今天没生成过
        if hour > review_hour
            && hour < 23
            && !handled.contains("heartbeat:Evening Review:")
            && !self.has_report_today("evening", &today)
        {
            drop(handled);
            info!("Catchup: missed Evening Review (review_hour={review_hour}), generating now at {hour}:xx");
            actions.push(heartbeat::Action::EveningReview);
            return;
        }

        // WeekStart：周一，错过了 brief_hour，还没到中午，且今天没生成过
        if now.weekday() == chrono::Weekday::Mon
            && hour > brief_hour
            && hour < 12
            && !handled.contains("heartbeat:Week Start:")
            && !self.has_report_today("week_start", &today)
        {
            drop(handled);
            info!("Catchup: missed Week Start, generating now at {hour}:xx");
            actions.push(heartbeat::Action::WeekStart);
        }
    }

    /// 检查今天是否已经生成过指定类型的报告
    fn has_report_today(&self, report_type: &str, today: &str) -> bool {
        self.store
            .get_latest_report(report_type)
            .ok()
            .flatten()
            .map(|r| r.created_at.starts_with(today))
            .unwrap_or(false)
    }
}
