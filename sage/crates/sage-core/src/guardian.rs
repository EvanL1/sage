use anyhow::Result;
use tracing::info;

use crate::applescript;
use crate::store::Store;

/// Guardian：基于规则的异常检测，无需调用 Claude，完全免费
/// 检查最近观察，发现过劳、高压信号后发送关怀提醒
pub async fn check(store: &Store) -> Result<bool> {
    // 去重：每天最多触发一次
    if store.has_recent_suggestion("guardian", "daily-check") {
        info!("Guardian: already alerted today, skipping");
        return Ok(false);
    }

    let observations = store.load_recent_observations(30)?;
    if observations.is_empty() {
        return Ok(false);
    }

    // 规则1：观察数量密集（30条上限内达到15+）→ 繁忙信号
    let high_density = observations.len() >= 15;

    // 规则2：紧急事项过多（category == "urgent" 达到3+）
    let urgent_count = observations
        .iter()
        .filter(|(cat, _)| cat == "urgent")
        .count();
    let urgent_overload = urgent_count >= 3;

    if !high_density && !urgent_overload {
        info!("Guardian: all clear, no anomalies detected");
        return Ok(false);
    }

    // 根据触发规则构建关怀消息
    let message = if urgent_overload && high_density {
        format!("今天处理了 {} 条紧急事务，节奏很快。辛苦了，记得留点时间给自己。", urgent_count)
    } else if urgent_overload {
        format!("今天有 {} 件紧急事务，压力不小。先处理完手头的，然后喝杯水休息一下。", urgent_count)
    } else {
        "今天看起来特别忙，观察到很多事情同时在推进。记得休息一下。".to_string()
    };

    info!("Guardian: alert triggered — high_density={high_density}, urgent={urgent_count}");

    store.record_suggestion("guardian", "daily-check", &message)?;
    applescript::notify("Sage 关心", &message).await?;

    Ok(true)
}
