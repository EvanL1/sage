# Sage — 你的个人 AI 参谋

> **不是替身，是参谋。** 决策权永远在你，Sage 提供推荐方案 + 理由 + 备选项。

Sage 是一个 macOS 桌面端个人 AI 助手。通过持续的对话积累对你的了解，在合适的时间主动给出建议——聊工作时像一个资深的 Chief of Staff，聊个人话题时像一个有洞察力的朋友。

## 核心能力

| 能力 | 说明 |
|------|------|
| **主动推送** | 后台 Daemon 轮询邮件/日历，生成 Morning Brief、Evening Review、Weekly Report |
| **对话记忆** | Chat 积累结构化记忆，深度随信任层级递进（初识 → 模式识别 → 深度伙伴） |
| **Skill 路由** | 根据话题自动切换对话风格：Strategist（工作/专业）或 Companion（个人/温暖） |
| **认知觉醒** | Evening Review 后触发 Coach → Mirror → Questioner 三角色链，提炼行为模式 |
| **多 Provider** | 支持 Claude CLI / Anthropic API / OpenAI / Gemini / DeepSeek，可排序优先级 |

## 架构

```
┌───────────────────────────────────────────────┐
│               Sage Desktop (Tauri 2)           │
│            React 18 + TypeScript               │
│  ┌──────┐ ┌──────┐ ┌───────┐ ┌──────┐        │
│  │ Chat │ │ Dash │ │History│ │ Set- │        │
│  │      │ │board │ │       │ │tings │        │
│  └──┬───┘ └──┬───┘ └───┬───┘ └──┬───┘        │
│     └────────┴─────────┴────────┘              │
│                   │ Tauri IPC                   │
└───────────────────┼────────────────────────────┘
                    │
┌───────────────────┼────────────────────────────┐
│             sage-core (Rust)                    │
│                   │                             │
│  ┌────────┐  ┌────┴───┐  ┌────────────────┐   │
│  │ Agent  │  │ Store  │  │  Skill Router  │   │
│  │(LLM)  │  │(SQLite)│  │ strategist /   │   │
│  └────┬───┘  └────┬───┘  │ companion     │   │
│       │           │      └────────┬───────┘   │
│  ┌────┴────────┐  │   ┌──────────┴────────┐   │
│  │ Provider    │  │   │ Coach / Mirror /  │   │
│  │ Discovery   │  │   │ Questioner /      │   │
│  │ + Priority  │  │   │ Guardian          │   │
│  └─────────────┘  │   └───────────────────┘   │
│                   │                             │
│  ┌────────┐  ┌────┴────┐  ┌───────────┐       │
│  │Heartbt │  │Channels │  │  Daemon   │       │
│  │(cron)  │  │email/cal│  │ (event    │       │
│  │        │  │hooks    │  │  loop)    │       │
│  └────────┘  └─────────┘  └───────────┘       │
└────────────────────────────────────────────────┘
```

## 技术栈

| 层 | 技术 |
|----|------|
| 运行时 | Rust，~3MB 二进制，<5MB 内存 |
| 存储 | SQLite（WAL 模式，FTS5 全文搜索） |
| 桌面 | Tauri v2 + React 18 + TypeScript |
| LLM | Claude / OpenAI / Gemini / DeepSeek — 多 provider 优先级排序 |
| 数据源 | AppleScript（Outlook 邮件/日历）、Claude Code hooks |
| 部署 | macOS LaunchAgent（开机自启 + 崩溃恢复） |

## 项目结构

```
sage/
├── apps/sage-desktop/          # Tauri 桌面应用
│   ├── src/                    #   React 前端（7 页面）
│   └── src-tauri/              #   Rust 后端（commands, tray, daemon）
├── crates/
│   ├── sage-core/              # 核心逻辑
│   │   ├── skills.rs           #   Skill 路由 + 加载（热插拔）
│   │   ├── store.rs            #   SQLite 存储（记忆、对话、建议）
│   │   ├── agent.rs            #   LLM 调用 + 防护
│   │   ├── discovery.rs        #   Provider 发现 + 优先级选择
│   │   ├── daemon.rs           #   后台事件循环
│   │   ├── router.rs           #   事件分类 + 路由
│   │   ├── channels/           #   email, calendar, hooks
│   │   ├── coach.rs            #   行为模式学习
│   │   ├── mirror.rs           #   温和行为反射
│   │   └── questioner.rs       #   苏格拉底式深度提问
│   └── sage-types/             # 共享类型
├── skills/                     # LLM Skill 文件
│   ├── sage-chat-strategist/   #   工作模式 — 策略顾问
│   ├── sage-chat-companion/    #   个人模式 — 倾听者
│   ├── sage-cognitive/         #   认知循环框架
│   └── sage-week-rhythm/       #   周节奏框架
├── sop/                        # Daemon 行为规范
├── mental-models/              # 决策/沟通/识人框架
├── .context/                   # 团队/项目/利益相关者
└── templates/                  # 邮件/周报/会议纪要模板
```

## 快速开始

```bash
# 安装前端依赖
cd sage/apps/sage-desktop && npm install

# 开发模式（热重载）
cargo tauri dev

# 生产构建
cargo tauri build

# 配置
cp sage/config.example.toml ~/.sage/config.toml
```

```bash
# 开发命令
cargo check --workspace        # 快速类型检查
cargo clippy --workspace       # Lint
cargo test --workspace         # 运行测试（138 tests）
npx tsc --noEmit               # TypeScript 类型检查
```

## Skill 系统

每个 Skill 是一个 `SKILL.md` 文件，定义了独立的 LLM persona。路由器根据消息关键词自动选择：

- **工作话题**（绩效、OKR、决策、会议…）→ `sage-chat-strategist` — 专业、结构化、先给结论
- **个人话题**（情绪、关系、自我、迷茫…）→ `sage-chat-companion` — 温暖、有深度、先倾听

Skill 支持热插拔：在 `~/.sage/skills/{name}/SKILL.md` 放自定义版本覆盖内置版本，无需重编译。

## 设计哲学

> "参谋不替主帅做决定，但要让主帅在 3 秒内做出决定。"

1. **辅助决策，不替代决策** — 给选项 + trade-off，让用户自己选
2. **系统思考** — 看结构，不看表面症状
3. **实用主义** — 能用就行，不追求完美形式
4. **给方向不给路径** — 提供框架让用户填充，不越俎代庖

## License

MIT
