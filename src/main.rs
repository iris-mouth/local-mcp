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
    /// Prepare a new, independent runtime. Does not read keys or start services.
    #[cfg(unix)]
    Prepare {
        #[arg(long)]
        destination: PathBuf,
        #[arg(long)]
        tunnel_id: String,
        #[arg(long)]
        tunnel_client: PathBuf,
        #[arg(long)]
        codex: PathBuf,
        /// Existing macOS Keychain item; shared items are never modified.
        #[arg(long, default_value = "chatgpt-mcp-tunnel")]
        keychain_service: String,
        #[arg(long)]
        keychain_account: Option<String>,
    },
    /// Start a selected project and automatically manage its Tunnel.
    #[cfg(unix)]
    Connect {
        project: Option<PathBuf>,
        #[arg(long)]
        runtime: Option<PathBuf>,
        /// Trusted execution ceilings, identical to `start --config`.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    #[cfg(unix)]
    #[command(hide = true)]
    TunnelRun {
        #[arg(long)]
        runtime: PathBuf,
        #[arg(long)]
        session: String,
    },
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
        #[cfg(unix)]
        Command::Prepare {
            destination,
            tunnel_id,
            tunnel_client,
            codex,
            keychain_service,
            keychain_account,
        } => local_mcp::connection::prepare(
            destination,
            tunnel_id,
            tunnel_client,
            codex,
            keychain_service,
            keychain_account,
        ),
        #[cfg(unix)]
        Command::Connect {
            project,
            runtime,
            config,
        } => {
            let settings = match config {
                Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
                None => Settings::default(),
            };
            local_mcp::connection::connect(project, runtime, settings).await
        }
        #[cfg(unix)]
        Command::TunnelRun { runtime, session } => {
            // Credential failures must not cause repeated Keychain prompts.
            if let Err(error) = local_mcp::connection::tunnel_run(&runtime, &session) {
                eprintln!("Tunnel startup failed: {error}");
                std::process::exit(78);
            }
            Ok(())
        }
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
