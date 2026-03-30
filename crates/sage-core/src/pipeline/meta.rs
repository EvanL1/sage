//! Meta Stage — 管线自我进化（参数调优 + Prompt 重写 + UI 生成）

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};

use crate::agent::Agent;
use crate::store::Store;

use super::{harness, CognitiveStage, PipelineContext, StageOutput};

pub struct MetaStage;

#[async_trait]
impl CognitiveStage for MetaStage {
    fn name(&self) -> &str { "meta" }

    async fn run(&self, agent: Agent, store: Arc<Store>, ctx: PipelineContext) -> Result<(StageOutput, PipelineContext)> {
        let mut total = 0;
        total += evolve_pipeline_params(&agent, &store).await.unwrap_or(0);
        total += evolve_prompts(&agent, &store).await.unwrap_or(0);
        total += evolve_ui(&agent, &store).await.unwrap_or(0);
        Ok((StageOutput::Bool(total > 0), ctx))
    }
}

// ─── Phase 1: 参数/结构进化 ──────────────────────────────────────────────────

async fn evolve_pipeline_params(agent: &Agent, store: &Arc<Store>) -> Result<usize> {
    let summary = store.get_pipeline_summary(14)?;
    if summary.is_empty() { return Ok(0); }

    let lines: Vec<String> = summary.iter()
        .map(|(stage, ok, empty, err)| {
            let total = ok + empty + err;
            format!("- {stage}: {total} runs, {ok} ok, {empty} empty, {err} errors")
        })
        .collect();

    let prompt = format!(
        "You are a meta-cognitive optimizer for an AI pipeline.\n\
         Below are the last 14 days of stage execution stats:\n{}\n\n\
         Analyze and output adjustment commands (one per line):\n\
         - DISABLE <stage> | <reason> — if a stage consistently produces no output (>80% empty)\n\
         - INCREASE <stage> <max_iterations> | <reason>\n\
         - DECREASE <stage> <max_iterations> | <reason>\n\
         - ENABLE <stage> | <reason>\n\
         - NONE — if no adjustments needed\n\n\
         Rules:\n\
         - Be conservative: only suggest changes with strong evidence (>5 data points)\n\
         - Never disable 'observer', 'coach', or 'evolution' — they are core\n\
         - Output ONLY commands, nothing else",
        lines.join("\n")
    );

    let text = harness::invoke_text(agent, &prompt, None).await?;
    let mut changes = 0;
    for line in text.lines() {
        let line = line.trim();
        if line == "NONE" || line.is_empty() { continue; }
        changes += apply_meta_command(line, store);
    }
    if changes > 0 { info!("Meta: {changes} pipeline adjustments applied"); }
    Ok(changes)
}

fn apply_meta_command(line: &str, store: &Store) -> usize {
    let parts: Vec<&str> = line.splitn(2, " | ").collect();
    let cmd = parts[0].trim();
    let reason = parts.get(1).map(|s| s.trim()).unwrap_or("");

    if let Some(stage) = cmd.strip_prefix("DISABLE ") {
        let stage = stage.trim();
        if ["observer", "coach", "evolution"].contains(&stage) {
            warn!("Meta: refused to disable core stage '{stage}'");
            return 0;
        }
        let _ = store.set_pipeline_override(stage, "enabled", "false", reason);
        info!("Meta: disabled '{stage}' — {reason}");
        return 1;
    }
    if let Some(rest) = cmd.strip_prefix("INCREASE ") {
        let tokens: Vec<&str> = rest.trim().splitn(2, ' ').collect();
        if tokens.len() == 2 {
            let _ = store.set_pipeline_override(tokens[0], "max_iterations", tokens[1], reason);
            info!("Meta: increased '{}' max_iterations to {} — {reason}", tokens[0], tokens[1]);
            return 1;
        }
    }
    if let Some(rest) = cmd.strip_prefix("DECREASE ") {
        let tokens: Vec<&str> = rest.trim().splitn(2, ' ').collect();
        if tokens.len() == 2 {
            let _ = store.set_pipeline_override(tokens[0], "max_iterations", tokens[1], reason);
            info!("Meta: decreased '{}' max_iterations to {} — {reason}", tokens[0], tokens[1]);
            return 1;
        }
    }
    if let Some(stage) = cmd.strip_prefix("ENABLE ") {
        let _ = store.delete_pipeline_override(stage.trim(), "enabled");
        info!("Meta: re-enabled '{}' — {reason}", stage.trim());
        return 1;
    }
    0
}

// ─── Phase 2: Prompt 自我编辑 ───────────────────────────────────────────────

async fn evolve_prompts(agent: &Agent, store: &Arc<Store>) -> Result<usize> {
    let task_rules = store.get_memories_by_category("calibration_task")?;
    let report_rules = store.get_memories_by_category("calibration")?;
    let mut changes = 0;

    if task_rules.len() >= 3 {
        changes += rewrite_prompt(
            agent, store, "task_intelligence_user",
            &task_rules.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
        ).await.unwrap_or(0);
    }

    for report_type in &["morning", "evening", "weekly"] {
        let tag = format!("[{report_type}]");
        let matching: Vec<&str> = report_rules.iter()
            .filter(|m| m.content.contains(&tag))
            .map(|m| m.content.as_str()).collect();
        if matching.len() >= 3 {
            let prompt_name = match *report_type {
                "morning" => "cmd_dashboard_brief_system",
                "evening" => "observer_user",
                "weekly" => "mirror_weekly_user",
                _ => continue,
            };
            changes += rewrite_prompt(agent, store, prompt_name, &matching).await.unwrap_or(0);
        }
    }
    Ok(changes)
}

async fn rewrite_prompt(agent: &Agent, store: &Store, name: &str, rules: &[&str]) -> Result<usize> {
    let lang = store.prompt_lang();
    let current = crate::prompts::load_prompt(name, &lang);
    if current.is_empty() { return Ok(0); }

    let rules_text = rules.join("\n");
    if rules.iter().all(|r| current.contains(r)) { return Ok(0); }

    let prompt = format!(
        "You are a prompt engineer. Improve this prompt by incorporating learned rules.\n\n\
         ## Current prompt:\n```\n{current}\n```\n\n\
         ## Rules to incorporate:\n{rules_text}\n\n\
         Instructions:\n\
         - Integrate rules NATURALLY into the existing structure\n\
         - Preserve ALL template variables ({{tasks_text}}, {{actions_text}}, etc.)\n\
         - Keep it concise — merge overlapping rules\n\
         - Output ONLY the improved prompt, nothing else"
    );

    let new_prompt = harness::invoke_raw(agent, &prompt, None).await?;
    if new_prompt.is_empty() || new_prompt.len() < current.len() / 2  {
        warn!("Meta: prompt rewrite for '{name}' rejected (too short)");
        return Ok(0);
    }

    let l = if lang == "en" { "en" } else { "zh" };
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::path::Path::new(&home).join(".sage/prompts").join(l);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.md"));

    if path.exists() {
        let _ = std::fs::copy(&path, dir.join(format!("{name}.md.bak")));
    }
    std::fs::write(&path, new_prompt)?;
    info!("Meta: rewrote prompt '{}' at {}", name, path.display());

    for rule in rules {
        let _ = store.save_memory("calibration_baked", &format!("[baked:{name}] {rule}"), "meta", 0.5);
    }
    Ok(1)
}

// ─── Phase 3: UI 页面进化 ───────────────────────────────────────────────────

async fn evolve_ui(agent: &Agent, store: &Arc<Store>) -> Result<usize> {
    let existing = store.list_custom_pages(100)?;
    let auto_pages: Vec<_> = existing.iter().filter(|p| p.1.starts_with("[auto]")).collect();
    if let Some(latest) = auto_pages.last() {
        let week_ago = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(7))
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y-%m-%d").to_string();
        if latest.2 > week_ago { return Ok(0); }
    }

    let memory_count = store.count_memories().unwrap_or(0);
    let task_summary = store.get_pipeline_summary(30)?;
    let obs_count = store.count_observations_since(
        &chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(30))
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y-%m-%d").to_string(),
    ).unwrap_or(0);

    if memory_count < 10 && obs_count < 5 { return Ok(0); }

    let stats = task_summary.iter()
        .map(|(s, ok, empty, err)| format!("- {s}: ok={ok}, empty={empty}, error={err}"))
        .collect::<Vec<_>>().join("\n");

    let prompt = format!(
        "You are a UI designer for Sage.\n\
         Generate a markdown page using: <Stat>, <StatRow>, <DataTable>, <Chart>, <KanbanBoard>, <Timeline>, <Progress>.\n\n\
         Data: {memory_count} memories, {obs_count} observations (30d).\nPipeline (30d):\n{stats}\n\n\
         Design ONE focused insight page. Output ONLY markdown. Start with `# Title`."
    );

    let md = harness::invoke_raw(agent, &prompt, None).await?;
    if md.is_empty() || !md.starts_with('#') { return Ok(0); }

    let title = md.lines().next().unwrap_or("Auto Insight").trim_start_matches('#').trim();
    store.save_custom_page(&format!("[auto] {title}"), &md)?;
    info!("Meta UI: generated page '[auto] {title}'");
    Ok(1)
}
