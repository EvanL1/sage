# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Sage

Sage is a local-first AI cognitive OS for macOS. It runs as a Tauri 2 desktop app with an embedded background daemon that watches email, calendar, chat, and browser activity. It builds a structured memory of the user, generates daily briefs, detects behavioral patterns, and reflects insights back — all data stays on-device in SQLite.

## Build & Test Commands

```bash
# Build desktop app (release)
cd apps/sage-desktop && cargo tauri build

# Deploy (build + install to /Applications + ~/.sage/bin/)
bash deploy.sh

# Check a specific crate
cargo check -p sage-core
cargo check -p sage-store

# Run tests by crate (478 total)
cargo test -p sage-store          # 217 tests — SQLite persistence
cargo test -p sage-core           # 203 tests — business logic
cargo test -p sage-types          # 42 tests — shared types
cargo test -p sage-channels       # 10 tests — input channels
cargo test -p sage-llm            # 6 tests — LLM abstraction

# Run tests matching a pattern
cargo test -p sage-core -- pipeline
cargo test -p sage-store -- memory

# Frontend
cd apps/sage-desktop && npm run dev    # Vite dev server
cd apps/sage-desktop && npm run build  # tsc + vite build
cd apps/sage-desktop && npm test       # vitest

# Tauri dev (frontend + Rust hot-reload)
cd apps/sage-desktop && cargo tauri dev
```

## Workspace Crate Dependency DAG

```
sage-types         ← pure data models, no workspace deps
  ├→ sage-store    ← SQLite persistence (migrations v49)
  │    ├→ sage-llm      ← LLM provider abstraction
  │    │
  ├→ sage-channels      ← email/calendar/feed input
  │
  └─────→ sage-core     ← orchestration hub (depends on all 4 above)
              ├→ sage-cli      ← CLI binary
              └→ sage-desktop  ← Tauri 2 app
```

**Key rule**: `sage-cli` and `sage-desktop` depend only on `sage-core` + `sage-types`. They never import `sage-store`/`sage-llm`/`sage-channels` directly. `sage-core/src/lib.rs` re-exports everything:
- `sage_store` as `crate::store`
- `sage_llm::{agent, discovery, provider, AgentConfig}`
- `sage_channels::{applescript, channel}`

## Architecture: How Data Flows

```
Email/Calendar/WeChat/Browser → Daemon tick loop → Router → Cognitive Pipeline → Store (SQLite)
```

### Daemon (`sage-core/src/daemon.rs`)
The central event loop. Runs on a configurable heartbeat interval (default 30min). Each tick:
1. Polls all input channels (email, calendar, wechat, hooks, feeds)
2. Evaluates heartbeat schedule (morning brief, evening review, weekly report)
3. Routes events through the cognitive pipeline
4. Runs periodic tasks: task intelligence, staleness check (every 3 ticks)

### Cognitive Pipeline — DAG Execution Engine (`pipeline.rs` + `pipeline/`)

All cognitive stages are **UserDefinedStage** instances (preset or custom), stored in SQLite `custom_stages` table. No hardcoded module-per-stage files — all run through a unified constraint engine.

**Evening pipeline** (16 preset stages, DAG-ordered):
```
observer → coach → mirror → questioner → calibrator
              └→ person_observer              strategist (parallel)

evolution_merge → synth → condense → link → decay → promote

meta_params → meta_prompts → meta_ui
```

**Constraint architecture** (see `docs/PIPELINE_REVIEW.md` for full details):
- **ConstrainedInvoker trait** (`pipeline/invoker.rs`): compile-time enforced — modules can't access raw Agent
- **ACTION system** (`pipeline/actions.rs`): 22 action types with whitelist + parameter validation + rate limit
- **Data source filtering** (`load_filtered_context`): each stage declares allowed inputs, can't read others
- **Pre-condition SQL**: optional gate query that must return >0 before stage executes

**Self-evolution** (meta stages):
- `meta_params`: adjusts pipeline parameters based on 14-day execution stats
- `meta_prompts`: bakes calibration rules into prompt files at `~/.sage/prompts/`
- `meta_ui`: generates personalized insight pages

### Router (`sage-core/src/router.rs`)
Dispatches `Event` by type and priority. Holds `Agent` (LLM interface) + `Arc<Store>`.

### Agent (`sage-llm/src/agent.rs`)
Wraps LLM provider with invocation counting and max_iterations safety valve. Clone produces independent counter (for tokio::spawn). Provider is trait-based (`LlmProvider`) — supports Claude CLI, OpenAI API, Gemini, local models.

### Store (`sage-store`)
SQLite via rusqlite with `Mutex<Connection>`. Submodules split by domain: `memories.rs`, `messages.rs`, `graph.rs`, `tasks.rs`, `observations.rs`, `pipeline.rs`, etc. All migrations in `migrations.rs` (currently at v49). Uses `Store::open_in_memory()` for tests.

### Prompts (`sage-core/src/prompts.rs` + `prompts/{en,zh}/*.md`)
Bilingual prompt system. Markdown templates compiled via `include_str!()`. Users can override at `~/.sage/prompts/{lang}/{name}.md`. Language auto-detected from store config.

### Tauri Command Layer (`apps/sage-desktop/src-tauri/src/commands/`)
~100 `#[tauri::command]` functions across 11 files, organized by domain: `chat.rs`, `dashboard.rs`, `email.rs`, `feed.rs`, `memory.rs`, `messages.rs`, `pages.rs`, `profile.rs`, `providers.rs`, `reports.rs`, `tasks.rs`. State shared via `tauri::State<Arc<Store>>` and `tauri::State<Arc<Daemon>>`.

### Frontend (`apps/sage-desktop/src/`)
React 18 + TypeScript + react-router-dom (HashRouter). 15 pages in `pages/`, bilingual via `i18n.ts` (530 keys). Key routes: `/` Dashboard, `/chat` Chat, `/about` AboutYou, `/graph` MemoryGraph, `/tasks` Tasks, `/feed` FeedIntelligence, `/mail` Mail, `/messages` MessageFlow, `/people` People, `/settings` Settings.

### CLI (`sage-cli`)
Binary `sage` with two modes: subcommands (`brief`, `status`, `memories`, `search`, `learn`, `observe`, `correct`, `pipe` — all support `--json`) or no-args TUI mode (ratatui live dashboard with vim keys).

## Key Patterns

### Memory Depth Hierarchy
Memories have a `depth` field: `episodic` → `semantic` → `procedural` → `axiom`. Evolution pipeline promotes memories upward. Only Evolution can assign `axiom`.

### Message Lifecycle
Messages have `action_state`: `pending` → `resolved` | `expired` | `info_only`. Staleness checker (`staleness.rs`) runs every 3 ticks with three layers: reply detection → TTL expiration → LLM classification.

### Memory Graph
`memory_edges` table stores weighted relationships between memories. Spreading activation (BFS with 0.7 decay) for contextual retrieval. Hebbian strengthening: co-accessed memories get edge weight +0.05.

### Browser Bridge
HTTP server (`bridge.rs`) on `127.0.0.1:18522`. Chrome extension posts message events and behaviors. CORS allows `chrome-extension://` origins.

## Configuration

- Config file: `~/.sage/config.toml` (see `config.example.toml`)
- Database: `~/.sage/data/sage.db`
- Logs: `~/.sage/logs/sage.{out,err}.log`
- LaunchAgent: `~/Library/LaunchAgents/com.sage.daemon.plist`

## Known Gotchas

- **Proxy required** (China network): `.app` bundles don't inherit shell env. Provider code injects `http_proxy=127.0.0.1:7890` automatically for CLI subprocesses.
- **Single instance**: `tauri-plugin-single-instance` must be first in Builder chain. Second instance focuses existing window + `exit(0)`.
- **LaunchAgent `KeepAlive`**: Use `SuccessfulExit: false`, not `true` — avoids infinite restart loop when single-instance guard exits 0.
- **`max_iterations` safety valve**: Agent limits LLM calls per tick. Long flows (memory evolution ~26 calls) need `reset_counter()` per batch.
- **Outlook AppleScript**: Chinese macOS can't use `default calendar` — must iterate `every calendar`.
- **UTF-8 in `strip_html()`**: Must use `find()` not byte-by-byte iteration — multi-byte chars cause panics.
- **`INSERT OR IGNORE`**: `save_message_with_direction` returns rowid 0 for duplicate rows — check `id > 0` before using the returned ID.

## Code Style

- Immutable-first, functions <50 lines, files <800 lines, nesting <4 levels
- Chinese comments in sage-core (matches team language)
- Error messages in Chinese for store operations
- Organize by feature/domain, not by type
- After editing a file: `cargo check -p <crate>` + `cargo test -p <crate>`
- Rust toolchain: `1.92.0` (pinned in `rust-toolchain.toml`)
