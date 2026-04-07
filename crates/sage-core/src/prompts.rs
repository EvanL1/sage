//! Bilingual LLM prompts — texts in `prompts/{en,zh}/*.md`, compiled via `include_str!()`.
//!
//! Users can override any prompt by placing a file at `~/.sage/prompts/{lang}/{name}.md`.

use std::borrow::Cow;

// ─── Prompt Registry ──────────────────────────────────────────────────────────

macro_rules! define_prompts {
    (
        bilingual: [$($bi:literal),* $(,)?],
        en_only: [$($en:literal),* $(,)?]
    ) => {
        fn builtin(name: &str, lang: &str) -> &'static str {
            let l = if lang == "en" { "en" } else { "zh" };
            match (l, name) {
                $(
                    ("en", $bi) => include_str!(concat!("../prompts/en/", $bi, ".md")),
                    ("zh", $bi) => include_str!(concat!("../prompts/zh/", $bi, ".md")),
                )*
                $(
                    (_, $en) => include_str!(concat!("../prompts/en/", $en, ".md")),
                )*
                _ => "",
            }
        }
    };
}

define_prompts! {
    bilingual: [
        "reconciler_system", "reconciler_incremental", "reconciler_full",
        "persona_intro", "persona_rules", "persona_context_header",
        "cmd_first_impression_user", "cmd_first_impression_system",
        "cmd_extract_memories_user", "cmd_extract_memories_system",
        "cmd_import_ai_memory_user",
        "cmd_analyze_message_flow_user", "cmd_analyze_message_flow_system",
        "cmd_summarize_channel", "cmd_dashboard_brief_system",
        "cmd_task_extraction_system", "cmd_verification_system",
        "feed_filter", "feed_deep_read", "feed_deep_note", "feed_digest_system", "feed_digest_user",
        "chat_memory_write_protocol", "chat_safety_protocol",
        "page_gen_system",
        "person_extract",
        "memory_integrator",
        "task_intelligence_system",
        "task_intelligence_user",
        "cmd_verification_user",
    ],
    en_only: []
}

/// Load a prompt with user-override support.
/// Checks `~/.sage/prompts/{lang}/{name}.md` first, falls back to compiled-in default.
pub fn load_prompt(name: &str, lang: &str) -> Cow<'static, str> {
    if let Some(text) = load_user_override(name, lang) {
        Cow::Owned(text)
    } else {
        Cow::Borrowed(builtin(name, lang))
    }
}

fn load_user_override(name: &str, lang: &str) -> Option<String> {
    if name.contains('/') || name.contains('\\') || name.starts_with('.') {
        return None;
    }
    let l = if lang == "en" { "en" } else { "zh" };
    let home = std::env::var("HOME").ok()?;
    let path = std::path::Path::new(&home)
        .join(".sage")
        .join("prompts")
        .join(l)
        .join(format!("{name}.md"));
    std::fs::read_to_string(path).ok()
}

/// Shorthand: static builtin (no override).
#[inline]
fn s(name: &str, lang: &str) -> &'static str {
    builtin(name, lang)
}

/// Shorthand: with user override.
#[inline]
fn p(name: &str, lang: &str) -> Cow<'static, str> {
    load_prompt(name, lang)
}

// ─── Reconciler ───────────────────────────────────────────────────────────────

pub fn reconciler_system(lang: &str) -> &'static str { s("reconciler_system", lang) }

pub fn reconciler_incremental(lang: &str, new_content: &str, items_text: &str) -> String {
    p("reconciler_incremental", lang)
        .replace("{new_content}", new_content)
        .replace("{items_text}", items_text)
}

pub fn reconciler_full(lang: &str, items_text: &str) -> String {
    p("reconciler_full", lang).replace("{items_text}", items_text)
}

// ─── Memory Integrator ────────────────────────────────────────────────────────

pub fn memory_integrator_template(lang: &str) -> &'static str { s("memory_integrator", lang) }

// ─── Persona ──────────────────────────────────────────────────────────────────

pub fn persona_intro(lang: &str, name: &str) -> String {
    p("persona_intro", lang).replace("{name}", name)
}

pub fn persona_rules(lang: &str) -> &'static str { s("persona_rules", lang) }
pub fn persona_context_header(lang: &str) -> &'static str { s("persona_context_header", lang) }

// ─── Person Extract ──────────────────────────────────────────────────────────

pub fn person_extract(lang: &str, events: &str) -> String {
    p("person_extract", lang).replace("{events}", events)
}

// ─── Task Intelligence ────────────────────────────────────────────────────────

pub fn task_intelligence_system(lang: &str) -> &'static str { s("task_intelligence_system", lang) }

pub fn task_intelligence_user_template(lang: &str) -> &'static str { s("task_intelligence_user", lang) }

// ─── Commands ─────────────────────────────────────────────────────────────────

pub fn cmd_first_impression_user(lang: &str, profile_summary: &str) -> String {
    p("cmd_first_impression_user", lang).replace("{profile_summary}", profile_summary)
}

pub fn cmd_first_impression_system(lang: &str) -> &'static str {
    s("cmd_first_impression_system", lang)
}

pub fn cmd_extract_memories_user(lang: &str, existing_text: &str, conversation: &str) -> String {
    p("cmd_extract_memories_user", lang)
        .replace("{existing_text}", existing_text)
        .replace("{conversation}", conversation)
}

pub fn cmd_extract_memories_system(lang: &str) -> &'static str {
    s("cmd_extract_memories_system", lang)
}

pub fn cmd_import_ai_memory_user(lang: &str, text: &str) -> String {
    p("cmd_import_ai_memory_user", lang).replace("{text}", text)
}

pub fn cmd_analyze_message_flow_user(lang: &str, label: &str, context: &str) -> String {
    p("cmd_analyze_message_flow_user", lang)
        .replace("{label}", label)
        .replace("{context}", context)
}

pub fn cmd_analyze_message_flow_system(lang: &str) -> &'static str {
    s("cmd_analyze_message_flow_system", lang)
}

/// Channel summary — runtime `type_label` logic stays in Rust.
pub fn cmd_summarize_channel_prompt(
    lang: &str,
    channel: &str,
    chat_type: &str,
    messages_text: &str,
) -> String {
    let type_label = match (lang, chat_type) {
        ("en", "group") => "group chat",
        ("en", "channel") => "team channel",
        ("en", "p2p") => "direct message",
        ("en", _) => "conversation",
        (_, "group") => "群聊",
        (_, "channel") => "团队频道",
        (_, "p2p") => "私聊",
        (_, _) => "对话",
    };
    p("cmd_summarize_channel", lang)
        .replace("{type_label}", type_label)
        .replace("{channel}", channel)
        .replace("{messages_text}", messages_text)
}

pub fn cmd_dashboard_brief_system(lang: &str, user_name: &str) -> String {
    p("cmd_dashboard_brief_system", lang).replace("{user_name}", user_name)
}

pub fn cmd_task_extraction_system(lang: &str, today: &str) -> String {
    p("cmd_task_extraction_system", lang).replace("{today}", today)
}

pub fn cmd_verification_system(lang: &str) -> &'static str { s("cmd_verification_system", lang) }

pub fn cmd_verification_user(lang: &str, task_content: &str) -> String {
    p("cmd_verification_user", lang).replace("{task_content}", task_content)
}

// ─── Feed ─────────────────────────────────────────────────────────────────────

pub fn feed_filter_prompt(
    lang: &str,
    interests_line: &str,
    personality_section: &str,
    listing: &str,
) -> String {
    p("feed_filter", lang)
        .replace("{interests_line}", interests_line)
        .replace("{personality_section}", personality_section)
        .replace("{listing}", listing)
}

/// Feed deep-read — conditional `personality_line` construction stays in Rust.
pub fn feed_deep_read_prompt(
    lang: &str,
    sentence_count: &str,
    personality: &str,
    project_section: &str,
    truncated: &str,
) -> String {
    let personality_line = if personality.trim().is_empty() {
        String::new()
    } else {
        match lang {
            "en" => format!("User profile: {personality}\n"),
            _ => format!("用户画像：{personality}\n"),
        }
    };
    p("feed_deep_read", lang)
        .replace("{personality_line}", &personality_line)
        .replace("{project_section}", project_section)
        .replace("{sentence_count}", sentence_count)
        .replace("{truncated}", truncated)
}

/// 深度阅读笔记 prompt
pub fn feed_deep_note_prompt(lang: &str, title: &str, content: &str) -> String {
    p("feed_deep_note", lang)
        .replace("{title}", title)
        .replace("{content}", content)
}

pub fn feed_digest_system(lang: &str) -> &'static str { s("feed_digest_system", lang) }

pub fn feed_digest_user(lang: &str, items_text: &str) -> String {
    p("feed_digest_user", lang).replace("{items_text}", items_text)
}

// ─── Chat ─────────────────────────────────────────────────────────────────────

pub fn chat_memory_write_protocol(lang: &str) -> &'static str {
    s("chat_memory_write_protocol", lang)
}

pub fn chat_safety_protocol(lang: &str) -> &'static str { s("chat_safety_protocol", lang) }

// ─── Page Generation ──────────────────────────────────────────────────────────

pub fn page_gen_system(lang: &str) -> &'static str { s("page_gen_system", lang) }
