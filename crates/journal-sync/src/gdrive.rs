//! Google Drive encrypted backup orchestration (`LogJournal` folder).
//!
//! Security invariant: only `.age`-encrypted archives are ever uploaded remotely.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use chrono::Utc;
use journal_types::{BackupConfig, SyncResult};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use crate::crypto::{create_backup, list_backups, update_backup_sidecar};
use crate::error::{Result, SyncError};

const GDRIVE_FILES_API: &str = "https://www.googleapis.com/drive/v3/files";
const GDRIVE_UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3/files";

#[derive(Debug, Deserialize)]
struct DriveFileList {
    #[serde(default)]
    files: Vec<DriveFileEntry>,
}

#[derive(Debug, Deserialize)]
struct DriveFileEntry {
    id: String,
    #[serde(default)]
    name: String,
}

/// Create (when `passphrase_opt` is provided) or select the latest `.age`-encrypted
/// vault archive and upload it to a single Google Drive folder (`config.gdrive_folder`,
/// defaulting to `"LogJournal"`).
///
/// When offline or when neither `LOG_GDRIVE_TOKEN` nor `LOG_GDRIVE_USE_CLI=1` is set,
/// this returns `Ok(SyncResult { success: true, uploaded_remote: false, ... })` with
/// the local encrypted backup details intact so the caller never loses local progress.
pub async fn sync_gdrive(
    vault_dir: &Path,
    passphrase_opt: Option<&str>,
    config: &BackupConfig,
) -> Result<SyncResult> {
    let vault_dir_owned = vault_dir.to_path_buf();
    let passphrase_owned = passphrase_opt
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let config_owned = config.clone();

    let mut backup = tokio::task::spawn_blocking(move || match passphrase_owned {
        Some(passphrase) => create_backup(&vault_dir_owned, &passphrase, &config_owned),
        None => {
            let mut existing = list_backups(&vault_dir_owned)?;
            if existing.is_empty() {
                return Err(SyncError::InvalidInput(
                    "No encrypted backup exists yet; provide a passphrase to create one."
                        .to_string(),
                ));
            }
            Ok(existing.remove(0))
        }
    })
    .await
    .map_err(|e| SyncError::Io(std::io::Error::other(format!("backup task failed: {e}"))))??;

    // Enforce client-side encryption invariant before any remote transfer.
    if !backup.encrypted || !backup.filename.ends_with(".age") {
        return Err(SyncError::Crypto(format!(
            "Refusing to upload unencrypted backup '{}'",
            backup.filename
        )));
    }

    let archive_path = vault_dir.join("backups").join(&backup.filename);
    if !archive_path.is_file() {
        return Err(SyncError::NotFound(archive_path.display().to_string()));
    }

    let folder_name = if config.gdrive_folder.trim().is_empty() {
        "LogJournal"
    } else {
        config.gdrive_folder.trim()
    };

    if let Ok(token) = std::env::var("LOG_GDRIVE_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            match upload_via_rest(token, folder_name, &archive_path, &backup.filename).await {
                Ok(remote_id) => {
                    backup.location = "gdrive".to_string();
                    backup.remote_id = Some(remote_id.clone());
                    let sidecar_vault = vault_dir.to_path_buf();
                    let sidecar_backup = backup.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        update_backup_sidecar(&sidecar_vault, &sidecar_backup)
                    })
                    .await;
                    return Ok(SyncResult {
                        success: true,
                        backup: Some(backup.clone()),
                        uploaded_remote: true,
                        message: format!(
                            "Encrypted archive '{}' uploaded to Google Drive folder '{folder_name}' (id: {remote_id}).",
                            backup.filename
                        ),
                        synced_at: Utc::now(),
                    });
                }
                Err(err) => {
                    return Ok(SyncResult {
                        success: true,
                        backup: Some(backup.clone()),
                        uploaded_remote: false,
                        message: format!(
                            "Local encrypted backup '{}' saved; Google Drive upload unavailable ({err}).",
                            backup.filename
                        ),
                        synced_at: Utc::now(),
                    });
                }
            }
        }
    }

    let use_cli = std::env::var("LOG_GDRIVE_USE_CLI")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    if use_cli {
        match upload_via_cli(folder_name, &archive_path).await {
            Ok(remote_id_opt) => {
                backup.location = "gdrive".to_string();
                backup.remote_id = remote_id_opt;
                let sidecar_vault = vault_dir.to_path_buf();
                let sidecar_backup = backup.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    update_backup_sidecar(&sidecar_vault, &sidecar_backup)
                })
                .await;
                return Ok(SyncResult {
                    success: true,
                    backup: Some(backup.clone()),
                    uploaded_remote: true,
                    message: format!(
                        "Encrypted archive '{}' synced to Google Drive folder '{folder_name}' via gdrive CLI.",
                        backup.filename
                    ),
                    synced_at: Utc::now(),
                });
            }
            Err(err) => {
                return Ok(SyncResult {
                    success: true,
                    backup: Some(backup.clone()),
                    uploaded_remote: false,
                    message: format!(
                        "Local encrypted backup '{}' saved; gdrive CLI sync skipped ({err}).",
                        backup.filename
                    ),
                    synced_at: Utc::now(),
                });
            }
        }
    }

    Ok(SyncResult {
        success: true,
        backup: Some(backup.clone()),
        uploaded_remote: false,
        message: format!(
            "Local encrypted backup '{}' saved; remote Google Drive sync not configured (set LOG_GDRIVE_TOKEN or run scripts/sync-gdrive.sh).",
            backup.filename
        ),
        synced_at: Utc::now(),
    })
}

const MULTIPART_UPLOAD_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;
const RESUMABLE_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const CLI_QUERY_TIMEOUT: Duration = Duration::from_secs(20);
const CLI_UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

fn shared_gdrive_client() -> std::result::Result<&'static reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<std::result::Result<reqwest::Client, String>> =
        std::sync::OnceLock::new();
    let res = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())
    });
    match res {
        Ok(client) => Ok(client),
        Err(err) => Err(err.clone()),
    }
}

async fn upload_via_rest(
    bearer_token: &str,
    folder_name: &str,
    archive_path: &Path,
    filename: &str,
) -> std::result::Result<String, String> {
    let client = shared_gdrive_client()?;

    let folder_id = ensure_drive_folder(client, bearer_token, folder_name).await?;
    let total_len = tokio::fs::metadata(archive_path)
        .await
        .map_err(|e| format!("failed to stat archive: {e}"))?
        .len();

    if total_len <= MULTIPART_UPLOAD_THRESHOLD_BYTES {
        let bytes = tokio::fs::read(archive_path)
            .await
            .map_err(|e| format!("failed to read archive: {e}"))?;

        let metadata = serde_json::json!({
            "name": filename,
            "parents": [folder_id],
        });

        let metadata_part = Part::text(metadata.to_string())
            .mime_str("application/json; charset=UTF-8")
            .map_err(|e| e.to_string())?;
        let media_part = Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| e.to_string())?;

        let form = Form::new()
            .part("metadata", metadata_part)
            .part("file", media_part);

        let resp = client
            .post(format!(
                "{GDRIVE_UPLOAD_API}?uploadType=multipart&fields=id,name"
            ))
            .bearer_auth(bearer_token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {status}: {body}"));
        }

        let uploaded: DriveFileEntry = resp.json().await.map_err(|e| e.to_string())?;
        return Ok(uploaded.id);
    }

    upload_via_resumable(
        client,
        bearer_token,
        &folder_id,
        archive_path,
        filename,
        total_len,
    )
    .await
}

async fn upload_via_resumable(
    client: &reqwest::Client,
    bearer_token: &str,
    folder_id: &str,
    archive_path: &Path,
    filename: &str,
    total_len: u64,
) -> std::result::Result<String, String> {
    use tokio::io::AsyncReadExt;

    let init_resp = client
        .post(format!(
            "{GDRIVE_UPLOAD_API}?uploadType=resumable&fields=id,name"
        ))
        .bearer_auth(bearer_token)
        .header("X-Upload-Content-Type", "application/octet-stream")
        .header("X-Upload-Content-Length", total_len.to_string())
        .json(&serde_json::json!({
            "name": filename,
            "parents": [folder_id],
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !init_resp.status().is_success() {
        let status = init_resp.status();
        let body = init_resp.text().await.unwrap_or_default();
        return Err(format!("Drive resumable init HTTP {status}: {body}"));
    }

    let session_uri = init_resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| "Drive resumable init missing Location header".to_string())?;

    let mut file = tokio::fs::File::open(archive_path)
        .await
        .map_err(|e| format!("failed to open archive: {e}"))?;
    let mut offset: u64 = 0;

    loop {
        let mut buf = vec![0u8; RESUMABLE_CHUNK_BYTES];
        let mut filled = 0usize;
        while filled < buf.len() {
            let n = file
                .read(&mut buf[filled..])
                .await
                .map_err(|e| format!("failed to read archive chunk: {e}"))?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            return Err("Drive resumable upload ended before server returned file id".to_string());
        }
        buf.truncate(filled);

        let end = offset + (filled as u64) - 1;
        let content_range = format!("bytes {offset}-{end}/{total_len}");
        let chunk_resp = client
            .put(&session_uri)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", filled.to_string())
            .header("Content-Range", content_range)
            .body(buf)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = chunk_resp.status();
        if status.as_u16() == 308 {
            offset = end + 1;
            continue;
        }
        if !status.is_success() {
            let body = chunk_resp.text().await.unwrap_or_default();
            return Err(format!("Drive resumable chunk HTTP {status}: {body}"));
        }

        let uploaded: DriveFileEntry = chunk_resp.json().await.map_err(|e| e.to_string())?;
        return Ok(uploaded.id);
    }
}

async fn ensure_drive_folder(
    client: &reqwest::Client,
    bearer_token: &str,
    folder_name: &str,
) -> std::result::Result<String, String> {
    let escaped = folder_name.replace('\\', "\\\\").replace('\'', "\\'");
    let q = format!(
        "name = '{escaped}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false"
    );

    let search_resp = client
        .get(GDRIVE_FILES_API)
        .bearer_auth(bearer_token)
        .query(&[
            ("q", q.as_str()),
            ("fields", "files(id,name)"),
            ("pageSize", "10"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !search_resp.status().is_success() {
        let status = search_resp.status();
        let body = search_resp.text().await.unwrap_or_default();
        return Err(format!("Drive folder search HTTP {status}: {body}"));
    }

    let list: DriveFileList = search_resp.json().await.map_err(|e| e.to_string())?;
    if let Some(existing) = list.files.into_iter().find(|f| f.name == folder_name) {
        return Ok(existing.id);
    }

    let create_resp = client
        .post(GDRIVE_FILES_API)
        .bearer_auth(bearer_token)
        .query(&[("fields", "id,name")])
        .json(&serde_json::json!({
            "name": folder_name,
            "mimeType": "application/vnd.google-apps.folder"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !create_resp.status().is_success() {
        let status = create_resp.status();
        let body = create_resp.text().await.unwrap_or_default();
        return Err(format!("Drive mkdir HTTP {status}: {body}"));
    }

    let created: DriveFileEntry = create_resp.json().await.map_err(|e| e.to_string())?;
    Ok(created.id)
}

async fn run_cli_with_timeout(
    cli: &str,
    args: &[&str],
    timeout_dur: Duration,
    action: &str,
) -> std::result::Result<std::process::Output, String> {
    let mut cmd = tokio::process::Command::new(cli);
    cmd.args(args).stdin(Stdio::null());
    tokio::time::timeout(timeout_dur, cmd.output())
        .await
        .map_err(|_| format!("{cli} {action} timed out after {}s", timeout_dur.as_secs()))?
        .map_err(|e| format!("failed to run {cli} {action}: {e}"))
}

async fn upload_via_cli(
    folder_name: &str,
    archive_path: &Path,
) -> std::result::Result<Option<String>, String> {
    let cli = std::env::var("LOG_GDRIVE_CLI").unwrap_or_else(|_| "gdrive".to_string());

    let search_out = run_cli_with_timeout(
        &cli,
        &[
            "readonly",
            "search",
            "--name-exact",
            folder_name,
            "--type",
            "folder",
            "--json",
        ],
        CLI_QUERY_TIMEOUT,
        "search",
    )
    .await?;

    if !search_out.status.success() {
        let err = String::from_utf8_lossy(&search_out.stderr);
        return Err(format!("{cli} search failed: {}", err.trim()));
    }

    let folder_id = parse_cli_first_id(&search_out.stdout).unwrap_or_default();
    let folder_id = if folder_id.is_empty() {
        let mkdir_out = run_cli_with_timeout(
            &cli,
            &["mutate", "mkdir", folder_name, "--json"],
            CLI_QUERY_TIMEOUT,
            "mkdir",
        )
        .await?;
        if !mkdir_out.status.success() {
            let err = String::from_utf8_lossy(&mkdir_out.stderr);
            return Err(format!("{cli} mkdir failed: {}", err.trim()));
        }
        parse_cli_first_id(&mkdir_out.stdout)
            .ok_or_else(|| "gdrive mkdir did not return folder id".to_string())?
    } else {
        folder_id
    };

    let archive_str = archive_path.to_string_lossy().into_owned();
    let upload_out = run_cli_with_timeout(
        &cli,
        &[
            "mutate",
            "upload",
            &archive_str,
            "--parent",
            &folder_id,
            "--mime-type",
            "application/octet-stream",
            "--json",
        ],
        CLI_UPLOAD_TIMEOUT,
        "upload",
    )
    .await?;

    if !upload_out.status.success() {
        let err = String::from_utf8_lossy(&upload_out.stderr);
        return Err(format!("{cli} upload failed: {}", err.trim()));
    }

    Ok(parse_cli_first_id(&upload_out.stdout))
}

fn parse_cli_first_id(bytes: &[u8]) -> Option<String> {
    let val: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if let Some(id) = val.get("id").and_then(|v| v.as_str()) {
        return Some(id.to_string());
    }
    if let Some(arr) = val.as_array() {
        return arr
            .first()
            .and_then(|item| item.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    if let Some(arr) = val.get("files").and_then(|v| v.as_array()) {
        return arr
            .first()
            .and_then(|item| item.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    None
}
