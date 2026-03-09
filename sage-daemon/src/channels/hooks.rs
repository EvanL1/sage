use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::channel::{Event, EventType, InputChannel};

/// Claude Code 行为监听：观察使用模式，提取习惯
pub struct HooksChannel {
    watch_dir: PathBuf,
    seen_sessions: Mutex<HashSet<String>>,
}

impl HooksChannel {
    pub fn new(watch_dir: PathBuf) -> Self {
        Self {
            watch_dir,
            seen_sessions: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl InputChannel for HooksChannel {
    fn name(&self) -> &str {
        "hooks"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        let memory_dir = self.watch_dir.join("projects");
        if !memory_dir.exists() {
            return Ok(vec![]);
        }

        let mut events = Vec::new();
        let mut current_active = HashSet::new();
        let now = std::time::SystemTime::now();

        if let Ok(entries) = std::fs::read_dir(&memory_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        let age = now.duration_since(modified).unwrap_or_default();
                        if age.as_secs() < 1800 {
                            let name = entry.file_name().to_string_lossy().to_string();
                            current_active.insert(name);
                        }
                    }
                }
            }
        }

        // 只报告新出现的活跃 session
        let mut seen = self.seen_sessions.lock().unwrap();
        for name in &current_active {
            if !seen.contains(name) {
                events.push(Event {
                    source: "hooks".into(),
                    event_type: EventType::PatternObserved,
                    title: format!("New session started: {name}"),
                    body: String::new(),
                    metadata: Default::default(),
                    timestamp: chrono::Local::now(),
                });
            }
        }

        // 报告结束的 session
        for name in seen.iter() {
            if !current_active.contains(name) {
                events.push(Event {
                    source: "hooks".into(),
                    event_type: EventType::PatternObserved,
                    title: format!("Session ended: {name}"),
                    body: String::new(),
                    metadata: Default::default(),
                    timestamp: chrono::Local::now(),
                });
            }
        }

        *seen = current_active;
        Ok(events)
    }
}
