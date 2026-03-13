# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Sage

Sage 是一个 macOS 桌面端个人 AI 助手。Tauri 2（Rust 后端 + React 前端），内嵌持续运行的 Daemon 事件循环。核心能力：

1. **主动推送** — Daemon 轮询邮件/日历/工作 session，在合适时间生成报告（Morning Brief, Evening Review, Weekly Report）
2. **对话记忆** — 通过 Chat 积累结构化记忆，对话深度随记忆量递进（safety → patterns → deep 三层级）
3. **认知觉醒** — Evening Review 后触发 Coach → Mirror → Questioner 三角色链，提炼行为模式

## Build & Test

```bash
# Rust
cargo check --workspace          # 快速类型检查
cargo clippy --workspace         # lint
cargo test --workspace           # 213 tests
cargo test -p sage-core          # 只跑 sage-core
cargo test -p sage-core -- heartbeat  # 跑特定测试

# Frontend
cd apps/sage-desktop
npx tsc --noEmit                 # TypeScript 类型检查

# 完整构建（含 Tauri 打包）
cd apps/sage-desktop && cargo tauri build

# 开发模式
cd apps/sage-desktop && cargo tauri dev
```

## Workspace 结构

```
sage/
├── crates/
│   ├── sage-core/        # 核心业务逻辑（daemon, router, store, channels, LLM agent）
│   └── sage-types/       # 共享类型（Event, UserProfile, Memory 等）
├── apps/
│   └── sage-desktop/
│       ├── src/          # React 前端（8 页面）
│       └── src-tauri/    # Tauri Rust 后端（main.rs, commands.rs, tray.rs）
├── skills/               # LLM 技能文档（注入 system prompt）
├── sop/                  # 全局行为规范 SOP
├── .context/             # 工作上下文（projects, team, stakeholders）
└── mental-models/        # 决策/沟通/识人框架
```

## 架构要点

### 两条数据流

**Daemon 事件循环（后台）**：
`tick()` → Channels 轮询 → Heartbeat 时间窗口判断 → Router 分级路由 → Agent LLM 调用 → Store 持久化 → macOS 通知

**Desktop 对话（前端）**：
`invoke("chat")` → 搜索相关 memories（FTS5）→ 动态构建 system prompt（层级 + 画像 + 铁律）→ LLM 调用 → 解析 `sage-memory` JSON 块写记忆 → 返回文本

### 共享 Store

Daemon 和 Desktop 共享同一 SQLite 实例（`Arc<Store>`），WAL 模式 + busy_timeout=5000 支持并发读写。DB 路径 `~/.sage/data/sage.db`。

### Provider 热重载

每个 tick 检测 provider 变更（Settings 配置 API key 后），无需重启 Daemon。优先级：claude-cli → anthropic-api → openai-api → codex-cli → gemini-cli → deepseek-api。

### 记忆写入协议

Chat 中 LLM 通过 ` ```sage-memory { "category": "...", "content": "..." } ``` ` 块协议写结构化记忆，commands.rs 解析后存入 memories 表，从响应文本中移除。

### 部署方式

macOS LaunchAgent（`~/Library/LaunchAgents/com.sage.daemon.plist`），`sage --background` 启动，窗口隐藏，Daemon 持续运行。用户双击 App 时显示窗口。关闭窗口 = 隐藏（不退出）。

## 关键约定

- **config.toml** 在 `~/.sage/config.toml`，`Config::load_or_default()` 加载
- **Profile → SOP 动态生成**：`profile::generate_sop()` 将 UserProfile 编译成 system prompt 文本
- **Claude Code Session 反向注入**：`session_analyzer.ingest_sessions()` 解析 Claude Code JSONL session 文件，供报告使用
- **记忆同步**：`store.sync_to_claude_memory()` 把 Sage 记忆同步到 `~/.claude/projects/*/memory/MEMORY.md`
- **代理地址** `127.0.0.1:7890` 作为 fallback 写在 `provider.rs`（中国网络环境）
- **通知**：直接调用 `applescript::notify()`，不经过 OutputChannel 抽象

## 自定义命令

项目 `.claude/commands/` 包含非代码类命令（weekly-review, email-draft, meeting-prep 等），这些是 Sage 的「参谋」能力，给 Evan 用的决策支持工具，不是开发命令。

## 上下文文件

- `.context/` — 团队/项目/利益相关者信息
- `mental-models/` — 决策/沟通/识人/跨部门框架
- `templates/` — 邮件/周报/会议纪要模板
- `skills/` — LLM 技能文档，通过 `include_str!` 编译时嵌入
