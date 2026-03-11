use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::info;

use crate::config::AgentConfig;
use crate::provider::{self, LlmProvider};

pub struct Agent {
    config: AgentConfig,
    provider: Box<dyn LlmProvider>,
    /// 当前实例已调用 invoke() 的次数（原子计数，线程安全）
    invocation_count: AtomicUsize,
}

#[derive(Debug)]
pub struct AgentResponse {
    pub text: String,
}

impl Agent {
    pub fn new(config: AgentConfig) -> Self {
        let provider = provider::create_provider(&config);
        info!("Agent initialized with provider: {}", provider.name());
        Self {
            config,
            provider,
            invocation_count: AtomicUsize::new(0),
        }
    }

    /// 使用已发现的 provider 创建 Agent（用于动态 provider 选择）
    pub fn with_provider(provider: Box<dyn LlmProvider>) -> Self {
        info!("Agent initialized with dynamic provider: {}", provider.name());
        Self {
            config: AgentConfig::default(),
            provider,
            invocation_count: AtomicUsize::new(0),
        }
    }

    /// 调用 LLM 做推理（自动路由到 Claude/Codex/Gemini）
    /// 超过 max_iterations 后返回错误，防止无限循环
    pub async fn invoke(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
    ) -> Result<AgentResponse> {
        // 先获取当前计数，再递增（fetch_add 返回递增前的值）
        let count = self.invocation_count.fetch_add(1, Ordering::SeqCst);
        if count >= self.config.max_iterations {
            return Err(anyhow!(
                "Agent 调用次数已达上限（{}次），请调用 reset_counter() 后重试",
                self.config.max_iterations
            ));
        }
        // 注入实时时钟，让 LLM 所有时间推理都基于系统时间
        let now = chrono::Local::now();
        let time_header = format!(
            "[当前时间: {} ({})]\n\n",
            now.format("%Y-%m-%d %A %H:%M"),
            now.format("%Z UTC%:z"),
        );
        let enriched_system = match system_prompt {
            Some(sp) => format!("{time_header}{sp}"),
            None => time_header,
        };
        let text = self.provider.invoke(prompt, Some(&enriched_system)).await?;
        Ok(AgentResponse { text })
    }

    /// 重置调用计数器（daemon 每个 tick 开始时调用，避免跨 tick 累积）
    pub fn reset_counter(&self) {
        self.invocation_count.store(0, Ordering::SeqCst);
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// Mock LLM Provider，用于测试
    struct MockProvider {
        name: &'static str,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> &str {
            self.name
        }

        async fn invoke(&self, _prompt: &str, _system: Option<&str>) -> Result<String> {
            Ok("mock response".to_string())
        }
    }

    fn make_mock_agent(max_iterations: usize) -> Agent {
        let config = AgentConfig {
            max_iterations,
            ..AgentConfig::default()
        };
        Agent {
            config,
            provider: Box::new(MockProvider { name: "mock" }),
            invocation_count: AtomicUsize::new(0),
        }
    }

    #[test]
    fn test_max_iterations_default() {
        // 默认配置的 max_iterations 应为 10
        let config = AgentConfig::default();
        assert_eq!(config.max_iterations, 10);
    }

    #[tokio::test]
    async fn test_invocation_counter() {
        // 调用 invoke 后计数应递增
        let agent = make_mock_agent(10);
        assert_eq!(agent.invocation_count.load(Ordering::SeqCst), 0);

        agent.invoke("hello", None).await.unwrap();
        assert_eq!(agent.invocation_count.load(Ordering::SeqCst), 1);

        agent.invoke("world", None).await.unwrap();
        assert_eq!(agent.invocation_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_max_iterations_enforced() {
        // 达到上限后应返回 Err
        let agent = make_mock_agent(3);

        agent.invoke("1", None).await.unwrap();
        agent.invoke("2", None).await.unwrap();
        agent.invoke("3", None).await.unwrap();

        // 第 4 次超出上限
        let result = agent.invoke("4", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("上限"));
    }

    #[tokio::test]
    async fn test_reset_counter() {
        // reset_counter 后应能继续调用
        let agent = make_mock_agent(2);

        agent.invoke("1", None).await.unwrap();
        agent.invoke("2", None).await.unwrap();

        // 超出上限
        assert!(agent.invoke("3", None).await.is_err());

        // 重置后恢复
        agent.reset_counter();
        assert!(agent.invoke("after_reset", None).await.is_ok());
    }
}
