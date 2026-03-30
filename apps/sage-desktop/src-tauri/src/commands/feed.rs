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

    // 构建 URL→action 映射：同一 URL 的任意 observation 有 action 都算
    let mut url_actions: std::collections::HashMap<String, (String, Option<String>)> =
        std::collections::HashMap::new();
    for row in &items {
        if let Some((action, cat)) = actions.get(&row.id) {
            let url = extract_url(row);
            if !url.is_empty() {
                url_actions.entry(url).or_insert_with(|| (action.clone(), cat.clone()));
            }
        }
    }

    // 按 URL 去重，保留最新（items 已按 created_at DESC 排序）
    let mut seen_urls = std::collections::HashSet::new();
    let mut result = Vec::new();
    for row in &items {
        let (title, url, score, insight, summary, idea) = parse_feed_row(row);
        let dedup_key = if !url.is_empty() { url.clone() } else { title.clone() };
        if !dedup_key.is_empty() && !seen_urls.insert(dedup_key) {
            continue;
        }
        // action：先查当前 id，再查 URL 映射（继承旧 observation 的状态）
        let (action, category) = actions.get(&row.id)
            .map(|(a, c)| (a.as_str(), c.as_deref()))
            .or_else(|| url_actions.get(&url).map(|(a, c)| (a.as_str(), c.as_deref())))
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

/// 即时搜索：按 topic 搜索 HN + Reddit → LLM 打分 → deep read → 存入 feed
#[tauri::command]
pub async fn search_feed_topic(
    state: State<'_, AppState>,
    query: String,
) -> Result<usize, String> {
    use sage_core::channels::feed::search_topic;

    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("没有可用的 AI 服务")?;
    let agent_config = super::default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);
    let agent = sage_core::agent::Agent::with_provider(provider);

    search_topic(&query, &agent, &state.store).await.map_err(map_err)
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

/// 从 observation 行解析 URL
fn extract_url(row: &sage_core::store::ObservationRow) -> String {
    let (_, url, ..) = parse_feed_row(row);
    url
}

/// 解析 feed observation 行为结构化字段
fn parse_feed_row(row: &sage_core::store::ObservationRow) -> (String, String, u8, String, String, String) {
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
    let (raw_url, score, insight, summary, idea) = row
        .raw_data
        .as_deref()
        .map(|s| {
            let mut parts = s.splitn(5, '\n');
            let u = parts.next().unwrap_or("").trim().to_string();
            let sc = parts.next().unwrap_or("3").trim().parse::<u8>().unwrap_or(3);
            let ins = parts.next().unwrap_or("").trim().to_string();
            let sum = parts.next().unwrap_or("").trim().to_string();
            let act = parts.next().unwrap_or("").trim().to_string();
            (u, sc, ins, sum, act)
        })
        .unwrap_or_default();
    let url = if !raw_url.is_empty() && raw_url.starts_with("http") { raw_url } else { obs_url };
    (title, url, score, insight, summary, idea)
}

/// 从记忆中总结用户兴趣
#[tauri::command]
pub async fn summarize_user_interests(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let memories = state.store.load_memories().map_err(map_err)?;
    if memories.is_empty() {
        return Ok(Vec::new());
    }
    let mut sorted = memories;
    sorted.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<String> = sorted.iter()
        .take(30)
        .map(|m| format!("[{}] {}", m.category, m.content))
        .collect();

    let lang = state.store.prompt_lang();
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let Some((info, config)) = sage_core::discovery::select_best_provider(&discovered, &configs) else {
        let cats: std::collections::BTreeSet<String> = sorted.iter().map(|m| m.category.clone()).collect();
        return Ok(cats.into_iter().collect());
    };
    let agent_config = super::default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);
    let system = match lang.as_str() {
        "en" => "Extract the user's interest KEYWORDS from their memories. These keywords will be used as search queries to find relevant news/articles. Output one keyword or short phrase per line (max 10, each ≤4 words). Examples: Rust, distributed systems, LLM agents, embedded systems. Do NOT output full sentences or descriptions. Output ONLY the keyword list.",
        _ => "从用户记忆中提取兴趣关键词。这些关键词将用于搜索相关新闻/文章。每行一个关键词或短语（最多10个，每个不超过4个词）。示例：Rust、分布式系统、LLM agent、嵌入式开发。不要输出完整句子或描述。只输出关键词列表。",
    };
    let prompt = format!("用户记忆：\n{}", top.join("\n"));
    let resp = provider.invoke(&prompt, Some(system)).await.unwrap_or_default();
    let items: Vec<String> = resp.lines()
        .map(|l| l.trim().trim_start_matches(|c: char| c == '-' || c == '•' || c.is_ascii_digit() || c == '.').trim().to_string())
        .filter(|s| !s.is_empty() && s.len() > 2)
        .take(10)
        .collect();
    if items.is_empty() {
        let cats: std::collections::BTreeSet<String> = sorted.iter().map(|m| m.category.clone()).collect();
        return Ok(cats.into_iter().collect());
    }
    Ok(items)
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
            "trending_languages": fc.github.trending_languages,
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

/// 自然语言更新 Feed 配置：LLM 解析意图 → 更新 config
#[tauri::command]
pub async fn update_feed_natural(
    state: State<'_, AppState>,
    text: String,
) -> Result<Value, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("内容不能为空".into());
    }

    // 读取当前配置
    let current = invoke_get_feed_config()?;

    // 构造 prompt
    let prompt = format!(
        r#"你是 Feed 配置助手。用户用自然语言描述了他想修改的 Feed 订阅源配置。

当前配置（JSON）：
{current}

用户说："{text}"

请理解用户意图，输出修改后的**完整** JSON 配置。规则：
1. 保留用户没提到的已有配置不变
2. 新增话题时：加到 user_interests（中英文关键词）、合适的 subreddits、arxiv.keywords
3. 删除话题时：从对应字段中移除
4. 只输出 JSON，不要解释

输出格式（必须是合法 JSON）：
{{
  "user_interests": [...],
  "reddit": {{ "enabled": true, "subreddits": [...], "poll_interval_secs": 3600 }},
  "hackernews": {{ "enabled": true, "min_score": 50, "poll_interval_secs": 1800 }},
  "github": {{ "enabled": true, "trending_languages": [...], "poll_interval_secs": 7200 }},
  "arxiv": {{ "enabled": true, "categories": [...], "keywords": [...], "poll_interval_secs": 86400 }},
  "rss": {{ "enabled": false, "feeds": [...], "poll_interval_secs": 3600 }}
}}"#
    );

    // 调用 LLM
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("没有可用的 AI 服务")?;
    let agent_config = super::default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let raw = provider.invoke(&prompt, None).await.map_err(map_err)?;

    // 提取 JSON
    let json_str = raw
        .find('{')
        .and_then(|start| raw.rfind('}').map(|end| &raw[start..=end]))
        .ok_or("LLM 未返回有效 JSON")?;
    let new_config: Value = serde_json::from_str(json_str).map_err(map_err)?;

    // 写入 config.toml（复用已有逻辑）
    save_feed_config_inner(&new_config)?;

    Ok(new_config)
}

/// 读取当前 Feed 配置为 JSON 字符串（内部用）
fn invoke_get_feed_config() -> Result<String, String> {
    let path = config_path()?;
    let config = sage_core::config::Config::load_or_default(&path);
    let fc = &config.channels.feed;
    let val = json!({
        "user_interests": fc.user_interests,
        "reddit": { "enabled": fc.reddit.enabled, "subreddits": fc.reddit.subreddits, "poll_interval_secs": fc.reddit.poll_interval_secs },
        "hackernews": { "enabled": fc.hackernews.enabled, "min_score": fc.hackernews.min_score, "poll_interval_secs": fc.hackernews.poll_interval_secs },
        "github": { "enabled": fc.github.enabled, "trending_languages": fc.github.trending_languages, "poll_interval_secs": fc.github.poll_interval_secs },
        "arxiv": { "enabled": fc.arxiv.enabled, "categories": fc.arxiv.categories, "keywords": fc.arxiv.keywords, "poll_interval_secs": fc.arxiv.poll_interval_secs },
        "rss": { "enabled": fc.rss.enabled, "feeds": fc.rss.feeds, "poll_interval_secs": fc.rss.poll_interval_secs },
    });
    serde_json::to_string_pretty(&val).map_err(map_err)
}

/// 保存 Feed 配置到 config.toml（内部逻辑，供多处复用）
fn save_feed_config_inner(feed_config: &Value) -> Result<(), String> {
    let path = config_path()?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml::Value = content
        .parse::<toml::Value>()
        .unwrap_or(toml::Value::Table(Default::default()));
    let root = doc.as_table_mut().ok_or("TOML root is not a table")?;
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
    if let Some(arr) = feed_config.get("user_interests").and_then(|v| v.as_array()) {
        let items: Vec<toml::Value> = arr.iter()
            .filter_map(|v| v.as_str().map(|s| toml::Value::String(s.to_string())))
            .collect();
        feed.insert("user_interests".into(), toml::Value::Array(items));
    }
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
        for field in ["min_score", "limit"] {
            if let Some(n) = val.get(field).and_then(|v| v.as_i64()) {
                tbl.insert(field.into(), toml::Value::Integer(n));
            }
        }
        for field in ["subreddits", "trending_languages", "categories", "keywords", "feeds"] {
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

/// 保存 Feed 配置到 config.toml
#[tauri::command]
pub async fn save_feed_config(feed_config: Value) -> Result<(), String> {
    save_feed_config_inner(&feed_config)
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

/// 深入学习 feed 条目：全方位研究 → LLM 提取记忆 → 存入 memories
#[tauri::command]
pub async fn deep_learn_feed_item(
    state: State<'_, AppState>,
    observation_id: i64,
    url: String,
    title: String,
) -> Result<String, String> {
    state.store.mark_feed_learning(observation_id).map_err(map_err)?;

    let lang = state.store.prompt_lang();
    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or("没有可用的 AI 服务")?;
    let agent_config = super::default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let client = build_http_client();

    // 根据 URL 类型采集不同深度的内容
    let content = if is_github_repo(&url) {
        gather_github_repo(&client, &url).await
    } else {
        gather_web_content(&client, &url, &title).await
    };

    if content.len() < 50 {
        let _ = state.store.unarchive_feed_item(observation_id);
        return Err("内容太少，无法深入学习".into());
    }

    // LLM 提取记忆
    let existing = state.store.load_memories().unwrap_or_default();
    let existing_text = if existing.is_empty() {
        "（暂无）".to_string()
    } else {
        existing.iter().take(30)
            .map(|m| format!("[{}] {} (置信度: {:.1})", m.category, m.content, m.confidence))
            .collect::<Vec<_>>().join("\n")
    };
    let conversation = format!(
        "User: 我想深入了解：{title}\n\n以下是采集到的详细资料：\n{content}"
    );
    let system = sage_core::prompts::cmd_extract_memories_system(&lang);
    let prompt = sage_core::prompts::cmd_extract_memories_user(&lang, &existing_text, &conversation);

    let resp = provider.invoke(&prompt, Some(system)).await.map_err(map_err)?;
    let learned_items = parse_memory_items(&resp);

    let store_arc = std::sync::Arc::clone(&state.store);
    let (_, count) = super::extract_and_save_memories(&resp, &store_arc).await;
    let _ = state.store.mark_feed_learned(observation_id);

    // 生成阅读笔记并存文件
    let note = generate_and_save_note(&provider, &lang, observation_id, &title, &content).await;

    Ok(serde_json::to_string(&json!({
        "count": count,
        "title": title,
        "items": learned_items,
        "note": note.unwrap_or_default(),
    })).unwrap_or_default())
}

/// 读取已保存的阅读笔记
#[tauri::command]
pub async fn get_feed_note(observation_id: i64) -> Result<String, String> {
    let path = notes_path(observation_id);
    match std::fs::read_to_string(&path) {
        Ok(s) => Ok(s),
        Err(_) => Ok(String::new()),
    }
}

// ─── Deep Learn Helpers ─────────────────────────────────

/// 笔记文件路径：~/.sage/data/notes/{id}.md
fn notes_path(observation_id: i64) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(format!(".sage/data/notes/{observation_id}.md"))
}

/// 调用 LLM 生成阅读笔记并写入文件
async fn generate_and_save_note(
    provider: &Box<dyn sage_core::provider::LlmProvider>,
    lang: &str,
    observation_id: i64,
    title: &str,
    content: &str,
) -> Option<String> {
    let prompt = sage_core::prompts::feed_deep_note_prompt(lang, title, content);
    let note = provider.invoke(&prompt, None).await.ok()?;
    let path = notes_path(observation_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &note);
    Some(note)
}

fn build_http_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .user_agent("Sage/1.0 (Personal AI Advisor)")
        .timeout(std::time::Duration::from_secs(15));
    // 中国网络代理
    for &(key, default_val) in &[
        ("http_proxy", "http://127.0.0.1:7890"),
        ("https_proxy", "http://127.0.0.1:7890"),
    ] {
        if let Ok(val) = std::env::var(key) {
            if let Ok(proxy) = reqwest::Proxy::all(&val) {
                builder = builder.proxy(proxy);
                break;
            }
        } else if let Ok(proxy) = reqwest::Proxy::all(default_val) {
            builder = builder.proxy(proxy);
            break;
        }
    }
    builder.build().unwrap_or_default()
}

fn is_github_repo(url: &str) -> bool {
    if !url.starts_with("https://github.com/") { return false; }
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    parts.len() == 5 // github.com / owner / repo
}

/// GitHub 仓库深度采集：元信息 + 文件树 + README + 关键源码
async fn gather_github_repo(client: &reqwest::Client, url: &str) -> String {
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    let (owner, repo) = (parts[3], parts[4]);
    let mut sections = Vec::new();

    // 1. Repo 元信息（description, stars, language, topics）
    if let Ok(resp) = client
        .get(format!("https://api.github.com/repos/{owner}/{repo}"))
        .header("Accept", "application/vnd.github.v3+json")
        .send().await
    {
        if let Ok(info) = resp.json::<serde_json::Value>().await {
            let desc = info["description"].as_str().unwrap_or("");
            let stars = info["stargazers_count"].as_u64().unwrap_or(0);
            let lang = info["language"].as_str().unwrap_or("unknown");
            let topics: Vec<&str> = info["topics"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            sections.push(format!(
                "## 项目概览\n- 名称: {owner}/{repo}\n- 描述: {desc}\n- Stars: {stars}\n- 主语言: {lang}\n- 标签: {}",
                topics.join(", ")
            ));
        }
    }

    // 2. 文件树结构（仅展示前 80 个条目，截断过深路径）
    if let Ok(resp) = client
        .get(format!("https://api.github.com/repos/{owner}/{repo}/git/trees/HEAD?recursive=1"))
        .header("Accept", "application/vnd.github.v3+json")
        .send().await
    {
        if let Ok(tree) = resp.json::<serde_json::Value>().await {
            if let Some(items) = tree["tree"].as_array() {
                let paths: Vec<&str> = items.iter()
                    .filter_map(|it| it["path"].as_str())
                    .filter(|p| p.matches('/').count() < 4) // 不超过 3 层深
                    .take(80)
                    .collect();
                sections.push(format!("## 文件结构（{}个文件）\n```\n{}\n```", items.len(), paths.join("\n")));
            }
        }
    }

    // 3. README
    if let Ok(resp) = client
        .get(format!("https://raw.githubusercontent.com/{owner}/{repo}/HEAD/README.md"))
        .send().await
    {
        if let Ok(text) = resp.text().await {
            let readme: String = text.chars().take(3000).collect();
            if readme.len() > 50 {
                sections.push(format!("## README\n{readme}"));
            }
        }
    }

    // 4. 关键源码文件（Cargo.toml / package.json / main 入口）
    let key_files = [
        "Cargo.toml", "package.json", "pyproject.toml", "go.mod",
        "src/main.rs", "src/lib.rs", "src/index.ts", "src/index.js",
        "main.go", "app/main.py",
    ];
    for file in key_files {
        if let Ok(resp) = client
            .get(format!("https://raw.githubusercontent.com/{owner}/{repo}/HEAD/{file}"))
            .send().await
        {
            if resp.status().is_success() {
                if let Ok(text) = resp.text().await {
                    let truncated: String = text.chars().take(1500).collect();
                    if truncated.len() > 20 {
                        sections.push(format!("## {file}\n```\n{truncated}\n```"));
                    }
                }
            }
        }
    }

    let full = sections.join("\n\n");
    // 总体截断到 8000 字符（LLM 上下文限制）
    full.chars().take(8000).collect()
}

/// 非 GitHub URL：抓取页面内容 + 用 LLM 搜索相关背景
async fn gather_web_content(client: &reqwest::Client, url: &str, title: &str) -> String {
    let mut sections = Vec::new();

    // 1. 直接抓取页面内容
    if let Ok(resp) = client.get(url).send().await {
        if let Ok(text) = resp.text().await {
            let clean = sage_core::channels::feed::strip_html_tags(&text);
            let truncated: String = clean.chars().take(5000).collect();
            if truncated.len() > 50 {
                sections.push(format!("## 原文内容\n{truncated}"));
            }
        }
    }

    // 2. 用 DuckDuckGo Lite 搜索相关信息（无需 API key）
    let raw_query = format!("{title} site:reddit.com OR site:news.ycombinator.com");
    let query: String = url::form_urlencoded::byte_serialize(raw_query.as_bytes()).collect();
    if let Ok(resp) = client
        .get(format!("https://lite.duckduckgo.com/lite/?q={query}"))
        .send().await
    {
        if let Ok(text) = resp.text().await {
            let clean = sage_core::channels::feed::strip_html_tags(&text);
            let snippets: String = clean.chars().take(2000).collect();
            if snippets.len() > 100 {
                sections.push(format!("## 网络讨论\n{snippets}"));
            }
        }
    }

    let full = sections.join("\n\n");
    full.chars().take(8000).collect()
}
