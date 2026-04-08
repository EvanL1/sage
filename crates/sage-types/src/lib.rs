use chrono::{DateTime, Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Timestamp 工具 ──────────────────────────────────

/// 将各种格式的时间戳标准化为 ISO 8601（`YYYY-MM-DDTHH:MM:SS`）。
/// 已是 ISO 格式的直接返回，无法解析的原样返回。
pub fn normalize_timestamp(raw: &str) -> String {
    let s = raw.trim();

    // 已经是 ISO 格式
    if s.len() >= 19 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(10) == Some(&b'T') {
        return s.to_string();
    }

    // 中文格式："2026年3月23日 星期一 15:48:19"
    if let Some(result) = parse_chinese_timestamp(s) {
        return result;
    }

    // RFC 2822: "Mon, 23 Mar 2026 15:48:19 +0800"
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    }

    // 常见格式尝试
    let formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y/%m/%d %H:%M:%S",
        "%d/%m/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
    ];
    for fmt in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return dt.format("%Y-%m-%dT%H:%M:%S").to_string();
        }
    }

    // 无法解析，原样返回
    s.to_string()
}

#[cfg(test)]
mod timestamp_tests {
    use super::normalize_timestamp;

    #[test]
    fn iso_passthrough() {
        assert_eq!(
            normalize_timestamp("2026-03-23T15:48:19"),
            "2026-03-23T15:48:19"
        );
    }

    #[test]
    fn iso_with_millis() {
        assert!(normalize_timestamp("2026-03-26T08:25:42.123").starts_with("2026-03-26T08:25:42"));
    }

    #[test]
    fn chinese_with_weekday() {
        assert_eq!(
            normalize_timestamp("2026年3月23日 星期一 15:48:19"),
            "2026-03-23T15:48:19"
        );
    }

    #[test]
    fn chinese_without_weekday() {
        assert_eq!(
            normalize_timestamp("2026年3月5日 09:30:00"),
            "2026-03-05T09:30:00"
        );
    }

    #[test]
    fn dash_format() {
        assert_eq!(
            normalize_timestamp("2026-03-23 15:48:19"),
            "2026-03-23T15:48:19"
        );
    }

    #[test]
    fn unparseable_passthrough() {
        assert_eq!(normalize_timestamp("garbage"), "garbage");
    }
}

/// 解析中文日期格式："2026年3月23日 星期一 15:48:19" 或 "2026年3月23日 15:48:19"
fn parse_chinese_timestamp(s: &str) -> Option<String> {
    if !s.contains('年') {
        return None;
    }

    let year = s.split('年').next()?.trim().parse::<i32>().ok()?;
    let after_year = s.split('年').nth(1)?;
    let month = after_year.split('月').next()?.trim().parse::<u32>().ok()?;
    let after_month = after_year.split('月').nth(1)?;
    let day = after_month.split('日').next()?.trim().parse::<u32>().ok()?;

    // 时间部分：在 "日" 之后，可能有 "星期X " 前缀
    let after_day = after_month.split('日').nth(1)?.trim();
    let time_part = if after_day.starts_with("星期") {
        // 跳过 "星期X "
        after_day.splitn(2, ' ').nth(1).unwrap_or("00:00:00").trim()
    } else {
        after_day
    };

    let parts: Vec<&str> = time_part.split(':').collect();
    let (hour, min, sec) = match parts.len() {
        3 => (
            parts[0].parse::<u32>().unwrap_or(0),
            parts[1].parse::<u32>().unwrap_or(0),
            parts[2].parse::<u32>().unwrap_or(0),
        ),
        2 => (
            parts[0].parse::<u32>().unwrap_or(0),
            parts[1].parse::<u32>().unwrap_or(0),
            0,
        ),
        _ => (0, 0, 0),
    };

    Some(format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}"))
}

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
    /// 汇报线：["Alex", "Jordan (Director)", "Sam (CTO)"]
    pub reporting_line: Vec<String>,
    /// 主要工作语言
    pub primary_language: String,
    /// 次要工作语言
    pub secondary_language: String,
    /// LLM prompt 语言: "zh" | "en"
    #[serde(default = "default_prompt_lang")]
    pub prompt_language: String,
}

fn default_prompt_lang() -> String {
    "zh".into()
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
    pub role: String, // "user" | "sage"
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
    pub category: String, // "identity" | "values" | "behavior" | "thinking" | "emotion" | "growth"
    pub content: String,
    pub source: String,  // "chat" | "observation" | "feedback"
    pub confidence: f64, // 0.0-1.0
    /// 可见性层级：public（Digital Twin 可见）| private（仅 Alex）| subconscious（AI 推断）
    #[serde(default = "default_visibility")]
    pub visibility: String,
    pub created_at: String,
    pub updated_at: String,
    /// 记忆所描述的人（None = 关于 Alex 自己）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about_person: Option<String>,
    /// 上次被 Chat/Dashboard 访问的时间（use-it-or-lose-it 衰减）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<String>,
    /// 认知深度：episodic | semantic | procedural | axiom
    #[serde(default = "default_depth")]
    pub depth: String,
    /// 有效期（ISO-8601 字符串），None 表示永久有效
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    /// 被验证次数（用于提升 depth）
    #[serde(default)]
    pub validation_count: i64,
    /// 语义向量嵌入（f32 数组的 little-endian 字节序列），None 表示未生成
    #[serde(default, skip_serializing)]
    pub embedding: Option<Vec<u8>>,
    /// 溯源：这条记忆由哪些记忆演化而来（JSON 数组 "[12, 47]"）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<String>,
    /// 演化备注：为什么产生这条变更
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evolution_note: Option<String>,
    /// 可重新推导标记：1 = 该记忆是原始观察的直接重述，evolution 优先衰减
    #[serde(default)]
    pub derivable: i64,
}

fn default_visibility() -> String {
    "public".to_string()
}

fn default_depth() -> String {
    "episodic".to_string()
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
    pub from_type: String, // "memory" | "message" | "observation" | "question"
    pub from_id: i64,
    pub to_type: String, // "memory" | "message" | "observation" | "question"
    pub to_id: i64,
    pub relation: String, // "references" | "triggers" | "answers" | "supports" | "contradicts" | "co_occurred" | "causes" | "derived_from" | "similar"
    pub weight: f64,      // 0.0-1.0
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
    NeedsLogin,
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
    pub direction: String,    // "received" | "sent"
    pub action_state: String, // "pending" | "resolved" | "expired" | "info_only"
    pub resolved_at: Option<String>,
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

// ─── Message Source 模型（通用消息源配置）────────────────

/// 通用消息源配置（IMAP/SMTP/Slack/Teams 等）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSource {
    pub id: i64,
    pub label: String,        // "Work Gmail", "Personal Outlook"
    pub source_type: String,  // "imap", "exchange", "slack", ...
    pub config: String,       // JSON blob — type-specific config
    pub enabled: bool,
    pub created_at: String,
}

/// IMAP 类型的配置 JSON 结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapSourceConfig {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password_enc: String,  // base64(XOR obfuscated) — empty when using OAuth2
    pub use_tls: bool,
    pub email: String,         // display email address
    /// "password" | "oauth2" — default password for backward compat
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    /// OAuth2 provider: "microsoft" | "google"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_provider: Option<String>,
    /// OAuth2 client_id (user-provided or default)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    /// OAuth2 access token (encrypted)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_access_token: Option<String>,
    /// OAuth2 refresh token (encrypted)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_refresh_token: Option<String>,
    /// OAuth2 token expiry (ISO-8601)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_token_expires_at: Option<String>,
}

fn default_auth_type() -> String {
    "password".to_string()
}

/// 缓存的邮件消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    pub id: i64,
    pub source_id: i64,       // references message_sources.id
    pub uid: String,           // IMAP UID
    pub folder: String,
    pub from_addr: String,
    pub to_addr: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub is_read: bool,
    pub date: String,
    pub fetched_at: String,
}

// ─── Report Calibration 校准 ──────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportCorrection {
    pub id: i64,
    pub report_type: String,
    pub wrong_claim: String,
    pub correct_fact: String,
    pub context_hint: String,
    pub confidence: f64,
    pub applied_count: i64,
    pub created_at: String,
    pub superseded_at: Option<String>,
}
