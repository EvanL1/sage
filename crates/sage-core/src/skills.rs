//! Skill 加载层 — 支持热插拔
//!
//! 优先读取 `~/.sage/skills/{name}/SKILL.md`（用户覆盖），
//! 找不到则 fallback 到 `include_str!` 编译时内置版本。
//!
//! 各模块通过 `load_section()` 获取所需段落注入 LLM prompt。

use std::borrow::Cow;
use std::path::PathBuf;

/// sage-cognitive: 5 阶段认知循环 (Know/Observe/Reflect/Question/Care)
pub const COGNITIVE_SKILL: &str = include_str!("../../../skills/sage-cognitive/SKILL.md");

/// sage-week-rhythm: 周节奏框架 (Week Start/Daily Pulse/Week End/Growth)
pub const WEEK_RHYTHM_SKILL: &str = include_str!("../../../skills/sage-week-rhythm/SKILL.md");

// TODO(skill): sage-voice 和 sage-decision-journal 需要对话通道支持后再接入
#[allow(dead_code)]
pub const VOICE_SKILL: &str = include_str!("../../../skills/sage-voice/SKILL.md");

#[allow(dead_code)]
pub const DECISION_JOURNAL_SKILL: &str =
    include_str!("../../../skills/sage-decision-journal/SKILL.md");

/// sage-chat-strategist: 工作模式 — 专业策略顾问
pub const CHAT_STRATEGIST_SKILL: &str =
    include_str!("../../../skills/sage-chat-strategist/SKILL.md");

/// sage-chat-companion: 个人模式 — 有温度的倾听者
pub const CHAT_COMPANION_SKILL: &str = include_str!("../../../skills/sage-chat-companion/SKILL.md");

/// 用户 skill 覆盖目录（支持 SAGE_SKILLS_DIR 环境变量覆盖）
fn skills_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SAGE_SKILLS_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".sage/skills"))
}

/// 加载 skill 全文：优先用户覆盖文件，fallback 到编译时内置
fn load_skill(name: &str) -> Cow<'static, str> {
    if let Some(dir) = skills_dir() {
        let path = dir.join(name).join("SKILL.md");
        if let Ok(content) = std::fs::read_to_string(&path) {
            tracing::debug!("Skill override loaded: {path:?}");
            return Cow::Owned(content);
        }
    }
    Cow::Borrowed(bundled_skill(name))
}

/// 编译时内置 skill 名称映射
fn bundled_skill(name: &str) -> &'static str {
    match name {
        "sage-cognitive" => COGNITIVE_SKILL,
        "sage-week-rhythm" => WEEK_RHYTHM_SKILL,
        "sage-voice" => VOICE_SKILL,
        "sage-decision-journal" => DECISION_JOURNAL_SKILL,
        "sage-chat-strategist" => CHAT_STRATEGIST_SKILL,
        "sage-chat-companion" => CHAT_COMPANION_SKILL,
        _ => "",
    }
}

/// Chat skill 路由 — 根据消息内容选择合适的对话 skill
///
/// 返回 skill 名称（"sage-chat-strategist" 或 "sage-chat-companion"）
pub fn route_chat_skill(message: &str) -> &'static str {
    const COMPANION_KEYWORDS: &[&str] = &[
        // Chinese
        "焦虑",
        "迷茫",
        "情绪",
        "感受",
        "压力",
        "失眠",
        "孤独",
        "关系",
        "分手",
        "吵架",
        "家庭",
        "父母",
        "伴侣",
        "意义",
        "价值观",
        "人生",
        "自我",
        "身份",
        "活着",
        "害怕",
        "恐惧",
        "难过",
        "伤心",
        "委屈",
        "愤怒",
        "累了",
        "崩溃",
        "撑不住",
        "不想",
        "算了",
        "梦到",
        "小时候",
        "回忆",
        // English
        "anxious",
        "anxiety",
        "confused",
        "stressed",
        "emotional",
        "lonely",
        "sad",
        "angry",
        "frustrated",
        "overwhelmed",
        "burnout",
        "depressed",
        "worried",
        "scared",
        "afraid",
        "upset",
        "exhausted",
        "hopeless",
        "relationship",
        "breakup",
        "family",
        "meaning",
        "identity",
    ];

    let msg_lower = message.to_lowercase();
    for kw in COMPANION_KEYWORDS {
        if msg_lower.contains(kw) {
            return "sage-chat-companion";
        }
    }

    "sage-chat-strategist"
}

/// 检测消息是否为页面生成请求（触发词 + 页面名词同时存在）
pub fn is_page_gen_request(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    let triggers = [
        "帮我做", "给我做", "生成一个", "生成一份", "创建一个", "做一个",
        "make me", "create a", "build a", "generate a", "make a",
    ];
    let nouns = [
        "看板", "dashboard", "kanban", "报告", "report", "页面", "page",
        "视图", "view", "图表", "chart", "仪表盘", "工作台", "workbench",
        "计时器", "timer", "pomodoro",
    ];
    triggers.iter().any(|t| lower.contains(t)) && nouns.iter().any(|n| lower.contains(n))
}

/// 加载 chat skill 并替换模板变量
pub fn load_chat_skill(skill_name: &str, user_name: &str, layer: &str) -> String {
    let raw = load_skill(skill_name);

    // 去掉 YAML frontmatter
    let content = if let Some(stripped) = raw.strip_prefix("---") {
        if let Some(end) = stripped.find("---") {
            stripped[end + 3..].trim_start().to_string()
        } else {
            raw.to_string()
        }
    } else {
        raw.to_string()
    };

    // 替换模板变量
    let content = content.replace("{{user_name}}", user_name);

    // 提取层级对应段落
    let layer_heading = match layer {
        "safety" => "### 初识阶段",
        "patterns" => "### 模式识别阶段",
        _ => "### 深度伙伴阶段",
    };

    let layer_section = extract_section(&content, layer_heading).to_string();

    // 重新组装：skill 内容（去掉层级指导段落）+ 当前层级段落
    let base = if let Some(pos) = content.find("## 层级指导") {
        content[..pos].trim_end().to_string()
    } else {
        content
    };

    if layer_section.is_empty() {
        base
    } else {
        let section_body = layer_section.trim_start_matches(layer_heading).trim();
        format!("{}\n\n## 当前阶段\n{}", base, section_body)
    }
}

/// 从 skill 中提取指定段落（支持热插拔）
///
/// 例: `load_section("sage-cognitive", "## Phase 1: OBSERVE")`
pub fn load_section(skill_name: &str, heading: &str) -> String {
    let content = load_skill(skill_name);
    extract_section(&content, heading).to_string()
}

/// 从 SKILL.md 文本中提取指定标题到下一个同级标题之间的内容
pub fn extract_section<'a>(skill: &'a str, heading: &str) -> &'a str {
    let start = match skill.find(heading) {
        Some(i) => i,
        None => return "",
    };
    let level = heading.bytes().take_while(|&b| b == b'#').count();
    let content_start = start + heading.len();

    let mut end = skill.len();
    let mut pos = content_start;
    let mut first_line = true;

    for line in skill[content_start..].lines() {
        if first_line {
            first_line = false;
            pos += line.len() + 1;
            continue;
        }
        let h = line.bytes().take_while(|&b| b == b'#').count();
        if h > 0 && h <= level && line.as_bytes().get(h) == Some(&b' ') {
            end = pos;
            break;
        }
        pos += line.len() + 1;
    }

    skill[start..end].trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_include_str_loaded() {
        assert!(COGNITIVE_SKILL.contains("Phase 1: OBSERVE"));
        assert!(WEEK_RHYTHM_SKILL.contains("Week Start"));
        assert!(VOICE_SKILL.contains("sage-voice"));
        assert!(DECISION_JOURNAL_SKILL.contains("sage-decision-journal"));
    }

    #[test]
    fn test_extract_section_basic() {
        let md = "# Title\n\n## Section A\nContent A\n\n## Section B\nContent B\n";
        let result = extract_section(md, "## Section A");
        assert!(result.starts_with("## Section A"));
        assert!(result.contains("Content A"));
        assert!(!result.contains("Section B"));
    }

    #[test]
    fn test_extract_section_last() {
        let md = "## First\nAAA\n\n## Last\nBBB\n";
        let result = extract_section(md, "## Last");
        assert!(result.contains("BBB"));
    }

    #[test]
    fn test_extract_section_not_found() {
        let md = "## Foo\nBar\n";
        assert_eq!(extract_section(md, "## Missing"), "");
    }

    #[test]
    fn test_extract_cognitive_phase1() {
        let section = extract_section(COGNITIVE_SKILL, "## Phase 1: OBSERVE");
        assert!(!section.is_empty());
        assert!(section.contains("Coach"));
        assert!(!section.contains("## Phase 2"));
    }

    #[test]
    fn test_extract_week_start() {
        let section = extract_section(WEEK_RHYTHM_SKILL, "## Week Start (Monday)");
        assert!(!section.is_empty());
        assert!(section.contains("calibration"));
        assert!(!section.contains("## Daily Pulse"));
    }

    #[test]
    fn test_subsection_not_break_parent() {
        // ### 子标题不应截断 ## 段落
        let md = "## Parent\n\n### Child A\nAAA\n\n### Child B\nBBB\n\n## Next\nCCC\n";
        let result = extract_section(md, "## Parent");
        assert!(result.contains("Child A"));
        assert!(result.contains("Child B"));
        assert!(!result.contains("## Next"));
    }

    #[test]
    fn test_bundled_skill_mapping() {
        assert!(!bundled_skill("sage-cognitive").is_empty());
        assert!(!bundled_skill("sage-week-rhythm").is_empty());
        assert!(!bundled_skill("sage-voice").is_empty());
        assert!(!bundled_skill("sage-decision-journal").is_empty());
        assert!(!bundled_skill("sage-chat-strategist").is_empty());
        assert!(!bundled_skill("sage-chat-companion").is_empty());
        assert!(bundled_skill("nonexistent").is_empty());
    }

    #[test]
    fn test_chat_skill_include_str() {
        assert!(CHAT_STRATEGIST_SKILL.contains("Strategist"));
        assert!(CHAT_COMPANION_SKILL.contains("Companion"));
    }

    #[test]
    fn test_route_chat_skill_default_is_strategist() {
        assert_eq!(
            route_chat_skill("明天的会议怎么准备"),
            "sage-chat-strategist"
        );
        assert_eq!(route_chat_skill("OKR 怎么写"), "sage-chat-strategist");
        assert_eq!(route_chat_skill("帮我分析一下"), "sage-chat-strategist");
        assert_eq!(route_chat_skill("你好"), "sage-chat-strategist");
    }

    #[test]
    fn test_route_chat_skill_personal_keywords() {
        assert_eq!(route_chat_skill("最近有点焦虑"), "sage-chat-companion");
        assert_eq!(route_chat_skill("我和女朋友吵架了"), "sage-chat-companion");
        assert_eq!(route_chat_skill("感觉很迷茫"), "sage-chat-companion");
        assert_eq!(route_chat_skill("和父母的关系"), "sage-chat-companion");
        assert_eq!(route_chat_skill("人生的意义是什么"), "sage-chat-companion");
    }

    #[test]
    fn test_load_chat_skill_strips_frontmatter() {
        let result = load_chat_skill("sage-chat-strategist", "Alex", "safety");
        assert!(!result.contains("---"));
        assert!(!result.contains("name: sage-chat-strategist"));
        assert!(result.contains("Alex"));
    }

    #[test]
    fn test_load_chat_skill_replaces_user_name() {
        let result = load_chat_skill("sage-chat-companion", "TestUser", "deep");
        assert!(result.contains("TestUser"));
        assert!(!result.contains("{{user_name}}"));
    }

    #[test]
    fn test_load_chat_skill_extracts_layer() {
        let safety = load_chat_skill("sage-chat-strategist", "X", "safety");
        assert!(safety.contains("当前阶段"));
        assert!(!safety.contains("## 层级指导"));

        let deep = load_chat_skill("sage-chat-strategist", "X", "deep");
        assert!(deep.contains("当前阶段"));
    }

    #[test]
    fn test_load_section_fallback_to_bundled() {
        // 没有覆盖文件时，load_section 应返回编译时内置内容
        let section = load_section("sage-cognitive", "## Phase 1: OBSERVE");
        assert!(!section.is_empty());
        assert!(section.contains("Coach"));
    }

    #[test]
    fn test_load_section_unknown_skill() {
        let section = load_section("nonexistent", "## Foo");
        assert!(section.is_empty());
    }

    #[test]
    fn test_load_skill_override() {
        // 用 tempdir + SAGE_SKILLS_DIR 验证热加载
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "## Custom\nOverridden content\n",
        )
        .unwrap();

        std::env::set_var("SAGE_SKILLS_DIR", dir.path());
        let content = load_skill("test-skill");
        std::env::remove_var("SAGE_SKILLS_DIR");

        assert!(content.contains("Overridden content"));
    }
}
