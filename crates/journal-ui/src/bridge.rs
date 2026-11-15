//! Typed HTTP / Tauri v2 IPC bridge to `journal-server` or `src-tauri`.

use chrono::NaiveDate;
use gloo_net::http::Request;
use journal_types::{
    AssetManifest, BacklinkRef, BackupConfig, BackupInfo, CalendarDaySummary,
    CreateNotebookRequest, DailyContext, DeleteEntryResponse, Entry, EntrySummary,
    ImportDayOneRequest, ImportNotionRequest, MigrationReport, Notebook, OnThisDayGroup,
    PolaroidMeta, RestoreBackupRequest, RestoreBackupResponse, SaveEntryRequest, SearchHit,
    SearchQuery, SyncResult, TagCount, Template, VaultStats, VoiceMemoMeta,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::FormData;

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: String,
}

#[derive(Debug, Serialize)]
struct EmptyPayload {}

#[derive(Debug, Serialize)]
struct ListEntriesPayload {
    notebook_id: Option<String>,
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Serialize)]
struct GetEntryPayload {
    id: String,
}

#[derive(Debug, Serialize)]
struct DeleteEntryPayload {
    id: Uuid,
}

#[derive(Debug, Serialize)]
struct CalendarMonthPayload {
    year: i32,
    month: u32,
    notebook_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OnThisDayPayload {
    month: Option<u32>,
    day: Option<u32>,
}

#[derive(Debug, Serialize)]
struct BacklinksPayload {
    entry_id: Uuid,
}

#[derive(Debug, Serialize)]
struct DailyContextPayload {
    date: Option<NaiveDate>,
    lat: Option<f64>,
    lng: Option<f64>,
}

#[derive(Debug, Serialize)]
struct CreateBackupPayload {
    passphrase: String,
    upload_gdrive: bool,
}

#[derive(Debug, Serialize)]
struct SyncGdrivePayload {
    passphrase: Option<String>,
    config: BackupConfig,
}

/// Returns `true` when running inside a Tauri v2 WebView (`window.__TAURI_INTERNALS__`).
pub fn is_tauri() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(internals) = js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI_INTERNALS__"))
    else {
        return false;
    };
    !internals.is_undefined() && !internals.is_null()
}

/// Invoke a backend command via Tauri v2 IPC when present, or `POST /api/invoke/{cmd}`.
pub async fn invoke<P, R>(cmd: &str, payload: &P) -> Result<R, String>
where
    P: Serialize + ?Sized,
    R: DeserializeOwned,
{
    if is_tauri() {
        return invoke_tauri(cmd, payload).await;
    }

    let url = format!("/api/invoke/{cmd}");
    let request = Request::post(&url)
        .json(payload)
        .map_err(|err| format!("Failed to encode request for {cmd}: {err}"))?;

    let response = request
        .send()
        .await
        .map_err(|err| format!("Network error calling {cmd}: {err}"))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("Failed reading response for {cmd}: {err}"))?;

    if !(200..300).contains(&status) {
        if let Ok(err_body) = serde_json::from_str::<ApiErrorBody>(&text) {
            return Err(err_body.error);
        }
        return Err(format!("HTTP {status} from {cmd}: {text}"));
    }

    serde_json::from_str::<R>(&text)
        .map_err(|err| format!("Invalid JSON response from {cmd}: {err}"))
}

async fn invoke_tauri<P, R>(cmd: &str, payload: &P) -> Result<R, String>
where
    P: Serialize + ?Sized,
    R: DeserializeOwned,
{
    let window = web_sys::window().ok_or_else(|| "Missing browser window".to_string())?;
    let internals = js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI_INTERNALS__"))
        .map_err(|err| format!("Failed to access __TAURI_INTERNALS__: {err:?}"))?;

    let invoke_fn_val = js_sys::Reflect::get(&internals, &JsValue::from_str("invoke"))
        .map_err(|err| format!("Missing __TAURI_INTERNALS__.invoke: {err:?}"))?;
    let invoke_fn: js_sys::Function = invoke_fn_val
        .dyn_into()
        .map_err(|_| "__TAURI_INTERNALS__.invoke is not a function".to_string())?;

    let payload_json = serde_json::to_string(payload)
        .map_err(|err| format!("Failed serializing payload for {cmd}: {err}"))?;
    let js_args = js_sys::JSON::parse(&payload_json)
        .map_err(|err| format!("Failed converting payload to JS for {cmd}: {err:?}"))?;

    let promise_val = invoke_fn
        .call2(&internals, &JsValue::from_str(cmd), &js_args)
        .map_err(|err| format!("Tauri invoke({cmd}) threw: {err:?}"))?;
    let promise: js_sys::Promise = promise_val
        .dyn_into()
        .map_err(|_| format!("Tauri invoke({cmd}) did not return a Promise"))?;

    let resolved = JsFuture::from(promise)
        .await
        .map_err(|err| format_js_error(&err))?;

    let result_json = js_sys::JSON::stringify(&resolved)
        .map_err(|err| format!("Failed serializing Tauri result for {cmd}: {err:?}"))?
        .as_string()
        .unwrap_or_else(|| "null".to_string());

    serde_json::from_str::<R>(&result_json)
        .map_err(|err| format!("Failed decoding Tauri response for {cmd}: {err}"))
}

fn format_js_error(val: &JsValue) -> String {
    if let Some(s) = val.as_string() {
        return s;
    }
    if let Ok(msg) = js_sys::Reflect::get(val, &JsValue::from_str("message")) {
        if let Some(s) = msg.as_string() {
            return s;
        }
    }
    if let Ok(s) = js_sys::JSON::stringify(val) {
        if let Some(txt) = s.as_string() {
            return txt;
        }
    }
    format!("{val:?}")
}

/// Upload a multipart `FormData` file under field `"file"` to the HTTP upload endpoint.
pub async fn upload_file<R: DeserializeOwned>(
    endpoint: &str,
    file: &web_sys::File,
) -> Result<R, String> {
    let form_data =
        FormData::new().map_err(|err| format!("Failed creating FormData: {err:?}"))?;
    form_data
        .append_with_blob_and_filename("file", file, &file.name())
        .map_err(|err| format!("Failed appending file to FormData: {err:?}"))?;
    send_multipart(endpoint, &form_data).await
}

/// Upload a raw `Blob` (such as recorded microphone audio) under field `"file"`.
pub async fn upload_blob<R: DeserializeOwned>(
    endpoint: &str,
    blob: &web_sys::Blob,
    filename: &str,
) -> Result<R, String> {
    let form_data =
        FormData::new().map_err(|err| format!("Failed creating FormData: {err:?}"))?;
    form_data
        .append_with_blob_and_filename("file", blob, filename)
        .map_err(|err| format!("Failed appending blob to FormData: {err:?}"))?;
    send_multipart(endpoint, &form_data).await
}

async fn send_multipart<R: DeserializeOwned>(
    endpoint: &str,
    form_data: &FormData,
) -> Result<R, String> {
    let request = Request::post(endpoint)
        .body(form_data)
        .map_err(|err| format!("Failed building upload request to {endpoint}: {err}"))?;

    let response = request
        .send()
        .await
        .map_err(|err| format!("Upload network error for {endpoint}: {err}"))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("Failed reading upload response from {endpoint}: {err}"))?;

    if !(200..300).contains(&status) {
        if let Ok(err_body) = serde_json::from_str::<ApiErrorBody>(&text) {
            return Err(err_body.error);
        }
        return Err(format!("HTTP {status} from {endpoint}: {text}"));
    }

    serde_json::from_str::<R>(&text)
        .map_err(|err| format!("Invalid JSON upload response from {endpoint}: {err}"))
}

// ---------------------------------------------------------------------------
// Typed Command Helpers (21 Commands + 4 Multipart Uploads)
// ---------------------------------------------------------------------------

pub async fn get_vault_stats() -> Result<VaultStats, String> {
    invoke("get_vault_stats", &EmptyPayload {}).await
}

pub async fn list_notebooks() -> Result<Vec<Notebook>, String> {
    invoke("list_notebooks", &EmptyPayload {}).await
}

pub async fn create_notebook(req: CreateNotebookRequest) -> Result<Notebook, String> {
    invoke("create_notebook", &req).await
}

pub async fn list_entries(
    notebook_id: Option<String>,
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<EntrySummary>, String> {
    invoke(
        "list_entries",
        &ListEntriesPayload {
            notebook_id,
            tag,
            limit,
            offset,
        },
    )
    .await
}

pub async fn get_entry(id: Uuid) -> Result<Entry, String> {
    invoke(
        "get_entry",
        &GetEntryPayload {
            id: id.to_string(),
        },
    )
    .await
}

pub async fn save_entry(req: SaveEntryRequest) -> Result<Entry, String> {
    invoke("save_entry", &req).await
}

pub async fn delete_entry(id: Uuid) -> Result<DeleteEntryResponse, String> {
    invoke("delete_entry", &DeleteEntryPayload { id }).await
}

pub async fn search_entries(query: SearchQuery) -> Result<Vec<SearchHit>, String> {
    invoke("search_entries", &query).await
}

pub async fn list_tags() -> Result<Vec<TagCount>, String> {
    invoke("list_tags", &EmptyPayload {}).await
}

pub async fn get_calendar_month(
    year: i32,
    month: u32,
    notebook_id: Option<String>,
) -> Result<Vec<CalendarDaySummary>, String> {
    invoke(
        "get_calendar_month",
        &CalendarMonthPayload {
            year,
            month,
            notebook_id,
        },
    )
    .await
}

pub async fn get_on_this_day(
    month: Option<u32>,
    day: Option<u32>,
) -> Result<Vec<OnThisDayGroup>, String> {
    invoke("get_on_this_day", &OnThisDayPayload { month, day }).await
}

pub async fn get_backlinks(entry_id: Uuid) -> Result<Vec<BacklinkRef>, String> {
    invoke("get_backlinks", &BacklinksPayload { entry_id }).await
}

pub async fn list_templates() -> Result<Vec<Template>, String> {
    invoke("list_templates", &EmptyPayload {}).await
}

pub async fn get_daily_context(
    date: Option<NaiveDate>,
    lat: Option<f64>,
    lng: Option<f64>,
) -> Result<DailyContext, String> {
    invoke("get_daily_context", &DailyContextPayload { date, lat, lng }).await
}

pub async fn get_asset_manifest() -> Result<AssetManifest, String> {
    invoke("get_asset_manifest", &EmptyPayload {}).await
}

pub async fn import_dayone(req: ImportDayOneRequest) -> Result<MigrationReport, String> {
    invoke("import_dayone", &req).await
}

pub async fn import_notion(req: ImportNotionRequest) -> Result<MigrationReport, String> {
    invoke("import_notion", &req).await
}

pub async fn list_backups() -> Result<Vec<BackupInfo>, String> {
    invoke("list_backups", &EmptyPayload {}).await
}

pub async fn create_backup(passphrase: String, upload_gdrive: bool) -> Result<BackupInfo, String> {
    invoke(
        "create_backup",
        &CreateBackupPayload {
            passphrase,
            upload_gdrive,
        },
    )
    .await
}

pub async fn restore_backup(req: RestoreBackupRequest) -> Result<RestoreBackupResponse, String> {
    invoke("restore_backup", &req).await
}

pub async fn sync_gdrive(
    passphrase: Option<String>,
    config: BackupConfig,
) -> Result<SyncResult, String> {
    invoke("sync_gdrive", &SyncGdrivePayload { passphrase, config }).await
}

pub async fn upload_photo(file: &web_sys::File) -> Result<PolaroidMeta, String> {
    upload_file("/api/upload/photo", file).await
}

pub async fn upload_audio_blob(
    blob: &web_sys::Blob,
    filename: &str,
) -> Result<VoiceMemoMeta, String> {
    upload_blob("/api/upload/audio", blob, filename).await
}

pub async fn upload_audio_file(file: &web_sys::File) -> Result<VoiceMemoMeta, String> {
    upload_file("/api/upload/audio", file).await
}

pub async fn upload_dayone_zip(file: &web_sys::File) -> Result<MigrationReport, String> {
    upload_file("/api/upload/dayone", file).await
}

pub async fn upload_notion_archive(file: &web_sys::File) -> Result<MigrationReport, String> {
    upload_file("/api/upload/notion", file).await
}
