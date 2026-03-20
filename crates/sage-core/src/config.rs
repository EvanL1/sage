use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub use crate::plugin::PluginConfig;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub memory: MemoryConfig,
    pub agent: AgentConfig,
    pub channels: ChannelsConfig,
    /// External plugin definitions (optional section in config.toml).
    #[serde(default)]
    pub plugins: Vec<PluginConfig>,
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
    #[serde(default)]
    pub feed: FeedConfig,
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

// ─── Feed Intelligence 配置 ──────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FeedConfig {
    #[serde(default)]
    pub user_interests: String,
    #[serde(default)]
    pub reddit: RedditFeedConfig,
    #[serde(default)]
    pub github: GitHubFeedConfig,
    #[serde(default)]
    pub hackernews: HackerNewsFeedConfig,
    #[serde(default)]
    pub arxiv: ArxivFeedConfig,
    #[serde(default)]
    pub rss: RssFeedConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedditFeedConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_feed_poll_3600")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub subreddits: Vec<String>,
    #[serde(default = "default_feed_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubFeedConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_feed_poll_7200")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub trending_language: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HackerNewsFeedConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_feed_poll_1800")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_hn_min_score")]
    pub min_score: u32,
    #[serde(default = "default_feed_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArxivFeedConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_feed_poll_86400")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RssFeedConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_feed_poll_3600")]
    pub poll_interval_secs: u64,
    /// List of RSS/Atom feed URLs (including RSSHub routes)
    #[serde(default)]
    pub feeds: Vec<String>,
}

impl Default for RssFeedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_feed_poll_3600(),
            feeds: Vec::new(),
        }
    }
}

fn default_feed_poll_3600() -> u64 {
    3600
}
fn default_feed_poll_7200() -> u64 {
    7200
}
fn default_feed_poll_1800() -> u64 {
    1800
}
fn default_feed_poll_86400() -> u64 {
    86400
}
fn default_hn_min_score() -> u32 {
    50
}
fn default_feed_limit() -> usize {
    10
}

impl Default for RedditFeedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_feed_poll_3600(),
            subreddits: Vec::new(),
            limit: default_feed_limit(),
        }
    }
}

impl Default for GitHubFeedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_feed_poll_7200(),
            trending_language: String::new(),
        }
    }
}

impl Default for HackerNewsFeedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_feed_poll_1800(),
            min_score: default_hn_min_score(),
            limit: default_feed_limit(),
        }
    }
}

impl Default for ArxivFeedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_feed_poll_86400(),
            categories: Vec::new(),
            keywords: Vec::new(),
        }
    }
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
            plugins: Vec::new(),
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
                feed: FeedConfig::default(),
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

    #[test]
    fn test_feed_config_defaults_all_disabled() {
        let cfg = FeedConfig::default();
        assert!(!cfg.reddit.enabled);
        assert!(!cfg.github.enabled);
        assert!(!cfg.hackernews.enabled);
        assert!(!cfg.arxiv.enabled);
        assert!(cfg.user_interests.is_empty());
    }

    #[test]
    fn test_channels_config_default_includes_feed_disabled() {
        let config = Config::default();
        assert!(!config.channels.feed.reddit.enabled);
        assert!(!config.channels.feed.github.enabled);
        assert!(!config.channels.feed.hackernews.enabled);
        assert!(!config.channels.feed.arxiv.enabled);
    }
}
