use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::provider::LlmProvider;

/// 弹性 Provider — 支持重试（指数退避）和 fallback 回退
///
/// 工作流程：
/// 1. 调用 primary，失败则按指数退避重试（最多 max_retries 次）
/// 2. 重试耗尽后，尝试 fallback（如有）
/// 3. 全部失败则返回最后一个错误
pub struct ReliableProvider {
    /// 主 provider
    primary: Box<dyn LlmProvider>,
    /// 回退 provider（primary 全部失败时启用）
    fallback: Option<Box<dyn LlmProvider>>,
    /// 最大重试次数（默认 3）
    max_retries: usize,
    /// 基础退避间隔（默认 1s，实际延迟 = base_delay * 2^attempt）
    base_delay: Duration,
}

impl ReliableProvider {
    /// 创建一个只有 primary 的弹性 provider（默认 3 次重试，1s 基础间隔）
    pub fn new(primary: Box<dyn LlmProvider>) -> Self {
        Self {
            primary,
            fallback: None,
            max_retries: 3,
            base_delay: Duration::from_secs(1),
        }
    }

    /// 设置最大重试次数
    pub fn max_retries(mut self, n: usize) -> Self {
        self.max_retries = n;
        self
    }

    /// 设置基础退避间隔
    pub fn base_delay(mut self, d: Duration) -> Self {
        self.base_delay = d;
        self
    }

    /// 设置回退 provider
    pub fn fallback(mut self, provider: Box<dyn LlmProvider>) -> Self {
        self.fallback = Some(provider);
        self
    }
}

#[async_trait]
impl LlmProvider for ReliableProvider {
    fn name(&self) -> &str {
        "reliable"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let mut last_error: Option<anyhow::Error> = None;

        // 尝试 primary（含重试）
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                // 指数退避：base_delay * 2^(attempt-1)
                let delay = self.base_delay * (1u32 << (attempt - 1));
                warn!(
                    "Provider '{}' 第 {attempt} 次重试，等待 {:?}",
                    self.primary.name(),
                    delay
                );
                sleep(delay).await;
            }

            match self.primary.invoke(prompt, system_prompt).await {
                Ok(result) => {
                    if attempt > 0 {
                        info!("Provider '{}' 第 {attempt} 次重试成功", self.primary.name());
                    }
                    return Ok(result);
                }
                Err(e) => {
                    warn!(
                        "Provider '{}' 第 {} 次调用失败：{e}",
                        self.primary.name(),
                        attempt + 1
                    );
                    last_error = Some(e);
                }
            }
        }

        // primary 全部失败，尝试 fallback
        if let Some(fb) = &self.fallback {
            info!(
                "Primary '{}' 已耗尽重试，切换到 fallback '{}'",
                self.primary.name(),
                fb.name()
            );
            return fb.invoke(prompt, system_prompt).await;
        }

        // 全部失败，返回最后一个错误
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("未知错误")))
    }
}

// ─── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock Provider — 前 N 次失败，之后成功
    struct MockProvider {
        name: String,
        /// 调用次数计数器
        call_count: Arc<AtomicUsize>,
        /// 前几次调用失败
        fail_times: usize,
        /// 成功时返回的内容
        success_response: String,
    }

    impl MockProvider {
        fn new(name: &str, fail_times: usize, success_response: &str) -> Self {
            Self {
                name: name.into(),
                call_count: Arc::new(AtomicUsize::new(0)),
                fail_times,
                success_response: success_response.into(),
            }
        }

        /// 返回调用次数的共享引用（用于断言）
        fn call_count(&self) -> Arc<AtomicUsize> {
            self.call_count.clone()
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn invoke(&self, _prompt: &str, _system_prompt: Option<&str>) -> Result<String> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count < self.fail_times {
                anyhow::bail!("mock 失败 (第 {} 次)", count + 1)
            } else {
                Ok(self.success_response.clone())
            }
        }
    }

    /// 前 2 次失败，第 3 次成功 → 应该在重试后成功
    #[tokio::test]
    async fn test_retry_then_succeed() {
        let primary = MockProvider::new("primary", 2, "ok");
        let call_count = primary.call_count();

        let provider = ReliableProvider::new(Box::new(primary))
            .max_retries(3)
            .base_delay(Duration::from_millis(1));

        let result = provider.invoke("test", None).await;
        assert!(result.is_ok(), "应当在重试后成功");
        assert_eq!(result.unwrap(), "ok");
        // 第 1 次失败 + 第 2 次失败 + 第 3 次成功 = 3 次调用
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    /// primary 全部重试失败，fallback 成功
    #[tokio::test]
    async fn test_fallback_on_exhaustion() {
        // primary 永远失败（fail_times 超过重试次数）
        let primary = MockProvider::new("primary", 999, "primary-ok");
        let fallback = MockProvider::new("fallback", 0, "fallback-ok");
        let fallback_count = fallback.call_count();

        let provider = ReliableProvider::new(Box::new(primary))
            .max_retries(2)
            .base_delay(Duration::from_millis(1))
            .fallback(Box::new(fallback));

        let result = provider.invoke("test", None).await;
        assert!(result.is_ok(), "fallback 应当成功");
        assert_eq!(result.unwrap(), "fallback-ok");
        // fallback 应被调用 1 次
        assert_eq!(fallback_count.load(Ordering::SeqCst), 1);
    }

    /// primary 和 fallback 全部失败，返回错误
    #[tokio::test]
    async fn test_all_fail() {
        let primary = MockProvider::new("primary", 999, "");
        let fallback = MockProvider::new("fallback", 999, "");

        let provider = ReliableProvider::new(Box::new(primary))
            .max_retries(1)
            .base_delay(Duration::from_millis(1))
            .fallback(Box::new(fallback));

        let result = provider.invoke("test", None).await;
        assert!(result.is_err(), "全部失败时应返回错误");
    }

    /// 首次成功不触发重试
    #[tokio::test]
    async fn test_no_retry_on_success() {
        let primary = MockProvider::new("primary", 0, "instant-ok");
        let call_count = primary.call_count();

        let provider = ReliableProvider::new(Box::new(primary))
            .max_retries(3)
            .base_delay(Duration::from_millis(1));

        let result = provider.invoke("test", None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "instant-ok");
        // 只应调用 1 次
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }
}
