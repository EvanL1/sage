//! 认知管线 — Agents-as-Tools 模式
//!
//! 每个认知阶段实现 `CognitiveStage` trait，注册到 `CognitivePipeline` 中。
//! 管线顺序由 config.toml 的 `[pipeline]` 段驱动，不再硬编码。

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::{error, info, warn};

use crate::agent::Agent;
use crate::memory_evolution::EvolutionResult;
use crate::store::Store;

// ─── Trait + Output ──────────────────────────────────────────────────────────

/// 认知管线阶段的统一输出
pub enum StageOutput {
    Bool(bool),
    Evolution(EvolutionResult),
}

/// 每个认知阶段必须实现的接口
#[async_trait]
pub trait CognitiveStage: Send + Sync {
    fn name(&self) -> &str;
    async fn run(&self, agent: &Agent, store: &Arc<Store>) -> Result<StageOutput>;
}

// ─── Stage Wrappers（宏生成 bool 类型） ──────────────────────────────────────

macro_rules! bool_stage {
    ($struct_name:ident, $label:literal, $fn_path:expr) => {
        pub struct $struct_name;
        #[async_trait]
        impl CognitiveStage for $struct_name {
            fn name(&self) -> &str { $label }
            async fn run(&self, agent: &Agent, store: &Arc<Store>) -> Result<StageOutput> {
                $fn_path(agent, store).await.map(StageOutput::Bool)
            }
        }
    };
}

bool_stage!(ObserverStage, "observer", crate::observer::annotate);
bool_stage!(CoachStage, "coach", crate::coach::learn);
bool_stage!(MirrorStage, "mirror", crate::mirror::reflect);
bool_stage!(QuestionerStage, "questioner", crate::questioner::ask);
bool_stage!(PersonObserverStage, "person_observer", crate::person_observer::extract_persons);
bool_stage!(CalibratorStage, "calibrator", crate::calibrator::reflect_patterns);
bool_stage!(StrategistStage, "strategist", crate::strategist::strategize);
bool_stage!(MirrorWeeklyStage, "mirror_weekly", crate::mirror::mirror_weekly);

/// Memory Evolution 单独实现（返回 EvolutionResult）
pub struct EvolutionStage;
#[async_trait]
impl CognitiveStage for EvolutionStage {
    fn name(&self) -> &str { "evolution" }
    async fn run(&self, agent: &Agent, store: &Arc<Store>) -> Result<StageOutput> {
        crate::memory_evolution::evolve(agent, store).await.map(StageOutput::Evolution)
    }
}

// ─── Pipeline Registry ──────────────────────────────────────────────────────

/// 每个 stage 的可选配置覆盖
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StageConfig {
    pub max_iterations: Option<usize>,
}

pub struct CognitivePipeline {
    stages: HashMap<String, Box<dyn CognitiveStage>>,
    evening_order: Vec<String>,
    weekly_order: Vec<String>,
    stage_configs: HashMap<String, StageConfig>,
}

impl CognitivePipeline {
    pub fn new(
        evening: Vec<String>,
        weekly: Vec<String>,
        configs: HashMap<String, StageConfig>,
    ) -> Self {
        Self {
            stages: HashMap::new(),
            evening_order: evening,
            weekly_order: weekly,
            stage_configs: configs,
        }
    }

    pub fn register(&mut self, stage: Box<dyn CognitiveStage>) {
        self.stages.insert(stage.name().to_string(), stage);
    }

    /// 执行晚间认知管线
    pub async fn run_evening(&self, agent: &Agent, store: &Arc<Store>) {
        self.run_sequence(&self.evening_order, "evening", agent, store).await;
    }

    /// 执行周报管线
    pub async fn run_weekly(&self, agent: &Agent, store: &Arc<Store>) {
        self.run_sequence(&self.weekly_order, "weekly", agent, store).await;
    }

    async fn run_sequence(&self, order: &[String], pipeline_name: &str, agent: &Agent, store: &Arc<Store>) {
        for name in order {
            // 检查运行时覆盖：enabled=false 则跳过
            if is_stage_disabled(store, name) {
                info!("{name}: skipped (disabled by self-evolution)");
                continue;
            }

            let Some(stage) = self.stages.get(name) else {
                warn!("Pipeline: unknown stage '{name}', skipping");
                continue;
            };

            let stage_agent = self.make_stage_agent(name, agent, store);
            let start = std::time::Instant::now();

            let (outcome, log_msg) = match stage.run(&stage_agent, store).await {
                Ok(StageOutput::Bool(true)) => ("ok", format!("{name}: completed")),
                Ok(StageOutput::Bool(false)) => ("empty", String::new()),
                Ok(StageOutput::Evolution(r)) => {
                    let total = r.consolidated + r.condensed + r.linked + r.decayed + r.promoted;
                    if total > 0 {
                        ("ok", format!(
                            "{name}: consolidated={}, condensed={}, linked={}, decayed={}, promoted={}",
                            r.consolidated, r.condensed, r.linked, r.decayed, r.promoted
                        ))
                    } else {
                        ("empty", String::new())
                    }
                }
                Err(e) => {
                    error!("{name} failed: {e}");
                    ("error", String::new())
                }
            };

            if !log_msg.is_empty() {
                info!("{log_msg}");
            }

            // 记录执行日志（供 Meta stage 分析）
            let elapsed = start.elapsed().as_millis() as i64;
            let _ = store.log_pipeline_run(name, pipeline_name, outcome, elapsed);
        }
    }

    /// 为 stage 创建独立的 Agent（clone = 独立计数器）
    /// 优先级：运行时覆盖 > config.toml > 默认值
    fn make_stage_agent(&self, name: &str, base: &Agent, store: &Arc<Store>) -> Agent {
        let mut agent = base.clone();
        // 1. config.toml 静态覆盖
        if let Some(cfg) = self.stage_configs.get(name) {
            if let Some(mi) = cfg.max_iterations {
                agent.set_max_iterations(mi);
            }
        }
        // 2. 运行时覆盖（self-evolution 写入的，优先级更高）
        if let Ok(overrides) = store.get_pipeline_overrides(name) {
            for o in &overrides {
                if o.key == "max_iterations" {
                    if let Ok(mi) = o.value.parse::<usize>() {
                        agent.set_max_iterations(mi);
                    }
                }
            }
        }
        agent
    }
}

/// 检查 stage 是否被运行时覆盖禁用
fn is_stage_disabled(store: &Store, name: &str) -> bool {
    store
        .get_pipeline_overrides(name)
        .unwrap_or_default()
        .iter()
        .any(|o| o.key == "enabled" && o.value == "false")
}

/// 构建完整管线，注册所有内置 stage
pub fn build_pipeline(
    evening: Vec<String>,
    weekly: Vec<String>,
    configs: HashMap<String, StageConfig>,
) -> CognitivePipeline {
    let mut p = CognitivePipeline::new(evening, weekly, configs);
    p.register(Box::new(ObserverStage));
    p.register(Box::new(CoachStage));
    p.register(Box::new(MirrorStage));
    p.register(Box::new(QuestionerStage));
    p.register(Box::new(EvolutionStage));
    p.register(Box::new(PersonObserverStage));
    p.register(Box::new(CalibratorStage));
    p.register(Box::new(StrategistStage));
    p.register(Box::new(MirrorWeeklyStage));
    p.register(Box::new(MetaStage));
    p
}

// ─── Meta Stage（自我进化）──────────────────────────────────────────────────

/// Meta 阶段：两层自我进化
/// 1. 参数/结构进化：分析执行日志，调整 max_iterations / 启停 stage
/// 2. Prompt 进化：检测校准规则积累，重写 prompt 模板文件
pub struct MetaStage;

#[async_trait]
impl CognitiveStage for MetaStage {
    fn name(&self) -> &str { "meta" }

    async fn run(&self, agent: &Agent, store: &Arc<Store>) -> Result<StageOutput> {
        let mut total_changes = 0;

        // Phase 1: 参数/结构进化
        total_changes += evolve_pipeline_params(agent, store).await.unwrap_or(0);

        // Phase 2: Prompt 自我编辑
        total_changes += evolve_prompts(agent, store).await.unwrap_or(0);

        // Phase 3: 前端 UI 进化（自动生成数据页面）
        total_changes += evolve_ui(agent, store).await.unwrap_or(0);

        Ok(StageOutput::Bool(total_changes > 0))
    }
}

/// Phase 1: 分析管线执行历史 → DISABLE/INCREASE/DECREASE/ENABLE
async fn evolve_pipeline_params(agent: &Agent, store: &Arc<Store>) -> Result<usize> {
    let summary = store.get_pipeline_summary(14)?;
    if summary.is_empty() {
        return Ok(0);
    }

    let mut lines = Vec::new();
    for (stage, ok, empty, err) in &summary {
        let total = ok + empty + err;
        lines.push(format!(
            "- {stage}: {total} runs, {ok} ok, {empty} empty, {err} errors"
        ));
    }

    let prompt = format!(
        "You are a meta-cognitive optimizer for an AI pipeline.\n\
         Below are the last 14 days of stage execution stats:\n{}\n\n\
         Analyze and output adjustment commands (one per line):\n\
         - DISABLE <stage> | <reason> — if a stage consistently produces no output (>80% empty)\n\
         - INCREASE <stage> <max_iterations> | <reason> — if a stage frequently errors from iteration limits\n\
         - DECREASE <stage> <max_iterations> | <reason> — if a stage uses few iterations but has high budget\n\
         - ENABLE <stage> | <reason> — if a previously disabled stage should be re-enabled\n\
         - NONE — if no adjustments needed\n\n\
         Rules:\n\
         - Be conservative: only suggest changes with strong evidence (>5 data points)\n\
         - Never disable 'observer', 'coach', or 'evolution' — they are core\n\
         - Output ONLY commands, nothing else",
        lines.join("\n")
    );

    let resp = agent.invoke(&prompt, None).await?;
    let mut changes = 0;
    for line in resp.text.lines() {
        let line = line.trim();
        if line == "NONE" || line.is_empty() { continue; }
        changes += apply_meta_command(line, store);
    }
    if changes > 0 {
        info!("Meta: {changes} pipeline adjustments applied");
    }
    Ok(changes)
}

/// Phase 2: 检测校准规则积累 → 重写 prompt 模板
/// 当某个 prompt 积累了 ≥3 条校准规则时，将规则"烘焙"进 prompt 文件
async fn evolve_prompts(agent: &Agent, store: &Arc<Store>) -> Result<usize> {
    // 收集所有 calibration 规则，按关联的 stage/prompt 分组
    let task_rules = store.get_memories_by_category("calibration_task")?;
    let report_rules = store.get_memories_by_category("calibration")?;

    let mut changes = 0;

    // Task intelligence prompt 进化
    if task_rules.len() >= 3 {
        changes += rewrite_prompt(
            agent, store,
            "task_intelligence_user",
            &task_rules.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
        ).await.unwrap_or(0);
    }

    // Report prompts 进化（按类型分组）
    for report_type in &["morning", "evening", "weekly"] {
        let tag = format!("[{report_type}]");
        let matching: Vec<&str> = report_rules
            .iter()
            .filter(|m| m.content.contains(&tag))
            .map(|m| m.content.as_str())
            .collect();
        if matching.len() >= 3 {
            // 找到对应的 prompt 名称
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

/// 将校准规则烘焙进 prompt 模板文件
async fn rewrite_prompt(
    agent: &Agent,
    store: &Store,
    prompt_name: &str,
    rules: &[&str],
) -> Result<usize> {
    let lang = store.prompt_lang();
    let current = crate::prompts::load_prompt(prompt_name, &lang);
    if current.is_empty() {
        return Ok(0);
    }

    // 检查是否已经烘焙过这些规则（避免重复）
    let rules_text = rules.join("\n");
    let already_baked = rules.iter().all(|r| current.contains(r));
    if already_baked {
        return Ok(0);
    }

    let prompt = format!(
        "You are a prompt engineer. Your task is to improve a prompt template by incorporating learned rules.\n\n\
         ## Current prompt:\n```\n{current}\n```\n\n\
         ## Rules to incorporate (from user feedback):\n{rules_text}\n\n\
         Instructions:\n\
         - Integrate the rules NATURALLY into the prompt's existing Rules/Instructions section\n\
         - Do NOT just append them — weave them into the existing structure\n\
         - Preserve ALL existing template variables (like {{tasks_text}}, {{actions_text}}, etc.)\n\
         - Preserve the prompt's overall structure, tone, and format\n\
         - Keep it concise — merge overlapping rules\n\
         - Output ONLY the improved prompt text, nothing else (no code fences, no explanation)"
    );

    let resp = agent.invoke(&prompt, None).await?;
    let new_prompt = resp.text.trim();

    if new_prompt.is_empty() || new_prompt.len() < current.len() / 2 {
        warn!("Meta: prompt rewrite for '{prompt_name}' rejected (too short or empty)");
        return Ok(0);
    }

    // 写入 ~/.sage/prompts/{lang}/{name}.md（热加载覆盖）
    let l = if lang == "en" { "en" } else { "zh" };
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::path::Path::new(&home).join(".sage/prompts").join(l);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{prompt_name}.md"));

    // 备份旧文件（如果存在）
    if path.exists() {
        let backup = dir.join(format!("{prompt_name}.md.bak"));
        let _ = std::fs::copy(&path, &backup);
    }

    std::fs::write(&path, new_prompt)?;
    info!("Meta: rewrote prompt '{}' at {}", prompt_name, path.display());

    // 标记规则已烘焙（归档 calibration memories，避免下次重复）
    for rule in rules {
        // 在规则内容前加 [baked] 标记
        let baked = format!("[baked:{prompt_name}] {rule}");
        let _ = store.save_memory("calibration_baked", &baked, "meta", 0.5);
    }

    Ok(1)
}

fn apply_meta_command(line: &str, store: &Store) -> usize {
    let parts: Vec<&str> = line.splitn(2, " | ").collect();
    let cmd = parts[0].trim();
    let reason = parts.get(1).map(|s| s.trim()).unwrap_or("");

    if let Some(stage) = cmd.strip_prefix("DISABLE ") {
        let stage = stage.trim();
        // 安全检查：核心 stage 不能禁用
        if matches!(stage, "observer" | "coach" | "evolution") {
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
            info!("Meta: increased '{}'  max_iterations to {} — {reason}", tokens[0], tokens[1]);
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

// ─── Phase 3: 前端 UI 进化 ───────────────────────────────────────────────────

/// 分析用户行为模式，自动生成数据可视化页面
/// 使用已注册的 DynamicPage 组件：Stat, DataTable, Chart, KanbanBoard, Timeline
async fn evolve_ui(agent: &Agent, store: &Arc<Store>) -> Result<usize> {
    // 每 7 天最多生成一个页面（避免刷屏）
    let existing = store.list_custom_pages(100)?;
    // list_custom_pages 返回 (id, title, created_at, updated_at)
    let auto_pages: Vec<_> = existing.iter().filter(|p| p.1.starts_with("[auto]")).collect();
    if let Some(latest) = auto_pages.last() {
        let week_ago = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(7))
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y-%m-%d")
            .to_string();
        if latest.2 > week_ago {
            return Ok(0); // 上次生成不足 7 天
        }
    }

    // 收集数据指标
    let memory_count = store.count_memories().unwrap_or(0);
    let task_summary = store.get_pipeline_summary(30)?;
    let observations_count = store.count_observations_since(
        &chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(30))
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y-%m-%d")
            .to_string(),
    ).unwrap_or(0);

    if memory_count < 10 && observations_count < 5 {
        return Ok(0); // 数据不足
    }

    let pipeline_stats = task_summary
        .iter()
        .map(|(s, ok, empty, err)| format!("- {s}: ok={ok}, empty={empty}, error={err}"))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are a UI designer for a personal AI system called Sage.\n\
         Generate a markdown page using these components:\n\
         - `<Stat label=\"...\" value=\"...\" />` — single metric card\n\
         - `<StatRow>...</StatRow>` — horizontal row of Stats\n\
         - `<DataTable source=\"memories\" filter=\"category=pattern\" columns=\"content,confidence\" />` — data table\n\
         - `<Chart source=\"memories\" field=\"category\" type=\"pie\" label=\"...\" />` — pie/bar/line chart\n\
         - `<KanbanBoard source=\"tasks\" groupBy=\"status\" titleField=\"content\" />` — kanban board\n\
         - `<Timeline source=\"memories\" dateField=\"created_at\" titleField=\"content\" />` — timeline view\n\
         - `<Progress value=\"...\" max=\"...\" label=\"...\" />` — progress bar\n\n\
         Current data:\n\
         - Total memories: {memory_count}\n\
         - Recent observations (30d): {observations_count}\n\
         - Pipeline stats (30d):\n{pipeline_stats}\n\n\
         Design a useful insight page. Output ONLY the markdown (with embedded components), nothing else.\n\
         Start with a `# Title` header. Keep it focused on ONE insight theme.\n\
         Do NOT wrap in code fences."
    );

    let resp = agent.invoke(&prompt, None).await?;
    let markdown = resp.text.trim();

    if markdown.is_empty() || !markdown.starts_with('#') {
        return Ok(0);
    }

    // 提取标题
    let title = markdown
        .lines()
        .next()
        .unwrap_or("Auto Insight")
        .trim_start_matches('#')
        .trim();
    let auto_title = format!("[auto] {title}");

    store.save_custom_page(&auto_title, markdown)?;
    info!("Meta UI: generated page '{auto_title}'");
    Ok(1)
}

// ─── 默认管线顺序 ───────────────────────────────────────────────────────────

pub fn default_evening_stages() -> Vec<String> {
    vec![
        "observer", "coach", "mirror", "questioner",
        "evolution", "person_observer", "calibrator", "meta",
    ].into_iter().map(String::from).collect()
}

pub fn default_weekly_stages() -> Vec<String> {
    vec!["strategist", "mirror_weekly"]
        .into_iter().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock stage 用于测试
    struct MockStage {
        label: String,
        log: Arc<tokio::sync::Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl CognitiveStage for MockStage {
        fn name(&self) -> &str { &self.label }
        async fn run(&self, _agent: &Agent, _store: &Arc<Store>) -> Result<StageOutput> {
            self.log.lock().await.push(self.label.clone());
            Ok(StageOutput::Bool(true))
        }
    }

    fn mock_agent() -> Agent {
        Agent::new(crate::AgentConfig::default())
    }

    fn mock_store() -> Arc<Store> {
        Arc::new(Store::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn pipeline_runs_in_order() {
        let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let mut p = CognitivePipeline::new(
            vec!["a".into(), "b".into(), "c".into()],
            vec![],
            HashMap::new(),
        );
        for name in ["a", "b", "c"] {
            p.register(Box::new(MockStage {
                label: name.into(),
                log: Arc::clone(&log),
            }));
        }
        p.run_evening(&mock_agent(), &mock_store()).await;
        assert_eq!(*log.lock().await, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn pipeline_skips_unknown_stage() {
        let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let mut p = CognitivePipeline::new(
            vec!["a".into(), "unknown".into(), "b".into()],
            vec![],
            HashMap::new(),
        );
        for name in ["a", "b"] {
            p.register(Box::new(MockStage {
                label: name.into(),
                log: Arc::clone(&log),
            }));
        }
        p.run_evening(&mock_agent(), &mock_store()).await;
        // "unknown" 被跳过，a 和 b 正常执行
        assert_eq!(*log.lock().await, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn pipeline_empty_order_is_noop() {
        let p = CognitivePipeline::new(vec![], vec![], HashMap::new());
        // 不 panic，正常返回
        p.run_evening(&mock_agent(), &mock_store()).await;
    }

    #[test]
    fn default_evening_has_eight_stages() {
        assert_eq!(default_evening_stages().len(), 8);
        assert_eq!(default_evening_stages().last().unwrap(), "meta");
    }

    #[test]
    fn default_weekly_has_two_stages() {
        assert_eq!(default_weekly_stages().len(), 2);
    }
}
