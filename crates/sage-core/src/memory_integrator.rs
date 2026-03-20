//! LLM-mediated memory integration.
//!
//! Every incoming memory goes through LLM arbitration before being written:
//! 1. FTS5 search for related existing memories (top 8)
//! 2. If none found → direct CREATE
//! 3. If related found → LLM decides UPDATE / CREATE / SKIP
//! 4. Execute the decision; on LLM error, fall back to simple insert.

use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, error, info, warn};

use crate::prompts;
use crate::provider::LlmProvider;
use crate::store::Store;

// ─── Public types ──────────────────────────────────────────────────────────

/// A single piece of information arriving for integration.
#[derive(Debug, Clone)]
pub struct IncomingMemory {
    pub content: String,
    pub category: String,
    pub source: String,
    pub confidence: f64,
    pub about_person: Option<String>,
}

/// Result counts from a batch integration run.
#[derive(Debug, Default)]
pub struct IntegrationResult {
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
}

// ─── Integrator ────────────────────────────────────────────────────────────

pub struct MemoryIntegrator {
    store: Arc<Store>,
}

impl MemoryIntegrator {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// Integrate a batch of incoming memories through LLM arbitration.
    /// Processes entries sequentially so each one sees the freshly-updated store.
    pub async fn integrate(
        &self,
        entries: Vec<IncomingMemory>,
        provider: &dyn LlmProvider,
    ) -> Result<IntegrationResult> {
        let mut result = IntegrationResult::default();

        for entry in entries {
            match self.integrate_one(&entry, provider).await {
                Ok(action) => match action {
                    Action::Created => result.created += 1,
                    Action::Updated => result.updated += 1,
                    Action::Skipped => result.skipped += 1,
                },
                Err(e) => {
                    warn!("MemoryIntegrator: LLM arbitration failed for '{}', falling back to insert: {e}", entry.content);
                    self.fallback_insert(&entry);
                    result.created += 1;
                }
            }
        }

        info!(
            "MemoryIntegrator: created={} updated={} skipped={}",
            result.created, result.updated, result.skipped
        );
        Ok(result)
    }

    // ─── Private helpers ────────────────────────────────────────────────

    async fn integrate_one(
        &self,
        entry: &IncomingMemory,
        provider: &dyn LlmProvider,
    ) -> Result<Action> {
        let content = entry.content.trim();
        if content.is_empty() {
            return Ok(Action::Skipped);
        }

        // Search for related existing memories
        let related = self.store.search_memories(content, 8)?;

        if related.is_empty() {
            // No competition — just create
            debug!("MemoryIntegrator: no related memories, creating directly");
            self.execute_create(entry)?;
            return Ok(Action::Created);
        }

        // Build the related-memories block for the prompt
        let related_text = related
            .iter()
            .map(|m| format!("[id={}] {}", m.id, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = prompts::memory_integrator_template("en")
            .replace("{content}", content)
            .replace("{source}", &entry.source)
            .replace("{category}", &entry.category)
            .replace("{related_text}", &related_text);

        let response = provider.invoke(&prompt, None).await?;
        let action_line = response.lines().find(|l| {
            let t = l.trim();
            t.starts_with("UPDATE ") || t.starts_with("CREATE ") || t == "SKIP"
        });

        match action_line {
            None => {
                warn!("MemoryIntegrator: could not parse LLM response, falling back to create");
                self.execute_create(entry)?;
                Ok(Action::Created)
            }
            Some(line) => self.execute_action(line.trim(), entry),
        }
    }

    fn execute_action(&self, line: &str, entry: &IncomingMemory) -> Result<Action> {
        if line == "SKIP" {
            debug!("MemoryIntegrator: SKIP '{}'", entry.content);
            return Ok(Action::Skipped);
        }

        if let Some(rest) = line.strip_prefix("UPDATE ") {
            // Parse: UPDATE {id} → {text}
            if let Some((id_part, text_part)) = rest.split_once(" → ") {
                let id_str = id_part.trim();
                let text = text_part.trim();
                if let Ok(id) = id_str.parse::<i64>() {
                    if !text.is_empty() {
                        debug!("MemoryIntegrator: UPDATE id={id} content='{text}'");
                        self.store.update_memory_content(id, text)?;
                        return Ok(Action::Updated);
                    }
                }
            }
            // Malformed UPDATE — fall through to create
            warn!("MemoryIntegrator: malformed UPDATE line '{line}', creating instead");
            self.execute_create(entry)?;
            return Ok(Action::Created);
        }

        if let Some(rest) = line.strip_prefix("CREATE → ") {
            let text = rest.trim();
            let effective_content = if text.is_empty() {
                &entry.content
            } else {
                text
            };
            let mut create_entry = entry.clone();
            create_entry.content = effective_content.to_string();
            debug!("MemoryIntegrator: CREATE '{effective_content}'");
            self.execute_create(&create_entry)?;
            return Ok(Action::Created);
        }

        // Unrecognised line
        warn!("MemoryIntegrator: unrecognised action line '{line}', creating");
        self.execute_create(entry)?;
        Ok(Action::Created)
    }

    fn execute_create(&self, entry: &IncomingMemory) -> Result<()> {
        if let Some(ref person) = entry.about_person {
            self.store.save_memory_about_person(
                &entry.category,
                &entry.content,
                &entry.source,
                entry.confidence,
                "public",
                person,
            )?;
        } else {
            self.store.save_memory(
                &entry.category,
                &entry.content,
                &entry.source,
                entry.confidence,
            )?;
        }
        Ok(())
    }

    /// Fallback: simple insert without LLM (used when the LLM call fails).
    fn fallback_insert(&self, entry: &IncomingMemory) {
        if let Err(e) = self.execute_create(entry) {
            error!("MemoryIntegrator: fallback insert failed: {e}");
        }
    }
}

// ─── Internal action enum ──────────────────────────────────────────────────

enum Action {
    Created,
    Updated,
    Skipped,
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use async_trait::async_trait;

    // ── Mock provider that returns a fixed response ──────────────────────

    struct MockProvider {
        response: String,
    }

    impl MockProvider {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }
        async fn invoke(&self, _prompt: &str, _system: Option<&str>) -> Result<String> {
            Ok(self.response.clone())
        }
    }

    fn make_entry(content: &str) -> IncomingMemory {
        IncomingMemory {
            content: content.to_string(),
            category: "behavior".to_string(),
            source: "test".to_string(),
            confidence: 0.8,
            about_person: None,
        }
    }

    // ── Helper: run a single-entry integration ───────────────────────────

    async fn integrate_one_entry(
        store: Arc<Store>,
        entry: IncomingMemory,
        provider_response: &str,
    ) -> IntegrationResult {
        let integrator = MemoryIntegrator::new(Arc::clone(&store));
        let provider = MockProvider::new(provider_response);
        integrator.integrate(vec![entry], &provider).await.unwrap()
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_empty_content_skipped() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let result = integrate_one_entry(Arc::clone(&store), make_entry("   "), "SKIP").await;
        assert_eq!(result.skipped, 1);
        assert_eq!(store.count_memories().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_no_related_creates_directly() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let result = integrate_one_entry(
            Arc::clone(&store),
            make_entry("loves hiking on weekends"),
            "SKIP", // provider never called when no related memories
        )
        .await;
        assert_eq!(result.created, 1);
        assert_eq!(store.count_memories().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_llm_create_action() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        // Pre-seed an unrelated memory so FTS search returns something
        store
            .save_memory("behavior", "likes coffee", "test", 0.9)
            .unwrap();

        let result = integrate_one_entry(
            Arc::clone(&store),
            make_entry("loves hiking on weekends"),
            "CREATE → loves hiking on weekends",
        )
        .await;
        assert_eq!(result.created, 1);
        assert_eq!(store.count_memories().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_llm_skip_action() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .save_memory("behavior", "likes coffee in the morning", "test", 0.9)
            .unwrap();

        let result =
            integrate_one_entry(Arc::clone(&store), make_entry("likes coffee"), "SKIP").await;
        assert_eq!(result.skipped, 1);
        assert_eq!(store.count_memories().unwrap(), 1); // no new memory
    }

    #[tokio::test]
    async fn test_llm_update_action() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        // The existing memory must contain the incoming text as a substring for the LIKE search to match.
        let id = store
            .save_memory("behavior", "likes coffee with sugar and milk", "test", 0.9)
            .unwrap();

        // Incoming text "likes coffee with sugar" is a substring of the existing memory → LIKE match
        let result = integrate_one_entry(
            Arc::clone(&store),
            make_entry("likes coffee with sugar"),
            &format!("UPDATE {id} → likes coffee with sugar, no milk"),
        )
        .await;
        assert_eq!(result.updated, 1);
        assert_eq!(store.count_memories().unwrap(), 1); // no new row

        let memories = store.load_memories().unwrap();
        assert_eq!(memories[0].content, "likes coffee with sugar, no milk");
    }

    #[tokio::test]
    async fn test_llm_malformed_update_falls_back_to_create() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .save_memory("behavior", "likes coffee", "test", 0.9)
            .unwrap();

        // Malformed UPDATE (no arrow)
        let result = integrate_one_entry(
            Arc::clone(&store),
            make_entry("likes tea"),
            "UPDATE garbage line",
        )
        .await;
        assert_eq!(result.created, 1);
        assert_eq!(store.count_memories().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_batch_sequential_integration() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let integrator = MemoryIntegrator::new(Arc::clone(&store));

        // Provider always creates
        let provider = MockProvider::new("CREATE → passthrough");
        let entries = vec![
            make_entry("entry one"),
            make_entry("entry two"),
            make_entry("entry three"),
        ];
        let result = integrator.integrate(entries, &provider).await.unwrap();
        // First entry has no related → direct create; others go through LLM → create
        assert_eq!(result.created, 3);
        assert_eq!(store.count_memories().unwrap(), 3);
    }

    #[tokio::test]
    async fn test_llm_failure_falls_back_to_insert() {
        struct FailingProvider;
        #[async_trait]
        impl LlmProvider for FailingProvider {
            fn name(&self) -> &str {
                "failing"
            }
            async fn invoke(&self, _: &str, _: Option<&str>) -> Result<String> {
                anyhow::bail!("network error")
            }
        }

        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .save_memory("behavior", "likes coffee", "test", 0.9)
            .unwrap();

        let integrator = MemoryIntegrator::new(Arc::clone(&store));
        let result = integrator
            .integrate(vec![make_entry("likes tea")], &FailingProvider)
            .await
            .unwrap();
        // Fallback insert should still create
        assert_eq!(result.created, 1);
        assert_eq!(store.count_memories().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_about_person_create() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let integrator = MemoryIntegrator::new(Arc::clone(&store));
        let provider = MockProvider::new("CREATE → Alice is a manager");
        let entry = IncomingMemory {
            content: "Alice is a manager".to_string(),
            category: "person".to_string(),
            source: "chat".to_string(),
            confidence: 0.9,
            about_person: Some("Alice".to_string()),
        };
        let result = integrator.integrate(vec![entry], &provider).await.unwrap();
        assert_eq!(result.created, 1);
        // Verify about_person was set
        let mems = store.get_memories_about_person("Alice").unwrap();
        assert_eq!(mems.len(), 1);
    }
}
