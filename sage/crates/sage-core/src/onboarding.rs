// Phase 4: Onboarding 状态机

use anyhow::{bail, Context, Result};
use sage_types::{
    CommPrefs, CommStyle, OnboardingStep, Project, ProjectStatus, Stakeholder, SuggestionFormat,
    UserIdentity, UserProfile, WorkSchedule,
};
use serde::Deserialize;
use serde_json::Value;

/// Onboarding 状态机，逐步收集用户信息构建 UserProfile
pub struct OnboardingState {
    current_step: OnboardingStep,
    partial_profile: UserProfile,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingState {
    /// 初始化：从 BasicInfo 步骤开始
    pub fn new() -> Self {
        Self {
            current_step: OnboardingStep::BasicInfo,
            partial_profile: UserProfile::default(),
        }
    }

    /// 当前步骤
    pub fn current_step(&self) -> &OnboardingStep {
        &self.current_step
    }

    /// 进度：(当前步骤序号, 总步骤数)
    pub fn progress(&self) -> (usize, usize) {
        (self.current_step.index(), OnboardingStep::total_steps())
    }

    /// 提交当前步骤数据，成功后自动推进到下一步
    pub fn submit_step(&mut self, data: Value) -> Result<()> {
        match self.current_step {
            OnboardingStep::BasicInfo => self.handle_basic_info(data)?,
            OnboardingStep::ReportingLine => self.handle_reporting_line(data)?,
            OnboardingStep::Projects => self.handle_projects(data)?,
            OnboardingStep::Schedule => self.handle_schedule(data)?,
            OnboardingStep::CommunicationStyle => self.handle_communication(data)?,
            OnboardingStep::Stakeholders => self.handle_stakeholders(data)?,
            OnboardingStep::Review => { /* Review 步骤无数据，直接推进 */ }
            OnboardingStep::Completed => bail!("Onboarding 已完成，无法继续提交"),
        }
        self.advance();
        Ok(())
    }

    /// 是否已完成全部步骤
    pub fn is_complete(&self) -> bool {
        self.current_step == OnboardingStep::Completed
    }

    /// 消费 self，返回完成的 UserProfile
    pub fn into_profile(self) -> UserProfile {
        self.partial_profile
    }

    /// 预览当前 partial profile 生成的 SOP
    pub fn preview_sop(&self) -> String {
        crate::profile::generate_sop(&self.partial_profile)
    }

    // ── 步骤处理 ──────────────────────────

    fn handle_basic_info(&mut self, data: Value) -> Result<()> {
        #[derive(Deserialize)]
        struct BasicInfo {
            name: String,
            role: String,
            #[serde(default)]
            primary_language: Option<String>,
            #[serde(default)]
            secondary_language: Option<String>,
        }

        let info: BasicInfo = serde_json::from_value(data)
            .context("BasicInfo 数据格式无效，期望 {name, role, primary_language?, secondary_language?}")?;

        self.partial_profile.identity = UserIdentity {
            name: info.name,
            role: info.role,
            primary_language: info.primary_language.unwrap_or_else(|| "zh".to_string()),
            secondary_language: info.secondary_language.unwrap_or_else(|| "en".to_string()),
            ..Default::default()
        };
        Ok(())
    }

    fn handle_reporting_line(&mut self, data: Value) -> Result<()> {
        #[derive(Deserialize)]
        struct ReportingLineData {
            reporting_line: Vec<String>,
        }

        let rl: ReportingLineData = serde_json::from_value(data)
            .context("ReportingLine 数据格式无效，期望 {reporting_line: [...]}")?;

        self.partial_profile.identity.reporting_line = rl.reporting_line;
        Ok(())
    }

    fn handle_projects(&mut self, data: Value) -> Result<()> {
        #[derive(Deserialize)]
        struct ProjectsData {
            projects: Vec<ProjectInput>,
        }

        #[derive(Deserialize)]
        struct ProjectInput {
            name: String,
            description: String,
            #[serde(default = "default_status")]
            status: String,
        }

        fn default_status() -> String {
            "Active".to_string()
        }

        let pd: ProjectsData = serde_json::from_value(data)
            .context("Projects 数据格式无效，期望 {projects: [{name, description, status}]}")?;

        self.partial_profile.work_context.projects = pd
            .projects
            .into_iter()
            .map(|p| Project {
                name: p.name,
                description: p.description,
                status: parse_project_status(&p.status),
            })
            .collect();
        Ok(())
    }

    fn handle_schedule(&mut self, data: Value) -> Result<()> {
        #[derive(Deserialize)]
        struct ScheduleData {
            #[serde(default = "default_8")]
            morning_brief_hour: u32,
            #[serde(default = "default_18")]
            evening_review_hour: u32,
            #[serde(default)]
            weekly_report_day: Option<String>,
            #[serde(default = "default_16")]
            weekly_report_hour: u32,
            #[serde(default = "default_8")]
            work_start_hour: u32,
            #[serde(default = "default_19")]
            work_end_hour: u32,
        }

        fn default_8() -> u32 { 8 }
        fn default_16() -> u32 { 16 }
        fn default_18() -> u32 { 18 }
        fn default_19() -> u32 { 19 }

        let sd: ScheduleData = serde_json::from_value(data)
            .context("Schedule 数据格式无效，期望 {morning_brief_hour, evening_review_hour, ...}")?;

        self.partial_profile.schedule = WorkSchedule {
            morning_brief_hour: sd.morning_brief_hour,
            evening_review_hour: sd.evening_review_hour,
            weekly_report_day: sd
                .weekly_report_day
                .map(|d| parse_weekday(&d))
                .unwrap_or(sage_types::Weekday::Fri),
            weekly_report_hour: sd.weekly_report_hour,
            work_start_hour: sd.work_start_hour,
            work_end_hour: sd.work_end_hour,
        };
        Ok(())
    }

    fn handle_communication(&mut self, data: Value) -> Result<()> {
        #[derive(Deserialize)]
        struct CommData {
            #[serde(default = "default_style")]
            style: String,
            #[serde(default = "default_max_chars")]
            notification_max_chars: usize,
        }

        fn default_style() -> String { "Direct".to_string() }
        fn default_max_chars() -> usize { 200 }

        let cd: CommData = serde_json::from_value(data)
            .context("CommunicationStyle 数据格式无效，期望 {style, notification_max_chars}")?;

        self.partial_profile.communication = CommPrefs {
            style: parse_comm_style(&cd.style),
            notification_max_chars: cd.notification_max_chars,
            suggestion_format: SuggestionFormat::ThreePartAdvice,
        };
        Ok(())
    }

    fn handle_stakeholders(&mut self, data: Value) -> Result<()> {
        #[derive(Deserialize)]
        struct StakeholdersData {
            stakeholders: Vec<StakeholderInput>,
        }

        #[derive(Deserialize)]
        struct StakeholderInput {
            name: String,
            role: String,
            relationship: String,
            #[serde(default)]
            email_domain: Option<String>,
        }

        let sd: StakeholdersData = serde_json::from_value(data)
            .context("Stakeholders 数据格式无效，期望 {stakeholders: [{name, role, relationship}]}")?;

        self.partial_profile.work_context.stakeholders = sd
            .stakeholders
            .into_iter()
            .map(|s| Stakeholder {
                name: s.name,
                role: s.role,
                relationship: s.relationship,
                email_domain: s.email_domain,
            })
            .collect();
        Ok(())
    }

    /// 推进到下一步
    fn advance(&mut self) {
        if let Some(next) = self.current_step.next() {
            self.current_step = next;
        }
    }
}

// ── 辅助解析函数 ──────────────────────────

fn parse_project_status(s: &str) -> ProjectStatus {
    match s.to_lowercase().as_str() {
        "active" => ProjectStatus::Active,
        "planning" => ProjectStatus::Planning,
        "onhold" | "on_hold" | "on-hold" => ProjectStatus::OnHold,
        "completed" => ProjectStatus::Completed,
        _ => ProjectStatus::Active,
    }
}

fn parse_weekday(s: &str) -> sage_types::Weekday {
    match s.to_lowercase().as_str() {
        "mon" | "monday" => sage_types::Weekday::Mon,
        "tue" | "tuesday" => sage_types::Weekday::Tue,
        "wed" | "wednesday" => sage_types::Weekday::Wed,
        "thu" | "thursday" => sage_types::Weekday::Thu,
        "fri" | "friday" => sage_types::Weekday::Fri,
        "sat" | "saturday" => sage_types::Weekday::Sat,
        "sun" | "sunday" => sage_types::Weekday::Sun,
        _ => sage_types::Weekday::Fri,
    }
}

fn parse_comm_style(s: &str) -> CommStyle {
    match s.to_lowercase().as_str() {
        "direct" => CommStyle::Direct,
        "formal" => CommStyle::Formal,
        "casual" => CommStyle::Casual,
        _ => CommStyle::Direct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_new_state() {
        let state = OnboardingState::new();
        assert_eq!(*state.current_step(), OnboardingStep::BasicInfo);
        assert_eq!(state.progress(), (0, 7));
        assert!(!state.is_complete());
    }

    #[test]
    fn test_full_onboarding_flow() {
        let mut state = OnboardingState::new();

        // Step 1: BasicInfo
        assert_eq!(*state.current_step(), OnboardingStep::BasicInfo);
        state
            .submit_step(json!({
                "name": "Evan",
                "role": "EMS Team Lead",
                "primary_language": "zh",
                "secondary_language": "en"
            }))
            .unwrap();
        assert_eq!(*state.current_step(), OnboardingStep::ReportingLine);

        // Step 2: ReportingLine
        state
            .submit_step(json!({
                "reporting_line": ["Evan", "Shawn (Director)", "Bob (CTO)"]
            }))
            .unwrap();
        assert_eq!(*state.current_step(), OnboardingStep::Projects);

        // Step 3: Projects
        state
            .submit_step(json!({
                "projects": [
                    {"name": "PULSE", "description": "工厂能源监控平台", "status": "Active"},
                    {"name": "VoltageEMS", "description": "Rust 架构 EMS", "status": "Active"}
                ]
            }))
            .unwrap();
        assert_eq!(*state.current_step(), OnboardingStep::Schedule);

        // Step 4: Schedule
        state
            .submit_step(json!({
                "morning_brief_hour": 8,
                "evening_review_hour": 18,
                "weekly_report_day": "Fri",
                "weekly_report_hour": 16,
                "work_start_hour": 8,
                "work_end_hour": 19
            }))
            .unwrap();
        assert_eq!(*state.current_step(), OnboardingStep::CommunicationStyle);

        // Step 5: CommunicationStyle
        state
            .submit_step(json!({
                "style": "Direct",
                "notification_max_chars": 200
            }))
            .unwrap();
        assert_eq!(*state.current_step(), OnboardingStep::Stakeholders);

        // Step 6: Stakeholders
        state
            .submit_step(json!({
                "stakeholders": [
                    {"name": "Shawn", "role": "Director", "relationship": "直属上级"},
                    {"name": "Bob", "role": "CTO", "relationship": "上级的上级"}
                ]
            }))
            .unwrap();
        assert_eq!(*state.current_step(), OnboardingStep::Review);

        // Step 7: Review（无数据）
        state.submit_step(json!({})).unwrap();
        assert!(state.is_complete());
        assert_eq!(state.progress(), (7, 7));

        // 验证最终 profile
        let profile = state.into_profile();
        assert_eq!(profile.identity.name, "Evan");
        assert_eq!(profile.identity.role, "EMS Team Lead");
        assert_eq!(profile.identity.reporting_line.len(), 3);
        assert_eq!(profile.work_context.projects.len(), 2);
        assert_eq!(profile.work_context.stakeholders.len(), 2);
        assert_eq!(profile.schedule.morning_brief_hour, 8);
        assert_eq!(profile.communication.notification_max_chars, 200);
    }

    #[test]
    fn test_preview_sop() {
        let mut state = OnboardingState::new();
        state
            .submit_step(json!({
                "name": "Test",
                "role": "Engineer"
            }))
            .unwrap();

        let sop = state.preview_sop();
        assert!(sop.contains("Test"));
        assert!(sop.contains("Engineer"));
        assert!(sop.contains("## 第一部分：身份与原则"));
    }

    #[test]
    fn test_invalid_data_returns_error() {
        let mut state = OnboardingState::new();

        // BasicInfo 缺少必填字段 name
        let result = state.submit_step(json!({"role": "test"}));
        assert!(result.is_err());

        // 步骤不应推进
        assert_eq!(*state.current_step(), OnboardingStep::BasicInfo);
    }

    #[test]
    fn test_submit_after_completed_returns_error() {
        let mut state = OnboardingState::new();

        // 快速走完所有步骤
        state.submit_step(json!({"name": "A", "role": "B"})).unwrap();
        state.submit_step(json!({"reporting_line": []})).unwrap();
        state.submit_step(json!({"projects": []})).unwrap();
        state.submit_step(json!({})).unwrap(); // schedule with defaults
        state.submit_step(json!({})).unwrap(); // comm with defaults
        state.submit_step(json!({"stakeholders": []})).unwrap();
        state.submit_step(json!({})).unwrap(); // review

        assert!(state.is_complete());
        let result = state.submit_step(json!({}));
        assert!(result.is_err());
    }
}
