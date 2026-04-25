mod config;
mod daemon;
mod ipc;
mod process;

use anyhow::Result;
use clap::Parser;
use config::DaemonConfig;
use daemon::TwilightDaemon;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Twilight Bark Daemon — persistent fabric node process")]
struct Args {
    /// Path to daemon.toml config file
    #[arg(long, env = "TWILIGHT_CONFIG")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = DaemonConfig::load(&args.config)?;

    env_logger::Builder::new()
        .filter_level(
            config.daemon.log_level
                .parse()
                .unwrap_or(log::LevelFilter::Info),
        )
        .target(env_logger::Target::Stderr)
        .init();

    let mut daemon = TwilightDaemon::start(config).await?;
    daemon.run_until_signal().await
}
