#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri v2 desktop shell for **Log** registering all 21 IPC commands and
//! Wayland WebKit DMABUF fallback rendering protection.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
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

struct DesktopState {
    vault: Arc<Vault>,
}

fn err_str(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn resolve_asset_manifest_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets/manifest.json"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets/manifest.json"));
            if let Some(p1) = dir.parent() {
                candidates.push(p1.join("assets/manifest.json"));
                if let Some(p2) = p1.parent() {
                    candidates.push(p2.join("assets/manifest.json"));
                }
            }
        }
    }
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(root) = manifest_root.parent() {
        candidates.push(root.join("assets/manifest.json"));
    }
    candidates.into_iter().find(|p| p.exists())
}

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
    let path = resolve_asset_manifest_path()
        .ok_or_else(|| "Could not locate assets/manifest.json".to_string())?;
    let raw = std::fs::read_to_string(&path).map_err(err_str)?;
    serde_json::from_str(&raw).map_err(err_str)
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

fn main() {
    // Guard against WebKitGTK Wayland DMABUF blank-canvas rendering bugs on Arch/Linux.
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    let vault = Vault::open_default().expect("failed to initialize local journal vault");
    let state = DesktopState {
        vault: Arc::new(vault),
    };

    tauri::Builder::default()
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Log desktop application");
}
