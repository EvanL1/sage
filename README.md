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
  <a href="#architecture">Architecture</a> •
  <a href="#contributing">Contributing</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%2014%2B-black?style=flat-square&logo=apple" />
  <img src="https://img.shields.io/badge/runtime-Tauri%202%20+%20Rust-orange?style=flat-square&logo=rust" />
  <img src="https://img.shields.io/badge/storage-SQLite%20(local)-blue?style=flat-square" />
  <img src="https://img.shields.io/badge/tests-437%20passing-brightgreen?style=flat-square" />
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
Every conversation, decision, and commitment gets indexed into a local SQLite database. Sage doesn't just store transcripts — it extracts patterns, builds a cognitive profile, and connects dots across weeks and months.

**2. It tells you what to do today.**
Each morning, Sage generates a brief: what's on your calendar, what tasks are open, what needs attention. Tasks are personality-aware — generated to match your working style and decision preferences.

**3. It self-corrects what it thinks about you.**
A Verifier stage tests existing memories against today's evidence. A Contradiction Detector finds conflicting beliefs. An Integrator promotes, demotes, or archives memories based on accumulated evidence — not just recency.

## How It Works

```
You work normally
     │
     ▼
┌─────────────────────────────────────────────────┐
│  Input Channels                                 │
│  Email (Outlook AppleScript) + Calendar         │
│  Browser Bridge (Chrome Extension → Teams/Slack)│
│  Hooks (file watchers) + WeChat                 │
│  Feed Intelligence (Reddit, HN, GitHub, arXiv)  │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│  Cognitive Pipeline (self-correcting DAG)        │
│                                                 │
│  Wave 1: Observer ‖ Contradiction Detector      │
│  Wave 2: Verifier ‖ Coach ‖ Person Observer     │
│  Wave 3: Mirror ‖ Questioner                    │
│  Wave 4: Integrator (promote/demote/archive)    │
│  Then:   Evolution (merge/condense/link/decay)  │
│  Finally: Meta (params/prompts/ui self-improve) │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│  SQLite (WAL, FTS5, 54 migrations)              │
│  ~/.sage/data/sage.db                           │
│                                                 │
│  Memories → 4-layer depth hierarchy             │
│    episodic → semantic → procedural → axiom     │
│  Memory Graph → weighted edges, spreading       │
│    activation, Hebbian strengthening            │
│  Tasks → structured extraction with action_key  │
│  Person Aliases → merge tracking                │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
              Desktop UI (Tauri 2, bilingual en/zh)
              DashboardProvider (single source of truth)
              + macOS notifications
```

## Features

### Self-Correcting Cognition
Not just accumulation — every memory is periodically verified against new evidence. The **Verifier** confirms or challenges, the **Contradiction Detector** finds conflicts, and the **Integrator** makes evidence-based promote/demote decisions. Axiom promotion requires validation_count ≥ 10 and confidence ≥ 0.9.

### Memory Depth Hierarchy
Four layers with strict promotion gates:
- **Events** (episodic): what happened today
- **Patterns** (semantic): what recurs across days
- **Judgments** (procedural): stable behavioral strategies (requires 5+ validations)
- **Beliefs** (axiom): core identity that survives environment change (requires 10+ validations, ≥0.9 confidence, manual-level scrutiny)

### autoDream Evolution
Background memory consolidation triggered by three gates: 24h since last run AND 10+ new memories AND quiet hours. Processes in small rotating batches (15+10 items) — full coverage over multiple runs without timeouts.

### Personality-Aware Task Intelligence
Tasks are generated matching your working style — procedural and axiom memories inject your communication habits, decision preferences, and priority frameworks into the task planner. Structured extraction with `action_key` (verb:entity:person) prevents duplicates across rephrasing.

### Memory Graph
Force-directed visualization of your memory network. Hebbian co-activation strengthens edges. Cold edges decay. Spreading activation for contextual retrieval.

### DashboardProvider Architecture
Single React Context owns all dashboard data. `invokeDeduped` cache prevents duplicate Tauri IPC calls. Daemon pushes `sage:data:{domain}` events for targeted widget refresh — no polling, no stale data, no duplication.

### Feed Intelligence
Reddit, HackerNews, GitHub Trending, arXiv, RSS. LLM-scored relevance with deep-read learning. Hot-reloaded config — no restart needed.

### People Cognition
Extracts observations about other people from emails and messages. Apple Photos-style merge with alias redirect — merged names automatically remap on future extraction.

### Multi-LLM Support
Claude CLI, Codex CLI, Gemini CLI, Anthropic API, OpenAI API, DeepSeek API. Priority-sorted with per-model configuration.

### Bilingual
Full English/Chinese interface (479 translation keys, 30 prompt files each). All context headers, prompts, and UI strings bilingual.

## Tech Stack

| Layer | Tech |
|-------|------|
| Desktop | **Tauri 2** — Rust backend, single binary |
| Frontend | **React 18** + TypeScript + react-router-dom |
| Storage | **SQLite** — WAL mode, FTS5, 54 migrations |
| LLM | Multi-provider priority queue + per-model config |
| i18n | Zero-dependency bilingual system (en/zh), 479 keys |
| Platform | macOS 14+ — LaunchAgent-driven background daemon |
| Extensions | Chrome MV3 — Browser Bridge |

## Quick Start

### Requirements

- macOS 14+
- Rust toolchain (`rustup`) — pinned to `1.92.0` via `rust-toolchain.toml`
- Node.js 20+ (recommend `fnm`)
- At least one LLM provider — recommended: [Claude CLI](https://docs.anthropic.com/en/docs/claude-code)

### Build & Run

```bash
git clone https://github.com/EvanL1/sage.git
cd sage
bash deploy.sh        # build + install to /Applications + launch
```

Or for development:
```bash
cd apps/sage-desktop
npm install
cargo tauri dev       # dev mode with hot reload
```

### Configure

```bash
cp config.example.toml ~/.sage/config.toml
vim ~/.sage/config.toml
```

Data: `~/.sage/data/sage.db` — Logs: `~/.sage/logs/`

## Architecture

```
Daemon tick() → poll channels → heartbeat schedule → route events → cognitive pipeline

Cognitive Pipeline (13 stages, 4-wave DAG):
  Wave 1: Observer ‖ Contradiction Detector
  Wave 2: Verifier ‖ Coach ‖ Person Observer
  Wave 3: Mirror ‖ Questioner
  Wave 4: Integrator
  Evolution: Transform → Graph (batch rotation, autoDream gated)
  Meta: Params ‖ Prompts ‖ UI (self-improvement)

Task Intelligence (every 3rd tick):
  open tasks × recent events + user traits → structured extraction → action_key dedup

Memory Evolution (autoDream: 24h + 10 memories + quiet hours):
  merge similar → synthesize traits → condense verbose → link related → decay stale
```

## Design Philosophy

> "A chief of staff doesn't make decisions for the commander — but makes sure the commander can decide in 3 seconds."

1. **Assist, never replace.** Options + trade-offs + reasoning. You decide.
2. **Self-correct, don't accumulate.** Verify existing beliefs, don't just add new ones.
3. **Local-first, always.** Your cognitive model belongs to you.
4. **Pragmatism over perfection.** Ship what works.

## Contributing

Contributions welcome. Run tests before submitting:

```bash
cargo test --workspace
cargo clippy --workspace
cd apps/sage-desktop && npx tsc --noEmit
```

## License

MIT — do whatever you want with it.

---

<p align="center">
  <strong>Built by <a href="https://github.com/EvanL1">Evan</a>.</strong><br/>
  <em>Because the best AI assistant is the one that already knows what you need.</em>
</p>
