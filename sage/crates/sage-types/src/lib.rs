use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Event 模型 ──────────────────────────────────

/// 从外部世界进入 Sage 的事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub source: String,
    pub event_type: EventType,
    pub title: String,
    pub body: String,
    pub metadata: HashMap<String, String>,
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    NewEmail,
    UrgentEmail,
    UpcomingMeeting,
    NewMessage,
    PatternObserved,
    ScheduledTask,
}

// ─── UserProfile 模型 ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    pub identity: UserIdentity,
    pub work_context: WorkContext,
    pub communication: CommPrefs,
    pub schedule: WorkSchedule,
    pub preferences: BehaviorPrefs,
    /// "不要再这样做"的规则
    pub negative_rules: Vec<String>,
    /// SOP 版本号，变更时触发重新生成
    pub sop_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserIdentity {
    pub name: String,
    pub role: String,
    /// 汇报线：["Evan", "Shawn (Director)", "Bob (CTO)"]
    pub reporting_line: Vec<String>,
    /// 主要工作语言
    pub primary_language: String,
    /// 次要工作语言
    pub secondary_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkContext {
    pub projects: Vec<Project>,
    pub stakeholders: Vec<Stakeholder>,
    pub tech_stack: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub description: String,
    pub status: ProjectStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectStatus {
    Active,
    Planning,
    OnHold,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stakeholder {
    pub name: String,
    pub role: String,
    pub relationship: String,
    /// 邮件地址或域名，用于紧急邮件判断
    pub email_domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommPrefs {
    /// 沟通风格偏好
    pub style: CommStyle,
    /// 通知最大长度
    pub notification_max_chars: usize,
    /// 建议格式
    pub suggestion_format: SuggestionFormat,
}

impl Default for CommPrefs {
    fn default() -> Self {
        Self {
            style: CommStyle::Direct,
            notification_max_chars: 200,
            suggestion_format: SuggestionFormat::ThreePartAdvice,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommStyle {
    Direct,
    Formal,
    Casual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestionFormat {
    /// 推荐方案 → 理由 → 备选项
    ThreePartAdvice,
    /// 简短结论
    BriefConclusion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkSchedule {
    pub morning_brief_hour: u32,
    pub evening_review_hour: u32,
    pub weekly_report_day: Weekday,
    pub weekly_report_hour: u32,
    pub work_start_hour: u32,
    pub work_end_hour: u32,
}

impl Default for WorkSchedule {
    fn default() -> Self {
        Self {
            morning_brief_hour: 8,
            evening_review_hour: 18,
            weekly_report_day: Weekday::Fri,
            weekly_report_hour: 16,
            work_start_hour: 8,
            work_end_hour: 19,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn to_chrono(&self) -> chrono::Weekday {
        match self {
            Weekday::Mon => chrono::Weekday::Mon,
            Weekday::Tue => chrono::Weekday::Tue,
            Weekday::Wed => chrono::Weekday::Wed,
            Weekday::Thu => chrono::Weekday::Thu,
            Weekday::Fri => chrono::Weekday::Fri,
            Weekday::Sat => chrono::Weekday::Sat,
            Weekday::Sun => chrono::Weekday::Sun,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BehaviorPrefs {
    /// 紧急关键词列表
    pub urgent_keywords: Vec<String>,
    /// 重要发件人域名
    pub important_sender_domains: Vec<String>,
}

// ─── Feedback 模型 ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeedbackAction {
    Useful,
    NotUseful,
    /// 永远不要再这样做 + 原因
    NeverDoThis(String),
    /// 纠正 + 正确内容
    Correction(String),
}

// ─── Onboarding 模型 ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OnboardingStep {
    BasicInfo,
    ReportingLine,
    Projects,
    Schedule,
    CommunicationStyle,
    Stakeholders,
    Review,
    Completed,
}

impl OnboardingStep {
    pub fn index(&self) -> usize {
        match self {
            Self::BasicInfo => 0,
            Self::ReportingLine => 1,
            Self::Projects => 2,
            Self::Schedule => 3,
            Self::CommunicationStyle => 4,
            Self::Stakeholders => 5,
            Self::Review => 6,
            Self::Completed => 7,
        }
    }

    pub fn total_steps() -> usize {
        7
    }

    pub fn next(&self) -> Option<Self> {
        match self {
            Self::BasicInfo => Some(Self::ReportingLine),
            Self::ReportingLine => Some(Self::Projects),
            Self::Projects => Some(Self::Schedule),
            Self::Schedule => Some(Self::CommunicationStyle),
            Self::CommunicationStyle => Some(Self::Stakeholders),
            Self::Stakeholders => Some(Self::Review),
            Self::Review => Some(Self::Completed),
            Self::Completed => None,
        }
    }
}

// ─── Suggestion 模型（建议历史记录）──────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: i64,
    pub event_source: String,
    pub prompt: String,
    pub response: String,
    pub timestamp: DateTime<Local>,
    pub feedback: Option<FeedbackAction>,
}

// ─── Chat 模型 ──────────────────────────────────

/// 聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,        // "user" | "sage"
    pub content: String,
    pub session_id: String,
    pub created_at: String,
}

/// 聊天会话概览（从 chat_messages 聚合）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub session_id: String,
    pub preview: String,
    pub message_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 记忆/洞察
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub category: String,    // "identity" | "values" | "behavior" | "thinking" | "emotion" | "growth"
    pub content: String,
    pub source: String,      // "chat" | "observation" | "feedback"
    pub confidence: f64,     // 0.0-1.0
    /// 可见性层级：public（Digital Evan 可见）| private（仅 Evan）| subconscious（AI 推断）
    #[serde(default = "default_visibility")]
    pub visibility: String,
    pub created_at: String,
    pub updated_at: String,
}

fn default_visibility() -> String {
    "public".to_string()
}

// ─── Memory Edge 模型（记忆图谱连接）────────────────

/// 记忆之间的语义连接
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEdge {
    pub id: i64,
    pub from_id: i64,
    pub to_id: i64,
    /// 关系类型：similar / derived_from / co_occurred / causes / contradicts / supports
    pub relation: String,
    /// 关系强度 0.0-1.0
    pub weight: f64,
    pub created_at: String,
}

// ─── Knowledge Edge 模型（通用知识图谱连接）────────────────

/// 不同类型节点之间的语义连接
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    pub id: i64,
    pub from_type: String,   // "memory" | "message" | "observation" | "question"
    pub from_id: i64,
    pub to_type: String,     // "memory" | "message" | "observation" | "question"
    pub to_id: i64,
    pub relation: String,    // "references" | "triggers" | "answers" | "supports" | "contradicts" | "co_occurred" | "causes" | "derived_from" | "similar"
    pub weight: f64,         // 0.0-1.0
    pub created_at: String,
}

// ─── Report 模型（定时报告记录）────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: i64,
    pub report_type: String,
    pub content: String,
    pub created_at: String,
}

// ─── Provider 模型 ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderKind {
    Cli,
    HttpApi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProviderStatus {
    Ready,
    NeedsApiKey,
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub kind: ProviderKind,
    pub status: ProviderStatus,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider_id: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub enabled: bool,
    /// 用户自定义优先级（覆盖默认值，越小越优先）
    #[serde(default)]
    pub priority: Option<u8>,
}

// ─── Message 模型（通讯消息）────────────────────────

/// 来自 Teams/Email/Slack 等的通讯消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub sender: String,
    pub channel: String,
    pub content: Option<String>,
    pub source: String,       // "teams" | "email" | "slack"
    pub message_type: String, // "text" | "file" | "meeting"
    pub timestamp: String,    // 消息原始时间
    pub created_at: String,   // 入库时间
}

// ─── Browser Bridge 模型 ──────────────────────────

/// 浏览器插件导入记忆请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeImportRequest {
    /// 来源平台：claude / chatgpt / gemini / other
    pub source: String,
    /// 记忆条目
    pub memories: Vec<BridgeMemoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMemoryEntry {
    pub category: String,
    pub content: String,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
}

fn default_confidence() -> f64 {
    0.7
}

/// 浏览器行为事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeBehaviorEvent {
    /// 来源平台
    pub source: String,
    /// 事件类型：conversation_start / conversation_end / topic_switch / memory_created
    pub event_type: String,
    /// 附加数据（值可以是字符串、数字、布尔等任意 JSON 类型）
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Bridge 状态响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStatusResponse {
    pub status: String,
    pub version: String,
    pub memory_count: usize,
}
