use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

use crate::channel::{Event, EventType, InputChannel, OutputChannel};

/// WeChat 通道：通过 JSONL 文件与 sidecar 进程通信
///
/// sidecar (Bun + Wechaty) 写入消息到 events_file,
/// Rust daemon 读取并处理后清空。
/// 发送消息时写入 outbox_file, sidecar 读取并发送。
pub struct WechatChannel {
    events_file: PathBuf,
    outbox_file: PathBuf,
}

impl WechatChannel {
    #[allow(dead_code)]
    pub fn new(events_file: PathBuf) -> Self {
        let outbox_file = events_file
            .parent()
            .unwrap_or(std::path::Path::new("/tmp"))
            .join("sage-wechat-outbox.jsonl");
        Self {
            events_file,
            outbox_file,
        }
    }
}

#[async_trait]
impl InputChannel for WechatChannel {
    fn name(&self) -> &str {
        "wechat"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        if !self.events_file.exists() {
            return Ok(vec![]);
        }

        let content = tokio::fs::read_to_string(&self.events_file).await?;
        if content.trim().is_empty() {
            return Ok(vec![]);
        }

        let mut events = Vec::new();
        for line in content.lines() {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                let from = msg["from"].as_str().unwrap_or("unknown");
                let text = msg["text"].as_str().unwrap_or("");
                events.push(Event {
                    source: "wechat".into(),
                    event_type: EventType::NewMessage,
                    title: from.into(),
                    body: text.into(),
                    metadata: [("from".into(), from.into())].into_iter().collect(),
                    timestamp: chrono::Local::now(),
                });
            }
        }

        // 清空已读事件
        tokio::fs::write(&self.events_file, "").await?;
        Ok(events)
    }
}

#[async_trait]
impl OutputChannel for WechatChannel {
    fn name(&self) -> &str {
        "wechat"
    }

    async fn send(&self, title: &str, body: &str) -> Result<()> {
        let msg = serde_json::json!({
            "to": title,
            "text": body,
            "timestamp": chrono::Local::now().to_rfc3339(),
        });

        let line = format!("{}\n", serde_json::to_string(&msg)?);

        // 追加到 outbox, sidecar 负责读取并发送
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.outbox_file)
            .await?
            .write_all(line.as_bytes())
            .await?;

        Ok(())
    }
}

// 需要 tokio::io::AsyncWriteExt
use tokio::io::AsyncWriteExt;
