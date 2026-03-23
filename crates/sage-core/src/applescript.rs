use anyhow::{Context, Result};
use tokio::process::Command;

/// 执行 AppleScript 并返回 stdout
pub async fn run(script: &str) -> Result<String> {
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .current_dir("/tmp")
        .output()
        .await
        .context("Failed to run osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("AppleScript error: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 发送通知（已禁用——macOS 未签名 App 通知不可靠）
pub async fn notify(_title: &str, _body: &str, _route: &str) -> Result<()> {
    Ok(())
}
