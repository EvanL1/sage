//! 认知管线 — Agents-as-Tools 模式
//!
//! 每个认知阶段实现 `CognitiveStage` trait，注册到 `CognitivePipeline` 中。
//! 管线顺序由 config.toml 的 `[pipeline]` 段驱动，不再硬编码。

pub mod actions;
pub mod stages;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::{error, info, warn};

use crate::agent::Agent;
use crate::memory_evolution::EvolutionResult;
use crate::store::Store;

// Re-export 常用类型
pub use stages::{
    ObserverStage, CoachStage, MirrorStage, QuestionerStage,
    PersonObserverStage, CalibratorStage, StrategistStage,
    MirrorWeeklyStage, EvolutionStage, UserDefinedStage, PresetCtxKey,
};

// ─── Pipeline Context（类型化 Stage I/O 契约）─────────────────────────────────

/// 管线共享上下文 — 每个 stage 读上游结果、写自己的结果
#[derive(Debug, Clone, Default)]
pub struct PipelineContext {
    pub observer: Option<ObserverOutput>,
    pub coach: Option<CoachOutput>,
    pub mirror: Option<MirrorOutput>,
    pub questioner: Option<QuestionerOutput>,
    pub evolution: Option<EvolutionOutput>,
    pub stage_results: Vec<StageResult>,
}

impl PipelineContext {
    pub fn summary(&self) -> String {
        let total = self.stage_results.len();
        let ok = self.stage_results.iter().filter(|r| r.status == StageStatus::Ok).count();
        let degraded = self.stage_results.iter().filter(|r| matches!(r.status, StageStatus::Degraded(_))).count();
        let errors = self.stage_results.iter().filter(|r| matches!(r.status, StageStatus::Error(_))).count();
        let empty = self.stage_results.iter().filter(|r| r.status == StageStatus::Empty).count();
        let skipped = self.stage_results.iter().filter(|r| r.status == StageStatus::Skipped).count();

        let mut parts = vec![format!("{ok} ok")];
        if empty > 0 { parts.push(format!("{empty} empty")); }
        if degraded > 0 { parts.push(format!("{degraded} degraded")); }
        if errors > 0 { parts.push(format!("{errors} errors")); }
        if skipped > 0 { parts.push(format!("{skipped} skipped")); }
        format!("Pipeline: {total} stages — {}", parts.join(", "))
    }
}

#[derive(Debug, Clone)]
pub struct StageResult {
    pub name: String,
    pub status: StageStatus,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StageStatus {
    Ok,
    Empty,
    Degraded(String),
    Skipped,
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct ObserverOutput { pub notes: Vec<String> }

#[derive(Debug, Clone, Default)]
pub struct CoachOutput {
    pub insights: Vec<String>,
    pub observations_archived: usize,
    pub degraded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MirrorOutput { pub reflection: String, pub notified: bool }

#[derive(Debug, Clone, Default)]
pub struct QuestionerOutput { pub question: String, pub is_resurface: bool }

#[derive(Debug, Clone, Default)]
pub struct EvolutionOutput {
    pub consolidated: usize,
    pub condensed: usize,
    pub linked: usize,
    pub decayed: usize,
    pub promoted: usize,
}

// ─── Trait + Output ──────────────────────────────────────────────────────────

pub enum StageOutput {
    Bool(bool),
    Evolution(EvolutionResult),
}

#[async_trait]
pub trait CognitiveStage: Send + Sync {
    fn name(&self) -> &str;
    async fn run(&self, agent: &Agent, store: &Arc<Store>, ctx: &mut PipelineContext) -> Result<StageOutput>;
}

// ─── Pipeline Registry ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StageConfig {
    pub max_iterations: Option<usize>,
}

pub struct CognitivePipeline {
    stages: HashMap<String, Box<dyn CognitiveStage>>,
    adj: HashMap<String, Vec<String>>,
    rev_adj: HashMap<String, Vec<String>>,
    all_nodes: Vec<String>,
    stage_configs: HashMap<String, StageConfig>,
    pub core_stages: Vec<String>,
}

impl CognitivePipeline {
    pub fn new(config: &crate::config::PipelineConfig) -> Self {
        use std::collections::HashSet;

        let mut node_set: HashSet<String> = config.evening.iter().cloned().collect();
        for s in &config.weekly { node_set.insert(s.clone()); }
        for e in &config.edges {
            node_set.insert(e.from.clone());
            node_set.insert(e.to.clone());
        }

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        let mut rev_adj: HashMap<String, Vec<String>> = HashMap::new();
        for e in &config.edges {
            adj.entry(e.from.clone()).or_default().push(e.to.clone());
            rev_adj.entry(e.to.clone()).or_default().push(e.from.clone());
        }

        if config.edges.is_empty() {
            for win in config.evening.windows(2) {
                adj.entry(win[0].clone()).or_default().push(win[1].clone());
                rev_adj.entry(win[1].clone()).or_default().push(win[0].clone());
            }
            for win in config.weekly.windows(2) {
                adj.entry(win[0].clone()).or_default().push(win[1].clone());
                rev_adj.entry(win[1].clone()).or_default().push(win[0].clone());
            }
        }

        let all_nodes = config.evening_order();
        let core_stages = config.evening.iter().take(2).cloned()
            .chain(std::iter::once("evolution".into()))
            .collect();

        Self {
            stages: HashMap::new(),
            adj, rev_adj, all_nodes,
            stage_configs: config.stages.clone(),
            core_stages,
        }
    }

    pub fn register(&mut self, stage: Box<dyn CognitiveStage>) {
        self.stages.insert(stage.name().to_string(), stage);
    }

    pub async fn run(&self, pipeline_name: &str, nodes: &[String], agent: &Agent, store: &Arc<Store>) -> PipelineContext {
        use std::collections::{HashSet, VecDeque};

        let mut ctx = PipelineContext::default();
        let node_set: HashSet<&str> = nodes.iter().map(|s| s.as_str()).collect();

        let mut in_degree: HashMap<String, usize> = nodes.iter().map(|n| (n.clone(), 0)).collect();
        for n in nodes {
            if let Some(preds) = self.rev_adj.get(n) {
                let count = preds.iter().filter(|p| node_set.contains(p.as_str())).count();
                in_degree.insert(n.clone(), count);
            }
        }

        let mut ready: VecDeque<String> = nodes.iter()
            .filter(|n| in_degree.get(*n).copied().unwrap_or(0) == 0)
            .cloned()
            .collect();

        let mut completed: HashSet<String> = HashSet::new();

        while let Some(name) = ready.pop_front() {
            if completed.contains(&name) || !node_set.contains(name.as_str()) { continue; }

            if is_stage_disabled(store, &name) {
                info!("{name}: skipped (disabled by self-evolution)");
                ctx.stage_results.push(StageResult {
                    name: name.clone(), status: StageStatus::Skipped, duration_ms: 0,
                });
                completed.insert(name.clone());
                self.push_ready(&name, &node_set, &mut in_degree, &completed, &mut ready);
                continue;
            }

            let Some(stage) = self.stages.get(&name) else {
                warn!("Pipeline: unknown stage '{name}', skipping");
                completed.insert(name.clone());
                self.push_ready(&name, &node_set, &mut in_degree, &completed, &mut ready);
                continue;
            };

            let stage_agent = self.make_stage_agent(&name, agent, store);
            let start = std::time::Instant::now();

            let (outcome, log_msg, status) = match stage.run(&stage_agent, store, &mut ctx).await {
                Ok(StageOutput::Bool(true)) => ("ok", format!("{name}: completed"), StageStatus::Ok),
                Ok(StageOutput::Bool(false)) => ("empty", String::new(), StageStatus::Empty),
                Ok(StageOutput::Evolution(r)) => {
                    let total = r.consolidated + r.condensed + r.linked + r.decayed + r.promoted;
                    if total > 0 {
                        ("ok", format!(
                            "{name}: consolidated={}, condensed={}, linked={}, decayed={}, promoted={}",
                            r.consolidated, r.condensed, r.linked, r.decayed, r.promoted
                        ), StageStatus::Ok)
                    } else {
                        ("empty", String::new(), StageStatus::Empty)
                    }
                }
                Err(e) => {
                    error!("{name} failed: {e}");
                    ("error", String::new(), StageStatus::Error(e.to_string()))
                }
            };

            if !log_msg.is_empty() { info!("{log_msg}"); }
            let elapsed = start.elapsed().as_millis() as u64;
            ctx.stage_results.push(StageResult { name: name.clone(), status, duration_ms: elapsed });
            let _ = store.log_pipeline_run(&name, pipeline_name, outcome, elapsed as i64);
            completed.insert(name.clone());
            self.push_ready(&name, &node_set, &mut in_degree, &completed, &mut ready);
        }
        ctx
    }

    pub async fn run_evening(&self, agent: &Agent, store: &Arc<Store>) -> PipelineContext {
        self.run("evening", &self.all_nodes.clone(), agent, store).await
    }

    pub async fn run_weekly(&self, agent: &Agent, store: &Arc<Store>) -> PipelineContext {
        let weekly: Vec<String> = self.stages.keys()
            .filter(|k| k.as_str() == "strategist" || k.as_str() == "mirror_weekly")
            .cloned().collect();
        if weekly.is_empty() { return PipelineContext::default(); }
        self.run("weekly", &weekly, agent, store).await
    }

    fn push_ready(
        &self, node: &str, node_set: &std::collections::HashSet<&str>,
        in_degree: &mut HashMap<String, usize>,
        completed: &std::collections::HashSet<String>,
        ready: &mut std::collections::VecDeque<String>,
    ) {
        if let Some(successors) = self.adj.get(node) {
            for next in successors {
                if !node_set.contains(next.as_str()) || completed.contains(next) { continue; }
                let d = in_degree.entry(next.clone()).or_default();
                if *d > 0 { *d -= 1; }
                if *d == 0 { ready.push_back(next.clone()); }
            }
        }
    }

    fn make_stage_agent(&self, name: &str, base: &Agent, store: &Arc<Store>) -> Agent {
        let mut agent = base.clone();
        if let Some(cfg) = self.stage_configs.get(name) {
            if let Some(mi) = cfg.max_iterations { agent.set_max_iterations(mi); }
        }
        if let Ok(overrides) = store.get_pipeline_overrides(name) {
            for o in &overrides {
                if o.key == "max_iterations" {
                    if let Ok(mi) = o.value.parse::<usize>() { agent.set_max_iterations(mi); }
                }
            }
        }
        agent
    }
}

fn is_stage_disabled(store: &Store, name: &str) -> bool {
    store.get_pipeline_overrides(name).unwrap_or_default()
        .iter().any(|o| o.key == "enabled" && o.value == "false")
}

// ─── build_pipeline ──────────────────────────────────────────────────────────

pub fn build_pipeline(config: &crate::config::PipelineConfig, store: &Store) -> CognitivePipeline {
    let mut p = CognitivePipeline::new(config);

    let custom_stages = store.list_custom_stages().unwrap_or_default();
    let preset_names: Vec<String> = custom_stages.iter()
        .filter(|cs| cs.is_preset && cs.enabled)
        .map(|cs| cs.name.clone())
        .collect();

    // 内置 stage：如果有同名预设则跳过
    if !preset_names.contains(&"observer".into()) { p.register(Box::new(ObserverStage)); }
    if !preset_names.contains(&"coach".into()) { p.register(Box::new(CoachStage)); }
    if !preset_names.contains(&"mirror".into()) { p.register(Box::new(MirrorStage)); }
    if !preset_names.contains(&"questioner".into()) { p.register(Box::new(QuestionerStage)); }
    p.register(Box::new(EvolutionStage));
    if !preset_names.contains(&"person_observer".into()) { p.register(Box::new(PersonObserverStage)); }
    if !preset_names.contains(&"calibrator".into()) { p.register(Box::new(CalibratorStage)); }
    if !preset_names.contains(&"strategist".into()) { p.register(Box::new(StrategistStage)); }
    p.register(Box::new(MirrorWeeklyStage));
    p.register(Box::new(MetaStage));

    // 自定义/预设 stage
    for cs in &custom_stages {
        if !cs.enabled { continue; }
        let ctx_key = match cs.name.as_str() {
            "observer" => Some(PresetCtxKey::Observer),
            "coach" => Some(PresetCtxKey::Coach),
            "mirror" => Some(PresetCtxKey::Mirror),
            "questioner" => Some(PresetCtxKey::Questioner),
            _ => None,
        };
        p.register(Box::new(UserDefinedStage::new(
            cs.name.clone(), cs.prompt.clone(), cs.output_format.clone(),
            cs.available_actions.clone(), cs.allowed_inputs.clone(),
            cs.max_actions, cs.pre_condition.clone(),
            cs.archive_observations, ctx_key,
        )));
        if !cs.is_preset && !cs.insert_after.is_empty() {
            p.adj.entry(cs.insert_after.clone()).or_default().push(cs.name.clone());
            p.rev_adj.entry(cs.name.clone()).or_default().push(cs.insert_after.clone());
            if !p.all_nodes.contains(&cs.name) {
                if let Some(pos) = p.all_nodes.iter().position(|s| s == &cs.insert_after) {
                    p.all_nodes.insert(pos + 1, cs.name.clone());
                } else {
                    p.all_nodes.push(cs.name.clone());
                }
            }
        }
    }
    p
}

// ─── Meta Stage ──────────────────────────────────────────────────────────────

mod meta;
pub use meta::MetaStage;

// ─── 默认管线顺序 ───────────────────────────────────────────────────────────

pub fn default_evening_stages() -> Vec<String> {
    vec!["observer", "coach", "mirror", "questioner",
         "evolution", "person_observer", "calibrator", "meta"]
        .into_iter().map(String::from).collect()
}

pub fn default_weekly_stages() -> Vec<String> {
    vec!["strategist", "mirror_weekly"].into_iter().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockStage {
        label: String,
        log: Arc<tokio::sync::Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl CognitiveStage for MockStage {
        fn name(&self) -> &str { &self.label }
        async fn run(&self, _: &Agent, _: &Arc<Store>, _: &mut PipelineContext) -> Result<StageOutput> {
            self.log.lock().await.push(self.label.clone());
            Ok(StageOutput::Bool(true))
        }
    }

    fn mock_agent() -> Agent { Agent::new(crate::AgentConfig::default()) }
    fn mock_store() -> Arc<Store> { Arc::new(Store::open_in_memory().unwrap()) }

    #[tokio::test]
    async fn pipeline_runs_in_order() {
        let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let mut p = CognitivePipeline::new(&crate::config::PipelineConfig {
            evening: vec!["a".into(), "b".into(), "c".into()],
            weekly: vec![], ..Default::default()
        });
        for name in ["a", "b", "c"] {
            p.register(Box::new(MockStage { label: name.into(), log: Arc::clone(&log) }));
        }
        p.run_evening(&mock_agent(), &mock_store()).await;
        assert_eq!(*log.lock().await, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn pipeline_skips_unknown_stage() {
        let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let mut p = CognitivePipeline::new(&crate::config::PipelineConfig {
            evening: vec!["a".into(), "unknown".into(), "b".into()],
            weekly: vec![], ..Default::default()
        });
        for name in ["a", "b"] {
            p.register(Box::new(MockStage { label: name.into(), log: Arc::clone(&log) }));
        }
        p.run_evening(&mock_agent(), &mock_store()).await;
        assert_eq!(*log.lock().await, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn pipeline_empty_order_is_noop() {
        let p = CognitivePipeline::new(&crate::config::PipelineConfig::default());
        p.run_evening(&mock_agent(), &mock_store()).await;
    }

    #[tokio::test]
    async fn pipeline_graph_branching() {
        let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let config = crate::config::PipelineConfig {
            evening: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            weekly: vec![],
            edges: vec![
                crate::config::EdgeConfig { from: "a".into(), to: "b".into() },
                crate::config::EdgeConfig { from: "a".into(), to: "c".into() },
                crate::config::EdgeConfig { from: "b".into(), to: "d".into() },
                crate::config::EdgeConfig { from: "c".into(), to: "d".into() },
            ],
            ..Default::default()
        };
        let mut p = CognitivePipeline::new(&config);
        for name in ["a", "b", "c", "d"] {
            p.register(Box::new(MockStage { label: name.into(), log: Arc::clone(&log) }));
        }
        p.run_evening(&mock_agent(), &mock_store()).await;
        let executed = log.lock().await;
        assert_eq!(executed[0], "a");
        assert_eq!(executed[3], "d");
        assert!(executed.contains(&"b".to_string()));
        assert!(executed.contains(&"c".to_string()));
    }

    #[test]
    fn topo_sort_respects_edges() {
        let order = crate::config::topo_sort(
            &["a", "b", "c", "d"].map(String::from).to_vec(),
            &[
                crate::config::EdgeConfig { from: "a".into(), to: "c".into() },
                crate::config::EdgeConfig { from: "a".into(), to: "b".into() },
                crate::config::EdgeConfig { from: "b".into(), to: "d".into() },
                crate::config::EdgeConfig { from: "c".into(), to: "d".into() },
            ],
        );
        assert_eq!(order[0], "a");
        assert_eq!(*order.last().unwrap(), "d");
        assert_eq!(order.len(), 4);
    }

    #[test]
    fn default_evening_has_eight_stages() {
        assert_eq!(default_evening_stages().len(), 8);
        assert_eq!(default_evening_stages().last().unwrap(), "meta");
    }

    #[test]
    fn default_weekly_has_two_stages() { assert_eq!(default_weekly_stages().len(), 2); }

    #[test]
    fn action_validation_rejects_unknown() {
        assert!(actions::validate_action_params("bogus", &["bogus"]).is_some());
    }

    #[test]
    fn action_validation_accepts_save_memory_visible() {
        let p = ["save_memory_visible", "cat", "content", "confidence:0.8", "visibility:subconscious"];
        assert!(actions::validate_action_params("save_memory_visible", &p).is_none());
    }

    #[test]
    fn action_validation_rejects_bad_visibility() {
        let p = ["save_memory_visible", "cat", "content", "confidence:0.8", "visibility:bogus"];
        assert!(actions::validate_action_params("save_memory_visible", &p).is_some());
    }

    #[test]
    fn action_validation_accepts_person_memory() {
        let p = ["save_person_memory", "张三", "behavior", "很有耐心", "confidence:0.7", "visibility:private"];
        assert!(actions::validate_action_params("save_person_memory", &p).is_none());
    }

    #[test]
    fn action_validation_rejects_empty_person_name() {
        let p = ["save_person_memory", "", "behavior", "content"];
        assert!(actions::validate_action_params("save_person_memory", &p).is_some());
    }

    #[test]
    fn action_validation_accepts_suggestion_dedup() {
        let p = ["record_suggestion_dedup", "mirror", "daily-reflection", "反思内容"];
        assert!(actions::validate_action_params("record_suggestion_dedup", &p).is_none());
    }
}
