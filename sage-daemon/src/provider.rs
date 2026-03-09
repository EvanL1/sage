use anyhow::Result;
use async_trait::async_trait;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, info};

static CODEX_SEQ: AtomicU64 = AtomicU64::new(0);

/// CLI 调用超时（防止网络故障时长时间挂起）
const CLI_TIMEOUT: Duration = Duration::from_secs(90);

use crate::config::AgentConfig;

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

        let output = tokio::time::timeout(CLI_TIMEOUT, cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("CLI timed out after {}s", CLI_TIMEOUT.as_secs()))??;
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
        // Codex 不支持 --system-prompt flag，将 system prompt 注入 prompt 前缀
        let full_prompt = match system_prompt {
            Some(sp) => format!("[System Instructions]\n{sp}\n\n[Task]\n{prompt}"),
            None => prompt.to_string(),
        };

        let seq = CODEX_SEQ.fetch_add(1, Ordering::Relaxed);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp_out = format!("/tmp/sage-codex-{ts}-{seq}.txt");

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

        let output = tokio::time::timeout(CLI_TIMEOUT, cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("CLI timed out after {}s", CLI_TIMEOUT.as_secs()))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!("Codex CLI failed (exit {}): {msg}", output.status);
        }

        // 读取 -o 输出文件
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
        // Gemini 不支持 --system-prompt flag，注入 prompt 前缀
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

        let output = tokio::time::timeout(CLI_TIMEOUT, cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("CLI timed out after {}s", CLI_TIMEOUT.as_secs()))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!("Gemini CLI failed (exit {}): {msg}", output.status);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
