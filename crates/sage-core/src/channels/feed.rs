use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, error, info, warn};

use crate::pipeline::harness;
use sage_types::{Event, EventType};

use crate::store::Store;

use crate::agent::Agent;
use crate::channel::InputChannel;
use crate::config::{
    ArxivFeedConfig, GitHubFeedConfig, HackerNewsFeedConfig, RedditFeedConfig, RssFeedConfig,
};
use crate::prompts;

// ─── Raw item model ──────────────────────────────────────

struct RawFeedItem {
    title: String,
    url: String,
    snippet: String,
    #[allow(dead_code)] // used by raw_to_event in tests
    source: String,
}

struct FeedDeepRead {
    summary: String,
    idea: String,
}

// ─── Shared helpers ──────────────────────────────────────

fn build_feed_client() -> Client {
    let mut builder = Client::builder()
        .user_agent("Sage/1.0 (Personal AI Advisor)")
        .timeout(std::time::Duration::from_secs(15));

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

/// Check if enough time has elapsed; if so, update the timestamp and return true.
fn should_poll(last_polled: &Mutex<Instant>, interval_secs: u64) -> bool {
    let mut guard = last_polled.lock().unwrap_or_else(|e| e.into_inner());
    if guard.elapsed().as_secs() >= interval_secs {
        *guard = Instant::now();
        true
    } else {
        false
    }
}

/// Filter and summarise raw items through LLM. Returns Event per kept item.
async fn filter_and_summarise(
    agent: &Agent,
    items: &[RawFeedItem],
    lang: &str,
    interests: &str,
    personality: &str,
) -> Vec<Event> {
    if items.is_empty() {
        return Vec::new();
    }

    let max_items = items.len().min(20);
    let listing = items[..max_items]
        .iter()
        .enumerate()
        .map(|(i, it)| format!("[{}] {} | {} | {}", i + 1, it.title, it.url, it.snippet))
        .collect::<Vec<_>>()
        .join("\n");

    let interests_line = if interests.trim().is_empty() {
        "based on general tech professional interests".to_string()
    } else {
        interests.to_string()
    };

    let personality_section = if personality.trim().is_empty() {
        String::new()
    } else {
        match lang {
            "en" => format!("User profile: {personality}\n\n"),
            _ => format!("用户画像：{personality}\n\n"),
        }
    };

    let prompt = prompts::feed_filter_prompt(lang, &interests_line, &personality_section, &listing);

    match harness::invoke_text(agent, &prompt, None).await {
        Ok(text) => parse_llm_response(&text),
        Err(e) => {
            warn!("Feed LLM filter failed: {e}");
            Vec::new()
        }
    }
}

fn parse_llm_response(text: &str) -> Vec<Event> {
    text.lines().filter_map(parse_pipe_line).collect()
}

fn parse_pipe_line(line: &str) -> Option<Event> {
    let line = line.trim();
    if line.is_empty() || line.eq_ignore_ascii_case("NONE") {
        return None;
    }
    // Skip markdown table artifacts
    if line.starts_with("---")
        || line.starts_with("===")
        || line.eq_ignore_ascii_case("SCORE")
        || line.chars().all(|c| c == '-' || c == '|' || c == ' ')
    {
        return None;
    }
    let parts: Vec<&str> = line.splitn(4, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    let score: u8 = parts[0].trim().parse().unwrap_or(3);
    let title = parts[1].trim().to_string();
    let url = parts[2].trim().to_string();
    let insight = parts[3].trim().to_string();
    if title.is_empty() || url.is_empty() {
        return None;
    }
    Some(make_event(title, url, insight, "feed", score))
}

#[cfg(test)]
fn raw_to_event(item: &RawFeedItem) -> Event {
    make_event(
        item.title.clone(),
        item.url.clone(),
        item.snippet.clone(),
        &item.source,
        3,
    )
}

fn make_event(title: String, url: String, insight: String, source: &str, score: u8) -> Event {
    let mut metadata = HashMap::new();
    metadata.insert("score".into(), score.to_string());
    Event {
        source: source.to_string(),
        event_type: EventType::PatternObserved,
        title,
        body: format!("{url}\n{score}\n{insight}"),
        metadata,
        timestamp: chrono::Local::now(),
    }
}

/// Deep-read top-scoring items: fetch URL, extract text, summarise with LLM.
/// Processes up to 8 items (score >= 3) in descending score order, serially.
async fn deep_read_items(
    client: &Client,
    agent: &Agent,
    lang: &str,
    personality: &str,
    project_focus: &str,
    events: &mut Vec<Event>,
) {
    let mut candidates: Vec<(usize, String, u8)> = events
        .iter()
        .enumerate()
        .filter_map(|(i, ev)| {
            let score = ev
                .metadata
                .get("score")
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(0);
            if score >= 3 {
                let url = ev.body.lines().next().unwrap_or("").to_string();
                if !url.is_empty() && url.starts_with("http") {
                    return Some((i, url, score));
                }
            }
            None
        })
        .collect();
    candidates.sort_by(|a, b| b.2.cmp(&a.2));
    candidates.truncate(8);

    for (idx, url, score) in candidates {
        if let Some(analysis) =
            fetch_and_summarise(client, agent, lang, &url, score, personality, project_focus).await
        {
            if let Some(ev) = events.get_mut(idx) {
                let mut lines = ev.body.lines();
                let original_url = lines.next().unwrap_or("").to_string();
                let original_score = lines.next().unwrap_or("").to_string();
                let original_insight = lines.next().unwrap_or("").to_string();
                ev.body = format!(
                    "{original_url}\n{original_score}\n{original_insight}\n{}\n{}",
                    analysis.summary, analysis.idea
                );
            }
        }
    }
}

async fn fetch_and_summarise(
    client: &Client,
    agent: &Agent,
    lang: &str,
    url: &str,
    score: u8,
    personality: &str,
    project_focus: &str,
) -> Option<FeedDeepRead> {
    let char_limit = if score >= 4 { 3000 } else { 1000 };

    // GitHub 仓库页面 HTML 充斥 feature flags JSON，改抓 README raw 内容
    let fetch_url = if url.starts_with("https://github.com/") {
        let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
        // github.com/user/repo → 3 段 path，追加 README
        if parts.len() == 5 {
            format!("https://raw.githubusercontent.com/{}/{}/HEAD/README.md", parts[3], parts[4])
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    };

    let text = match client
        .get(&fetch_url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(resp) => resp.text().await.ok()?,
        Err(e) => {
            debug!("Deep read fetch failed for {fetch_url}: {e}");
            return None;
        }
    };

    // GitHub README 已经是 markdown，不需要 strip_html
    let clean = if fetch_url.contains("raw.githubusercontent.com") {
        text.clone()
    } else {
        strip_html_tags(&text)
    };
    let truncated: String = clean.chars().take(char_limit).collect();

    if truncated.len() < 50 {
        return None;
    }

    let sentence_count = if score >= 4 { "2-3" } else { "1" };
    let project_section = if project_focus.trim().is_empty() {
        match lang {
            "en" => "Current project context: none provided. If useful, propose a practical idea for the user's likely work based on their interests."
                .to_string(),
            _ => "当前项目上下文：未提供。如果有帮助，可以基于用户兴趣提出一个贴近实际工作的想法。"
                .to_string(),
        }
    } else {
        match lang {
            "en" => format!("Current project context:\n{project_focus}"),
            _ => format!("当前项目上下文：\n{project_focus}"),
        }
    };
    let prompt = prompts::feed_deep_read_prompt(lang, sentence_count, personality, &project_section, &truncated);

    match harness::invoke_text(agent, &prompt, None).await {
        Ok(text) => parse_deep_read_response(&text),
        Err(e) => {
            warn!("Deep read summarise failed for {url}: {e}");
            None
        }
    }
}

fn parse_deep_read_response(text: &str) -> Option<FeedDeepRead> {
    let mut summary = String::new();
    let mut idea = String::new();

    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(rest) = line
            .strip_prefix("SUMMARY:")
            .or_else(|| line.strip_prefix("TAKEAWAY:"))
            .or_else(|| line.strip_prefix("摘要："))
            .or_else(|| line.strip_prefix("摘要:"))
            .or_else(|| line.strip_prefix("洞察："))
            .or_else(|| line.strip_prefix("洞察:"))
        {
            summary = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("IDEA:")
            .or_else(|| line.strip_prefix("ACTION:"))
            .or_else(|| line.strip_prefix("启发："))
            .or_else(|| line.strip_prefix("启发:"))
            .or_else(|| line.strip_prefix("行动："))
            .or_else(|| line.strip_prefix("行动:"))
        {
            idea = rest.trim().to_string();
            continue;
        }

        if !idea.is_empty() {
            if !idea.ends_with(' ') {
                idea.push(' ');
            }
            idea.push_str(line);
        } else if !summary.is_empty() {
            if !summary.ends_with(' ') {
                summary.push(' ');
            }
            summary.push_str(line);
        }
    }

    if summary.is_empty() {
        let fallback = text.trim();
        if fallback.is_empty() || fallback.eq_ignore_ascii_case("NONE") {
            return None;
        }
        summary = fallback.to_string();
    }

    if idea.eq_ignore_ascii_case("NONE") || idea == "无" || idea.eq_ignore_ascii_case("ACTION: NONE") {
        idea.clear();
    }

    Some(FeedDeepRead { summary, idea })
}

/// Simple HTML tag stripper using find() for UTF-8 safety.
pub fn strip_html_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut r = input;

    while let Some(open) = r.find('<') {
        output.push_str(&r[..open]);
        if let Some(close) = r[open..].find('>') {
            r = &r[open + close + 1..];
        } else {
            break;
        }
    }
    output.push_str(r);

    let output = output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ─── Reddit Channel ──────────────────────────────────────

pub struct RedditChannel {
    config: RedditFeedConfig,
    client: Client,
    agent: Arc<Agent>,
    store: Arc<Store>,
    interests: String,
    personality: String,
    project_focus: String,
    last_polled: Mutex<Instant>,
}

impl RedditChannel {
    pub fn new(
        config: RedditFeedConfig,
        store: Arc<Store>,
        interests: String,
        personality: String,
        project_focus: String,
        agent: Arc<Agent>,
    ) -> Self {
        let past = Instant::now()
            .checked_sub(std::time::Duration::from_secs(
                config.poll_interval_secs + 1,
            ))
            .unwrap_or_else(Instant::now);
        Self {
            config,
            client: build_feed_client(),
            agent,
            store,
            interests,
            personality,
            project_focus,
            last_polled: Mutex::new(past),
        }
    }
}

#[async_trait]
impl InputChannel for RedditChannel {
    fn name(&self) -> &str {
        "reddit"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        if self.config.subreddits.is_empty() {
            return Ok(Vec::new());
        }
        if !should_poll(&self.last_polled, self.config.poll_interval_secs) {
            return Ok(Vec::new());
        }

        let mut items = Vec::new();
        for sub in &self.config.subreddits {
            match fetch_reddit_hot(&self.client, sub, self.config.limit).await {
                Ok(mut fetched) => items.append(&mut fetched),
                Err(e) => error!("Reddit r/{sub} fetch failed: {e}"),
            }
        }
        info!("Reddit: fetched {} raw items", items.len());
        let mut events = filter_and_summarise(
            &self.agent,
            &items,
            &self.store.prompt_lang(),
            &self.interests,
            &self.personality,
        )
        .await;
        deep_read_items(
            &self.client,
            &self.agent,
            &self.store.prompt_lang(),
            &self.personality,
            &self.project_focus,
            &mut events,
        )
        .await;
        Ok(events)
    }
}

async fn fetch_reddit_hot(client: &Client, sub: &str, limit: usize) -> Result<Vec<RawFeedItem>> {
    let url = format!("https://www.reddit.com/r/{sub}/hot.json?limit={limit}");
    let resp = client
        .get(&url)
        .header("User-Agent", "Sage/1.0")
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;

    let children = resp
        .pointer("/data/children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let items = children
        .iter()
        .filter_map(|child| {
            let d = child.get("data")?;
            let title = d.get("title")?.as_str()?.to_string();
            let url = d
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let permalink = d.get("permalink").and_then(|v| v.as_str()).unwrap_or("");
            let full_url = if url.is_empty() {
                format!("https://reddit.com{permalink}")
            } else {
                url
            };
            Some(RawFeedItem {
                title,
                url: full_url,
                snippet: format!("r/{sub}"),
                source: format!("reddit/r/{sub}"),
            })
        })
        .collect();

    Ok(items)
}

// ─── HackerNews Channel ──────────────────────────────────

pub struct HackerNewsChannel {
    config: HackerNewsFeedConfig,
    client: Client,
    agent: Arc<Agent>,
    store: Arc<Store>,
    interests: String,
    personality: String,
    project_focus: String,
    last_polled: Mutex<Instant>,
}

impl HackerNewsChannel {
    pub fn new(
        config: HackerNewsFeedConfig,
        store: Arc<Store>,
        interests: String,
        personality: String,
        project_focus: String,
        agent: Arc<Agent>,
    ) -> Self {
        let past = Instant::now()
            .checked_sub(std::time::Duration::from_secs(
                config.poll_interval_secs + 1,
            ))
            .unwrap_or_else(Instant::now);
        Self {
            config,
            client: build_feed_client(),
            agent,
            store,
            interests,
            personality,
            project_focus,
            last_polled: Mutex::new(past),
        }
    }
}

#[async_trait]
impl InputChannel for HackerNewsChannel {
    fn name(&self) -> &str {
        "hackernews"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        if !should_poll(&self.last_polled, self.config.poll_interval_secs) {
            return Ok(Vec::new());
        }

        let items = fetch_hn_top(&self.client, self.config.limit, self.config.min_score).await?;
        info!("HackerNews: fetched {} raw items", items.len());
        let mut events = filter_and_summarise(
            &self.agent,
            &items,
            &self.store.prompt_lang(),
            &self.interests,
            &self.personality,
        )
        .await;
        deep_read_items(
            &self.client,
            &self.agent,
            &self.store.prompt_lang(),
            &self.personality,
            &self.project_focus,
            &mut events,
        )
        .await;
        Ok(events)
    }
}

async fn fetch_hn_top(client: &Client, limit: usize, min_score: u32) -> Result<Vec<RawFeedItem>> {
    let ids: Vec<u64> = client
        .get("https://hacker-news.firebaseio.com/v0/topstories.json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut items = Vec::new();
    for &id in ids.iter().take(limit * 3) {
        if items.len() >= limit {
            break;
        }
        let url = format!("https://hacker-news.firebaseio.com/v0/item/{id}.json");
        match client
            .get(&url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(resp) => {
                if let Ok(item) = resp.json::<serde_json::Value>().await {
                    let score = item.get("score").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    if score < min_score {
                        continue;
                    }
                    let title = item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let item_url = item
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&format!("https://news.ycombinator.com/item?id={id}"))
                        .to_string();
                    if !title.is_empty() {
                        items.push(RawFeedItem {
                            title,
                            url: item_url,
                            snippet: format!("score: {score}"),
                            source: "hackernews".to_string(),
                        });
                    }
                }
            }
            Err(e) => debug!("HN item {id} fetch failed: {e}"),
        }
    }
    Ok(items)
}

// ─── GitHub Channel ──────────────────────────────────────

pub struct GitHubChannel {
    config: GitHubFeedConfig,
    client: Client,
    agent: Arc<Agent>,
    store: Arc<Store>,
    interests: String,
    personality: String,
    project_focus: String,
    last_polled: Mutex<Instant>,
}

impl GitHubChannel {
    pub fn new(
        config: GitHubFeedConfig,
        store: Arc<Store>,
        interests: String,
        personality: String,
        project_focus: String,
        agent: Arc<Agent>,
    ) -> Self {
        let past = Instant::now()
            .checked_sub(std::time::Duration::from_secs(
                config.poll_interval_secs + 1,
            ))
            .unwrap_or_else(Instant::now);
        Self {
            config,
            client: build_feed_client(),
            agent,
            store,
            interests,
            personality,
            project_focus,
            last_polled: Mutex::new(past),
        }
    }
}

#[async_trait]
impl InputChannel for GitHubChannel {
    fn name(&self) -> &str {
        "github"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        if !should_poll(&self.last_polled, self.config.poll_interval_secs) {
            return Ok(Vec::new());
        }

        let items = fetch_github_trending(&self.client, &self.config.trending_languages.join(",")).await?;
        info!("GitHub: fetched {} raw items", items.len());
        let mut events = filter_and_summarise(
            &self.agent,
            &items,
            &self.store.prompt_lang(),
            &self.interests,
            &self.personality,
        )
        .await;
        deep_read_items(
            &self.client,
            &self.agent,
            &self.store.prompt_lang(),
            &self.personality,
            &self.project_focus,
            &mut events,
        )
        .await;
        Ok(events)
    }
}

async fn fetch_github_trending(client: &Client, languages: &str) -> Result<Vec<RawFeedItem>> {
    let lang_part = if languages.is_empty() {
        "stars:>100".to_string()
    } else {
        // 支持多语言：Rust,Go → stars:>100+language:Rust+language:Go
        let langs: Vec<&str> = languages.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let lang_q = langs.iter().map(|l| format!("language:{l}")).collect::<Vec<_>>().join("+");
        format!("stars:>100+{lang_q}")
    };
    let url = format!(
        "https://api.github.com/search/repositories?q={lang_part}&sort=updated&order=desc&per_page=10"
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "Sage/1.0")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;

    let items_arr = resp
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let items = items_arr
        .iter()
        .filter_map(|repo| {
            let name = repo.get("full_name")?.as_str()?.to_string();
            let desc = repo
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let html_url = repo.get("html_url")?.as_str()?.to_string();
            let stars = repo
                .get("stargazers_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Some(RawFeedItem {
                title: name,
                url: html_url,
                snippet: format!("{desc} (★{stars})"),
                source: "github".to_string(),
            })
        })
        .collect();

    Ok(items)
}

// ─── Arxiv Channel ───────────────────────────────────────

pub struct ArxivChannel {
    config: ArxivFeedConfig,
    client: Client,
    agent: Arc<Agent>,
    store: Arc<Store>,
    interests: String,
    personality: String,
    project_focus: String,
    last_polled: Mutex<Instant>,
}

impl ArxivChannel {
    pub fn new(
        config: ArxivFeedConfig,
        store: Arc<Store>,
        interests: String,
        personality: String,
        project_focus: String,
        agent: Arc<Agent>,
    ) -> Self {
        let past = Instant::now()
            .checked_sub(std::time::Duration::from_secs(
                config.poll_interval_secs + 1,
            ))
            .unwrap_or_else(Instant::now);
        Self {
            config,
            client: build_feed_client(),
            agent,
            store,
            interests,
            personality,
            project_focus,
            last_polled: Mutex::new(past),
        }
    }
}

#[async_trait]
impl InputChannel for ArxivChannel {
    fn name(&self) -> &str {
        "arxiv"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        if !should_poll(&self.last_polled, self.config.poll_interval_secs) {
            return Ok(Vec::new());
        }

        let items =
            fetch_arxiv(&self.client, &self.config.categories, &self.config.keywords).await?;
        info!("Arxiv: fetched {} raw items", items.len());
        let mut events = filter_and_summarise(
            &self.agent,
            &items,
            &self.store.prompt_lang(),
            &self.interests,
            &self.personality,
        )
        .await;
        deep_read_items(
            &self.client,
            &self.agent,
            &self.store.prompt_lang(),
            &self.personality,
            &self.project_focus,
            &mut events,
        )
        .await;
        Ok(events)
    }
}

async fn fetch_arxiv(
    client: &Client,
    categories: &[String],
    keywords: &[String],
) -> Result<Vec<RawFeedItem>> {
    let mut parts: Vec<String> = categories.iter().map(|c| format!("cat:{c}")).collect();
    parts.extend(keywords.iter().map(|k| format!("all:{k}")));

    if parts.is_empty() {
        return Ok(Vec::new());
    }

    let query = parts.join("+OR+");
    let url = format!(
        "https://export.arxiv.org/api/query?search_query={query}&sortBy=lastUpdatedDate&max_results=10"
    );

    let xml = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    Ok(parse_arxiv_xml(&xml))
}

fn parse_arxiv_xml(xml: &str) -> Vec<RawFeedItem> {
    let mut items = Vec::new();
    let mut rest = xml;

    while let Some(entry_start) = rest.find("<entry>") {
        rest = &rest[entry_start + "<entry>".len()..];
        let entry_end = rest.find("</entry>").unwrap_or(rest.len());
        let entry = &rest[..entry_end];

        let title = extract_xml_text(entry, "title")
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let id = extract_xml_text(entry, "id").unwrap_or_default();
        let summary = extract_xml_text(entry, "summary")
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();

        if !title.is_empty() && !id.is_empty() {
            items.push(RawFeedItem {
                title,
                url: id.trim().to_string(),
                snippet: summary.chars().take(200).collect(),
                source: "arxiv".to_string(),
            });
        }

        if entry_end >= rest.len() {
            break;
        }
        rest = &rest[entry_end + "</entry>".len()..];
    }

    items
}

fn extract_xml_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    if end >= start {
        Some(&xml[start..end])
    } else {
        None
    }
}

// ─── RSS/Atom Channel ────────────────────────────────────

pub struct RssChannel {
    config: RssFeedConfig,
    client: Client,
    agent: Arc<Agent>,
    store: Arc<Store>,
    interests: String,
    personality: String,
    project_focus: String,
    last_polled: Mutex<Instant>,
}

impl RssChannel {
    pub fn new(
        config: RssFeedConfig,
        store: Arc<Store>,
        interests: String,
        personality: String,
        project_focus: String,
        agent: Arc<Agent>,
    ) -> Self {
        let past = Instant::now()
            .checked_sub(std::time::Duration::from_secs(
                config.poll_interval_secs + 1,
            ))
            .unwrap_or_else(Instant::now);
        Self {
            config,
            client: build_feed_client(),
            agent,
            store,
            interests,
            personality,
            project_focus,
            last_polled: Mutex::new(past),
        }
    }
}

#[async_trait]
impl InputChannel for RssChannel {
    fn name(&self) -> &str {
        "rss"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        if self.config.feeds.is_empty() {
            return Ok(Vec::new());
        }
        if !should_poll(&self.last_polled, self.config.poll_interval_secs) {
            return Ok(Vec::new());
        }

        let mut items = Vec::new();
        for url in &self.config.feeds {
            match fetch_rss_feed(&self.client, url).await {
                Ok(mut fetched) => items.append(&mut fetched),
                Err(e) => error!("RSS feed {url} fetch failed: {e}"),
            }
        }
        info!(
            "RSS: fetched {} raw items from {} feeds",
            items.len(),
            self.config.feeds.len()
        );
        let mut events = filter_and_summarise(
            &self.agent,
            &items,
            &self.store.prompt_lang(),
            &self.interests,
            &self.personality,
        )
        .await;
        deep_read_items(
            &self.client,
            &self.agent,
            &self.store.prompt_lang(),
            &self.personality,
            &self.project_focus,
            &mut events,
        )
        .await;
        Ok(events)
    }
}

async fn fetch_rss_feed(client: &Client, url: &str) -> Result<Vec<RawFeedItem>> {
    let xml = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Try RSS <item> first, then Atom <entry>
    let items = parse_rss_xml(&xml, url);
    if !items.is_empty() {
        return Ok(items);
    }
    Ok(parse_atom_xml(&xml, url))
}

/// Parse RSS 2.0 — extracts <item> blocks.
fn parse_rss_xml(xml: &str, feed_url: &str) -> Vec<RawFeedItem> {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item>").or_else(|| rest.find("<item ")) {
        let tag_end = rest[start..]
            .find('>')
            .map(|i| start + i + 1)
            .unwrap_or(rest.len());
        rest = &rest[tag_end..];
        let end = rest.find("</item>").unwrap_or(rest.len());
        let block = &rest[..end];

        let title = extract_xml_text(block, "title")
            .unwrap_or_default()
            .trim()
            .to_string();
        let link = extract_xml_text(block, "link")
            .or_else(|| extract_xml_attr(block, "link", "href"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let desc = extract_xml_text(block, "description")
            .or_else(|| extract_xml_text(block, "summary"))
            .unwrap_or_default();
        let snippet: String = desc
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(200)
            .collect();

        if !title.is_empty() {
            items.push(RawFeedItem {
                title,
                url: link,
                snippet,
                source: feed_url.to_string(),
            });
        }
        if end >= rest.len() {
            break;
        }
        rest = &rest[end + "</item>".len()..];
    }
    items
}

/// Parse Atom 1.0 — extracts <entry> blocks (reuses arXiv-style logic).
fn parse_atom_xml(xml: &str, feed_url: &str) -> Vec<RawFeedItem> {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(entry_start) = rest.find("<entry>") {
        rest = &rest[entry_start + "<entry>".len()..];
        let entry_end = rest.find("</entry>").unwrap_or(rest.len());
        let block = &rest[..entry_end];

        let title = extract_xml_text(block, "title")
            .unwrap_or_default()
            .trim()
            .to_string();
        let link = extract_xml_attr(block, "link", "href")
            .or_else(|| extract_xml_text(block, "link"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let summary = extract_xml_text(block, "summary")
            .or_else(|| extract_xml_text(block, "content"))
            .unwrap_or_default();
        let snippet: String = summary
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(200)
            .collect();

        if !title.is_empty() {
            items.push(RawFeedItem {
                title,
                url: link,
                snippet,
                source: feed_url.to_string(),
            });
        }
        if entry_end >= rest.len() {
            break;
        }
        rest = &rest[entry_end + "</entry>".len()..];
    }
    items
}

/// Extract the value of `attr` from a self-closing or paired `<tag ... attr="value">` element.
fn extract_xml_attr<'a>(xml: &'a str, tag: &str, attr: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let tag_start = xml.find(&open)?;
    let tag_end = xml[tag_start..].find('>')?;
    let tag_body = &xml[tag_start..tag_start + tag_end];
    let attr_key = format!("{attr}=\"");
    let val_start = tag_body.find(&attr_key)? + attr_key.len();
    let val_end = tag_body[val_start..].find('"')?;
    Some(&tag_body[val_start..val_start + val_end])
}

// ─── Topic Search (instant, no LLM) ─────────────────────

/// 即时搜索：并行查询 HN Algolia + Reddit → LLM 打分筛选 → deep read 总结 → 存入 feed。
/// 需要 Agent（LLM）+ Store。返回最终存入的条目数。
pub async fn search_topic(query: &str, agent: &Agent, store: &Store) -> Result<usize> {
    let client = build_feed_client();
    let q = query.to_string();

    // 并行搜索 HN + Reddit
    let (hn_result, reddit_result) = tokio::join!(
        search_hn_algolia(&client, &q),
        search_reddit(&client, &q),
    );

    let mut items: Vec<RawFeedItem> = Vec::new();
    match hn_result {
        Ok(v) => items.extend(v),
        Err(e) => warn!("HN search failed: {e}"),
    }
    match reddit_result {
        Ok(v) => items.extend(v),
        Err(e) => warn!("Reddit search failed: {e}"),
    }

    if items.is_empty() {
        return Ok(0);
    }
    info!("Topic search '{q}': fetched {} raw items, sending to LLM", items.len());

    let lang = store.prompt_lang();
    let interests = q.clone(); // 搜索词本身就是兴趣上下文
    let personality = store
        .search_memories("identity personality traits", 5)
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.content)
        .collect::<Vec<_>>()
        .join("; ");
    let project_focus = String::new();

    // LLM 打分 + 筛选
    let mut events = filter_and_summarise(agent, &items, &lang, &interests, &personality).await;

    // Deep read：抓取原文 + LLM 总结/建议
    deep_read_items(&client, agent, &lang, &personality, &project_focus, &mut events).await;

    // 存入 observations（去重）
    let mut saved = 0;
    for ev in &events {
        if !store.has_feed_observation(&ev.title) {
            if store.record_observation("feed", &ev.title, Some(&ev.body)).is_ok() {
                saved += 1;
            }
        }
    }

    info!("Topic search '{q}': {saved}/{} items saved after LLM filter", events.len());
    Ok(saved)
}

async fn search_hn_algolia(client: &Client, query: &str) -> Result<Vec<RawFeedItem>> {
    let url = format!(
        "https://hn.algolia.com/api/v1/search?query={}&tags=story&hitsPerPage=15",
        urlencoding::encode(query)
    );
    let resp = client.get(&url).send().await?.error_for_status()?.json::<serde_json::Value>().await?;

    let hits = resp["hits"].as_array().cloned().unwrap_or_default();
    let items = hits.iter().filter_map(|h| {
        let title = h["title"].as_str()?.to_string();
        let item_url = h["url"].as_str().unwrap_or("").to_string();
        let url = if item_url.is_empty() {
            let object_id = h["objectID"].as_str().unwrap_or("");
            format!("https://news.ycombinator.com/item?id={object_id}")
        } else {
            item_url
        };
        let points = h["points"].as_u64().unwrap_or(0);
        Some(RawFeedItem {
            title,
            url,
            snippet: format!("HN {points}pts"),
            source: "hackernews".into(),
        })
    }).collect();

    Ok(items)
}

async fn search_reddit(client: &Client, query: &str) -> Result<Vec<RawFeedItem>> {
    let url = format!(
        "https://www.reddit.com/search.json?q={}&sort=relevance&limit=15",
        urlencoding::encode(query)
    );
    let resp = client
        .get(&url)
        .header("User-Agent", "Sage/1.0")
        .send().await?
        .error_for_status()?
        .json::<serde_json::Value>().await?;

    let children = resp.pointer("/data/children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let items = children.iter().filter_map(|child| {
        let d = child.get("data")?;
        let title = d["title"].as_str()?.to_string();
        let sub = d["subreddit"].as_str().unwrap_or("?");
        let ups = d["ups"].as_i64().unwrap_or(0);
        let post_url = d["url"].as_str().unwrap_or("").to_string();
        let permalink = d["permalink"].as_str().unwrap_or("");
        let url = if post_url.is_empty() {
            format!("https://reddit.com{permalink}")
        } else {
            post_url
        };
        Some(RawFeedItem {
            title,
            url,
            snippet: format!("r/{sub} {ups}ups"),
            source: "reddit".into(),
        })
    }).collect();

    Ok(items)
}

// ─── Tests ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_poll_respects_interval() {
        // Starts already elapsed (constructor sets it to past)
        let last = Mutex::new(
            Instant::now()
                .checked_sub(std::time::Duration::from_secs(10))
                .unwrap_or_else(Instant::now),
        );
        assert!(should_poll(&last, 5), "should poll when interval elapsed");
        // Now the timestamp was just updated
        assert!(!should_poll(&last, 5), "should NOT poll immediately again");
    }

    #[test]
    fn test_arxiv_xml_parse() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed>
  <entry>
    <title>Attention Is All You Need</title>
    <id>https://arxiv.org/abs/1706.03762</id>
    <summary>We propose a new architecture called Transformer.</summary>
  </entry>
  <entry>
    <title>BERT: Pre-training</title>
    <id>https://arxiv.org/abs/1810.04805</id>
    <summary>Language model pre-training.</summary>
  </entry>
</feed>"#;

        let items = parse_arxiv_xml(xml);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Attention Is All You Need");
        assert_eq!(items[0].url, "https://arxiv.org/abs/1706.03762");
        assert!(items[0].snippet.contains("Transformer"));
        assert_eq!(items[1].title, "BERT: Pre-training");
    }

    #[test]
    fn test_filter_summarise_parse_pipe_response() {
        let llm_output = "3 | Rust async deep dive | https://example.com/rust | Great article on async Rust patterns\nNONE";
        let events = parse_llm_response(llm_output);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Rust async deep dive");
        assert!(events[0].body.contains("https://example.com/rust"));
    }

    #[tokio::test]
    async fn test_reddit_empty_subreddits_returns_ok_empty() {
        use crate::config::AgentConfig;
        let agent = Arc::new(Agent::new(AgentConfig::default()));
        let cfg = RedditFeedConfig {
            enabled: true,
            poll_interval_secs: 3600,
            subreddits: Vec::new(),
            limit: 10,
        };
        let store = Arc::new(Store::open_in_memory().expect("test store"));
        let ch = RedditChannel::new(
            cfg,
            store,
            "rust programming".into(),
            String::new(),
            String::new(),
            agent,
        );
        let result = ch.poll().await.expect("should not error");
        assert!(result.is_empty());
    }

    #[test]
    fn test_arxiv_xml_parse_empty() {
        let xml = "<feed></feed>";
        let items = parse_arxiv_xml(xml);
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_pipe_line_none_keyword() {
        assert!(parse_pipe_line("NONE").is_none());
        assert!(parse_pipe_line("none").is_none());
        assert!(parse_pipe_line("").is_none());
    }

    #[test]
    fn test_parse_pipe_line_malformed() {
        // Only 3 parts (need 4 now) — should return None
        assert!(parse_pipe_line("title | url | insight").is_none());
    }

    #[test]
    fn test_parse_pipe_line_with_score() {
        let ev =
            parse_pipe_line("4 | Great Article | https://example.com | Insightful read").unwrap();
        assert_eq!(ev.title, "Great Article");
        assert_eq!(ev.metadata.get("score").unwrap(), "4");
        assert!(ev.body.contains("https://example.com"));
        assert!(ev.body.contains("Insightful read"));
    }

    #[test]
    fn test_parse_pipe_line_score_fallback() {
        // Non-numeric score should fallback to 3
        let ev = parse_pipe_line("high | My Title | https://example.com | Some insight").unwrap();
        assert_eq!(ev.metadata.get("score").unwrap(), "3");
    }

    #[test]
    fn test_strip_html_tags() {
        let html = "<html><body><h1>Hello</h1><p>World &amp; more</p></body></html>";
        let result = strip_html_tags(html);
        assert!(result.contains("Hello"));
        assert!(result.contains("World & more"));
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
    }

    #[test]
    fn test_parse_deep_read_response_extracts_summary_and_idea() {
        let parsed = parse_deep_read_response(
            "SUMMARY: This explains a faster retrieval pattern.\nIDEA: Reuse the ranking step in the current feed pipeline.",
        )
        .unwrap();
        assert_eq!(parsed.summary, "This explains a faster retrieval pattern.");
        assert_eq!(
            parsed.idea,
            "Reuse the ranking step in the current feed pipeline."
        );
    }

    #[test]
    fn test_parse_deep_read_response_treats_none_idea_as_empty() {
        let parsed = parse_deep_read_response(
            "SUMMARY: This article is mostly background context.\nIDEA: NONE",
        )
        .unwrap();
        assert_eq!(parsed.summary, "This article is mostly background context.");
        assert!(parsed.idea.is_empty());
    }

    #[test]
    fn test_parse_deep_read_response_supports_zh_labels() {
        let parsed = parse_deep_read_response(
            "摘要：这篇文章解释了新的缓存策略。\n启发：可以把这个思路用在 Feed 排序里。",
        )
        .unwrap();
        assert_eq!(parsed.summary, "这篇文章解释了新的缓存策略。");
        assert_eq!(parsed.idea, "可以把这个思路用在 Feed 排序里。");
    }

    #[test]
    fn test_parse_deep_read_response_treats_zh_none_as_empty() {
        let parsed = parse_deep_read_response("摘要：这是一篇背景资料。\n启发：无").unwrap();
        assert_eq!(parsed.summary, "这是一篇背景资料。");
        assert!(parsed.idea.is_empty());
    }

    #[test]
    fn test_rss2_parse() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item>
    <title>Rust 1.77 Released</title>
    <link>https://blog.rust-lang.org/2024/03/21/Rust-1.77.0.html</link>
    <description>What's new in Rust 1.77.</description>
  </item>
  <item>
    <title>Async Rust in 2024</title>
    <link>https://example.com/async</link>
    <description>Deep dive into async.</description>
  </item>
</channel></rss>"#;
        let items = parse_rss_xml(xml, "https://blog.rust-lang.org/feed.xml");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Rust 1.77 Released");
        assert_eq!(
            items[0].url,
            "https://blog.rust-lang.org/2024/03/21/Rust-1.77.0.html"
        );
        assert!(items[0].snippet.contains("Rust 1.77"));
    }

    #[test]
    fn test_atom_parse() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <title>OpenAI announces GPT-5</title>
    <link href="https://openai.com/blog/gpt5"/>
    <summary>Details about GPT-5 launch.</summary>
  </entry>
</feed>"#;
        let items = parse_atom_xml(xml, "https://openai.com/blog/rss.xml");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "OpenAI announces GPT-5");
        assert_eq!(items[0].url, "https://openai.com/blog/gpt5");
    }

    #[test]
    fn test_extract_xml_attr() {
        let xml = r#"<link href="https://example.com/page" rel="alternate"/>"#;
        assert_eq!(
            extract_xml_attr(xml, "link", "href"),
            Some("https://example.com/page")
        );
    }

    #[test]
    fn test_rss_empty_feed_url_list() {
        let items = parse_rss_xml("<rss><channel></channel></rss>", "http://x");
        assert!(items.is_empty());
    }

    #[test]
    fn test_raw_to_event_default_score() {
        let item = RawFeedItem {
            title: "Test".into(),
            url: "https://example.com".into(),
            snippet: "A snippet".into(),
            source: "test".into(),
        };
        let ev = raw_to_event(&item);
        assert_eq!(ev.metadata.get("score").unwrap(), "3");
        assert!(ev.body.contains("https://example.com"));
    }
}
