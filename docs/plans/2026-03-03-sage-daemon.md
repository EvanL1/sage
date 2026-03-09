# Sage Daemon Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build an immortal, event-driven, self-evolving personal AI daemon in Rust that auto-starts on boot, monitors email/calendar/WeChat, learns from Claude Code usage patterns, and proactively serves Evan.

**Architecture:** Rust async daemon (tokio) inspired by OpenClaw's Gateway pattern — a lightweight event loop with heartbeat scheduling, channel adapters for I/O, and on-demand Claude CLI invocation for AI reasoning. Local-first memory stored as Markdown files on disk.

**Tech Stack:** Rust (tokio, serde, toml, notify, tokio-tungstenite), AppleScript via `osascript`, Claude CLI (`claude --print`), Wechaty (sidecar for WeChat)

---

## Architecture Overview

```
┌──────────────────────────────────────────────────────┐
│                macOS LaunchAgent                      │
│                (com.sage.daemon)                      │
│                       │                               │
│              ┌────────▼────────┐                      │
│              │   Sage Daemon   │                      │
│              │   (Rust binary) │                      │
│              └────────┬────────┘                      │
│                       │                               │
│         ┌─────────────┼─────────────┐                 │
│         │             │             │                 │
│   ┌─────▼─────┐ ┌─────▼─────┐ ┌────▼─────┐          │
│   │ Heartbeat │ │  Watcher  │ │ Channel  │          │
│   │ (30 min)  │ │ (fswatch) │ │ Listener │          │
│   └─────┬─────┘ └─────┬─────┘ └────┬─────┘          │
│         └─────────────┼─────────────┘                 │
│                       │                               │
│              ┌────────▼────────┐                      │
│              │  Event Router   │                      │
│              │  (rule engine)  │                      │
│              └────────┬────────┘                      │
│                       │                               │
│         ┌─────────────┼─────────────┐                 │
│         │             │             │                 │
│   ┌─────▼─────┐ ┌─────▼─────┐ ┌────▼─────┐          │
│   │  Local    │ │  Claude   │ │  Memory  │          │
│   │  Action   │ │  --print  │ │  Update  │          │
│   │ (notify)  │ │ (AI call) │ │ (learn)  │          │
│   └───────────┘ └───────────┘ └──────────┘          │
│                                                       │
│   Channels:                                           │
│   ┌────────┐ ┌──────────┐ ┌────────┐ ┌───────────┐  │
│   │ Email  │ │ Calendar │ │ WeChat │ │ CC Hooks  │  │
│   │(Apple  │ │(Apple    │ │(sidecar│ │(file      │  │
│   │ Script)│ │ Script)  │ │ bridge)│ │ watcher)  │  │
│   └────────┘ └──────────┘ └────────┘ └───────────┘  │
└──────────────────────────────────────────────────────┘
```

## Project Structure

```
sage-daemon/
├── Cargo.toml
├── config.toml                  # 默认配置
├── src/
│   ├── main.rs                  # CLI 入口 + daemon 启动
│   ├── config.rs                # TOML 配置加载
│   ├── daemon.rs                # Daemon 生命周期管理
│   ├── heartbeat.rs             # 心跳调度器
│   ├── router.rs                # 事件路由 + 规则引擎
│   ├── agent.rs                 # Claude CLI 调用封装
│   ├── memory.rs                # Markdown 记忆系统
│   ├── channel.rs               # Channel trait 定义
│   ├── channels/
│   │   ├── email.rs             # Outlook 邮件适配器
│   │   ├── calendar.rs          # Outlook 日历适配器
│   │   ├── wechat.rs            # WeChat 桥接适配器
│   │   ├── hooks.rs             # Claude Code hooks 监听
│   │   └── notification.rs      # macOS 通知输出
│   └── applescript.rs           # AppleScript 执行器
├── heartbeat/
│   └── HEARTBEAT.md             # 心跳任务清单
├── memory/
│   ├── MEMORY.md                # 核心记忆
│   ├── patterns.md              # 行为模式
│   └── decisions.md             # 决策记录
├── launchd/
│   └── com.sage.daemon.plist    # macOS LaunchAgent
├── sidecar/
│   └── wechat-bridge/           # WeChat Bun sidecar
│       ├── package.json
│       └── index.ts
└── tests/
    ├── config_test.rs
    ├── heartbeat_test.rs
    ├── agent_test.rs
    └── memory_test.rs
```

---

## Task 1: Rust 项目脚手架 + 配置系统

**Files:**
- Create: `sage-daemon/Cargo.toml`
- Create: `sage-daemon/src/main.rs`
- Create: `sage-daemon/src/config.rs`
- Create: `sage-daemon/config.toml`

**Step 1: 初始化 Cargo 项目**

```bash
cd /Users/lyf/dev/digital-twin
cargo init sage-daemon
```

**Step 2: 添加依赖到 Cargo.toml**

```toml
[package]
name = "sage-daemon"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
notify = "7"
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
clap = { version = "4", features = ["derive"] }
```

**Step 3: 创建配置文件 config.toml**

```toml
[daemon]
heartbeat_interval_secs = 1800  # 30 minutes
log_level = "info"
pid_file = "/tmp/sage-daemon.pid"

[memory]
base_dir = "~/.sage/memory"
heartbeat_file = "~/.sage/HEARTBEAT.md"

[agent]
claude_binary = "claude"
default_model = "sonnet"
project_dir = "/Users/lyf/dev/digital-twin"
max_budget_usd = 0.50
permission_mode = "bypassPermissions"

[channels.email]
enabled = true
poll_interval_secs = 300  # 5 minutes

[channels.calendar]
enabled = true
poll_interval_secs = 900  # 15 minutes

[channels.wechat]
enabled = false  # v2
bridge_socket = "/tmp/sage-wechat.sock"

[channels.hooks]
enabled = true
watch_dir = "~/.claude"

[channels.notification]
enabled = true
```

**Step 4: 实现 config.rs**

```rust
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub memory: MemoryConfig,
    pub agent: AgentConfig,
    pub channels: ChannelsConfig,
}

#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    pub heartbeat_interval_secs: u64,
    pub log_level: String,
    pub pid_file: String,
}

#[derive(Debug, Deserialize)]
pub struct MemoryConfig {
    pub base_dir: String,
    pub heartbeat_file: String,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    pub claude_binary: String,
    pub default_model: String,
    pub project_dir: String,
    pub max_budget_usd: f64,
    pub permission_mode: String,
}

#[derive(Debug, Deserialize)]
pub struct ChannelsConfig {
    pub email: ChannelToggle,
    pub calendar: ChannelToggle,
    pub wechat: WechatConfig,
    pub hooks: HooksConfig,
    pub notification: ChannelToggle,
}

#[derive(Debug, Deserialize)]
pub struct ChannelToggle {
    pub enabled: bool,
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct WechatConfig {
    pub enabled: bool,
    pub bridge_socket: String,
}

#[derive(Debug, Deserialize)]
pub struct HooksConfig {
    pub enabled: bool,
    pub watch_dir: String,
}

fn default_poll() -> u64 { 300 }

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn expand_path(path: &str) -> PathBuf {
        if path.starts_with('~') {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(path.replacen('~', &home, 1))
        } else {
            PathBuf::from(path)
        }
    }
}
```

**Step 5: 实现 main.rs 骨架**

```rust
mod config;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser)]
#[command(name = "sage", about = "Sage Daemon — Evan's immortal AI counselor")]
struct Cli {
    /// Config file path
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Run in foreground (don't daemonize)
    #[arg(long)]
    foreground: bool,

    /// Run heartbeat once and exit
    #[arg(long)]
    heartbeat_once: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("sage_daemon=info")
        .init();

    let config = config::Config::load(&cli.config)?;
    info!("Sage Daemon starting with config: {:?}", config.daemon);

    if cli.heartbeat_once {
        info!("Running single heartbeat...");
        // TODO: heartbeat::run_once(&config).await?;
        return Ok(());
    }

    info!("Sage Daemon running. Heartbeat every {}s",
        config.daemon.heartbeat_interval_secs);

    // TODO: Start event loop
    // daemon::run(config).await?;

    Ok(())
}
```

**Step 6: 验证编译**

```bash
cd /Users/lyf/dev/digital-twin/sage-daemon && cargo check
```

**Step 7: Commit**

```bash
git add sage-daemon/
git commit -m "feat: sage-daemon scaffold with config system"
```

---

## Task 2: Daemon 生命周期 + 心跳循环

**Files:**
- Create: `sage-daemon/src/daemon.rs`
- Create: `sage-daemon/src/heartbeat.rs`
- Create: `sage-daemon/heartbeat/HEARTBEAT.md`
- Modify: `sage-daemon/src/main.rs`

**Step 1: 创建 HEARTBEAT.md**

```markdown
# Sage Heartbeat Checklist

每次心跳时 Sage 检查以下项目，决定是否需要行动：

## 每日任务
- [ ] 早间 brief：扫描未读邮件 + 今日日程，生成摘要
- [ ] 晚间回顾：总结今日完成的工作，更新 memory

## 持续监控
- [ ] 检查是否有需要回复的紧急邮件
- [ ] 检查未来 1 小时内是否有会议
- [ ] 检查是否有新的 Git 活动需要关注

## 每周任务
- [ ] 周五：生成周报草稿
- [ ] 周一：检查本周日程并提醒重点事项
```

**Step 2: 实现 heartbeat.rs**

```rust
use anyhow::Result;
use chrono::Local;
use std::path::Path;
use tracing::{info, warn};

use crate::config::Config;

pub struct Heartbeat {
    config: Config,
}

#[derive(Debug)]
pub enum HeartbeatAction {
    MorningBrief,
    EveningReview,
    UrgentEmailCheck,
    UpcomingMeetingAlert,
    WeeklyReport,
    NoAction,
}

impl Heartbeat {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn tick(&self) -> Result<Vec<HeartbeatAction>> {
        let now = Local::now();
        let hour = now.hour();
        let weekday = now.weekday();
        let mut actions = Vec::new();

        // 早间 brief (8:00-9:00)
        if hour == 8 {
            actions.push(HeartbeatAction::MorningBrief);
        }

        // 晚间回顾 (18:00-19:00)
        if hour == 18 {
            actions.push(HeartbeatAction::EveningReview);
        }

        // 持续：检查紧急邮件
        if self.config.channels.email.enabled {
            actions.push(HeartbeatAction::UrgentEmailCheck);
        }

        // 持续：检查即将到来的会议
        if self.config.channels.calendar.enabled {
            actions.push(HeartbeatAction::UpcomingMeetingAlert);
        }

        // 周五下午：周报
        if weekday == chrono::Weekday::Fri && hour >= 16 {
            actions.push(HeartbeatAction::WeeklyReport);
        }

        if actions.is_empty() {
            actions.push(HeartbeatAction::NoAction);
        }

        info!("Heartbeat tick: {} actions determined", actions.len());
        Ok(actions)
    }
}
```

**Step 3: 实现 daemon.rs — 主事件循环**

```rust
use anyhow::Result;
use std::time::Duration;
use tokio::time;
use tracing::{info, error};

use crate::config::Config;
use crate::heartbeat::{Heartbeat, HeartbeatAction};

pub async fn run(config: Config) -> Result<()> {
    let interval = Duration::from_secs(config.daemon.heartbeat_interval_secs);
    let heartbeat = Heartbeat::new(config);

    info!("Daemon event loop started");

    let mut ticker = time::interval(interval);

    loop {
        ticker.tick().await;

        match heartbeat.tick().await {
            Ok(actions) => {
                for action in actions {
                    if let Err(e) = handle_action(&heartbeat, action).await {
                        error!("Action failed: {e}");
                    }
                }
            }
            Err(e) => error!("Heartbeat tick failed: {e}"),
        }
    }
}

async fn handle_action(heartbeat: &Heartbeat, action: HeartbeatAction) -> Result<()> {
    match action {
        HeartbeatAction::NoAction => {
            info!("HEARTBEAT_OK");
        }
        HeartbeatAction::MorningBrief => {
            info!("Generating morning brief...");
            // TODO: agent::invoke for morning brief
        }
        HeartbeatAction::UrgentEmailCheck => {
            info!("Checking urgent emails...");
            // TODO: channels::email::check_urgent
        }
        HeartbeatAction::UpcomingMeetingAlert => {
            info!("Checking upcoming meetings...");
            // TODO: channels::calendar::check_upcoming
        }
        action => {
            info!("Action {:?} not yet implemented", action);
        }
    }
    Ok(())
}
```

**Step 4: 更新 main.rs 连接 daemon 和 heartbeat**

接入 `mod daemon; mod heartbeat;`，调用 `daemon::run(config).await`

**Step 5: 编译验证**

```bash
cargo check
```

**Step 6: Commit**

```bash
git commit -m "feat: daemon event loop with heartbeat scheduler"
```

---

## Task 3: Claude CLI Agent 封装

**Files:**
- Create: `sage-daemon/src/agent.rs`
- Test: `sage-daemon/tests/agent_test.rs`

**Step 1: 实现 agent.rs — Claude CLI 进程管理**

```rust
use anyhow::{Result, Context};
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, debug};

use crate::config::AgentConfig;

pub struct Agent {
    config: AgentConfig,
}

#[derive(Debug)]
pub struct AgentResponse {
    pub text: String,
    pub model: String,
}

impl Agent {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }

    /// 调用 Claude CLI 进行推理
    pub async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<AgentResponse> {
        let mut cmd = Command::new(&self.config.claude_binary);

        cmd.arg("--print")
           .arg("--model").arg(&self.config.default_model)
           .arg("--permission-mode").arg(&self.config.permission_mode)
           .arg("--max-budget-usd").arg(self.config.max_budget_usd.to_string())
           .arg("--add-dir").arg(&self.config.project_dir)
           .arg("--output-format").arg("text")
           .arg("--no-session-persistence");

        if let Some(sp) = system_prompt {
            cmd.arg("--system-prompt").arg(sp);
        }

        cmd.arg(prompt);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        info!("Invoking Claude CLI (model: {})", self.config.default_model);
        debug!("Prompt: {}", &prompt[..prompt.len().min(100)]);

        let output = cmd.output().await
            .context("Failed to execute claude CLI")?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Claude CLI failed: {stderr}");
        }

        Ok(AgentResponse {
            text,
            model: self.config.default_model.clone(),
        })
    }

    /// 用 sonnet 做快速轻量判断
    pub async fn quick_judge(&self, prompt: &str) -> Result<String> {
        let mut config = self.config.clone();
        config.default_model = "sonnet".to_string();
        config.max_budget_usd = 0.05;

        let agent = Agent::new(config);
        let resp = agent.invoke(prompt, None).await?;
        Ok(resp.text)
    }
}
```

**Step 2: 编译验证**

```bash
cargo check
```

**Step 3: Commit**

```bash
git commit -m "feat: Claude CLI agent invocation wrapper"
```

---

## Task 4: Memory 系统

**Files:**
- Create: `sage-daemon/src/memory.rs`
- Create: `sage-daemon/memory/MEMORY.md`
- Create: `sage-daemon/memory/patterns.md`

**Step 1: 实现 memory.rs**

```rust
use anyhow::Result;
use chrono::Local;
use std::path::{Path, PathBuf};
use tracing::info;

pub struct Memory {
    base_dir: PathBuf,
}

impl Memory {
    pub fn new(base_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// 读取核心记忆
    pub fn read_core(&self) -> Result<String> {
        let path = self.base_dir.join("MEMORY.md");
        if path.exists() {
            Ok(std::fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// 追加记忆条目
    pub fn append(&self, file: &str, entry: &str) -> Result<()> {
        let path = self.base_dir.join(file);
        let timestamp = Local::now().format("%Y-%m-%d %H:%M");
        let content = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };

        let updated = format!("{content}\n\n## {timestamp}\n{entry}");
        std::fs::write(&path, updated.trim())?;
        info!("Memory updated: {file}");
        Ok(())
    }

    /// 记录行为模式
    pub fn record_pattern(&self, category: &str, observation: &str) -> Result<()> {
        self.append("patterns.md", &format!("**{category}**: {observation}"))
    }

    /// 记录决策
    pub fn record_decision(&self, context: &str, decision: &str) -> Result<()> {
        self.append("decisions.md",
            &format!("- **Context**: {context}\n- **Decision**: {decision}"))
    }

    /// 获取所有记忆作为上下文（给 Claude 用）
    pub fn as_context(&self) -> Result<String> {
        let mut ctx = String::new();
        for file in ["MEMORY.md", "patterns.md", "decisions.md"] {
            let path = self.base_dir.join(file);
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                ctx.push_str(&format!("\n## {file}\n{content}\n"));
            }
        }
        Ok(ctx)
    }
}
```

**Step 2: 初始化 memory 文件**

创建 `memory/MEMORY.md`:
```markdown
# Sage Daemon Memory

## Core Identity
Sage is Evan's personal AI counselor daemon, running persistently on macOS.

## Learned Patterns
(auto-populated by observation)

## Key Preferences
(auto-populated from interactions)
```

**Step 3: Commit**

```bash
git commit -m "feat: markdown-based memory system"
```

---

## Task 5: Channel Trait + Email/Calendar 适配器

**Files:**
- Create: `sage-daemon/src/channel.rs`
- Create: `sage-daemon/src/channels/email.rs`
- Create: `sage-daemon/src/channels/calendar.rs`
- Create: `sage-daemon/src/applescript.rs`
- Create: `sage-daemon/src/channels/notification.rs`

**Step 1: 定义 Channel trait**

```rust
// channel.rs
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Event {
    pub source: String,      // "email", "calendar", "wechat", "hooks"
    pub event_type: EventType,
    pub title: String,
    pub body: String,
    pub metadata: std::collections::HashMap<String, String>,
    pub timestamp: chrono::DateTime<chrono::Local>,
}

#[derive(Debug, Clone)]
pub enum EventType {
    NewEmail,
    UrgentEmail,
    UpcomingMeeting,
    NewMessage,         // WeChat
    PatternObserved,    // Claude Code hooks
    ScheduledTask,      // Heartbeat
}

#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    async fn poll(&self) -> Result<Vec<Event>>;
}

#[async_trait]
pub trait OutputChannel: Send + Sync {
    async fn send(&self, title: &str, body: &str) -> Result<()>;
}
```

**Step 2: 实现 applescript.rs 执行器**

```rust
// applescript.rs
use anyhow::{Result, Context};
use tokio::process::Command;

pub async fn run(script: &str) -> Result<String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .context("Failed to run osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("AppleScript error: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
```

**Step 3: 实现 email.rs**

```rust
// channels/email.rs — 复用 outlook MCP 的 AppleScript 方法
use anyhow::Result;
use async_trait::async_trait;
use crate::applescript;
use crate::channel::{Channel, Event, EventType};

pub struct EmailChannel {
    pub poll_interval_secs: u64,
}

#[async_trait]
impl Channel for EmailChannel {
    fn name(&self) -> &str { "email" }

    async fn poll(&self) -> Result<Vec<Event>> {
        let script = r#"
            tell application "Microsoft Outlook"
                set unreadMsgs to messages of inbox whose is read is false
                set output to ""
                repeat with msg in (items 1 thru (min of {5, count of unreadMsgs}) of unreadMsgs)
                    set output to output & "SUBJECT:" & subject of msg & "||FROM:" & (address of sender of msg) & "||DATE:" & (time sent of msg as string) & "|||"
                end repeat
                return output
            end tell
        "#;

        let raw = applescript::run(script).await?;
        parse_emails(&raw)
    }
}

fn parse_emails(raw: &str) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    for entry in raw.split("|||") {
        let entry = entry.trim();
        if entry.is_empty() { continue; }

        let mut subject = String::new();
        let mut from = String::new();

        for field in entry.split("||") {
            if let Some(val) = field.strip_prefix("SUBJECT:") {
                subject = val.to_string();
            } else if let Some(val) = field.strip_prefix("FROM:") {
                from = val.to_string();
            }
        }

        events.push(Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
            title: subject,
            body: format!("From: {from}"),
            metadata: [("from".into(), from)].into(),
            timestamp: chrono::Local::now(),
        });
    }
    Ok(events)
}
```

**Step 4: 实现 calendar.rs (类似模式)**

复用 outlook MCP 的日历 AppleScript，使用 `||` 和 `|||` 分隔符格式。

**Step 5: 实现 notification.rs (macOS 通知输出)**

```rust
// channels/notification.rs
use anyhow::Result;
use async_trait::async_trait;
use crate::applescript;
use crate::channel::OutputChannel;

pub struct NotificationChannel;

#[async_trait]
impl OutputChannel for NotificationChannel {
    async fn send(&self, title: &str, body: &str) -> Result<()> {
        let escaped_title = title.replace('"', "\\\"");
        let escaped_body = body.replace('"', "\\\"");
        let script = format!(
            r#"display notification "{escaped_body}" with title "Sage" subtitle "{escaped_title}""#
        );
        applescript::run(&script).await?;
        Ok(())
    }
}
```

**Step 6: Commit**

```bash
git commit -m "feat: channel trait + email/calendar/notification adapters"
```

---

## Task 6: Claude Code Hooks 行为监听

**Files:**
- Create: `sage-daemon/src/channels/hooks.rs`

**Step 1: 实现 hooks.rs — 监听 Claude Code 活动**

```rust
// channels/hooks.rs
use anyhow::Result;
use async_trait::async_trait;
use notify::{Watcher, RecursiveMode, Event as FsEvent};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::info;

use crate::channel::{Channel, Event, EventType};
use crate::memory::Memory;

pub struct HooksChannel {
    watch_dir: PathBuf,
    memory: Memory,
}

impl HooksChannel {
    pub fn new(watch_dir: PathBuf, memory: Memory) -> Self {
        Self { watch_dir, memory }
    }

    /// 分析 Claude Code session 文件，提取使用模式
    fn extract_pattern(&self, path: &std::path::Path) -> Option<String> {
        // 监听 ~/.claude/projects/ 下的 session 变化
        // 提取：常用命令、活跃时段、常编辑文件
        let filename = path.file_name()?.to_str()?;

        if filename.contains("memory") || filename == "MEMORY.md" {
            return Some("memory_updated".into());
        }
        if filename.contains("session") {
            return Some("session_activity".into());
        }
        None
    }
}

#[async_trait]
impl Channel for HooksChannel {
    fn name(&self) -> &str { "hooks" }

    async fn poll(&self) -> Result<Vec<Event>> {
        // hooks 通道主要通过 file watcher 被动触发
        // poll 只做定期的模式汇总
        Ok(vec![])
    }
}
```

**Step 2: Commit**

```bash
git commit -m "feat: Claude Code hooks behavior monitoring"
```

---

## Task 7: 事件路由 + 规则引擎

**Files:**
- Create: `sage-daemon/src/router.rs`
- Modify: `sage-daemon/src/daemon.rs`

**Step 1: 实现 router.rs**

```rust
use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::channel::{Event, EventType, OutputChannel};
use crate::channels::notification::NotificationChannel;
use crate::memory::Memory;

pub struct Router {
    agent: Agent,
    memory: Memory,
    notifier: NotificationChannel,
}

impl Router {
    pub fn new(agent: Agent, memory: Memory) -> Self {
        Self {
            agent,
            memory,
            notifier: NotificationChannel,
        }
    }

    pub async fn route(&self, event: Event) -> Result<()> {
        match self.classify(&event) {
            Priority::Immediate => self.handle_immediate(event).await,
            Priority::Normal => self.handle_normal(event).await,
            Priority::Background => self.handle_background(event).await,
        }
    }

    fn classify(&self, event: &Event) -> Priority {
        match event.event_type {
            EventType::UrgentEmail => Priority::Immediate,
            EventType::UpcomingMeeting => Priority::Immediate,
            EventType::NewEmail => Priority::Normal,
            EventType::NewMessage => Priority::Normal,
            EventType::PatternObserved => Priority::Background,
            EventType::ScheduledTask => Priority::Normal,
        }
    }

    async fn handle_immediate(&self, event: Event) -> Result<()> {
        // 紧急事件：直接通知 + 可选 AI 摘要
        let summary = self.agent.invoke(
            &format!("简洁总结这个事件并给出建议行动：\n标题：{}\n内容：{}",
                event.title, event.body),
            Some("你是 Sage，Evan 的个人参谋。用中文简洁回复。"),
        ).await?;

        self.notifier.send(&event.title, &summary.text).await
    }

    async fn handle_normal(&self, event: Event) -> Result<()> {
        // 普通事件：聚合后批量处理
        info!("Normal event queued: {} - {}", event.source, event.title);
        Ok(())
    }

    async fn handle_background(&self, event: Event) -> Result<()> {
        // 后台事件：静默记录到 memory
        self.memory.record_pattern(&event.source, &event.title)?;
        Ok(())
    }
}

enum Priority {
    Immediate,
    Normal,
    Background,
}
```

**Step 2: 更新 daemon.rs 集成 router**

将 heartbeat actions 和 channel events 都路由到 Router。

**Step 3: Commit**

```bash
git commit -m "feat: event router with priority-based handling"
```

---

## Task 8: macOS LaunchAgent 自启动

**Files:**
- Create: `sage-daemon/launchd/com.sage.daemon.plist`
- Create: `sage-daemon/scripts/install.sh`

**Step 1: 创建 LaunchAgent plist**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.sage.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/lyf/.sage/bin/sage-daemon</string>
        <string>--config</string>
        <string>/Users/lyf/.sage/config.toml</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/lyf/.sage/logs/sage.out.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/lyf/.sage/logs/sage.err.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>/Users/lyf</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/Users/lyf/.cargo/bin</string>
    </dict>
</dict>
</plist>
```

**Step 2: 创建安装脚本**

```bash
#!/bin/bash
set -euo pipefail

SAGE_HOME="$HOME/.sage"
PLIST_SRC="$(dirname "$0")/../launchd/com.sage.daemon.plist"
PLIST_DST="$HOME/Library/LaunchAgents/com.sage.daemon.plist"

echo "Building sage-daemon..."
cargo build --release

echo "Installing..."
mkdir -p "$SAGE_HOME/bin" "$SAGE_HOME/logs" "$SAGE_HOME/memory"
cp target/release/sage-daemon "$SAGE_HOME/bin/"
cp config.toml "$SAGE_HOME/config.toml"

echo "Installing LaunchAgent..."
cp "$PLIST_SRC" "$PLIST_DST"
launchctl load "$PLIST_DST"

echo "Sage Daemon installed and started."
echo "Logs: $SAGE_HOME/logs/"
echo "Config: $SAGE_HOME/config.toml"
```

**Step 3: Commit**

```bash
git commit -m "feat: macOS LaunchAgent auto-start setup"
```

---

## Task 9: WeChat Sidecar Bridge (Phase 2)

**Files:**
- Create: `sage-daemon/sidecar/wechat-bridge/package.json`
- Create: `sage-daemon/sidecar/wechat-bridge/index.ts`
- Create: `sage-daemon/src/channels/wechat.rs`

**Step 1: WeChat bridge sidecar (Bun + Wechaty)**

```typescript
// sidecar/wechat-bridge/index.ts
// Wechaty iPad 协议桥接 → Unix socket 通信 → Rust daemon
import { WechatyBuilder } from 'wechaty'
import { serve } from 'bun'

const bot = WechatyBuilder.build({ name: 'sage-wechat' })

bot.on('message', async (msg) => {
    const data = {
        type: 'message',
        from: msg.talker().name(),
        text: msg.text(),
        room: msg.room()?.topic() || null,
        timestamp: new Date().toISOString(),
    }
    // 写入共享文件供 Rust daemon 读取
    await Bun.write('/tmp/sage-wechat-events.jsonl',
        JSON.stringify(data) + '\n', { append: true })
})

bot.start()
```

**Step 2: Rust 端 wechat.rs — 读取 sidecar 事件文件**

```rust
// channels/wechat.rs
use anyhow::Result;
use async_trait::async_trait;
use crate::channel::{Channel, Event, EventType};

pub struct WechatChannel {
    events_file: std::path::PathBuf,
}

#[async_trait]
impl Channel for WechatChannel {
    fn name(&self) -> &str { "wechat" }

    async fn poll(&self) -> Result<Vec<Event>> {
        // 读取 sidecar 写入的 JSONL 事件文件
        // 处理后清空文件
        if !self.events_file.exists() {
            return Ok(vec![]);
        }

        let content = tokio::fs::read_to_string(&self.events_file).await?;
        let mut events = Vec::new();

        for line in content.lines() {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                events.push(Event {
                    source: "wechat".into(),
                    event_type: EventType::NewMessage,
                    title: msg["from"].as_str().unwrap_or("unknown").into(),
                    body: msg["text"].as_str().unwrap_or("").into(),
                    metadata: Default::default(),
                    timestamp: chrono::Local::now(),
                });
            }
        }

        // 清空已处理的事件
        tokio::fs::write(&self.events_file, "").await?;
        Ok(events)
    }
}
```

**Step 3: Commit**

```bash
git commit -m "feat: WeChat sidecar bridge via Wechaty"
```

---

## Task 10: 集成测试 + 端到端验证

**Step 1:** `cargo build --release` 编译
**Step 2:** `./target/release/sage-daemon --foreground --config config.toml` 前台运行验证
**Step 3:** `./target/release/sage-daemon --heartbeat-once` 单次心跳验证
**Step 4:** 安装 LaunchAgent，重启验证自启动
**Step 5:** Commit final

```bash
git commit -m "feat: sage-daemon v0.1 — immortal AI counselor"
```

---

## Execution Order

```
Task 1 (scaffold)
    │
    ├── Task 2 (daemon + heartbeat)  ──── parallel ──── Task 4 (memory)
    │         │
    │         ├── Task 3 (agent/claude)
    │         │
    │         └── Task 5 (channels) ──── parallel ──── Task 6 (hooks)
    │                   │
    │                   └── Task 7 (router)
    │
    ├── Task 8 (LaunchAgent) ──── independent
    │
    ├── Task 9 (WeChat) ──── independent, Phase 2
    │
    └── Task 10 (integration test)
```

**并行分组：**
- **Agent A (核心):** Task 1 → Task 2 → Task 3 → Task 7 → Task 10
- **Agent B (通道):** Task 5 → Task 6
- **Agent C (基础设施):** Task 4 → Task 8 → Task 9
