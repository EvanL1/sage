use anyhow::Result;

use crate::store::Store;
use sage_types::FeedbackAction;

/// 反馈处理后的副作用
#[derive(Debug)]
pub enum FeedbackEffect {
    /// 简单记录，无额外动作
    Recorded,
    /// NotUseful 累计达到阈值，建议降级同类建议
    DemotionSuggested { category: String, count: usize },
    /// NeverDoThis 触发，需要更新 profile 的 negative_rules 并重新生成 SOP
    NegativeRuleAdded { rule: String },
}

/// NotUseful 累计触发降级的阈值
const DEMOTION_THRESHOLD: usize = 3;

/// 反馈处理器
pub struct FeedbackProcessor<'a> {
    store: &'a Store,
}

impl<'a> FeedbackProcessor<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// 处理反馈并返回副作用
    pub fn process(&self, suggestion_id: i64, action: FeedbackAction) -> Result<FeedbackEffect> {
        // 统一记录反馈
        self.store.record_feedback(suggestion_id, &action)?;

        match action {
            FeedbackAction::Useful | FeedbackAction::Correction(_) => Ok(FeedbackEffect::Recorded),

            FeedbackAction::NotUseful => {
                // 查找该 suggestion 的 event_source，检查同类累计
                let suggestions = self.store.get_recent_suggestions(1000)?;
                let event_source = suggestions
                    .iter()
                    .find(|s| s.id == suggestion_id)
                    .map(|s| s.event_source.clone())
                    .unwrap_or_default();

                let count = self
                    .store
                    .count_feedback_by_source_and_type(&event_source, "NotUseful")?;

                if count >= DEMOTION_THRESHOLD {
                    Ok(FeedbackEffect::DemotionSuggested {
                        category: event_source,
                        count,
                    })
                } else {
                    Ok(FeedbackEffect::Recorded)
                }
            }

            FeedbackAction::NeverDoThis(ref reason) => {
                let rule = reason.clone();
                let mut profile = self.store.load_profile()?.unwrap_or_default();
                // 去重：>60% 相似则替换为更长版本，不新增
                if let Some(idx) = crate::similarity::find_similar(
                    &profile.negative_rules, &rule, 0.6,
                ) {
                    // 保留更长/更具体的版本
                    if rule.len() >= profile.negative_rules[idx].len() {
                        profile.negative_rules[idx] = rule.clone();
                    }
                } else {
                    profile.negative_rules.push(rule.clone());
                }
                profile.sop_version += 1;
                self.store.save_profile(&profile)?;

                Ok(FeedbackEffect::NegativeRuleAdded { rule })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_useful_feedback() {
        let store = Store::open_in_memory().unwrap();
        let sid = store.record_suggestion("email", "test", "resp").unwrap();

        let processor = FeedbackProcessor::new(&store);
        let effect = processor.process(sid, FeedbackAction::Useful).unwrap();
        assert!(matches!(effect, FeedbackEffect::Recorded));
    }

    #[test]
    fn test_correction_feedback() {
        let store = Store::open_in_memory().unwrap();
        let sid = store.record_suggestion("email", "test", "resp").unwrap();

        let processor = FeedbackProcessor::new(&store);
        let effect = processor
            .process(sid, FeedbackAction::Correction("更好的回复".into()))
            .unwrap();
        assert!(matches!(effect, FeedbackEffect::Recorded));
    }

    #[test]
    fn test_not_useful_below_threshold() {
        let store = Store::open_in_memory().unwrap();
        let processor = FeedbackProcessor::new(&store);

        // 同一 event_source 两次 NotUseful，不触发降级
        let s1 = store.record_suggestion("email", "p1", "r1").unwrap();
        let effect = processor.process(s1, FeedbackAction::NotUseful).unwrap();
        assert!(matches!(effect, FeedbackEffect::Recorded));

        let s2 = store.record_suggestion("email", "p2", "r2").unwrap();
        let effect = processor.process(s2, FeedbackAction::NotUseful).unwrap();
        assert!(matches!(effect, FeedbackEffect::Recorded));
    }

    #[test]
    fn test_not_useful_triggers_demotion() {
        let store = Store::open_in_memory().unwrap();
        let processor = FeedbackProcessor::new(&store);

        // 同一 event_source 累计 3 次 NotUseful，触发降级
        for i in 0..3 {
            let sid = store
                .record_suggestion("email", &format!("p{i}"), &format!("r{i}"))
                .unwrap();
            let effect = processor.process(sid, FeedbackAction::NotUseful).unwrap();
            if i < 2 {
                assert!(matches!(effect, FeedbackEffect::Recorded));
            } else {
                match &effect {
                    FeedbackEffect::DemotionSuggested { category, count } => {
                        assert_eq!(category, "email");
                        assert_eq!(*count, 3);
                    }
                    _ => panic!("应该触发 DemotionSuggested，实际: {effect:?}"),
                }
            }
        }
    }

    #[test]
    fn test_not_useful_different_sources_no_demotion() {
        let store = Store::open_in_memory().unwrap();
        let processor = FeedbackProcessor::new(&store);

        // 不同 event_source 的 NotUseful 不互相累计
        let s1 = store.record_suggestion("email", "p1", "r1").unwrap();
        processor.process(s1, FeedbackAction::NotUseful).unwrap();

        let s2 = store.record_suggestion("calendar", "p2", "r2").unwrap();
        processor.process(s2, FeedbackAction::NotUseful).unwrap();

        let s3 = store.record_suggestion("hooks", "p3", "r3").unwrap();
        let effect = processor.process(s3, FeedbackAction::NotUseful).unwrap();
        assert!(matches!(effect, FeedbackEffect::Recorded));
    }

    #[test]
    fn test_never_do_this_adds_negative_rule() {
        let store = Store::open_in_memory().unwrap();
        let sid = store.record_suggestion("email", "test", "resp").unwrap();

        let processor = FeedbackProcessor::new(&store);
        let effect = processor
            .process(sid, FeedbackAction::NeverDoThis("不要总结邮件".into()))
            .unwrap();

        match &effect {
            FeedbackEffect::NegativeRuleAdded { rule } => {
                assert_eq!(rule, "不要总结邮件");
            }
            _ => panic!("应该返回 NegativeRuleAdded，实际: {effect:?}"),
        }

        // 验证 profile 已更新
        let profile = store.load_profile().unwrap().unwrap();
        assert!(profile.negative_rules.contains(&"不要总结邮件".to_string()));
        assert_eq!(profile.sop_version, 1);
    }

    #[test]
    fn test_never_do_this_with_existing_profile() {
        let store = Store::open_in_memory().unwrap();

        // 先保存一个已有 profile
        let mut profile = sage_types::UserProfile::default();
        profile.negative_rules.push("已有规则".into());
        profile.sop_version = 5;
        store.save_profile(&profile).unwrap();

        let sid = store.record_suggestion("email", "test", "resp").unwrap();
        let processor = FeedbackProcessor::new(&store);
        processor
            .process(sid, FeedbackAction::NeverDoThis("新规则".into()))
            .unwrap();

        let loaded = store.load_profile().unwrap().unwrap();
        assert_eq!(loaded.negative_rules.len(), 2);
        assert_eq!(loaded.negative_rules[0], "已有规则");
        assert_eq!(loaded.negative_rules[1], "新规则");
        assert_eq!(loaded.sop_version, 6);
    }
}
