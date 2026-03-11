# Sage — 你的个人 AI 参谋

Sage 是一个 macOS 桌面端个人 AI 助手。它通过持续的对话积累对你的了解，在合适的时间主动给出建议——聊工作时像一个资深的 Chief of Staff，聊个人话题时像一个有洞察力的朋友。

## 核心能力

**主动推送** — 后台 Daemon 轮询邮件/日历，在合适的时间生成报告（Morning Brief、Evening Review、Weekly Report）

**对话记忆** — 通过 Chat 积累结构化记忆，对话深度随信任层级递进（初识 → 模式识别 → 深度伙伴）

**Skill 路由** — 根据话题自动切换对话风格：工作决策用 Strategist 模式（专业、结构化），个人反思用 Companion 模式（温暖、有深度）

**认知觉醒** — Evening Review 后触发 Coach → Mirror → Questioner 三角色链，提炼行为模式

## 技术栈

- **前端**：React 18 + TypeScript + react-router-dom
- **桌面框架**：Tauri 2（Rust 后端）
- **数据存储**：SQLite（WAL 模式，FTS5 全文搜索）
- **LLM Provider**：支持 Claude CLI / Anthropic API / OpenAI API / Gemini CLI / DeepSeek API，可排序优先级

## 项目结构

```
sage/
├── apps/sage-desktop/          # Tauri 桌面应用
│   ├── src/                    # React 前端（7 页面）
│   └── src-tauri/              # Rust 后端（commands, tray, daemon）
├── crates/
│   ├── sage-core/              # 核心逻辑（daemon, router, store, agent, skills）
│   └── sage-types/             # 共享类型
├── skills/                     # LLM Skill 文件（对话模式、认知框架）
│   ├── sage-chat-strategist/   # 工作模式 — 策略顾问
│   ├── sage-chat-companion/    # 个人模式 — 倾听者
│   ├── sage-cognitive/         # 认知循环（Observe/Reflect/Question）
│   └── sage-week-rhythm/       # 周节奏框架
├── sop/                        # Daemon 行为规范
├── docs/                       # 设计文档
├── launchd/                    # macOS LaunchAgent 配置
└── scripts/                    # 安装脚本
```

## 快速开始

### 前提条件

- macOS 14+
- Rust toolchain（`rustup`）
- Node.js 20+（推荐用 `fnm`）
- 至少一个 LLM provider（推荐 [Claude CLI](https://docs.anthropic.com/en/docs/claude-code)）

### 构建

```bash
# 安装前端依赖
cd apps/sage-desktop && npm install

# 开发模式（热重载）
cargo tauri dev

# 生产构建
cargo tauri build
```

### 配置

```bash
# 复制示例配置
cp config.example.toml ~/.sage/config.toml

# 编辑配置（设置 provider、channels 等）
vim ~/.sage/config.toml
```

数据存储在 `~/.sage/data/sage.db`，日志在 `~/.sage/logs/`。

## 开发

```bash
cargo check --workspace        # 快速类型检查
cargo clippy --workspace       # Lint
cargo test --workspace         # 运行全部测试
cargo test -p sage-core        # 只跑核心 crate
npx tsc --noEmit               # TypeScript 类型检查（在 apps/sage-desktop 目录下）
```

## 架构要点

### 两条数据流

```
后台 Daemon 事件循环：
  tick() → Channels 轮询 → Heartbeat 时间窗判断 → Router 分级路由
  → Agent LLM 调用 → Store 持久化 → macOS 通知

桌面对话：
  invoke("chat") → 搜索相关 memories（FTS5）→ route_chat_skill() 选择 Skill
  → 加载 SKILL.md + 用户上下文 → LLM 调用 → 解析记忆块 → 返回响应
```

### Skill 系统

每个 Skill 是一个 `SKILL.md` 文件，定义了独立的 LLM persona（身份、语气、框架、规则）。路由器根据消息关键词选择合适的 Skill，默认走 Strategist（工作模式）。

Skill 支持热插拔：在 `~/.sage/skills/{name}/SKILL.md` 放置自定义版本即可覆盖内置版本，无需重新编译。

### 记忆协议

Chat 中 LLM 通过 `` ```sage-memory``` `` 代码块写入结构化记忆，类型包括 task / insight / decision / reminder。记忆通过 FTS5 全文索引，在后续对话中按相关性召回。

## License

MIT
