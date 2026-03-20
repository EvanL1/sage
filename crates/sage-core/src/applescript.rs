use anyhow::{Context, Result};
use std::sync::OnceLock;
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

/// 通知回调：(title, body, route)
type NotifyCallback = Box<dyn Fn(&str, &str, &str) + Send + Sync>;
static NOTIFY_CALLBACK: OnceLock<NotifyCallback> = OnceLock::new();

/// 注册通知回调（sage-desktop 在启动时调用）
pub fn set_notify_callback(cb: impl Fn(&str, &str, &str) + Send + Sync + 'static) {
    let _ = NOTIFY_CALLBACK.set(Box::new(cb));
}

/// 发送通知，route 指定点击后跳转的前端路由（如 "/", "/chat", "/about"）
pub async fn notify(title: &str, body: &str, route: &str) -> Result<()> {
    if let Some(cb) = NOTIFY_CALLBACK.get() {
        cb(title, body, route);
        return Ok(());
    }

    // Fallback: 无回调时用 osascript（CLI 模式、测试等场景）
    let escaped_title = title.replace('"', "\\\"");
    let escaped_body = body.replace('"', "\\\"");
    let script = format!(
        r#"display notification "{escaped_body}" with title "Sage" subtitle "{escaped_title}""#
    );
    run(&script).await?;
    Ok(())
}
