use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Local};
use std::collections::HashMap;

/// 从外部世界进入 Sage 的事件
#[derive(Debug, Clone)]
pub struct Event {
    pub source: String,
    pub event_type: EventType,
    pub title: String,
    pub body: String,
    #[allow(dead_code)]
    pub metadata: HashMap<String, String>,
    #[allow(dead_code)]
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub enum EventType {
    NewEmail,
    #[allow(dead_code)]
    UrgentEmail,
    UpcomingMeeting,
    NewMessage,
    PatternObserved,
    ScheduledTask,
}

/// 输入通道：从外部拉取事件（邮件、日历、微信、hooks）
#[async_trait]
#[allow(dead_code)]
pub trait InputChannel: Send + Sync {
    fn name(&self) -> &str;
    async fn poll(&self) -> Result<Vec<Event>>;
}

/// 输出通道：向外部推送消息（微信、通知、CLI）
#[async_trait]
#[allow(dead_code)]
pub trait OutputChannel: Send + Sync {
    fn name(&self) -> &str;
    async fn send(&self, title: &str, body: &str) -> Result<()>;
}
