//! 反思信号检测器 / Reflective Signal Detector
//!
//! 规则引擎，扫描文本中的反思/脆弱/矛盾信号。零 LLM 开销。
//! Rule-based engine that scans text for reflective/vulnerable/contradictory signals.
//! Zero LLM cost — runs on every text ingestion.

/// 检测到的反思信号
#[derive(Debug, Clone)]
pub struct DetectedSignal {
    pub signal_type: String,
    pub raw_text: String,
    pub intensity: f64,
    pub armor_pattern: Option<String>,
}

/// 扫描文本，返回检测到的反思信号（去重，保留最高 intensity）
pub fn scan(text: &str, _source: &str) -> Vec<DetectedSignal> {
    let mut signals = Vec::new();
    signals.extend(detect_uncertainty(text));
    signals.extend(detect_contradiction(text));
    signals.extend(detect_vulnerability(text));
    signals.extend(detect_defensive_abstraction(text));
    signals.extend(detect_blocked_state(text));
    signals.extend(detect_self_analysis(text));
    signals.extend(detect_divergence(text));
    dedup_by_type(signals)
}

/// 保留每种 signal_type 中 intensity 最高的
fn dedup_by_type(mut signals: Vec<DetectedSignal>) -> Vec<DetectedSignal> {
    signals.sort_by(|a, b| a.signal_type.cmp(&b.signal_type));
    let mut result: Vec<DetectedSignal> = Vec::new();
    for sig in signals {
        if let Some(last) = result.last_mut() {
            if last.signal_type == sig.signal_type {
                if sig.intensity > last.intensity {
                    *last = sig;
                }
                continue;
            }
        }
        result.push(sig);
    }
    result
}

// ─── 不确定性 / Uncertainty ─────────────────────────────────────────────

const UNCERTAINTY_ZH: &[&str] = &[
    "如果", "也许", "可能", "不确定", "不太确定", "说不定", "大概",
];
const UNCERTAINTY_EN: &[&str] = &[
    "if i ", "if we ", "maybe ", "perhaps ", "not sure", "i think ",
    "i guess ", "probably ", "might be", "could be",
];
const HEDGED_ZH: &[&str] = &["还不够", "不太", "半成品", "还差", "不太行"];
const HEDGED_EN: &[&str] = &["not ready", "not good enough", "half-baked", "not quite"];

fn detect_uncertainty(text: &str) -> Vec<DetectedSignal> {
    let lower = text.to_lowercase();
    let mut hits = 0u32;
    let mut matched = String::new();

    for kw in UNCERTAINTY_ZH.iter().chain(UNCERTAINTY_EN.iter()) {
        if lower.contains(kw) {
            hits += 1;
            if matched.is_empty() {
                matched = extract_around(&lower, kw, 40);
            }
        }
    }
    for kw in HEDGED_ZH.iter().chain(HEDGED_EN.iter()) {
        if lower.contains(kw) {
            hits += 1;
            if matched.is_empty() {
                matched = extract_around(&lower, kw, 40);
            }
        }
    }

    if hits == 0 {
        return vec![];
    }
    let intensity = (hits as f64 * 0.25).min(1.0);
    vec![DetectedSignal {
        signal_type: "uncertainty".into(),
        raw_text: matched,
        intensity,
        armor_pattern: Some("decisive_action".into()),
    }]
}

// ─── 自我矛盾 / Contradiction ──────────────────────────────────────────

const CONTRADICTION_ZH: &[&str] = &["但是", "其实", "矛盾", "反过来说", "话又说回来"];
const CONTRADICTION_EN: &[&str] = &[
    "however", "but actually", "contradicts", "on the other hand",
    "then again", "actually no",
];

fn detect_contradiction(text: &str) -> Vec<DetectedSignal> {
    let lower = text.to_lowercase();
    let mut hits = 0u32;
    let mut matched = String::new();

    for kw in CONTRADICTION_ZH.iter().chain(CONTRADICTION_EN.iter()) {
        if lower.contains(kw) {
            hits += 1;
            if matched.is_empty() {
                matched = extract_around(&lower, kw, 40);
            }
        }
    }
    // 至少 2 个矛盾标记才触发（单个"但是"太常见）
    if hits < 2 {
        return vec![];
    }
    vec![DetectedSignal {
        signal_type: "contradiction".into(),
        raw_text: matched,
        intensity: (hits as f64 * 0.3).min(1.0),
        armor_pattern: Some("consistent_framework".into()),
    }]
}

// ─── 脆弱性 / Vulnerability ────────────────────────────────────────────

const VULNERABILITY_ZH: &[&str] = &[
    "说实话", "承认", "其实很", "坦白说", "不得不承认",
    "我很难", "我做不到", "害怕", "焦虑", "不安",
];
const VULNERABILITY_EN: &[&str] = &[
    "honestly", "i have to admit", "to be honest", "feels hard",
    "i can't", "i'm afraid", "anxious", "vulnerable", "scared",
    "struggling with",
];

fn detect_vulnerability(text: &str) -> Vec<DetectedSignal> {
    let lower = text.to_lowercase();
    let mut hits = 0u32;
    let mut matched = String::new();

    for kw in VULNERABILITY_ZH.iter().chain(VULNERABILITY_EN.iter()) {
        if lower.contains(kw) {
            hits += 1;
            if matched.is_empty() {
                matched = extract_around(&lower, kw, 40);
            }
        }
    }
    if hits == 0 {
        return vec![];
    }
    vec![DetectedSignal {
        signal_type: "vulnerability".into(),
        raw_text: matched,
        intensity: (hits as f64 * 0.35).min(1.0),
        armor_pattern: Some("emotional_control".into()),
    }]
}

// ─── 防御性抽象 / Defensive Abstraction ─────────────────────────────────

const ABSTRACT_ZH: &[&str] = &["从哲学", "系统性", "框架", "结构性", "本质上", "抽象地说"];
const ABSTRACT_EN: &[&str] = &[
    "framework", "systemic", "structurally", "philosophically",
    "fundamentally", "in abstract",
];

fn detect_defensive_abstraction(text: &str) -> Vec<DetectedSignal> {
    // 仅对较长文本检测（短文本用抽象词是正常的）
    if text.chars().count() < 200 {
        return vec![];
    }
    let lower = text.to_lowercase();
    let mut hits = 0u32;
    for kw in ABSTRACT_ZH.iter().chain(ABSTRACT_EN.iter()) {
        if lower.contains(kw) {
            hits += 1;
        }
    }
    // 高密度抽象词（≥3 个）才触发
    if hits < 3 {
        return vec![];
    }
    let matched = text.chars().take(80).collect::<String>() + "…";
    vec![DetectedSignal {
        signal_type: "defensive_abstraction".into(),
        raw_text: matched,
        intensity: (hits as f64 * 0.2).min(1.0),
        armor_pattern: Some("intellectualization".into()),
    }]
}

// ─── 卡住/等待 / Blocked State ──────────────────────────────────────────

const BLOCKED_ZH: &[&str] = &[
    "一直在等", "没有进展", "卡住了", "没进展", "被阻塞",
    "在等", "等审批", "取决于",
];
const BLOCKED_EN: &[&str] = &[
    "stuck", "blocked", "waiting for", "no progress", "depends on",
    "stalled", "on hold",
];

fn detect_blocked_state(text: &str) -> Vec<DetectedSignal> {
    let lower = text.to_lowercase();
    let mut hits = 0u32;
    let mut matched = String::new();

    for kw in BLOCKED_ZH.iter().chain(BLOCKED_EN.iter()) {
        if lower.contains(kw) {
            hits += 1;
            if matched.is_empty() {
                matched = extract_around(&lower, kw, 40);
            }
        }
    }
    if hits == 0 {
        return vec![];
    }
    vec![DetectedSignal {
        signal_type: "blocked_state".into(),
        raw_text: matched,
        intensity: (hits as f64 * 0.3).min(1.0),
        armor_pattern: Some("proactive_resolution".into()),
    }]
}

// ─── 自我分析 / Self-Analysis ───────────────────────────────────────────

const SELF_ANALYSIS_ZH: &[&str] = &[
    "我发现自己", "我意识到", "我的模式", "我注意到自己",
    "回头看", "反思一下", "我的习惯",
];
const SELF_ANALYSIS_EN: &[&str] = &[
    "i notice i", "i realize i", "my pattern", "looking back",
    "i tend to", "reflecting on", "my habit",
];

fn detect_self_analysis(text: &str) -> Vec<DetectedSignal> {
    let lower = text.to_lowercase();
    let mut hits = 0u32;
    let mut matched = String::new();

    for kw in SELF_ANALYSIS_ZH.iter().chain(SELF_ANALYSIS_EN.iter()) {
        if lower.contains(kw) {
            hits += 1;
            if matched.is_empty() {
                matched = extract_around(&lower, kw, 40);
            }
        }
    }
    if hits == 0 {
        return vec![];
    }
    vec![DetectedSignal {
        signal_type: "self_analysis".into(),
        raw_text: matched,
        intensity: (hits as f64 * 0.3).min(1.0),
        armor_pattern: None,
    }]
}

// ─── 基线偏离 / Divergence from Baseline ────────────────────────────────

fn detect_divergence(text: &str) -> Vec<DetectedSignal> {
    let char_count = text.chars().count();
    // 单轮 >800 字 = 偏离"确认了就切下一个"基线
    if char_count < 800 {
        return vec![];
    }
    let intensity = ((char_count as f64 - 800.0) / 1000.0).clamp(0.3, 1.0);
    vec![DetectedSignal {
        signal_type: "divergence_from_baseline".into(),
        raw_text: format!(
            "[{} chars] {}…",
            char_count,
            text.chars().take(60).collect::<String>()
        ),
        intensity,
        armor_pattern: Some("concise_communication".into()),
    }]
}

// ─── 工具 / Helpers ─────────────────────────────────────────────────────

/// 提取关键词附近的文本片段
fn extract_around(text: &str, keyword: &str, radius: usize) -> String {
    if let Some(pos) = text.find(keyword) {
        let start = text[..pos]
            .char_indices()
            .rev()
            .nth(radius)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let end_offset = pos + keyword.len();
        let end = text[end_offset..]
            .char_indices()
            .nth(radius)
            .map(|(i, _)| end_offset + i)
            .unwrap_or(text.len());
        text[start..end].to_string()
    } else {
        text.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── uncertainty ──
    #[test]
    fn detects_uncertainty_zh() {
        let signals = scan("如果我能更勇敢一点，也许结果会不同", "chat");
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].signal_type, "uncertainty");
        assert!(signals[0].intensity > 0.0);
    }

    #[test]
    fn no_uncertainty_in_neutral() {
        let signals = scan("今天完成了三个 PR review", "chat");
        assert!(signals.is_empty());
    }

    // ── contradiction ──
    #[test]
    fn detects_contradiction() {
        let signals = scan("我说了要果断决策，但是其实我一直在犹豫，话又说回来也没什么", "chat");
        let c = signals.iter().find(|s| s.signal_type == "contradiction");
        assert!(c.is_some());
    }

    #[test]
    fn no_contradiction_single_marker() {
        // 单个"但是"不触发（太常见）
        let signals = scan("我喜欢咖啡但是今天喝了茶", "chat");
        let c = signals.iter().find(|s| s.signal_type == "contradiction");
        assert!(c.is_none());
    }

    // ── vulnerability ──
    #[test]
    fn detects_vulnerability_zh() {
        let signals = scan("说实话我很焦虑，害怕做错选择", "chat");
        let v = signals.iter().find(|s| s.signal_type == "vulnerability");
        assert!(v.is_some());
    }

    #[test]
    fn no_vulnerability_in_report() {
        let signals = scan("Q3 revenue target achieved, team delivered on time", "chat");
        let v = signals.iter().find(|s| s.signal_type == "vulnerability");
        assert!(v.is_none());
    }

    // ── defensive abstraction ──
    #[test]
    fn detects_defensive_abstraction() {
        let long_text = "从哲学角度看，这个问题的系统性本质上是一个框架选择问题。\
            我们需要考虑结构性的方案，而不是简单的补丁。这涉及到底层的架构设计哲学，\
            以及我们如何看待技术债务这个概念本身。从哲学上说，技术债务是一种隐喻，\
            它暗示了某种道德判断——有些代码是好的，有些是坏的。但这种二元框架\
            其实忽略了上下文。系统性地看，每个决定都是当时约束条件下的最优解。\
            从结构性的视角来看，我们更应该关注系统整体的一致性，而不是个别模块的完美。\
            这才是真正的框架思维——把问题放在更大的系统性背景下去理解。";
        let signals = scan(long_text, "chat");
        let d = signals.iter().find(|s| s.signal_type == "defensive_abstraction");
        assert!(d.is_some());
    }

    #[test]
    fn no_defensive_abstraction_short() {
        let signals = scan("这个框架的系统性设计不错", "chat");
        let d = signals.iter().find(|s| s.signal_type == "defensive_abstraction");
        assert!(d.is_none());
    }

    // ── blocked state ──
    #[test]
    fn detects_blocked_state() {
        let signals = scan("一直在等审批，没有进展，项目卡住了", "chat");
        let b = signals.iter().find(|s| s.signal_type == "blocked_state");
        assert!(b.is_some());
    }

    #[test]
    fn no_blocked_in_action() {
        let signals = scan("已完成部署，正在验证", "chat");
        let b = signals.iter().find(|s| s.signal_type == "blocked_state");
        assert!(b.is_none());
    }

    // ── self analysis ──
    #[test]
    fn detects_self_analysis() {
        let signals = scan("我发现自己总是在回避这类对话，我意识到这是一种模式", "chat");
        let s = signals.iter().find(|s| s.signal_type == "self_analysis");
        assert!(s.is_some());
    }

    #[test]
    fn no_self_analysis_in_neutral() {
        let signals = scan("发现 bug 在第 42 行，已修复", "chat");
        let s = signals.iter().find(|s| s.signal_type == "self_analysis");
        assert!(s.is_none());
    }

    // ── divergence ──
    #[test]
    fn detects_divergence_long_text() {
        let long = "a".repeat(1000);
        let signals = scan(&long, "chat");
        let d = signals.iter().find(|s| s.signal_type == "divergence_from_baseline");
        assert!(d.is_some());
    }

    #[test]
    fn no_divergence_short_text() {
        let signals = scan("short message", "chat");
        let d = signals.iter().find(|s| s.signal_type == "divergence_from_baseline");
        assert!(d.is_none());
    }

    // ── dedup ──
    #[test]
    fn dedup_keeps_highest_intensity() {
        let signals = vec![
            DetectedSignal { signal_type: "uncertainty".into(), raw_text: "low".into(), intensity: 0.3, armor_pattern: None },
            DetectedSignal { signal_type: "uncertainty".into(), raw_text: "high".into(), intensity: 0.8, armor_pattern: None },
        ];
        let result = dedup_by_type(signals);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].raw_text, "high");
    }
}
