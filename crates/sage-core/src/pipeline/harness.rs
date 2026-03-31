//! LLM 调用 Harness — 基于 &Agent 的旧式调用（feed.rs / daemon.rs 使用）
//!
//! 新代码应使用 pipeline::invoker（基于 ConstrainedInvoker trait）。
//! 保留的方法：invoke_text, invoke_raw

use anyhow::Result;

use crate::agent::Agent;

use super::parser;

// ─── invoke_text ────────────────────────────────────────────────────────────

/// 调用 LLM → 提取 `<output>` 块 → 返回纯文本。适用于 feed 等。
pub async fn invoke_text(agent: &Agent, prompt: &str, system: Option<&str>) -> Result<String> {
    let resp = agent.invoke(prompt, system).await?;
    Ok(parser::extract_output_block(&resp.text).to_string())
}

// ─── invoke_raw ─────────────────────────────────────────────────────────────

/// 调用 LLM → 不做任何提取，原样返回 trimmed 文本。适用于 morning brief / report 等。
pub async fn invoke_raw(agent: &Agent, prompt: &str, system: Option<&str>) -> Result<String> {
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
    async fn invoke_raw_returns_untouched() {
        let agent = agent_with("  <output>this stays</output>  ");
        let result = invoke_raw(&agent, "prompt", None).await.unwrap();
        // raw 不做 output 提取，只 trim
        assert!(result.contains("<output>"));
    }
}
