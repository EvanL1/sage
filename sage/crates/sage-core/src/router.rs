use std::sync::Arc;

use anyhow::Result;
use tracing::{error, info};

use sage_types::{Event, EventType};

use crate::agent::Agent;
use crate::applescript;
use crate::coach;
use crate::mirror;
use crate::questioner;
use crate::store::Store;

enum Priority {
    Immediate,
    Scheduled,
    Normal,
    Background,
}

pub struct Router {
    agent: Agent,
    /// 缓存的 SOP 文本（Phase 3 将改为动态生成）
    sop_text: String,
    /// SQLite 存储，替代 memory.rs 的 Markdown 文件系统
    store: Arc<Store>,
}

impl Router {
    pub fn new(agent: Agent, store: Arc<Store>) -> Self {
        let sop_text = String::from("You are Sage, a personal AI counselor. Provide structured advice.");
        Self {
            agent,
            sop_text,
            store,
        }
    }

    /// 设置 SOP 文本（供外部注入动态生成的 SOP）
    pub fn set_sop(&mut self, sop: String) {
        self.sop_text = sop;
    }

    /// 替换 Agent（用于 provider 热更新）
    pub fn set_agent(&mut self, agent: Agent) {
        self.agent = agent;
    }

    /// 触发学习教练：读 observations → 发现模式 → 保存 coach_insight → 归档
    pub async fn run_coach(&self) -> Result<bool> {
        coach::learn(&self.agent, &self.store).await
    }

    /// 触发镜子：从 coach_insight 记忆反映一个行为模式给用户
    pub async fn run_mirror(&self) -> Result<bool> {
        mirror::reflect(&self.agent, &self.store).await
    }

    /// 触发提问者：生成一个苏格拉底式深度问题
    pub async fn run_questioner(&self) -> Result<bool> {
        questioner::ask(&self.agent, &self.store).await
    }

    pub async fn route(&self, event: Event) -> Result<()> {
        match classify(&event) {
            Priority::Immediate => self.handle_immediate(event).await,
            Priority::Scheduled => self.handle_scheduled(event).await,
            Priority::Normal => self.handle_normal(event).await,
            Priority::Background => self.handle_background(event).await,
        }
    }

    /// 构建完整 system prompt = SOP + 记忆上下文（从 SQLite memories 表读取）
    fn full_system_prompt(&self) -> String {
        let base = format!(
            "{}\n\n---\n\n## 行为指引\n用中文回复，简洁有结构。通知内容不超过 200 字符。",
            self.sop_text
        );
        let ctx = self.store.get_memory_context(2000).unwrap_or_default();
        if !ctx.is_empty() {
            format!("{base}\n\n## 你的记忆\n{ctx}")
        } else {
            base
        }
    }

    async fn handle_scheduled(&self, event: Event) -> Result<()> {
        let system = self.full_system_prompt();

        let prompt = match event.title.as_str() {
            "Morning Brief" => format!(
                "现在是早间 briefing 时间。{}\n\n生成今日 morning brief，包括：\n1. 需要关注的邮件\n2. 今日会议安排\n3. 建议的优先事项",
                event.body
            ),
            "Evening Review" => {
                "现在是晚间回顾时间。根据你的记忆，总结今天的工作：\n1. 完成了什么\n2. 发现了什么模式\n3. 明天需要关注什么".into()
            }
            "Weekly Report" => {
                "现在是周报时间。根据你的记忆，生成本周工作周报草稿：\n1. 本周完成的重要事项\n2. 进行中的工作\n3. 下周计划\n4. 需要上级关注的问题".into()
            }
            "Week Start" => {
                "新的一周开始了。根据你的记忆，提醒：\n1. 本周重点事项\n2. 需要跟进的待办\n3. 预期的挑战".into()
            }
            _ => format!("处理定时任务：{}\n{}", event.title, event.body),
        };

        // 跳过 12 小时内已有相同建议的 Claude 调用（节省 API 费用）
        if self.store.has_recent_suggestion(&event.source, &prompt) {
            info!("Skipping duplicate scheduled event: {}", event.title);
            return Ok(());
        }

        let resp = self.agent.invoke(&prompt, Some(&system)).await?;

        if let Err(e) = self.store.record_suggestion(&event.source, &prompt, &resp.text) {
            error!("Failed to persist suggestion: {e}");
        }

        let notify_text = if resp.text.len() > 200 {
            format!("{}...", truncate_str(&resp.text, 200))
        } else {
            resp.text.clone()
        };
        applescript::notify(&event.title, &notify_text).await?;

        // 将决策写入 SQLite memories 表（替代 memory.rs 的 decisions.md）
        if let Err(e) = self.store.append_decision(&event.title, &resp.text) {
            error!("Failed to append decision: {e}");
        }

        // 日志层：记录 observation（低成本）
        let _ = self.store.record_observation("scheduled", &event.title, Some(&resp.text));
        Ok(())
    }

    async fn handle_immediate(&self, event: Event) -> Result<()> {
        let system = self.full_system_prompt();
        let prompt = format!(
            "简洁总结并给出建议行动：\n标题：{}\n内容：{}",
            event.title, event.body
        );

        // 跳过 12 小时内已有相同建议的 Claude 调用
        if self.store.has_recent_suggestion(&event.source, &prompt) {
            info!("Skipping duplicate immediate event: {}", event.title);
            return Ok(());
        }

        let resp = self.agent.invoke(&prompt, Some(&system)).await?;

        if let Err(e) = self.store.record_suggestion(&event.source, &prompt, &resp.text) {
            error!("Failed to persist suggestion: {e}");
        }

        applescript::notify(&event.title, &resp.text).await?;

        // 将决策写入 SQLite memories 表（替代 memory.rs 的 decisions.md）
        if let Err(e) = self.store.append_decision(&event.title, &resp.text) {
            error!("Failed to append decision: {e}");
        }

        let obs = format!("{}: {}", event.title, event.body);
        let _ = self.store.record_observation("urgent", &obs, Some(&resp.text));
        Ok(())
    }

    async fn handle_normal(&self, event: Event) -> Result<()> {
        info!("Normal event: {} - {}", event.source, event.title);

        // 将行为模式写入 SQLite memories 表（替代 memory.rs 的 patterns.md）
        if let Err(e) = self.store.append_pattern(&event.source, &event.title) {
            error!("Failed to append pattern: {e}");
        }

        // Email events: create lightweight suggestion + notification (no Claude needed)
        if event.source == "email" {
            let summary = format!("📧 {}\n{}", event.title, event.body);
            if let Err(e) = self.store.record_suggestion(&event.source, &event.title, &summary) {
                error!("Failed to persist email suggestion: {e}");
            }
            if let Err(e) = applescript::notify("新邮件", &event.title).await {
                error!("Email notification failed: {e}");
            }
        }

        // 日志层：所有 normal 事件记录 observation
        let obs = format!("[{}] {}", event.source, event.title);
        let _ = self.store.record_observation("normal", &obs, None);

        Ok(())
    }

    async fn handle_background(&self, event: Event) -> Result<()> {
        // 将行为模式写入 SQLite memories 表（替代 memory.rs 的 patterns.md）
        if let Err(e) = self.store.append_pattern(&event.source, &event.title) {
            error!("Failed to append pattern: {e}");
        }
        let _ = self.store.record_observation("background", &event.title, None);
        Ok(())
    }
}

/// 安全截断 UTF-8 字符串，确保不在字符中间截断
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    match s.char_indices().take_while(|(i, _)| *i < max_bytes).last() {
        Some((i, c)) => &s[..i + c.len_utf8()],
        None => "",
    }
}

fn classify(event: &Event) -> Priority {
    match event.event_type {
        EventType::UrgentEmail | EventType::UpcomingMeeting => Priority::Immediate,
        EventType::ScheduledTask => Priority::Scheduled,
        EventType::NewEmail | EventType::NewMessage => Priority::Normal,
        EventType::PatternObserved => Priority::Background,
    }
}
