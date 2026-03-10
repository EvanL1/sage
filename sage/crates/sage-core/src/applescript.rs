use anyhow::{Context, Result};
use tokio::process::Command;

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

/// 发送 macOS 通知，点击后激活 Sage Desktop
///
/// 优先使用 terminal-notifier（支持 `-activate bundleId`，点击可打开 app），
/// 不可用时回退到 AppleScript `display notification`（无点击行为）。
/// 同时写入 `~/.sage/notify/` JSON 文件，供 Desktop/menubar 轮询读取。
const SAGE_BUNDLE_ID: &str = "com.sage.desktop";
const TERMINAL_NOTIFIER_PATHS: &[&str] = &[
    "/opt/homebrew/bin/terminal-notifier",
    "/usr/local/bin/terminal-notifier",
];

pub async fn notify(title: &str, body: &str) -> Result<()> {
    let notifier = TERMINAL_NOTIFIER_PATHS
        .iter()
        .find(|p| std::path::Path::new(p).exists());

    if let Some(bin) = notifier {
        let output = Command::new(bin)
            .arg("-title")
            .arg("Sage")
            .arg("-subtitle")
            .arg(title)
            .arg("-message")
            .arg(body)
            .arg("-activate")
            .arg(SAGE_BUNDLE_ID)
            .arg("-sender")
            .arg(SAGE_BUNDLE_ID)
            .output()
            .await
            .context("Failed to run terminal-notifier")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::debug!("terminal-notifier 失败: {stderr}，回退到 AppleScript");
            send_applescript_notification(title, body).await?;
        }
    } else {
        send_applescript_notification(title, body).await?;
    }

    // 同时写 JSON 到 ~/.sage/notify/，供 Desktop/menubar 轮询读取
    if let Err(e) = write_notify_json(title, body) {
        tracing::debug!("写 notify JSON 失败: {e}");
    }

    Ok(())
}

/// AppleScript display notification（无点击行为，作为 fallback）
async fn send_applescript_notification(title: &str, body: &str) -> Result<()> {
    let escaped_title = title.replace('"', "\\\"");
    let escaped_body = body.replace('"', "\\\"");
    let script = format!(
        r#"display notification "{escaped_body}" with title "Sage" subtitle "{escaped_title}""#
    );
    run(&script).await?;
    Ok(())
}

/// 写通知 JSON 到 ~/.sage/notify/（原子文件名避免冲突）
fn write_notify_json(title: &str, body: &str) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let dir = std::path::PathBuf::from(&home).join(".sage/notify");
    std::fs::create_dir_all(&dir)?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let filename = format!("{ts}-{seq}.json");

    let json = serde_json::json!({
        "title": title,
        "body": body,
        "timestamp": chrono::Local::now().to_rfc3339(),
    });
    std::fs::write(dir.join(filename), json.to_string())?;
    Ok(())
}
