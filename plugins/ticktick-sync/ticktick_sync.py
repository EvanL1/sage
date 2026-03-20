#!/usr/bin/env python3
"""
Sage ↔ TickTick sync plugin.

Reads one JSON-lines event from stdin, syncs to TickTick Open API,
writes a JSON result to stdout. Never crashes — always emits a response.

Env vars:
  TICKTICK_TOKEN       OAuth2 Bearer token (required)
  TICKTICK_PROJECT_ID  Target project ID (default: inbox)
"""

import json
import os
import sys
import urllib.request
import urllib.error
from datetime import datetime, timezone

BASE_URL = "https://api.ticktick.com/open/v1"

PRIORITY_MAP = {
    "P0": 5,
    "P1": 3,
    "P2": 1,
}

def get_env() -> tuple[str, str]:
    token = os.environ.get("TICKTICK_TOKEN", "")
    project_id = os.environ.get("TICKTICK_PROJECT_ID", "inbox")
    return token, project_id


def api(method: str, path: str, token: str, body: dict | None = None) -> dict:
    url = BASE_URL + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        raw = resp.read()
        return json.loads(raw) if raw else {}


def format_due_date(date_str: str | None) -> str | None:
    if not date_str:
        return None
    try:
        dt = datetime.fromisoformat(date_str).replace(tzinfo=timezone.utc)
        return dt.strftime("%Y-%m-%dT%H:%M:%S+0000")
    except ValueError:
        return None


def build_task_body(task: dict, project_id: str) -> dict:
    body: dict = {
        "projectId": project_id,
        "title": task.get("content", "Untitled"),
        "priority": PRIORITY_MAP.get(task.get("priority", ""), 0),
    }
    if task.get("description"):
        body["content"] = task["description"]
    due = format_due_date(task.get("due_date"))
    if due:
        body["dueDate"] = due
    return body


def handle_created(task: dict, token: str, project_id: str) -> dict:
    body = build_task_body(task, project_id)
    result = api("POST", f"/project/{project_id}/task", token, body)
    external_id = result.get("id", "")
    return {"ok": True, "external_id": external_id}


def handle_updated(task: dict, changes: dict, token: str, project_id: str) -> dict:
    external_id = task.get("external_id", "")
    if not external_id:
        return {"ok": False, "error": "missing external_id for update"}

    new_status = changes.get("status")

    if new_status == "done":
        api("POST", f"/project/{project_id}/task/{external_id}/complete", token)
        return {"ok": True, "external_id": external_id}

    if new_status == "cancelled":
        api("DELETE", f"/project/{project_id}/task/{external_id}", token)
        return {"ok": True, "external_id": external_id}

    # Generic field update — patch whatever changed
    body = {k: v for k, v in build_task_body(task, project_id).items()}
    body["id"] = external_id
    api("POST", f"/project/{project_id}/task/{external_id}", token, body)
    return {"ok": True, "external_id": external_id}


def process(line: str) -> dict:
    token, project_id = get_env()
    if not token:
        return {"ok": False, "error": "TICKTICK_TOKEN not set"}

    event = json.loads(line)
    event_type = event.get("type")
    task = event.get("task", {})

    if event_type == "task.created":
        return handle_created(task, token, project_id)

    if event_type == "task.updated":
        changes = event.get("changes", {})
        return handle_updated(task, changes, token, project_id)

    return {"ok": False, "error": f"unknown event type: {event_type}"}


def main() -> None:
    line = sys.stdin.readline()
    try:
        result = process(line.strip())
    except urllib.error.HTTPError as exc:
        result = {"ok": False, "error": f"HTTP {exc.code}: {exc.reason}"}
    except urllib.error.URLError as exc:
        result = {"ok": False, "error": f"network error: {exc.reason}"}
    except json.JSONDecodeError as exc:
        result = {"ok": False, "error": f"invalid JSON input: {exc}"}
    except Exception as exc:  # noqa: BLE001
        result = {"ok": False, "error": str(exc)}

    sys.stdout.write(json.dumps(result) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    main()
