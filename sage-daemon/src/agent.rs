use anyhow::Result;
use tracing::info;

use crate::config::AgentConfig;
use crate::provider::{self, LlmProvider};

pub struct Agent {
    config: AgentConfig,
    provider: Box<dyn LlmProvider>,
}

pub struct AgentResponse {
    pub text: String,
}

impl Agent {
    pub fn new(config: AgentConfig) -> Self {
        let provider = provider::create_provider(&config);
        info!("Agent initialized with provider: {}", provider.name());
        Self { config, provider }
    }

    /// 调用 LLM 做推理（自动路由到 Claude/Codex/Gemini）
    pub async fn invoke(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
    ) -> Result<AgentResponse> {
        let text = self.provider.invoke(prompt, system_prompt).await?;
        Ok(AgentResponse { text })
    }

    /// 快速轻量判断（临时切换到更便宜的配置）
    #[allow(dead_code)]
    pub async fn quick_judge(&self, prompt: &str) -> Result<String> {
        let config = AgentConfig {
            default_model: "sonnet".into(),
            max_budget_usd: 0.05,
            ..self.config.clone()
        };
        let agent = Agent::new(config);
        let resp = agent.invoke(prompt, None).await?;
        Ok(resp.text)
    }
}
