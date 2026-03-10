use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser)]
#[command(name = "sage", about = "Sage Daemon — your personal AI counselor")]
struct Cli {
    /// Config file path
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Run in foreground (don't daemonize)
    #[arg(long)]
    foreground: bool,

    /// Run heartbeat once and exit
    #[arg(long)]
    heartbeat_once: bool,

    /// Trigger a specific report type (morning/evening/weekly/week_start) and exit
    #[arg(long)]
    trigger: Option<String>,
}

/// 尝试获取事件循环排他锁
fn try_acquire_event_loop_lock() -> Option<std::fs::File> {
    let lock_path = dirs::home_dir()?.join(".sage/data/event-loop.pid");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .ok()?;
    use fs2::FileExt;
    match file.try_lock_exclusive() {
        Ok(()) => {
            use std::io::Write;
            let mut f = &file;
            let _ = f.write_all(format!("{}", std::process::id()).as_bytes());
            Some(file)
        }
        Err(_) => None,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("sage=info,sage_core=info,sage_daemon=info")
        .init();

    let config = sage_core::config::Config::load(&cli.config)?;
    info!("Sage Daemon v{}", env!("CARGO_PKG_VERSION"));

    if let Some(report_type) = cli.trigger {
        info!("Triggering report: {report_type}");
        let daemon = sage_core::Daemon::new(config)?;
        let result = daemon.trigger_report(&report_type).await?;
        println!("{result}");
        return Ok(());
    }

    if cli.heartbeat_once {
        info!("Running single heartbeat...");
        let daemon = sage_core::Daemon::new(config)?;
        daemon.heartbeat_once().await?;
        return Ok(());
    }

    // 常驻模式：需要获取事件循环锁
    let _lock_file = match try_acquire_event_loop_lock() {
        Some(f) => f,
        None => {
            anyhow::bail!("其他进程（Desktop 或另一个 daemon）已持有事件循环锁，退出");
        }
    };

    info!(
        "Starting event loop (heartbeat: {}s)",
        config.daemon.heartbeat_interval_secs
    );

    let daemon = sage_core::Daemon::new(config)?;
    daemon.run().await
}
