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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("sage=info,sage_core=info,sage_daemon=info")
        .init();

    let config = sage_core::config::Config::load(&cli.config)?;
    info!("Sage Daemon v{}", env!("CARGO_PKG_VERSION"));

    if cli.heartbeat_once {
        info!("Running single heartbeat...");
        let daemon = sage_core::Daemon::new(config)?;
        daemon.heartbeat_once().await?;
        return Ok(());
    }

    info!(
        "Starting event loop (heartbeat: {}s)",
        config.daemon.heartbeat_interval_secs
    );

    let daemon = sage_core::Daemon::new(config)?;
    daemon.run().await
}
