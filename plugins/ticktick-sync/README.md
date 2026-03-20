# sage-plugin: ticktick-sync

Syncs Sage tasks to [TickTick](https://ticktick.com) via the TickTick Open API.
This is the **reference implementation** for the Sage plugin protocol — it is intentionally small
and dependency-free so you can use it as a starting point for any integration.

## What it does

| Sage event | TickTick action |
|---|---|
| `task.created` | Creates a new task in the configured project |
| `task.updated` with `status → done` | Marks the task complete |
| `task.updated` with `status → cancelled` | Deletes the task |
| `task.updated` (other fields) | Patches the task title / priority / due date |

Priority is mapped as: `P0 → 5` (highest), `P1 → 3`, `P2 → 1`, unset → `0`.

## Setup

### 1. Get a TickTick OAuth token

Follow the [TickTick Open API docs](https://developer.ticktick.com/docs) to create an app and
obtain an OAuth 2.0 Bearer token. The token needs the `tasks:write` scope.

### 2. Find your Project ID

In TickTick, open the project you want tasks to land in, copy the ID from the URL, or use `inbox`
to send them to your default inbox.

### 3. Set environment variables

```bash
export TICKTICK_TOKEN="your_oauth_token_here"
export TICKTICK_PROJECT_ID="your_project_id"   # optional, defaults to "inbox"
```

Add these to `~/.zshrc` / `~/.bashrc` or to a `.env` file you load before starting Sage.

### 4. Register the plugin in `~/.sage/config.toml`

```toml
[[plugins]]
name    = "ticktick-sync"
command = "/path/to/sage/plugins/ticktick-sync/ticktick_sync.py"
events  = ["task.created", "task.updated"]

# Environment variables forwarded to the plugin process
[plugins.env]
TICKTICK_TOKEN      = "${TICKTICK_TOKEN}"
TICKTICK_PROJECT_ID = "${TICKTICK_PROJECT_ID}"
```

Sage will start the plugin process on demand, write one JSON line to its stdin, and read one JSON
line back from stdout.

---

## Plugin Protocol Specification

This section documents the protocol so you can write a plugin in any language.

### Overview

```
Sage ──(stdin JSON-line)──► plugin process ──(stdout JSON-line)──► Sage
```

- One event per process invocation (Sage spawns the plugin fresh for each event).
- All messages are newline-terminated JSON on a single line — no pretty-printing.
- The plugin must always write exactly one response line before exiting.

### Inbound events (Sage → plugin stdin)

#### `task.created`

```json
{
  "type": "task.created",
  "task": {
    "id": 42,
    "content": "Fix login bug",
    "status": "open",
    "priority": "P0",
    "due_date": "2026-03-20",
    "description": "Crash on null email",
    "outcome": null
  }
}
```

#### `task.updated`

```json
{
  "type": "task.updated",
  "task": {
    "id": 42,
    "content": "Fix login bug",
    "status": "done",
    "priority": "P0",
    "due_date": "2026-03-20",
    "description": "Crash on null email",
    "outcome": "Fixed by adding null-check in auth.rs",
    "external_id": "ticktick_abc123"
  },
  "changes": {
    "status": "done",
    "outcome": "Fixed by adding null-check in auth.rs"
  }
}
```

`changes` contains only the fields that changed. `task.external_id` is the ID your plugin
returned when the task was first created — use it to locate the record in the external system.

### Outbound response (plugin stdout → Sage)

#### Success

```json
{"ok": true, "external_id": "ticktick_abc123"}
```

Return `external_id` on creation so Sage can store it and pass it back on future updates.
For updates you may omit or repeat the same `external_id`.

#### Failure

```json
{"ok": false, "error": "HTTP 401: Unauthorized"}
```

Sage will log the error. The task remains in Sage regardless.

### Field reference

| Field | Type | Notes |
|---|---|---|
| `task.id` | integer | Sage-internal task ID |
| `task.content` | string | Task title |
| `task.status` | `"open"` \| `"done"` \| `"cancelled"` | Current status |
| `task.priority` | `"P0"` \| `"P1"` \| `"P2"` \| `null` | Urgency level |
| `task.due_date` | `"YYYY-MM-DD"` \| `null` | ISO date string |
| `task.description` | string \| `null` | Longer body text |
| `task.outcome` | string \| `null` | Filled when completed |
| `task.external_id` | string \| `null` | Your ID from prior response |
| `changes` | object | Subset of task fields that changed |

### Writing a plugin in another language

The contract is minimal — here is the skeleton in pseudocode:

```
line  = read_line(stdin)
event = parse_json(line)

if event.type == "task.created":
    id = create_in_external_system(event.task)
    write_line(stdout, json({"ok": true, "external_id": id}))

elif event.type == "task.updated":
    update_in_external_system(event.task, event.changes)
    write_line(stdout, json({"ok": true, "external_id": event.task.external_id}))

else:
    write_line(stdout, json({"ok": false, "error": "unknown event"}))
```

Any language that can read stdin and write stdout works: Node.js, Ruby, Go, shell scripts, etc.
