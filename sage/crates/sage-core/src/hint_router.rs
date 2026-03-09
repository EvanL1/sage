use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::info;

use crate::provider::LlmProvider;

/// Hint 路由 Provider — 根据 prompt 开头的 `hint:<name>\n` 前缀分发到对应 provider
///
/// 格式：
/// ```text
/// hint:reasoning
/// 请分析这段代码的时间复杂度...
/// ```
///
/// 工作流程：
/// 1. 解析 prompt 开头的 `hint:<name>\n` 前缀
/// 2. 如有匹配路由，用对应 provider 处理（**去掉 hint 前缀再传**）
/// 3. 无匹配或无 hint，用 default provider
pub struct HintRouter {
    /// 默认 provider（无 hint 或未注册 hint 时使用）
    default: Box<dyn LlmProvider>,
    /// hint 名称 → provider 的路由表
    routes: HashMap<String, Box<dyn LlmProvider>>,
}

impl HintRouter {
    /// 创建只有 default provider 的路由器
    pub fn new(default: Box<dyn LlmProvider>) -> Self {
        Self {
            default,
            routes: HashMap::new(),
        }
    }

    /// 注册一条 hint 路由
    pub fn add_route(mut self, hint_name: impl Into<String>, provider: Box<dyn LlmProvider>) -> Self {
        self.routes.insert(hint_name.into(), provider);
        self
    }

    /// 解析 prompt 开头的 hint 前缀
    ///
    /// 返回 `(hint_name, actual_prompt)`：
    /// - 如果以 `hint:<name>\n` 开头，提取 name 和剩余内容
    /// - 否则返回 `(None, original_prompt)`
    pub fn parse_hint(prompt: &str) -> (Option<&str>, &str) {
        // 必须以 "hint:" 开头
        let Some(rest) = prompt.strip_prefix("hint:") else {
            return (None, prompt);
        };

        // 找到第一个换行符，hint name 就是换行前的内容
        if let Some(newline_pos) = rest.find('\n') {
            let hint_name = &rest[..newline_pos];
            let actual_prompt = &rest[newline_pos + 1..];
            (Some(hint_name), actual_prompt)
        } else {
            // 没有换行符，整行都是 hint name，prompt 为空
            (Some(rest), "")
        }
    }
}

#[async_trait]
impl LlmProvider for HintRouter {
    fn name(&self) -> &str {
        "hint-router"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let (hint, actual_prompt) = Self::parse_hint(prompt);

        match hint {
            Some(name) => {
                if let Some(provider) = self.routes.get(name) {
                    info!("HintRouter: hint='{}' → provider='{}'", name, provider.name());
                    provider.invoke(actual_prompt, system_prompt).await
                } else {
                    info!(
                        "HintRouter: hint='{}' 未注册，回落到 default='{}'",
                        name,
                        self.default.name()
                    );
                    // 未注册的 hint 走 default，但传入去掉前缀后的 prompt
                    self.default.invoke(actual_prompt, system_prompt).await
                }
            }
            None => {
                info!("HintRouter: 无 hint → default='{}'", self.default.name());
                self.default.invoke(prompt, system_prompt).await
            }
        }
    }
}

// ─── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock Provider — 记录调用，返回固定响应
    struct MockProvider {
        name: String,
        call_count: Arc<AtomicUsize>,
        /// 记录最后一次收到的 prompt
        last_prompt: Arc<tokio::sync::Mutex<String>>,
    }

    impl MockProvider {
        fn new(name: &str) -> Self {
            Self {
                name: name.into(),
                call_count: Arc::new(AtomicUsize::new(0)),
                last_prompt: Arc::new(tokio::sync::Mutex::new(String::new())),
            }
        }

        fn call_count(&self) -> Arc<AtomicUsize> {
            self.call_count.clone()
        }

        fn last_prompt(&self) -> Arc<tokio::sync::Mutex<String>> {
            self.last_prompt.clone()
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn invoke(&self, prompt: &str, _system_prompt: Option<&str>) -> Result<String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            *self.last_prompt.lock().await = prompt.to_string();
            Ok(format!("response-from-{}", self.name))
        }
    }

    /// hint:reasoning → opus, hint:fast → haiku, no hint → default
    #[tokio::test]
    async fn test_hint_routing() {
        let default = MockProvider::new("default");
        let opus = MockProvider::new("opus");
        let haiku = MockProvider::new("haiku");

        let opus_count = opus.call_count();
        let haiku_count = haiku.call_count();
        let default_count = default.call_count();

        let router = HintRouter::new(Box::new(default))
            .add_route("reasoning", Box::new(opus))
            .add_route("fast", Box::new(haiku));

        // hint:reasoning → opus
        let r = router.invoke("hint:reasoning\n分析这段代码", None).await.unwrap();
        assert_eq!(r, "response-from-opus");
        assert_eq!(opus_count.load(Ordering::SeqCst), 1);

        // hint:fast → haiku
        let r = router.invoke("hint:fast\n快速回答", None).await.unwrap();
        assert_eq!(r, "response-from-haiku");
        assert_eq!(haiku_count.load(Ordering::SeqCst), 1);

        // no hint → default
        let r = router.invoke("直接提问", None).await.unwrap();
        assert_eq!(r, "response-from-default");
        assert_eq!(default_count.load(Ordering::SeqCst), 1);
    }

    /// 传给 provider 的 prompt 不包含 hint: 前缀
    #[tokio::test]
    async fn test_strip_hint_from_prompt() {
        let default = MockProvider::new("default");
        let opus = MockProvider::new("opus");
        let opus_prompt = opus.last_prompt();

        let router = HintRouter::new(Box::new(default))
            .add_route("reasoning", Box::new(opus));

        router
            .invoke("hint:reasoning\n实际任务内容", None)
            .await
            .unwrap();

        let received_prompt = opus_prompt.lock().await.clone();
        assert_eq!(received_prompt, "实际任务内容", "provider 收到的 prompt 不应包含 hint 前缀");
        assert!(!received_prompt.contains("hint:"), "prompt 中不应有 hint: 前缀");
    }

    /// 未注册的 hint 走 default provider
    #[tokio::test]
    async fn test_unknown_hint_uses_default() {
        let default = MockProvider::new("default");
        let default_count = default.call_count();
        let default_prompt = default.last_prompt();

        let router = HintRouter::new(Box::new(default));

        // "unknown" hint 未注册
        let r = router
            .invoke("hint:unknown\n这是任务内容", None)
            .await
            .unwrap();

        assert_eq!(r, "response-from-default");
        assert_eq!(default_count.load(Ordering::SeqCst), 1);

        // 验证 default 收到的是去掉 hint 前缀的 prompt
        let received = default_prompt.lock().await.clone();
        assert_eq!(received, "这是任务内容");
    }

    /// parse_hint 静态方法的单元测试
    #[test]
    fn test_parse_hint() {
        // 正常 hint
        let (hint, prompt) = HintRouter::parse_hint("hint:reasoning\n实际任务");
        assert_eq!(hint, Some("reasoning"));
        assert_eq!(prompt, "实际任务");

        // 无 hint
        let (hint, prompt) = HintRouter::parse_hint("直接提问内容");
        assert_eq!(hint, None);
        assert_eq!(prompt, "直接提问内容");

        // hint 但无换行（edge case）
        let (hint, prompt) = HintRouter::parse_hint("hint:fast");
        assert_eq!(hint, Some("fast"));
        assert_eq!(prompt, "");
    }
}
