use serde_json::{json, Value};
use tauri::State;

use super::map_err;
use crate::AppState;

/// 从 LLM 返回的 sage-memory 块中提取记忆内容文本
fn parse_memory_items(raw: &str) -> Vec<String> {
    let marker = "```sage-memory";
    let Some(start) = raw.find(marker) else { return vec![] };
    let json_start = start + marker.len();
    let Some(end) = raw[json_start..].find("```") else { return vec![] };
    let json_str = raw[json_start..json_start + end].trim();
    let items: Vec<Value> = serde_json::from_str(json_str).unwrap_or_default();
    items.iter()
        .filter_map(|it| it["content"].as_str().map(String::from))
        .filter(|s| !s.is_empty())
        .collect()
}

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
    let actions = state.store.get_feed_actions().unwrap_or_default();
    // 按 URL 去重，保留最新（items 已按 created_at DESC 排序）
    let mut seen_urls = std::collections::HashSet::new();
    let mut result = Vec::new();
    for row in &items {
        // observation: "title|url" or "title"
        // raw_data: "url\nscore\ninsight" (after deep_read: "url\nscore\ninsight\nsummary\nidea")
        let (title, obs_url) = {
            let obs = &row.observation;
            if let Some(idx) = obs.rfind('|') {
                let t = obs[..idx].trim().to_string();
                let u = obs[idx + 1..].trim().to_string();
                if u.starts_with("http") { (t, u) } else { (obs.clone(), String::new()) }
            } else {
                (obs.clone(), String::new())
            }
        };
        let (url, score, insight, summary, idea) = row
            .raw_data
            .as_deref()
            .map(|s| {
                let mut parts = s.splitn(5, '\n');
                let raw_url = parts.next().unwrap_or("").trim().to_string();
                let sc = parts.next().unwrap_or("3").trim().parse::<u8>().unwrap_or(3);
                let ins = parts.next().unwrap_or("").trim().to_string();
                let sum = parts.next().unwrap_or("").trim().to_string();
                let act = parts.next().unwrap_or("").trim().to_string();
                (raw_url, sc, ins, sum, act)
            })
            .unwrap_or_default();
        // URL: 优先从 raw_data 取，fallback 到 observation 解析的
        let url = if !url.is_empty() && url.starts_with("http") { url } else { obs_url };
        let dedup_key = if !url.is_empty() { url.clone() } else { title.clone() };
        if !dedup_key.is_empty() && !seen_urls.insert(dedup_key) {
            continue;
        }
        let (action, category) = actions.get(&row.id)
            .map(|(a, c)| (a.as_str(), c.as_deref()))
            .unwrap_or(("", None));
        result.push(json!({
            "id": row.id,
            "title": title,
            "url": url,
            "score": score,
            "insight": insight,
            "summary": summary,
            "idea": idea,
            "created_at": row.created_at,
            "action": action,
            "category": category,
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

/// 获取今日 Feed 简报（优先读缓存，无缓存返回空）
#[tauri::command]
pub async fn get_feed_digest(state: State<'_, AppState>) -> Result<String, String> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    match state.store.get_feed_digest_for_date(&today).map_err(map_err)? {
        Some(content) => Ok(content),
        None => Ok(String::new()),
    }
}

/// 强制重新生成 Feed 简报（LLM 调用 + 更新缓存）
#[tauri::command]
pub async fn regenerate_feed_digest(state: State<'_, AppState>) -> Result<String, String> {
    let lang = state.store.prompt_lang();
    let items = state.store.load_feed_observations(30).map_err(map_err)?;
    if items.is_empty() {
        return Ok(if lang == "en" {
            "No feed items yet. Configure sources in settings and fetch.".into()
        } else {
            "暂无信息源条目。请在设置中配置数据源并抓取。".into()
        });
    }

    let archived_ids = state.store.get_archived_feed_ids().unwrap_or_default();
    let mut lines = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in &items {
        if archived_ids.contains(&row.id) { continue; }
        let obs = &row.observation;
        let title = if let Some(idx) = obs.rfind('|') {
            obs[..idx].trim()
        } else {
            obs.as_str()
        };
        if !seen.insert(title.to_string()) { continue; }
        // raw_data format: "url\nscore\ninsight\n..."
        let (score, insight) = row.raw_data.as_deref().map(|s| {
            let mut parts = s.splitn(3, '\n');
            let _url = parts.next().unwrap_or("");
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

    let content = provider.invoke(&user, Some(system)).await.map_err(map_err)?;

    // 写入缓存
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let _ = state.store.save_feed_digest(&today, &content);

    Ok(content)
}

/// 归档 feed 条目
#[tauri::command]
pub async fn archive_feed_item(
    state: State<'_, AppState>,
    observation_id: i64,
    category: Option<String>,
) -> Result<(), String> {
    state.store.archive_feed_item(observation_id, category.as_deref()).map_err(map_err)
}

/// 取消归档
#[tauri::command]
pub async fn unarchive_feed_item(
    state: State<'_, AppState>,
    observation_id: i64,
) -> Result<(), String> {
    state.store.unarchive_feed_item(observation_id).map_err(map_err)
}

/// 深入学习 feed 条目：抓取 URL → LLM 提取记忆 → 存入 memories
#[tauri::command]
pub async fn deep_learn_feed_item(
    state: State<'_, AppState>,
    observation_id: i64,
    url: String,
    title: String,
) -> Result<String, String> {
    // 标记为学习中
    state.store.mark_feed_learning(observation_id).map_err(map_err)?;

    let lang = state.store.prompt_lang();
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("没有可用的 AI 服务")?;
    let agent_config = super::default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    // 抓取内容
    let client = reqwest::Client::builder()
        .user_agent("Sage/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    // GitHub 仓库用 README
    let fetch_url = if url.starts_with("https://github.com/") {
        let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
        if parts.len() == 5 {
            format!("https://raw.githubusercontent.com/{}/{}/HEAD/README.md", parts[3], parts[4])
        } else {
            url.clone()
        }
    } else {
        url.clone()
    };

    let text = client.get(&fetch_url).send().await
        .map_err(|e| format!("抓取失败: {e}"))?
        .text().await
        .map_err(|e| format!("读取内容失败: {e}"))?;

    let content: String = text.chars().take(5000).collect();
    if content.len() < 50 {
        return Err("内容太短，无法学习".into());
    }

    // 用 memory extraction prompt 提取记忆
    let existing = state.store.load_memories().unwrap_or_default();
    let existing_text = if existing.is_empty() {
        "（暂无）".to_string()
    } else {
        existing.iter().take(30)
            .map(|m| format!("[{}] {} (置信度: {:.1})", m.category, m.content, m.confidence))
            .collect::<Vec<_>>().join("\n")
    };
    let conversation = format!(
        "User: 我想深入了解这个项目/文章：{title}\n\n以下是内容：\n{content}"
    );
    let system = sage_core::prompts::cmd_extract_memories_system(&lang);
    let prompt = sage_core::prompts::cmd_extract_memories_user(&lang, &existing_text, &conversation);

    let resp = provider.invoke(&prompt, Some(system)).await.map_err(map_err)?;

    // 从 LLM 返回中提取记忆内容（用于展示）
    let learned_items = parse_memory_items(&resp);

    let store_arc = std::sync::Arc::clone(&state.store);
    let (_, count) = super::extract_and_save_memories(&resp, &store_arc).await;

    // 标记为已学习并归档
    let _ = state.store.mark_feed_learned(observation_id);

    // 返回 JSON：count + 具体学到的内容
    Ok(serde_json::to_string(&json!({
        "count": count,
        "title": title,
        "items": learned_items,
    })).unwrap_or_default())
}
