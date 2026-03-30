//! LLM 调用 Harness — 所有结构化 LLM 调用的统一入口
//!
//! 四种调用模式：
//! - `invoke_text`:     提取 `<output>` 块的纯文本（大多数阶段）
//! - `invoke_commands`: 提取 + 行命令解析为 typed enum（evolution/task_intelligence/reconciler 等）
//! - `invoke_json`:     提取 + JSON 反序列化（router task extraction/bridge memory extract）
//! - `invoke_raw`:      不做提取，原样返回（mirror/report/morning brief — 输出直接给用户）
//!
//! 所有模式自动享有：
//! - Agent 层：invocation 计数 + 时钟注入 + structured tracing
//! - Provider 层：3 次指数退避重试
//! - Pipeline 层：阶段超时 + 预算耗尽降级

use anyhow::Result;
use tracing::warn;

use crate::agent::Agent;

use super::parser::{self, ParseResult};

// ─── invoke_text ────────────────────────────────────────────────────────────

/// 调用 LLM → 提取 `<output>` 块 → 返回纯文本。
/// 适用于：observer, coach, staleness, calibrator, strategist, questioner, feed 等。
pub async fn invoke_text(
    agent: &Agent,
    prompt: &str,
    system: Option<&str>,
) -> Result<String> {
    let resp = agent.invoke(prompt, system).await?;
    Ok(parser::extract_output_block(&resp.text).to_string())
}

/// 长超时版（600s），用于 memory evolution 等重分析任务。
pub async fn invoke_text_long(
    agent: &Agent,
    prompt: &str,
    system: Option<&str>,
) -> Result<String> {
    let resp = agent.invoke_long(prompt, system).await?;
    Ok(parser::extract_output_block(&resp.text).to_string())
}

// ─── invoke_commands ────────────────────────────────────────────────────────

/// 调用 LLM → 提取 `<output>` 块 → 逐行解析为 typed commands。
/// 返回成功解析的命令 + 被拒绝的行（含原因）。
/// 适用于：evolution, task_intelligence, reconciler, person_observer, meta 等。
pub async fn invoke_commands<T, F>(
    agent: &Agent,
    prompt: &str,
    system: Option<&str>,
    line_parser: F,
) -> Result<ParseResult<T>>
where
    F: Fn(&str) -> Result<Option<T>>,
{
    let resp = agent.invoke(prompt, system).await?;
    let result = parser::parse_commands(&resp.text, line_parser);
    if !result.rejected.is_empty() {
        warn!("harness: {} commands rejected out of {} total lines",
            result.rejected.len(), result.commands.len() + result.rejected.len());
    }
    Ok(result)
}

// ─── invoke_json ────────────────────────────────────────────────────────────

/// 调用 LLM → 提取 `<output>` 块 → 去 markdown fence → JSON 反序列化。
/// 适用于：router task extraction, bridge memory extract 等。
pub async fn invoke_json<T: serde::de::DeserializeOwned>(
    agent: &Agent,
    prompt: &str,
    system: Option<&str>,
) -> Result<T> {
    let resp = agent.invoke(prompt, system).await?;
    let block = parser::extract_output_block(&resp.text);
    parser::parse_json_fenced(block)
}

// ─── invoke_raw ─────────────────────────────────────────────────────────────

/// 调用 LLM → 不做任何提取，原样返回 trimmed 文本。
/// 适用于：mirror reflection, morning brief, report 等面向用户的自由文本输出。
pub async fn invoke_raw(
    agent: &Agent,
    prompt: &str,
    system: Option<&str>,
) -> Result<String> {
    let resp = agent.invoke(prompt, system).await?;
    Ok(resp.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::LlmProvider;
    use async_trait::async_trait;

    struct FixedProvider(String);

    #[async_trait]
    impl LlmProvider for FixedProvider {
        fn name(&self) -> &str { "test" }
        async fn invoke(&self, _: &str, _: Option<&str>) -> anyhow::Result<String> {
            Ok(self.0.clone())
        }
    }

    fn agent_with(response: &str) -> Agent {
        Agent::with_provider(Box::new(FixedProvider(response.to_string())))
    }

    #[tokio::test]
    async fn invoke_text_extracts_output_block() {
        let agent = agent_with("<thinking>analysis</thinking>\n<output>\nclean text\n</output>");
        let result = invoke_text(&agent, "prompt", None).await.unwrap();
        assert_eq!(result, "clean text");
    }

    #[tokio::test]
    async fn invoke_text_fallback_without_tags() {
        let agent = agent_with("plain response");
        let result = invoke_text(&agent, "prompt", None).await.unwrap();
        assert_eq!(result, "plain response");
    }

    #[tokio::test]
    async fn invoke_commands_parses_typed() {
        let agent = agent_with("<output>\nCMD 1\nCMD 2\nbad line\n</output>");
        let result = invoke_commands(&agent, "prompt", None, |line| {
            if line.starts_with("CMD") {
                Ok(Some(line.to_string()))
            } else {
                Err(anyhow::anyhow!("not a command"))
            }
        }).await.unwrap();
        assert_eq!(result.commands.len(), 2);
        assert_eq!(result.rejected.len(), 1);
    }

    #[tokio::test]
    async fn invoke_json_parses_fenced() {
        let agent = agent_with("```json\n{\"key\": \"value\"}\n```");
        let result: serde_json::Value = invoke_json(&agent, "prompt", None).await.unwrap();
        assert_eq!(result["key"], "value");
    }

    #[tokio::test]
    async fn invoke_json_with_output_tags() {
        let agent = agent_with("Here is the result:\n<output>\n{\"x\": 42}\n</output>");
        let result: serde_json::Value = invoke_json(&agent, "prompt", None).await.unwrap();
        assert_eq!(result["x"], 42);
    }

    #[tokio::test]
    async fn invoke_raw_returns_untouched() {
        let agent = agent_with("  <output>this stays</output>  ");
        let result = invoke_raw(&agent, "prompt", None).await.unwrap();
        // raw 不做 output 提取，只 trim
        assert!(result.contains("<output>"));
    }
}
