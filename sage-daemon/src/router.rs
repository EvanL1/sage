use anyhow::Result;
use tracing::{error, info};

use crate::agent::Agent;
use crate::applescript;
use crate::channel::{Event, EventType};
use crate::memory::Memory;

const SAGE_SOP: &str = include_str!("../sop/SAGE_SOP.md");

/// 基础 system prompt = SOP 全文 + 简短行为指引
fn build_system_prompt() -> String {
    format!(
        "{SAGE_SOP}\n\n---\n\n## 行为指引\n用中文回复，简洁有结构。通知内容不超过 200 字符。"
    )
}

pub struct Router {
    agent: Agent,
    memory: Memory,
}

enum Priority {
    Immediate,
    Scheduled,
    Normal,
    Background,
}

impl Router {
    pub fn new(agent: Agent, memory: Memory) -> Self {
        Self { agent, memory }
    }

    pub async fn route(&self, event: Event) -> Result<()> {
        match classify(&event) {
            Priority::Immediate => self.handle_immediate(event).await,
            Priority::Scheduled => self.handle_scheduled(event).await,
            Priority::Normal => self.handle_normal(event).await,
            Priority::Background => self.handle_background(event).await,
        }
    }

    /// 构建完整 system prompt = SOP + 记忆上下文
    fn full_system_prompt(&self) -> String {
        let base = build_system_prompt();
        match self.memory.as_context() {
            Ok(ctx) if !ctx.is_empty() => {
                let truncated = if ctx.len() > 2000 {
                    format!("{}...(truncated)", &ctx[..2000])
                } else {
                    ctx
                };
                format!("{base}\n\n## 你的记忆\n{truncated}")
            }
            _ => base,
        }
    }

    /// 处理定时任务（Morning Brief / Evening Review 等）
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
                "新的一周开始了。根据你的记忆，提醒 Evan：\n1. 本周重点事项\n2. 需要跟进的待办\n3. 预期的挑战".into()
            }
            _ => format!("处理定时任务：{}\n{}", event.title, event.body),
        };

        let resp = self.agent.invoke(&prompt, Some(&system)).await?;

        // 通知 + 记忆
        let notify_text = if resp.text.len() > 200 {
            format!("{}...", &resp.text[..200])
        } else {
            resp.text.clone()
        };
        applescript::notify(&event.title, &notify_text).await?;
        self.memory.record_decision(&event.title, &resp.text)?;
        Ok(())
    }

    async fn handle_immediate(&self, event: Event) -> Result<()> {
        let system = self.full_system_prompt();
        let prompt = format!(
            "简洁总结并给出建议行动：\n标题：{}\n内容：{}",
            event.title, event.body
        );

        let resp = self.agent.invoke(&prompt, Some(&system)).await?;

        applescript::notify(&event.title, &resp.text).await?;
        self.memory.record_decision(&event.title, &resp.text)?;
        Ok(())
    }

    async fn handle_normal(&self, event: Event) -> Result<()> {
        info!("Normal event: {} - {}", event.source, event.title);
        self.memory
            .record_pattern(&event.source, &event.title)?;
        Ok(())
    }

    async fn handle_background(&self, event: Event) -> Result<()> {
        if let Err(e) = self.memory.record_pattern(&event.source, &event.title) {
            error!("Memory write failed: {e}");
        }
        Ok(())
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
