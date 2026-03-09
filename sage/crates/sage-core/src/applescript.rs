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

/// 发送 macOS 通知
pub async fn notify(title: &str, body: &str) -> Result<()> {
    let escaped_title = title.replace('"', "\\\"");
    let escaped_body = body.replace('"', "\\\"");
    let script = format!(
        r#"display notification "{escaped_body}" with title "Sage" subtitle "{escaped_title}""#
    );
    run(&script).await?;
    Ok(())
}
