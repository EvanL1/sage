//! 内置 Stage wrappers + UserDefinedStage（预设/自定义共用执行引擎）

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::info;

use crate::store::Store;

use super::{
    ConstrainedInvoker, CognitiveStage, PipelineContext, StageOutput,
    ObserverOutput, CoachOutput, MirrorOutput, QuestionerOutput,
};
use super::actions;

// ─── UserDefinedStage（预设 + 自定义共用引擎）────────────────────────────

/// 预设 stage 的上下文输出类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PresetCtxKey {
    Observer,
    Coach,
    Mirror,
    Questioner,
}

/// 用户自定义/预设的认知阶段：硬+软约束 + LLM 处理 + ACTION 执行
pub struct UserDefinedStage {
    stage_name: String,
    prompt_template: String,
    output_format: String,
    available_actions: Vec<String>,
    allowed_inputs: Vec<String>,
    max_actions: usize,
    pre_condition: String,
    /// 预设 stage 的 post-hook：归档已消费的 observations
    archive_observations: bool,
    /// 预设 stage 写入 PipelineContext 的 key
    preset_ctx_key: Option<PresetCtxKey>,
}

impl UserDefinedStage {
    pub fn new(
        name: String, prompt: String, output_format: String,
        actions_csv: String, inputs_csv: String, max_actions: i32,
        pre_condition: String, archive_observations: bool,
        preset_ctx_key: Option<PresetCtxKey>,
    ) -> Self {
        let split_csv = |s: &str| {
            s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        };
        Self {
            stage_name: name,
            prompt_template: prompt,
            output_format,
            available_actions: split_csv(&actions_csv),
            allowed_inputs: split_csv(&inputs_csv),
            max_actions: max_actions.max(0) as usize,
            pre_condition,
            archive_observations,
            preset_ctx_key,
        }
    }
}

#[async_trait]
impl CognitiveStage for UserDefinedStage {
    fn name(&self) -> &str { &self.stage_name }

    async fn run(&self, invoker: Box<dyn ConstrainedInvoker>, store: Arc<Store>, mut ctx: PipelineContext) -> Result<(StageOutput, PipelineContext)> {
        // ═══ PRE-HOOK（硬约束）═══

        if !self.pre_condition.is_empty() && !actions::check_pre_condition(&store, &self.pre_condition) {
            info!("{}: skipped (pre-condition not met)", self.stage_name);
            return Ok((StageOutput::Bool(false), ctx));
        }

        let context = actions::load_filtered_context(&store, &self.allowed_inputs);

        if context.is_empty() {
            info!("{}: skipped (no data from declared inputs)", self.stage_name);
            return Ok((StageOutput::Bool(false), ctx));
        }

        // ═══ LLM 执行（软约束在 prompt 中）═══

        let mut prompt = self.prompt_template.replace("{context}", &context);
        if !self.output_format.is_empty() {
            prompt.push_str(&format!("\n\n## 输出格式要求\n{}", self.output_format));
        }
        if !self.available_actions.is_empty() {
            prompt.push_str(&format!("\n\n{}", actions::action_docs(&self.available_actions)));
        }

        invoker.reset_counter();
        let text = super::invoker::invoke_text(&*invoker, &prompt, None).await?;

        if text.is_empty() || text == "NONE" {
            return Ok((StageOutput::Bool(false), ctx));
        }

        // ═══ POST-HOOK（硬约束）═══

        let action_result = if !self.available_actions.is_empty() {
            actions::execute_actions(&text, &self.available_actions, &store, &self.stage_name, self.max_actions)
        } else {
            actions::ActionResult { count: 0, results: Vec::new() }
        };

        // 分析文本保存为 observation
        let analysis: Vec<&str> = text.lines()
            .filter(|l| !l.trim().starts_with("ACTION "))
            .collect();
        if !analysis.is_empty() {
            let category = format!("custom_{}", self.stage_name);
            store.record_observation(&category, &analysis.join("\n"), None)?;
        }

        // 预设 post-hook：归档 observations
        if self.archive_observations {
            if let Ok(obs) = store.load_unprocessed_observations(200) {
                let ids: Vec<i64> = obs.iter().map(|o| o.id).collect();
                if !ids.is_empty() {
                    let _ = store.mark_observations_processed(&ids);
                    info!("{}: archived {} observations", self.stage_name, ids.len());
                }
            }
        }

        // 预设 ctx 桥接：把 ACTION 产出回填到 PipelineContext
        if let Some(key) = &self.preset_ctx_key {
            write_preset_ctx(&mut ctx, key, &action_result, &analysis);
        }

        Ok((StageOutput::Bool(action_result.count > 0 || !analysis.is_empty()), ctx))
    }
}

/// 将预设 stage 的 ACTION 执行结果回填到 PipelineContext
fn write_preset_ctx(
    ctx: &mut PipelineContext,
    key: &PresetCtxKey,
    result: &actions::ActionResult,
    analysis: &[&str],
) {
    match key {
        PresetCtxKey::Observer => {
            ctx.observer = Some(ObserverOutput {
                notes: analysis.iter().map(|s| s.to_string()).collect(),
            });
        }
        PresetCtxKey::Coach => {
            let insights: Vec<String> = result.results.iter()
                .filter(|(name, _)| name == "save_memory" || name == "save_memory_visible")
                .map(|(_, id)| format!("insight#{id}"))
                .collect();
            ctx.coach = Some(CoachOutput {
                insights,
                observations_archived: 0,
                degraded: false,
            });
        }
        PresetCtxKey::Mirror => {
            ctx.mirror = Some(MirrorOutput {
                reflection: analysis.join("\n"),
                notified: result.results.iter().any(|(n, _)| n == "notify_user"),
            });
        }
        PresetCtxKey::Questioner => {
            ctx.questioner = Some(QuestionerOutput {
                question: analysis.join("\n"),
                is_resurface: false,
            });
        }
    }
}
