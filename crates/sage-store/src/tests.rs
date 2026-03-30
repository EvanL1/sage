use super::*;
use sage_types::*;

fn make_test_profile() -> UserProfile {
    UserProfile {
        identity: UserIdentity {
            name: "Alex".into(),
            role: "Team Lead".into(),
            reporting_line: vec!["Alex".into(), "Jordan".into()],
            primary_language: "zh".into(),
            secondary_language: "en".into(),
            prompt_language: "zh".into(),
        },
        sop_version: 1,
        negative_rules: vec!["不要发重复邮件".into()],
        ..Default::default()
    }
}

#[test]
fn test_open_in_memory() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.load_profile().unwrap().is_none());
}

#[test]
fn test_save_and_load_profile() {
    let store = Store::open_in_memory().unwrap();
    let profile = make_test_profile();
    store.save_profile(&profile).unwrap();

    let loaded = store.load_profile().unwrap().unwrap();
    assert_eq!(loaded.identity.name, "Alex");
    assert_eq!(loaded.sop_version, 1);
    assert_eq!(loaded.negative_rules, vec!["不要发重复邮件"]);
}

#[test]
fn test_profile_upsert() {
    let store = Store::open_in_memory().unwrap();
    let mut profile = make_test_profile();
    store.save_profile(&profile).unwrap();

    profile.sop_version = 2;
    profile.negative_rules.push("不要在晚上发通知".into());
    store.save_profile(&profile).unwrap();

    let loaded = store.load_profile().unwrap().unwrap();
    assert_eq!(loaded.sop_version, 2);
    assert_eq!(loaded.negative_rules.len(), 2);
}

#[test]
fn test_get_sop_version() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.get_sop_version().unwrap(), 0);

    let profile = make_test_profile();
    store.save_profile(&profile).unwrap();
    assert_eq!(store.get_sop_version().unwrap(), 1);
}

#[test]
fn test_record_and_get_suggestions() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .record_suggestion("email", "处理这封邮件", "建议回复确认")
        .unwrap();
    let id2 = store
        .record_suggestion("calendar", "明天的会议", "准备议题")
        .unwrap();

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);

    let suggestions = store.get_recent_suggestions(10).unwrap();
    assert_eq!(suggestions.len(), 2);
    assert_eq!(suggestions[0].event_source, "calendar");
    assert_eq!(suggestions[1].event_source, "email");
}

#[test]
fn test_record_feedback() {
    let store = Store::open_in_memory().unwrap();
    let sid = store
        .record_suggestion("email", "test", "response")
        .unwrap();

    store.record_feedback(sid, &FeedbackAction::Useful).unwrap();
    store
        .record_feedback(sid, &FeedbackAction::NotUseful)
        .unwrap();
    store
        .record_feedback(sid, &FeedbackAction::NeverDoThis("不要总结邮件".into()))
        .unwrap();
    store
        .record_feedback(sid, &FeedbackAction::Correction("应该直接转发".into()))
        .unwrap();

    let count = store.count_feedback_by_type("NotUseful").unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_count_feedback_by_source_and_type() {
    let store = Store::open_in_memory().unwrap();
    let s1 = store.record_suggestion("email", "p1", "r1").unwrap();
    let s2 = store.record_suggestion("email", "p2", "r2").unwrap();
    let s3 = store.record_suggestion("calendar", "p3", "r3").unwrap();

    store
        .record_feedback(s1, &FeedbackAction::NotUseful)
        .unwrap();
    store
        .record_feedback(s2, &FeedbackAction::NotUseful)
        .unwrap();
    store
        .record_feedback(s3, &FeedbackAction::NotUseful)
        .unwrap();

    let email_count = store
        .count_feedback_by_source_and_type("email", "NotUseful")
        .unwrap();
    assert_eq!(email_count, 2);

    let cal_count = store
        .count_feedback_by_source_and_type("calendar", "NotUseful")
        .unwrap();
    assert_eq!(cal_count, 1);
}

#[test]
fn test_suggestion_with_feedback() {
    let store = Store::open_in_memory().unwrap();
    let sid = store
        .record_suggestion("email", "test prompt", "test response")
        .unwrap();
    store.record_feedback(sid, &FeedbackAction::Useful).unwrap();

    let suggestions = store.get_recent_suggestions(10).unwrap();
    assert_eq!(suggestions.len(), 1);
    assert!(matches!(
        suggestions[0].feedback,
        Some(FeedbackAction::Useful)
    ));
}

#[test]
fn test_record_observation() {
    let store = Store::open_in_memory().unwrap();
    store
        .record_observation("pattern", "每天下午3点查邮件", Some("{\"count\": 5}"))
        .unwrap();
    store
        .record_observation("habit", "偏好直接沟通", None)
        .unwrap();
}

#[test]
fn test_load_recent_observations() {
    let store = Store::open_in_memory().unwrap();

    let empty = store.load_recent_observations(10).unwrap();
    assert!(empty.is_empty());

    store
        .record_observation("pattern", "每天下午3点查邮件", None)
        .unwrap();
    store
        .record_observation("habit", "偏好直接沟通", None)
        .unwrap();
    store
        .record_observation("pattern", "喜欢用类比解释概念", None)
        .unwrap();

    let all = store.load_recent_observations(10).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].0, "pattern");
    assert_eq!(all[0].1, "喜欢用类比解释概念");

    let limited = store.load_recent_observations(2).unwrap();
    assert_eq!(limited.len(), 2);
}

#[test]
fn test_get_suggestions_with_feedback() {
    let store = Store::open_in_memory().unwrap();

    let empty = store.get_suggestions_with_feedback(10).unwrap();
    assert!(empty.is_empty());

    let s1 = store
        .record_suggestion("email", "prompt1", "response1")
        .unwrap();
    let _s2 = store
        .record_suggestion("calendar", "prompt2", "response2")
        .unwrap();

    store.record_feedback(s1, &FeedbackAction::Useful).unwrap();

    let results = store.get_suggestions_with_feedback(10).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "calendar");
    assert_eq!(results[0].1, "response2");
    assert!(results[0].2.is_none());
    assert_eq!(results[1].0, "email");
    assert!(results[1].2.is_some());
}

#[test]
fn test_save_and_load_provider_config() {
    let store = Store::open_in_memory().unwrap();
    let config = ProviderConfig {
        provider_id: "anthropic-api".into(),
        api_key: Some("sk-test-123".into()),
        model: Some("claude-sonnet-4-20250514".into()),
        base_url: None,
        enabled: true,
        priority: None,
    };
    store.save_provider_config(&config).unwrap();

    let configs = store.load_provider_configs().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].provider_id, "anthropic-api");
    assert_eq!(configs[0].api_key, Some("sk-test-123".into()));
    assert!(configs[0].enabled);
}

#[test]
fn test_provider_config_upsert() {
    let store = Store::open_in_memory().unwrap();
    let mut config = ProviderConfig {
        provider_id: "openai-api".into(),
        api_key: Some("sk-old".into()),
        model: None,
        base_url: None,
        enabled: true,
        priority: None,
    };
    store.save_provider_config(&config).unwrap();

    config.api_key = Some("sk-new".into());
    config.model = Some("gpt-4o".into());
    store.save_provider_config(&config).unwrap();

    let configs = store.load_provider_configs().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].api_key, Some("sk-new".into()));
    assert_eq!(configs[0].model, Some("gpt-4o".into()));
}

#[test]
fn test_delete_provider_config() {
    let store = Store::open_in_memory().unwrap();
    let config = ProviderConfig {
        provider_id: "deepseek-api".into(),
        api_key: Some("ds-key".into()),
        model: None,
        base_url: None,
        enabled: true,
        priority: None,
    };
    store.save_provider_config(&config).unwrap();
    assert_eq!(store.load_provider_configs().unwrap().len(), 1);

    store.delete_provider_config("deepseek-api").unwrap();
    assert_eq!(store.load_provider_configs().unwrap().len(), 0);
}

#[test]
fn test_load_empty_provider_configs() {
    let store = Store::open_in_memory().unwrap();
    let configs = store.load_provider_configs().unwrap();
    assert!(configs.is_empty());
}

#[test]
fn test_load_unprocessed_observations() {
    let store = Store::open_in_memory().unwrap();

    let empty = store.load_unprocessed_observations(10).unwrap();
    assert!(empty.is_empty());

    store
        .record_observation("normal", "[email] 新邮件", None)
        .unwrap();
    store
        .record_observation("urgent", "紧急会议", Some("raw"))
        .unwrap();
    store
        .record_observation("scheduled", "Morning Brief", None)
        .unwrap();

    let all = store.load_unprocessed_observations(10).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].category, "normal");
    assert_eq!(all[2].category, "scheduled");
    assert!(all[0].id > 0);
}

#[test]
fn test_mark_observations_processed() {
    let store = Store::open_in_memory().unwrap();
    store.record_observation("a", "obs1", None).unwrap();
    store.record_observation("b", "obs2", None).unwrap();
    store.record_observation("c", "obs3", None).unwrap();

    let all = store.load_unprocessed_observations(10).unwrap();
    assert_eq!(all.len(), 3);

    store
        .mark_observations_processed(&[all[0].id, all[1].id])
        .unwrap();

    let remaining = store.load_unprocessed_observations(10).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].observation, "obs3");
}

#[test]
fn test_mark_empty_ids() {
    let store = Store::open_in_memory().unwrap();
    store.mark_observations_processed(&[]).unwrap();
}

#[test]
fn test_search_memories() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("values", "重视团队成长胜过个人表现", "chat", 0.8)
        .unwrap();
    store
        .save_memory("thinking", "用系统思考分析问题", "chat", 0.7)
        .unwrap();
    store
        .save_memory("behavior", "每天下午三点查邮件", "chat", 0.6)
        .unwrap();

    let results = store.search_memories("团队", 5).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].content.contains("团队"));

    let results = store.search_memories("邮件", 5).unwrap();
    assert_eq!(results.len(), 1);

    let results = store.search_memories("values", 5).unwrap();
    assert_eq!(results.len(), 1);

    let all = store.search_memories("", 10).unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn test_search_memories_update() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("growth", "想学习 Rust 异步编程", "chat", 0.7)
        .unwrap();

    store
        .update_memory(id, "已掌握 Rust 异步编程基础", 0.9)
        .unwrap();
    let results = store.search_memories("异步编程", 5).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].content.contains("已掌握"));
}

#[test]
fn test_search_memories_delete() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("emotion", "会议压力大", "chat", 0.5)
        .unwrap();

    store.delete_memory(id).unwrap();
    let results = store.search_memories("压力", 5).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_get_memory_context_empty() {
    let store = Store::open_in_memory().unwrap();
    let ctx = store.get_memory_context(2000).unwrap();
    assert!(ctx.is_empty(), "空数据库应返回空字符串，但得到: {ctx:?}");
}

#[test]
fn test_get_memory_context_includes_all_categories() {
    let store = Store::open_in_memory().unwrap();

    store
        .save_memory("identity", "重视团队成长", "test", 0.9)
        .unwrap();
    store
        .save_memory("behavior", "每天下午查邮件", "test", 0.7)
        .unwrap();
    store
        .save_memory("task", "选择 Rust 作为核心语言", "test", 0.8)
        .unwrap();
    store
        .save_memory("coach_insight", "Alex 决策偏系统思考", "test", 0.8)
        .unwrap();

    let ctx = store.get_memory_context(10000).unwrap();

    // Phase 1a: depth-based layout (axiom/procedural/semantic/episodic)
    // identity/values → axiom → "## 信念（始终有效）"
    // coach_insight → procedural → "## 判断模式"
    // behavior → semantic → "## 行为模式"
    // task → episodic (or fallback to tier-based for old data without deep depth)
    assert!(ctx.contains("重视团队成长"), "应包含信念内容");
    assert!(ctx.contains("每天下午查邮件"), "应包含行为内容");
    assert!(ctx.contains("选择 Rust 作为核心语言"), "应包含任务内容");
    assert!(ctx.contains("Alex 决策偏系统思考"), "应包含教练洞察内容");
}

#[test]
fn test_get_memory_context_truncation() {
    let store = Store::open_in_memory().unwrap();
    let long_content = "A".repeat(500);
    store
        .save_memory("core", &long_content, "test", 0.9)
        .unwrap();
    store
        .save_memory("pattern", &long_content, "test", 0.7)
        .unwrap();
    store
        .save_memory("decision", &long_content, "test", 0.8)
        .unwrap();

    let ctx = store.get_memory_context(300).unwrap();
    assert!(ctx.len() <= 300, "截断后字节数 {} 应 ≤ 300", ctx.len());
    assert!(
        std::str::from_utf8(ctx.as_bytes()).is_ok(),
        "截断结果应为有效 UTF-8"
    );
}

#[test]
fn test_get_memory_context_utf8_safe_truncation() {
    let store = Store::open_in_memory().unwrap();
    let chinese_content = "重视团队成长胜过个人表现，这是核心价值观。";
    store
        .save_memory("core", chinese_content, "test", 0.9)
        .unwrap();

    let ctx = store.get_memory_context(20).unwrap();
    assert!(
        std::str::from_utf8(ctx.as_bytes()).is_ok(),
        "中文截断后应仍为有效 UTF-8，得到字节数: {}",
        ctx.len()
    );
    assert!(ctx.len() <= 20, "截断后字节数 {} 应 ≤ 20", ctx.len());
}

#[test]
fn test_append_pattern_stores_and_searchable() {
    let store = Store::open_in_memory().unwrap();

    let id = store
        .append_pattern("behavior", "每天下午三点查邮件")
        .unwrap();
    assert!(id > 0, "append_pattern 应返回正整数 id");

    let results = store.search_memories("下午三点", 5).unwrap();
    assert_eq!(results.len(), 1, "应找到 1 条 pattern");
    assert_eq!(results[0].category, "pattern");
    assert!(results[0].content.contains("每天下午三点查邮件"));
    assert!(results[0].content.contains("behavior"));
}

#[test]
fn test_append_decision_stores_and_searchable() {
    let store = Store::open_in_memory().unwrap();

    let id = store
        .append_decision("架构选型", "选择 Rust 实现 ProjectX 核心")
        .unwrap();
    assert!(id > 0, "append_decision 应返回正整数 id");

    let results = store.search_memories("Rust", 5).unwrap();
    assert_eq!(results.len(), 1, "应找到 1 条 decision");
    assert_eq!(results[0].category, "decision");
    assert!(results[0].content.contains("Context"));
    assert!(results[0].content.contains("Decision"));
    assert!(results[0].content.contains("架构选型"));
}

#[test]
fn test_get_today_handled_actions() {
    let store = Store::open_in_memory().unwrap();

    store
        .append_decision("Morning Brief", "今日日程...")
        .unwrap();
    store.append_decision("Email Check", "2封未读邮件").unwrap();

    let actions = store.get_today_handled_actions().unwrap();
    assert_eq!(actions.len(), 2);
    assert!(actions.contains(&"Morning Brief".to_string()));
    assert!(actions.contains(&"Email Check".to_string()));
}

#[test]
fn test_save_coach_insight_stores_and_searchable() {
    let store = Store::open_in_memory().unwrap();

    let id = store
        .save_coach_insight("Alex 在压力下倾向于系统化思考而非直觉决策")
        .unwrap();
    assert!(id > 0, "save_coach_insight 应返回正整数 id");

    let results = store.search_memories("系统化思考", 5).unwrap();
    assert_eq!(results.len(), 1, "应找到 1 条 coach_insight");
    assert_eq!(results[0].category, "coach_insight");
    assert_eq!(results[0].source, "coach");
}

#[test]
fn test_append_pattern_appears_in_context() {
    let store = Store::open_in_memory().unwrap();

    store
        .append_pattern("communication", "偏好直接沟通")
        .unwrap();
    store
        .append_decision("工具选型", "使用 Claude Code")
        .unwrap();
    store.save_coach_insight("主动学习型用户").unwrap();

    let ctx = store.get_memory_context(10000).unwrap();

    assert!(ctx.contains("偏好直接沟通"), "pattern 应出现在上下文");
    assert!(ctx.contains("工具选型"), "decision 应出现在上下文");
    assert!(
        ctx.contains("主动学习型用户"),
        "coach_insight 应出现在上下文"
    );
}

#[test]
fn test_get_recent_messages_for_prompt() {
    let store = Store::open_in_memory().unwrap();
    let sid = "test-session-window";

    for i in 0..50 {
        let role = if i % 2 == 0 { "user" } else { "sage" };
        store
            .save_chat_message(role, &format!("消息 {}", i), sid)
            .unwrap();
    }

    let messages = store.get_recent_messages_for_prompt(sid, 20).unwrap();
    assert_eq!(messages.len(), 20, "应返回 20 条消息");
    assert!(
        messages.last().unwrap().content.contains("49"),
        "最后一条应是最新消息（索引49）"
    );
    assert!(
        messages.first().unwrap().content.contains("30"),
        "第一条应是窗口开始处的消息（索引30）"
    );
}

#[test]
fn test_recent_messages_preserves_order() {
    let store = Store::open_in_memory().unwrap();
    let sid = "test-session-order";

    for i in 0..5 {
        store
            .save_chat_message("user", &format!("消息 {}", i), sid)
            .unwrap();
    }

    let messages = store.get_recent_messages_for_prompt(sid, 10).unwrap();
    assert_eq!(messages.len(), 5);

    let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(ids, sorted_ids, "消息应按 id 正序返回（时间正序）");

    assert!(messages[0].content.contains("0"), "第一条应是最早的消息");
    assert!(messages[4].content.contains("4"), "最后一条应是最新的消息");
}

#[test]
fn test_recent_messages_limit_less_than_total() {
    let store = Store::open_in_memory().unwrap();
    let sid = "test-session-small";

    for i in 0..5 {
        store
            .save_chat_message("user", &format!("msg {}", i), sid)
            .unwrap();
    }

    let messages = store.get_recent_messages_for_prompt(sid, 20).unwrap();
    assert_eq!(messages.len(), 5, "消息总数少于 limit 时应返回全部");
}

#[test]
fn test_sync_to_claude_memory_creates_file() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("identity", "Software team lead", "test", 0.9)
        .unwrap();
    store
        .save_memory(
            "values",
            "Team growth over individual performance",
            "test",
            0.8,
        )
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    store.sync_to_claude_memory(dir.path()).unwrap();

    let content = std::fs::read_to_string(dir.path().join("MEMORY.md")).unwrap();
    assert!(content.contains("SAGE_SYNC_START"));
    assert!(content.contains("SAGE_SYNC_END"));
    assert!(content.contains("Software team lead"));
    assert!(content.contains("Team growth"));
    assert!(content.contains("Sage Shared Memory"));
}

#[test]
fn test_sync_preserves_existing_content() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("identity", "Test user", "test", 0.9)
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let memory_file = dir.path().join("MEMORY.md");
    std::fs::write(&memory_file, "# My Project\n\nManual notes here.\n").unwrap();

    store.sync_to_claude_memory(dir.path()).unwrap();

    let content = std::fs::read_to_string(&memory_file).unwrap();
    assert!(content.contains("# My Project"), "manual content preserved");
    assert!(
        content.contains("Manual notes here"),
        "manual content preserved"
    );
    assert!(content.contains("Test user"), "sync content added");
}

#[test]
fn test_sync_replaces_existing_sync_section() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("identity", "Updated info", "test", 0.9)
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let memory_file = dir.path().join("MEMORY.md");
    std::fs::write(
        &memory_file,
        "# Project\n\n<!-- SAGE_SYNC_START -->\nold data\n<!-- SAGE_SYNC_END -->\n\n# Footer\n",
    )
    .unwrap();

    store.sync_to_claude_memory(dir.path()).unwrap();

    let content = std::fs::read_to_string(&memory_file).unwrap();
    assert!(!content.contains("old data"), "old sync section replaced");
    assert!(content.contains("Updated info"), "new sync content present");
    assert!(content.contains("# Footer"), "content after sync preserved");
}

#[test]
fn test_replace_sync_section_static() {
    let existing = "# Header\n\nSome content.\n\n<!-- SAGE_SYNC_START -->\nold\n<!-- SAGE_SYNC_END -->\n\nMore content.\n";
    let result = Store::replace_sync_section(
        existing,
        "<!-- SAGE_SYNC_START -->\nnew\n<!-- SAGE_SYNC_END -->",
    );
    assert!(result.contains("# Header"));
    assert!(result.contains("new"));
    assert!(!result.contains("old"));
    assert!(result.contains("More content."));
}

#[test]
fn test_save_and_load_report() {
    let store = Store::open_in_memory().unwrap();
    store.save_report("weekly", "本周报告内容").unwrap();
    store.save_report("weekly", "更新的周报").unwrap();
    store.save_report("morning", "早间 brief").unwrap();

    // 同天同类型 upsert：第二次 save 应更新而非新增
    let latest = store.get_latest_report("weekly").unwrap();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().content, "更新的周报");

    let all = store.get_reports("weekly", 10).unwrap();
    assert_eq!(all.len(), 1); // 同天只保留一条
}

#[test]
fn test_get_latest_report_empty() {
    let store = Store::open_in_memory().unwrap();
    let result = store.get_latest_report("morning").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_get_all_reports() {
    let store = Store::open_in_memory().unwrap();
    store.save_report("morning", "早间报告").unwrap();
    store.save_report("evening", "晚间回顾").unwrap();
    store.save_report("weekly", "周报").unwrap();

    let all = store.get_all_reports(10).unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn test_get_memories_since() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("decision", "chose Rust", "chat", 0.8)
        .unwrap();
    store
        .save_memory("identity", "Alex is a team lead", "chat", 0.9)
        .unwrap();

    let memories = store
        .get_memories_since("2000-01-01T00:00:00+00:00")
        .unwrap();
    assert_eq!(memories.len(), 2);

    let empty = store
        .get_memories_since("2099-01-01T00:00:00+00:00")
        .unwrap();
    assert!(empty.is_empty());
}

#[test]
fn test_count_observations_since() {
    let store = Store::open_in_memory().unwrap();
    store.record_observation("pattern", "obs1", None).unwrap();
    store.record_observation("habit", "obs2", None).unwrap();

    let count = store
        .count_observations_since("2000-01-01T00:00:00+00:00")
        .unwrap();
    assert_eq!(count, 2);

    let zero = store
        .count_observations_since("2099-01-01T00:00:00+00:00")
        .unwrap();
    assert_eq!(zero, 0);
}

#[test]
fn test_get_session_summaries_since() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory(
            "session",
            "[session] fix bugs — 50 msgs",
            "claude-code",
            0.8,
        )
        .unwrap();
    store
        .save_memory("decision", "chose async", "chat", 0.7)
        .unwrap();

    let sessions = store
        .get_session_summaries_since("2000-01-01T00:00:00+00:00")
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].category, "session");
}

#[test]
fn test_get_coach_insights_since() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("coach_insight", "Alex 偏系统思考", "coach", 0.8)
        .unwrap();
    store
        .save_memory("coach_insight", "喜欢类比解释", "coach", 0.7)
        .unwrap();
    store
        .save_memory("decision", "not a coach insight", "chat", 0.5)
        .unwrap();

    let insights = store
        .get_coach_insights_since("2000-01-01T00:00:00+00:00")
        .unwrap();
    assert_eq!(insights.len(), 2);
    assert!(insights.iter().all(|s| !s.is_empty()));
}

#[test]
fn test_load_active_memories() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("behavior", "active memory", "test", 0.8)
        .unwrap();
    store
        .save_memory("behavior", "another active", "test", 0.6)
        .unwrap();

    let active = store.load_active_memories().unwrap();
    assert_eq!(active.len(), 2);
    assert!(active[0].confidence >= active[1].confidence);
}

#[test]
fn test_load_active_excludes_expired() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("behavior", "active one", "test", 0.8)
        .unwrap();

    let id = store
        .save_memory("behavior", "will expire", "test", 0.3)
        .unwrap();
    let conn = store.conn.lock().unwrap();
    conn.execute(
        "UPDATE memories SET status = 'expired' WHERE id = ?1",
        rusqlite::params![id],
    )
    .unwrap();
    drop(conn);

    let active = store.load_active_memories().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].content, "active one");
}

#[test]
fn test_decay_stale_archive_memories() {
    let store = Store::open_in_memory().unwrap();

    let id = store
        .save_memory("pattern", "old pattern", "test", 0.5)
        .unwrap();
    let old_date = (chrono::Local::now() - chrono::Duration::days(90))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let conn = store.conn.lock().unwrap();
    conn.execute(
        "UPDATE memories SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![old_date, id],
    )
    .unwrap();
    drop(conn);

    // Phase 1a: decay is disabled — function always returns 0, memory is unchanged
    let decayed = store.decay_stale_archive_memories(60, 0.1, 0.2).unwrap();
    assert_eq!(decayed, 0);

    let memories = store.load_memories().unwrap();
    let m = memories.iter().find(|m| m.id == id).unwrap();
    // confidence unchanged (no decay)
    assert!((m.confidence - 0.5).abs() < 0.01);
}

#[test]
fn test_decay_expires_low_confidence() {
    let store = Store::open_in_memory().unwrap();

    let id = store
        .save_memory("pattern", "weak pattern", "test", 0.2)
        .unwrap();
    let old_date = (chrono::Local::now() - chrono::Duration::days(90))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let conn = store.conn.lock().unwrap();
    conn.execute(
        "UPDATE memories SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![old_date, id],
    )
    .unwrap();
    drop(conn);

    // Phase 1a: decay is disabled — memory remains active
    store.decay_stale_archive_memories(60, 0.1, 0.2).unwrap();

    let active = store.load_active_memories().unwrap();
    assert!(active.iter().any(|m| m.id == id), "memory should still be active (decay disabled)");
}

#[test]
fn test_promote_high_confidence_memories() {
    let store = Store::open_in_memory().unwrap();

    let id = store
        .save_memory("behavior", "consistent pattern", "coach", 0.9)
        .unwrap();
    store
        .update_memory(id, "consistent pattern (confirmed)", 0.9)
        .unwrap();

    let promoted = store.promote_high_confidence_memories(0.85).unwrap();
    assert_eq!(promoted, 1);

    let conn = store.conn.lock().unwrap();
    let tier: String = conn
        .query_row(
            "SELECT tier FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tier, "core");
}

#[test]
fn test_promote_ignores_unconfirmed() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("behavior", "new observation", "coach", 0.9)
        .unwrap();

    let promoted = store.promote_high_confidence_memories(0.85).unwrap();
    assert_eq!(promoted, 0);
}

#[test]
fn test_promote_ignores_wrong_categories() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("identity", "I am Alex", "user", 0.95)
        .unwrap();
    store
        .update_memory(id, "I am Alex (confirmed)", 0.95)
        .unwrap();

    let promoted = store.promote_high_confidence_memories(0.85).unwrap();
    assert_eq!(promoted, 0);
}

#[test]
fn test_save_and_search_open_question() {
    let store = Store::open_in_memory().unwrap();

    let id = store
        .save_open_question("你为什么选择这个方向？", None)
        .unwrap();
    assert!(id > 0);

    let results = store.search_open_questions("方向").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, id);
    assert!(results[0].1.contains("方向"));
}

#[test]
fn test_open_question_with_suggestion_link() {
    let store = Store::open_in_memory().unwrap();
    let suggestion_id = store
        .record_suggestion("questioner", "daily-question", "test q")
        .unwrap();
    let q_id = store
        .save_open_question("test question", Some(suggestion_id))
        .unwrap();
    assert!(q_id > 0);
}

#[test]
fn test_get_due_questions_respects_time() {
    let store = Store::open_in_memory().unwrap();
    store.save_open_question("future question", None).unwrap();

    let due = store.get_due_questions(10).unwrap();
    assert!(due.is_empty(), "新问题不应该立即到期");
}

#[test]
fn test_get_due_questions_returns_past_due() {
    let store = Store::open_in_memory().unwrap();

    let id = store.save_open_question("overdue question", None).unwrap();
    let past = (chrono::Local::now() - chrono::Duration::hours(1))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let conn = store.conn.lock().unwrap();
    conn.execute(
        "UPDATE open_questions SET next_ask_at = ?1 WHERE id = ?2",
        rusqlite::params![past, id],
    )
    .unwrap();
    drop(conn);

    let due = store.get_due_questions(10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].0, id);
}

#[test]
fn test_answer_question() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_open_question("will be answered", None).unwrap();
    store.answer_question(id).unwrap();

    let results = store.search_open_questions("answered").unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_bump_question_ask_increments() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_open_question("bump test", None).unwrap();
    store.bump_question_ask(id).unwrap();

    let conn = store.conn.lock().unwrap();
    let (count, status): (i32, String) = conn
        .query_row(
            "SELECT ask_count, status FROM open_questions WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    drop(conn);
    assert_eq!(count, 2);
    assert_eq!(status, "open");
}

#[test]
fn test_bump_question_archives_after_max() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_open_question("will archive", None).unwrap();

    let conn = store.conn.lock().unwrap();
    conn.execute(
        "UPDATE open_questions SET ask_count = 3 WHERE id = ?1",
        rusqlite::params![id],
    )
    .unwrap();
    drop(conn);

    store.bump_question_ask(id).unwrap();

    let conn = store.conn.lock().unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM open_questions WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "archived");
}

#[test]
fn test_due_questions_excludes_answered_and_archived() {
    let store = Store::open_in_memory().unwrap();

    let id1 = store.save_open_question("answered q", None).unwrap();
    let id2 = store.save_open_question("archived q", None).unwrap();
    let id3 = store.save_open_question("open q", None).unwrap();

    store.answer_question(id1).unwrap();

    let conn = store.conn.lock().unwrap();
    conn.execute(
        "UPDATE open_questions SET status = 'archived' WHERE id = ?1",
        rusqlite::params![id2],
    )
    .unwrap();
    let past = (chrono::Local::now() - chrono::Duration::hours(1))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    conn.execute(
        "UPDATE open_questions SET next_ask_at = ?1",
        rusqlite::params![past],
    )
    .unwrap();
    drop(conn);

    let due = store.get_due_questions(10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].0, id3);
}

#[test]
fn test_chat_auto_answer_flow() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_open_question("你对团队协作有什么看法？", None)
        .unwrap();

    let matches = store.search_open_questions("团队协作").unwrap();
    assert!(!matches.is_empty());
    assert_eq!(matches[0].0, id);

    store.answer_question(matches[0].0).unwrap();

    let matches_after = store.search_open_questions("团队协作").unwrap();
    assert!(matches_after.is_empty());
}

#[test]
fn test_observer_note_tier_and_ttl() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("observer_note", "邮件频率增加 ← 本周第3封", "observer", 0.6)
        .unwrap();

    let conn = store.conn.lock().unwrap();
    let (tier, expires_at): (String, Option<String>) = conn
        .query_row(
            "SELECT tier, expires_at FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(tier, "working");
    assert!(expires_at.is_some(), "observer_note should have expires_at");
}

#[test]
fn test_load_observer_notes_recent_empty() {
    let store = Store::open_in_memory().unwrap();
    let notes = store.load_observer_notes_recent().unwrap();
    assert!(notes.is_empty());
}

#[test]
fn test_load_observer_notes_recent_returns_today() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("observer_note", "note1 ← 首次出现", "observer", 0.6)
        .unwrap();
    store
        .save_memory("observer_note", "note2 ← 本周第2次", "observer", 0.6)
        .unwrap();
    store
        .save_memory("behavior", "some behavior", "chat", 0.8)
        .unwrap();

    let notes = store.load_observer_notes_recent().unwrap();
    assert_eq!(notes.len(), 2);
    assert!(notes[0].contains("note1"));
    assert!(notes[1].contains("note2"));
}

#[test]
fn test_save_and_get_memory_edge() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_memory("behavior", "喜欢直接沟通", "chat", 0.7)
        .unwrap();
    let id2 = store
        .save_memory("values", "重视团队协作", "chat", 0.8)
        .unwrap();

    let edge_id = store.save_memory_edge(id1, id2, "supports", 0.6).unwrap();
    assert!(edge_id > 0);

    let edges = store.get_memory_edges(id1).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from_id, id1);
    assert_eq!(edges[0].to_id, id2);
    assert_eq!(edges[0].relation, "supports");
    assert!((edges[0].weight - 0.6).abs() < 0.01);
}

#[test]
fn test_edge_bidirectional_query() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

    store.save_memory_edge(id1, id2, "causes", 0.8).unwrap();

    let edges = store.get_memory_edges(id2).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from_id, id1);
}

#[test]
fn test_edge_upsert_updates_weight() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

    store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();
    store.save_memory_edge(id1, id2, "similar", 0.9).unwrap();

    let edges = store.get_memory_edges(id1).unwrap();
    assert_eq!(edges.len(), 1);
    assert!((edges[0].weight - 0.9).abs() < 0.01);
}

#[test]
fn test_edge_different_relations() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

    store.save_memory_edge(id1, id2, "supports", 0.5).unwrap();
    store
        .save_memory_edge(id1, id2, "co_occurred", 0.3)
        .unwrap();

    let edges = store.get_memory_edges(id1).unwrap();
    assert_eq!(edges.len(), 2);
}

#[test]
fn test_get_all_memory_edges() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    let id3 = store.save_memory("values", "C", "chat", 0.8).unwrap();

    store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();
    store.save_memory_edge(id2, id3, "supports", 0.7).unwrap();

    let all_edges = store.get_all_memory_edges().unwrap();
    assert_eq!(all_edges.len(), 2);
}

#[test]
fn test_delete_memory_edge() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

    let edge_id = store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();
    store.delete_memory_edge(edge_id).unwrap();

    let edges = store.get_memory_edges(id1).unwrap();
    assert_eq!(edges.len(), 0);
}

#[test]
fn test_count_memory_edges() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.count_memory_edges().unwrap(), 0);

    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    store.save_memory_edge(id1, id2, "similar", 0.5).unwrap();

    assert_eq!(store.count_memory_edges().unwrap(), 1);
}

#[test]
fn test_connected_memories_traversal() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_memory("behavior", "A - 起点", "chat", 0.7)
        .unwrap();
    let id2 = store
        .save_memory("behavior", "B - 一跳", "chat", 0.7)
        .unwrap();
    let id3 = store
        .save_memory("values", "C - 两跳", "chat", 0.8)
        .unwrap();

    store.save_memory_edge(id1, id2, "supports", 0.8).unwrap();
    store.save_memory_edge(id2, id3, "causes", 0.9).unwrap();

    let hop1 = store.get_connected_memories(id1, 1).unwrap();
    assert_eq!(hop1.len(), 1);
    assert_eq!(hop1[0].0.id, id2);

    let hop2 = store.get_connected_memories(id1, 2).unwrap();
    assert_eq!(hop2.len(), 2);
    assert!(hop2[0].1 > hop2[1].1);
}

#[test]
fn test_connected_memories_empty() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_memory("behavior", "孤立节点", "chat", 0.7)
        .unwrap();

    let result = store.get_connected_memories(id1, 3).unwrap();
    assert_eq!(result.len(), 0);
}

#[test]
fn test_coach_reads_observer_notes_over_raw_obs() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory(
            "observer_note",
            "Morning Brief ← 今天第2次触发",
            "observer",
            0.6,
        )
        .unwrap();
    store
        .record_observation("scheduled", "Morning Brief", None)
        .unwrap();

    let notes = store.load_observer_notes_recent().unwrap();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("第2次触发"));

    let raw = store.load_unprocessed_observations(50).unwrap();
    assert_eq!(raw.len(), 1);
}

#[test]
fn test_save_browser_behavior() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_browser_behavior("chatgpt", "conversation_start", r#"{"topic":"rust"}"#)
        .unwrap();
    store
        .save_browser_behavior("claude", "memory_created", r#"{"count":3}"#)
        .unwrap();
    let rows = store.get_browser_behaviors(10).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].source, "claude");
    assert_eq!(rows[1].source, "chatgpt");
}

#[test]
fn test_count_memories() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("identity", "test memory", "chat", 0.9)
        .unwrap();
    store
        .save_memory("values", "another one", "import", 0.8)
        .unwrap();
    let count = store.count_memories().unwrap();
    assert_eq!(count, 2);
}

#[test]
fn test_browser_behavior_empty() {
    let store = Store::open_in_memory().unwrap();
    let rows = store.get_browser_behaviors(10).unwrap();
    assert!(rows.is_empty());
}

#[test]
fn test_save_memory_with_browser_source() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory(
            "behavior",
            "uses chatgpt for brainstorming",
            "browser:chatgpt",
            0.7,
        )
        .unwrap();
    assert!(id > 0);
    let mems = store.search_memories("brainstorming", 10).unwrap();
    assert_eq!(mems.len(), 1);
    assert_eq!(mems[0].source, "browser:chatgpt");
}

#[test]
fn test_add_and_get_tags() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("behavior", "早起跑步", "chat", 0.8)
        .unwrap();

    store.add_tag(id, "health").unwrap();
    store.add_tag(id, "Routine").unwrap();

    let tags = store.get_tags(id).unwrap();
    assert_eq!(tags, vec!["health", "routine"]);
}

#[test]
fn test_add_tag_idempotent() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("values", "注重团队", "chat", 0.9)
        .unwrap();

    store.add_tag(id, "leadership").unwrap();
    store.add_tag(id, "leadership").unwrap();
    store.add_tag(id, " Leadership ").unwrap();

    let tags = store.get_tags(id).unwrap();
    assert_eq!(tags.len(), 1);
}

#[test]
fn test_add_tags_batch() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("thinking", "系统思维", "chat", 0.7)
        .unwrap();

    store.add_tags(id, &["work", "cognition", ""]).unwrap();

    let tags = store.get_tags(id).unwrap();
    assert_eq!(tags, vec!["cognition", "work"]);
}

#[test]
fn test_remove_tag() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("emotion", "对加班敏感", "chat", 0.6)
        .unwrap();

    store.add_tags(id, &["stress", "work"]).unwrap();
    store.remove_tag(id, "stress").unwrap();

    let tags = store.get_tags(id).unwrap();
    assert_eq!(tags, vec!["work"]);
}

#[test]
fn test_get_all_tags_counts() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "跑步", "chat", 0.8).unwrap();
    let id2 = store.save_memory("behavior", "冥想", "chat", 0.7).unwrap();
    let id3 = store
        .save_memory("values", "健康优先", "chat", 0.9)
        .unwrap();

    store.add_tag(id1, "health").unwrap();
    store.add_tag(id2, "health").unwrap();
    store.add_tag(id3, "health").unwrap();
    store.add_tag(id1, "morning").unwrap();
    store.add_tag(id2, "mindfulness").unwrap();

    let all = store.get_all_tags().unwrap();
    assert_eq!(all[0], ("health".to_string(), 3));
    assert_eq!(all.len(), 3);
}

#[test]
fn test_get_memories_by_tag() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_memory("behavior", "写日记", "chat", 0.8)
        .unwrap();
    let id2 = store.save_memory("growth", "学 Rust", "chat", 0.9).unwrap();

    store.add_tag(id1, "daily").unwrap();
    store.add_tag(id2, "daily").unwrap();
    store.add_tag(id2, "coding").unwrap();

    let daily = store.get_memories_by_tag("daily").unwrap();
    assert_eq!(daily.len(), 2);
    assert!(daily.contains(&id1));
    assert!(daily.contains(&id2));

    let coding = store.get_memories_by_tag("coding").unwrap();
    assert_eq!(coding, vec![id2]);
}

#[test]
fn test_tags_cascade_on_memory_delete() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_memory("task", "买菜", "chat", 1.0).unwrap();
    store.add_tags(id, &["errand", "daily"]).unwrap();

    store.delete_memory(id).unwrap();

    let tags = store.get_tags(id).unwrap();
    assert!(tags.is_empty());
}

#[test]
fn test_edges_cascade_on_memory_delete() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    let id3 = store.save_memory("values", "C", "chat", 0.8).unwrap();

    store.save_memory_edge(id1, id2, "supports", 0.8).unwrap();
    store.save_memory_edge(id2, id3, "causes", 0.9).unwrap();
    assert_eq!(store.count_memory_edges().unwrap(), 2);

    store.delete_memory(id2).unwrap();
    assert_eq!(store.count_memory_edges().unwrap(), 0);
}

#[test]
fn test_connected_memories_with_cycle() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.8).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.8).unwrap();
    let c = store.save_memory("behavior", "C", "chat", 0.8).unwrap();

    store.save_memory_edge(a, b, "causes", 0.9).unwrap();
    store.save_memory_edge(b, c, "supports", 0.8).unwrap();
    store.save_memory_edge(c, a, "derived_from", 0.7).unwrap();

    let result = store.get_connected_memories(a, 5).unwrap();
    assert_eq!(result.len(), 2);
}

#[test]
fn test_edge_reverse_direction_separate() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "B", "chat", 0.7).unwrap();

    store.save_memory_edge(id1, id2, "supports", 0.5).unwrap();
    store.save_memory_edge(id2, id1, "supports", 0.8).unwrap();

    let all = store.get_all_memory_edges().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn test_connected_memories_activation_decay() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "起点", "chat", 0.9).unwrap();
    let b = store.save_memory("behavior", "一跳", "chat", 0.8).unwrap();
    let c = store.save_memory("values", "两跳", "chat", 0.7).unwrap();
    let d = store.save_memory("values", "三跳", "chat", 0.6).unwrap();

    store.save_memory_edge(a, b, "supports", 1.0).unwrap();
    store.save_memory_edge(b, c, "causes", 1.0).unwrap();
    store.save_memory_edge(c, d, "supports", 1.0).unwrap();

    let result = store.get_connected_memories(a, 3).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].0.id, b);
    assert_eq!(result[1].0.id, c);
    assert_eq!(result[2].0.id, d);
    assert!((result[0].1 - 0.7).abs() < 0.01);
    assert!((result[1].1 - 0.49).abs() < 0.01);
    assert!((result[2].1 - 0.343).abs() < 0.01);
}

#[test]
fn test_search_with_graph_returns_neighbors() {
    let store = Store::open_in_memory().unwrap();
    let a = store
        .save_memory("behavior", "每天早起跑步", "chat", 0.8)
        .unwrap();
    let b = store
        .save_memory("values", "重视健康", "chat", 0.7)
        .unwrap();
    store.save_memory_edge(a, b, "supports", 0.9).unwrap();

    let results = store.search_memories_with_graph("跑步", 5, 10).unwrap();
    let ids: Vec<i64> = results.iter().map(|m| m.id).collect();
    assert!(ids.contains(&a), "seed should be found");
    assert!(ids.contains(&b), "graph neighbor should be included");
}

#[test]
fn test_search_with_graph_deduplicates() {
    let store = Store::open_in_memory().unwrap();
    let a = store
        .save_memory("behavior", "喜欢写代码", "chat", 0.9)
        .unwrap();
    let b = store
        .save_memory("behavior", "写代码很快乐", "chat", 0.8)
        .unwrap();
    store.save_memory_edge(a, b, "similar", 0.8).unwrap();

    let results = store.search_memories_with_graph("代码", 5, 10).unwrap();
    let b_count = results.iter().filter(|m| m.id == b).count();
    assert_eq!(b_count, 1, "should deduplicate");
}

#[test]
fn test_search_with_graph_respects_total_limit() {
    let store = Store::open_in_memory().unwrap();
    for i in 0..10 {
        store
            .save_memory(
                "behavior",
                &format!("测试记忆{i}"),
                "chat",
                0.5 + i as f64 * 0.05,
            )
            .unwrap();
    }
    let results = store.search_memories_with_graph("测试", 10, 3).unwrap();
    assert_eq!(results.len(), 3, "should respect total_limit");
}

#[test]
fn test_strengthen_creates_new_edge() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    assert_eq!(store.count_memory_edges().unwrap(), 0);

    let n = store.strengthen_edges(&[a, b]).unwrap();
    assert_eq!(n, 1);

    let edges = store.get_memory_edges(a).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].relation, "co_occurred");
    assert!((edges[0].weight - 0.3).abs() < 0.01);
}

#[test]
fn test_strengthen_boosts_existing_edge() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    store.save_memory_edge(a, b, "supports", 0.5).unwrap();

    store.strengthen_edges(&[a, b]).unwrap();

    let edges = store.get_memory_edges(a).unwrap();
    assert_eq!(edges.len(), 1);
    assert!(
        (edges[0].weight - 0.55).abs() < 0.01,
        "should be 0.5 + 0.05"
    );
}

#[test]
fn test_strengthen_caps_at_1() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    store.save_memory_edge(a, b, "causes", 0.98).unwrap();

    store.strengthen_edges(&[a, b]).unwrap();

    let edges = store.get_memory_edges(a).unwrap();
    assert!((edges[0].weight - 1.0).abs() < 0.01, "should cap at 1.0");
}

#[test]
fn test_strengthen_single_id_noop() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let n = store.strengthen_edges(&[a]).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn test_strengthen_multiple_pairs() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    let c = store.save_memory("values", "C", "chat", 0.8).unwrap();

    let n = store.strengthen_edges(&[a, b, c]).unwrap();
    assert_eq!(n, 3);
    assert_eq!(store.count_memory_edges().unwrap(), 3);
}

#[test]
fn test_decay_cold_edges_decreases_weight() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    store.save_memory_edge(a, b, "supports", 0.8).unwrap();

    store.decay_cold_edges(0, 0.5, 0.1).unwrap();

    let edges = store.get_memory_edges(a).unwrap();
    assert_eq!(edges.len(), 1);
    assert!((edges[0].weight - 0.4).abs() < 0.01, "0.8 * 0.5 = 0.4");
}

#[test]
fn test_decay_cold_edges_deletes_below_threshold() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    store.save_memory_edge(a, b, "similar", 0.15).unwrap();

    store.decay_cold_edges(0, 0.5, 0.1).unwrap();

    assert_eq!(
        store.count_memory_edges().unwrap(),
        0,
        "should delete edge below threshold"
    );
}

#[test]
fn test_decay_skips_recently_activated() {
    let store = Store::open_in_memory().unwrap();
    let a = store.save_memory("behavior", "A", "chat", 0.7).unwrap();
    let b = store.save_memory("behavior", "B", "chat", 0.7).unwrap();
    store.strengthen_edges(&[a, b]).unwrap();

    store.decay_cold_edges(30, 0.5, 0.1).unwrap();

    let edges = store.get_memory_edges(a).unwrap();
    assert!(
        (edges[0].weight - 0.3).abs() < 0.01,
        "recently activated edge should not decay"
    );
}

#[test]
fn test_save_and_get_knowledge_edge() {
    let store = Store::open_in_memory().unwrap();
    let mem_id = store
        .save_memory("behavior", "likes running", "chat", 0.7)
        .unwrap();

    let edge_id = store
        .save_knowledge_edge("memory", mem_id, "message", 999, "references", 0.8)
        .unwrap();
    assert!(edge_id > 0);

    let edges = store.get_knowledge_edges("memory", mem_id).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from_type, "memory");
    assert_eq!(edges[0].to_type, "message");
    assert_eq!(edges[0].relation, "references");
}

#[test]
fn test_knowledge_edge_upsert() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 2, "references", 0.5)
        .unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 2, "references", 0.9)
        .unwrap();

    let edges = store.get_knowledge_edges("memory", 1).unwrap();
    assert_eq!(edges.len(), 1);
    assert!((edges[0].weight - 0.9).abs() < 0.01);
}

#[test]
fn test_knowledge_edge_different_relations() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 2, "references", 0.5)
        .unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 2, "triggers", 0.7)
        .unwrap();

    let edges = store.get_knowledge_edges("memory", 1).unwrap();
    assert_eq!(edges.len(), 2);
}

#[test]
fn test_knowledge_edge_bidirectional_query() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 2, "references", 0.8)
        .unwrap();

    let edges = store.get_knowledge_edges("message", 2).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from_type, "memory");
}

#[test]
fn test_get_knowledge_edges_between_types() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 10, "references", 0.8)
        .unwrap();
    store
        .save_knowledge_edge("memory", 2, "message", 11, "triggers", 0.6)
        .unwrap();
    store
        .save_knowledge_edge("memory", 3, "observation", 20, "supports", 0.7)
        .unwrap();

    let mem_msg = store
        .get_knowledge_edges_between_types("memory", "message", 10)
        .unwrap();
    assert_eq!(mem_msg.len(), 2);

    let mem_obs = store
        .get_knowledge_edges_between_types("memory", "observation", 10)
        .unwrap();
    assert_eq!(mem_obs.len(), 1);
}

#[test]
fn test_get_all_knowledge_edges() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 2, "references", 0.8)
        .unwrap();
    store
        .save_knowledge_edge("memory", 3, "question", 4, "answers", 0.9)
        .unwrap();

    let all = store.get_all_knowledge_edges().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn test_delete_knowledge_edge() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_knowledge_edge("memory", 1, "message", 2, "references", 0.5)
        .unwrap();
    store.delete_knowledge_edge(id).unwrap();

    assert_eq!(store.count_knowledge_edges().unwrap(), 0);
}

#[test]
fn test_count_knowledge_edges() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.count_knowledge_edges().unwrap(), 0);

    store
        .save_knowledge_edge("memory", 1, "message", 2, "references", 0.5)
        .unwrap();
    store
        .save_knowledge_edge("observation", 3, "memory", 4, "triggers", 0.7)
        .unwrap();
    assert_eq!(store.count_knowledge_edges().unwrap(), 2);
}

#[test]
fn test_knowledge_edge_cross_type_graph() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_knowledge_edge("memory", 1, "message", 10, "references", 0.9)
        .unwrap();
    store
        .save_knowledge_edge("message", 10, "memory", 2, "triggers", 0.8)
        .unwrap();

    let edges = store.get_knowledge_edges("message", 10).unwrap();
    assert_eq!(edges.len(), 2);
}

#[test]
fn test_save_and_get_message() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_message(
            "Alice",
            "#general",
            Some("Hello world"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    assert!(id > 0);

    let msgs = store.get_messages_by_channel("#general", 10).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].sender, "Alice");
    assert_eq!(msgs[0].content, Some("Hello world".to_string()));
    assert_eq!(msgs[0].source, "teams");
    assert_eq!(msgs[0].message_type, "text");
}

#[test]
fn test_save_message_without_content() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_message("Sam", "#dev", None, "teams", "file", "2026-03-12T11:00:00")
        .unwrap();

    let msgs = store.get_messages_by_channel("#dev", 10).unwrap();
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].content.is_none());
    assert_eq!(msgs[0].message_type, "file");
}

#[test]
fn test_get_messages_by_source() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_message(
            "A",
            "#ch1",
            Some("hi"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    store
        .save_message(
            "B",
            "inbox",
            Some("email"),
            "email",
            "text",
            "2026-03-12T10:01:00",
        )
        .unwrap();
    store
        .save_message(
            "C",
            "#ch2",
            Some("yo"),
            "teams",
            "text",
            "2026-03-12T10:02:00",
        )
        .unwrap();

    let teams = store.get_messages_by_source("teams", 10).unwrap();
    assert_eq!(teams.len(), 2);

    let email = store.get_messages_by_source("email", 10).unwrap();
    assert_eq!(email.len(), 1);
    assert_eq!(email[0].sender, "B");
}

#[test]
fn test_search_messages() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_message(
            "Alice",
            "#general",
            Some("meeting with Sam"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    store
        .save_message(
            "Carol",
            "#dev",
            Some("code review done"),
            "teams",
            "text",
            "2026-03-12T10:01:00",
        )
        .unwrap();

    let found = store.search_messages("meeting", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].sender, "Alice");

    let found_sender = store.search_messages("Carol", 10).unwrap();
    assert_eq!(found_sender.len(), 1);
}

#[test]
fn test_count_messages() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.count_messages().unwrap(), 0);

    store
        .save_message(
            "A",
            "#ch",
            Some("x"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    store
        .save_message(
            "B",
            "#ch",
            Some("y"),
            "slack",
            "text",
            "2026-03-12T10:01:00",
        )
        .unwrap();
    assert_eq!(store.count_messages().unwrap(), 2);
}

#[test]
fn test_delete_message() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_message(
            "A",
            "#ch",
            Some("tmp"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    store.delete_message(id).unwrap();
    assert_eq!(store.count_messages().unwrap(), 0);
}

#[test]
fn test_today_message_stats() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_message(
            "A",
            "#ch1",
            Some("a"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    store
        .save_message(
            "B",
            "#ch2",
            Some("b"),
            "teams",
            "text",
            "2026-03-12T10:01:00",
        )
        .unwrap();
    store
        .save_message(
            "C",
            "inbox",
            Some("c"),
            "email",
            "text",
            "2026-03-12T10:02:00",
        )
        .unwrap();

    let stats = store.get_today_message_stats().unwrap();
    assert!(stats.len() >= 2);
    assert_eq!(stats[0].0, "teams");
    assert_eq!(stats[0].1, 2);
}

#[test]
fn test_get_message_channels() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_message(
            "A",
            "#general",
            Some("hi"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    store
        .save_message(
            "B",
            "#general",
            Some("hey"),
            "teams",
            "text",
            "2026-03-12T10:01:00",
        )
        .unwrap();
    store
        .save_message(
            "C",
            "#dev",
            Some("PR"),
            "teams",
            "text",
            "2026-03-12T10:02:00",
        )
        .unwrap();
    store
        .save_message(
            "D",
            "inbox",
            Some("email"),
            "email",
            "text",
            "2026-03-12T10:03:00",
        )
        .unwrap();

    let channels = store.get_message_channels().unwrap();
    assert_eq!(channels.len(), 3);
    assert_eq!(channels[0].0, "#general");
    assert_eq!(channels[0].1, "teams");
    assert_eq!(channels[0].2, 2);
}

#[test]
fn test_save_message_dedup_by_timestamp() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_message(
            "Alice",
            "#general",
            Some("hello"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    let id2 = store
        .save_message(
            "Alice",
            "#general",
            Some("hello"),
            "teams",
            "text",
            "2026-03-12T10:00:00",
        )
        .unwrap();
    let id3 = store
        .save_message(
            "Alice",
            "#general",
            Some("world"),
            "teams",
            "text",
            "2026-03-12T10:01:00",
        )
        .unwrap();

    assert!(id1 > 0);
    assert_eq!(store.count_messages().unwrap(), 2);
    assert_ne!(id1, id3);
    let _ = id2;
}

#[test]
fn test_save_memory_default_visibility_is_public() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory("identity", "test content", "chat", 0.8)
        .unwrap();
    let mems = store.load_memories().unwrap();
    assert_eq!(mems.len(), 1);
    assert_eq!(mems[0].visibility, "public");
}

#[test]
fn test_save_memory_with_visibility() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory_with_visibility("emotion", "feeling tired", "chat", 0.7, "private")
        .unwrap();
    assert!(id > 0);
    let mems = store.load_memories().unwrap();
    assert_eq!(mems[0].visibility, "private");
}

#[test]
fn test_search_public_memories_filters_correctly() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory_with_visibility("identity", "public fact", "onboarding", 0.9, "public")
        .unwrap();
    store
        .save_memory_with_visibility("emotion", "private feeling", "chat", 0.8, "private")
        .unwrap();
    store
        .save_memory_with_visibility(
            "coach_insight",
            "subconscious pattern",
            "coach",
            0.7,
            "subconscious",
        )
        .unwrap();

    let public = store.search_public_memories("", 100).unwrap();
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].content, "public fact");

    let public_search = store.search_public_memories("fact", 100).unwrap();
    assert_eq!(public_search.len(), 1);

    let no_match = store.search_public_memories("feeling", 100).unwrap();
    assert!(no_match.is_empty());
}

#[test]
fn test_get_memories_by_visibility() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory_with_visibility("identity", "a", "chat", 0.9, "public")
        .unwrap();
    store
        .save_memory_with_visibility("emotion", "b", "chat", 0.8, "private")
        .unwrap();
    store
        .save_memory_with_visibility("emotion", "c", "chat", 0.7, "private")
        .unwrap();
    store
        .save_memory_with_visibility("pattern", "d", "coach", 0.6, "subconscious")
        .unwrap();

    assert_eq!(store.get_memories_by_visibility("public").unwrap().len(), 1);
    assert_eq!(
        store.get_memories_by_visibility("private").unwrap().len(),
        2
    );
    assert_eq!(
        store
            .get_memories_by_visibility("subconscious")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn test_update_memory_visibility() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_memory("identity", "test", "chat", 0.8).unwrap();
    assert_eq!(store.load_memories().unwrap()[0].visibility, "public");

    store.update_memory_visibility(id, "private").unwrap();
    assert_eq!(store.load_memories().unwrap()[0].visibility, "private");

    store.update_memory_visibility(id, "subconscious").unwrap();
    assert_eq!(store.load_memories().unwrap()[0].visibility, "subconscious");
}

#[test]
fn test_count_memories_by_visibility() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory_with_visibility("a", "x", "s", 0.9, "public")
        .unwrap();
    store
        .save_memory_with_visibility("b", "y", "s", 0.8, "public")
        .unwrap();
    store
        .save_memory_with_visibility("c", "z", "s", 0.7, "private")
        .unwrap();
    store
        .save_memory_with_visibility("d", "w", "s", 0.6, "subconscious")
        .unwrap();

    let counts = store.count_memories_by_visibility().unwrap();
    assert_eq!(counts[0], ("public".to_string(), 2));
    assert!(counts.iter().any(|(v, c)| v == "private" && *c == 1));
    assert!(counts.iter().any(|(v, c)| v == "subconscious" && *c == 1));
}

#[test]
fn test_migration_backfill_visibility() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory_with_visibility("coach_insight", "pattern X", "coach", 0.8, "subconscious")
        .unwrap();
    let search = store.search_memories("pattern", 10).unwrap();
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].visibility, "subconscious");
}

#[test]
fn test_graph_search_preserves_visibility() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_memory_with_visibility("identity", "node A", "chat", 0.9, "private")
        .unwrap();
    let id2 = store
        .save_memory_with_visibility("identity", "node B", "onboarding", 0.8, "public")
        .unwrap();
    store.save_memory_edge(id1, id2, "similar", 0.9).unwrap();

    let results = store.search_memories_with_graph("node", 10, 20).unwrap();
    assert!(results.len() >= 2);
    for m in &results {
        assert!(m.visibility == "private" || m.visibility == "public");
    }
}

#[test]
fn test_save_memory_about_person() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory_about_person("behavior", "对成本敏感", "chat", 0.8, "private", "David")
        .unwrap();
    let memories = store.load_memories().unwrap();
    let m = memories.iter().find(|m| m.id == id).unwrap();
    assert_eq!(m.about_person.as_deref(), Some("David"));
    assert_eq!(m.content, "对成本敏感");
}

#[test]
fn test_get_memories_about_person() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory_about_person("behavior", "技术能力强", "chat", 0.8, "private", "Alice")
        .unwrap();
    store
        .save_memory_about_person("behavior", "沟通直接", "chat", 0.7, "private", "Sam")
        .unwrap();
    store
        .save_memory("identity", "关于自己的记忆", "chat", 0.9)
        .unwrap();

    let alice_mems = store.get_memories_about_person("Alice").unwrap();
    assert_eq!(alice_mems.len(), 1);
    assert_eq!(alice_mems[0].content, "技术能力强");

    let bob_mems = store.get_memories_about_person("Sam").unwrap();
    assert_eq!(bob_mems.len(), 1);
}

#[test]
fn test_get_known_persons() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory_about_person("behavior", "记忆1", "chat", 0.8, "private", "Alice")
        .unwrap();
    store
        .save_memory_about_person("behavior", "记忆2", "chat", 0.7, "private", "Alice")
        .unwrap();
    store
        .save_memory_about_person("behavior", "记忆3", "chat", 0.6, "private", "Sam")
        .unwrap();
    store
        .save_memory("identity", "关于自己", "chat", 0.9)
        .unwrap();

    let persons = store.get_known_persons().unwrap();
    assert_eq!(persons, vec!["Alice", "Sam"]);
}

#[test]
fn test_about_person_null_by_default() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_memory("behavior", "普通记忆", "chat", 0.8)
        .unwrap();
    let memories = store.load_memories().unwrap();
    let m = memories.iter().find(|m| m.id == id).unwrap();
    assert!(m.about_person.is_none());
}

#[test]
fn test_about_person_with_graph() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_memory_about_person("behavior", "擅长前端", "chat", 0.8, "public", "Shengkai")
        .unwrap();
    let id2 = store
        .save_memory_about_person(
            "behavior",
            "喜欢 Pipeline",
            "chat",
            0.7,
            "public",
            "Shengkai",
        )
        .unwrap();
    store.save_memory_edge(id1, id2, "similar", 0.8).unwrap();

    let mems = store.get_memories_about_person("Shengkai").unwrap();
    assert_eq!(mems.len(), 2);
    for m in &mems {
        assert_eq!(m.about_person.as_deref(), Some("Shengkai"));
    }
}

#[test]
fn test_about_person_visibility_filter() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_memory_about_person("behavior", "公开特质", "chat", 0.8, "public", "Sarah")
        .unwrap();
    store
        .save_memory_about_person("behavior", "私人观察", "chat", 0.7, "private", "Sarah")
        .unwrap();

    let public = store.get_memories_by_visibility("public").unwrap();
    let sarah_public: Vec<_> = public
        .iter()
        .filter(|m| m.about_person.as_deref() == Some("Sarah"))
        .collect();
    assert_eq!(sarah_public.len(), 1);
    assert_eq!(sarah_public[0].content, "公开特质");
}

// ── v28: importance + kv_store 测试 ──

#[test]
fn test_save_task_signal_with_importance() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_task_signal_with_importance("new_task", None, "写测试", "发现缺少覆盖", None, 0.8)
        .unwrap();
    assert!(id > 0);

    // 通过 get_pending_signals 验证，importance 列存在且不为 DEFAULT 值（0.5）
    let conn = store.conn.lock().unwrap();
    let importance: f32 = conn
        .query_row(
            "SELECT importance FROM task_signals WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert!((importance - 0.8).abs() < 1e-4, "importance 应为 0.8，实际 {importance}");
}

#[test]
fn test_get_signal_accept_rate_empty() {
    let store = Store::open_in_memory().unwrap();
    let (accepted, total) = store.get_signal_accept_rate(30).unwrap();
    assert_eq!(accepted, 0);
    assert_eq!(total, 0);
}

#[test]
fn test_get_signal_accept_rate_mixed() {
    let store = Store::open_in_memory().unwrap();
    // 保存 3 条信号（使用语义差异足够大的标题，避免去重）
    let id1 = store
        .save_task_signal("new_task", None, "整理项目文档并发送邮件", "证据A", None)
        .unwrap();
    let id2 = store
        .save_task_signal("new_task", None, "去超市购买晚餐食材", "证据B", None)
        .unwrap();
    let id3 = store
        .save_task_signal("new_task", None, "修复登录页面的样式问题", "证据C", None)
        .unwrap();
    // 2 accepted, 1 dismissed
    store.update_signal_status(id1, "accepted").unwrap();
    store.update_signal_status(id2, "accepted").unwrap();
    store.update_signal_status(id3, "dismissed").unwrap();

    let (accepted, total) = store.get_signal_accept_rate(30).unwrap();
    assert_eq!(accepted, 2);
    assert_eq!(total, 3);
}

#[test]
fn test_importance_threshold_default() {
    let store = Store::open_in_memory().unwrap();
    let threshold = store.get_importance_threshold().unwrap();
    assert!((threshold - 0.65).abs() < 1e-4, "默认阈值应为 0.65，实际 {threshold}");
}

#[test]
fn test_importance_threshold_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    store.set_importance_threshold(0.72).unwrap();
    let threshold = store.get_importance_threshold().unwrap();
    assert!((threshold - 0.72).abs() < 1e-4, "阈值应为 0.72，实际 {threshold}");
}

#[test]
fn test_kv_store_upsert() {
    let store = Store::open_in_memory().unwrap();
    store.set_importance_threshold(0.6).unwrap();
    store.set_importance_threshold(0.9).unwrap();
    let threshold = store.get_importance_threshold().unwrap();
    assert!((threshold - 0.9).abs() < 1e-4, "第二次写入应覆盖第一次，实际 {threshold}");
}

// ─── Phase 1b: Embedding 向量搜索测试 ────────────────────────────

#[test]
fn test_embed_roundtrip() {
    use crate::memories::{bytes_to_embed, embed_to_bytes};
    let original = vec![1.0f32, 0.5, -0.25, 3.14, 0.0];
    let bytes = embed_to_bytes(&original);
    let recovered = bytes_to_embed(&bytes);
    assert_eq!(original.len(), recovered.len());
    for (a, b) in original.iter().zip(&recovered) {
        assert!((a - b).abs() < 1e-7, "往返精度失败: {a} vs {b}");
    }
}

#[test]
fn test_cosine_similarity_identical() {
    use crate::memories::cosine_similarity;
    let v = vec![0.6f32, 0.8, 0.0];
    let sim = cosine_similarity(&v, &v);
    assert!((sim - 1.0).abs() < 1e-6, "相同向量相似度应为 1.0，实际 {sim}");
}

#[test]
fn test_cosine_similarity_orthogonal() {
    use crate::memories::cosine_similarity;
    let a = vec![1.0f32, 0.0, 0.0];
    let b = vec![0.0f32, 1.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!(sim.abs() < 1e-6, "正交向量相似度应为 0.0，实际 {sim}");
}

#[test]
fn test_save_and_search_embedding() {
    let store = Store::open_in_memory().unwrap();

    // 插入两条记忆
    let id1 = store.save_memory("identity", "用户喜欢 Rust 编程", "test", 0.9).unwrap();
    let id2 = store.save_memory("values", "注重代码可维护性", "test", 0.8).unwrap();

    // 保存 embedding：id1 与 query 方向接近，id2 正交
    let emb1 = vec![1.0f32, 0.0, 0.0]; // 与 query 完全相同
    let emb2 = vec![0.0f32, 1.0, 0.0]; // 与 query 正交
    store.save_embedding(id1, &emb1).unwrap();
    store.save_embedding(id2, &emb2).unwrap();

    let query = vec![1.0f32, 0.0, 0.0];
    let results = store.search_memories_by_vector(&query, 5).unwrap();

    assert_eq!(results.len(), 2, "应返回 2 条有 embedding 的记忆");
    // id1 相似度 1.0 > id2 相似度 0.0，id1 应排在前面
    assert_eq!(results[0].0.id, id1, "相似度最高的记忆应排第一");
    assert!((results[0].1 - 1.0).abs() < 1e-6, "相似度应接近 1.0");
    assert!(results[1].1.abs() < 1e-6, "正交记忆相似度应接近 0.0");
}

#[test]
fn test_search_without_embedding_fallback() {
    let store = Store::open_in_memory().unwrap();

    // 插入记忆但不保存 embedding
    store.save_memory("identity", "没有 embedding 的记忆", "test", 0.9).unwrap();

    let query = vec![1.0f32, 0.0, 0.0];
    let results = store.search_memories_by_vector(&query, 5).unwrap();

    assert_eq!(results.len(), 0, "没有 embedding 的记忆不应出现在向量搜索结果中");
}

// ─── Message Sources + Emails tests ──────────────────────────

fn make_test_source() -> MessageSource {
    MessageSource {
        id: 0,
        label: "Test Gmail".into(),
        source_type: "imap".into(),
        config: r#"{"imap_host":"imap.gmail.com","imap_port":993,"smtp_host":"smtp.gmail.com","smtp_port":587,"username":"test@gmail.com","password_enc":"b64:","use_tls":true,"email":"test@gmail.com"}"#.into(),
        enabled: true,
        created_at: String::new(),
    }
}

#[test]
fn test_message_source_crud() {
    let store = Store::open_in_memory().unwrap();
    let source = make_test_source();
    let id = store.save_message_source(&source).unwrap();
    assert!(id > 0);

    let sources = store.get_message_sources().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].label, "Test Gmail");
    assert_eq!(sources[0].source_type, "imap");
}

#[test]
fn test_message_source_update() {
    let store = Store::open_in_memory().unwrap();
    let source = make_test_source();
    let id = store.save_message_source(&source).unwrap();

    let mut updated = store.get_message_source(id).unwrap().unwrap();
    updated.label = "Work Gmail".into();
    store.save_message_source(&updated).unwrap();

    let fetched = store.get_message_source(id).unwrap().unwrap();
    assert_eq!(fetched.label, "Work Gmail");
}

#[test]
fn test_message_source_delete_cascades_emails() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_message_source(&make_test_source()).unwrap();

    let email = EmailMessage {
        id: 0, source_id: id, uid: "1".into(), folder: "INBOX".into(),
        from_addr: "a@b.com".into(), to_addr: "c@d.com".into(),
        subject: "Test".into(), body_text: "Hello".into(), body_html: None,
        is_read: false, date: "2026-03-22".into(), fetched_at: String::new(),
    };
    store.save_email(&email).unwrap();
    assert_eq!(store.get_emails(id, "INBOX", 10).unwrap().len(), 1);

    store.delete_message_source(id).unwrap();
    assert_eq!(store.get_emails(id, "INBOX", 10).unwrap().len(), 0);
    assert!(store.get_message_sources().unwrap().is_empty());
}

#[test]
fn test_message_sources_by_type() {
    let store = Store::open_in_memory().unwrap();
    store.save_message_source(&make_test_source()).unwrap();

    let mut slack = make_test_source();
    slack.source_type = "slack".into();
    slack.label = "Work Slack".into();
    store.save_message_source(&slack).unwrap();

    let imap_sources = store.get_message_sources_by_type("imap").unwrap();
    assert_eq!(imap_sources.len(), 1);
    assert_eq!(imap_sources[0].label, "Test Gmail");

    let all = store.get_message_sources().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn test_email_save_and_fetch() {
    let store = Store::open_in_memory().unwrap();
    let sid = store.save_message_source(&make_test_source()).unwrap();

    for i in 1..=5 {
        let email = EmailMessage {
            id: 0, source_id: sid, uid: format!("{i}"), folder: "INBOX".into(),
            from_addr: format!("sender{i}@test.com"), to_addr: "me@test.com".into(),
            subject: format!("Subject {i}"), body_text: format!("Body {i}"),
            body_html: None, is_read: i % 2 == 0,
            date: format!("2026-03-{:02}", 20 + i), fetched_at: String::new(),
        };
        store.save_email(&email).unwrap();
    }

    let emails = store.get_emails(sid, "INBOX", 10).unwrap();
    assert_eq!(emails.len(), 5);
    // Ordered by date DESC
    assert_eq!(emails[0].subject, "Subject 5");
}

#[test]
fn test_email_uid_dedup() {
    let store = Store::open_in_memory().unwrap();
    let sid = store.save_message_source(&make_test_source()).unwrap();

    let email = EmailMessage {
        id: 0, source_id: sid, uid: "42".into(), folder: "INBOX".into(),
        from_addr: "a@b.com".into(), to_addr: "c@d.com".into(),
        subject: "First".into(), body_text: "First body".into(),
        body_html: None, is_read: false, date: "2026-03-22".into(),
        fetched_at: String::new(),
    };
    store.save_email(&email).unwrap();
    store.save_email(&email).unwrap(); // duplicate UID

    let emails = store.get_emails(sid, "INBOX", 10).unwrap();
    assert_eq!(emails.len(), 1);
}

#[test]
fn test_email_mark_read() {
    let store = Store::open_in_memory().unwrap();
    let sid = store.save_message_source(&make_test_source()).unwrap();

    let email = EmailMessage {
        id: 0, source_id: sid, uid: "1".into(), folder: "INBOX".into(),
        from_addr: "a@b.com".into(), to_addr: "c@d.com".into(),
        subject: "Unread".into(), body_text: "Body".into(),
        body_html: None, is_read: false, date: "2026-03-22".into(),
        fetched_at: String::new(),
    };
    store.save_email(&email).unwrap();

    let emails = store.get_emails(sid, "INBOX", 10).unwrap();
    let eid = emails[0].id;
    assert!(!emails[0].is_read);

    store.mark_email_read(eid).unwrap();
    let updated = store.get_email(eid).unwrap().unwrap();
    assert!(updated.is_read);
}

#[test]
fn test_email_search() {
    let store = Store::open_in_memory().unwrap();
    let sid = store.save_message_source(&make_test_source()).unwrap();

    let e1 = EmailMessage {
        id: 0, source_id: sid, uid: "1".into(), folder: "INBOX".into(),
        from_addr: "alice@company.com".into(), to_addr: "me@test.com".into(),
        subject: "Q1 Budget Review".into(), body_text: "Please review the budget".into(),
        body_html: None, is_read: false, date: "2026-03-22".into(), fetched_at: String::new(),
    };
    let e2 = EmailMessage {
        id: 0, source_id: sid, uid: "2".into(), folder: "INBOX".into(),
        from_addr: "bob@other.com".into(), to_addr: "me@test.com".into(),
        subject: "Lunch tomorrow?".into(), body_text: "Free for lunch?".into(),
        body_html: None, is_read: false, date: "2026-03-22".into(), fetched_at: String::new(),
    };
    store.save_email(&e1).unwrap();
    store.save_email(&e2).unwrap();

    let results = store.search_emails("budget", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].subject, "Q1 Budget Review");

    let results = store.search_emails("alice", 10).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_count_unread_emails() {
    let store = Store::open_in_memory().unwrap();
    let sid = store.save_message_source(&make_test_source()).unwrap();

    for i in 1..=4 {
        let email = EmailMessage {
            id: 0, source_id: sid, uid: format!("{i}"), folder: "INBOX".into(),
            from_addr: "a@b.com".into(), to_addr: "c@d.com".into(),
            subject: format!("Mail {i}"), body_text: "body".into(),
            body_html: None, is_read: i <= 2, // first 2 read, last 2 unread
            date: "2026-03-22".into(), fetched_at: String::new(),
        };
        store.save_email(&email).unwrap();
    }

    assert_eq!(store.count_unread_emails(sid).unwrap(), 2);
}

// ─── Message Action State 测试 ─────────────────────

#[test]
fn test_message_action_state_default() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_message("Alice", "#general", Some("Hello"), "teams", "text", "2026-03-23T10:00:00")
        .unwrap();

    let msgs = store.get_messages_by_channel("#general", 10).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].action_state, "pending");
    assert!(msgs[0].resolved_at.is_none());
}

#[test]
fn test_update_message_action_state() {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .save_message("Bob", "#dev", Some("Need review"), "teams", "text", "2026-03-23T10:00:00")
        .unwrap();

    store.update_message_action_state(id, "resolved").unwrap();

    let msgs = store.get_messages_by_channel("#dev", 10).unwrap();
    assert_eq!(msgs[0].action_state, "resolved");
    assert!(msgs[0].resolved_at.is_some());
}

#[test]
fn test_get_pending_messages_older_than() {
    let store = Store::open_in_memory().unwrap();
    // 使用固定时间戳确保测试不受时区影响
    let old_time = "2020-01-01 10:00:00";
    let future_time = "2099-12-31 23:59:59";
    {
        let conn = store.conn.lock().unwrap();
        // 旧消息 — 应出现在结果中
        conn.execute(
            "INSERT INTO messages (sender, channel, content, source, message_type, timestamp, created_at, direction, action_state)
             VALUES ('Alice', '#ch', 'old msg', 'teams', 'text', '2020-01-01T10:00:00', ?1, 'received', 'pending')",
            rusqlite::params![old_time],
        ).unwrap();
        // 未来时间的新消息 — 不应出现在结果中
        conn.execute(
            "INSERT INTO messages (sender, channel, content, source, message_type, timestamp, created_at, direction, action_state)
             VALUES ('Bob', '#ch', 'new msg', 'teams', 'text', '2099-12-31T23:59:59', ?1, 'received', 'pending')",
            rusqlite::params![future_time],
        ).unwrap();
        // 已解决的旧消息 — 不应出现在结果中
        conn.execute(
            "INSERT INTO messages (sender, channel, content, source, message_type, timestamp, created_at, direction, action_state)
             VALUES ('Carol', '#ch', 'resolved', 'teams', 'text', '2020-01-01T10:00:00', ?1, 'received', 'resolved')",
            rusqlite::params![old_time],
        ).unwrap();
    }

    let pending = store.get_pending_messages_older_than(1).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].sender, "Alice");
}

#[test]
fn test_resolve_messages_batch() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store
        .save_message("A", "#ch", Some("msg1"), "teams", "text", "2026-03-23T10:00:00")
        .unwrap();
    let id2 = store
        .save_message("B", "#ch", Some("msg2"), "teams", "text", "2026-03-23T10:01:00")
        .unwrap();
    let id3 = store
        .save_message("C", "#ch", Some("msg3"), "teams", "text", "2026-03-23T10:02:00")
        .unwrap();

    let resolved = store.resolve_messages(&[id1, id3]).unwrap();
    assert_eq!(resolved, 2);

    // id2 应仍为 pending
    let msgs = store.get_messages_by_channel("#ch", 10).unwrap();
    let pending: Vec<_> = msgs.iter().filter(|m| m.action_state == "pending").collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id2);
}

#[test]
fn test_count_pending_messages() {
    let store = Store::open_in_memory().unwrap();
    store
        .save_message("A", "#ch", Some("m1"), "teams", "text", "2026-03-23T10:00:00")
        .unwrap();
    let id2 = store
        .save_message("B", "#ch", Some("m2"), "teams", "text", "2026-03-23T10:01:00")
        .unwrap();
    store
        .save_message_with_direction("Me", "#ch", Some("sent"), "teams", "text", "2026-03-23T10:02:00", "sent")
        .unwrap();

    assert_eq!(store.count_pending_messages().unwrap(), 2);

    store.update_message_action_state(id2, "resolved").unwrap();
    assert_eq!(store.count_pending_messages().unwrap(), 1);
}

#[test]
fn test_archive_memory() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_memory("behavior", "test content", "chat", 0.7).unwrap();
    store.archive_memory(id, "merged with #99").unwrap();
    let active = store.load_active_memories().unwrap();
    assert!(active.iter().all(|m| m.id != id));
}

#[test]
fn test_save_memory_with_provenance() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "loves coffee in the morning", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "drinks espresso every single day", "chat", 0.6).unwrap();
    let new_id = store.save_memory_with_provenance(
        "behavior", "caffeine dependent person confirmed", "evolution", 0.8,
        &[id1, id2], "合并咖啡相关行为",
    ).unwrap();
    assert!(new_id > 0);
    let mem = store.get_memory_by_id(new_id).unwrap().unwrap();
    assert!(mem.derived_from.is_some());
    assert_eq!(mem.evolution_note.as_deref(), Some("合并咖啡相关行为"));
}

#[test]
fn test_archived_memory_preserves_content() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_memory("behavior", "original content here", "chat", 0.7).unwrap();
    store.archive_memory(id, "dedup: 与其他记忆重复").unwrap();
    // 归档记忆仍可通过 ID 直接查询
    let mem = store.get_memory_by_id(id).unwrap().unwrap();
    assert_eq!(mem.content, "original content here");
    assert_eq!(mem.evolution_note.as_deref(), Some("dedup: 与其他记忆重复"));
}

#[test]
fn test_info_only_messages() {
    let store = Store::open_in_memory().unwrap();
    // info_only 消息不应计入 pending 统计
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages (sender, channel, content, source, message_type, timestamp, direction, action_state)
             VALUES ('System', '#announcements', 'FYI: server maintenance', 'teams', 'text', '2026-03-23T09:00:00', 'received', 'info_only')",
            [],
        ).unwrap();
    }
    store
        .save_message("Alice", "#ch", Some("pending msg"), "teams", "text", "2026-03-23T10:00:00")
        .unwrap();

    assert_eq!(store.count_pending_messages().unwrap(), 1);
}

// ─── 分层衰减测试 ──────────────────────────────

#[test]
fn test_decay_axiom_never_decays() {
    // axiom 层记忆永不自动衰减（即使 last_accessed_at = NULL）
    let store = Store::open_in_memory().unwrap();
    let id = store.save_memory("values", "核心信念：用户第一", "evolution", 0.95).unwrap();
    store.update_memory_depth(id, "axiom").unwrap();

    // 使用 0 天阈值强制触发所有层的衰减，axiom 仍不受影响
    let decayed = store.decay_memories_by_depth_with_thresholds(0, 0, 0).unwrap();
    assert_eq!(decayed, 0, "axiom 层不应被衰减");

    let axioms = store.load_memories_by_depth("axiom").unwrap();
    assert_eq!(axioms.len(), 1, "axiom 记忆应仍存在");
    assert!((axioms[0].confidence - 0.95).abs() < 0.01, "confidence 不应变化");
}

#[test]
fn test_decay_semantic_uses_90_day_threshold() {
    // semantic 层在 0 天阈值时应衰减（last_accessed_at = NULL 视为从未访问）
    // 验证 semantic 用独立阈值且 axiom 同批不受影响
    let store = Store::open_in_memory().unwrap();
    let sid = store.save_memory("behavior", "喜欢清晨工作", "coach", 0.7).unwrap();
    store.update_memory_depth(sid, "semantic").unwrap();
    let aid = store.save_memory("values", "核心信念：诚实", "evolution", 0.95).unwrap();
    store.update_memory_depth(aid, "axiom").unwrap();

    // 0 天阈值：semantic 应衰减，axiom 不受影响
    let decayed = store.decay_memories_by_depth_with_thresholds(0, 0, 0).unwrap();
    assert!(decayed >= 1, "semantic 记忆应被衰减（0 天阈值）");

    let semantic = store.load_memories_by_depth("semantic").unwrap();
    let m = semantic.iter().find(|m| m.id == sid).unwrap();
    assert!((m.confidence - 0.63).abs() < 0.02, "semantic confidence 应约为 0.7 × 0.9 = 0.63");

    let axioms = store.load_memories_by_depth("axiom").unwrap();
    let a = axioms.iter().find(|m| m.id == aid).unwrap();
    assert!((a.confidence - 0.95).abs() < 0.01, "axiom confidence 不应变化");
}

// ─── evict_low_quality_memories 测试 ──────────────────────────────

/// 插入 n 条 working/episodic 记忆，confidence 从 low_conf 到 1.0 线性分布
/// 使用差异化内容避免相似度去重合并
fn insert_working_episodic(store: &Store, n: usize, low_conf: f64) -> Vec<i64> {
    // 预定义足够多的不同内容，避免 LCS > 60% 触发去重
    let topics = [
        "周一早会记录：讨论了产品路线图和 Q2 目标",
        "收到客户张总的邮件：要求下周交付演示版本",
        "代码审查：合并了认证模块的 PR，有 3 个 comment",
        "下午 3 点参加了技术架构讨论，决定采用微服务方案",
        "本周读了《深度工作》前两章，感触颇深",
        "和设计师对齐了新版首页的视觉风格",
        "修复了生产环境的内存泄漏问题，耗时两小时",
        "完成了季度 OKR 自评，提交给 HR",
        "下午参与了产品演示，客户反馈积极",
        "整理了技术债清单，共 12 项待处理",
        "和导师进行了月度 1on1，讨论了职业发展方向",
        "完成了 API 文档的更新，覆盖所有新接口",
        "参加了行业线上分享会，主题是 LLM 应用实践",
        "处理了 5 封重要邮件，回复了合作伙伴的询价",
        "完成了前端性能优化，首屏加载缩短 40%",
        "下周的项目排期已确认，优先级已和 PM 对齐",
        "阅读了竞品分析报告，整理了 3 点差异化机会",
        "修改了用户研究访谈提纲，增加了情境问题",
        "完成了本月安全漏洞扫描，无高危问题",
        "参与了招聘面试，候选人技术能力符合预期",
        "整理了上线后的异常日志，发现 2 个潜在 bug",
        "和运营团队对齐了活动方案，确定了推广渠道",
        "学习了新的 Rust async 模式，记录了最佳实践",
        "完成了数据库索引优化，查询速度提升 3 倍",
        "向老板汇报了项目进展，获得了额外预算支持",
        "下午调试了 WebSocket 连接断开问题",
        "完成了压力测试，系统在 1000 并发下稳定运行",
        "梳理了用户反馈，归纳为 5 个核心诉求",
        "和法务确认了新合同条款，无风险点",
        "参加了公司全员会议，了解了 H2 战略方向",
        "完成了登录流程的可访问性改造，符合 WCAG AA",
        "和同事结对编程解决了一个复杂的并发 bug",
        "整理了本季度的技术分享主题，提交给团队",
        "协助新同事完成了开发环境配置",
        "阅读了 3 篇关于向量数据库的论文并做了摘要",
        "参与了 sprint 回顾，提出了 2 项流程改进建议",
        "更新了 CI/CD 流水线，减少了部署时间",
        "完成了多语言支持的 i18n 文案整理",
        "处理了一个线上紧急事故，恢复时间 15 分钟",
        "和产品讨论了新功能的 MVP 范围",
        "完成了个人年度总结文档",
        "参加了外部技术沙龙，结识了两位业内人士",
        "代码重构：将 utils.rs 拆分为 3 个专用模块",
        "和市场团队对齐了品牌视觉规范的使用方式",
        "完成了本月的技术债还款计划，关闭 4 个 issue",
        "参与了跨部门需求评审会，明确了接口协议",
        "进行了月度数据备份检查，确认备份正常",
        "完成了新手引导流程的 A/B 测试分析",
        "处理了用户投诉：账号被误封问题，已恢复",
        "整理了本周的学习笔记并发布到内部 wiki",
    ];
    (0..n)
        .map(|i| {
            let conf = low_conf + (1.0 - low_conf) * (i as f64 / (n as f64).max(1.0));
            let content = topics[i % topics.len()];
            // 加 index 后缀确保与同 category 其它条目无重复
            let content = format!("{content} [{i}]");
            let id = store
                .save_memory("session", &content, "test", conf)
                .unwrap();
            // session → working tier (already), force episodic depth
            store.update_memory_depth(id, "episodic").unwrap();
            id
        })
        .collect()
}

#[test]
fn test_evict_over_cap_archives_lowest_scored() {
    let store = Store::open_in_memory().unwrap();
    // 插入 50 条 working/episodic，confidence 0.1~1.0
    insert_working_episodic(&store, 50, 0.1);
    assert_eq!(store.count_memories().unwrap(), 50);

    // cap=30 → 应驱逐 min(evict_count=20, available) 条
    let evicted = store.evict_low_quality_memories(30, 20).unwrap();
    assert_eq!(evicted, 20, "应归档 20 条低分记忆");

    let remaining_active = store.count_memories().unwrap();
    assert_eq!(remaining_active, 30, "活跃记忆应剩 30 条");
}

#[test]
fn test_evict_under_cap_evicts_nothing() {
    let store = Store::open_in_memory().unwrap();
    // 插入 20 条，cap=30 → 无需驱逐
    insert_working_episodic(&store, 20, 0.3);
    let evicted = store.evict_low_quality_memories(30, 10).unwrap();
    assert_eq!(evicted, 0, "低于 cap 时不应驱逐任何记忆");
    assert_eq!(store.count_memories().unwrap(), 20);
}

#[test]
fn test_evict_skips_core_and_deep_memories() {
    let store = Store::open_in_memory().unwrap();

    // 插入 5 条 core tier（identity 类，各内容独特防止去重）
    let core_topics = [
        "我是一名后端工程师，专注分布式系统设计",
        "我的核心价值观：诚实、负责、持续学习",
        "我在职业中最重视的是技术影响力与团队成长",
        "我的个人原则：先理解再执行，先问题再方案",
        "我对高质量代码有执念：可读性优先于性能",
    ];
    for content in &core_topics {
        store.save_memory("identity", content, "user", 0.9).unwrap();
    }
    // 插入 5 条 working/procedural（使用 session 类型，再覆写 depth）
    let proc_topics = [
        "每次 code review 前先跑一遍本地测试",
        "重要邮件回复前整理三点要点再写",
        "每周五下午做本周工作复盘和下周计划",
        "遇到卡点超过 30 分钟就向同事或文档求助",
        "新项目第一步：画出数据流图",
    ];
    for content in &proc_topics {
        let id = store.save_memory("session", content, "test", 0.5).unwrap();
        store.update_memory_depth(id, "procedural").unwrap();
    }
    // 插入 5 条 working/axiom（使用 session 类型，再覆写 depth）
    let axiom_topics = [
        "简单优于复杂，永远选择最小够用方案",
        "团队效能大于个人英雄主义",
        "产品思维：用户的痛点才是北极星",
        "技术决策要有可逆性，避免过早锁定",
        "沟通成本是系统的隐性瓶颈",
    ];
    for content in &axiom_topics {
        let id = store.save_memory("session", content, "test", 0.5).unwrap();
        store.update_memory_depth(id, "axiom").unwrap();
    }

    // 插入 10 条 working/episodic（可被驱逐），使总数超过 cap=10
    insert_working_episodic(&store, 10, 0.1);

    let total = store.count_memories().unwrap();
    // 至少有 25 条（可能有合并，但各组内容独特应全部插入）
    assert_eq!(total, 25, "初始应有 25 条活跃记忆");

    // cap=10，evict_count=20：只有 working/episodic 10 条可被驱逐
    let evicted = store.evict_low_quality_memories(10, 20).unwrap();
    assert_eq!(evicted, 10, "只应驱逐 working/episodic 条目");

    // core + procedural + axiom 全部保留（15 条）
    let remaining = store.count_memories().unwrap();
    assert_eq!(remaining, 15, "core/procedural/axiom 不应被驱逐");
}

// ─── append_negative_rule 测试 ────────────────────────────────────────────────

#[test]
fn test_append_negative_rule_adds_to_empty_profile() {
    let store = Store::open_in_memory().unwrap();
    store.append_negative_rule("不要发重复提醒").unwrap();
    let profile = store.load_profile().unwrap().unwrap();
    assert_eq!(profile.negative_rules, vec!["不要发重复提醒"]);
}

#[test]
fn test_append_negative_rule_no_duplicate() {
    let store = Store::open_in_memory().unwrap();
    store.append_negative_rule("不要发重复提醒").unwrap();
    store.append_negative_rule("不要发重复提醒").unwrap(); // 重复插入
    let profile = store.load_profile().unwrap().unwrap();
    assert_eq!(profile.negative_rules.len(), 1, "重复规则不应被写入两次");
}

#[test]
fn test_append_negative_rule_multiple_distinct() {
    let store = Store::open_in_memory().unwrap();
    store.append_negative_rule("规则A").unwrap();
    store.append_negative_rule("规则B").unwrap();
    store.append_negative_rule("规则A").unwrap(); // 重复
    let profile = store.load_profile().unwrap().unwrap();
    assert_eq!(profile.negative_rules, vec!["规则A", "规则B"]);
}

#[test]
fn test_append_negative_rule_trims_whitespace() {
    let store = Store::open_in_memory().unwrap();
    store.append_negative_rule("  规则带空格  ").unwrap();
    store.append_negative_rule("规则带空格").unwrap(); // trim 后与上面相同
    let profile = store.load_profile().unwrap().unwrap();
    assert_eq!(profile.negative_rules.len(), 1);
    assert_eq!(profile.negative_rules[0], "规则带空格");
}

#[test]
fn test_append_negative_rule_empty_is_noop() {
    let store = Store::open_in_memory().unwrap();
    store.append_negative_rule("").unwrap();
    store.append_negative_rule("   ").unwrap();
    assert!(store.load_profile().unwrap().is_none(), "空规则不应创建 profile");
}
