[English](README.md) | [中文](README.zh-CN.md)

<p align="center">
  <img src="assets/banner.png" alt="Sage" width="900"/>
</p>

<h1 align="center">Sage</h1>

<p align="center">
  <strong>本地优先的 AI 认知操作系统——了解你是谁，观察你在做什么，告诉你下一步该做什么。</strong>
</p>

<p align="center">
  <em>你的数据从不离开你的设备。一次都不会。</em>
</p>

<p align="center">
  <a href="#快速开始">快速开始</a> •
  <a href="#工作原理">工作原理</a> •
  <a href="#功能特性">功能特性</a> •
  <a href="#架构">架构</a> •
  <a href="#贡献">贡献</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/平台-macOS%2014%2B-black?style=flat-square&logo=apple" />
  <img src="https://img.shields.io/badge/运行时-Tauri%202%20+%20Rust-orange?style=flat-square&logo=rust" />
  <img src="https://img.shields.io/badge/存储-SQLite%20(本地)-blue?style=flat-square" />
  <img src="https://img.shields.io/badge/测试-437%20通过-brightgreen?style=flat-square" />
  <img src="https://img.shields.io/badge/i18n-en%20%7C%20zh-purple?style=flat-square" />
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" />
</p>

---

## 痛点

你带一个团队，每天 6 个会。决策在 Teams 里做，跟进在邮件里发，上下文全在脑子里。到了周五，你已经忘了周一承诺了什么。

现有工具帮不上忙——任务管理器需要手动输入，AI 助手每次对话都从零开始，而有趣的工具全要把数据发到别人的服务器上。

## Sage 做什么

Sage 作为后台 daemon 运行在你的 Mac 上。它监控你的邮件、日历和聊天工具（通过 Chrome 插件桥接 Teams/Slack）。它构建关于你的结构化记忆——你是谁、你在做什么、什么对你重要。然后做三件事：

**1. 记住你会忘记的事。**
每一次对话、每一个决定、每一个承诺都被索引到本地 SQLite 数据库中。Sage 不只是存日志——它提取模式、构建认知画像、跨越数周数月连接线索。

**2. 告诉你今天该做什么。**
每天早上生成简报：今天有什么会、哪些任务待办、什么需要关注。任务按你的人格特质生成——匹配你的工作风格和决策偏好。

**3. 自我修正对你的认知。**
验证器用今日证据检验已有记忆。矛盾检测器发现冲突的信念。整合器基于累积证据做出提升、降级或归档的决策——不是靠时间远近，而是靠证据。

## 工作原理

```
你正常工作
     │
     ▼
┌───────────────────────────────────────────────┐
│  输入通道                                      │
│  邮件（Outlook AppleScript）+ 日历             │
│  浏览器桥接（Chrome 插件 → Teams/Slack）        │
│  Hooks（文件监听）+ 微信                        │
│  Feed 智能（Reddit, HN, GitHub, arXiv）        │
└──────────────────────┬────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│  认知管线（自我修正 DAG）                         │
│                                                 │
│  Wave 1: 观察者 ‖ 矛盾检测器                    │
│  Wave 2: 验证器 ‖ 教练 ‖ 人物观察者              │
│  Wave 3: 镜像 ‖ 提问者                          │
│  Wave 4: 整合器（提升/降级/归档）                 │
│  然后：  演化（合并/精简/关联/衰减）              │
│  最后：  Meta（参数/提示词/UI 自我改进）          │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│  SQLite（WAL, FTS5, 54 次迁移）                  │
│  ~/.sage/data/sage.db                           │
│                                                 │
│  记忆 → 四层深度体系                             │
│    事件 → 规律 → 判断 → 信念                     │
│  记忆图谱 → 加权边，扩散激活，Hebbian 强化       │
│  任务 → 结构化提取 + action_key 去重             │
│  人物别名 → 合并追踪                             │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
              桌面 UI（Tauri 2，中英双语）
              DashboardProvider（单一数据源）
              + macOS 通知
```

## 功能特性

### 自我修正认知
不只是积累——每条记忆定期被新证据验证。**验证器**确认或挑战，**矛盾检测器**发现冲突，**整合器**基于证据做提升/降级决策。信念层晋升需要验证次数 ≥ 10 且置信度 ≥ 0.9。

### 认知深度体系
四层，严格的晋升门控：
- **事件**（episodic）：今天发生了什么
- **规律**（semantic）：跨天的重复模式
- **判断**（procedural）：稳定的行为策略（需 5+ 次验证）
- **信念**（axiom）：环境改变也不会放弃的核心身份（需 10+ 次验证，≥0.9 置信度）

### autoDream 记忆演化
后台记忆整合，三重门控触发：距上次 >24h AND 新增记忆 ≥10 AND 安静时段。小批量轮转处理（15+10 条）——多次运行覆盖全部记忆，不超时。

### 人格感知的任务智能
任务按你的工作风格生成——procedural 和 axiom 记忆将你的沟通习惯、决策偏好和优先级框架注入任务规划器。结构化提取 `action_key`（动词:实体:人）防止不同措辞的重复。

### 记忆图谱
力导向图可视化记忆网络。Hebbian 共激活强化边权重。冷边衰减。扩散激活用于上下文检索。

### DashboardProvider 架构
单一 React Context 拥有所有面板数据。`invokeDeduped` 缓存防止重复 Tauri IPC 调用。Daemon 推送 `sage:data:{domain}` 事件精准刷新——不轮询、不过时、不重复。

### Feed 智能
Reddit、HackerNews、GitHub Trending、arXiv、RSS。LLM 评分相关性 + 深度学习阅读。配置热更新——无需重启。

### 人物认知
从邮件和消息中提取他人的行为观察。Apple Photos 风格合并 + 别名重定向——合并后的人名在未来提取时自动映射。

### 多 LLM 支持
Claude CLI、Codex CLI、Gemini CLI、Anthropic API、OpenAI API、DeepSeek API。优先级排序，按模型独立配置。

### 双语
完整中英文界面（479 个翻译键，30 个提示词文件）。所有上下文标题、提示词、UI 字符串均为双语。

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面端 | **Tauri 2** — Rust 后端，单一二进制 |
| 前端 | **React 18** + TypeScript + react-router-dom |
| 存储 | **SQLite** — WAL 模式，FTS5 全文搜索，54 次迁移 |
| LLM | 多 Provider 优先级队列 + 按模型独立配置 |
| 国际化 | 零依赖双语系统（en/zh），479 个键 |
| 平台 | macOS 14+ — LaunchAgent 驱动后台 daemon |
| 插件 | Chrome MV3 — 浏览器桥接 |

## 快速开始

### 环境要求

- macOS 14+
- Rust 工具链（`rustup`）— 通过 `rust-toolchain.toml` 固定到 `1.92.0`
- Node.js 20+（推荐 `fnm`）
- 至少一个 LLM Provider — 推荐：[Claude CLI](https://docs.anthropic.com/en/docs/claude-code)

### 构建与运行

```bash
git clone https://github.com/EvanL1/sage.git
cd sage
bash deploy.sh        # 构建 + 安装到 /Applications + 启动
```

或者开发模式：
```bash
cd apps/sage-desktop
npm install
cargo tauri dev       # 开发模式（热重载）
```

### 配置

```bash
cp config.example.toml ~/.sage/config.toml
vim ~/.sage/config.toml
```

数据：`~/.sage/data/sage.db` — 日志：`~/.sage/logs/`

## 架构

```
Daemon tick() → 轮询通道 → 心跳调度 → 路由事件 → 认知管线

认知管线（13 个 stage，4 波 DAG）：
  Wave 1: 观察者 ‖ 矛盾检测器
  Wave 2: 验证器 ‖ 教练 ‖ 人物观察者
  Wave 3: 镜像 ‖ 提问者
  Wave 4: 整合器
  演化：变换 → 图谱（批量轮转，autoDream 门控）
  Meta：参数 ‖ 提示词 ‖ UI（自我改进）

任务智能（每 3 个 tick）：
  待办 × 近期事件 + 用户特质 → 结构化提取 → action_key 去重

记忆演化（autoDream：24h + 10 条记忆 + 安静时段）：
  合并相似 → 提炼特征 → 压缩冗余 → 关联链接 → 衰减过时
```

## 设计哲学

> "参谋不替主帅做决定，但要让主帅在 3 秒内做出决定。"

1. **辅助决策，不替代决策。** 给选项 + trade-off + 推理。你来决定。
2. **自我修正，不盲目积累。** 验证已有信念，而不只是添加新的。
3. **本地优先，始终如此。** 你的认知模型属于你。
4. **实用主义。** 能用就发。

## 贡献

欢迎贡献。提交前运行测试：

```bash
cargo test --workspace
cargo clippy --workspace
cd apps/sage-desktop && npx tsc --noEmit
```

## License

MIT — 随便用。

---

<p align="center">
  <strong>由 <a href="https://github.com/EvanL1">Evan</a> 构建。</strong><br/>
  <em>因为最好的 AI 助手，是那个已经知道你需要什么的。</em>
</p>
