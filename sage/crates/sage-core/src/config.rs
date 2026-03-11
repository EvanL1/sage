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
pub struct DaemonConfig {
    pub heartbeat_interval_secs: u64,
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryConfig {
    pub base_dir: String,
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
    /// 单个 Agent 实例最多调用 LLM 的次数（护栏，防止无限循环）
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
}

fn default_max_iterations() -> usize {
    10
}

fn default_provider() -> String {
    "claude".into()
}

#[derive(Debug, Deserialize)]
pub struct ChannelsConfig {
    pub email: PollChannelConfig,
    pub calendar: PollChannelConfig,
    pub wechat: WechatConfig,
    pub hooks: HooksConfig,
    pub notification: ToggleConfig,
}

#[derive(Debug, Deserialize)]
pub struct PollChannelConfig {
    pub enabled: bool,
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
    /// 日历来源："outlook"（默认）、"apple"、"both"
    #[serde(default = "default_calendar_source")]
    pub source: String,
}

fn default_calendar_source() -> String {
    "outlook".into()
}

#[derive(Debug, Deserialize)]
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
pub struct ToggleConfig {
    pub enabled: bool,
}

fn default_poll() -> u64 {
    300
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_secs: 300,
            log_level: "info".into(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            base_dir: "~/.sage/memory".into(),
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: "claude".into(),
            claude_binary: "claude".into(),
            codex_binary: String::new(),
            gemini_binary: String::new(),
            default_model: "sonnet".into(),
            project_dir: "~".into(),
            max_budget_usd: 0.50,
            permission_mode: "bypassPermissions".into(),
            max_iterations: 10,
        }
    }
}

impl Default for PollChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: 300,
            source: default_calendar_source(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            daemon: DaemonConfig::default(),
            memory: MemoryConfig::default(),
            agent: AgentConfig::default(),
            channels: ChannelsConfig {
                email: PollChannelConfig {
                    enabled: false,
                    poll_interval_secs: 300,
                    source: default_calendar_source(),
                },
                calendar: PollChannelConfig {
                    enabled: false,
                    poll_interval_secs: 900,
                    source: default_calendar_source(),
                },
                wechat: WechatConfig {
                    enabled: false,
                    events_file: "/tmp/sage-wechat-events.jsonl".into(),
                },
                hooks: HooksConfig {
                    enabled: false,
                    watch_dir: "~/.claude".into(),
                },
                notification: ToggleConfig { enabled: true },
            },
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// 加载配置，文件不存在时返回默认值
    pub fn load_or_default(path: &Path) -> Self {
        if path.exists() {
            Self::load(path).unwrap_or_default()
        } else {
            Self::default()
        }
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
        std::env::set_var("HOME", "/Users/testuser");
        let result = Config::expand_path("~/.sage");
        assert_eq!(result, PathBuf::from("/Users/testuser/.sage"));
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
}
