use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub claude_binary: String,
    #[serde(default)]
    pub codex_binary: String,
    #[serde(default)]
    pub gemini_binary: String,
    pub default_model: String,
    pub project_dir: String,
    pub max_budget_usd: f64,
    pub permission_mode: String,
    /// 单个 Agent 实例最多调用 LLM 的次数（护栏，防止无限循环）
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
}

fn default_max_iterations() -> usize {
    10
}

fn default_provider() -> String {
    "claude".into()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: "claude".into(),
            claude_binary: "claude".into(),
            codex_binary: String::new(),
            gemini_binary: String::new(),
            default_model: "sonnet".into(),
            project_dir: "~".into(),
            max_budget_usd: 0.50,
            permission_mode: "bypassPermissions".into(),
            max_iterations: 10,
        }
    }
}
