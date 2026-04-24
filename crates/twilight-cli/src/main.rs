use clap::{Parser, Subcommand};
use twilight_bus::TwilightBus;
use twilight_core::{create_default_identity, create_presence};
use twilight_proto::twilight::AgentStatus;
use std::sync::Arc;
use anyhow::Result;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a node and advertise presence
    Run {
        #[arg(short, long, default_value = "twilight-cli")]
        name: String,
    },
    /// List all agents in the registry
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    let bus = Arc::new(TwilightBus::new("default", "local").await?);

    match &cli.command {
        Commands::Run { name } => {
            let identity = create_default_identity(name, "cli");
            let presence = create_presence(identity.clone(), AgentStatus::Online);
            
            println!("Starting node: {} ({})", name, identity.node_uuid);
            bus.publish_presence(&presence).await?;
            
            // Keep running to send heartbeats (omitted for brevity in MVP)
            tokio::signal::ctrl_c().await?;
        }
        Commands::List => {
            // In a real system, the CLI would query the traffic controller or listen for presence
            println!("Querying registry (placeholder)...");
        }
    }

    Ok(())
}
