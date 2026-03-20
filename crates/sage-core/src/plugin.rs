//! Plugin Runner — spawns child processes and dispatches task events via JSON-lines stdio.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

// ─── Config ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PluginConfig {
    pub name: String,
    /// e.g. ["python3", "ticktick.py"] or ["sage-plugin-ticktick"]
    pub command: Vec<String>,
    /// e.g. ["task.created", "task.updated"]
    pub events: Vec<String>,
}

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub id: i64,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub due_date: Option<String>,
    pub description: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginEvent {
    pub event_type: String,
    pub task: TaskSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct PluginResponse {
    pub ok: bool,
    pub external_id: Option<String>,
    pub error: Option<String>,
}

// ─── Runner ──────────────────────────────────────────────────────────────────

pub struct PluginRunner {
    configs: Vec<PluginConfig>,
}

impl PluginRunner {
    pub fn new(configs: Vec<PluginConfig>) -> Self {
        Self { configs }
    }

    /// Dispatch an event to all subscribed plugins. Never panics.
    pub async fn dispatch(&self, event: PluginEvent) {
        if self.configs.is_empty() {
            return;
        }
        for cfg in &self.configs {
            if !cfg.events.contains(&event.event_type) {
                continue;
            }
            if let Err(e) = dispatch_to_plugin(cfg, &event).await {
                tracing::warn!(
                    plugin = %cfg.name,
                    event = %event.event_type,
                    "Plugin dispatch failed: {e}"
                );
            }
        }
    }
}

const PLUGIN_TIMEOUT: Duration = Duration::from_secs(10);

async fn dispatch_to_plugin(cfg: &PluginConfig, event: &PluginEvent) -> anyhow::Result<()> {
    let (program, args) = cfg
        .command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("Plugin '{}' has empty command", cfg.name))?;

    let mut child = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn '{}': {e}", cfg.name))?;

    let payload = serde_json::to_string(event)? + "\n";

    let result = timeout(PLUGIN_TIMEOUT, async {
        // Write event to stdin.
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(payload.as_bytes()).await?;
            stdin.flush().await?;
        }
        drop(child.stdin.take());

        // Read one JSON line from stdout.
        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
        let mut lines = BufReader::new(stdout).lines();
        let line = lines.next_line().await?.unwrap_or_default();
        Ok::<String, anyhow::Error>(line)
    })
    .await
    .map_err(|_| anyhow::anyhow!("Plugin '{}' timed out after 10s", cfg.name))??;

    let _ = child.wait().await;

    if result.is_empty() {
        return Ok(());
    }
    match serde_json::from_str::<PluginResponse>(&result) {
        Ok(resp) if !resp.ok => {
            tracing::warn!(
                plugin = %cfg.name,
                "Plugin returned error: {:?}",
                resp.error
            );
        }
        Ok(resp) => {
            tracing::info!(
                plugin = %cfg.name,
                external_id = ?resp.external_id,
                "Plugin dispatch ok"
            );
        }
        Err(e) => {
            tracing::warn!(plugin = %cfg.name, "Bad plugin response JSON: {e}");
        }
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(event_type: &str) -> PluginEvent {
        PluginEvent {
            event_type: event_type.into(),
            task: TaskSnapshot {
                id: 1,
                content: "Buy milk".into(),
                status: "open".into(),
                priority: "normal".into(),
                due_date: None,
                description: None,
                outcome: None,
            },
            changes: None,
        }
    }

    #[tokio::test]
    async fn dispatch_with_no_plugins_is_noop() {
        let runner = PluginRunner::new(vec![]);
        // Should complete instantly without error.
        runner.dispatch(make_event("task.created")).await;
    }

    #[tokio::test]
    async fn dispatch_skips_unsubscribed_event() {
        let cfg = PluginConfig {
            name: "test".into(),
            command: vec!["false".into()], // would fail if called
            events: vec!["task.deleted".into()],
        };
        let runner = PluginRunner::new(vec![cfg]);
        // "task.created" not in events → no child spawned, no error.
        runner.dispatch(make_event("task.created")).await;
    }

    #[tokio::test]
    async fn dispatch_logs_warn_on_bad_command() {
        let cfg = PluginConfig {
            name: "nonexistent".into(),
            command: vec!["__sage_plugin_nonexistent_binary__".into()],
            events: vec!["task.created".into()],
        };
        let runner = PluginRunner::new(vec![cfg]);
        // Should warn but not panic.
        runner.dispatch(make_event("task.created")).await;
    }

    #[tokio::test]
    async fn dispatch_echo_plugin_parses_ok_response() {
        // Use `echo` to return a valid JSON response on stdout.
        let response = r#"{"ok":true,"external_id":"ext-123","error":null}"#;
        let cfg = PluginConfig {
            name: "echo-test".into(),
            command: vec!["echo".into(), response.into()],
            events: vec!["task.created".into()],
        };
        let runner = PluginRunner::new(vec![cfg]);
        runner.dispatch(make_event("task.created")).await;
    }
}
