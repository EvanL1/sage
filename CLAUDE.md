# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Sage

Sage is a local-first AI cognitive OS for macOS. It runs as a Tauri 2 desktop app with an embedded background daemon that watches email, calendar, chat, and browser activity. It builds a structured memory of the user, generates daily briefs, detects behavioral patterns, and reflects insights back — all data stays on-device in SQLite.

## Build & Test Commands

```bash
# Build desktop app (release)
cd apps/sage-desktop && cargo tauri build

# Install after build
cp -R target/release/bundle/macos/Sage.app /Applications/Sage.app
cp target/release/sage-desktop ~/.sage/bin/sage

# Check a specific crate
cargo check -p sage-core
cargo check -p sage-types

# Run all tests (~413)
cargo test -p sage-core

# Run tests matching a pattern
cargo test -p sage-core -- message
cargo test -p sage-core -- email_filter
cargo test -p sage-core -- evolution

# Frontend dev
cd apps/sage-desktop && npm run dev    # Vite dev server
cd apps/sage-desktop && npm run build  # tsc + vite build
cd apps/sage-desktop && npm test       # vitest

# Tauri dev (frontend + Rust hot-reload)
cd apps/sage-desktop && cargo tauri dev
```

## Workspace Structure

```
Cargo.toml              # workspace root — 4 members
├── crates/sage-types/  # Shared types (Event, Message, Memory, UserProfile, etc.)
├── crates/sage-core/   # All business logic — daemon, cognitive pipeline, store
├── crates/sage-cli/    # CLI binary (thin wrapper)
└── apps/sage-desktop/  # Tauri 2 app
    ├── src-tauri/      # Rust backend — commands/, main.rs, tray.rs
    └── src/            # React 18 frontend — pages/, components/
```

Rust toolchain: `1.92.0` (pinned in `rust-toolchain.toml`).

## Architecture: How Data Flows

```
Email/Calendar/WeChat/Browser → Daemon tick loop → Router → Cognitive Pipeline → Store (SQLite)
```

### Daemon (`daemon.rs`)
The central event loop. Runs on a configurable heartbeat interval (default 30min). Each tick:
1. Polls all input channels (email, calendar, wechat, hooks, feeds)
2. Evaluates heartbeat schedule (morning brief, evening review, weekly report)
3. Routes events through the cognitive pipeline
4. Runs periodic tasks: task intelligence, staleness check (every 3 ticks)

### Cognitive Pipeline (triggered by Evening Review)
```
Observer → Coach → Mirror → Questioner → Memory Evolution → Calibrator
```
- **Observer** (`observer.rs`): Raw events → semantic annotation with frequency + intent
- **Coach** (`coach.rs`): Annotated observations → behavioral patterns (episodic/semantic/procedural)
- **Mirror** (`mirror.rs`): Patterns → reflections surfaced to user
- **Questioner** (`questioner.rs`): Generates Socratic questions with max-info-gain priority
- **Memory Evolution** (`memory_evolution.rs`): merge → synthesize → link → decay → promote
- **Calibrator** (`calibrator.rs`): Detects when user corrects Sage, generates calibration rules

### Router (`router.rs`)
Dispatches `Event` by type and priority. Holds `Agent` (LLM interface) + `Arc<Store>`.

### Agent (`agent.rs`)
Wraps LLM provider with invocation counting and max_iterations safety valve. Clone produces independent counter (for tokio::spawn). Provider is trait-based (`LlmProvider`) — supports Claude CLI, OpenAI API, Gemini, local models.

### Store (`store.rs` + submodules)
SQLite via rusqlite with `Mutex<Connection>`. Submodules split by domain: `memories.rs`, `messages.rs`, `graph.rs`, `tasks.rs`, `observations.rs`, etc. All migrations in `migrations.rs` (currently at v35). Uses `Store::open_in_memory()` for tests.

### Prompts (`prompts.rs` + `prompts/{en,zh}/*.md`)
Bilingual prompt system. Markdown templates compiled via `include_str!()`. Users can override at `~/.sage/prompts/{lang}/{name}.md`. Language auto-detected from store config.

## Key Patterns

### Memory Depth Hierarchy
Memories have a `depth` field: `episodic` → `semantic` → `procedural` → `axiom`. Evolution pipeline promotes memories upward. Only Evolution can assign `axiom`.

### Message Lifecycle
Messages have `action_state`: `pending` → `resolved` | `expired` | `info_only`. Staleness checker (`staleness.rs`) runs every 3 ticks with three layers: reply detection → TTL expiration → LLM classification.

### Memory Graph
`memory_edges` table stores weighted relationships between memories. Spreading activation (BFS with 0.7 decay) for contextual retrieval. Hebbian strengthening: co-accessed memories get edge weight +0.05.

### Browser Bridge
HTTP server (`bridge.rs`) on `127.0.0.1:18522`. Chrome extension posts message events and behaviors. CORS allows `chrome-extension://` origins.

### Tauri Commands
Frontend invokes Rust via `invoke()`. Commands live in `apps/sage-desktop/src-tauri/src/commands/`. Each file maps to a domain (chat, memory, messages, tasks, etc.). State is shared via `tauri::State<Arc<Store>>` and `tauri::State<Arc<Daemon>>`.

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
