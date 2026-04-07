[English](README.md) | [中文](README.zh-CN.md)

<p align="center">
  <img src="assets/banner.png" alt="Sage" width="900"/>
</p>

<h1 align="center">Sage</h1>

<p align="center">
  <strong>A local-first AI cognitive OS that learns who you are, watches what you do, and tells you what to do next.</strong>
</p>

<p align="center">
  <em>Your data never leaves your machine. Not even once.</em>
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> •
  <a href="#how-it-works">How It Works</a> •
  <a href="#features">Features</a> •
  <a href="#plugin-system">Plugins</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#contributing">Contributing</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%2014%2B-black?style=flat-square&logo=apple" />
  <img src="https://img.shields.io/badge/runtime-Tauri%202%20+%20Rust-orange?style=flat-square&logo=rust" />
  <img src="https://img.shields.io/badge/storage-SQLite%20(local)-blue?style=flat-square" />
  <img src="https://img.shields.io/badge/tests-478%20passing-brightgreen?style=flat-square" />
  <img src="https://img.shields.io/badge/i18n-en%20%7C%20zh-purple?style=flat-square" />
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" />
</p>

---

## The Problem

You manage a team. You're in 6 meetings a day. Decisions happen in Teams, follow-ups land in email, context lives in your head. By Friday, you've forgotten what you promised on Monday.

Existing tools don't help — task managers need manual input, AI assistants forget you every session, and everything interesting requires sending your data to someone else's server.

## What Sage Does

Sage runs as a background daemon on your Mac. It watches your email, calendar, and chat tools (via a Chrome extension that bridges into Teams/Slack). It builds a structured memory of who you are, what you're working on, and what matters to you. Then it does three things:

**1. It remembers what you won't.**
Every conversation, decision, and commitment gets indexed into a local SQLite database with full-text search. Sage doesn't just store transcripts — it extracts patterns, builds a cognitive profile, and connects dots across weeks and months.

**2. It tells you what to do today.**
Each morning, Sage generates a brief: what's on your calendar, what tasks are open, what needs attention. It cross-references your open tasks against recent events and suggests which ones are done, which to cancel, and what new ones to create.

**3. It reflects back what you might miss.**
A rule-based Mirror Layer detects moments of uncertainty, contradiction, and vulnerability in your text — the "cracks in the armor" that reveal where you're actually stuck or growing. Weekly mirror reports surface unresolved threads and behavioral divergences without judgment or advice.

## How It Works

```
You work normally
     │
     ▼
┌─────────────────────────────────────────────────┐
│  Browser Bridge (Chrome Extension)              │
│  Captures: Teams chats, email, AI conversations │
│  Method: XHR interception + Native Messaging    │
│  Privacy: all data goes to local process only   │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│  Sage Daemon (background, runs every N minutes) │
│                                                 │
│  ┌──────────────────────────────────────────┐   │
│  │ Cognitive Pipeline (DAG execution engine) │   │
│  │                                          │   │
│  │ observer → coach → mirror → questioner   │   │
│  │              └→ person_observer           │   │
│  │ calibrator   strategist (parallel)       │   │
│  │                                          │   │
│  │ evolution: merge→synth→condense→link     │   │
│  │            →decay→promote                │   │
│  │                                          │   │
│  │ meta: params→prompts→ui (self-evolution) │   │
│  └──────────────────────────────────────────┘   │
│                                                 │
│  ┌───────────────┐  ┌────────────────────────┐  │
│  │ Task Engine   │  │ Reflective Detector    │  │
│  │ open tasks ×  │  │ 7 rule-based signal    │  │
│  │ recent events │  │ types (zero-LLM)       │  │
│  │ → suggestions │  │                        │  │
│  └───────────────┘  └────────────────────────┘  │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│  SQLite (WAL mode, FTS5, 49 migrations)         │
│  ~/.sage/data/sage.db                           │
│                                                 │
│  Memories → structured, indexed, graph-linked   │
│  Tasks    → with source, priority, due date     │
│  Profile  → evolving cognitive model of you     │
│  Signals  → reflective moments, intensity-scored│
│  Pipeline → execution logs, runtime overrides   │
└─────────────────────────────────────────────────┘
                       │
                       ▼
              Desktop UI (Tauri 2, bilingual en/zh)
              + Plugin outputs (TickTick, Todoist, etc.)
              + macOS notifications
```

## Features

### Proactive Intelligence
Background daemon polls email and calendar, generates **Morning Brief**, **Evening Review**, and **Weekly Report** automatically. You open your laptop, your day is already organized.

### Deep Memory
Every conversation accumulates structured memory with FTS5 indexing. Future interactions recall relevant context by semantic similarity. Sage doesn't start from zero — it starts from everything it knows about you.

### Cognitive Depth
Memories are stratified into four layers: **Events** (what happened) → **Patterns** (what recurs) → **Judgments** (what you believe) → **Beliefs** (what you won't compromise on). Memory evolution automatically promotes through these layers based on evidence.

### Memory Evolution
Automated 6-stage lifecycle keeps memory sharp:
**Merge similar** → **Synthesize traits** → **Compress verbose** → **Link related** → **Decay stale** → **Promote validated**

### Memory Graph
Force-directed visualization of your memory network. Edge weights strengthen through Hebbian co-activation — memories that fire together, wire together. Cold edges decay over time.

### Task Intelligence
Open tasks are cross-referenced against recent events every 3rd daemon tick. Sage suggests: **Done** (with evidence), **Cancel** (outdated), or **New** (detected from conversations). Each task gets LLM-generated verification questions tailored to its specific content — not generic checkboxes.

### Reflective Signal Detection
A zero-LLM rule-based engine scans every ingested text for 7 signal types: **uncertainty**, **contradiction**, **vulnerability**, **defensive abstraction**, **blocked state**, **self-analysis**, and **divergence from baseline**. Weekly mirror reports aggregate these signals into a reflection — what's unresolved, where you diverged from your patterns, where you deployed "armor."

### Cognitive Pipeline (Self-Evolving DAG)
16 preset stages execute as a DAG (directed acyclic graph) — not a fixed linear chain. All stages run through a **unified constraint engine** with:
- **ACTION whitelist**: each stage declares exactly which write operations it can perform (22 action types)
- **Input source filtering**: stages only see data they're authorized to read
- **Rate limiting**: max actions per execution to prevent runaway LLM output
- **Pre-condition SQL gates**: optional queries that must pass before a stage executes
- **Meta stages** (`meta_params`, `meta_prompts`, `meta_ui`): the pipeline evolves itself — adjusting parameters, rewriting prompts, and generating UI pages based on execution history

### Skill Routing
Automatic persona switching: **Strategist** for work decisions, **Companion** for personal reflection. Each skill has its own system prompt and behavioral rules.

### Multi-LLM Support
Claude CLI, Codex CLI, Gemini CLI, Cursor CLI, Anthropic API, OpenAI API, DeepSeek API. Priority-sorted with per-model configuration and thinking depth control.

### Browser Bridge
Chrome extension (MV3) syncs AI conversations from Claude/ChatGPT/Gemini and captures browsing context. XHR interception with Native Messaging pipes everything back to the local Sage process. **No data touches any external server.**

### Bilingual UI
Full English/Chinese interface with 479 translation keys. Language follows your profile setting and switches instantly — no restart needed. All LLM prompts also respect the language preference.

### Self-Correction
Dashboard includes inline correction: when Sage gets a fact wrong, you quote the error, provide the truth, and it calibrates. Corrections accumulate into calibration rules that prevent the same mistakes.

## Plugin System

Sage plugins are **standalone processes in any language**. If it can read JSON from stdin, it's a Sage plugin.

```
Sage writes task to SQLite
     │
     ▼
Plugin Hook fires
     │
     ▼
┌─────────────────────────────┐
│ stdin (JSON)                │
│ {                           │
│   "event": "task_created",  │
│   "task": {                 │
│     "content": "...",       │
│     "priority": "high",     │
│     "due_date": "...",      │
│     "description": "..."    │
│   }                         │
│ }                           │
└──────────────┬──────────────┘
               │
               ▼
     Your code (any language)
               │
               ▼
┌─────────────────────────────┐
│ stdout (JSON)               │
│ { "status": "ok" }          │
└─────────────────────────────┘
```

**Built-in:** TickTick sync (Rust)
**Community:** Todoist, Apple Reminders, Notion — PRs welcome.

```toml
# ~/.sage/config.toml
[[plugins]]
name = "ticktick"
command = "sage-plugin-ticktick"
on = ["task_created", "task_updated"]
```

## Tech Stack

| Layer | Tech |
|-------|------|
| Desktop | **Tauri 2** — Rust backend, single binary |
| Frontend | **React 18** + TypeScript + react-router-dom |
| Storage | **SQLite** — WAL mode, FTS5 full-text search, 49 migrations |
| LLM | Multi-provider priority queue + per-model config |
| i18n | Zero-dependency bilingual system (en/zh), 530 keys |
| Platform | macOS 14+ — LaunchAgent-driven background daemon |
| Extensions | Chrome MV3 — Browser Bridge |

## Project Structure

```
sage/
├── apps/sage-desktop/             # Tauri 2 desktop app
│   ├── src/                       # React 18 frontend (15 pages)
│   │   ├── i18n.ts                # Bilingual translation (530 keys)
│   │   ├── LangContext.tsx        # Language context provider
│   │   └── pages/                 # Dashboard, Chat, Tasks, Settings, ...
│   └── src-tauri/                 # Rust backend (commands, tray, daemon)
├── crates/
│   ├── sage-types/                # Shared types (Event, Memory, Message, etc.)
│   ├── sage-store/                # SQLite persistence (217 tests, 49 migrations)
│   ├── sage-llm/                  # LLM provider abstraction (6 tests)
│   ├── sage-channels/             # Input channels: email, calendar, feeds (10 tests)
│   ├── sage-core/                 # Business logic (203 tests)
│   │   ├── daemon.rs              # Background event loop
│   │   ├── pipeline.rs            # DAG cognitive pipeline engine
│   │   ├── pipeline/              # invoker, actions, stages, parser
│   │   ├── router.rs              # Event routing + skill dispatch
│   │   ├── task_intelligence.rs   # Task signal detection
│   │   ├── reflective_detector.rs # Rule-based reflective signal engine
│   │   └── ...
│   └── sage-cli/                  # CLI binary (thin wrapper)
├── plugins/                       # Plugin implementations (TickTick, etc.)
├── skills/                        # LLM skill files (persona definitions)
├── extensions/chrome/             # Browser Bridge extension (MV3)
└── launchd/                       # macOS LaunchAgent templates
```

## Quick Start

### Requirements

- macOS 14+
- Rust toolchain (`rustup`) — pinned to `1.92.0` via `rust-toolchain.toml`
- Node.js 20+ (recommend `fnm`)
- At least one LLM provider — recommended: [Claude CLI](https://docs.anthropic.com/en/docs/claude-code)

### Build & Run

```bash
git clone https://github.com/EvanL1/sage.git
cd sage/apps/sage-desktop

npm install
cargo tauri dev        # dev mode with hot reload
cargo tauri build      # production build
```

### Configure

```bash
cp config.example.toml ~/.sage/config.toml
vim ~/.sage/config.toml
```

Data: `~/.sage/data/sage.db` — Logs: `~/.sage/logs/`

### Dev Commands

```bash
cargo check --workspace       # type check
cargo clippy --workspace      # lint
cargo test --workspace        # run all tests (478)
npx tsc --noEmit              # TypeScript check (in apps/sage-desktop)
```

## Architecture

```
Background Daemon (event loop):
  tick() → Email/Calendar polling → Time-window check → Cognitive Pipeline
  → DAG-ordered stages (16 preset) → ACTION constraint system → SQLite

Cognitive Pipeline (evening, 16 stages):
  observer → coach → mirror → questioner → calibrator
  → person_observer, strategist (parallel)
  → evolution: merge → synth → condense → link → decay → promote
  → meta: params → prompts → ui (self-evolution)

Desktop Chat:
  invoke("chat") → FTS5 memory search + graph-augmented retrieval
  → route_chat_skill() → SKILL.md + user context → LLM → parse memories → response

Task Intelligence (every 3rd tick):
  open tasks × recent events → LLM comparison → DONE / CANCEL / NEW signals → user review

Memory Evolution (6 stages, daily or manual):
  merge similar → synthesize traits → condense verbose → link related → decay stale → promote validated

Mirror Layer (continuous + weekly):
  text → rule-based signal detection (7 types) → SQLite
  weekly → LLM aggregation → mirror report (unresolved / divergences / armor / open questions)
```

## Design Philosophy

> "A chief of staff doesn't make decisions for the commander — but makes sure the commander can decide in 3 seconds."

1. **Assist, never replace.** Options + trade-offs + reasoning. You decide.
2. **Think in systems.** See structure, not symptoms.
3. **Pragmatism over perfection.** Ship what works.
4. **Direction, not paths.** Provide frameworks, not instructions.

## Comparison

| | Sage | Motion / Reclaim | Mem0 / MemOS |
|---|---|---|---|
| **Runs locally** | All data on your machine | Cloud | Optional |
| **Cognitive profile** | Learns who you are | No | Memory only |
| **Task intelligence** | Cross-references events | Auto-schedules | No |
| **Reflective signals** | Detects uncertainty, contradiction | No | No |
| **Memory evolution** | 6-stage lifecycle | No | Yes |
| **Bilingual** | Full en/zh UI + prompts | English only | English only |
| **Plugin system** | Any language via stdin/stdout | No | No |
| **Open source** | MIT | No | Yes |

## Roadmap

- [ ] Linux support (systemd daemon)
- [ ] Windows support (Windows Service)
- [ ] Plugin marketplace
- [ ] Mobile companion app (read-only view)
- [ ] CalDAV / CardDAV integration
- [ ] MCP server mode (expose Sage as a tool for other agents)

## Contributing

Sage is a personal project built by one person to solve a real problem. Contributions welcome.

**Good first issues:**
- Write a plugin for your favorite task app (Todoist, Notion, Apple Reminders)
- Improve memory evolution heuristics
- Add support for new LLM providers
- Linux/Windows daemon implementation

```bash
# Run tests before submitting
cargo test --workspace
cargo clippy --workspace
```

## License

MIT — do whatever you want with it.

---

<p align="center">
  <strong>Built by <a href="https://github.com/EvanL1">Evan</a>.</strong><br/>
  <em>Because the best AI assistant is the one that already knows what you need.</em>
</p>
