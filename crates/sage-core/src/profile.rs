// Phase 3: Profile → 动态 SOP 生成

use sage_types::{CommStyle, UserProfile, Weekday};
use std::fmt::Write;

/// 根据用户 Profile 动态生成 SOP 文本（语言取自 profile.identity.prompt_language）
pub fn generate_sop(profile: &UserProfile) -> String {
    generate_sop_lang(profile, &profile.identity.prompt_language)
}

/// 同上，但允许外部传入语言覆盖 profile 设置
pub fn generate_sop_lang(profile: &UserProfile, lang: &str) -> String {
    let mut sop = String::with_capacity(4096);

    write_identity_section(&mut sop, profile, lang);
    sop.push_str("\n---\n\n");

    write_event_matrix_section(&mut sop, profile, lang);
    sop.push_str("\n---\n\n");

    write_scheduled_tasks_section(&mut sop, profile, lang);
    sop.push_str("\n---\n\n");

    write_email_section(&mut sop, profile, lang);
    sop.push_str("\n---\n\n");

    write_meeting_section(&mut sop, lang);
    sop.push_str("\n---\n\n");

    write_wellbeing_section(&mut sop, profile, lang);
    sop.push_str("\n---\n\n");

    write_communication_section(&mut sop, profile, lang);

    if !profile.negative_rules.is_empty() {
        sop.push_str("\n---\n\n");
        write_negative_rules_section(&mut sop, profile, lang);
    }

    sop
}

fn write_identity_section(sop: &mut String, profile: &UserProfile, lang: &str) {
    let id = &profile.identity;
    let name = if id.name.is_empty() {
        match lang {
            "en" => "User",
            _ => "用户",
        }
    } else {
        &id.name
    };
    let role = if id.role.is_empty() {
        match lang {
            "en" => "Not set".to_string(),
            _ => "未设置".to_string(),
        }
    } else {
        id.role.clone()
    };

    let (heading, tagline, role_label, report_label, principles_label) = match lang {
        "en" => (
            "## Part 1: Identity & Principles",
            format!("**You are Sage, {name}'s personal advisor — not a stand-in.**"),
            "Role",
            "Reporting line",
            "**Core behavioral principles**:",
        ),
        _ => (
            "## 第一部分：身份与原则",
            format!("**你是 Sage，{name} 的个人参谋，不是替身。**"),
            "角色",
            "汇报线",
            "**核心行为原则**：",
        ),
    };

    let _ = writeln!(sop, "{heading}");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "{tagline}");
    let _ = writeln!(sop);
    let _ = writeln!(sop, "- {role_label}：{role}");

    if !id.reporting_line.is_empty() {
        let _ = writeln!(sop, "- {report_label}：{}", id.reporting_line.join(" → "));
    }

    let _ = writeln!(sop);
    let _ = writeln!(sop, "{principles_label}");
    let _ = writeln!(sop);

    if lang == "en" {
        let _ = writeln!(sop, "1. Accelerate decisions, never replace judgment");
        let _ = writeln!(sop, "2. {name} decides; Sage advises");
        let _ = writeln!(
            sop,
            "3. Recommendation format: preferred option → rationale → alternatives (with trade-offs)"
        );
        let _ = writeln!(sop, "4. Systems thinking: look at structure, not surface symptoms");
        let _ = writeln!(sop, "5. Pragmatism: good enough ships, perfection doesn't");
    } else {
        let _ = writeln!(sop, "1. 不替代判断，只加速决策");
        let _ = writeln!(sop, "2. {name} 做决策，Sage 提供建议");
        let _ = writeln!(sop, "3. 建议格式：推荐方案 → 理由 → 备选项（含 trade-off）");
        let _ = writeln!(sop, "4. 系统思考：看结构，不看表面症状");
        let _ = writeln!(sop, "5. 实用主义：能用就行，不追求完美形式");
    }
}

fn write_event_matrix_section(sop: &mut String, profile: &UserProfile, lang: &str) {
    let urgent = &profile.preferences.urgent_keywords;

    if lang == "en" {
        let _ = writeln!(sop, "## Part 2: Event Classification & Response Matrix");
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "| Event Type | Trigger | Response | Priority |"
        );
        let _ = writeln!(sop, "|-----------|---------|----------|----------|");
        let _ = writeln!(
            sop,
            "| Urgent email | Key sender / urgent keyword | Immediate alert + AI summary + action advice | Immediate |"
        );
        let _ = writeln!(
            sop,
            "| Upcoming meeting | Meeting within 30 min | Immediate reminder + talking points | Immediate |"
        );
        let _ = writeln!(
            sop,
            "| Scheduled task | Heartbeat window hit | AI-generated structured output | Scheduled |"
        );
        let _ = writeln!(
            sop,
            "| Routine email | Non-urgent new email | Log pattern, no interruption | Normal |"
        );
        let _ = writeln!(
            sop,
            "| New message | Teams / Lark message | Log pattern, no interruption | Normal |"
        );
        let _ = writeln!(
            sop,
            "| Behavior pattern | Repeated behavior detected | Silent logging | Background |"
        );
        let _ = writeln!(sop);
        if !urgent.is_empty() {
            let _ = writeln!(sop, "**Urgent keywords**: {}", urgent.join(", "));
        } else {
            let _ = writeln!(
                sop,
                "**Urgent keywords**: urgent, ASAP, blocker, critical, help, deadline"
            );
        }
    } else {
        let _ = writeln!(sop, "## 第二部分：事件分类与响应矩阵");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "| 事件类型 | 触发条件 | 响应方式 | 优先级 |");
        let _ = writeln!(sop, "|---------|---------|---------|-------|");
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
        let _ = writeln!(sop);
        if !urgent.is_empty() {
            let _ = writeln!(sop, "**紧急关键词**：{}", urgent.join("、"));
        } else {
            let _ = writeln!(
                sop,
                "**紧急关键词**：urgent、ASAP、blocker、critical、help、deadline"
            );
        }
    }
}

fn write_scheduled_tasks_section(sop: &mut String, profile: &UserProfile, lang: &str) {
    let s = &profile.schedule;
    let day_name = weekday_display(&s.weekly_report_day, lang);

    if lang == "en" {
        let _ = writeln!(sop, "## Part 3: Scheduled Task SOPs");
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "### Morning Brief (weekdays {:02}:00–{:02}:59)",
            s.morning_brief_hour, s.morning_brief_hour
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "1. Load yesterday's unfinished items from memory");
        let _ = writeln!(sop, "2. Fetch unread emails (sorted by sender priority)");
        let _ = writeln!(sop, "3. Fetch today's calendar (flag meetings that need prep)");
        let _ = writeln!(sop, "4. AI generates structured brief");
        let _ = writeln!(sop, "5. Push notification; write full content to memory");
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "### Evening Review (weekdays {:02}:00–{:02}:59)",
            s.evening_review_hour, s.evening_review_hour
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "1. Load all decisions made today");
        let _ = writeln!(sop, "2. Identify unfinished items");
        let _ = writeln!(sop, "3. Summarize newly detected behavior patterns");
        let _ = writeln!(sop, "4. AI generates summary + tomorrow's recommendations");
        let _ = writeln!(sop, "5. Update memory, push notification");
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "### Weekly Report ({day_name} {:02}:00+)",
            s.weekly_report_hour
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "1. Aggregate all decisions from the week");
        let _ = writeln!(sop, "2. Analyze newly detected patterns");
        let _ = writeln!(sop, "3. Generate weekly report draft from template");
        let _ = writeln!(sop, "4. Notify user to review");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Week Start (Monday {:02}:00)", s.morning_brief_hour);
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "1. Pull this week's calendar; flag key meetings and deadlines"
        );
        let _ = writeln!(sop, "2. Load last week's unfinished items");
        let _ = writeln!(sop, "3. Generate this week's priority reminders");
    } else {
        let _ = writeln!(sop, "## 第三部分：定时任务 SOP");
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "### Morning Brief（工作日 {:02}:00-{:02}:59）",
            s.morning_brief_hour, s.morning_brief_hour
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "1. 读取 memory 中昨日未完成事项");
        let _ = writeln!(sop, "2. 拉取未读邮件（按发件人优先级排序）");
        let _ = writeln!(sop, "3. 拉取今日日程（标注需要准备的会议）");
        let _ = writeln!(sop, "4. AI 生成结构化 brief");
        let _ = writeln!(sop, "5. 推送通知，完整内容写入 memory");
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "### Evening Review（工作日 {:02}:00-{:02}:59）",
            s.evening_review_hour, s.evening_review_hour
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "1. 读取今日所有决策记录");
        let _ = writeln!(sop, "2. 识别未完成事项");
        let _ = writeln!(sop, "3. 汇总新增行为模式");
        let _ = writeln!(sop, "4. AI 生成总结 + 明日建议");
        let _ = writeln!(sop, "5. 更新 memory，推送通知");
        let _ = writeln!(sop);
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
        let _ = writeln!(sop, "### Week Start（周一 {:02}:00）", s.morning_brief_hour);
        let _ = writeln!(sop);
        let _ = writeln!(sop, "1. 拉取本周日程，标注重点会议和 deadline");
        let _ = writeln!(sop, "2. 读取上周未完成事项");
        let _ = writeln!(sop, "3. 生成本周重点提醒");
    }
}

fn write_email_section(sop: &mut String, profile: &UserProfile, lang: &str) {
    let important_senders: Vec<String> = profile
        .work_context
        .stakeholders
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let domains = &profile.preferences.important_sender_domains;

    if lang == "en" {
        let _ = writeln!(sop, "## Part 4: Email Handling SOP");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Urgent email criteria");
        let _ = writeln!(sop);
        let _ = writeln!(
            sop,
            "Escalate to urgent if any of the following match:"
        );
        if !important_senders.is_empty() {
            let _ = writeln!(sop, "- Sender: {}", important_senders.join(", "));
        }
        if !domains.is_empty() {
            let _ = writeln!(sop, "- Sender domain: {}", domains.join(", "));
        }
        let _ = writeln!(sop, "- Subject contains an urgent keyword");
        let _ = writeln!(
            sop,
            "- Reply thread exceeds 5 messages and user is the last recipient"
        );
        let _ = writeln!(
            sop,
            "- Email sent outside work hours (before {:02}:00 / after {:02}:00)",
            profile.schedule.work_start_hour,
            profile.schedule.work_end_hour + 1
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Email summary format");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "```");
        let _ = writeln!(sop, "[Sender] [Time]");
        let _ = writeln!(sop, "Subject: ...");
        let _ = writeln!(sop, "Core content (1–2 sentences): ...");
        let _ = writeln!(sop, "Required action: [Reply / Decide / Forward / Archive]");
        let _ = writeln!(sop, "Suggested priority: High / Medium / Low");
        let _ = writeln!(sop, "```");
    } else {
        let _ = writeln!(sop, "## 第四部分：邮件处理 SOP");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### 紧急邮件判断规则");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "命中以下任一条件，升级为紧急邮件：");
        if !important_senders.is_empty() {
            let _ = writeln!(sop, "- 发件人：{}", important_senders.join("、"));
        }
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
}

fn write_meeting_section(sop: &mut String, lang: &str) {
    if lang == "en" {
        let _ = writeln!(sop, "## Part 5: Meeting Prep SOP");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Trigger");
        let _ = writeln!(sop, "30 minutes before the meeting starts");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Steps");
        let _ = writeln!(
            sop,
            "1. Fetch meeting info from calendar (title, attendees, agenda)"
        );
        let _ = writeln!(sop, "2. Look up relevant attendee backgrounds");
        let _ = writeln!(sop, "3. Look up related project status");
        let _ = writeln!(
            sop,
            "4. AI generates talking points (3–5 items; focus on decisions or alignment needed)"
        );
        let _ = writeln!(sop, "5. Push notification");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Talking point guidelines");
        let _ = writeln!(sop, "- Opening: confirm agenda alignment");
        let _ = writeln!(sop, "- Body: items to drive forward (by priority)");
        let _ = writeln!(sop, "- Close: clarify next action + owner");
    } else {
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
}

fn write_wellbeing_section(sop: &mut String, profile: &UserProfile, lang: &str) {
    let s = &profile.schedule;

    if lang == "en" {
        let _ = writeln!(sop, "## Part 6: Proactive Wellbeing Rules");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Work-hours monitoring");
        let _ = writeln!(
            sop,
            "- Work hours: {:02}:00 – {:02}:00",
            s.work_start_hour, s.work_end_hour
        );
        let _ = writeln!(
            sop,
            "- If focus is unbroken for 3+ hours with no break >15 min, send a rest reminder"
        );
        let _ = writeln!(
            sop,
            "- Reminder tone: brief, non-preachy, one concrete suggestion"
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Holiday awareness");
        let _ = writeln!(sop, "- Send reminder the day before a holiday");
        let _ = writeln!(
            sop,
            "- During holidays: skip Morning Brief, but keep urgent-email monitoring active"
        );
    } else {
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
        let _ = writeln!(sop, "- 提醒内容：简短，不说教，给一个具体建议");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### 节假日识别");
        let _ = writeln!(sop, "- 节假日前一天推送提醒");
        let _ = writeln!(sop, "- 节假日期间不触发 Morning Brief，但保留紧急邮件监控");
    }
}

fn write_communication_section(sop: &mut String, profile: &UserProfile, lang: &str) {
    let comm = &profile.communication;
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

    if lang == "en" {
        let style_desc = match comm.style {
            CommStyle::Direct => "Direct, professional, no fluff",
            CommStyle::Formal => "Formal, courteous, structured",
            CommStyle::Casual => "Relaxed, concise, conversational",
        };
        let _ = writeln!(sop, "## Part 7: Communication Style Guide");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Output language");
        let _ = writeln!(sop, "- Primary: {primary}");
        let _ = writeln!(sop, "- Secondary: {secondary}");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Tone");
        let _ = writeln!(sop, "- Style: {style_desc}");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Formatting");
        let _ = writeln!(sop, "- Use Markdown structure: headings + bullet points");
        let _ = writeln!(sop, "- Avoid long paragraphs; prefer lists and tables");
        let _ = writeln!(
            sop,
            "- Max notification length: {} characters",
            comm.notification_max_chars
        );
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### Recommendation format");
        let _ = writeln!(sop, "For every decision recommendation:");
        let _ = writeln!(sop, "1. **Preferred option** — be explicit about which one");
        let _ = writeln!(sop, "2. **Rationale** — reasoning grounded in mental models");
        let _ = writeln!(
            sop,
            "3. **Alternatives** — other options with their trade-offs"
        );
    } else {
        let style_desc = match comm.style {
            CommStyle::Direct => "直接、专业、不啰嗦",
            CommStyle::Formal => "正式、礼貌、结构化",
            CommStyle::Casual => "轻松、简短、对话式",
        };
        let _ = writeln!(sop, "## 第七部分：沟通风格指南");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### 输出语言");
        let _ = writeln!(sop, "- 默认语言：{primary}");
        let _ = writeln!(sop, "- 辅助语言：{secondary}");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### 语气");
        let _ = writeln!(sop, "- 风格：{style_desc}");
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### 格式规范");
        let _ = writeln!(sop, "- 用 Markdown 结构：标题 + 要点");
        let _ = writeln!(sop, "- 避免长段落，用列表和表格");
        let _ = writeln!(sop, "- 通知最大长度：{} 字符", comm.notification_max_chars);
        let _ = writeln!(sop);
        let _ = writeln!(sop, "### 建议格式");
        let _ = writeln!(sop, "每次给决策建议时：");
        let _ = writeln!(sop, "1. **推荐方案** — 明确说做哪个");
        let _ = writeln!(sop, "2. **理由** — 基于思维模型的推理");
        let _ = writeln!(sop, "3. **备选项** — 其他方案 + trade-off");
    }
}

fn write_negative_rules_section(sop: &mut String, profile: &UserProfile, lang: &str) {
    let heading = match lang {
        "en" => "## Prohibited behaviors",
        _ => "## 禁止行为",
    };
    let _ = writeln!(sop, "{heading}");
    let _ = writeln!(sop);
    for rule in &profile.negative_rules {
        let _ = writeln!(sop, "- {rule}");
    }
}

fn weekday_display(day: &Weekday, lang: &str) -> &'static str {
    match (day, lang) {
        (Weekday::Mon, "en") => "Monday",
        (Weekday::Tue, "en") => "Tuesday",
        (Weekday::Wed, "en") => "Wednesday",
        (Weekday::Thu, "en") => "Thursday",
        (Weekday::Fri, "en") => "Friday",
        (Weekday::Sat, "en") => "Saturday",
        (Weekday::Sun, "en") => "Sunday",
        (Weekday::Mon, _) => "周一",
        (Weekday::Tue, _) => "周二",
        (Weekday::Wed, _) => "周三",
        (Weekday::Thu, _) => "周四",
        (Weekday::Fri, _) => "周五",
        (Weekday::Sat, _) => "周六",
        (Weekday::Sun, _) => "周日",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_types::UserProfile;

    #[test]
    fn test_generate_sop_default_profile_zh() {
        let profile = UserProfile::default();
        let sop = generate_sop_lang(&profile, "zh");

        assert!(sop.contains("## 第一部分：身份与原则"));
        assert!(sop.contains("## 第二部分：事件分类与响应矩阵"));
        assert!(sop.contains("## 第三部分：定时任务 SOP"));
        assert!(sop.contains("## 第四部分：邮件处理 SOP"));
        assert!(sop.contains("## 第五部分：会议准备 SOP"));
        assert!(sop.contains("## 第六部分：主动关怀规则"));
        assert!(sop.contains("## 第七部分：沟通风格指南"));

        let separator_count = sop.matches("---").count();
        assert!(
            separator_count >= 6,
            "至少需要 6 个分隔线，实际 {separator_count}"
        );
        assert!(!sop.contains("## 禁止行为"));
    }

    #[test]
    fn test_generate_sop_default_profile_en() {
        let profile = UserProfile::default();
        let sop = generate_sop_lang(&profile, "en");

        assert!(sop.contains("## Part 1: Identity & Principles"));
        assert!(sop.contains("## Part 2: Event Classification & Response Matrix"));
        assert!(sop.contains("## Part 3: Scheduled Task SOPs"));
        assert!(sop.contains("## Part 4: Email Handling SOP"));
        assert!(sop.contains("## Part 5: Meeting Prep SOP"));
        assert!(sop.contains("## Part 6: Proactive Wellbeing Rules"));
        assert!(sop.contains("## Part 7: Communication Style Guide"));

        let separator_count = sop.matches("---").count();
        assert!(
            separator_count >= 6,
            "need at least 6 separators, got {separator_count}"
        );
        assert!(!sop.contains("## Prohibited behaviors"));
    }

    #[test]
    fn test_generate_sop_with_filled_profile() {
        let mut profile = UserProfile::default();
        profile.identity.name = "Alex".to_string();
        profile.identity.role = "EMS Team Lead".to_string();
        profile.identity.reporting_line = vec![
            "Alex".to_string(),
            "Jordan (Director)".to_string(),
            "Sam (CTO)".to_string(),
        ];
        profile.identity.primary_language = "zh".to_string();
        profile.identity.secondary_language = "en".to_string();

        profile.preferences.urgent_keywords = vec![
            "urgent".to_string(),
            "ASAP".to_string(),
            "blocker".to_string(),
        ];
        profile.preferences.important_sender_domains = vec!["example.com".to_string()];

        let sop = generate_sop_lang(&profile, "zh");

        assert!(sop.contains("Alex"));
        assert!(sop.contains("EMS Team Lead"));
        assert!(sop.contains("Alex → Jordan (Director) → Sam (CTO)"));
        assert!(sop.contains("urgent、ASAP、blocker"));
        assert!(sop.contains("example.com"));
    }

    #[test]
    fn test_generate_sop_with_negative_rules() {
        let mut profile = UserProfile::default();
        profile.negative_rules = vec![
            "不要在通知中使用表情符号".to_string(),
            "不要主动发送消息给 Sam".to_string(),
        ];

        let sop = generate_sop_lang(&profile, "zh");

        assert!(sop.contains("## 禁止行为"));
        assert!(sop.contains("不要在通知中使用表情符号"));
        assert!(sop.contains("不要主动发送消息给 Sam"));
    }

    #[test]
    fn test_generate_sop_negative_rules_en() {
        let mut profile = UserProfile::default();
        profile.negative_rules = vec!["Do not use emojis in notifications".to_string()];

        let sop = generate_sop_lang(&profile, "en");

        assert!(sop.contains("## Prohibited behaviors"));
        assert!(sop.contains("Do not use emojis in notifications"));
    }

    #[test]
    fn test_generate_sop_schedule_times() {
        let mut profile = UserProfile::default();
        profile.schedule.morning_brief_hour = 9;
        profile.schedule.evening_review_hour = 20;
        profile.schedule.weekly_report_hour = 17;

        let sop = generate_sop_lang(&profile, "zh");

        assert!(sop.contains("09:00-09:59"));
        assert!(sop.contains("20:00-20:59"));
        assert!(sop.contains("17:00+"));
    }

    #[test]
    fn test_generate_sop_stakeholders_in_email() {
        let mut profile = UserProfile::default();
        profile.work_context.stakeholders = vec![sage_types::Stakeholder {
            name: "Sam".to_string(),
            role: "CTO".to_string(),
            relationship: "上级的上级".to_string(),
            email_domain: Some("example.com".to_string()),
        }];

        let sop = generate_sop_lang(&profile, "zh");
        assert!(sop.contains("Sam"));
    }

    #[test]
    fn test_weekday_display_bilingual() {
        assert_eq!(weekday_display(&Weekday::Mon, "en"), "Monday");
        assert_eq!(weekday_display(&Weekday::Fri, "en"), "Friday");
        assert_eq!(weekday_display(&Weekday::Mon, "zh"), "周一");
        assert_eq!(weekday_display(&Weekday::Sun, "zh"), "周日");
    }
}
