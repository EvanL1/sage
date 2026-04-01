use sage_core::onboarding::OnboardingState;
use sage_core::profile;
use sage_types::{
    BehaviorPrefs, CommPrefs, Project, ProviderConfig, Stakeholder, UserIdentity, UserProfile,
    WorkContext, WorkSchedule,
};
use serde_json::{json, Value};
use tauri::State;

use super::{get_provider, map_err, trigger_memory_sync};
use crate::AppState;

/// 基于 UserProfile 生成个性化的"第一印象"，存为 insight 记忆，并返回文本
pub async fn generate_first_impression(
    state: &AppState,
    profile: &sage_types::UserProfile,
) -> Option<String> {
    let provider = get_provider(&state.store).ok()?;

    let name = if profile.identity.name.is_empty() {
        "新用户"
    } else {
        &profile.identity.name
    };
    let role = profile.identity.role.as_str();
    let projects: Vec<&str> = profile
        .work_context
        .projects
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let stakeholders: Vec<&str> = profile
        .work_context
        .stakeholders
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    let reporting_line = profile.identity.reporting_line.join(" → ");

    let profile_summary = format!(
        "姓名：{name}\n\
         角色：{role}\n\
         汇报线：{reporting_line}\n\
         在推项目：{}\n\
         关键协作者：{}",
        projects.join("、"),
        stakeholders.join("、"),
    );

    let lang = state.store.prompt_lang();
    let prompt = sage_core::prompts::cmd_first_impression_user(&lang, &profile_summary);
    let system = sage_core::prompts::cmd_first_impression_system(&lang);

    match provider.invoke(&prompt, Some(system)).await {
        Ok(text) => {
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                return None;
            }
            let action_line = format!(
                "save_memory_visible | insight | {trimmed} | confidence:0.9 | visibility:public"
            );
            sage_core::pipeline::actions::execute_single_action(
                &action_line, &["save_memory_visible"], &state.store, "tauri_profile",
            );
            trigger_memory_sync(&state.store);
            tracing::info!("Onboarding first impression 已生成并存储");
            Some(trimmed)
        }
        Err(e) => {
            tracing::warn!("生成 first impression 失败（跳过）: {e}");
            None
        }
    }
}

#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>) -> Result<Option<Value>, String> {
    let profile = state.store.load_profile().map_err(map_err)?;
    match profile {
        Some(p) => Ok(Some(serde_json::to_value(&p).map_err(map_err)?)),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn save_profile(state: State<'_, AppState>, profile: Value) -> Result<(), String> {
    let p: sage_types::UserProfile = serde_json::from_value(profile).map_err(map_err)?;
    state.store.save_profile(&p).map_err(map_err)?;
    Ok(())
}

#[tauri::command]
pub async fn submit_onboarding_step(
    state: State<'_, AppState>,
    data: Value,
) -> Result<Value, String> {
    let completed_profile: Option<(sage_types::UserProfile, String)> = {
        let mut guard = state.onboarding.lock().map_err(map_err)?;

        if guard.is_none() {
            *guard = Some(OnboardingState::new());
        }

        let ob = guard.as_mut().unwrap();
        ob.submit_step(data).map_err(map_err)?;

        if ob.is_complete() {
            let final_profile = guard.take().unwrap().into_profile();
            let sop_preview = profile::generate_sop_lang(&final_profile, &final_profile.identity.prompt_language);
            state.store.save_profile(&final_profile).map_err(map_err)?;
            Some((final_profile, sop_preview))
        } else {
            let (index, total) = ob.progress();
            let sop_preview = ob.preview_sop();
            return Ok(json!({
                "step": format!("{:?}", ob.current_step()),
                "index": index,
                "total": total,
                "sop_preview": sop_preview,
            }));
        }
    };

    if let Some((final_profile, sop_preview)) = completed_profile {
        let first_impression = generate_first_impression(&state, &final_profile).await;
        return Ok(json!({
            "step": "Completed",
            "index": 7,
            "total": 7,
            "sop_preview": sop_preview,
            "first_impression": first_impression,
        }));
    }

    Ok(json!({"step": "Unknown"}))
}

#[tauri::command]
pub async fn reset_onboarding(state: State<'_, AppState>) -> Result<(), String> {
    let mut onboarding = state.onboarding.lock().map_err(map_err)?;
    *onboarding = Some(OnboardingState::new());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn quick_setup(
    state: State<'_, AppState>,
    name: String,
    role: Option<String>,
    reporting_line: Option<Vec<String>>,
    projects: Option<Vec<Value>>,
    stakeholders: Option<Vec<Value>>,
    schedule: Option<Value>,
    communication: Option<Value>,
    api_key: Option<String>,
    provider_id: Option<String>,
    prompt_language: Option<String>,
) -> Result<Value, String> {
    let parsed_projects: Vec<Project> = projects
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    let parsed_stakeholders: Vec<Stakeholder> = stakeholders
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    let parsed_schedule: WorkSchedule = schedule
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let parsed_comm: CommPrefs = communication
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let profile = UserProfile {
        identity: UserIdentity {
            name,
            role: role.unwrap_or_default(),
            reporting_line: reporting_line.unwrap_or_default(),
            primary_language: "zh".into(),
            secondary_language: "en".into(),
            prompt_language: prompt_language.unwrap_or_else(|| "zh".into()),
        },
        work_context: WorkContext {
            projects: parsed_projects,
            stakeholders: parsed_stakeholders,
            tech_stack: Vec::new(),
        },
        communication: parsed_comm,
        schedule: parsed_schedule,
        preferences: BehaviorPrefs::default(),
        negative_rules: Vec::new(),
        sop_version: 1,
    };

    if let (Some(key), Some(pid)) = (api_key, provider_id) {
        let config = ProviderConfig {
            provider_id: pid,
            api_key: Some(key),
            model: None,
            base_url: None,
            enabled: true,
            priority: None,
        };
        state.store.save_provider_config(&config).map_err(map_err)?;
    }

    state.store.save_profile(&profile).map_err(map_err)?;
    Ok(json!({ "status": "ready" }))
}

#[tauri::command]
pub async fn get_system_status(state: State<'_, AppState>) -> Result<Value, String> {
    let has_profile = state.store.load_profile().map_err(map_err)?.is_some();
    let sop_version = state.store.get_sop_version().map_err(map_err)?;

    Ok(json!({
        "status": if has_profile { "ready" } else { "needs_onboarding" },
        "has_profile": has_profile,
        "sop_version": sop_version,
    }))
}
