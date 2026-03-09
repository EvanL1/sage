use anyhow::Result;
use chrono::Local;
use std::path::PathBuf;
use tracing::info;

pub struct Memory {
    base_dir: PathBuf,
}

impl Memory {
    pub fn new(base_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    #[allow(dead_code)]
    pub fn read_core(&self) -> Result<String> {
        let path = self.base_dir.join("MEMORY.md");
        if path.exists() {
            Ok(std::fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    pub fn append(&self, file: &str, entry: &str) -> Result<()> {
        let path = self.base_dir.join(file);
        let timestamp = Local::now().format("%Y-%m-%d %H:%M");
        let existing = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let updated = format!("{existing}\n\n## {timestamp}\n{entry}");
        std::fs::write(&path, updated.trim())?;
        info!("Memory updated: {file}");
        Ok(())
    }

    pub fn record_pattern(&self, category: &str, observation: &str) -> Result<()> {
        self.append("patterns.md", &format!("**{category}**: {observation}"))
    }

    pub fn record_decision(&self, context: &str, decision: &str) -> Result<()> {
        self.append(
            "decisions.md",
            &format!("- **Context**: {context}\n- **Decision**: {decision}"),
        )
    }

    /// 汇总所有记忆，作为 Claude 调用的上下文
    #[allow(dead_code)]
    pub fn as_context(&self) -> Result<String> {
        let mut ctx = String::new();
        for file in ["MEMORY.md", "patterns.md", "decisions.md"] {
            let path = self.base_dir.join(file);
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                ctx.push_str(&format!("\n## {file}\n{content}\n"));
            }
        }
        Ok(ctx)
    }
}
