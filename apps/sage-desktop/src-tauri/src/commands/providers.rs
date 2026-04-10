use sage_types::ProviderConfig;
use serde_json::{json, Value};
use tauri::State;

use super::{default_agent_config, map_err};
use crate::AppState;

#[tauri::command]
pub async fn discover_providers(
    state: State<'_, AppState>,
) -> Result<Vec<sage_types::ProviderInfo>, String> {
    Ok(sage_core::discovery::discover_providers(&state.store))
}

#[tauri::command]
pub async fn save_provider_config(
    state: State<'_, AppState>,
    config: sage_types::ProviderConfig,
) -> Result<(), String> {
    state.store.save_provider_config(&config).map_err(map_err)
}

#[tauri::command]
pub async fn get_provider_configs(
    state: State<'_, AppState>,
) -> Result<Vec<sage_types::ProviderConfig>, String> {
    state.store.load_provider_configs().map_err(map_err)
}

#[tauri::command]
pub async fn test_provider(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Value, String> {
    let providers = sage_core::discovery::discover_providers(&state.store);
    let info = providers
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Provider not found: {provider_id}"))?;

    let configs = state.store.load_provider_configs().map_err(map_err)?;
    let config = configs
        .into_iter()
        .find(|c| c.provider_id == provider_id)
        .unwrap_or(ProviderConfig {
            provider_id: provider_id.clone(),
            api_key: None,
            model: None,
            base_url: None,
            enabled: true,
            priority: None,
        });

    let agent_config = default_agent_config();
    let provider = sage_core::provider::create_provider_from_config(&info, &config, &agent_config);

    match provider.invoke("Say hello in one sentence.", None).await {
        Ok(response) => Ok(json!({ "success": true, "response": response })),
        Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
    }
}

/// 获取各 provider 最近一次调用错误（从 kv_store 读取）
#[tauri::command]
pub async fn get_provider_errors(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Value>, String> {
    let mut result = std::collections::HashMap::new();
    // 扫描所有已知 provider 的错误记录
    let providers = sage_core::discovery::discover_providers(&state.store);
    for p in &providers {
        let key = format!("provider_error:{}", p.id);
        if let Ok(Some(val)) = state.store.kv_get(&key) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&val) {
                result.insert(p.id.clone(), parsed);
            }
        }
    }
    Ok(result)
}

/// 批量保存 provider 优先级（接收有序的 provider_id 列表）
#[tauri::command]
pub async fn save_provider_priorities(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let saved = state.store.load_provider_configs().map_err(map_err)?;
    for (i, id) in ordered_ids.iter().enumerate() {
        let existing = saved.iter().find(|c| c.provider_id == *id);
        let config = sage_types::ProviderConfig {
            provider_id: id.clone(),
            api_key: existing.and_then(|c| c.api_key.clone()),
            model: existing.and_then(|c| c.model.clone()),
            base_url: existing.and_then(|c| c.base_url.clone()),
            enabled: existing.map(|c| c.enabled).unwrap_or(true),
            priority: Some(i as u8),
        };
        state.store.save_provider_config(&config).map_err(map_err)?;
    }
    Ok(())
}
