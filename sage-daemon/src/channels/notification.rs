use anyhow::Result;
use async_trait::async_trait;

use crate::applescript;
use crate::channel::OutputChannel;

/// macOS 原生通知输出通道
pub struct NotificationChannel;

#[async_trait]
impl OutputChannel for NotificationChannel {
    fn name(&self) -> &str {
        "notification"
    }

    async fn send(&self, title: &str, body: &str) -> Result<()> {
        applescript::notify(title, body).await
    }
}
