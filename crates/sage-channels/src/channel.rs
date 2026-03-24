use anyhow::Result;
use async_trait::async_trait;

// Re-export types from sage-types for convenience
pub use sage_types::{Event, EventType};

/// 输入通道：从外部拉取事件（邮件、日历、微信、hooks）
#[async_trait]
pub trait InputChannel: Send + Sync {
    fn name(&self) -> &str;
    async fn poll(&self) -> Result<Vec<Event>>;
}

/// 输出通道：向外部推送消息（微信、通知、CLI）
#[async_trait]
pub trait OutputChannel: Send + Sync {
    fn name(&self) -> &str;
    async fn send(&self, title: &str, body: &str) -> Result<()>;
}
