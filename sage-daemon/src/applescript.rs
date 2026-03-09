use anyhow::{Context, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;

static NOTIFY_SEQ: AtomicU64 = AtomicU64::new(0);

/// 执行 AppleScript 并返回 stdout
pub async fn run(script: &str) -> Result<String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .context("Failed to run osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("AppleScript error: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 发送通知：写入 ~/.sage/notify/ 目录，由 Sage Notification Agent 读取并显示原生通知
pub async fn notify(title: &str, body: &str) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let dir = format!("{home}/.sage/notify");
    tokio::fs::create_dir_all(&dir).await.ok();
    let ts = chrono::Utc::now().timestamp_millis();
    let seq = NOTIFY_SEQ.fetch_add(1, Ordering::Relaxed);
    let path = format!("{dir}/{ts}-{seq}.json");
    let payload = serde_json::json!({"title": title, "body": body});
    tokio::fs::write(&path, payload.to_string()).await?;
    Ok(())
}
