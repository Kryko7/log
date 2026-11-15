//! Local Axum HTTP API & static asset server (`log-journal` binary).

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "log-journal", about = "Local-first personal journal server")]
struct Cli {
    /// Port to bind the local HTTP server.
    #[arg(long, default_value_t = 3210)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let vault = journal_core::default_vault_dir();
    println!(
        "log-journal skeleton initialized (port {}, vault {})",
        cli.port,
        vault.display()
    );
    Ok(())
}
