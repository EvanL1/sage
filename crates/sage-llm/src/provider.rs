use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;
use tracing::{debug, info};

static CODEX_SEQ: AtomicU64 = AtomicU64::new(0);

/// 默认 Claude CLI 调用超时（秒），与 agent.rs 中的 TIMEOUT_NORMAL_SECS 保持一致
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// .app bundle 不继承 shell 环境变量，CLI 需要 proxy 才能连接（中国网络）
const PROXY_ENVS: &[(&str, &str)] = &[
    ("http_proxy", "http://127.0.0.1:7890"),
    ("https_proxy", "http://127.0.0.1:7890"),
    ("all_proxy", "socks5://127.0.0.1:7890"),
];

/// 返回安全的工作目录（~/.sage），避免 .app 从 Finder 启动时 cwd="/" 导致 CLI 扫描整个文件系统
fn safe_working_dir() -> std::path::PathBuf {
    let dir = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".sage"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 解析 "model:reasoning_effort" 复合格式（如 "o3:high" → ("o3", Some("high"))）
fn parse_model_with_effort(model: &str) -> (&str, Option<&str>) {
    match model.split_once(':') {
        Some((m, effort)) => (m, Some(effort)),
        None => (model, None),
    }
}

/// 将短名称（如 "claude"）解析为完整路径（.app bundle 的 PATH 通常不含 /opt/homebrew/bin）
fn resolve_binary(name: &str) -> String {
    if name.contains('/') {
        return name.to_string();
    }
    let candidates = [
        format!("/opt/homebrew/bin/{name}"),
        format!("/usr/local/bin/{name}"),
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path.clone();
        }
    }
    name.to_string()
}

/// 为 CLI Command 注入 proxy 环境变量（仅当环境中不存在时）
fn inject_proxy(cmd: &mut Command) {
    for &(key, default_val) in PROXY_ENVS {
        if std::env::var(key).is_err() {
            cmd.env(key, default_val);
        }
    }
}

/// Path to an empty MCP config file — used with --strict-mcp-config to skip all MCP servers.
/// Created once on first call, reused thereafter.
fn empty_mcp_config_path() -> String {
    let path = std::path::PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
    )
    .join(".sage/empty-mcp.json");
    if !path.exists() {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new("/tmp")));
        let _ = std::fs::write(&path, r#"{"mcpServers":{}}"#);
    }
    path.to_string_lossy().to_string()
}

use crate::config::AgentConfig;
use sage_types::{ProviderConfig, ProviderInfo, ProviderKind};

/// LLM Provider trait — 统一 Claude/Codex/Gemini 调用接口
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String>;
    /// 带自定义超时的调用（默认委托到 invoke，仅 ClaudeProvider 覆写）
    /// 默认实现忽略 timeout_secs，仅 ClaudeProvider 实际生效。
    /// 其他 provider（Codex/Gemini/Cursor）使用各自 CLI 的默认超时。
    async fn invoke_with_timeout(&self, prompt: &str, system_prompt: Option<&str>, _timeout_secs: u64) -> Result<String> {
        self.invoke(prompt, system_prompt).await
    }
}

/// 根据 config 创建对应的 provider
pub fn create_provider(config: &AgentConfig) -> Box<dyn LlmProvider> {
    match config.provider.as_str() {
        "codex" => Box::new(CodexProvider::new(config)),
        "gemini" => Box::new(GeminiProvider::new(config)),
        "cursor" => Box::new(CursorProvider::new(config)),
        _ => Box::new(ClaudeProvider::new(config)),
    }
}

// ─── Claude Provider ──────────────────────────────────

struct ClaudeProvider {
    binary: String,
    model: String,
    max_budget_usd: f64,
    permission_mode: String,
}

impl ClaudeProvider {
    fn new(config: &AgentConfig) -> Self {
        Self {
            binary: resolve_binary(&config.claude_binary),
            model: config.default_model.clone(),
            max_budget_usd: config.max_budget_usd,
            permission_mode: config.permission_mode.clone(),
        }
    }

    async fn run_cli(&self, prompt: &str, system_prompt: Option<&str>, timeout_secs: u64) -> Result<String> {
        let mut cmd = Command::new(&self.binary);

        let safe_dir = safe_working_dir();
        cmd.current_dir(&safe_dir);

        let (model, effort) = parse_model_with_effort(&self.model);
        cmd.arg("--print")
            .arg("--strict-mcp-config")
            .arg("--mcp-config")
            .arg(empty_mcp_config_path())
            .arg("--model")
            .arg(model)
            .arg("--permission-mode")
            .arg(&self.permission_mode)
            .arg("--max-budget-usd")
            .arg(self.max_budget_usd.to_string())
            .arg("--output-format")
            .arg("text")
            .arg("--no-session-persistence");
        if let Some(e) = effort {
            cmd.arg("--effort").arg(e);
        }

        if let Some(sp) = system_prompt {
            cmd.arg("--system-prompt").arg(sp);
        }

        cmd.arg(prompt);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.env_remove("CLAUDECODE");
        inject_proxy(&mut cmd);

        info!("Invoking Claude (model: {}, effort: {:?}, timeout: {}s)", model, effort, timeout_secs);
        let preview: String = prompt.chars().take(100).collect();
        debug!("Prompt: {preview}");

        let child = cmd.spawn().context("Failed to spawn Claude CLI")?;
        let pid = child.id();
        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            child.wait_with_output(),
        ).await {
            Ok(result) => result.context("Claude CLI process failed")?,
            Err(_) => {
                if let Some(pid) = pid {
                    let _ = tokio::process::Command::new("kill").arg(pid.to_string()).output().await;
                }
                anyhow::bail!("Claude CLI timed out after {timeout_secs}s");
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let msg = if stderr.is_empty() { &stdout } else { &stderr };
            anyhow::bail!("Claude CLI failed (exit {}): {msg}", output.status);
        }

        Ok(stdout)
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        self.run_cli(prompt, system_prompt, DEFAULT_TIMEOUT_SECS).await
    }

    async fn invoke_with_timeout(&self, prompt: &str, system_prompt: Option<&str>, timeout_secs: u64) -> Result<String> {
        self.run_cli(prompt, system_prompt, timeout_secs).await
    }
}

// ─── Codex Provider ──────────────────────────────────

struct CodexProvider {
    binary: String,
    model: String,
}

impl CodexProvider {
    fn new(config: &AgentConfig) -> Self {
        Self {
            binary: resolve_binary(if config.codex_binary.is_empty() {
                "codex"
            } else {
                &config.codex_binary
            }),
            model: config.default_model.clone(),
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
            .unwrap_or_default()
            .as_nanos();
        let tmp_out = format!("/tmp/sage-codex-{}-{}.txt", ts, seq);

        let safe_dir = safe_working_dir();
        let (model, reasoning_effort) = parse_model_with_effort(&self.model);
        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(&safe_dir);
        cmd.arg("exec")
            .arg("-m")
            .arg(model)
            .arg("--sandbox")
            .arg("read-only")
            .arg("-o")
            .arg(&tmp_out)
            .arg("-C")
            .arg(safe_dir.to_str().unwrap_or("/tmp"))
            .arg("--ephemeral")
            // Sage 在 ~/.sage 下跑定时任务，不一定是受信任的 Git 目录。
            .arg("--skip-git-repo-check");
        if let Some(effort) = reasoning_effort {
            cmd.arg("-c").arg(format!("reasoning_effort=\"{effort}\""));
        }
        cmd.arg(&full_prompt);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        inject_proxy(&mut cmd);

        info!(
            "Invoking Codex (model: {}, reasoning_effort: {:?})",
            model, reasoning_effort
        );
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
        cmd.current_dir(safe_working_dir());
        cmd.arg("-p")
            .arg(&full_prompt)
            .arg("-m")
            .arg(&self.model)
            .arg("-o")
            .arg("text")
            .arg("--yolo");

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        inject_proxy(&mut cmd);

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

// ─── Cursor Provider ──────────────────────────────────

struct CursorProvider {
    binary: String,
    model: String,
}

impl CursorProvider {
    fn new(config: &AgentConfig) -> Self {
        Self {
            binary: resolve_binary("agent"),
            model: config.default_model.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for CursorProvider {
    fn name(&self) -> &str {
        "cursor"
    }

    async fn invoke(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String> {
        let full_prompt = match system_prompt {
            Some(sp) => format!("[System Instructions]\n{sp}\n\n[Task]\n{prompt}"),
            None => prompt.to_string(),
        };

        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(safe_working_dir());
        cmd.arg("-p")
            .arg("--model")
            .arg(&self.model)
            .arg("--output-format")
            .arg("text")
            .arg("--yolo");
        cmd.arg(&full_prompt);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        inject_proxy(&mut cmd);

        info!("Invoking Cursor (model: {})", self.model);
        let preview: String = full_prompt.chars().take(100).collect();
        debug!("Prompt: {preview}");

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!("Cursor CLI failed (exit {}): {msg}", output.status);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// ─── Anthropic HTTP API Provider ──────────────────────────

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-6-20250627";
const DEFAULT_MAX_TOKENS: u32 = 4096;

struct AnthropicHttpProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicHttpProvider {
    fn new(api_key: String, model: Option<String>, base_url: Option<String>) -> Self {
        // .app bundle 不继承 shell 环境变量，需要显式配置 proxy
        let client = Self::build_client();
        Self {
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.into()),
            base_url: base_url.unwrap_or_else(|| ANTHROPIC_API_URL.into()),
            client,
        }
    }

    fn build_client() -> reqwest::Client {
        let mut builder = reqwest::Client::builder();
        // 如果环境中没有 proxy 变量，使用默认值（中国网络需要）
        if std::env::var("https_proxy").is_err() && std::env::var("HTTPS_PROXY").is_err() {
            if let Ok(proxy) = reqwest::Proxy::all("http://127.0.0.1:7890") {
                builder = builder.proxy(proxy);
            }
        }
        builder.build().unwrap_or_else(|_| reqwest::Client::new())
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

        let data: AnthropicResponse = resp.json().await.context("解析 Anthropic API 响应失败")?;

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
            let mut cfg = agent_config.clone();
            // 根据实际发现的 provider 分发，而非依赖 config.toml 的固定值
            cfg.provider = match info.id.as_str() {
                "codex-cli" => "codex".into(),
                "gemini-cli" => "gemini".into(),
                "cursor-cli" => "cursor".into(),
                _ => "claude".into(),
            };
            // UI 配了 model 时优先用（只影响本次调用，不改 CLI 全局配置）
            if let Some(ref m) = config.model {
                cfg.default_model = m.clone();
            }
            create_provider(&cfg)
        }
    }
}

/// 根据 provider id 解析对应的环境变量 API key
fn resolve_api_key_from_env(provider_id: &str) -> Option<String> {
    let env_var = match provider_id {
        "anthropic-api" => "ANTHROPIC_API_KEY",
        "openai-api" => "OPENAI_API_KEY",
        "deepseek-api" => "DEEPSEEK_API_KEY",
        "cursor-cli" => "CURSOR_API_KEY",
        _ => return None,
    };
    std::env::var(env_var).ok()
}
