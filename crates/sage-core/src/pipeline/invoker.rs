//! ConstrainedInvoker — 约束型 LLM 调用 trait
//!
//! 编译期强制所有 LLM 调用通过此 trait，模块拿不到 raw Agent。
//! 两种模式：invoke（标准）、invoke_long（长超时）。
//! 写入副作用通过 write_action / write_actions_from_text 走 ACTION 约束层。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::warn;

use crate::agent::Agent;
use crate::store::Store;

use super::actions::{self, ActionResult};
use super::parser;

/// 约束型 LLM 调用器 — 所有 cognitive module 只接受此 trait
#[async_trait]
pub trait ConstrainedInvoker: Send + Sync {
    /// 调用 LLM（标准超时 300s）
    async fn invoke(&self, prompt: &str, system: Option<&str>) -> Result<String>;

    /// 调用 LLM（长超时 600s，用于 evolution 等重任务）
    async fn invoke_long(&self, prompt: &str, system: Option<&str>) -> Result<String>;

    /// 受约束的单条 Store 写入
    /// action_line 格式："create_task | content | priority:P1 | due:2026-04-01"
    fn write_action(&self, action_line: &str, allowed: &[&str]) -> Option<i64>;

    /// 从 LLM 文本中解析 ACTION 行并批量执行（带 rate limit）
    fn write_actions_from_text(&self, text: &str, allowed: &[&str], max: usize) -> ActionResult;

    /// 重置调用计数器（批次处理场景）
    fn reset_counter(&self);

    /// 克隆为 Box（用于 tokio::spawn 等需要 owned 值的场景）
    fn clone_boxed(&self) -> Box<dyn ConstrainedInvoker>;

    /// 获取底层 LLM Provider（用于需要直接访问 provider 的场景，如 MemoryIntegrator）
    fn as_provider(&self) -> &dyn crate::provider::LlmProvider;
}

// ─── Harness 便捷方法（基于 ConstrainedInvoker）──────────────────────────

/// invoke → 提取 `<output>` 块 → 返回纯文本
pub async fn invoke_text(invoker: &dyn ConstrainedInvoker, prompt: &str, system: Option<&str>) -> Result<String> {
    let resp = invoker.invoke(prompt, system).await?;
    Ok(parser::extract_output_block(&resp).to_string())
}

/// invoke_long → 提取 `<output>` 块
pub async fn invoke_text_long(invoker: &dyn ConstrainedInvoker, prompt: &str, system: Option<&str>) -> Result<String> {
    let resp = invoker.invoke_long(prompt, system).await?;
    Ok(parser::extract_output_block(&resp).to_string())
}

/// invoke → 提取 + 行命令解析
pub async fn invoke_commands<T, F>(
    invoker: &dyn ConstrainedInvoker, prompt: &str, system: Option<&str>, line_parser: F,
) -> Result<parser::ParseResult<T>>
where
    F: Fn(&str) -> Result<Option<T>>,
{
    let resp = invoker.invoke(prompt, system).await?;
    let result = parser::parse_commands(&resp, line_parser);
    if !result.rejected.is_empty() {
        warn!("harness: {} commands rejected", result.rejected.len());
    }
    Ok(result)
}

/// invoke → 提取 + JSON 反序列化
pub async fn invoke_json<T: serde::de::DeserializeOwned>(
    invoker: &dyn ConstrainedInvoker, prompt: &str, system: Option<&str>,
) -> Result<T> {
    let resp = invoker.invoke(prompt, system).await?;
    let block = parser::extract_output_block(&resp);
    parser::parse_json_fenced(block)
}

/// invoke → 原样返回 trimmed（不做 output 提取）
pub async fn invoke_raw(invoker: &dyn ConstrainedInvoker, prompt: &str, system: Option<&str>) -> Result<String> {
    let resp = invoker.invoke(prompt, system).await?;
    Ok(resp.trim().to_string())
}

// ─── HarnessedAgent 实现 ────────────────────────────────────────────────

/// 持有 Agent + Store 的约束执行器
pub struct HarnessedAgent {
    agent: Agent,
    store: Arc<Store>,
    caller: String,
}

impl HarnessedAgent {
    pub fn new(agent: Agent, store: Arc<Store>, caller: impl Into<String>) -> Self {
        Self { agent, store, caller: caller.into() }
    }

    /// 获取底层 provider 引用（仅供 MemoryIntegrator 等需要 &dyn LlmProvider 的场景）
    pub fn provider(&self) -> &dyn crate::provider::LlmProvider {
        self.agent.provider()
    }
}

#[async_trait]
impl ConstrainedInvoker for HarnessedAgent {
    async fn invoke(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let resp = self.agent.invoke(prompt, system).await?;
        Ok(resp.text)
    }

    async fn invoke_long(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let resp = self.agent.invoke_long(prompt, system).await?;
        Ok(resp.text)
    }

    fn write_action(&self, action_line: &str, allowed: &[&str]) -> Option<i64> {
        actions::execute_single_action(action_line, allowed, &self.store, &self.caller)
    }

    fn write_actions_from_text(&self, text: &str, allowed: &[&str], max: usize) -> ActionResult {
        let allowed_strings: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
        actions::execute_actions(text, &allowed_strings, &self.store, &self.caller, max)
    }

    fn reset_counter(&self) {
        self.agent.reset_counter();
    }

    fn clone_boxed(&self) -> Box<dyn ConstrainedInvoker> {
        Box::new(HarnessedAgent::new(self.agent.clone(), Arc::clone(&self.store), self.caller.clone()))
    }

    fn as_provider(&self) -> &dyn crate::provider::LlmProvider {
        self.agent.provider()
    }
}
