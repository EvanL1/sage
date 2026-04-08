# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Sage

Sage is a local-first AI cognitive OS for macOS. Tauri 2 desktop app with embedded background daemon. Watches email, calendar, chat, browser activity. Builds structured memory of the user with self-correcting cognition. All data stays on-device in SQLite.

## Build & Test

```bash
bash deploy.sh                        # build + install + launch
cargo check -p sage-core              # type check a crate
cargo test -p sage-core -p sage-store # run tests (437 total)
cd apps/sage-desktop && npx tsc --noEmit  # frontend type check
```

## Workspace Structure

```
sage-types       ← pure data models
  ├→ sage-store  ← SQLite persistence (migrations v54)
  ├→ sage-llm    ← LLM provider abstraction
  ├→ sage-channels ← email/calendar/feed input
  └→ sage-core   ← orchestration hub
       ├→ sage-cli     ← CLI binary
       └→ sage-desktop ← Tauri 2 app (React 18 frontend)
```

## Architecture

```
Email/Calendar/WeChat/Browser → Daemon tick → Router → Cognitive Pipeline → SQLite
```

### Cognitive Pipeline — Self-Correcting DAG

All stages are `UserDefinedStage` instances in `custom_stages` table. DAG topology in `pipeline.rs`.

**Evening pipeline (4 waves, 13 stages):**
```
Wave 1: Observer ‖ Contradiction Detector    (parallel — different data)
Wave 2: Verifier ‖ Coach ‖ Person Observer   (depend on Observer)
Wave 3: Mirror ‖ Questioner                  (depend on Coach + Verifier)
Wave 4: Integrator                           (synthesizes all results)
Then:   Evolution Transform → Graph          (housekeeping)
Finally: Meta Params ‖ Prompts ‖ UI          (self-improvement)
```

**Key stages:**
- **Verifier**: tests existing memories against today's evidence (CONFIRM/CHALLENGE)
- **Contradiction Detector**: finds logically contradicting memory pairs
- **Integrator**: makes promote/demote/archive decisions with full context
- **Evolution Transform**: batch merge + synthesize + condense (small batch rotation)
- **Evolution Graph**: link relationships + decay stale memories

**Constraint architecture:**
- ACTION whitelist per stage (22 action types with parameter validation)
- `load_filtered_context`: each stage declares allowed data sources
- Pre-condition SQL gates
- Harness gates on promote_memory: must be sequential, axiom needs val≥10 + conf≥0.9

### Memory Depth Hierarchy
`episodic` → `semantic` → `procedural` → `axiom`. Only Integrator promotes. Axiom = identity/values that survive environment change. Promote gates in harness, not just prompt.

### Memory Layering (Index + Topic)
- **Index layer**: axiom + procedural always loaded into LLM context
- **Topic layer**: semantic + episodic loaded by keyword relevance to current event
- `full_system_prompt(topic_hint)` passes event context for relevant retrieval

### autoDream Evolution Trigger
Three-gate: last evolution >24h AND new memories ≥10 AND quiet hours (outside work_start..work_end). Replaces fixed evening_review trigger.

### DashboardProvider (Frontend)
Single `DashboardContext` owns all 9 data domains. Widgets consume, don't fetch. `invokeDeduped` cache prevents duplicate Tauri IPC. Daemon pushes `sage:data:{domain}` events for targeted refresh.

### Task Planner
Structured extraction with `action_key` (verb:entity:person). Triple dedup: action_key match → text_similarity → LLM context. Granularity gate: atomic only (<2h). Personality-aware: injects procedural/axiom memories.

### Store
SQLite via rusqlite with `Mutex<Connection>`. **Never hold conn across calls to other Store methods** (self-deadlock risk). Migrations at v54.

### Prompts
Bilingual (en/zh), 30 files each. `include_str!()` compiled in, user override at `~/.sage/prompts/{lang}/{name}.md`.

## Configuration

- Config: `~/.sage/config.toml`
- Database: `~/.sage/data/sage.db`
- Logs: `~/.sage/logs/sage.{out,err}.log`
- Deploy: `bash deploy.sh`

## Known Gotchas

- **Mutex deadlock**: `Store.conn()` is `std::sync::Mutex` (not tokio). Never call a Store method while holding conn from another Store method.
- **Proxy**: China network — provider code injects `http_proxy=127.0.0.1:7890` for CLI subprocesses.
- **Single instance**: `tauri-plugin-single-instance` must be first in Builder chain.
- **Evolution batch size**: Keep context ≤25 items per stage to avoid LLM timeout (600s limit).
- **Migration idempotency**: `seed_preset_stages` uses `INSERT OR IGNORE`. To update a preset, DELETE first then re-seed.
- **Bilingual**: All context headers, prompts, and i18n keys must have both en and zh versions.

## Code Style

- Immutable-first, functions <50 lines, files <800 lines, nesting <4 levels
- Chinese comments in sage-core
- Organize by feature/domain, not by type
- After editing: `cargo check -p <crate>` + `cargo test -p <crate>`
- Rust toolchain: `1.92.0`
