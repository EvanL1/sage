use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub use crate::plugin::PluginConfig;
pub use crate::pipeline::StageConfig;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub memory: MemoryConfig,
    pub agent: AgentConfig,
    pub channels: ChannelsConfig,
    /// External plugin definitions (optional section in config.toml).
    #[serde(default)]
    pub plugins: Vec<PluginConfig>,
    /// 认知管线配置（可选，缺失时用默认 7+2 阶段）
    #[serde(default)]
    pub pipeline: PipelineConfig,
}

// ─── Pipeline 配置 ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PipelineConfig {
    #[serde(default = "crate::pipeline::default_evening_stages")]
    pub evening: Vec<String>,
    #[serde(default = "crate::pipeline::default_weekly_stages")]
    pub weekly: Vec<String>,
    /// Per-stage 覆盖（max_iterations 等）
    #[serde(default)]
    pub stages: std::collections::HashMap<String, StageConfig>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            evening: crate::pipeline::default_evening_stages(),
            weekly: crate::pipeline::default_weekly_stages(),
            stages: std::collections::HashMap::new(),
        }
    }
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

pub use sage_llm::AgentConfig;

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
    pub user_interests: Vec<String>,
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
    pub trending_languages: Vec<String>,
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
            trending_languages: Vec::new(),
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
            pipeline: PipelineConfig::default(),
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
