use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use crate::prompts;
use crate::provider::LlmProvider;
use crate::store::Store;

/// Digital Twin 人格引擎 — 只读 public 记忆，只回答外部询问
pub struct Persona {
    store: Arc<Store>,
}

impl Persona {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// 从 public memories + UserProfile 构建数字分身的 system prompt
    pub fn build_system_prompt(&self) -> Result<String> {
        let memories = self.store.get_memories_by_visibility("public")?;
        let lang = self.store.prompt_lang();
        let name = self
            .store
            .load_profile()?
            .filter(|p| !p.identity.name.is_empty())
            .map(|p| p.identity.name)
            .unwrap_or_else(|| match lang.as_str() {
                "en" => "User".to_string(),
                _ => "用户".to_string(),
            });

        // 按 category 分组
        let mut groups: HashMap<&str, Vec<String>> = HashMap::new();
        for mem in &memories {
            groups
                .entry(category_section(mem.category.as_str()))
                .or_default()
                .push(mem.content.clone());
        }

        let mut prompt = prompts::persona_intro(&lang, &name);

        // 按固定顺序输出各分组
        for key in SECTION_KEYS {
            if let Some(items) = groups.get(key) {
                let heading = section_header(key, &lang);
                prompt.push_str(&format!("## {heading}\n"));
                for item in items {
                    prompt.push_str(&format!("- {item}\n"));
                }
                prompt.push('\n');
            }
        }

        prompt.push_str(prompts::persona_rules(&lang));

        Ok(prompt)
    }

    /// 外部对话接口 — 只读模式，不写任何记忆
    pub async fn chat(&self, user_message: &str, provider: &dyn LlmProvider) -> Result<String> {
        let system_prompt = self.build_system_prompt()?;

        // 检索相关 public 记忆作为额外上下文
        let relevant = self.store.search_public_memories(user_message, 15)?;
        let lang = self.store.prompt_lang();
        let context = if relevant.is_empty() {
            String::new()
        } else {
            let items: Vec<String> = relevant
                .iter()
                .map(|m| format!("[{}] {}", m.category, m.content))
                .collect();
            format!(
                "{}{}",
                prompts::persona_context_header(&lang),
                items.join("\n")
            )
        };

        let full_system = format!("{system_prompt}{context}");
        let reply = provider.invoke(user_message, Some(&full_system)).await?;
        Ok(reply)
    }
}

/// 将 memory category 映射到 system prompt 分区 key
fn category_section(category: &str) -> &'static str {
    match category {
        "identity" => "identity",
        "personality" => "personality",
        "values" => "values",
        "behavior" => "behavior",
        "thinking" | "thinking_style" => "thinking",
        "growth" => "growth",
        "decision" => "decision",
        "pattern" | "coach_insight" => "pattern",
        _ => "other",
    }
}

/// Section keys in display order
const SECTION_KEYS: &[&str] = &[
    "identity",
    "personality",
    "values",
    "behavior",
    "thinking",
    "growth",
    "decision",
    "pattern",
    "other",
];

/// Language-aware section heading for a given key
fn section_header(key: &str, lang: &str) -> &'static str {
    match (key, lang) {
        ("identity", "en") => "About Me",
        ("identity", _) => "关于我",
        ("personality", "en") => "My Personality",
        ("personality", _) => "我的性格",
        ("values", "en") => "My Values",
        ("values", _) => "我的价值观",
        ("behavior", "en") => "Behavior Patterns",
        ("behavior", _) => "行为模式",
        ("thinking", "en") => "How I Think",
        ("thinking", _) => "思维方式",
        ("growth", "en") => "Growth Focus",
        ("growth", _) => "成长方向",
        ("decision", "en") => "Recent Decisions",
        ("decision", _) => "近期决策",
        ("pattern", "en") => "Observed Patterns",
        ("pattern", _) => "规律总结",
        ("other", "en") => "Other",
        _ => "其他",
    }
}

// ─── 测试 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn store_with_memories() -> Arc<Store> {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .save_memory_with_visibility(
                "identity",
                "EMS 产品负责人 @ Acme Corp",
                "test",
                0.9,
                "public",
            )
            .unwrap();
        store
            .save_memory_with_visibility(
                "values",
                "重视系统思考胜过局部优化",
                "test",
                0.8,
                "public",
            )
            .unwrap();
        // private 记忆不应出现在 system prompt
        store
            .save_memory_with_visibility("emotion", "有时感到工作压力很大", "test", 0.5, "private")
            .unwrap();
        store
    }

    #[test]
    fn test_build_system_prompt_includes_public_only() {
        let store = store_with_memories();
        let persona = Persona::new(Arc::clone(&store));
        let prompt = persona.build_system_prompt().unwrap();

        assert!(
            prompt.contains("EMS 产品负责人"),
            "应包含 identity 公开记忆"
        );
        assert!(prompt.contains("重视系统思考"), "应包含 values 公开记忆");
        assert!(!prompt.contains("工作压力"), "不应包含 private 记忆");
    }

    #[test]
    fn test_build_system_prompt_empty_memories() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let persona = Persona::new(store);
        let prompt = persona.build_system_prompt().unwrap();

        assert!(prompt.contains("的数字分身"), "应包含 header");
        assert!(prompt.contains("重要规则"), "应包含规则部分");
    }

    #[test]
    fn test_build_system_prompt_category_grouping() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .save_memory_with_visibility(
                "identity",
                "上交大毕业，曾在小米工作",
                "test",
                0.9,
                "public",
            )
            .unwrap();
        store
            .save_memory_with_visibility("personality", "行动先于准备", "test", 0.8, "public")
            .unwrap();
        store
            .save_memory_with_visibility(
                "values",
                "长期愿景：AI 驱动能源决策",
                "test",
                0.7,
                "public",
            )
            .unwrap();

        let persona = Persona::new(store);
        let prompt = persona.build_system_prompt().unwrap();

        let identity_pos = prompt.find("## 关于我").unwrap();
        let personality_pos = prompt.find("## 我的性格").unwrap();
        let values_pos = prompt.find("## 我的价值观").unwrap();
        assert!(
            identity_pos < personality_pos,
            "identity 应在 personality 之前"
        );
        assert!(personality_pos < values_pos, "personality 应在 values 之前");

        assert!(prompt.contains("上交大毕业"), "identity 内容应在对应分区");
        assert!(
            prompt.contains("行动先于准备"),
            "personality 内容应在对应分区"
        );
        assert!(
            prompt.contains("AI 驱动能源决策"),
            "values 内容应在对应分区"
        );
    }

    #[test]
    fn test_section_header_bilingual() {
        assert_eq!(section_header("identity", "en"), "About Me");
        assert_eq!(section_header("identity", "zh"), "关于我");
        assert_eq!(section_header("personality", "en"), "My Personality");
        assert_eq!(section_header("personality", "zh"), "我的性格");
        assert_eq!(section_header("values", "en"), "My Values");
        assert_eq!(section_header("pattern", "en"), "Observed Patterns");
        assert_eq!(section_header("pattern", "zh"), "规律总结");
        assert_eq!(section_header("other", "en"), "Other");
        assert_eq!(section_header("other", "zh"), "其他");
    }
}
