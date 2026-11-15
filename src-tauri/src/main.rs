#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Pure Tauri v2 desktop application & CLI (`log-journal`) for **Log**.
//!
//! Supports headless CLI administration (`seed-demo`, `import-dayone`, `import-notion`,
//! `backup`, `restore`, `sync`) alongside the native GTK/Wayland Tauri v2 desktop window
//! with `vault://` media streaming and all 35 `#[tauri::command]` IPC handlers.

mod audio_rec;
mod desktop;
mod media_gen;
mod seed;

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::{bail, Context, Result};
use audio_rec::RecorderManager;
use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use journal_core::Vault;
use journal_import::{import_dayone as core_import_dayone, import_notion as core_import_notion};
use journal_sync::{
    connect_gdrive_oauth as core_connect_gdrive_oauth,
    connect_gdrive_rclone as core_connect_gdrive_rclone, create_backup as core_create_backup,
    disconnect_gdrive as core_disconnect_gdrive, get_daily_context as core_get_daily_context,
    get_gdrive_status as core_get_gdrive_status, list_backups as core_list_backups,
    load_backup_config as core_load_backup_config, restore_backup as core_restore_backup,
    save_backup_config as core_save_backup_config, save_gdrive_token as core_save_gdrive_token,
    sync_gdrive_with_target as core_sync_gdrive_with_target,
};
use journal_types::{
    AssetManifest, BacklinkRef, BackupConfig, BackupInfo, CalendarDaySummary,
    ConnectGdriveOauthRequest, CreateNotebookRequest, DailyContext, DeleteEntryResponse,
    DeleteNotebookResponse, Entry, EntrySummary, GdriveAuthStatus, GeoLocation,
    ImportDayOneRequest, ImportNotionRequest, MigrationReport, Notebook, OnThisDayGroup,
    PolaroidMeta, RestoreBackupRequest, RestoreBackupResponse, SaveEntryRequest, SearchHit,
    SearchQuery, StampPlacement, SyncResult, TagCount, Template, UpdateNotebookRequest, VaultStats,
    VoiceMemoMeta, WeatherInfo,
};
use tauri::{Manager, State};
use uuid::Uuid;

const EMBEDDED_MANIFEST_JSON: &str = include_str!("../../ui/assets/manifest.json");
static ASSET_MANIFEST_CACHE: OnceLock<AssetManifest> = OnceLock::new();

struct DesktopState {
    vault: Arc<Vault>,
    recorder: Arc<RecorderManager>,
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

    /// Disable WebKitGTK GPU DMA-BUF rendering (fallback for older Wayland drivers).
    #[arg(long, global = true)]
    software_rendering: bool,

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
        #[arg(
            long,
            default_value_t = true,
            num_args = 0..=1,
            default_missing_value = "true",
            action = clap::ArgAction::Set
        )]
        overwrite: bool,
    },
    /// Upload an existing encrypted backup (or newest snapshot) to Google Drive.
    Sync {
        /// Optional backup ID or filename to upload (defaults to newest snapshot).
        #[arg(long, value_name = "ID")]
        backup: Option<String>,
        /// Target Google Drive folder name override.
        #[arg(long, value_name = "NAME")]
        folder: Option<String>,
    },
    /// Install the binary, hicolor icons, and Freedesktop `.desktop` launcher entry.
    InstallDesktop {
        /// Target directory for the executable (defaults to $XDG_BIN_HOME or ~/.local/bin).
        #[arg(long, value_name = "DIR")]
        bin_dir: Option<PathBuf>,
        /// Base XDG data directory for icons and desktop file (defaults to $XDG_DATA_HOME or ~/.local/share).
        #[arg(long, value_name = "DIR")]
        data_dir: Option<PathBuf>,
    },
    /// Remove the installed binary, hicolor icons, and Freedesktop `.desktop` launcher entry.
    UninstallDesktop {
        /// Target directory for the executable (defaults to $XDG_BIN_HOME or ~/.local/bin).
        #[arg(long, value_name = "DIR")]
        bin_dir: Option<PathBuf>,
        /// Base XDG data directory for icons and desktop file (defaults to $XDG_DATA_HOME or ~/.local/share).
        #[arg(long, value_name = "DIR")]
        data_dir: Option<PathBuf>,
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

/// Pure-Rust single-pass standard Base64 decoder (RFC 4648, tolerates optional `data:...;base64,` prefix and whitespace).
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let raw = input
        .trim()
        .split_once(";base64,")
        .map(|(_, data)| data)
        .unwrap_or_else(|| input.trim());

    let mut out = Vec::with_capacity((raw.len() / 4) * 3);
    let mut quad = [0u8; 4];
    let mut qlen = 0usize;
    let mut finished = false;

    for byte in raw.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if finished {
            return Err("Invalid base64 padding".to_string());
        }
        quad[qlen] = byte;
        qlen += 1;
        if qlen < 4 {
            continue;
        }
        qlen = 0;

        let mut vals = [0u8; 4];
        let mut pad = 0usize;
        for (idx, &ch) in quad.iter().enumerate() {
            if ch == b'=' {
                if idx < 2 || (idx == 2 && quad[3] != b'=') {
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

        if pad > 0 {
            finished = true;
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

    if qlen != 0 {
        return Err("Invalid base64 payload length".to_string());
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

enum ParsedRange {
    Bounds { start: u64, end: u64 },
    Unsatisfiable,
}

fn parse_byte_range(range_hdr: &str, total: u64) -> Option<ParsedRange> {
    let spec = range_hdr.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start_s, end_s) = spec.split_once('-')?;
    let start_s = start_s.trim();
    let end_s = end_s.trim();

    if total == 0 {
        return Some(ParsedRange::Unsatisfiable);
    }

    if start_s.is_empty() {
        let suffix = end_s.parse::<u64>().ok()?;
        if suffix == 0 {
            return Some(ParsedRange::Unsatisfiable);
        }
        let start = total.saturating_sub(suffix);
        return Some(ParsedRange::Bounds {
            start,
            end: total - 1,
        });
    }

    let start = start_s.parse::<u64>().ok()?;
    if start >= total {
        return Some(ParsedRange::Unsatisfiable);
    }

    let end = if end_s.is_empty() {
        total - 1
    } else {
        let requested_end = end_s.parse::<u64>().ok()?;
        if requested_end < start {
            return Some(ParsedRange::Unsatisfiable);
        }
        requested_end.min(total - 1)
    };

    Some(ParsedRange::Bounds { start, end })
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

    let is_head = request.method() == tauri::http::Method::HEAD;
    if !is_head && request.method() != tauri::http::Method::GET {
        return tauri::http::Response::builder()
            .status(405)
            .header("Allow", "GET, HEAD, OPTIONS")
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Access-Control-Allow-Origin", "*")
            .body(b"Method not allowed".to_vec())
            .unwrap();
    }

    let decoded_path = percent_decode(request.uri().path());
    let (base_dir, rel) = if let Some(rel) = decoded_path.strip_prefix("/vault-media/") {
        (vault_root.join("media"), rel)
    } else if let Some(rel) = decoded_path.strip_prefix("/vault-thumbs/") {
        (vault_root.join(".index").join("thumbs"), rel)
    } else {
        return error_http_response(404, b"Not found".to_vec());
    };

    if !is_safe_relative(rel) {
        return error_http_response(403, b"Forbidden path".to_vec());
    }

    let Ok(canon_base) = fs::canonicalize(&base_dir) else {
        return error_http_response(404, b"Media file not found".to_vec());
    };
    let Ok(canon_target) = fs::canonicalize(base_dir.join(rel)) else {
        return error_http_response(404, b"Media file not found".to_vec());
    };
    if !canon_target.starts_with(&canon_base) {
        return error_http_response(403, b"Forbidden path".to_vec());
    }

    let Ok(mut file) = File::open(&canon_target) else {
        return error_http_response(404, b"Media file not found".to_vec());
    };
    let Ok(meta) = file.metadata() else {
        return error_http_response(404, b"Media file not found".to_vec());
    };
    if !meta.is_file() {
        return error_http_response(404, b"Media file not found".to_vec());
    }

    let mime = infer_mime_type(&canon_target);
    let total = meta.len();

    if let Some(range_hdr) = request.headers().get("range").and_then(|v| v.to_str().ok()) {
        if let Some(parsed) = parse_byte_range(range_hdr, total) {
            match parsed {
                ParsedRange::Unsatisfiable => {
                    return tauri::http::Response::builder()
                        .status(416)
                        .header("Content-Range", format!("bytes */{total}"))
                        .header("Accept-Ranges", "bytes")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Vec::new())
                        .unwrap();
                }
                ParsedRange::Bounds { start, end } => {
                    let len = end - start + 1;
                    let body = if is_head {
                        Vec::new()
                    } else {
                        if file.seek(SeekFrom::Start(start)).is_err() {
                            return error_http_response(500, b"Failed to seek media file".to_vec());
                        }
                        let cap = usize::try_from(len)
                            .unwrap_or(16 * 1024 * 1024)
                            .min(16 * 1024 * 1024);
                        let mut slice = Vec::with_capacity(cap);
                        if file.take(len).read_to_end(&mut slice).is_err() {
                            return error_http_response(
                                500,
                                b"Failed to read media range".to_vec(),
                            );
                        }
                        slice
                    };
                    let content_range = format!("bytes {start}-{end}/{total}");
                    return tauri::http::Response::builder()
                        .status(206)
                        .header("Content-Type", mime)
                        .header("Content-Length", len.to_string())
                        .header("Accept-Ranges", "bytes")
                        .header("Content-Range", content_range)
                        .header("Cache-Control", "private, max-age=31536000, immutable")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(body)
                        .unwrap();
                }
            }
        }
    }

    let body = if is_head {
        Vec::new()
    } else {
        let cap = usize::try_from(total)
            .unwrap_or(16 * 1024 * 1024)
            .min(16 * 1024 * 1024);
        let mut bytes = Vec::with_capacity(cap);
        if file.read_to_end(&mut bytes).is_err() {
            return error_http_response(500, b"Failed to read media file".to_vec());
        }
        bytes
    };

    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", mime)
        .header("Content-Length", total.to_string())
        .header("Accept-Ranges", "bytes")
        .header("Cache-Control", "private, max-age=31536000, immutable")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
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

async fn spawn_vault<T, F>(vault: &Arc<Vault>, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Arc<Vault>) -> Result<T, String> + Send + 'static,
{
    let vault = Arc::clone(vault);
    tauri::async_runtime::spawn_blocking(move || f(vault))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

fn load_asset_manifest_from_disk_or_embedded() -> Result<AssetManifest, String> {
    if let Some(cached) = ASSET_MANIFEST_CACHE.get() {
        return Ok(cached.clone());
    }
    let manifest: AssetManifest = if let Some(path) = resolve_asset_manifest_path() {
        let raw = fs::read_to_string(&path).map_err(err_str)?;
        serde_json::from_str(&raw).map_err(err_str)?
    } else {
        serde_json::from_str(EMBEDDED_MANIFEST_JSON).map_err(err_str)?
    };
    let _ = ASSET_MANIFEST_CACHE.set(manifest.clone());
    Ok(manifest)
}

#[tauri::command(rename_all = "snake_case")]
async fn get_vault_stats(state: State<'_, DesktopState>) -> Result<VaultStats, String> {
    spawn_vault(&state.vault, |vault| {
        vault.get_vault_stats().map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn list_notebooks(state: State<'_, DesktopState>) -> Result<Vec<Notebook>, String> {
    spawn_vault(&state.vault, |vault| {
        vault.list_notebooks().map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn create_notebook(
    state: State<'_, DesktopState>,
    name: String,
    description: Option<String>,
    cover_asset: Option<String>,
    accent_color: Option<String>,
) -> Result<Notebook, String> {
    spawn_vault(&state.vault, move |vault| {
        vault
            .create_notebook(CreateNotebookRequest {
                name,
                description,
                cover_asset,
                accent_color,
            })
            .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn update_notebook(
    state: State<'_, DesktopState>,
    id: String,
    name: String,
    description: Option<String>,
    cover_asset: Option<String>,
    accent_color: Option<String>,
) -> Result<Notebook, String> {
    spawn_vault(&state.vault, move |vault| {
        vault
            .update_notebook(UpdateNotebookRequest {
                id,
                name,
                description,
                cover_asset,
                accent_color,
            })
            .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn delete_notebook(
    state: State<'_, DesktopState>,
    id: String,
    delete_entries: Option<bool>,
) -> Result<DeleteNotebookResponse, String> {
    spawn_vault(&state.vault, move |vault| {
        vault
            .delete_notebook(&id, delete_entries.unwrap_or(false))
            .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn list_entries(
    state: State<'_, DesktopState>,
    notebook_id: Option<String>,
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<EntrySummary>, String> {
    spawn_vault(&state.vault, move |vault| {
        vault
            .list_entries(notebook_id.as_deref(), tag.as_deref(), limit, offset)
            .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_entry(state: State<'_, DesktopState>, id: String) -> Result<Entry, String> {
    spawn_vault(&state.vault, move |vault| {
        vault.get_entry(&id).map_err(err_str)
    })
    .await
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "snake_case")]
async fn save_entry(
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
    spawn_vault(&state.vault, move |vault| {
        vault
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
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn delete_entry(
    state: State<'_, DesktopState>,
    id: Uuid,
) -> Result<DeleteEntryResponse, String> {
    spawn_vault(&state.vault, move |vault| {
        vault.delete_entry(id).map_err(err_str)
    })
    .await
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "snake_case")]
async fn search_entries(
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
    spawn_vault(&state.vault, move |vault| {
        vault
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
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn list_tags(state: State<'_, DesktopState>) -> Result<Vec<TagCount>, String> {
    spawn_vault(&state.vault, |vault| vault.list_tags().map_err(err_str)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_calendar_month(
    state: State<'_, DesktopState>,
    year: Option<i32>,
    month: Option<u32>,
    notebook_id: Option<String>,
) -> Result<Vec<CalendarDaySummary>, String> {
    let today = Local::now().date_naive();
    let y = year.unwrap_or_else(|| today.year());
    let m = month.unwrap_or_else(|| today.month());
    spawn_vault(&state.vault, move |vault| {
        vault
            .get_calendar_month(y, m, notebook_id.as_deref())
            .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_on_this_day(
    state: State<'_, DesktopState>,
    month: Option<u32>,
    day: Option<u32>,
) -> Result<Vec<OnThisDayGroup>, String> {
    let today = Local::now().date_naive();
    let m = month.unwrap_or_else(|| today.month());
    let d = day.unwrap_or_else(|| today.day());
    let y = today.year();
    spawn_vault(&state.vault, move |vault| {
        vault.get_on_this_day(m, d, y).map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_backlinks(
    state: State<'_, DesktopState>,
    entry_id: Uuid,
) -> Result<Vec<BacklinkRef>, String> {
    spawn_vault(&state.vault, move |vault| {
        vault.get_backlinks(entry_id).map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn list_templates(state: State<'_, DesktopState>) -> Result<Vec<Template>, String> {
    spawn_vault(&state.vault, |vault| {
        vault.list_templates().map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_daily_context(
    state: State<'_, DesktopState>,
    date: Option<NaiveDate>,
    lat: Option<f64>,
    lng: Option<f64>,
) -> Result<DailyContext, String> {
    let vault = Arc::clone(&state.vault);
    let target_date = date.unwrap_or_else(|| Local::now().date_naive());
    core_get_daily_context(&vault, target_date, lat, lng)
        .await
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn get_asset_manifest() -> Result<AssetManifest, String> {
    if let Some(cached) = ASSET_MANIFEST_CACHE.get() {
        return Ok(cached.clone());
    }
    tauri::async_runtime::spawn_blocking(load_asset_manifest_from_disk_or_embedded)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command(rename_all = "snake_case")]
async fn import_dayone(
    state: State<'_, DesktopState>,
    zip_path: String,
    notebook_id: Option<String>,
    dry_run: Option<bool>,
) -> Result<MigrationReport, String> {
    spawn_vault(&state.vault, move |vault| {
        core_import_dayone(
            &vault,
            &ImportDayOneRequest {
                zip_path,
                notebook_id,
                dry_run: dry_run.unwrap_or(false),
            },
        )
        .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn import_notion(
    state: State<'_, DesktopState>,
    source_path: String,
    notebook_id: Option<String>,
    dry_run: Option<bool>,
) -> Result<MigrationReport, String> {
    spawn_vault(&state.vault, move |vault| {
        core_import_notion(
            &vault,
            &ImportNotionRequest {
                source_path,
                notebook_id,
                dry_run: dry_run.unwrap_or(false),
            },
        )
        .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn list_backups(state: State<'_, DesktopState>) -> Result<Vec<BackupInfo>, String> {
    spawn_vault(&state.vault, |vault| {
        core_list_backups(vault.root()).map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn create_backup(
    state: State<'_, DesktopState>,
    passphrase: String,
    upload_gdrive: Option<bool>,
) -> Result<BackupInfo, String> {
    let cfg = tauri::async_runtime::spawn_blocking(core_load_backup_config)
        .await
        .map_err(err_str)?;
    if upload_gdrive.unwrap_or(false) {
        let vault = Arc::clone(&state.vault);
        let res = core_sync_gdrive_with_target(vault.root(), Some(&passphrase), None, &cfg)
            .await
            .map_err(err_str)?;
        res.backup
            .ok_or_else(|| "Backup completed without metadata".to_string())
    } else {
        spawn_vault(&state.vault, move |vault| {
            core_create_backup(vault.root(), &passphrase, &cfg).map_err(err_str)
        })
        .await
    }
}

#[tauri::command(rename_all = "snake_case")]
async fn restore_backup(
    state: State<'_, DesktopState>,
    backup_id_or_path: String,
    passphrase: String,
    target_vault_dir: Option<String>,
    overwrite_existing: Option<bool>,
) -> Result<RestoreBackupResponse, String> {
    spawn_vault(&state.vault, move |vault| {
        let resp = core_restore_backup(
            vault.root(),
            &RestoreBackupRequest {
                backup_id_or_path,
                passphrase,
                target_vault_dir,
                overwrite_existing: overwrite_existing.unwrap_or(true),
            },
        )
        .map_err(err_str)?;
        if resp.restored {
            if Path::new(&resp.target_vault_dir) == vault.root() {
                let _ = vault.rebuild_index();
            } else if let Ok(restored_vault) = Vault::open(&resp.target_vault_dir) {
                let _ = restored_vault.rebuild_index();
            }
        }
        Ok(resp)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn sync_gdrive(
    state: State<'_, DesktopState>,
    passphrase: Option<String>,
    backup_id: Option<String>,
    config: Option<BackupConfig>,
) -> Result<SyncResult, String> {
    let vault = Arc::clone(&state.vault);
    let cfg = match config {
        Some(c) => c,
        None => tauri::async_runtime::spawn_blocking(core_load_backup_config)
            .await
            .map_err(err_str)?,
    };
    core_sync_gdrive_with_target(
        vault.root(),
        passphrase.as_deref(),
        backup_id.as_deref(),
        &cfg,
    )
    .await
    .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn get_backup_config() -> Result<BackupConfig, String> {
    tauri::async_runtime::spawn_blocking(core_load_backup_config)
        .await
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn save_backup_config(config: BackupConfig) -> Result<BackupConfig, String> {
    tauri::async_runtime::spawn_blocking(move || core_save_backup_config(&config))
        .await
        .map_err(err_str)?
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn get_gdrive_status(config: Option<BackupConfig>) -> Result<GdriveAuthStatus, String> {
    let cfg = match config {
        Some(c) => c,
        None => tauri::async_runtime::spawn_blocking(core_load_backup_config)
            .await
            .map_err(err_str)?,
    };
    Ok(core_get_gdrive_status(&cfg).await)
}

#[tauri::command(rename_all = "snake_case")]
async fn connect_gdrive_rclone(remote_name: Option<String>) -> Result<GdriveAuthStatus, String> {
    core_connect_gdrive_rclone(remote_name.as_deref())
        .await
        .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn connect_gdrive_oauth(
    client_id: String,
    client_secret: Option<String>,
) -> Result<GdriveAuthStatus, String> {
    core_connect_gdrive_oauth(&ConnectGdriveOauthRequest {
        client_id,
        client_secret,
    })
    .await
    .map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn save_gdrive_token(access_token: String) -> Result<GdriveAuthStatus, String> {
    core_save_gdrive_token(&access_token).await.map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn disconnect_gdrive() -> Result<GdriveAuthStatus, String> {
    core_disconnect_gdrive().await.map_err(err_str)
}

#[tauri::command(rename_all = "snake_case")]
async fn upload_photo(
    state: State<'_, DesktopState>,
    filename: String,
    data_base64: String,
    caption: Option<String>,
) -> Result<PolaroidMeta, String> {
    spawn_vault(&state.vault, move |vault| {
        let bytes = decode_base64(&data_base64)?;
        vault
            .ingest_photo(&filename, &bytes, caption)
            .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn upload_audio(
    state: State<'_, DesktopState>,
    filename: String,
    data_base64: String,
    duration_secs: Option<f32>,
    mime_type: Option<String>,
) -> Result<VoiceMemoMeta, String> {
    spawn_vault(&state.vault, move |vault| {
        let bytes = decode_base64(&data_base64)?;
        vault
            .ingest_audio(&filename, &bytes, duration_secs, mime_type)
            .map_err(err_str)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn start_voice_recording(state: State<'_, DesktopState>) -> Result<String, String> {
    let recorder = Arc::clone(&state.recorder);
    tauri::async_runtime::spawn_blocking(move || recorder.start())
        .await
        .map_err(|e| format!("Audio recorder task failed: {e}"))?
}

#[tauri::command(rename_all = "snake_case")]
async fn stop_voice_recording(state: State<'_, DesktopState>) -> Result<VoiceMemoMeta, String> {
    let recorder = Arc::clone(&state.recorder);
    let vault = Arc::clone(&state.vault);
    tauri::async_runtime::spawn_blocking(move || recorder.stop(&vault))
        .await
        .map_err(|e| format!("Audio recorder stop task failed: {e}"))?
}

#[tauri::command(rename_all = "snake_case")]
async fn cancel_voice_recording(state: State<'_, DesktopState>) -> Result<(), String> {
    let recorder = Arc::clone(&state.recorder);
    tauri::async_runtime::spawn_blocking(move || recorder.cancel())
        .await
        .map_err(|e| format!("Audio recorder cancel task failed: {e}"))?
}

#[cfg(target_os = "linux")]
fn configure_linux_webview_media(window: &tauri::WebviewWindow) {
    use webkit2gtk::glib::prelude::*;
    use webkit2gtk::{
        PermissionRequestExt, SettingsExt, UserMediaPermissionRequest,
        UserMediaPermissionRequestExt, WebViewExt,
    };

    let _ = window.with_webview(|wv| {
        let webview = wv.inner();
        if let Some(settings) = webview.settings() {
            settings.set_enable_media_stream(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_webaudio(true);
        }
        webview.connect_permission_request(|_wv, req| {
            if let Some(media_req) = req.downcast_ref::<UserMediaPermissionRequest>() {
                if media_req.is_for_audio_device() && !media_req.is_for_video_device() {
                    req.allow();
                    return true;
                }
            }
            false
        });
    });
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
            let cfg = core_load_backup_config();
            if upload_gdrive {
                let rt = tokio::runtime::Runtime::new()?;
                let res = rt.block_on(core_sync_gdrive_with_target(
                    &vault_dir,
                    Some(&passphrase),
                    None,
                    &cfg,
                ))?;
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                let info = core_create_backup(&vault_dir, &passphrase, &cfg)?;
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
        Commands::Sync { backup, folder } => {
            let vault_dir = resolve_vault_path(cli_vault, None);
            let _vault = Vault::open(&vault_dir)?;
            let mut cfg = core_load_backup_config();
            if let Some(folder_name) = folder {
                if !folder_name.trim().is_empty() {
                    cfg.gdrive_folder = folder_name.trim().to_string();
                }
            }
            let rt = tokio::runtime::Runtime::new()?;
            let res = rt.block_on(core_sync_gdrive_with_target(
                &vault_dir,
                None,
                backup.as_deref(),
                &cfg,
            ))?;
            println!("{}", serde_json::to_string_pretty(&res)?);
            if !res.uploaded_remote {
                bail!("{}", res.message);
            }
        }
        Commands::InstallDesktop { bin_dir, data_dir } => {
            let report = desktop::install_desktop(bin_dir, data_dir)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::UninstallDesktop { bin_dir, data_dir } => {
            let report = desktop::uninstall_desktop(bin_dir, data_dir)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Launch) => {
            // Hardware DMA-BUF rendering is enabled by default for native Wayland (>60Hz)
            // frame pacing; `--software-rendering` opts into software compositing if needed.
            if cli.software_rendering
                && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
            {
                unsafe {
                    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
                }
            }

            #[cfg(target_os = "linux")]
            {
                glib::set_prgname(Some("dev.logjournal.app"));
                glib::set_application_name("Log");
            }

            let vault_dir = resolve_vault_path(cli.vault, None);
            let vault = Vault::open(&vault_dir).expect("failed to initialize local journal vault");
            let protocol_vault_root = Arc::new(vault.root().to_path_buf());
            let state = DesktopState {
                vault: Arc::new(vault),
                recorder: Arc::new(RecorderManager::new()),
            };

            tauri::Builder::default()
                .register_asynchronous_uri_scheme_protocol(
                    "vault",
                    move |_ctx, request, responder| {
                        let root = Arc::clone(&protocol_vault_root);
                        tauri::async_runtime::spawn_blocking(move || {
                            let response = build_vault_protocol_response(&root, &request);
                            responder.respond(response);
                        });
                    },
                )
                .setup(|app| {
                    #[cfg(target_os = "linux")]
                    gdk::set_program_class("dev.logjournal.app");

                    app.manage(state);
                    if app.get_webview_window("main").is_none() {
                        if let Some(window_cfg) = app.config().app.windows.first() {
                            tauri::WebviewWindowBuilder::from_config(app.handle(), window_cfg)?
                                .build()?;
                        }
                    }
                    #[cfg(target_os = "linux")]
                    if let Some(main_window) = app.get_webview_window("main") {
                        configure_linux_webview_media(&main_window);
                    }
                    Ok(())
                })
                .invoke_handler(tauri::generate_handler![
                    get_vault_stats,
                    list_notebooks,
                    create_notebook,
                    update_notebook,
                    delete_notebook,
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
                    get_backup_config,
                    save_backup_config,
                    get_gdrive_status,
                    connect_gdrive_rclone,
                    connect_gdrive_oauth,
                    save_gdrive_token,
                    disconnect_gdrive,
                    upload_photo,
                    upload_audio,
                    start_voice_recording,
                    stop_voice_recording,
                    cancel_voice_recording,
                ])
                .build(tauri::generate_context!())
                .expect("error while building Log desktop application")
                .run(|app_handle, event| {
                    if let tauri::RunEvent::Ready = event {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                });
        }
        Some(cmd) => {
            if let Err(err) = run_cli_subcommand(cli.vault, cmd) {
                eprintln!("Error: {err:#}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("log-tauri-{label}-{nanos}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn decode_base64_handles_data_uri_whitespace_and_rejects_trailing_padding() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(
            decode_base64("data:image/png;base64, SGVs\n bG8= ").unwrap(),
            b"Hello"
        );
        assert_eq!(decode_base64("YQ==").unwrap(), b"a");
        assert_eq!(decode_base64("YWI=").unwrap(), b"ab");
        assert!(decode_base64("YQ==YQ==").is_err());
        assert!(decode_base64("YQ=").is_err());
        assert!(decode_base64("Y=Q=").is_err());
    }

    #[test]
    fn vault_protocol_supports_range_head_416_and_blocks_symlink_escape() {
        let root = make_temp_dir("vault-proto");
        let media_dir = root.join("media");
        let outside = make_temp_dir("vault-outside");
        fs::create_dir_all(&media_dir).unwrap();
        let audio_path = media_dir.join("sample chord.wav");
        fs::write(&audio_path, b"0123456789ABCDEF").unwrap();
        let secret_path = outside.join("secret.txt");
        fs::write(&secret_path, b"top-secret").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret_path, media_dir.join("escape.txt")).unwrap();

        let range_req = tauri::http::Request::builder()
            .method("GET")
            .uri("vault://localhost/vault-media/sample%20chord.wav")
            .header("Range", "bytes=4-7")
            .body(Vec::new())
            .unwrap();
        let resp = build_vault_protocol_response(&root, &range_req);
        assert_eq!(resp.status(), 206);
        assert_eq!(resp.body(), b"4567");
        assert_eq!(
            resp.headers()
                .get("Content-Range")
                .unwrap()
                .to_str()
                .unwrap(),
            "bytes 4-7/16"
        );
        assert_eq!(
            resp.headers()
                .get("Cache-Control")
                .unwrap()
                .to_str()
                .unwrap(),
            "private, max-age=31536000, immutable"
        );

        let head_req = tauri::http::Request::builder()
            .method("HEAD")
            .uri("vault://localhost/vault-media/sample%20chord.wav")
            .body(Vec::new())
            .unwrap();
        let head_resp = build_vault_protocol_response(&root, &head_req);
        assert_eq!(head_resp.status(), 200);
        assert!(head_resp.body().is_empty());
        assert_eq!(
            head_resp
                .headers()
                .get("Content-Length")
                .unwrap()
                .to_str()
                .unwrap(),
            "16"
        );

        let bad_range_req = tauri::http::Request::builder()
            .method("GET")
            .uri("vault://localhost/vault-media/sample%20chord.wav")
            .header("Range", "bytes=999-1000")
            .body(Vec::new())
            .unwrap();
        let bad_range_resp = build_vault_protocol_response(&root, &bad_range_req);
        assert_eq!(bad_range_resp.status(), 416);
        assert_eq!(
            bad_range_resp
                .headers()
                .get("Content-Range")
                .unwrap()
                .to_str()
                .unwrap(),
            "bytes */16"
        );

        #[cfg(unix)]
        {
            let escape_req = tauri::http::Request::builder()
                .method("GET")
                .uri("vault://localhost/vault-media/escape.txt")
                .body(Vec::new())
                .unwrap();
            let escape_resp = build_vault_protocol_response(&root, &escape_req);
            assert_eq!(escape_resp.status(), 403);
        }

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn desktop_exec_escapes_percent_and_quotes_spaces() {
        assert_eq!(
            desktop::format_desktop_exec(Path::new("/usr/bin/log-journal")),
            "/usr/bin/log-journal"
        );
        assert_eq!(
            desktop::format_desktop_exec(Path::new("/tmp/my app/100%/log-journal")),
            "\"/tmp/my app/100%%/log-journal\""
        );
    }

    #[test]
    fn restore_overwrite_accepts_flag_and_explicit_bool_values() {
        let parse_overwrite = |extra: &[&str]| -> bool {
            let mut args = vec![
                "log-journal",
                "restore",
                "--file",
                "a.age",
                "--target",
                "out",
                "--passphrase-env",
                "PASS",
            ];
            args.extend_from_slice(extra);
            let cli = Cli::try_parse_from(args).expect("cli parse");
            match cli.command {
                Some(Commands::Restore { overwrite, .. }) => overwrite,
                other => panic!("unexpected command: {other:?}"),
            }
        };

        assert!(parse_overwrite(&[]));
        assert!(parse_overwrite(&["--overwrite"]));
        assert!(!parse_overwrite(&["--overwrite", "false"]));
        assert!(!parse_overwrite(&["--overwrite=false"]));
        assert!(parse_overwrite(&["--overwrite", "true"]));
    }

    #[test]
    fn software_rendering_flag_is_opt_in() {
        let default_cli = Cli::try_parse_from(["log-journal"]).expect("default cli parse");
        assert!(!default_cli.software_rendering);

        let flag_cli =
            Cli::try_parse_from(["log-journal", "--software-rendering"]).expect("flag cli parse");
        assert!(flag_cli.software_rendering);
    }
}
