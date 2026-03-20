use serde_json::{json, Value};
use tauri::State;

use super::{default_agent_config, map_err};
use crate::AppState;

/// 将日期转为相对标签：今日/明日/昨日/周X/MM-DD（支持中英双语）
fn date_to_relative_lang(target: chrono::NaiveDate, today: chrono::NaiveDate, lang: &str) -> String {
    use chrono::Datelike;
    let diff = (target - today).num_days();
    if lang == "en" {
        return match diff {
            0 => "Today".into(),
            1 => "Tomorrow".into(),
            -1 => "Yesterday".into(),
            2..=6 => {
                let wd = match target.weekday() {
                    chrono::Weekday::Mon => "Mon",
                    chrono::Weekday::Tue => "Tue",
                    chrono::Weekday::Wed => "Wed",
                    chrono::Weekday::Thu => "Thu",
                    chrono::Weekday::Fri => "Fri",
                    chrono::Weekday::Sat => "Sat",
                    chrono::Weekday::Sun => "Sun",
                };
                wd.into()
            }
            _ => format!("{}", target.format("%m-%d")),
        };
    }
    match diff {
        0 => "今日".into(),
        1 => "明日".into(),
        -1 => "昨日".into(),
        2..=6 => {
            let wd = match target.weekday() {
                chrono::Weekday::Mon => "一",
                chrono::Weekday::Tue => "二",
                chrono::Weekday::Wed => "三",
                chrono::Weekday::Thu => "四",
                chrono::Weekday::Fri => "五",
                chrono::Weekday::Sat => "六",
                chrono::Weekday::Sun => "日",
            };
            format!("周{wd}")
        }
        _ => format!("{}", target.format("%m-%d")),
    }
}

fn date_to_relative(target: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    date_to_relative_lang(target, today, "zh")
}

/// 从 timestamp 字符串提取日期并生成相对标签
fn timestamp_to_label(ts: &str, today: chrono::NaiveDate) -> String {
    let date = chrono::NaiveDate::parse_from_str(&ts[..10], "%Y-%m-%d")
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f").map(|dt| dt.date())
        })
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S").map(|dt| dt.date())
        });
    match date {
        Ok(d) => date_to_relative(d, today),
        Err(_) => String::new(),
    }
}

/// 后处理 hook：扫描 LLM 输出中的 MM-DD / YYYY-MM-DD，强制转为正确的相对标签
fn normalize_date_labels(items: Value) -> Value {
    use chrono::{Datelike, Local};
    let today = Local::now().date_naive();

    fn process_text(text: &str, today: chrono::NaiveDate) -> String {
        let year = today.year();
        let re = regex::Regex::new(r"(?:(\d{4})-)?(\d{1,2})-(\d{1,2})([\s，。、）\)：:])").unwrap();
        re.replace_all(text, |caps: &regex::Captures| {
            let y = caps
                .get(1)
                .map(|m| m.as_str().parse::<i32>().unwrap_or(year))
                .unwrap_or(year);
            let m: u32 = caps[2].parse().unwrap_or(0);
            let d: u32 = caps[3].parse().unwrap_or(0);
            let trail = caps.get(4).map(|m| m.as_str()).unwrap_or("");
            match chrono::NaiveDate::from_ymd_opt(y, m, d) {
                Some(date) => format!("{}{}", date_to_relative(date, today), trail),
                None => caps[0].to_string(),
            }
        })
        .to_string()
    }

    match items {
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|mut item| {
                    if let Some(content) = item
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                    {
                        item["content"] = json!(process_text(&content, today));
                    }
                    item
                })
                .collect(),
        ),
        other => other,
    }
}

/// 首页意识体：AI 编排当前最值得展示的 5-8 条信息
#[tauri::command]
pub async fn curate_homepage(state: State<'_, AppState>) -> Result<Value, String> {
    let suggestions = state.store.get_recent_suggestions(5).unwrap_or_default();
    let mut reports = std::collections::HashMap::new();
    for t in ["morning", "evening", "weekly"] {
        if let Ok(Some(r)) = state.store.get_latest_report(t) {
            reports.insert(t.to_string(), r);
        }
    }
    let memories = state.store.load_memories().unwrap_or_default();
    let memories: Vec<_> = memories.into_iter().take(10).collect();
    let daily_q = state.store.get_daily_question().ok().flatten();
    let lang = state.store.prompt_lang();
    let profile = state.store.load_profile().map_err(map_err)?;
    let fallback_name = if lang == "en" { "friend" } else { "朋友" };
    let user_name = profile
        .as_ref()
        .map(|p| p.identity.name.as_str())
        .unwrap_or(fallback_name);

    let now = chrono::Local::now();
    let time_label = if lang == "en" { "Current time" } else { "当前时间" };
    let mut context = format!("{}：{}\n\n", time_label, now.format("%Y-%m-%d %A %H:%M"));
    let today = now.date_naive();

    if !suggestions.is_empty() {
        let header = if lang == "en" { "## Pending Suggestions\n" } else { "## 待处理建议\n" };
        context.push_str(header);
        for s in &suggestions {
            let ts = s.timestamp.format("%Y-%m-%d %H:%M").to_string();
            let label = timestamp_to_label(&s.timestamp.to_rfc3339(), today);
            context.push_str(&format!(
                "- [{}｜{} {}] {}\n",
                s.event_source, label, ts, s.response
            ));
        }
        context.push('\n');
    }

    if !reports.is_empty() {
        let header = if lang == "en" { "## Recent Reports\n" } else { "## 最近报告\n" };
        context.push_str(header);
        for (rtype, r) in &reports {
            let preview: String = r.content.chars().take(200).collect();
            let label = timestamp_to_label(&r.created_at, today);
            context.push_str(&format!("- [{}｜{}] {}\n", rtype, label, preview));
        }
        context.push('\n');
    }

    if !memories.is_empty() {
        let header = if lang == "en" { "## Recent Memories\n" } else { "## 近期记忆\n" };
        context.push_str(header);
        for m in &memories {
            context.push_str(&format!("- [{}] {}\n", m.category, m.content));
        }
        context.push('\n');
    }

    if let Some(q) = &daily_q {
        let header = if lang == "en" { "## Today's Reflection" } else { "## 今日思考" };
        context.push_str(&format!("{}\n{}\n\n", header, q.prompt));
    }

    let system = sage_core::prompts::cmd_dashboard_brief_system(&lang, user_name);

    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or(if lang == "en" { "AI service not configured" } else { "未配置 AI 服务" })?;

    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    let raw = provider
        .invoke(&context, Some(&system))
        .await
        .map_err(map_err)?;

    let json_str = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let greeting = if lang == "en" {
        format!("Hello, {}. I'm ready.", user_name)
    } else {
        format!("{}，你好。我准备好了。", user_name)
    };
    let items: Value = serde_json::from_str(json_str)
        .unwrap_or_else(|_| json!([{"content": greeting, "category": "greeting"}]));

    Ok(normalize_date_labels(items))
}

/// 首页快照 — 从各数据源聚合内容卡片（纯数据，不调 LLM）
#[tauri::command]
pub fn get_dashboard_snapshot(state: State<'_, AppState>) -> Result<Value, String> {
    let mut items: Vec<Value> = Vec::new();

    let lang = state.store.prompt_lang();
    for rtype in ["morning", "evening", "weekly", "week_start"] {
        if let Ok(Some(r)) = state.store.get_latest_report(rtype) {
            let label = if lang == "en" {
                match rtype {
                    "morning" => "Morning Brief",
                    "evening" => "Evening Review",
                    "weekly" => "Weekly Report",
                    "week_start" => "Week Priorities",
                    _ => rtype,
                }
            } else {
                match rtype {
                    "morning" => "晨报",
                    "evening" => "晚报",
                    "weekly" => "周报",
                    "week_start" => "本周重点",
                    _ => rtype,
                }
            };
            let preview: String = r.content.chars().take(300).collect();
            items.push(json!({
                "ref_id": rtype,
                "content": preview,
                "category": "report",
                "meta": format!("{} · {}", label, &r.created_at[..16])
            }));
        }
    }

    let now_ts = chrono::Local::now();
    let memories = state.store.load_memories().unwrap_or_default();
    let mut scored: Vec<(f64, _)> = memories
        .into_iter()
        .map(|m| {
            let ref_time = m.last_accessed_at.as_deref().unwrap_or(&m.updated_at);
            let days_ago = chrono::NaiveDateTime::parse_from_str(ref_time, "%Y-%m-%dT%H:%M:%S%.f")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(ref_time, "%Y-%m-%d %H:%M:%S"))
                .map(|dt| (now_ts.naive_local() - dt).num_days().max(0) as f64)
                .unwrap_or(30.0);
            let recency = 0.2 + 0.8 * (-days_ago / 7.0_f64).exp();
            let relevance = m.confidence * recency;
            (relevance, m)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, m) in scored.into_iter().take(4) {
        items.push(json!({
            "id": m.id,
            "content": m.content,
            "category": "memory",
            "meta": format!("[{}] conf={:.0}%", m.category, m.confidence * 100.0)
        }));
    }

    if let Ok(sessions) = state.store.list_sessions(3) {
        for s in sessions {
            items.push(json!({
                "ref_id": s.session_id,
                "content": s.preview,
                "category": "session",
                "meta": format!("{}条消息", s.message_count)
            }));
        }
    }

    if let Ok(suggestions) = state.store.get_recent_suggestions(5) {
        for s in suggestions {
            let preview: String = s.response.chars().take(120).collect();
            items.push(json!({
                "id": s.id,
                "content": preview,
                "category": "suggestion",
                "meta": s.event_source.clone()
            }));
        }
    }

    if let Ok(Some(q)) = state.store.get_daily_question() {
        items.push(json!({
            "id": q.id,
            "content": q.response,
            "category": "question",
            "meta": "Daily Question"
        }));
    }

    Ok(json!(items))
}

/// 首页统计数据（纯聚合，不调 LLM）
#[tauri::command]
pub fn get_dashboard_stats(state: State<'_, AppState>) -> Result<Value, String> {
    let memory_count = state.store.count_memories().unwrap_or(0);
    let edge_count = state.store.count_memory_edges().unwrap_or(0);
    let session_count = state.store.count_distinct_sessions().unwrap_or(0);
    let message_count = state.store.count_messages().unwrap_or(0);
    let tags = state.store.get_all_tags().unwrap_or_default();
    let known_persons = state.store.get_known_persons().unwrap_or_default();

    Ok(json!({
        "memories": memory_count,
        "edges": edge_count,
        "sessions": session_count,
        "messages": message_count,
        "tag_count": tags.len(),
        "top_tags": tags.into_iter().take(6).map(|(t, c)| json!({"tag": t, "count": c})).collect::<Vec<_>>(),
        "known_persons": known_persons.len(),
    }))
}
