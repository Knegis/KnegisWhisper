//! `knegiswhisper-mcp` — kept thin on purpose, same as `knegiswhisper` itself.
//!
//! Everything that can be tested without a real stdio pipe lives in
//! [`kw_mcp::server`] and [`kw_mcp::tools`]. What is left here is: point
//! `tracing` at stderr and nowhere else, connect to the running application
//! through the same discovery file `knegiswhisper` reads, and hand the real
//! stdin/stdout to [`kw_mcp::server::run`].

use kw_cli::{CliError, Client, Exit};
use tracing_subscriber::EnvFilter;

/// Why the process is exiting other than cleanly.
///
/// The discovery/connection failures are exactly [`CliError`] — same
/// messages, same exit codes as `knegiswhisper` itself, per the brief: this
/// crate follows that contract rather than inventing a parallel one. An I/O
/// failure on stdin or stdout is not something the CLI's contract has a code
/// for, since the CLI never runs a long-lived loop over them; it is treated
/// as a generic failure.
#[derive(Debug, thiserror::Error)]
enum MainError {
    #[error(transparent)]
    Cli(#[from] CliError),
    #[error("the stdio loop stopped: {0}")]
    Io(#[from] std::io::Error),
}

impl MainError {
    const fn exit(&self) -> Exit {
        match self {
            Self::Cli(err) => err.exit(),
            Self::Io(_) => Exit::Failed,
        }
    }
}

fn main() -> std::process::ExitCode {
    // Every diagnostic goes to stderr, through `tracing`, and nothing else
    // ever writes to stdout: that is the one rule this whole crate exists to
    // keep, because a stray line there corrupts the JSON-RPC stream for
    // whatever is reading it.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("knegiswhisper-mcp: could not start: {err}");
            return std::process::ExitCode::from(Exit::Failed.code() as u8);
        }
    };

    match runtime.block_on(run()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("knegiswhisper-mcp: {err}");
            std::process::ExitCode::from(err.exit().code() as u8)
        }
    }
}

async fn run() -> Result<(), MainError> {
    // The app must already be running: this server has no backend of its
    // own, only the local API's. A missing or stale discovery file is
    // reported exactly as `knegiswhisper health` would report it, rather than
    // starting anything on the caller's behalf.
    let path = kw_cli::default_discovery_path()?;
    let client = Client::connect(&path)?;

    tracing::info!("connected to KnegisWhisper's local API; reading JSON-RPC from stdin");

    let reader = tokio::io::BufReader::new(tokio::io::stdin());
    let writer = tokio::io::stdout();

    kw_mcp::server::run(reader, writer, client).await?;

    tracing::info!("stdin closed; exiting");
    Ok(())
}
