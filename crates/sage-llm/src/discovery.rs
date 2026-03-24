use std::path::{Path, PathBuf};

use sage_types::{ProviderConfig, ProviderInfo, ProviderKind, ProviderStatus};
use serde_json::Value;

use sage_store::Store;

/// CLI provider 定义：(id, display_name, binary_name, priority)
const CLI_PROVIDERS: &[(&str, &str, &str, u8)] = &[
    ("claude-cli", "Claude CLI", "claude", 0),
    ("codex-cli", "Codex CLI", "codex", 3),
    ("gemini-cli", "Gemini CLI", "gemini", 4),
    ("cursor-cli", "Cursor CLI", "agent", 6),
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
    for &(id, name, binary, default_priority) in CLI_PROVIDERS {
        let status = detect_cli_status(id, binary);
        // 用户自定义优先级覆盖默认值
        let priority = saved
            .iter()
            .find(|c| c.provider_id == id)
            .and_then(|c| c.priority)
            .unwrap_or(default_priority);
        providers.push(ProviderInfo {
            id: id.into(),
            display_name: name.into(),
            kind: ProviderKind::Cli,
            status,
            priority,
        });
    }

    // 检测 API provider（通过环境变量或 Store 配置）
    for &(id, name, env_var, default_priority) in API_PROVIDERS {
        let has_env_key = std::env::var(env_var).is_ok();
        let has_saved_key = saved
            .iter()
            .any(|c| c.provider_id == id && c.api_key.is_some());
        let status = if has_env_key || has_saved_key {
            ProviderStatus::Ready
        } else {
            ProviderStatus::NeedsApiKey
        };
        let priority = saved
            .iter()
            .find(|c| c.provider_id == id)
            .and_then(|c| c.priority)
            .unwrap_or(default_priority);
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

fn detect_cli_status(id: &str, binary: &str) -> ProviderStatus {
    let Some(binary_path) = resolve_cli_binary(binary) else {
        return ProviderStatus::NotFound;
    };

    let authenticated = match id {
        "claude-cli" => check_claude_auth(&binary_path),
        "codex-cli" => check_codex_auth(&binary_path),
        "gemini-cli" => check_gemini_auth(),
        _ => true,
    };

    if authenticated {
        ProviderStatus::Ready
    } else {
        ProviderStatus::NeedsLogin
    }
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
                priority: None,
            });
        if config.enabled {
            return Some((info.clone(), config));
        }
    }
    None
}

fn resolve_cli_binary(binary: &str) -> Option<PathBuf> {
    let candidates = [
        format!("/opt/homebrew/bin/{binary}"),
        format!("/usr/local/bin/{binary}"),
        format!("/usr/bin/{binary}"),
    ];
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    // fallback: try which (works in terminal context)
    std::process::Command::new("which")
        .arg(binary)
        .current_dir("/tmp")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let resolved = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if resolved.is_empty() {
                None
            } else {
                Some(PathBuf::from(resolved))
            }
        })
}

fn run_cli_probe(binary_path: &Path, args: &[&str]) -> Option<std::process::Output> {
    std::process::Command::new(binary_path)
        .args(args)
        .current_dir("/tmp")
        .output()
        .ok()
}

fn check_claude_auth(binary_path: &Path) -> bool {
    let Some(output) = run_cli_probe(binary_path, &["auth", "status"]) else {
        return false;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Ok(parsed) = serde_json::from_str::<Value>(&stdout) {
        return parsed
            .get("loggedIn")
            .and_then(Value::as_bool)
            .unwrap_or(output.status.success());
    }

    output.status.success()
}

fn check_codex_auth(binary_path: &Path) -> bool {
    let Some(output) = run_cli_probe(binary_path, &["login", "status"]) else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    // codex 将状态信息输出到 stderr 而非 stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("Logged in") || stderr.contains("Logged in")
}

fn check_gemini_auth() -> bool {
    if std::env::var("GEMINI_API_KEY").is_ok() || std::env::var("GOOGLE_API_KEY").is_ok() {
        return true;
    }

    let use_vertex = std::env::var("GOOGLE_GENAI_USE_VERTEXAI")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false);
    if use_vertex
        && std::env::var("GOOGLE_CLOUD_PROJECT").is_ok()
        && std::env::var("GOOGLE_CLOUD_LOCATION").is_ok()
    {
        return true;
    }

    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let base_dir = PathBuf::from(home).join(".gemini");
    let settings_path = base_dir.join("settings.json");
    let selected = std::fs::read_to_string(&settings_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|json| {
            json.pointer("/security/auth/selectedType")
                .and_then(Value::as_str)
                .or_else(|| json.get("selectedAuthType").and_then(Value::as_str))
                .map(str::to_string)
        });

    match selected.as_deref() {
        Some(selected_type) if selected_type.starts_with("oauth") => {
            base_dir.join("google_accounts.json").exists()
        }
        Some(selected_type) if !selected_type.is_empty() => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_returns_all_providers() {
        let store = Store::open_in_memory().unwrap();
        let providers = discover_providers(&store);
        // 应该返回 7 个 provider（4 CLI + 3 API）
        assert_eq!(providers.len(), 7);
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
            ProviderInfo {
                id: "needs-login".into(),
                display_name: "Needs Login".into(),
                kind: ProviderKind::Cli,
                status: ProviderStatus::NeedsLogin,
                priority: 2,
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
            priority: None,
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
            priority: None,
        }];
        assert!(select_best_provider(&discovered, &saved).is_none());
    }
}
