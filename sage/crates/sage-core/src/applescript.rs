use anyhow::{Context, Result};
use tokio::process::Command;

/// 执行 AppleScript 并返回 stdout
pub async fn run(script: &str) -> Result<String> {
    let output = Command::new("osascript")
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

const SAGE_ICON_PATH: &str = "/Applications/Sage.app/Contents/Resources/icon.icns";
const TERMINAL_NOTIFIER_PATHS: &[&str] = &[
    "/opt/homebrew/bin/terminal-notifier",
    "/usr/local/bin/terminal-notifier",
];

/// 发送 macOS 通知（纯展示，无点击动作）
pub async fn notify(title: &str, body: &str) -> Result<()> {
    let notifier = TERMINAL_NOTIFIER_PATHS
        .iter()
        .find(|p| std::path::Path::new(p).exists());

    if let Some(bin) = notifier {
        let output = Command::new(bin)
            .current_dir("/tmp")
            .arg("-title").arg("Sage")
            .arg("-subtitle").arg(title)
            .arg("-message").arg(body)
            .arg("-appIcon").arg(SAGE_ICON_PATH)
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

    Ok(())
}

/// AppleScript display notification（fallback）
async fn send_applescript_notification(title: &str, body: &str) -> Result<()> {
    let escaped_title = title.replace('"', "\\\"");
    let escaped_body = body.replace('"', "\\\"");
    let script = format!(
        r#"display notification "{escaped_body}" with title "Sage" subtitle "{escaped_title}""#
    );
    run(&script).await?;
    Ok(())
}
