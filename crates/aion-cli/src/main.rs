mod app;
mod client;
mod config;
mod event;
mod ui;

use clap::{Parser, Subcommand};
use config::CliConfig;

#[derive(Parser)]
#[command(name = "aion", version, about = "Aion AI CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Server URL
    #[arg(long, env = "AION_SERVER_URL", default_value = "http://127.0.0.1:3456")]
    server_url: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a chat session
    Chat {
        /// Agent type
        #[arg(long, default_value = "acp")]
        agent: String,

        /// Model override
        #[arg(long)]
        model: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let (agent, model) = match cli.command {
        Some(Commands::Chat { agent, model }) => (agent, model),
        None => ("acp".to_string(), None),
    };

    let config = CliConfig {
        server_url: cli.server_url,
        agent_type: agent,
        model,
    };
    println!("Config: {config:?}");

    Ok(())
}
