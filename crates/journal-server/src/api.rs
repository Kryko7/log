//! Axum HTTP API (`/api/invoke/{cmd}`, `/api/upload/*`) and static file router.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{Datelike, NaiveDate, Utc};
use journal_core::Vault;
use journal_import::{import_dayone, import_notion};
use journal_sync::{create_backup, get_daily_context, list_backups, restore_backup, sync_gdrive};
use journal_types::{
    BackupConfig, CreateNotebookRequest, ImportDayOneRequest, ImportNotionRequest,
    RestoreBackupRequest, SaveEntryRequest, SearchQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

use crate::assets::{load_asset_manifest, resolve_repo_path};

/// Shared application state for Axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub vault: Arc<Vault>,
}

/// Structured JSON error returned with HTTP 4xx/5xx status codes.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
    }

    fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<journal_core::VaultError> for ApiError {
    fn from(err: journal_core::VaultError) -> Self {
        match err {
            journal_core::VaultError::NotFound(m) => Self::not_found(m),
            journal_core::VaultError::InvalidInput(m) => Self::bad_request(m),
            other => Self::internal(other.to_string()),
        }
    }
}

impl From<journal_import::ImportError> for ApiError {
    fn from(err: journal_import::ImportError) -> Self {
        match err {
            journal_import::ImportError::InvalidSource(m) => Self::bad_request(m),
            journal_import::ImportError::Vault(v) => Self::from(v),
            other => Self::internal(other.to_string()),
        }
    }
}

impl From<journal_sync::SyncError> for ApiError {
    fn from(err: journal_sync::SyncError) -> Self {
        match err {
            journal_sync::SyncError::NotFound(m) => Self::not_found(m),
            journal_sync::SyncError::InvalidInput(m) => Self::bad_request(m),
            other => Self::internal(other.to_string()),
        }
    }
}

/// Build the complete Axum router with API endpoints and static asset serving.
pub fn build_router(vault: Arc<Vault>) -> Router {
    let vault_root = vault.root().to_path_buf();
    let media_dir = vault_root.join("media");
    let thumbs_dir = vault_root.join(".index").join("thumbs");
    let assets_dir = resolve_repo_path("assets").unwrap_or_else(|| PathBuf::from("assets"));
    let dist_dir = resolve_repo_path("crates/journal-ui/dist")
        .unwrap_or_else(|| PathBuf::from("crates/journal-ui/dist"));

    let state = AppState { vault };

    let upload_routes = Router::new()
        .route("/api/upload/photo", post(upload_photo))
        .route("/api/upload/audio", post(upload_audio))
        .route("/api/upload/dayone", post(upload_dayone))
        .route("/api/upload/notion", post(upload_notion))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024));

    let spa_service = ServeDir::new(&dist_dir)
        .fallback(ServeFile::new(dist_dir.join("index.html")));

    Router::new()
        .route("/api/invoke/{cmd}", post(invoke_command))
        .merge(upload_routes)
        .nest_service("/assets", ServeDir::new(assets_dir))
        .nest_service("/vault-media", ServeDir::new(media_dir))
        .nest_service("/vault-thumbs", ServeDir::new(thumbs_dir))
        .fallback_service(spa_service)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[derive(Debug, Default, Deserialize)]
struct ListEntriesParams {
    #[serde(default)]
    notebook_id: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct GetEntryParams {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DeleteEntryParams {
    id: Uuid,
}

#[derive(Debug, Default, Deserialize)]
struct CalendarMonthParams {
    #[serde(default)]
    year: Option<i32>,
    #[serde(default)]
    month: Option<u32>,
    #[serde(default)]
    notebook_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OnThisDayParams {
    #[serde(default)]
    month: Option<u32>,
    #[serde(default)]
    day: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BacklinksParams {
    #[serde(default)]
    entry_id: Option<Uuid>,
    #[serde(default)]
    id: Option<Uuid>,
}

#[derive(Debug, Default, Deserialize)]
struct DailyContextParams {
    #[serde(default)]
    date: Option<NaiveDate>,
    #[serde(default)]
    lat: Option<f64>,
    #[serde(default)]
    lng: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct CreateBackupParams {
    #[serde(default)]
    passphrase: String,
    #[serde(default)]
    upload_gdrive: bool,
}

#[derive(Debug, Default, Deserialize)]
struct SyncGdriveParams {
    #[serde(default)]
    passphrase: Option<String>,
    #[serde(default)]
    config: BackupConfig,
}

fn parse_body<T: for<'de> Deserialize<'de>>(bytes: &Bytes) -> Result<T, ApiError> {
    let slice = if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        b"{}"
    } else {
        bytes.as_ref()
    };
    serde_json::from_slice(slice)
        .map_err(|err| ApiError::bad_request(format!("Invalid JSON payload: {err}")))
}

fn json_ok<T: Serialize>(value: T) -> Result<Response, ApiError> {
    Ok((StatusCode::OK, Json(value)).into_response())
}

async fn invoke_command(
    State(state): State<AppState>,
    AxumPath(cmd): AxumPath<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let vault = &state.vault;
    match cmd.as_str() {
        "get_vault_stats" => json_ok(vault.get_vault_stats()?),
        "list_notebooks" => json_ok(vault.list_notebooks()?),
        "create_notebook" => {
            let req: CreateNotebookRequest = parse_body(&body)?;
            json_ok(vault.create_notebook(req)?)
        }
        "list_entries" => {
            let p: ListEntriesParams = parse_body(&body)?;
            json_ok(vault.list_entries(
                p.notebook_id.as_deref(),
                p.tag.as_deref(),
                p.limit,
                p.offset,
            )?)
        }
        "get_entry" => {
            let p: GetEntryParams = parse_body(&body)?;
            json_ok(vault.get_entry(&p.id)?)
        }
        "save_entry" => {
            let req: SaveEntryRequest = parse_body(&body)?;
            json_ok(vault.save_entry(req)?)
        }
        "delete_entry" => {
            let p: DeleteEntryParams = parse_body(&body)?;
            json_ok(vault.delete_entry(p.id)?)
        }
        "search_entries" => {
            let query: SearchQuery = parse_body(&body)?;
            json_ok(vault.search_entries(&query)?)
        }
        "list_tags" => json_ok(vault.list_tags()?),
        "get_calendar_month" => {
            let p: CalendarMonthParams = parse_body(&body)?;
            let today = Utc::now().date_naive();
            let year = p.year.unwrap_or_else(|| today.year());
            let month = p.month.unwrap_or_else(|| today.month());
            json_ok(vault.get_calendar_month(year, month, p.notebook_id.as_deref())?)
        }
        "get_on_this_day" => {
            let p: OnThisDayParams = parse_body(&body)?;
            let today = Utc::now().date_naive();
            let month = p.month.unwrap_or_else(|| today.month());
            let day = p.day.unwrap_or_else(|| today.day());
            json_ok(vault.get_on_this_day(month, day, today.year())?)
        }
        "get_backlinks" => {
            let p: BacklinksParams = parse_body(&body)?;
            let entry_id = p
                .entry_id
                .or(p.id)
                .ok_or_else(|| ApiError::bad_request("Missing 'entry_id' UUID"))?;
            json_ok(vault.get_backlinks(entry_id)?)
        }
        "list_templates" => json_ok(vault.list_templates()?),
        "get_daily_context" => {
            let p: DailyContextParams = parse_body(&body)?;
            let date = p.date.unwrap_or_else(|| Utc::now().date_naive());
            let ctx = get_daily_context(vault, date, p.lat, p.lng).await?;
            json_ok(ctx)
        }
        "get_asset_manifest" => {
            let manifest = load_asset_manifest()
                .map_err(|e| ApiError::internal(format!("Failed to load asset manifest: {e}")))?;
            json_ok(manifest)
        }
        "import_dayone" => {
            let req: ImportDayOneRequest = parse_body(&body)?;
            json_ok(import_dayone(vault, &req)?)
        }
        "import_notion" => {
            let req: ImportNotionRequest = parse_body(&body)?;
            json_ok(import_notion(vault, &req)?)
        }
        "list_backups" => json_ok(list_backups(vault.root())?),
        "create_backup" => {
            let p: CreateBackupParams = parse_body(&body)?;
            if p.upload_gdrive {
                let sync_res = sync_gdrive(
                    vault.root(),
                    Some(&p.passphrase),
                    &BackupConfig::default(),
                )
                .await?;
                let info = sync_res
                    .backup
                    .ok_or_else(|| ApiError::internal("Backup succeeded without metadata"))?;
                json_ok(info)
            } else {
                json_ok(create_backup(
                    vault.root(),
                    &p.passphrase,
                    &BackupConfig::default(),
                )?)
            }
        }
        "restore_backup" => {
            let req: RestoreBackupRequest = parse_body(&body)?;
            let resp = restore_backup(vault.root(), &req)?;
            if resp.restored {
                if PathBuf::from(&resp.target_vault_dir) == vault.root() {
                    let _ = vault.rebuild_index();
                } else if let Ok(target_vault) = Vault::open(&resp.target_vault_dir) {
                    let _ = target_vault.rebuild_index();
                }
            }
            json_ok(resp)
        }
        "sync_gdrive" => {
            let p: SyncGdriveParams = parse_body(&body)?;
            let res = sync_gdrive(vault.root(), p.passphrase.as_deref(), &p.config).await?;
            json_ok(res)
        }
        other => Err(ApiError::not_found(format!("Unknown invoke command: {other}"))),
    }
}

struct UploadedFile {
    filename: String,
    content_type: Option<String>,
    bytes: Vec<u8>,
    caption: Option<String>,
    duration_secs: Option<f32>,
}

async fn read_multipart_file(mut multipart: Multipart) -> Result<UploadedFile, ApiError> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename = String::from("upload.bin");
    let mut content_type: Option<String> = None;
    let mut caption: Option<String> = None;
    let mut duration_secs: Option<f32> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("Malformed multipart payload: {e}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            if let Some(fname) = field.file_name() {
                if !fname.trim().is_empty() {
                    filename = fname.to_string();
                }
            }
            content_type = field.content_type().map(|ct| ct.to_string());
            let data = field
                .bytes()
                .await
                .map_err(|e| ApiError::bad_request(format!("Failed reading file field: {e}")))?;
            file_bytes = Some(data.to_vec());
        } else if name == "caption" {
            if let Ok(txt) = field.text().await {
                let trimmed = txt.trim().to_string();
                if !trimmed.is_empty() {
                    caption = Some(trimmed);
                }
            }
        } else if name == "duration_secs" {
            if let Ok(txt) = field.text().await {
                if let Ok(val) = txt.trim().parse::<f32>() {
                    duration_secs = Some(val);
                }
            }
        }
    }

    let bytes = file_bytes
        .ok_or_else(|| ApiError::bad_request("Missing multipart field named 'file'"))?;
    if bytes.is_empty() {
        return Err(ApiError::bad_request("Uploaded file cannot be empty"));
    }

    Ok(UploadedFile {
        filename,
        content_type,
        bytes,
        caption,
        duration_secs,
    })
}

async fn upload_photo(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let uploaded = read_multipart_file(multipart).await?;
    let meta = state
        .vault
        .ingest_photo(&uploaded.filename, &uploaded.bytes, uploaded.caption)?;
    json_ok(meta)
}

async fn upload_audio(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let uploaded = read_multipart_file(multipart).await?;
    let meta = state.vault.ingest_audio(
        &uploaded.filename,
        &uploaded.bytes,
        uploaded.duration_secs,
        uploaded.content_type,
    )?;
    json_ok(meta)
}

async fn upload_dayone(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let uploaded = read_multipart_file(multipart).await?;
    let ext = std::path::Path::new(&uploaded.filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("zip");
    let temp_path = std::env::temp_dir().join(format!("log-dayone-{}.{ext}", Uuid::now_v7()));
    tokio::fs::write(&temp_path, &uploaded.bytes)
        .await
        .map_err(|e| ApiError::internal(format!("Failed writing temp Day One archive: {e}")))?;

    let req = ImportDayOneRequest {
        zip_path: temp_path.to_string_lossy().into_owned(),
        notebook_id: None,
        dry_run: false,
    };
    let result = import_dayone(&state.vault, &req);
    let _ = tokio::fs::remove_file(&temp_path).await;
    json_ok(result?)
}

async fn upload_notion(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let uploaded = read_multipart_file(multipart).await?;
    let ext = std::path::Path::new(&uploaded.filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("zip");
    let temp_path = std::env::temp_dir().join(format!("log-notion-{}.{ext}", Uuid::now_v7()));
    tokio::fs::write(&temp_path, &uploaded.bytes)
        .await
        .map_err(|e| ApiError::internal(format!("Failed writing temp Notion archive: {e}")))?;

    let req = ImportNotionRequest {
        source_path: temp_path.to_string_lossy().into_owned(),
        notebook_id: None,
        dry_run: false,
    };
    let result = import_notion(&state.vault, &req);
    let _ = tokio::fs::remove_file(&temp_path).await;
    json_ok(result?)
}
