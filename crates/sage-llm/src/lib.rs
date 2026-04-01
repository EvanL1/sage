pub mod agent;
pub mod discovery;
pub mod provider;

mod config;

pub use agent::Agent;
pub use config::AgentConfig;
pub use discovery::{discover_providers, resolve_provider, select_best_provider};
pub use provider::{create_provider_from_config, LlmProvider};

use std::path::PathBuf;

/// 通过登录 shell 解析 CLI 二进制路径，适配任何安装方式。
/// 支持 bare name（"claude"）和绝对路径（fallback 到 shell discovery）。
pub(crate) fn resolve_cli_path(name: &str) -> Option<PathBuf> {
    // 绝对路径且存在 → 直接用
    if name.contains('/') {
        let p = PathBuf::from(name);
        if p.exists() {
            return Some(p);
        }
    }
    // 提取 bare name，通过登录 shell 继承用户完整 PATH
    let bare = std::path::Path::new(name)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(name);
    for shell in ["/bin/zsh", "/bin/bash"] {
        if let Some(path) = std::process::Command::new(shell)
            .args(["-lc", &format!("which {bare}")])
            .current_dir("/tmp")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}
