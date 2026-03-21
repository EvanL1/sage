use serde_json::{json, Value};
use tauri::State;

use super::{default_agent_config, map_err};
use crate::AppState;

/// 从 markdown 文本首行提取页面标题（去掉 `# ` 前缀）
fn extract_title(markdown: &str) -> String {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("# ") {
            return title.trim().to_string();
        }
    }
    "Untitled Page".to_string()
}

/// 通过 LLM 生成页面内容并保存到数据库
#[tauri::command]
pub async fn generate_page(
    state: State<'_, AppState>,
    prompt: String,
) -> Result<Value, String> {
    let lang = state.store.prompt_lang();

    let discovered = sage_core::discovery::discover_providers(&state.store);
    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let (info, config) = sage_core::discovery::select_best_provider(&discovered, &configs)
        .ok_or_else(|| {
            if lang == "en" {
                "No AI service available. Please configure an API key in Settings.".to_string()
            } else {
                "没有可用的 AI 服务。请在设置中配置 API Key。".to_string()
            }
        })?;

    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);
    let system_prompt = sage_core::prompts::page_gen_system(&lang).to_string();

    let markdown = provider
        .invoke(&prompt, Some(&system_prompt))
        .await
        .map_err(map_err)?;

    let title = extract_title(&markdown);
    let id = state
        .store
        .save_custom_page(&title, &markdown)
        .map_err(map_err)?;

    Ok(json!({
        "page_id": id,
        "title": title,
        "markdown": markdown,
    }))
}

/// 获取单个自定义页面
#[tauri::command]
pub async fn get_custom_page(state: State<'_, AppState>, id: i64) -> Result<Value, String> {
    match state.store.get_custom_page(id).map_err(map_err)? {
        Some((pid, title, markdown, created_at, updated_at)) => Ok(json!({
            "id": pid,
            "title": title,
            "markdown": markdown,
            "created_at": created_at,
            "updated_at": updated_at,
        })),
        None => Err(format!("Page {id} not found")),
    }
}

/// 列出所有自定义页面（不含 markdown 内容）
#[tauri::command]
pub async fn list_custom_pages(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let pages = state
        .store
        .list_custom_pages(limit.unwrap_or(50))
        .map_err(map_err)?;
    Ok(pages
        .into_iter()
        .map(|(id, title, created_at, updated_at)| {
            json!({
                "id": id,
                "title": title,
                "created_at": created_at,
                "updated_at": updated_at,
            })
        })
        .collect())
}

/// 更新自定义页面内容
#[tauri::command]
pub async fn update_custom_page(
    state: State<'_, AppState>,
    id: i64,
    title: String,
    markdown: String,
) -> Result<(), String> {
    state.store.update_custom_page(id, &title, &markdown).map_err(map_err)
}

/// 删除自定义页面
#[tauri::command]
pub async fn delete_custom_page(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.store.delete_custom_page(id).map_err(map_err)
}
