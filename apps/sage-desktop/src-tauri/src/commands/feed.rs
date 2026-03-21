use serde_json::{json, Value};
use tauri::State;

use super::map_err;
use crate::AppState;

fn config_path() -> Result<std::path::PathBuf, String> {
    dirs::home_dir()
        .map(|h| h.join(".sage/config.toml"))
        .ok_or_else(|| "无法确定 home 目录".into())
}

/// 获取最近的 Feed 条目
#[tauri::command]
pub async fn get_feed_items(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let items = state
        .store
        .load_feed_observations(limit.unwrap_or(50))
        .map_err(map_err)?;
    // 按 URL 去重，保留最新（items 已按 created_at DESC 排序）
    let mut seen_urls = std::collections::HashSet::new();
    let mut result = Vec::new();
    for row in &items {
        // observation: "title|url" or "title"
        // raw_data: "score\ninsight"
        let (title, url) = {
            let obs = &row.observation;
            if let Some(idx) = obs.rfind('|') {
                let t = obs[..idx].trim().to_string();
                let u = obs[idx + 1..].trim().to_string();
                if u.starts_with("http") { (t, u) } else { (obs.clone(), String::new()) }
            } else {
                (obs.clone(), String::new())
            }
        };
        let (score, insight, summary, idea) = row
            .raw_data
            .as_deref()
            .map(|s| {
                let mut parts = s.splitn(4, '\n');
                let sc = parts.next().unwrap_or("3").trim().parse::<u8>().unwrap_or(3);
                let ins = parts.next().unwrap_or("").trim().to_string();
                let sum = parts.next().unwrap_or("").trim().to_string();
                let act = parts.next().unwrap_or("").trim().to_string();
                (sc, ins, sum, act)
            })
            .unwrap_or((3, String::new(), String::new(), String::new()));
        let dedup_key = if !url.is_empty() { url.clone() } else { title.clone() };
        if !dedup_key.is_empty() && !seen_urls.insert(dedup_key) {
            continue;
        }
        result.push(json!({
            "id": row.id,
            "title": title,
            "url": url,
            "score": score,
            "insight": insight,
            "summary": summary,
            "idea": idea,
            "created_at": row.created_at,
        }));
    }
    Ok(result)
}

/// 手动触发 Feed 抓取
#[tauri::command]
pub async fn trigger_feed_poll(state: State<'_, AppState>) -> Result<String, String> {
    let daemon = state.daemon.clone();
    tauri::async_runtime::spawn(async move {
        daemon.trigger_feed_poll().await;
        tracing::info!("Feed poll completed");
        let _ = sage_core::applescript::notify("Feed Intelligence", "抓取完成", "/feed").await;
    });
    Ok("Feed 抓取已启动…".into())
}

/// 获取当前 Feed 配置
#[tauri::command]
pub async fn get_feed_config() -> Result<Value, String> {
    let path = config_path()?;
    let config = sage_core::config::Config::load_or_default(&path);
    let fc = &config.channels.feed;
    Ok(json!({
        "user_interests": fc.user_interests,
        "reddit": {
            "enabled": fc.reddit.enabled,
            "subreddits": fc.reddit.subreddits,
            "poll_interval_secs": fc.reddit.poll_interval_secs,
        },
        "hackernews": {
            "enabled": fc.hackernews.enabled,
            "min_score": fc.hackernews.min_score,
            "poll_interval_secs": fc.hackernews.poll_interval_secs,
        },
        "github": {
            "enabled": fc.github.enabled,
            "trending_language": fc.github.trending_language,
            "poll_interval_secs": fc.github.poll_interval_secs,
        },
        "arxiv": {
            "enabled": fc.arxiv.enabled,
            "categories": fc.arxiv.categories,
            "keywords": fc.arxiv.keywords,
            "poll_interval_secs": fc.arxiv.poll_interval_secs,
        },
        "rss": {
            "enabled": fc.rss.enabled,
            "feeds": fc.rss.feeds,
            "poll_interval_secs": fc.rss.poll_interval_secs,
        },
    }))
}

/// 保存 Feed 配置到 config.toml
#[tauri::command]
pub async fn save_feed_config(feed_config: Value) -> Result<(), String> {
    let path = config_path()?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml::Value = content
        .parse::<toml::Value>()
        .unwrap_or(toml::Value::Table(Default::default()));
    let root = doc.as_table_mut().ok_or("TOML root is not a table")?;

    // Ensure [channels] and [channels.feed] exist
    let channels = root
        .entry("channels")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or("channels is not a table")?;
    let feed = channels
        .entry("feed")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or("feed is not a table")?;

    // user_interests
    if let Some(v) = feed_config.get("user_interests").and_then(|v| v.as_str()) {
        feed.insert("user_interests".into(), toml::Value::String(v.to_string()));
    }

    // Helper to set a source sub-table
    fn set_source(feed: &mut toml::map::Map<String, toml::Value>, key: &str, val: &Value) {
        let tbl = feed
            .entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut();
        let Some(tbl) = tbl else { return };
        if let Some(b) = val.get("enabled").and_then(|v| v.as_bool()) {
            tbl.insert("enabled".into(), toml::Value::Boolean(b));
        }
        if let Some(n) = val.get("poll_interval_secs").and_then(|v| v.as_i64()) {
            tbl.insert("poll_interval_secs".into(), toml::Value::Integer(n));
        }
        // String fields
        for field in ["trending_language"] {
            if let Some(s) = val.get(field).and_then(|v| v.as_str()) {
                tbl.insert(field.into(), toml::Value::String(s.to_string()));
            }
        }
        // Integer fields
        for field in ["min_score", "limit"] {
            if let Some(n) = val.get(field).and_then(|v| v.as_i64()) {
                tbl.insert(field.into(), toml::Value::Integer(n));
            }
        }
        // Array<String> fields
        for field in ["subreddits", "categories", "keywords", "feeds"] {
            if let Some(arr) = val.get(field).and_then(|v| v.as_array()) {
                let items: Vec<toml::Value> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| toml::Value::String(s.to_string())))
                    .collect();
                tbl.insert(field.into(), toml::Value::Array(items));
            }
        }
    }

    for key in ["reddit", "hackernews", "github", "arxiv", "rss"] {
        if let Some(val) = feed_config.get(key) {
            set_source(feed, key, val);
        }
    }

    let output = toml::to_string_pretty(&doc).map_err(|e| format!("TOML serialize failed: {e}"))?;
    std::fs::write(&path, output).map_err(|e| format!("写入配置失败: {e}"))?;
    tracing::info!("Feed config saved to {}", path.display());
    Ok(())
}

/// 生成 Feed 每日摘要（LLM 调用）
#[tauri::command]
pub async fn get_feed_digest(state: State<'_, AppState>) -> Result<String, String> {
    let lang = state.store.prompt_lang();
    let items = state
        .store
        .load_feed_observations(30)
        .map_err(map_err)?;
    if items.is_empty() {
        return Ok(if lang == "en" {
            "No feed items yet. Configure sources in settings and fetch.".into()
        } else {
            "暂无信息源条目。请在设置中配置数据源并抓取。".into()
        });
    }

    // Build items text for digest prompt (only score >= 3)
    let mut lines = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in &items {
        let obs = &row.observation;
        let title = if let Some(idx) = obs.rfind('|') {
            obs[..idx].trim()
        } else {
            obs.as_str()
        };
        if !seen.insert(title.to_string()) { continue; }
        let (score, insight) = row.raw_data.as_deref().map(|s| {
            let mut parts = s.splitn(2, '\n');
            let sc = parts.next().unwrap_or("3").trim().parse::<u8>().unwrap_or(3);
            let ins = parts.next().unwrap_or("").trim().to_string();
            (sc, ins)
        }).unwrap_or((3, String::new()));
        if score >= 3 {
            lines.push(format!("{score} | {title} | {insight}"));
        }
    }
    if lines.is_empty() {
        return Ok(if lang == "en" {
            "No high-quality items to summarize yet.".into()
        } else {
            "暂无高质量条目可供汇总。".into()
        });
    }

    // Select provider and invoke LLM
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or(if lang == "en" {
            "No AI provider available."
        } else {
            "没有可用的 AI 服务。"
        })?;
    let agent_config = super::default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let system = sage_core::prompts::feed_digest_system(&lang);
    let user = sage_core::prompts::feed_digest_user(&lang, &lines.join("\n"));
    let full_prompt = format!("{user}");

    provider
        .invoke(&full_prompt, Some(system))
        .await
        .map_err(map_err)
}
