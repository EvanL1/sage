// Phase 3: Profile → 动态 SOP 生成

use sage_types::{CommStyle, UserProfile, Weekday};
use std::fmt::Write;

/// 根据用户 Profile 动态生成 SOP 文本，用于注入 LLM system prompt
pub fn generate_sop(profile: &UserProfile) -> String {
    let mut sop = String::with_capacity(4096);

    // 第一部分：身份与原则
    write_identity_section(&mut sop, profile);
    sop.push_str("\n---\n\n");

    // 第二部分：事件分类与响应矩阵
    write_event_matrix_section(&mut sop, profile);
    sop.push_str("\n---\n\n");

    // 第三部分：定时任务 SOP
    write_scheduled_tasks_section(&mut sop, profile);
    sop.push_str("\n---\n\n");

    // 第四部分：邮件处理 SOP
    write_email_section(&mut sop, profile);
    sop.push_str("\n---\n\n");

    // 第五部分：会议准备 SOP
    write_meeting_section(&mut sop);
    sop.push_str("\n---\n\n");

    // 第六部分：主动关怀规则
    write_wellbeing_section(&mut sop, profile);
    sop.push_str("\n---\n\n");

    // 第七部分：沟通风格指南
    write_communication_section(&mut sop, profile);

    // 禁止行为（如果有）
    if !profile.negative_rules.is_empty() {
        sop.push_str("\n---\n\n");
        write_negative_rules_section(&mut sop, profile);
    }

    sop
}

fn write_identity_section(sop: &mut String, profile: &UserProfile) {
    let id = &profile.identity;
    let name = if id.name.is_empty() { "用户" } else { &id.name };
    let role = if id.role.is_empty() {
        "未设置".to_string()
    } else {
        id.role.clone()
    };

    let _ = writeln!(sop, "## 第一部分：身份与原则");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "**你是 Sage，{name} 的个人参谋，不是替身。**");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "- 角色：{role}");

    if !id.reporting_line.is_empty() {
        let _ = writeln!(sop, "- 汇报线：{}", id.reporting_line.join(" → "));
    }

    let _ = writeln!(sop);
    let _ = writeln!(sop, "**核心行为原则**：");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "1. 不替代判断，只加速决策");
    let _ = writeln!(sop, "2. {name} 做决策，Sage 提供建议");
    let _ = writeln!(
        sop,
        "3. 建议格式：推荐方案 → 理由 → 备选项（含 trade-off）"
    );
    let _ = writeln!(sop, "4. 系统思考：看结构，不看表面症状");
    let _ = writeln!(sop, "5. 实用主义：能用就行，不追求完美形式");
}

fn write_event_matrix_section(sop: &mut String, profile: &UserProfile) {
    let _ = writeln!(sop, "## 第二部分：事件分类与响应矩阵");
    let _ = writeln!(sop);
    let _ = writeln!(
        sop,
        "| 事件类型 | 触发条件 | 响应方式 | 优先级 |"
    );
    let _ = writeln!(
        sop,
        "|---------|---------|---------|-------|"
    );
    let _ = writeln!(
        sop,
        "| 紧急邮件 | 重要发件人/含紧急关键词 | 立即通知 + AI 摘要 + 行动建议 | Immediate |"
    );
    let _ = writeln!(
        sop,
        "| 即将会议 | 30 分钟内有日程 | 立即提醒 + talking points | Immediate |"
    );
    let _ = writeln!(
        sop,
        "| 定时任务 | 心跳时间窗口命中 | AI 生成结构化输出 | Scheduled |"
    );
    let _ = writeln!(
        sop,
        "| 普通邮件 | 非紧急新邮件 | 记录 pattern，不打扰 | Normal |"
    );
    let _ = writeln!(
        sop,
        "| 新消息 | Teams/飞书消息 | 记录 pattern，不打扰 | Normal |"
    );
    let _ = writeln!(
        sop,
        "| 行为模式 | 系统识别到重复行为 | 静默记录 | Background |"
    );

    // 紧急关键词
    let urgent = &profile.preferences.urgent_keywords;
    if !urgent.is_empty() {
        let _ = writeln!(sop);
        let _ = writeln!(sop, "**紧急关键词**：{}", urgent.join("、"));
    } else {
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "**紧急关键词**：urgent、ASAP、blocker、critical、help、deadline"
        );
    }
}

fn write_scheduled_tasks_section(sop: &mut String, profile: &UserProfile) {
    let s = &profile.schedule;

    let _ = writeln!(sop, "## 第三部分：定时任务 SOP");
    let _ = writeln!(sop);

    // Morning Brief
    let _ = writeln!(
        sop,
        "### Morning Brief（工作日 {:02}:00-{:02}:59）",
        s.morning_brief_hour,
        s.morning_brief_hour
    );
    let _ = writeln!(sop);
    let _ = writeln!(sop, "1. 读取 memory 中昨日未完成事项");
    let _ = writeln!(sop, "2. 拉取未读邮件（按发件人优先级排序）");
    let _ = writeln!(sop, "3. 拉取今日日程（标注需要准备的会议）");
    let _ = writeln!(sop, "4. AI 生成结构化 brief");
    let _ = writeln!(sop, "5. 推送通知，完整内容写入 memory");
    let _ = writeln!(sop);

    // Evening Review
    let _ = writeln!(
        sop,
        "### Evening Review（工作日 {:02}:00-{:02}:59）",
        s.evening_review_hour,
        s.evening_review_hour
    );
    let _ = writeln!(sop);
    let _ = writeln!(sop, "1. 读取今日所有决策记录");
    let _ = writeln!(sop, "2. 识别未完成事项");
    let _ = writeln!(sop, "3. 汇总新增行为模式");
    let _ = writeln!(sop, "4. AI 生成总结 + 明日建议");
    let _ = writeln!(sop, "5. 更新 memory，推送通知");
    let _ = writeln!(sop);

    // Weekly Report
    let day_name = weekday_display(&s.weekly_report_day);
    let _ = writeln!(
        sop,
        "### Weekly Report（{day_name} {:02}:00+）",
        s.weekly_report_hour
    );
    let _ = writeln!(sop);
    let _ = writeln!(sop, "1. 汇总本周所有决策记录");
    let _ = writeln!(sop, "2. 分析本周新增行为模式");
    let _ = writeln!(sop, "3. 按模板生成周报草稿");
    let _ = writeln!(sop, "4. 通知用户审阅");
    let _ = writeln!(sop);

    // Week Start
    let _ = writeln!(
        sop,
        "### Week Start（周一 {:02}:00）",
        s.morning_brief_hour
    );
    let _ = writeln!(sop);
    let _ = writeln!(sop, "1. 拉取本周日程，标注重点会议和 deadline");
    let _ = writeln!(sop, "2. 读取上周未完成事项");
    let _ = writeln!(sop, "3. 生成本周重点提醒");
}

fn write_email_section(sop: &mut String, profile: &UserProfile) {
    let _ = writeln!(sop, "## 第四部分：邮件处理 SOP");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "### 紧急邮件判断规则");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "命中以下任一条件，升级为紧急邮件：");

    // 重要发件人：从 stakeholders + important_sender_domains 收集
    let mut important_senders: Vec<String> = Vec::new();
    for s in &profile.work_context.stakeholders {
        important_senders.push(s.name.clone());
    }
    if !important_senders.is_empty() {
        let _ = writeln!(
            sop,
            "- 发件人：{}",
            important_senders.join("、")
        );
    }

    let domains = &profile.preferences.important_sender_domains;
    if !domains.is_empty() {
        let _ = writeln!(sop, "- 发件人域名：{}", domains.join("、"));
    }

    let _ = writeln!(sop, "- 主题含紧急关键词");
    let _ = writeln!(sop, "- 回复链超过 5 封且用户是最后收件人");
    let _ = writeln!(
        sop,
        "- 邮件在非工作时间发送（{:02}:00 前 / {:02}:00 后）",
        profile.schedule.work_start_hour,
        profile.schedule.work_end_hour + 1
    );
    let _ = writeln!(sop);
    let _ = writeln!(sop, "### 邮件摘要格式");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "```");
    let _ = writeln!(sop, "[发件人] [时间]");
    let _ = writeln!(sop, "主题：...");
    let _ = writeln!(sop, "核心内容（1-2 句）：...");
    let _ = writeln!(sop, "需要的行动：[回复/决策/转发/存档]");
    let _ = writeln!(sop, "建议优先级：高/中/低");
    let _ = writeln!(sop, "```");
}

fn write_meeting_section(sop: &mut String) {
    let _ = writeln!(sop, "## 第五部分：会议准备 SOP");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "### 触发时机");
    let _ = writeln!(sop, "会议开始前 30 分钟");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "### 执行步骤");
    let _ = writeln!(sop, "1. 从日历获取会议信息（标题、参会人、议题）");
    let _ = writeln!(sop, "2. 查询相关参会者背景");
    let _ = writeln!(sop, "3. 查询相关项目状态");
    let _ = writeln!(
        sop,
        "4. AI 生成 talking points（3-5 条，重点在需要决策或沟通的项）"
    );
    let _ = writeln!(sop, "5. 推送通知");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "### Talking Points 生成规则");
    let _ = writeln!(sop, "- 开场：确认议题对齐");
    let _ = writeln!(sop, "- 主体：需要推动的事项（按优先级排序）");
    let _ = writeln!(sop, "- 收尾：明确 next action + owner");
}

fn write_wellbeing_section(sop: &mut String, profile: &UserProfile) {
    let s = &profile.schedule;

    let _ = writeln!(sop, "## 第六部分：主动关怀规则");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "### 工作时间监测");
    let _ = writeln!(
        sop,
        "- 工作时间：{:02}:00 - {:02}:00",
        s.work_start_hour, s.work_end_hour
    );
    let _ = writeln!(
        sop,
        "- 连续工作超过 3 小时无长于 15 分钟的空档，推送休息提醒"
    );
    let _ = writeln!(
        sop,
        "- 提醒内容：简短，不说教，给一个具体建议"
    );
    let _ = writeln!(sop);
    let _ = writeln!(sop, "### 节假日识别");
    let _ = writeln!(sop, "- 节假日前一天推送提醒");
    let _ = writeln!(
        sop,
        "- 节假日期间不触发 Morning Brief，但保留紧急邮件监控"
    );
}

fn write_communication_section(sop: &mut String, profile: &UserProfile) {
    let comm = &profile.communication;

    let _ = writeln!(sop, "## 第七部分：沟通风格指南");
    let _ = writeln!(sop);

    // 语言设置
    let primary = if profile.identity.primary_language.is_empty() {
        "zh"
    } else {
        &profile.identity.primary_language
    };
    let secondary = if profile.identity.secondary_language.is_empty() {
        "en"
    } else {
        &profile.identity.secondary_language
    };
    let _ = writeln!(sop, "### 输出语言");
    let _ = writeln!(sop, "- 默认语言：{primary}");
    let _ = writeln!(sop, "- 辅助语言：{secondary}");
    let _ = writeln!(sop);

    // 风格
    let style_desc = match comm.style {
        CommStyle::Direct => "直接、专业、不啰嗦",
        CommStyle::Formal => "正式、礼貌、结构化",
        CommStyle::Casual => "轻松、简短、对话式",
    };
    let _ = writeln!(sop, "### 语气");
    let _ = writeln!(sop, "- 风格：{style_desc}");
    let _ = writeln!(sop);

    // 通知长度
    let _ = writeln!(sop, "### 格式规范");
    let _ = writeln!(sop, "- 用 Markdown 结构：标题 + 要点");
    let _ = writeln!(sop, "- 避免长段落，用列表和表格");
    let _ = writeln!(
        sop,
        "- 通知最大长度：{} 字符",
        comm.notification_max_chars
    );
    let _ = writeln!(sop);

    // 建议格式
    let _ = writeln!(sop, "### 建议格式");
    let _ = writeln!(sop, "每次给决策建议时：");
    let _ = writeln!(sop, "1. **推荐方案** — 明确说做哪个");
    let _ = writeln!(
        sop,
        "2. **理由** — 基于思维模型的推理"
    );
    let _ = writeln!(
        sop,
        "3. **备选项** — 其他方案 + trade-off"
    );
}

fn write_negative_rules_section(sop: &mut String, profile: &UserProfile) {
    let _ = writeln!(sop, "## 禁止行为");
    let _ = writeln!(sop);
    for rule in &profile.negative_rules {
        let _ = writeln!(sop, "- {rule}");
    }
}

fn weekday_display(day: &Weekday) -> &'static str {
    match day {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_types::UserProfile;

    #[test]
    fn test_generate_sop_default_profile() {
        let profile = UserProfile::default();
        let sop = generate_sop(&profile);

        // 验证 7 个部分都存在
        assert!(sop.contains("## 第一部分：身份与原则"));
        assert!(sop.contains("## 第二部分：事件分类与响应矩阵"));
        assert!(sop.contains("## 第三部分：定时任务 SOP"));
        assert!(sop.contains("## 第四部分：邮件处理 SOP"));
        assert!(sop.contains("## 第五部分：会议准备 SOP"));
        assert!(sop.contains("## 第六部分：主动关怀规则"));
        assert!(sop.contains("## 第七部分：沟通风格指南"));

        // 验证 7 个分隔线（6 个 section 间 + 可能的末尾）
        let separator_count = sop.matches("---").count();
        assert!(separator_count >= 6, "至少需要 6 个分隔线，实际 {separator_count}");

        // 默认不应包含禁止行为
        assert!(!sop.contains("## 禁止行为"));
    }

    #[test]
    fn test_generate_sop_with_filled_profile() {
        let mut profile = UserProfile::default();
        profile.identity.name = "Evan".to_string();
        profile.identity.role = "EMS Team Lead".to_string();
        profile.identity.reporting_line = vec![
            "Evan".to_string(),
            "Shawn (Director)".to_string(),
            "Bob (CTO)".to_string(),
        ];
        profile.identity.primary_language = "zh".to_string();
        profile.identity.secondary_language = "en".to_string();

        profile.preferences.urgent_keywords = vec![
            "urgent".to_string(),
            "ASAP".to_string(),
            "blocker".to_string(),
        ];
        profile.preferences.important_sender_domains = vec!["voltageenergy.com".to_string()];

        let sop = generate_sop(&profile);

        assert!(sop.contains("Evan"));
        assert!(sop.contains("EMS Team Lead"));
        assert!(sop.contains("Evan → Shawn (Director) → Bob (CTO)"));
        assert!(sop.contains("urgent、ASAP、blocker"));
        assert!(sop.contains("voltageenergy.com"));
    }

    #[test]
    fn test_generate_sop_with_negative_rules() {
        let mut profile = UserProfile::default();
        profile.negative_rules = vec![
            "不要在通知中使用表情符号".to_string(),
            "不要主动发送消息给 Bob".to_string(),
        ];

        let sop = generate_sop(&profile);

        assert!(sop.contains("## 禁止行为"));
        assert!(sop.contains("不要在通知中使用表情符号"));
        assert!(sop.contains("不要主动发送消息给 Bob"));
    }

    #[test]
    fn test_generate_sop_schedule_times() {
        let mut profile = UserProfile::default();
        profile.schedule.morning_brief_hour = 9;
        profile.schedule.evening_review_hour = 20;
        profile.schedule.weekly_report_hour = 17;

        let sop = generate_sop(&profile);

        assert!(sop.contains("09:00-09:59"));
        assert!(sop.contains("20:00-20:59"));
        assert!(sop.contains("17:00+"));
    }

    #[test]
    fn test_generate_sop_stakeholders_in_email() {
        let mut profile = UserProfile::default();
        profile.work_context.stakeholders = vec![sage_types::Stakeholder {
            name: "Bob".to_string(),
            role: "CTO".to_string(),
            relationship: "上级的上级".to_string(),
            email_domain: Some("voltageenergy.com".to_string()),
        }];

        let sop = generate_sop(&profile);

        // 第四部分应包含 stakeholder 名字
        assert!(sop.contains("Bob"));
    }
}
