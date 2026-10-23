use anyhow::Result;
use clap::{Parser, Subcommand};
use local_mcp::{config::Settings, mcp, session};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Own one session and its App Server until Ctrl-C or stdin closes.
    Start {
        /// Session ID to use instead of generating a UUID.
        session_id: Option<String>,
        /// Trusted startup settings (JSON), never read from MCP arguments.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Official Codex CLI executable; command/exec does not call a model.
        #[arg(long, default_value = "codex")]
        codex: PathBuf,
    },
    /// Serve MCP over stdio; execution stays owned by `start`.
    Mcp,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command.unwrap_or(Command::Start {
        session_id: None,
        config: None,
        codex: PathBuf::from("codex"),
    }) {
        Command::Start {
            session_id,
            config,
            codex,
        } => {
            let settings = match config {
                Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
                None => Settings::default(),
            };
            session::start(session_id, settings, codex).await
        }
        Command::Mcp => mcp::serve().await,
    }
}
