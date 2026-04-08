//! 认知管线 — Agents-as-Tools 模式
//!
//! 每个认知阶段实现 `CognitiveStage` trait，注册到 `CognitivePipeline` 中。
//! 管线顺序由 config.toml 的 `[pipeline]` 段驱动，不再硬编码。

pub mod actions;
pub mod harness;
pub mod invoker;
pub mod parser;
pub mod stages;

pub use invoker::{ConstrainedInvoker, HarnessedAgent};

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::{error, info, warn};

use crate::agent::Agent;
use crate::store::Store;

// Re-export 常用类型
pub use stages::{UserDefinedStage, PresetCtxKey};

// ─── Pipeline Context（类型化 Stage I/O 契约）─────────────────────────────────

/// 管线共享上下文 — 每个 stage 读上游结果、写自己的结果
#[derive(Debug, Clone, Default)]
pub struct PipelineContext {
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

// ─── Trait + Output ──────────────────────────────────────────────────────────

/// 记忆进化结果（供 Tauri 命令和 pipeline classify 使用）
#[derive(Debug, Clone, Default)]
pub struct EvolutionResult {
    pub consolidated: usize,
    pub condensed: usize,
    pub linked: usize,
    pub decayed: usize,
    pub promoted: usize,
    pub purged: usize,
    pub summary: String,
}

pub enum StageOutput {
    Bool(bool),
    Evolution(EvolutionResult),
}

#[async_trait]
pub trait CognitiveStage: Send + Sync {
    fn name(&self) -> &str;
    async fn run(&self, invoker: Box<dyn ConstrainedInvoker>, store: Arc<Store>, ctx: PipelineContext) -> Result<(StageOutput, PipelineContext)>;
}

// ─── Pipeline Registry ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StageConfig {
    pub max_iterations: Option<usize>,
    /// 阶段级超时（秒）。超时后阶段标记为 Error("timeout")，后续阶段继续执行。
    pub timeout_secs: Option<u64>,
}

pub struct CognitivePipeline {
    stages: HashMap<String, Arc<dyn CognitiveStage>>,
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
        let core_stages = config.evening.iter().take(2).cloned().collect();

        Self {
            stages: HashMap::new(),
            adj, rev_adj, all_nodes,
            stage_configs: config.stages.clone(),
            core_stages,
        }
    }

    pub fn register(&mut self, stage: Arc<dyn CognitiveStage>) {
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

        // Wave-based 并行执行：每波取出所有 ready stage，区分可并行和需串行的
        while !ready.is_empty() {
            let mut wave: Vec<String> = Vec::new();
            while let Some(name) = ready.pop_front() {
                if completed.contains(&name) || !node_set.contains(name.as_str()) { continue; }
                if is_stage_disabled(store, &name) {
                    info!("{name}: skipped (disabled by self-evolution)");
                    ctx.stage_results.push(StageResult { name: name.clone(), status: StageStatus::Skipped, duration_ms: 0 });
                    completed.insert(name.clone());
                    self.push_ready(&name, &node_set, &mut in_degree, &completed, &mut ready);
                    continue;
                }
                if self.stages.contains_key(&name) {
                    wave.push(name);
                } else {
                    warn!("Pipeline: unknown stage '{name}', skipping");
                    completed.insert(name.clone());
                    self.push_ready(&name, &node_set, &mut in_degree, &completed, &mut ready);
                }
            }

            if wave.is_empty() { break; }

            // DAG 拓扑已保证依赖顺序，同一波内的 stage 天然无依赖 → 全部真并行
            let serial: Vec<String> = Vec::new();
            let parallel = wave;

            // 1. 串行组：顺序传递 ctx
            for name in &serial {
                let (result_ctx, elapsed, outcome, log_msg, status, score) =
                    self.run_stage_owned(name, agent, store, std::mem::take(&mut ctx)).await;
                ctx = result_ctx;
                if !log_msg.is_empty() { info!("{log_msg}"); }
                ctx.stage_results.push(StageResult { name: name.clone(), status, duration_ms: elapsed });
                let _ = store.log_pipeline_run_scored(name, pipeline_name, outcome, elapsed as i64, score);
                completed.insert(name.clone());
                self.push_ready(name, &node_set, &mut in_degree, &completed, &mut ready);
            }

            // 2. 并行组：tokio::spawn 真并行
            if parallel.len() == 1 {
                let name = &parallel[0];
                let (result_ctx, elapsed, outcome, log_msg, status, score) =
                    self.run_stage_owned(name, agent, store, ctx).await;
                ctx = result_ctx;
                if !log_msg.is_empty() { info!("{log_msg}"); }
                ctx.stage_results.push(StageResult { name: name.clone(), status, duration_ms: elapsed });
                let _ = store.log_pipeline_run_scored(name, pipeline_name, outcome, elapsed as i64, score);
                completed.insert(name.clone());
                self.push_ready(name, &node_set, &mut in_degree, &completed, &mut ready);
            } else if parallel.len() > 1 {
                info!("Pipeline: spawning {} parallel stages: {:?}", parallel.len(), &parallel);
                let mut handles = Vec::new();
                for name in &parallel {
                    let Some(stage_ref) = self.stages.get(name) else {
                        warn!("Pipeline: stage '{name}' not registered, skipping");
                        completed.insert(name.clone());
                        continue;
                    };
                    let stage = Arc::clone(stage_ref);
                    let stage_invoker = self.make_stage_invoker(name, agent, store);
                    let store_clone = Arc::clone(store);
                    let name_clone = name.clone();
                    let default_timeout = if name.starts_with("evolution_") { 600u64 } else { 180 };
                    let timeout = std::time::Duration::from_secs(
                        self.stage_configs.get(name).and_then(|c| c.timeout_secs).unwrap_or(default_timeout)
                    );
                    // 每个 parallel stage 拿空 ctx（不读上游），spawn 真并行
                    let handle = tokio::spawn(async move {
                        let start = std::time::Instant::now();
                        let result = tokio::time::timeout(
                            timeout,
                            stage.run(stage_invoker, store_clone, PipelineContext::default()),
                        ).await;
                        let elapsed = start.elapsed().as_millis() as u64;
                        (name_clone, result, elapsed)
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    let (name, result, elapsed) = handle.await.unwrap_or_else(|e| {
                        ("panic".into(), Ok(Err(anyhow::anyhow!("stage panicked: {e}"))), 0)
                    });
                    let (outcome, log_msg, status, score) = Self::classify_result(&name, result);
                    if !log_msg.is_empty() { info!("{log_msg}"); }
                    ctx.stage_results.push(StageResult { name: name.clone(), status, duration_ms: elapsed });
                    let _ = store.log_pipeline_run_scored(&name, pipeline_name, outcome, elapsed as i64, score);
                    completed.insert(name.clone());
                    self.push_ready(&name, &node_set, &mut in_degree, &completed, &mut ready);
                }
            }
        }
        ctx
    }

    fn classify_result(
        name: &str,
        result: Result<Result<(StageOutput, PipelineContext)>, tokio::time::error::Elapsed>,
    ) -> (&'static str, String, StageStatus, Option<f64>) {
        match result {
            Ok(Ok((StageOutput::Bool(true), _))) => ("ok", format!("{name}: completed"), StageStatus::Ok, Some(3.0)),
            Ok(Ok((StageOutput::Bool(false), _))) => ("empty", String::new(), StageStatus::Empty, Some(1.0)),
            Ok(Ok((StageOutput::Evolution(r), _))) => {
                let total = r.consolidated + r.condensed + r.linked + r.decayed + r.promoted + r.purged;
                if total > 0 {
                    // evolution 质量按操作数量打分：1-3 → 3分, 4-10 → 4分, >10 → 5分
                    let score = if total > 10 { 5.0 } else if total > 3 { 4.0 } else { 3.0 };
                    ("ok", format!("{name}: consolidated={}, condensed={}, linked={}, decayed={}, promoted={}, purged={}",
                        r.consolidated, r.condensed, r.linked, r.decayed, r.promoted, r.purged), StageStatus::Ok, Some(score))
                } else {
                    ("empty", String::new(), StageStatus::Empty, Some(1.0))
                }
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                if msg.contains("已达上限") {
                    warn!("{name}: LLM budget exhausted, degrading");
                    ("degraded", String::new(), StageStatus::Degraded("llm_budget_exhausted".into()), Some(0.5))
                } else {
                    error!("{name} failed: {msg}");
                    ("error", String::new(), StageStatus::Error(msg), Some(0.0))
                }
            }
            Err(_) => {
                error!("{name} timed out");
                ("error", String::new(), StageStatus::Error("timeout".into()), Some(0.0))
            }
        }
    }

    /// 执行单个 stage（owned ctx 传入传出），返回 (ctx, elapsed_ms, outcome, log_msg, status, score)
    async fn run_stage_owned(
        &self, name: &str, agent: &Agent, store: &Arc<Store>, ctx: PipelineContext,
    ) -> (PipelineContext, u64, &'static str, String, StageStatus, Option<f64>) {
        let Some(stage) = self.stages.get(name) else {
            return (ctx, 0, "error", String::new(), StageStatus::Error("not found".into()), Some(0.0));
        };
        let stage_invoker = self.make_stage_invoker(name, agent, store);
        let start = std::time::Instant::now();
        let default_timeout = if name.starts_with("evolution_") { 600u64 } else { 180 };
        let timeout = std::time::Duration::from_secs(
            self.stage_configs.get(name).and_then(|c| c.timeout_secs).unwrap_or(default_timeout)
        );
        let stage_result = tokio::time::timeout(
            timeout,
            stage.run(stage_invoker, Arc::clone(store), ctx),
        ).await;
        let elapsed = start.elapsed().as_millis() as u64;

        // 从结果中取回 ctx（成功时从返回值，失败/超时时用默认）
        let returned_ctx = match &stage_result {
            Ok(Ok((_, ref c))) => c.clone(),
            _ => PipelineContext::default(),
        };
        let (outcome, log_msg, status, score) = Self::classify_result(name, stage_result);
        (returned_ctx, elapsed, outcome, log_msg, status, score)
    }

    pub async fn run_evening(&self, agent: &Agent, store: &Arc<Store>) -> PipelineContext {
        self.run("evening", &self.all_nodes.clone(), agent, store).await
    }

    pub async fn run_weekly(&self, agent: &Agent, store: &Arc<Store>) -> PipelineContext {
        // 周频阶段由预设 stage 实现，使用 config 中配置的 weekly stage 列表
        let weekly: Vec<String> = self.stages.keys()
            .filter(|k| {
                let s = k.as_str();
                s == "mirror_weekly" || s.starts_with("weekly_")
            })
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

    fn make_stage_invoker(&self, name: &str, base: &Agent, store: &Arc<Store>) -> Box<dyn ConstrainedInvoker> {
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
        Box::new(HarnessedAgent::new(agent, Arc::clone(store), format!("stage:{name}")))
    }
}

fn is_stage_disabled(store: &Store, name: &str) -> bool {
    store.get_pipeline_overrides(name).unwrap_or_default()
        .iter().any(|o| o.key == "enabled" && o.value == "false")
}

// ─── build_pipeline ──────────────────────────────────────────────────────────

pub fn build_pipeline(config: &crate::config::PipelineConfig, store: &Store) -> CognitivePipeline {
    let mut p = CognitivePipeline::new(config);

    // 所有 stage 全由预设/自定义 stage 实现，无内置 stage
    let custom_stages = store.list_custom_stages().unwrap_or_default();
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
        p.register(Arc::new(UserDefinedStage::new(
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

// ─── 默认管线顺序 ───────────────────────────────────────────────────────────

pub fn default_evening_stages() -> Vec<String> {
    vec![
        "observer", "verifier", "contradiction_detector",
        "coach", "person_observer",
        "mirror", "questioner",
        "integrator",
        "evolution_transform", "evolution_graph",
        "meta_params", "meta_prompts", "meta_ui",
    ].into_iter().map(String::from).collect()
}

/// 默认晚间管线边：
/// Wave 1: observer（独立）+ contradiction_detector（读 core_memories，无依赖）
/// Wave 2: verifier + coach + person_observer（依赖 observer）
/// Wave 3: mirror + questioner（依赖 coach + verifier）
/// Wave 4: integrator（依赖 verifier + coach + contradiction_detector）
/// 进化链：evolution_transform → evolution_graph（依赖 integrator）
/// Meta 组：最后并行（依赖 integrator + evolution_graph）
pub fn default_evening_edges() -> Vec<crate::config::EdgeConfig> {
    use crate::config::EdgeConfig;
    vec![
        // Wave 1 → Wave 2: observer 输出供 verifier / coach / person_observer 消费
        EdgeConfig { from: "observer".into(), to: "verifier".into() },
        EdgeConfig { from: "observer".into(), to: "coach".into() },
        EdgeConfig { from: "observer".into(), to: "person_observer".into() },
        // Wave 2 → Wave 3: coach + verifier → mirror + questioner
        EdgeConfig { from: "coach".into(), to: "mirror".into() },
        EdgeConfig { from: "verifier".into(), to: "mirror".into() },
        EdgeConfig { from: "coach".into(), to: "questioner".into() },
        EdgeConfig { from: "verifier".into(), to: "questioner".into() },
        // Wave 2/1 → Wave 4: integrator 需要 verifier + coach + contradiction_detector
        EdgeConfig { from: "verifier".into(), to: "integrator".into() },
        EdgeConfig { from: "coach".into(), to: "integrator".into() },
        EdgeConfig { from: "contradiction_detector".into(), to: "integrator".into() },
        // Wave 4 → 进化链
        EdgeConfig { from: "integrator".into(), to: "evolution_transform".into() },
        EdgeConfig { from: "evolution_transform".into(), to: "evolution_graph".into() },
        // 进化链 + integrator → meta 组（并行）
        EdgeConfig { from: "integrator".into(), to: "meta_params".into() },
        EdgeConfig { from: "integrator".into(), to: "meta_prompts".into() },
        EdgeConfig { from: "integrator".into(), to: "meta_ui".into() },
        EdgeConfig { from: "evolution_graph".into(), to: "meta_params".into() },
        EdgeConfig { from: "evolution_graph".into(), to: "meta_prompts".into() },
        EdgeConfig { from: "evolution_graph".into(), to: "meta_ui".into() },
    ]
}

pub fn default_weekly_stages() -> Vec<String> {
    vec!["mirror_weekly"].into_iter().map(String::from).collect()
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
        async fn run(&self, _: Box<dyn ConstrainedInvoker>, _: Arc<Store>, ctx: PipelineContext) -> Result<(StageOutput, PipelineContext)> {
            self.log.lock().await.push(self.label.clone());
            Ok((StageOutput::Bool(true), ctx))
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
            p.register(Arc::new(MockStage { label: name.into(), log: Arc::clone(&log) }));
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
            p.register(Arc::new(MockStage { label: name.into(), log: Arc::clone(&log) }));
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
            p.register(Arc::new(MockStage { label: name.into(), log: Arc::clone(&log) }));
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
    fn default_evening_has_expected_stages() {
        let stages = default_evening_stages();
        assert_eq!(stages.len(), 13);
        assert_eq!(stages.last().unwrap(), "meta_ui");
        assert_eq!(stages.first().unwrap(), "observer");
        // new stages present
        assert!(stages.contains(&"verifier".to_string()));
        assert!(stages.contains(&"contradiction_detector".to_string()));
        assert!(stages.contains(&"integrator".to_string()));
    }

    #[test]
    fn default_edges_create_self_correcting_dag() {
        let edges = default_evening_edges();
        // observer → verifier (wave 1→2)
        assert!(edges.iter().any(|e| e.from == "observer" && e.to == "verifier"));
        // observer → coach
        assert!(edges.iter().any(|e| e.from == "observer" && e.to == "coach"));
        // verifier + coach → integrator
        assert!(edges.iter().any(|e| e.from == "verifier" && e.to == "integrator"));
        assert!(edges.iter().any(|e| e.from == "coach" && e.to == "integrator"));
        assert!(edges.iter().any(|e| e.from == "contradiction_detector" && e.to == "integrator"));
        // integrator → evolution_transform
        assert!(edges.iter().any(|e| e.from == "integrator" && e.to == "evolution_transform"));
        // evolution_transform → evolution_graph
        assert!(edges.iter().any(|e| e.from == "evolution_transform" && e.to == "evolution_graph"));
        // both integrator + evolution_graph → meta_params
        assert!(edges.iter().any(|e| e.from == "integrator" && e.to == "meta_params"));
        assert!(edges.iter().any(|e| e.from == "evolution_graph" && e.to == "meta_params"));
    }

    #[test]
    fn default_weekly_has_mirror_weekly() {
        let stages = default_weekly_stages();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0], "mirror_weekly");
    }

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
