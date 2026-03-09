use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub memory: MemoryConfig,
    pub agent: AgentConfig,
    pub channels: ChannelsConfig,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DaemonConfig {
    pub heartbeat_interval_secs: u64,
    pub log_level: String,
    pub pid_file: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MemoryConfig {
    pub base_dir: String,
    pub heartbeat_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub claude_binary: String,
    #[serde(default)]
    pub codex_binary: String,
    #[serde(default)]
    pub gemini_binary: String,
    pub default_model: String,
    pub project_dir: String,
    pub max_budget_usd: f64,
    pub permission_mode: String,
}

fn default_provider() -> String {
    "claude".into()
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChannelsConfig {
    pub email: PollChannelConfig,
    pub calendar: PollChannelConfig,
    pub wechat: WechatConfig,
    pub hooks: HooksConfig,
    pub notification: ToggleConfig,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PollChannelConfig {
    pub enabled: bool,
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct WechatConfig {
    pub enabled: bool,
    pub events_file: String,
}

#[derive(Debug, Deserialize)]
pub struct HooksConfig {
    pub enabled: bool,
    pub watch_dir: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ToggleConfig {
    pub enabled: bool,
}

fn default_poll() -> u64 {
    300
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn expand_path(path: &str) -> PathBuf {
        if path.starts_with('~') {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(path.replacen('~', &home, 1))
        } else {
            PathBuf::from(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_path_tilde_expands_to_home() {
        // 设置可预测的 HOME 环境变量后验证展开结果
        std::env::set_var("HOME", "/Users/testuser");
        let result = Config::expand_path("~/.sage");
        assert_eq!(result, PathBuf::from("/Users/testuser/.sage"));
    }

    #[test]
    fn test_expand_path_tilde_only() {
        std::env::set_var("HOME", "/Users/testuser");
        let result = Config::expand_path("~");
        assert_eq!(result, PathBuf::from("/Users/testuser"));
    }

    #[test]
    fn test_expand_path_absolute_path_unchanged() {
        let result = Config::expand_path("/etc/sage/config.toml");
        assert_eq!(result, PathBuf::from("/etc/sage/config.toml"));
    }

    #[test]
    fn test_expand_path_relative_path_unchanged() {
        let result = Config::expand_path("relative/path/config.toml");
        assert_eq!(result, PathBuf::from("relative/path/config.toml"));
    }

    #[test]
    fn test_expand_path_tilde_replaced_only_once() {
        // replacen(..., 1) 确保只替换第一个 ~
        std::env::set_var("HOME", "/home/evan");
        let result = Config::expand_path("~/projects/~/test");
        assert_eq!(result, PathBuf::from("/home/evan/projects/~/test"));
    }
}
