#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Pure Tauri v2 desktop application & CLI (`log-journal`) for **Log**.
//!
//! Supports headless CLI administration (`seed-demo`, `import-dayone`, `import-notion`,
//! `backup`, `restore`) alongside the native GTK/Wayland Tauri v2 desktop window with
//! `vault://` media streaming and all 23 `#[tauri::command]` IPC handlers.

mod media_gen;
mod seed;

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use journal_core::Vault;
use journal_import::{import_dayone as core_import_dayone, import_notion as core_import_notion};
use journal_sync::{
    create_backup as core_create_backup, get_daily_context as core_get_daily_context,
    list_backups as core_list_backups, restore_backup as core_restore_backup,
    sync_gdrive as core_sync_gdrive,
};
use journal_types::{
    AssetManifest, BacklinkRef, BackupConfig, BackupInfo, CalendarDaySummary,
    CreateNotebookRequest, DailyContext, DeleteEntryResponse, Entry, EntrySummary, GeoLocation,
    ImportDayOneRequest, ImportNotionRequest, MigrationReport, Notebook, OnThisDayGroup,
    PolaroidMeta, RestoreBackupRequest, RestoreBackupResponse, SaveEntryRequest, SearchHit,
    SearchQuery, StampPlacement, SyncResult, TagCount, Template, VaultStats, VoiceMemoMeta,
    WeatherInfo,
};
use tauri::{Manager, State};
use uuid::Uuid;

const EMBEDDED_MANIFEST_JSON: &str = include_str!("../../ui/assets/manifest.json");

struct DesktopState {
    vault: Arc<Vault>,
}

#[derive(Debug, Parser)]
#[command(
    name = "log-journal",
    version,
    about = "Log — Local-first personal journal desktop app"
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
    /// Launch the native Tauri v2 desktop window (default).
    Launch,
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

fn err_str(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn resolve_vault_path(cli_vault: Option<PathBuf>, sub_vault: Option<PathBuf>) -> PathBuf {
    sub_vault.or(cli_vault).unwrap_or_else(Vault::default_path)
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

fn resolve_asset_manifest_path() -> Option<PathBuf> {
    let candidates_rel = ["ui/assets/manifest.json", "assets/manifest.json"];
    let mut candidates = Vec::new();
    for rel in candidates_rel {
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(rel));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(rel));
                if let Some(p1) = dir.parent() {
                    candidates.push(p1.join(rel));
                    if let Some(p2) = p1.parent() {
                        candidates.push(p2.join(rel));
                    }
                }
            }
        }
        let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        if let Some(root) = manifest_root.parent() {
            candidates.push(root.join(rel));
        }
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Pure-Rust standard Base64 decoder (RFC 4648, tolerates optional `data:...;base64,` prefix and whitespace).
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let raw = input
        .trim()
        .split_once(";base64,")
        .map(|(_, data)| data)
        .unwrap_or_else(|| input.trim());

    let cleaned: Vec<u8> = raw
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();

    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    if cleaned.len() % 4 != 0 {
        return Err("Invalid base64 payload length".to_string());
    }

    let mut out = Vec::with_capacity((cleaned.len() / 4) * 3);
    for chunk in cleaned.chunks_exact(4) {
        let mut vals = [0u8; 4];
        let mut pad = 0usize;
        for (idx, &ch) in chunk.iter().enumerate() {
            if ch == b'=' {
                if idx < 2 || (idx == 2 && chunk[3] != b'=') {
                    return Err("Invalid base64 padding".to_string());
                }
                pad += 1;
                vals[idx] = 0;
            } else {
                vals[idx] = match ch {
                    b'A'..=b'Z' => ch - b'A',
                    b'a'..=b'z' => ch - b'a' + 26,
                    b'0'..=b'9' => ch - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    _ => return Err(format!("Invalid base64 byte: 0x{ch:02x}")),
                };
            }
        }

        let triple = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | ((vals[2] as u32) << 6)
            | (vals[3] as u32);

        out.push(((triple >> 16) & 0xff) as u8);
        if pad < 2 {
            out.push(((triple >> 8) & 0xff) as u8);
        }
        if pad == 0 {
            out.push((triple & 0xff) as u8);
        }
    }
    Ok(out)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn infer_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("webp") => "image/webp",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("m4a") => "audio/mp4",
        Some("webm") => "audio/webm",
        _ => "application/octet-stream",
    }
}

fn is_safe_relative(rel: &str) -> bool {
    let p = Path::new(rel);
    let mut has_normal = false;
    for comp in p.components() {
        match comp {
            Component::Normal(_) => has_normal = true,
            _ => return false,
        }
    }
    has_normal
}

fn build_vault_protocol_response(
    vault_root: &Path,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    if request.method() == tauri::http::Method::OPTIONS {
        return tauri::http::Response::builder()
            .status(204)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
            .body(Vec::new())
            .unwrap();
    }

    let decoded_path = percent_decode(request.uri().path());
    let target = if let Some(rel) = decoded_path.strip_prefix("/vault-media/") {
        if !is_safe_relative(rel) {
            return error_http_response(403, b"Forbidden path".to_vec());
        }
        vault_root.join("media").join(rel)
    } else if let Some(rel) = decoded_path.strip_prefix("/vault-thumbs/") {
        if !is_safe_relative(rel) {
            return error_http_response(403, b"Forbidden path".to_vec());
        }
        vault_root.join(".index").join("thumbs").join(rel)
    } else {
        return error_http_response(404, b"Not found".to_vec());
    };

    let Ok(bytes) = fs::read(&target) else {
        return error_http_response(404, b"Media file not found".to_vec());
    };

    let mime = infer_mime_type(&target);
    let total = bytes.len();

    if let Some(range_hdr) = request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(spec) = range_hdr.strip_prefix("bytes=") {
            if let Some((start_s, end_s)) = spec.split_once('-') {
                if let Ok(start) = start_s.trim().parse::<usize>() {
                    if start < total {
                        let end = if end_s.trim().is_empty() {
                            total - 1
                        } else {
                            end_s
                                .trim()
                                .parse::<usize>()
                                .unwrap_or(total - 1)
                                .min(total - 1)
                        };
                        if start <= end {
                            let slice = bytes[start..=end].to_vec();
                            let content_range = format!("bytes {start}-{end}/{total}");
                            return tauri::http::Response::builder()
                                .status(206)
                                .header("Content-Type", mime)
                                .header("Accept-Ranges", "bytes")
                                .header("Content-Range", content_range)
                                .header("Access-Control-Allow-Origin", "*")
                                .body(slice)
                                .unwrap();
                        }
                    }
                }
            }
        }
    }

    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .body(bytes)
        .unwrap()
}

fn error_http_response(status: u16, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap()
}

// ---------------------------------------------------------------------------
// 23 Tauri v2 IPC Commands (exact snake_case contract with ui/src/ipc.js)
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
fn get_vault_stats(state: State<'_, DesktopState>) -> Result<VaultStats, String> {
    state.vault.get_vault_stats().map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn list_notebooks(state: State<'_, DesktopState>) -> Result<Vec<Notebook>, String> {
    state.vault.list_notebooks().map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn create_notebook(
    state: State<'_, DesktopState>,
    name: String,
    description: Option<String>,
    cover_asset: Option<String>,
    accent_color: Option<String>,
) -> Result<Notebook, String> {
    state
        .vault
        .create_notebook(CreateNotebookRequest {
            name,
            description,
            cover_asset,
            accent_color,
        })
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn list_entries(
    state: State<'_, DesktopState>,
    notebook_id: Option<String>,
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<EntrySummary>, String> {
    state
        .vault
        .list_entries(notebook_id.as_deref(), tag.as_deref(), limit, offset)
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn get_entry(state: State<'_, DesktopState>, id: String) -> Result<Entry, String> {
    state.vault.get_entry(&id).map_err(err_str)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "snake_case")]
fn save_entry(
    state: State<'_, DesktopState>,
    id: Option<Uuid>,
    title: String,
    date: Option<DateTime<Utc>>,
    notebook_id: Option<String>,
    tags: Option<Vec<String>>,
    mood: Option<String>,
    starred: Option<bool>,
    weather: Option<WeatherInfo>,
    location: Option<GeoLocation>,
    stamps: Option<Vec<StampPlacement>>,
    photos: Option<Vec<PolaroidMeta>>,
    audio: Option<Vec<VoiceMemoMeta>>,
    body: String,
) -> Result<Entry, String> {
    state
        .vault
        .save_entry(SaveEntryRequest {
            id,
            title,
            date,
            notebook_id,
            tags: tags.unwrap_or_default(),
            mood,
            starred: starred.unwrap_or(false),
            weather,
            location,
            stamps: stamps.unwrap_or_default(),
            photos: photos.unwrap_or_default(),
            audio: audio.unwrap_or_default(),
            body,
        })
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn delete_entry(state: State<'_, DesktopState>, id: Uuid) -> Result<DeleteEntryResponse, String> {
    state.vault.delete_entry(id).map_err(err_str)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "snake_case")]
fn search_entries(
    state: State<'_, DesktopState>,
    text: Option<String>,
    notebook_id: Option<String>,
    tag: Option<String>,
    mood: Option<String>,
    starred_only: Option<bool>,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    state
        .vault
        .search_entries(&SearchQuery {
            text,
            notebook_id,
            tag,
            mood,
            starred_only: starred_only.unwrap_or(false),
            from_date,
            to_date,
            limit,
            offset,
        })
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn list_tags(state: State<'_, DesktopState>) -> Result<Vec<TagCount>, String> {
    state.vault.list_tags().map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn get_calendar_month(
    state: State<'_, DesktopState>,
    year: Option<i32>,
    month: Option<u32>,
    notebook_id: Option<String>,
) -> Result<Vec<CalendarDaySummary>, String> {
    let today = Utc::now().date_naive();
    state
        .vault
        .get_calendar_month(
            year.unwrap_or_else(|| today.year()),
            month.unwrap_or_else(|| today.month()),
            notebook_id.as_deref(),
        )
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn get_on_this_day(
    state: State<'_, DesktopState>,
    month: Option<u32>,
    day: Option<u32>,
) -> Result<Vec<OnThisDayGroup>, String> {
    let today = Utc::now().date_naive();
    state
        .vault
        .get_on_this_day(
            month.unwrap_or_else(|| today.month()),
            day.unwrap_or_else(|| today.day()),
            today.year(),
        )
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn get_backlinks(
    state: State<'_, DesktopState>,
    entry_id: Uuid,
) -> Result<Vec<BacklinkRef>, String> {
    state.vault.get_backlinks(entry_id).map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn list_templates(state: State<'_, DesktopState>) -> Result<Vec<Template>, String> {
    state.vault.list_templates().map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn get_daily_context(
    state: State<'_, DesktopState>,
    date: Option<NaiveDate>,
    lat: Option<f64>,
    lng: Option<f64>,
) -> Result<DailyContext, String> {
    let vault = Arc::clone(&state.vault);
    let target_date = date.unwrap_or_else(|| Utc::now().date_naive());
    core_get_daily_context(&vault, target_date, lat, lng)
        .await
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn get_asset_manifest() -> Result<AssetManifest, String> {
    if let Some(path) = resolve_asset_manifest_path() {
        let raw = fs::read_to_string(&path).map_err(err_str)?;
        return serde_json::from_str(&raw).map_err(err_str);
    }
    serde_json::from_str(EMBEDDED_MANIFEST_JSON).map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn import_dayone(
    state: State<'_, DesktopState>,
    zip_path: String,
    notebook_id: Option<String>,
    dry_run: Option<bool>,
) -> Result<MigrationReport, String> {
    core_import_dayone(
        &state.vault,
        &ImportDayOneRequest {
            zip_path,
            notebook_id,
            dry_run: dry_run.unwrap_or(false),
        },
    )
    .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn import_notion(
    state: State<'_, DesktopState>,
    source_path: String,
    notebook_id: Option<String>,
    dry_run: Option<bool>,
) -> Result<MigrationReport, String> {
    core_import_notion(
        &state.vault,
        &ImportNotionRequest {
            source_path,
            notebook_id,
            dry_run: dry_run.unwrap_or(false),
        },
    )
    .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn list_backups(state: State<'_, DesktopState>) -> Result<Vec<BackupInfo>, String> {
    core_list_backups(state.vault.root()).map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn create_backup(
    state: State<'_, DesktopState>,
    passphrase: String,
    upload_gdrive: Option<bool>,
) -> Result<BackupInfo, String> {
    let vault = Arc::clone(&state.vault);
    if upload_gdrive.unwrap_or(false) {
        let res = core_sync_gdrive(vault.root(), Some(&passphrase), &BackupConfig::default())
            .await
            .map_err(err_str)?;
        res.backup
            .ok_or_else(|| "Backup completed without metadata".to_string())
    } else {
        core_create_backup(vault.root(), &passphrase, &BackupConfig::default()).map_err(err_str)
    }
}

#[tauri::command(rename_all = "snake_case")]
fn restore_backup(
    state: State<'_, DesktopState>,
    backup_id_or_path: String,
    passphrase: String,
    target_vault_dir: Option<String>,
    overwrite_existing: Option<bool>,
) -> Result<RestoreBackupResponse, String> {
    let resp = core_restore_backup(
        state.vault.root(),
        &RestoreBackupRequest {
            backup_id_or_path,
            passphrase,
            target_vault_dir,
            overwrite_existing: overwrite_existing.unwrap_or(true),
        },
    )
    .map_err(err_str)?;
    if resp.restored {
        if PathBuf::from(&resp.target_vault_dir) == state.vault.root() {
            let _ = state.vault.rebuild_index();
        } else if let Ok(restored_vault) = Vault::open(&resp.target_vault_dir) {
            let _ = restored_vault.rebuild_index();
        }
    }
    Ok(resp)
}

#[tauri::command(rename_all = "snake_case")]
async fn sync_gdrive(
    state: State<'_, DesktopState>,
    passphrase: Option<String>,
    config: Option<BackupConfig>,
) -> Result<SyncResult, String> {
    let vault = Arc::clone(&state.vault);
    core_sync_gdrive(
        vault.root(),
        passphrase.as_deref(),
        &config.unwrap_or_default(),
    )
    .await
    .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn upload_photo(
    state: State<'_, DesktopState>,
    filename: String,
    data_base64: String,
    caption: Option<String>,
) -> Result<PolaroidMeta, String> {
    let bytes = decode_base64(&data_base64)?;
    state
        .vault
        .ingest_photo(&filename, &bytes, caption)
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
fn upload_audio(
    state: State<'_, DesktopState>,
    filename: String,
    data_base64: String,
    duration_secs: Option<f32>,
    mime_type: Option<String>,
) -> Result<VoiceMemoMeta, String> {
    let bytes = decode_base64(&data_base64)?;
    state
        .vault
        .ingest_audio(&filename, &bytes, duration_secs, mime_type)
        .map_err(err_str)
}

// ---------------------------------------------------------------------------
// Headless CLI Execution & Tauri v2 Desktop Entrypoint
// ---------------------------------------------------------------------------

fn run_cli_subcommand(cli_vault: Option<PathBuf>, cmd: Commands) -> Result<()> {
    match cmd {
        Commands::Launch => unreachable!("launch uses desktop runtime"),
        Commands::SeedDemo { vault: sub_vault } => {
            let vault_dir = resolve_vault_path(cli_vault, sub_vault);
            let vault = Vault::open(&vault_dir)?;
            let summary = seed::seed_demo_vault(&vault)?;
            println!(
                "Seeded showcase vault at {} ({} notebooks added, {} entries seeded)",
                summary.vault_path, summary.notebooks_created, summary.entries_seeded
            );
        }
        Commands::ImportDayone {
            zip,
            notebook,
            dry_run,
        } => {
            let vault_dir = resolve_vault_path(cli_vault, None);
            let vault = Vault::open(&vault_dir)?;
            let report = core_import_dayone(
                &vault,
                &ImportDayOneRequest {
                    zip_path: zip.to_string_lossy().into_owned(),
                    notebook_id: notebook,
                    dry_run,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::ImportNotion {
            path,
            notebook,
            dry_run,
        } => {
            let vault_dir = resolve_vault_path(cli_vault, None);
            let vault = Vault::open(&vault_dir)?;
            let report = core_import_notion(
                &vault,
                &ImportNotionRequest {
                    source_path: path.to_string_lossy().into_owned(),
                    notebook_id: notebook,
                    dry_run,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::Backup {
            passphrase_env,
            upload_gdrive,
        } => {
            let vault_dir = resolve_vault_path(cli_vault, None);
            let _vault = Vault::open(&vault_dir)?;
            let passphrase = read_passphrase_env(&passphrase_env)?;
            if upload_gdrive {
                let rt = tokio::runtime::Runtime::new()?;
                let res = rt.block_on(core_sync_gdrive(
                    &vault_dir,
                    Some(&passphrase),
                    &BackupConfig::default(),
                ))?;
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                let info = core_create_backup(&vault_dir, &passphrase, &BackupConfig::default())?;
                println!("{}", serde_json::to_string_pretty(&info)?);
            }
        }
        Commands::Restore {
            file,
            target,
            passphrase_env,
            overwrite,
        } => {
            let vault_dir = resolve_vault_path(cli_vault, None);
            let passphrase = read_passphrase_env(&passphrase_env)?;
            let req = RestoreBackupRequest {
                backup_id_or_path: file.to_string_lossy().into_owned(),
                passphrase,
                target_vault_dir: Some(target.to_string_lossy().into_owned()),
                overwrite_existing: overwrite,
            };
            let resp = core_restore_backup(&vault_dir, &req)?;
            if resp.restored {
                let restored_vault = Vault::open(&resp.target_vault_dir)?;
                restored_vault.rebuild_index()?;
            }
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Launch) => {
            // Guard against WebKitGTK Wayland DMABUF blank-canvas rendering bugs on Arch/Linux.
            if std::env::var_os("WAYLAND_DISPLAY").is_some()
                && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
            {
                unsafe {
                    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
                }
            }

            let vault_dir = resolve_vault_path(cli.vault, None);
            let vault = Vault::open(&vault_dir).expect("failed to initialize local journal vault");
            let protocol_vault_root = vault.root().to_path_buf();
            let state = DesktopState {
                vault: Arc::new(vault),
            };

            tauri::Builder::default()
                .register_uri_scheme_protocol("vault", move |_ctx, request| {
                    build_vault_protocol_response(&protocol_vault_root, &request)
                })
                .setup(|app| {
                    app.manage(state);
                    Ok(())
                })
                .invoke_handler(tauri::generate_handler![
                    get_vault_stats,
                    list_notebooks,
                    create_notebook,
                    list_entries,
                    get_entry,
                    save_entry,
                    delete_entry,
                    search_entries,
                    list_tags,
                    get_calendar_month,
                    get_on_this_day,
                    get_backlinks,
                    list_templates,
                    get_daily_context,
                    get_asset_manifest,
                    import_dayone,
                    import_notion,
                    list_backups,
                    create_backup,
                    restore_backup,
                    sync_gdrive,
                    upload_photo,
                    upload_audio,
                ])
                .run(tauri::generate_context!())
                .expect("error while running Log desktop application");
        }
        Some(cmd) => {
            if let Err(err) = run_cli_subcommand(cli.vault, cmd) {
                eprintln!("Error: {err:#}");
                std::process::exit(1);
            }
        }
    }
}
