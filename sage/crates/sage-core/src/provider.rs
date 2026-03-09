use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;
use tracing::{debug, info};

static CODEX_SEQ: AtomicU64 = AtomicU64::new(0);

use crate::config::AgentConfig;
use sage_types::{ProviderConfig, ProviderInfo, ProviderKind};

/// LLM Provider trait — 统一 Claude/Codex/Gemini 调用接口
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String>;
}

/// 根据 config 创建对应的 provider
pub fn create_provider(config: &AgentConfig) -> Box<dyn LlmProvider> {
    match config.provider.as_str() {
        "codex" => Box::new(CodexProvider::new(config)),
        "gemini" => Box::new(GeminiProvider::new(config)),
        _ => Box::new(ClaudeProvider::new(config)),
    }
}

// ─── Claude Provider ──────────────────────────────────

struct ClaudeProvider {
    binary: String,
    model: String,
    project_dir: String,
    max_budget_usd: f64,
    permission_mode: String,
}

impl ClaudeProvider {
    fn new(config: &AgentConfig) -> Self {
        Self {
            binary: config.claude_binary.clone(),
            model: config.default_model.clone(),
            project_dir: config.project_dir.clone(),
            max_budget_usd: config.max_budget_usd,
            permission_mode: config.permission_mode.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let mut cmd = Command::new(&self.binary);

        cmd.arg("--print")
            .arg("--model").arg(&self.model)
            .arg("--permission-mode").arg(&self.permission_mode)
            .arg("--max-budget-usd").arg(self.max_budget_usd.to_string())
            .arg("--add-dir").arg(&self.project_dir)
            .arg("--output-format").arg("text")
            .arg("--no-session-persistence");

        if let Some(sp) = system_prompt {
            cmd.arg("--system-prompt").arg(sp);
        }

        cmd.arg(prompt);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.env_remove("CLAUDECODE");

        info!("Invoking Claude (model: {})", self.model);
        let preview: String = prompt.chars().take(100).collect();
        debug!("Prompt: {preview}");

        let output = cmd.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let msg = if stderr.is_empty() { &stdout } else { &stderr };
            anyhow::bail!("Claude CLI failed (exit {}): {msg}", output.status);
        }

        Ok(stdout)
    }
}

// ─── Codex Provider ──────────────────────────────────

struct CodexProvider {
    binary: String,
    model: String,
    project_dir: String,
}

impl CodexProvider {
    fn new(config: &AgentConfig) -> Self {
        Self {
            binary: if config.codex_binary.is_empty() {
                "/opt/homebrew/bin/codex".into()
            } else {
                config.codex_binary.clone()
            },
            model: config.default_model.clone(),
            project_dir: config.project_dir.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for CodexProvider {
    fn name(&self) -> &str {
        "codex"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let full_prompt = match system_prompt {
            Some(sp) => format!("[System Instructions]\n{sp}\n\n[Task]\n{prompt}"),
            None => prompt.to_string(),
        };

        let seq = CODEX_SEQ.fetch_add(1, Ordering::Relaxed);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp_out = format!("/tmp/sage-codex-{}-{}.txt", ts, seq);

        let mut cmd = Command::new(&self.binary);
        cmd.arg("exec")
            .arg("-m").arg(&self.model)
            .arg("--sandbox").arg("read-only")
            .arg("-o").arg(&tmp_out)
            .arg("-C").arg(&self.project_dir)
            .arg("--ephemeral")
            .arg(&full_prompt);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        info!("Invoking Codex (model: {})", self.model);
        let preview: String = full_prompt.chars().take(100).collect();
        debug!("Prompt: {preview}");

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!("Codex CLI failed (exit {}): {msg}", output.status);
        }

        let result = if std::path::Path::new(&tmp_out).exists() {
            let text = tokio::fs::read_to_string(&tmp_out).await?;
            let _ = tokio::fs::remove_file(&tmp_out).await;
            text
        } else {
            String::from_utf8_lossy(&output.stdout).to_string()
        };

        Ok(result)
    }
}

// ─── Gemini Provider ──────────────────────────────────

struct GeminiProvider {
    binary: String,
    model: String,
}

impl GeminiProvider {
    fn new(config: &AgentConfig) -> Self {
        Self {
            binary: if config.gemini_binary.is_empty() {
                "/opt/homebrew/bin/gemini".into()
            } else {
                config.gemini_binary.clone()
            },
            model: config.default_model.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let full_prompt = match system_prompt {
            Some(sp) => format!("[System Instructions]\n{sp}\n\n[Task]\n{prompt}"),
            None => prompt.to_string(),
        };

        let mut cmd = Command::new(&self.binary);
        cmd.arg("-p").arg(&full_prompt)
            .arg("-m").arg(&self.model)
            .arg("-o").arg("text")
            .arg("--yolo");

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        info!("Invoking Gemini (model: {})", self.model);
        let preview: String = full_prompt.chars().take(100).collect();
        debug!("Prompt: {preview}");

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!("Gemini CLI failed (exit {}): {msg}", output.status);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// ─── Anthropic HTTP API Provider ──────────────────────────

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";
const DEFAULT_MAX_TOKENS: u32 = 4096;

struct AnthropicHttpProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicHttpProvider {
    fn new(api_key: String, model: Option<String>, base_url: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.into()),
            base_url: base_url.unwrap_or_else(|| ANTHROPIC_API_URL.into()),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

#[async_trait]
impl LlmProvider for AnthropicHttpProvider {
    fn name(&self) -> &str {
        "anthropic-api"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let body = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
            system: system_prompt.map(String::from),
            messages: vec![AnthropicMessage {
                role: "user".into(),
                content: prompt.into(),
            }],
        };

        info!("Invoking Anthropic API (model: {})", self.model);
        let preview: String = prompt.chars().take(100).collect();
        debug!("Prompt: {preview}");

        let resp = self
            .client
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Anthropic API 请求失败")?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API 返回 {status}: {err_text}");
        }

        let data: AnthropicResponse = resp
            .json()
            .await
            .context("解析 Anthropic API 响应失败")?;

        data.content
            .first()
            .map(|c| c.text.clone())
            .ok_or_else(|| anyhow::anyhow!("Anthropic API 响应中无 content"))
    }
}

// ─── 工厂函数：根据 ProviderInfo + ProviderConfig 创建 provider ──

/// 根据发现结果和用户配置创建 provider
pub fn create_provider_from_config(
    info: &ProviderInfo,
    config: &ProviderConfig,
    agent_config: &AgentConfig,
) -> Box<dyn LlmProvider> {
    match info.kind {
        ProviderKind::HttpApi => {
            // 优先用 config 中的 api_key，其次用环境变量
            let api_key = config
                .api_key
                .clone()
                .or_else(|| resolve_api_key_from_env(&info.id))
                .unwrap_or_default();
            Box::new(AnthropicHttpProvider::new(
                api_key,
                config.model.clone(),
                config.base_url.clone(),
            ))
        }
        ProviderKind::Cli => {
            // CLI provider 沿用现有 create_provider 逻辑
            create_provider(agent_config)
        }
    }
}

/// 根据 provider id 解析对应的环境变量 API key
fn resolve_api_key_from_env(provider_id: &str) -> Option<String> {
    let env_var = match provider_id {
        "anthropic-api" => "ANTHROPIC_API_KEY",
        "openai-api" => "OPENAI_API_KEY",
        "deepseek-api" => "DEEPSEEK_API_KEY",
        _ => return None,
    };
    std::env::var(env_var).ok()
}
