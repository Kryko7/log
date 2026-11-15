//! Local Axum HTTP API, CLI, showcase seeder, and Arch Wayland launcher (`log-journal`).

use journal_server::api::build_router;
use journal_server::seed::seed_demo_vault;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use journal_core::Vault;
use journal_import::{import_dayone, import_notion};
use journal_sync::{create_backup, restore_backup, sync_gdrive};
use journal_types::{
    BackupConfig, ImportDayOneRequest, ImportNotionRequest, RestoreBackupRequest,
};

const DEFAULT_PORT: u16 = 4377;

#[derive(Debug, Parser)]
#[command(
    name = "log-journal",
    version,
    about = "Log — Local-first tactile Markdown personal journal"
)]
struct Cli {
    /// Override vault directory (takes precedence over LOG_VAULT_DIR and ~/Personal/JournalVault).
    #[arg(long, global = true, value_name = "PATH")]
    vault: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Start the local server on 127.0.0.1 and launch a chromeless Wayland GPU app window.
    Launch {
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Run the headless local HTTP API and static asset server.
    Serve {
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Populate an illustrative showcase vault with literary entries, Polaroids, and audio.
    SeedDemo {
        /// Optional vault path override for seeding.
        #[arg(long, value_name = "PATH")]
        vault: Option<PathBuf>,
    },
    /// Import a Day One .zip or .json export into the local vault.
    ImportDayone {
        #[arg(long, value_name = "PATH")]
        zip: PathBuf,
        #[arg(long)]
        notebook: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Import a Notion .zip/directory export or MCP JSON snapshot (skipping Day One duplicates).
    ImportNotion {
        #[arg(long, value_name = "PATH")]
        path: PathBuf,
        #[arg(long)]
        notebook: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Create an age-encrypted .tar.gz.age vault snapshot (and optionally upload to Google Drive).
    Backup {
        #[arg(long, value_name = "ENV")]
        passphrase_env: String,
        #[arg(long)]
        upload_gdrive: bool,
    },
    /// Decrypt and restore an age-encrypted vault archive into a target directory.
    Restore {
        #[arg(long, value_name = "PATH")]
        file: PathBuf,
        #[arg(long, value_name = "DIR")]
        target: PathBuf,
        #[arg(long, value_name = "ENV")]
        passphrase_env: String,
        #[arg(long, default_value_t = true)]
        overwrite: bool,
    },
}

fn resolve_vault_path(cli_vault: Option<PathBuf>, sub_vault: Option<PathBuf>) -> PathBuf {
    sub_vault
        .or(cli_vault)
        .unwrap_or_else(Vault::default_path)
}

fn read_passphrase_env(env_name: &str) -> Result<String> {
    let val = std::env::var(env_name).with_context(|| {
        format!("Environment variable '{env_name}' is not set for backup passphrase")
    })?;
    if val.trim().is_empty() {
        bail!("Environment variable '{env_name}' must not be empty");
    }
    Ok(val)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let vault_dir = resolve_vault_path(cli.vault, None);
            run_server(vault_dir, DEFAULT_PORT, true).await?;
        }
        Some(Commands::Launch { port }) => {
            let vault_dir = resolve_vault_path(cli.vault, None);
            run_server(vault_dir, port, true).await?;
        }
        Some(Commands::Serve { port }) => {
            let vault_dir = resolve_vault_path(cli.vault, None);
            run_server(vault_dir, port, false).await?;
        }
        Some(Commands::SeedDemo { vault: sub_vault }) => {
            let vault_dir = resolve_vault_path(cli.vault, sub_vault);
            let vault = Vault::open(&vault_dir)?;
            let summary = seed_demo_vault(&vault)?;
            println!(
                "Seeded showcase vault at {} ({} notebooks added, {} entries seeded)",
                summary.vault_path, summary.notebooks_created, summary.entries_seeded
            );
        }
        Some(Commands::ImportDayone {
            zip,
            notebook,
            dry_run,
        }) => {
            let vault_dir = resolve_vault_path(cli.vault, None);
            let vault = Vault::open(&vault_dir)?;
            let report = import_dayone(
                &vault,
                &ImportDayOneRequest {
                    zip_path: zip.to_string_lossy().into_owned(),
                    notebook_id: notebook,
                    dry_run,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some(Commands::ImportNotion {
            path,
            notebook,
            dry_run,
        }) => {
            let vault_dir = resolve_vault_path(cli.vault, None);
            let vault = Vault::open(&vault_dir)?;
            let report = import_notion(
                &vault,
                &ImportNotionRequest {
                    source_path: path.to_string_lossy().into_owned(),
                    notebook_id: notebook,
                    dry_run,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some(Commands::Backup {
            passphrase_env,
            upload_gdrive,
        }) => {
            let vault_dir = resolve_vault_path(cli.vault, None);
            let _vault = Vault::open(&vault_dir)?;
            let passphrase = read_passphrase_env(&passphrase_env)?;
            if upload_gdrive {
                let res =
                    sync_gdrive(&vault_dir, Some(&passphrase), &BackupConfig::default()).await?;
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                let info = create_backup(&vault_dir, &passphrase, &BackupConfig::default())?;
                println!("{}", serde_json::to_string_pretty(&info)?);
            }
        }
        Some(Commands::Restore {
            file,
            target,
            passphrase_env,
            overwrite,
        }) => {
            let vault_dir = resolve_vault_path(cli.vault, None);
            let passphrase = read_passphrase_env(&passphrase_env)?;
            let req = RestoreBackupRequest {
                backup_id_or_path: file.to_string_lossy().into_owned(),
                passphrase,
                target_vault_dir: Some(target.to_string_lossy().into_owned()),
                overwrite_existing: overwrite,
            };
            let resp = restore_backup(&vault_dir, &req)?;
            if resp.restored {
                let restored_vault = Vault::open(&resp.target_vault_dir)?;
                restored_vault.rebuild_index()?;
            }
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
    }

    Ok(())
}

async fn run_server(vault_dir: PathBuf, port: u16, open_window: bool) -> Result<()> {
    let vault = Arc::new(Vault::open(&vault_dir)?);
    let app = build_router(vault);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind local server on http://{addr}"))?;
    let bound = listener.local_addr()?;
    let url = format!("http://{bound}");

    println!("Log server listening at {url} (vault: {})", vault_dir.display());

    if open_window {
        spawn_app_window(&url);
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn spawn_app_window(url: &str) {
    let app_flag = format!("--app={url}");
    let chrome = Command::new("google-chrome-stable")
        .arg(&app_flag)
        .arg("--class=log-journal")
        .spawn();

    if chrome.is_err() {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
