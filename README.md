# Sage — Evan 的个人参谋

## 这是什么

Sage 是一个基于 Claude Code 的个人参谋系统。它理解 Evan 的决策框架、沟通风格和管理哲学，在需要时提供结构化的建议和高效的执行支持。

**不是替身，是参谋。** 决策权永远在 Evan，Sage 提供推荐方案 + 理由 + 备选项。

## 快速开始

```bash
cd ~/dev/digital-twin
claude
```

Claude Code 自动加载 `CLAUDE.md`，Sage 即刻就绪。

## 使用自定义命令

```
/project:email-draft 给 Shawn 发邮件，汇报 PULSE 立项进展，需要他批准预算
/project:weekly-review 帮我生成本周 VoltageEMS 和 PULSE 的进展报告
/project:translate 我们的系统采用边缘优先架构，控制逻辑不依赖云端
/project:meeting-prep 明天跟 Bob 的 1:1，讨论 AI Data Center 机会
/project:code-review 看一下这个 PR，重点关注 Modbus 通信模块的错误处理
/project:strategy-note 观察到竞品 Ageto 支持 85+ 设备驱动，我们的设备适配库是核心壁垒
/project:team-feedback 帮我准备对 [某某] 的季度反馈
```

## 项目结构

```
sage/
├── CLAUDE.md                          # 核心身份 + 思维模型引用
├── README.md                          # 本文件
├── mental-models/                     # 思维模型（Sage 的核心）
│   ├── decision-framework.md          #   决策框架
│   ├── communication-matrix.md        #   沟通矩阵
│   ├── people-assessment.md           #   识人框架
│   └── cross-dept-strategy.md         #   跨部门策略
├── .context/                          # 工作上下文（按需更新）
│   ├── team.md                        #   团队结构
│   ├── projects.md                    #   项目清单
│   ├── vocabulary.md                  #   术语表
│   └── stakeholders.md               #   利益相关者
├── .claude/
│   └── commands/                      # 自定义命令
│       ├── weekly-review.md
│       ├── email-draft.md
│       ├── tech-doc.md
│       ├── meeting-prep.md
│       ├── code-review.md
│       ├── translate.md
│       ├── strategy-note.md
│       └── team-feedback.md
├── templates/                         # 文档模板
│   ├── weekly-progress.md
│   ├── project-proposal.md
│   ├── meeting-notes.md
│   └── email-templates.md
└── workflows/                         # 工作流指南
    └── agent-team-guide.md
```

### 三层架构

| 层 | 目录 | 作用 |
|----|------|------|
| **思维层** | `mental-models/` | 让 Sage 像 Evan 一样思考 |
| **记忆层** | `.context/` + `.claude/memory/` | 让 Sage 知道当前的人、事、项目 |
| **输出层** | `templates/` + `.claude/commands/` | 让 Sage 像 Evan 一样写 |

## 维护指南

| 文件 | 更新频率 | 更新内容 |
|------|----------|----------|
| `.context/projects.md` | 每月/项目变更时 | 项目状态、新项目 |
| `.context/team.md` | 人员变动时 | 团队成员、职责调整 |
| `.context/stakeholders.md` | 关系变化时 | 新的关键人物 |
| `.context/vocabulary.md` | 遇到新术语时 | 新增行业术语 |
| `mental-models/` | 决策习惯变化时 | 新的思维框架 |
| `CLAUDE.md` | 季度 | 工作重点、角色演进 |

## 设计哲学

> "参谋不替主帅做决定，但要让主帅在 3 秒内做出决定。"
