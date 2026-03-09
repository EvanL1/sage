use sage_types::{ProviderConfig, ProviderInfo, ProviderKind, ProviderStatus};

use crate::store::Store;

/// CLI provider 定义：(id, display_name, binary_name, priority)
const CLI_PROVIDERS: &[(&str, &str, &str, u8)] = &[
    ("claude-cli", "Claude CLI", "claude", 0),
    ("codex-cli", "Codex CLI", "codex", 3),
    ("gemini-cli", "Gemini CLI", "gemini", 4),
];

/// API provider 定义：(id, display_name, env_var, priority)
const API_PROVIDERS: &[(&str, &str, &str, u8)] = &[
    ("anthropic-api", "Anthropic API", "ANTHROPIC_API_KEY", 1),
    ("openai-api", "OpenAI API", "OPENAI_API_KEY", 2),
    ("deepseek-api", "DeepSeek API", "DEEPSEEK_API_KEY", 5),
];

/// 扫描系统中可用的 LLM provider
pub fn discover_providers(store: &Store) -> Vec<ProviderInfo> {
    let saved = store.load_provider_configs().unwrap_or_default();
    let mut providers = Vec::new();

    // 检测 CLI provider（通过 which 命令）
    for &(id, name, binary, priority) in CLI_PROVIDERS {
        let status = if check_cli_available(binary) {
            ProviderStatus::Ready
        } else {
            ProviderStatus::NotFound
        };
        providers.push(ProviderInfo {
            id: id.into(),
            display_name: name.into(),
            kind: ProviderKind::Cli,
            status,
            priority,
        });
    }

    // 检测 API provider（通过环境变量或 Store 配置）
    for &(id, name, env_var, priority) in API_PROVIDERS {
        let has_env_key = std::env::var(env_var).is_ok();
        let has_saved_key = saved
            .iter()
            .any(|c| c.provider_id == id && c.api_key.is_some());
        let status = if has_env_key || has_saved_key {
            ProviderStatus::Ready
        } else {
            ProviderStatus::NeedsApiKey
        };
        providers.push(ProviderInfo {
            id: id.into(),
            display_name: name.into(),
            kind: ProviderKind::HttpApi,
            status,
            priority,
        });
    }

    providers.sort_by_key(|p| p.priority);
    providers
}

/// 选择最佳可用 provider，返回 (info, config) 对
pub fn select_best_provider(
    discovered: &[ProviderInfo],
    saved: &[ProviderConfig],
) -> Option<(ProviderInfo, ProviderConfig)> {
    for info in discovered {
        if info.status != ProviderStatus::Ready {
            continue;
        }
        // 找到匹配的 saved config，或生成默认 config
        let config = saved
            .iter()
            .find(|c| c.provider_id == info.id)
            .cloned()
            .unwrap_or(ProviderConfig {
                provider_id: info.id.clone(),
                api_key: None,
                model: None,
                base_url: None,
                enabled: true,
            });
        if config.enabled {
            return Some((info.clone(), config));
        }
    }
    None
}

/// 用 `which` 检测 CLI 是否存在
fn check_cli_available(binary: &str) -> bool {
    std::process::Command::new("which")
        .arg(binary)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_returns_all_providers() {
        let store = Store::open_in_memory().unwrap();
        let providers = discover_providers(&store);
        // 应该返回 6 个 provider（3 CLI + 3 API）
        assert_eq!(providers.len(), 6);
    }

    #[test]
    fn test_discover_sorted_by_priority() {
        let store = Store::open_in_memory().unwrap();
        let providers = discover_providers(&store);
        for i in 1..providers.len() {
            assert!(providers[i].priority >= providers[i - 1].priority);
        }
    }

    #[test]
    fn test_select_best_skips_not_ready() {
        let discovered = vec![
            ProviderInfo {
                id: "missing-cli".into(),
                display_name: "Missing".into(),
                kind: ProviderKind::Cli,
                status: ProviderStatus::NotFound,
                priority: 0,
            },
            ProviderInfo {
                id: "needs-key".into(),
                display_name: "Needs Key".into(),
                kind: ProviderKind::HttpApi,
                status: ProviderStatus::NeedsApiKey,
                priority: 1,
            },
        ];
        assert!(select_best_provider(&discovered, &[]).is_none());
    }

    #[test]
    fn test_select_best_returns_first_ready() {
        let discovered = vec![
            ProviderInfo {
                id: "not-found".into(),
                display_name: "Not Found".into(),
                kind: ProviderKind::Cli,
                status: ProviderStatus::NotFound,
                priority: 0,
            },
            ProviderInfo {
                id: "anthropic-api".into(),
                display_name: "Anthropic".into(),
                kind: ProviderKind::HttpApi,
                status: ProviderStatus::Ready,
                priority: 1,
            },
        ];
        let saved = vec![ProviderConfig {
            provider_id: "anthropic-api".into(),
            api_key: Some("sk-test".into()),
            model: Some("claude-sonnet-4-20250514".into()),
            base_url: None,
            enabled: true,
        }];
        let result = select_best_provider(&discovered, &saved);
        assert!(result.is_some());
        let (info, config) = result.unwrap();
        assert_eq!(info.id, "anthropic-api");
        assert_eq!(config.api_key, Some("sk-test".into()));
    }

    #[test]
    fn test_select_best_skips_disabled() {
        let discovered = vec![ProviderInfo {
            id: "anthropic-api".into(),
            display_name: "Anthropic".into(),
            kind: ProviderKind::HttpApi,
            status: ProviderStatus::Ready,
            priority: 1,
        }];
        let saved = vec![ProviderConfig {
            provider_id: "anthropic-api".into(),
            api_key: Some("sk-test".into()),
            model: None,
            base_url: None,
            enabled: false,
        }];
        assert!(select_best_provider(&discovered, &saved).is_none());
    }
}
