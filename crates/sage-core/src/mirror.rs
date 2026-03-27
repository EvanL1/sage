use anyhow::Result;
use tracing::info;

use crate::agent::Agent;
use crate::applescript;
use crate::pipeline::{MirrorOutput, PipelineContext};
use crate::prompts;
use crate::reflective_detector;
use crate::skills;
use crate::store::Store;

/// 镜子角色：从 SQLite coach_insight 记忆中挑选一个行为模式，温和地反映给用户（每天最多一次）
/// 不再读 sage.md，改用 store.search_memories("coach_insight", 5) 获取最近洞察
pub async fn reflect(agent: &Agent, store: &Store, ctx: &mut PipelineContext) -> Result<bool> {
    // 读取最近的教练洞察（替代原来读 sage.md）
    let insights = store.search_memories("coach_insight", 5)?;
    if insights.is_empty() {
        info!("Mirror: no coach_insight records found, skipping");
        return Ok(false);
    }

    let insights_text = insights
        .iter()
        .map(|m| format!("- {}", m.content))
        .collect::<Vec<_>>()
        .join("\n");

    // 内容过短则跳过
    if insights_text.len() < 20 {
        info!("Mirror: insight content too short, skipping");
        return Ok(false);
    }

    // 去重：今天已经反映过则跳过
    if store.has_recent_suggestion("mirror", "daily-reflection") {
        info!("Mirror: already reflected today, skipping");
        return Ok(false);
    }

    info!(
        "Mirror: generating reflection from {} coach insights",
        insights.len()
    );

    let lang = store.prompt_lang();
    let prompt = prompts::mirror_user(&lang, &insights_text);
    let reflect_guide = skills::load_section("sage-cognitive", "## Phase 2: REFLECT");
    let system = format!(
        "{reflect_guide}\n\n{}",
        prompts::mirror_system_suffix(&lang)
    );
    let resp = agent.invoke(&prompt, Some(&system)).await?;

    let reflection = resp.text.trim().to_string();
    if reflection.is_empty() {
        info!("Mirror: empty response from agent, skipping");
        return Ok(false);
    }

    // 记录本次反映（用于去重）
    store.record_suggestion("mirror", "daily-reflection", &reflection)?;
    info!("Mirror: reflection recorded");

    // 通知用户
    let notify_title = if lang == "en" { "Sage Observation" } else { "Sage 观察" };
    let notified = applescript::notify(notify_title, &reflection, "/").await.is_ok();

    // 写入上下文
    ctx.mirror = Some(MirrorOutput {
        reflection,
        notified,
    });

    Ok(true)
}

/// 扫描文本中的反思信号并存入 SQLite / Detect reflective signals and persist them
pub fn detect_and_store(
    text: &str,
    source: &str,
    context: Option<&str>,
    store: &Store,
) -> Result<usize> {
    let signals = reflective_detector::scan(text, source);
    if signals.is_empty() {
        return Ok(0);
    }
    let mut count = 0;
    for sig in &signals {
        store.save_reflective_signal(
            source,
            &sig.signal_type,
            &sig.raw_text,
            context,
            sig.intensity, // baseline_divergence = intensity（规则引擎无独立基线）
            sig.armor_pattern.as_deref(),
            sig.intensity,
        )?;
        count += 1;
    }
    info!("Mirror: detected {count} reflective signals from {source}");
    Ok(count)
}

/// 周度 Mirror 报告：汇总本周反思信号，LLM 生成反映性报告
pub async fn mirror_weekly(agent: &Agent, store: &Store, _ctx: &mut PipelineContext) -> Result<bool> {
    // 去重：本周已生成过则跳过
    if store.has_recent_suggestion("mirror", "weekly-mirror") {
        info!("Mirror weekly: already generated this week, skipping");
        return Ok(false);
    }

    // 收集过去 7 天的信号
    let since = (chrono::Local::now() - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let signals = store.get_signals_since(&since)?;
    if signals.is_empty() {
        info!("Mirror weekly: no signals in the past 7 days, skipping");
        return Ok(false);
    }

    // 构建信号摘要文本
    let signals_text = signals
        .iter()
        .map(|s| {
            format!(
                "- [{}] ({}, intensity={:.1}) {}",
                s.signal_type,
                if s.resolved { "resolved" } else { "open" },
                s.intensity,
                s.raw_text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let lang = store.prompt_lang();
    let system = prompts::mirror_weekly_system(&lang);
    let user_prompt = prompts::mirror_weekly_user(&lang, &signals_text);

    agent.reset_counter();
    let resp = agent.invoke(&user_prompt, Some(system)).await?;
    let report = resp.text.trim().to_string();
    if report.is_empty() {
        return Ok(false);
    }

    // 存入 reports 表 + 去重标记
    store.save_report("mirror_weekly", &report)?;
    store.record_suggestion("mirror", "weekly-mirror", &report)?;
    info!("Mirror weekly: report saved ({} signals analyzed)", signals.len());

    // 通知
    let title = if lang == "en" { "Sage Mirror Report" } else { "Sage 镜像报告" };
    let preview: String = report.chars().take(100).collect();
    applescript::notify(title, &preview, "/").await?;

    Ok(true)
}
