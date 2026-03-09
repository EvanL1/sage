use anyhow::Result;
use clap::{Parser, Subcommand};
use sage_core::session_analyzer;
use sage_core::store::Store;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sage-ingest", about = "Sage 数据导入工具")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 分析 Claude Code session JSONL 并存入 SQLite
    Session {
        jsonl_path: PathBuf,
    },
    /// 手动触发 Sage → Claude Code 记忆同步
    Sync,
    /// 显示 Sage 记忆统计
    Status,
}

fn db_path() -> PathBuf {
    std::env::var("SAGE_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").expect("HOME 未设置"))
                .join(".sage/sage.db")
        })
}

fn default_memory_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME 未设置"));
    // 默认项目路径 ~/dev/digital-twin，/ 替换为 -
    let project = home.join("dev/digital-twin");
    let encoded = project
        .to_string_lossy()
        .replace('/', "-");
    home.join(".claude/projects")
        .join(encoded)
        .join("memory")
}

fn cmd_session(jsonl_path: &std::path::Path) -> Result<()> {
    let db = db_path();
    let store = Store::open(&db)?;

    let summary = session_analyzer::analyze_session(jsonl_path)?;

    // 存储 session 元信息
    let session_content = format!(
        "[session] {} — {} msgs, {} files modified, {} commands",
        summary.summary_hint,
        summary.message_count,
        summary.files_modified.len(),
        summary.commands_run.len(),
    );
    store.save_memory("session", &session_content, "claude-code", 0.8)?;

    // 如果有足够的用户消息，提取模式
    if summary.user_messages.len() >= 2 {
        let topics = summary
            .user_messages
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" / ");
        let pattern_content = format!("Claude Code session topics: {topics}");
        store.save_memory("pattern", &pattern_content, "claude-code", 0.7)?;
    }

    // 同步到 Claude Code MEMORY.md
    let memory_dir = default_memory_dir();
    if memory_dir.exists() {
        store.sync_to_claude_memory(&memory_dir)?;
        println!("已同步到 {}", memory_dir.display());
    }

    // 输出 top 工具使用统计
    let mut tools: Vec<_> = summary.tools_used.iter().collect();
    tools.sort_by(|a, b| b.1.cmp(a.1));
    let top_tools: Vec<_> = tools.iter().take(5).map(|(k, v)| format!("{}({})", k, v)).collect();

    println!("session 导入完成:");
    println!("  消息数: {}", summary.message_count);
    println!("  用户消息: {}", summary.user_messages.len());
    println!("  修改文件: {}", summary.files_modified.len());
    println!("  执行命令: {}", summary.commands_run.len());
    println!("  工具 Top5: {}", top_tools.join(", "));
    Ok(())
}

fn cmd_sync() -> Result<()> {
    let db = db_path();
    let store = Store::open(&db)?;
    let memory_dir = default_memory_dir();
    if !memory_dir.exists() {
        std::fs::create_dir_all(&memory_dir)?;
    }
    store.sync_to_claude_memory(&memory_dir)?;
    println!("同步完成 → {}", memory_dir.display());
    Ok(())
}

fn cmd_status() -> Result<()> {
    let db = db_path();
    let store = Store::open(&db)?;

    let memories = store.load_memories()?;
    let sessions = store.count_distinct_sessions()?;

    let session_mems: Vec<_> = memories.iter().filter(|m| m.category == "session").collect();

    println!("Sage 记忆统计");
    println!("  memories 总数: {}", memories.len());
    println!("  session records: {}", session_mems.len());
    println!("  chat sessions: {}", sessions);
    println!("  DB 路径: {}", db_path().display());
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let cli = Cli::parse();
    match &cli.command {
        Cmd::Session { jsonl_path } => cmd_session(jsonl_path),
        Cmd::Sync => cmd_sync(),
        Cmd::Status => cmd_status(),
    }
}
