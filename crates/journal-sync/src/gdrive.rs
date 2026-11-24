//! Google Drive encrypted backup orchestration (`LogJournal` folder).
//!
//! Security invariant: only `.age`-encrypted archives are ever uploaded remotely.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use base64::Engine as _;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use journal_types::{
    BackupConfig, BackupInfo, ConnectGdriveOauthRequest, GdriveAuthStatus, SyncResult,
};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::crypto::{
    create_backup, expand_tilde, list_backups, update_backup_sidecar, write_sidecars,
};
use crate::error::{Result, SyncError};

const GDRIVE_FILES_API: &str = "https://www.googleapis.com/drive/v3/files";
const GDRIVE_UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3/files";
const GDRIVE_ABOUT_API: &str = "https://www.googleapis.com/drive/v3/about";
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_OAUTH_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

const MULTIPART_UPLOAD_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;
const RESUMABLE_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const CLI_DETECT_TIMEOUT: Duration = Duration::from_secs(5);
const CLI_QUERY_TIMEOUT: Duration = Duration::from_secs(20);
const OAUTH_FLOW_TIMEOUT: Duration = Duration::from_secs(180);
const RCLONE_AUTH_TIMEOUT: Duration = Duration::from_secs(180);
const REMOTE_UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);

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

#[derive(Debug, Deserialize)]
struct DriveAboutResponse {
    #[serde(default)]
    user: Option<DriveAboutUser>,
}

#[derive(Debug, Deserialize)]
struct DriveAboutUser {
    #[serde(rename = "emailAddress", default)]
    email_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

fn has_required_drive_scope(scope_str: &str) -> bool {
    scope_str.split_whitespace().any(|s| {
        matches!(
            s,
            "https://www.googleapis.com/auth/drive.file" | "https://www.googleapis.com/auth/drive"
        )
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredGdriveAuth {
    client_id: Option<String>,
    client_secret: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    account_email: Option<String>,
}

enum RestUploadError {
    Unauthorized(String),
    Other(String),
}

impl RestUploadError {
    fn into_message(self) -> String {
        match self {
            Self::Unauthorized(msg) | Self::Other(msg) => msg,
        }
    }
}

fn config_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("LOG_CONFIG_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return expand_tilde(trimmed);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("log-journal")
}

fn backup_config_path() -> PathBuf {
    config_dir().join("backup-config.json")
}

fn gdrive_auth_path() -> PathBuf {
    config_dir().join("gdrive-auth.json")
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let bytes = serde_json::to_vec_pretty(value)?;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;

    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn normalize_backup_config(config: &BackupConfig) -> BackupConfig {
    let mut normalized = config.clone();
    normalized.destination_dir = normalized
        .destination_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let trimmed_folder = normalized.gdrive_folder.trim();
    normalized.gdrive_folder = if trimmed_folder.is_empty() {
        "LogJournal".to_string()
    } else {
        trimmed_folder.to_string()
    };
    normalized.has_passphrase_hint = normalized
        .has_passphrase_hint
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    normalized.rclone_remote = normalized
        .rclone_remote
        .as_deref()
        .map(str::trim)
        .map(|s| s.trim_end_matches(':'))
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    normalized
}

/// Load persisted backup configuration from `<config_dir>/backup-config.json`.
pub fn load_backup_config() -> BackupConfig {
    let path = backup_config_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return BackupConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<BackupConfig>(&raw) else {
        return BackupConfig::default();
    };
    normalize_backup_config(&parsed)
}

/// Persist backup configuration to `<config_dir>/backup-config.json` with `0600` mode on Unix.
pub fn save_backup_config(config: &BackupConfig) -> Result<BackupConfig> {
    let normalized = normalize_backup_config(config);
    write_private_json(&backup_config_path(), &normalized)?;
    Ok(normalized)
}

fn load_stored_gdrive_auth() -> StoredGdriveAuth {
    let path = gdrive_auth_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return StoredGdriveAuth::default();
    };
    serde_json::from_str::<StoredGdriveAuth>(&raw).unwrap_or_default()
}

fn save_stored_gdrive_auth(auth: &StoredGdriveAuth) -> Result<()> {
    write_private_json(&gdrive_auth_path(), auth)
}

fn find_binary_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn is_gdrive_cli_disabled() -> bool {
    std::env::var("LOG_GDRIVE_DISABLE_CLI")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn find_gdrive_cli() -> Option<String> {
    if is_gdrive_cli_disabled() {
        return None;
    }
    if let Ok(explicit) = std::env::var("LOG_GDRIVE_CLI") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            let explicit_path = expand_tilde(trimmed);
            if explicit_path.is_file() {
                return Some(explicit_path.to_string_lossy().into_owned());
            }
            if !trimmed.contains(std::path::MAIN_SEPARATOR)
                && find_binary_in_path(trimmed).is_some()
            {
                return Some(trimmed.to_string());
            }
            return None;
        }
    }

    if let Some(in_path) = find_binary_in_path("gdrive") {
        return Some(in_path.to_string_lossy().into_owned());
    }

    for candidate in ["~/.local/bin/gdrive", "~/.gemini/antigravity/bin/gdrive"] {
        let path = expand_tilde(candidate);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

fn parse_rclone_drive_remotes(stdout: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    let mut remotes = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((name, kind)) = trimmed.split_once(':') {
            let remote_type = kind.split_whitespace().next().unwrap_or("");
            if remote_type == "drive" {
                let clean = name.trim();
                if !clean.is_empty() && !remotes.iter().any(|r| r == clean) {
                    remotes.push(clean.to_string());
                }
            }
        }
    }
    remotes
}

async fn list_rclone_drive_remotes() -> (bool, Vec<String>) {
    let Some(rclone_bin) = find_binary_in_path("rclone") else {
        return (false, Vec::new());
    };
    let bin_str = rclone_bin.to_string_lossy().into_owned();
    let Ok(out) = run_cli_with_timeout(
        &bin_str,
        &["listremotes", "--long"],
        CLI_DETECT_TIMEOUT,
        "listremotes",
    )
    .await
    else {
        return (true, Vec::new());
    };
    if !out.status.success() {
        return (true, Vec::new());
    }
    (true, parse_rclone_drive_remotes(&out.stdout))
}

async fn is_gdrive_cli_authenticated(cli: &str) -> bool {
    run_cli_with_timeout(
        cli,
        &["readonly", "quota", "--json"],
        CLI_DETECT_TIMEOUT,
        "quota",
    )
    .await
    .map(|out| out.status.success())
    .unwrap_or(false)
}

fn select_rclone_remote(remotes: &[String], config: &BackupConfig) -> Option<String> {
    if remotes.is_empty() {
        return None;
    }
    if let Some(configured) = config
        .rclone_remote
        .as_deref()
        .map(|s| s.trim().trim_end_matches(':'))
        .filter(|s| !s.is_empty())
    {
        if let Some(found) = remotes.iter().find(|r| r.as_str() == configured) {
            return Some(found.clone());
        }
    }
    if let Ok(env_remote) = std::env::var("LOG_RCLONE_REMOTE") {
        let trimmed = env_remote.trim().trim_end_matches(':');
        if !trimmed.is_empty() {
            if let Some(found) = remotes.iter().find(|r| r.as_str() == trimmed) {
                return Some(found.clone());
            }
        }
    }
    remotes.first().cloned()
}

/// Inspect Google Drive authentication state and locally available sync transports.
pub async fn get_gdrive_status(config: &BackupConfig) -> GdriveAuthStatus {
    let (rclone_res, cli_opt) = tokio::join!(list_rclone_drive_remotes(), async {
        match find_gdrive_cli() {
            Some(cli) => {
                let authed = is_gdrive_cli_authenticated(&cli).await;
                Some((cli, authed))
            }
            None => None,
        }
    });

    let (has_rclone, rclone_remotes) = rclone_res;
    let has_gdrive_cli = cli_opt.is_some();
    let gdrive_cli_authed = cli_opt.as_ref().map(|(_, authed)| *authed).unwrap_or(false);
    let auth = load_stored_gdrive_auth();
    let saved_client_id = auth
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let env_token_set = std::env::var("LOG_GDRIVE_TOKEN")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    if env_token_set {
        return GdriveAuthStatus {
            connected: true,
            method: "env_token".to_string(),
            account_hint: None,
            detail: "Using LOG_GDRIVE_TOKEN environment variable".to_string(),
            has_rclone,
            rclone_remotes,
            has_gdrive_cli,
            saved_client_id,
        };
    }

    let has_refresh = auth
        .refresh_token
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if has_refresh && saved_client_id.is_some() {
        return GdriveAuthStatus {
            connected: true,
            method: "oauth".to_string(),
            account_hint: auth.account_email,
            detail: "Connected via Google OAuth 2.0".to_string(),
            has_rclone,
            rclone_remotes,
            has_gdrive_cli,
            saved_client_id,
        };
    }

    let has_valid_access = auth
        .access_token
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
        && auth.expires_at.map(|exp| exp > Utc::now()).unwrap_or(true);
    if has_valid_access {
        return GdriveAuthStatus {
            connected: true,
            method: "token".to_string(),
            account_hint: auth.account_email,
            detail: "Connected via saved Google Drive access token".to_string(),
            has_rclone,
            rclone_remotes,
            has_gdrive_cli,
            saved_client_id,
        };
    }

    if let Some(remote) = select_rclone_remote(&rclone_remotes, config) {
        return GdriveAuthStatus {
            connected: true,
            method: "rclone".to_string(),
            account_hint: Some(format!("{remote}:")),
            detail: format!("Connected via rclone remote '{remote}:'"),
            has_rclone,
            rclone_remotes,
            has_gdrive_cli,
            saved_client_id,
        };
    }

    if gdrive_cli_authed {
        return GdriveAuthStatus {
            connected: true,
            method: "gdrive_cli".to_string(),
            account_hint: None,
            detail: "Connected via gdrive CLI".to_string(),
            has_rclone,
            rclone_remotes,
            has_gdrive_cli,
            saved_client_id,
        };
    }

    GdriveAuthStatus {
        connected: false,
        method: "none".to_string(),
        account_hint: None,
        detail: "Google Drive not connected yet. Connect below via Google OAuth, rclone, or an access token.".to_string(),
        has_rclone,
        rclone_remotes,
        has_gdrive_cli,
        saved_client_id,
    }
}

fn sanitize_rclone_remote_name(remote_name: Option<&str>) -> String {
    let raw = remote_name
        .unwrap_or("logjournal")
        .trim()
        .trim_end_matches(':');
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    if clean.is_empty() {
        "logjournal".to_string()
    } else {
        clean
    }
}

/// Connect Google Drive via `rclone` browser sign-in (`rclone config create <name> drive`).
pub async fn connect_gdrive_rclone(remote_name: Option<&str>) -> Result<GdriveAuthStatus> {
    let Some(rclone_bin) = find_binary_in_path("rclone") else {
        return Err(SyncError::InvalidInput(
            "rclone is not installed. Install it on Arch Linux with: sudo pacman -S rclone"
                .to_string(),
        ));
    };

    let name = sanitize_rclone_remote_name(remote_name);
    let (_, existing_remotes) = list_rclone_drive_remotes().await;
    if !existing_remotes.iter().any(|r| r == &name) {
        let bin_str = rclone_bin.to_string_lossy().into_owned();
        let out = run_cli_with_timeout(
            &bin_str,
            &["config", "create", &name, "drive", "scope=drive.file"],
            RCLONE_AUTH_TIMEOUT,
            "config create",
        )
        .await
        .map_err(SyncError::InvalidInput)?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let detail = if !stderr.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };
            return Err(SyncError::InvalidInput(format!(
                "rclone Google Drive sign-in failed: {detail}"
            )));
        }
    }

    let (_, updated_remotes) = list_rclone_drive_remotes().await;
    if !updated_remotes.iter().any(|r| r == &name) {
        return Err(SyncError::InvalidInput(format!(
            "rclone remote '{name}:' was not detected as a Google Drive remote after sign-in."
        )));
    }

    let mut cfg = load_backup_config();
    cfg.rclone_remote = Some(name);
    let saved = save_backup_config(&cfg)?;
    Ok(get_gdrive_status(&saved).await)
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex_str, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query_params(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(percent_decode(k), percent_decode(v));
    }
    map
}

fn spawn_browser_open(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

fn escape_html_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn format_oauth_token_exchange_error(status: reqwest::StatusCode, body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    if lower.contains("client_secret") {
        return "Google Desktop OAuth requires both your Client ID and Client Secret. Paste the Client Secret from Google Cloud Console and click Sign in with Browser again.".to_string();
    }
    if lower.contains("invalid_client") {
        return "Google rejected the OAuth Client ID or Client Secret (invalid_client). Verify both credentials in Google Cloud Console and retry.".to_string();
    }
    let snippet = body.trim();
    if snippet.is_empty() {
        format!("Google token exchange failed (HTTP {status}).")
    } else {
        let short: String = snippet.chars().take(280).collect();
        format!("Google token exchange failed (HTTP {status}): {short}")
    }
}

async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status_line: &str,
    body_html: &str,
) {
    let body = body_html.as_bytes();
    let header = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.flush().await;
}

async fn wait_for_oauth_callback(
    listener: TcpListener,
    expected_state: &str,
) -> Result<(tokio::net::TcpStream, String, Option<String>)> {
    let deadline = tokio::time::Instant::now() + OAUTH_FLOW_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(SyncError::InvalidInput(
                "Timed out waiting for Google OAuth sign-in callback in your browser.".to_string(),
            ));
        }

        let accept_res = tokio::time::timeout(remaining, listener.accept()).await;
        let (mut stream, _) = match accept_res {
            Ok(Ok(conn)) => conn,
            Ok(Err(e)) => return Err(SyncError::Io(e)),
            Err(_) => {
                return Err(SyncError::InvalidInput(
                    "Timed out waiting for Google OAuth sign-in callback in your browser."
                        .to_string(),
                ));
            }
        };

        let mut request_line = String::new();
        {
            let mut reader = TokioBufReader::new(&mut stream);
            let _ =
                tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut request_line))
                    .await;
            for _ in 0..64 {
                let mut hdr = String::new();
                match tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut hdr)).await
                {
                    Ok(Ok(n)) if n > 0 && hdr != "\r\n" && hdr != "\n" => continue,
                    _ => break,
                }
            }
        }

        let target = request_line.split_whitespace().nth(1).unwrap_or("/");
        if target.starts_with("/favicon.ico") {
            write_http_response(&mut stream, "404 Not Found", "").await;
            continue;
        }
        if !target.starts_with("/callback") {
            write_http_response(
                &mut stream,
                "404 Not Found",
                "<html><body><p>Waiting for Google OAuth callback.</p></body></html>",
            )
            .await;
            continue;
        }

        let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
        let params = parse_query_params(query);

        if let Some(err_code) = params.get("error") {
            let desc = params
                .get("error_description")
                .map(|d| d.replace('+', " "))
                .unwrap_or_else(|| err_code.clone());
            write_http_response(
                &mut stream,
                "400 Bad Request",
                "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2rem;\"><h2>Google Drive sign-in cancelled</h2><p>You can close this tab and return to Log.</p></body></html>",
            )
            .await;
            return Err(SyncError::InvalidInput(format!(
                "Google OAuth authorization declined: {desc}"
            )));
        }

        let state = params.get("state").map(String::as_str).unwrap_or("");
        if state != expected_state {
            write_http_response(
                &mut stream,
                "400 Bad Request",
                "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2rem;\"><h2>Invalid OAuth state</h2><p>Please retry sign-in from Log.</p></body></html>",
            )
            .await;
            return Err(SyncError::InvalidInput(
                "Google OAuth state mismatch on loopback callback.".to_string(),
            ));
        }

        let Some(code) = params
            .get("code")
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
        else {
            write_http_response(
                &mut stream,
                "400 Bad Request",
                "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2rem;\"><h2>Missing authorization code</h2><p>Please retry sign-in from Log.</p></body></html>",
            )
            .await;
            return Err(SyncError::InvalidInput(
                "Google OAuth callback did not include an authorization code.".to_string(),
            ));
        };

        // Keep the HTTP response open until token exchange & credential persistence succeed
        // so the browser only shows "connected" when Log actually stored valid credentials.
        return Ok((stream, code, params.get("scope").cloned()));
    }
}

async fn fetch_google_account_email(
    client: &reqwest::Client,
    access_token: &str,
) -> Option<String> {
    let resp = client
        .get(GDRIVE_ABOUT_API)
        .query(&[("fields", "user(emailAddress)")])
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let about: DriveAboutResponse = resp.json().await.ok()?;
    about
        .user
        .and_then(|u| u.email_address)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Connect Google Drive via pure-Rust RFC 8252 + RFC 7636 PKCE loopback OAuth 2.0 flow.
pub async fn connect_gdrive_oauth(req: &ConnectGdriveOauthRequest) -> Result<GdriveAuthStatus> {
    let client_id = req.client_id.trim().to_string();
    if client_id.is_empty() {
        return Err(SyncError::InvalidInput(
            "Google OAuth Client ID must not be empty.".to_string(),
        ));
    }
    let existing_auth = load_stored_gdrive_auth();
    let client_secret = req
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            if existing_auth.client_id.as_deref().map(str::trim) == Some(client_id.as_str()) {
                existing_auth
                    .client_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            } else {
                None
            }
        });

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let code_verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge_digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge_digest);
    let state = Uuid::new_v4().simple().to_string();

    let auth_url = format!(
        "{GOOGLE_AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent&state={}",
        percent_encode(&client_id),
        percent_encode(&redirect_uri),
        percent_encode(GOOGLE_OAUTH_SCOPE),
        percent_encode(&code_challenge),
        percent_encode(&state),
    );

    if let Err(err) = spawn_browser_open(&auth_url) {
        return Err(SyncError::InvalidInput(format!(
            "Failed to open browser for Google sign-in ({err}). Open this URL manually: {auth_url}"
        )));
    }

    let (mut callback_stream, code, callback_scope) =
        wait_for_oauth_callback(listener, &state).await?;

    let client = match shared_gdrive_client() {
        Ok(c) => c,
        Err(err) => {
            let msg = format!("Google Drive HTTP client unavailable: {err}");
            let html = format!(
                "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2.5rem;max-width:540px;margin:auto;\"><h2 style=\"color:#B5563E;\">Google Drive sign-in did not complete</h2><p>{}</p><p>You may close this tab and return to Log.</p></body></html>",
                escape_html_text(&msg)
            );
            write_http_response(&mut callback_stream, "500 Internal Server Error", &html).await;
            return Err(SyncError::InvalidInput(msg));
        }
    };
    let mut form_params: Vec<(&str, &str)> = vec![
        ("client_id", &client_id),
        ("code", &code),
        ("code_verifier", &code_verifier),
        ("redirect_uri", &redirect_uri),
        ("grant_type", "authorization_code"),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form_params.push(("client_secret", secret));
    }

    let token_resp = match client
        .post(GOOGLE_TOKEN_URL)
        .form(&form_params)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Google token exchange failed: {e}");
            let html = format!(
                "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2.5rem;max-width:540px;margin:auto;\"><h2 style=\"color:#B5563E;\">Google Drive sign-in did not complete</h2><p>{}</p><p>You may close this tab and return to Log.</p></body></html>",
                escape_html_text(&msg)
            );
            write_http_response(&mut callback_stream, "502 Bad Gateway", &html).await;
            return Err(SyncError::InvalidInput(msg));
        }
    };

    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        let msg = format_oauth_token_exchange_error(status, &body);
        let html = format!(
            "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2.5rem;max-width:540px;margin:auto;\"><h2 style=\"color:#B5563E;\">Google Drive sign-in did not complete</h2><p>{}</p><p>You may close this tab and return to Log.</p></body></html>",
            escape_html_text(&msg)
        );
        write_http_response(&mut callback_stream, "400 Bad Request", &html).await;
        return Err(SyncError::InvalidInput(msg));
    }

    let token_data: GoogleTokenResponse = match token_resp.json().await {
        Ok(data) => data,
        Err(e) => {
            let msg = format!("Invalid Google token response: {e}");
            let html = format!(
                "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2.5rem;max-width:540px;margin:auto;\"><h2 style=\"color:#B5563E;\">Google Drive sign-in did not complete</h2><p>{}</p><p>You may close this tab and return to Log.</p></body></html>",
                escape_html_text(&msg)
            );
            write_http_response(&mut callback_stream, "502 Bad Gateway", &html).await;
            return Err(SyncError::InvalidInput(msg));
        }
    };

    let granted_scope = token_data
        .scope
        .as_deref()
        .or(callback_scope.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(scope_str) = granted_scope {
        if !has_required_drive_scope(scope_str) {
            let msg = "Google Drive file permission was not granted during sign-in. Sign in again and allow Log to access Google Drive files created by this app.".to_string();
            let html = format!(
                "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2.5rem;max-width:540px;margin:auto;\"><h2 style=\"color:#B5563E;\">Google Drive permission missing</h2><p>{}</p><p>You may close this tab and retry from Log.</p></body></html>",
                escape_html_text(&msg)
            );
            write_http_response(&mut callback_stream, "403 Forbidden", &html).await;
            return Err(SyncError::InvalidInput(msg));
        }
    }

    let mut auth = load_stored_gdrive_auth();
    let email = fetch_google_account_email(client, &token_data.access_token).await;
    auth.client_id = Some(client_id);
    if let Some(secret) = client_secret {
        auth.client_secret = Some(secret);
    }
    auth.access_token = Some(token_data.access_token);
    if let Some(refresh) = token_data.refresh_token.filter(|s| !s.trim().is_empty()) {
        auth.refresh_token = Some(refresh);
    }
    auth.expires_at = token_data
        .expires_in
        .map(|secs| Utc::now() + ChronoDuration::seconds(secs));
    if email.is_some() {
        auth.account_email = email.clone();
    }
    if let Err(err) = save_stored_gdrive_auth(&auth) {
        let msg = format!("Failed to save Google Drive credentials locally: {err}");
        let html = format!(
            "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2.5rem;max-width:540px;margin:auto;\"><h2 style=\"color:#B5563E;\">Could not save Google Drive credentials</h2><p>{}</p></body></html>",
            escape_html_text(&msg)
        );
        write_http_response(&mut callback_stream, "500 Internal Server Error", &html).await;
        return Err(err);
    }

    let account_line = email
        .as_deref()
        .map(|e| format!(" as <strong>{}</strong>", escape_html_text(e)))
        .unwrap_or_default();
    let success_html = format!(
        "<html><body style=\"font-family:Georgia,serif;background:#FBF7F0;color:#26201D;padding:2.5rem;max-width:520px;margin:auto;\"><h2>Log is connected to Google Drive</h2><p>Encrypted vault backups are now authorized{account_line}. You may close this browser tab and return to Log.</p></body></html>"
    );
    write_http_response(&mut callback_stream, "200 OK", &success_html).await;

    Ok(get_gdrive_status(&load_backup_config()).await)
}

/// Validate and store a direct Google Drive OAuth access token in `<config_dir>/gdrive-auth.json`.
pub async fn save_gdrive_token(access_token: &str) -> Result<GdriveAuthStatus> {
    let token = access_token.trim().to_string();
    if token.is_empty() {
        return Err(SyncError::InvalidInput(
            "Google Drive access token must not be empty.".to_string(),
        ));
    }

    let client = shared_gdrive_client().map_err(SyncError::InvalidInput)?;
    let resp = client
        .get(GDRIVE_ABOUT_API)
        .query(&[("fields", "user(emailAddress)")])
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| SyncError::InvalidInput(format!("Could not verify access token: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(SyncError::InvalidInput(format!(
            "Google Drive rejected the access token (HTTP {status}): {body}"
        )));
    }

    let about: DriveAboutResponse = resp
        .json()
        .await
        .unwrap_or(DriveAboutResponse { user: None });
    let email = about
        .user
        .and_then(|u| u.email_address)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut auth = load_stored_gdrive_auth();
    auth.access_token = Some(token);
    auth.refresh_token = None;
    auth.expires_at = None;
    auth.account_email = email;
    save_stored_gdrive_auth(&auth)?;

    Ok(get_gdrive_status(&load_backup_config()).await)
}

/// Clear stored Google Drive tokens while preserving any saved OAuth `client_id`.
pub async fn disconnect_gdrive() -> Result<GdriveAuthStatus> {
    let path = gdrive_auth_path();
    if path.exists() {
        let mut auth = load_stored_gdrive_auth();
        let has_client = auth
            .client_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        if has_client {
            auth.access_token = None;
            auth.refresh_token = None;
            auth.expires_at = None;
            auth.account_email = None;
            save_stored_gdrive_auth(&auth)?;
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(get_gdrive_status(&load_backup_config()).await)
}

async fn refresh_oauth_access_token(
    client: &reqwest::Client,
    auth: &mut StoredGdriveAuth,
) -> std::result::Result<String, String> {
    let client_id = auth
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing OAuth client_id for token refresh".to_string())?
        .to_string();
    let refresh_token = auth
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing OAuth refresh_token".to_string())?
        .to_string();

    let mut form_params: Vec<(&str, &str)> = vec![
        ("client_id", &client_id),
        ("refresh_token", &refresh_token),
        ("grant_type", "refresh_token"),
    ];
    let client_secret = auth
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(secret) = client_secret.as_deref() {
        form_params.push(("client_secret", secret));
    }

    let resp = client
        .post(GOOGLE_TOKEN_URL)
        .form(&form_params)
        .send()
        .await
        .map_err(|e| format!("OAuth refresh request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("OAuth refresh HTTP {status}: {body}"));
    }

    let refreshed: GoogleTokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("invalid OAuth refresh response: {e}"))?;
    auth.access_token = Some(refreshed.access_token.clone());
    if let Some(new_refresh) = refreshed.refresh_token.filter(|s| !s.trim().is_empty()) {
        auth.refresh_token = Some(new_refresh);
    }
    auth.expires_at = refreshed
        .expires_in
        .map(|secs| Utc::now() + ChronoDuration::seconds(secs));
    save_stored_gdrive_auth(auth).map_err(|e| e.to_string())?;
    Ok(refreshed.access_token)
}

/// Create (when `passphrase_opt` is provided) or select the latest `.age`-encrypted
/// vault archive and upload it to a single Google Drive folder (`config.gdrive_folder`,
/// defaulting to `"LogJournal"`).
pub async fn sync_gdrive(
    vault_dir: &Path,
    passphrase_opt: Option<&str>,
    config: &BackupConfig,
) -> Result<SyncResult> {
    sync_gdrive_with_target(vault_dir, passphrase_opt, None, config).await
}

fn mirror_archive_to_destination(
    backups_dir: &Path,
    archive_path: &Path,
    backup: &BackupInfo,
    config: &BackupConfig,
) -> Result<()> {
    let Some(dest_str) = config.destination_dir.as_deref() else {
        return Ok(());
    };
    let trimmed = dest_str.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let dest_dir = expand_tilde(trimmed);
    if dest_dir == backups_dir {
        return Ok(());
    }
    fs::create_dir_all(&dest_dir)?;
    let dest_archive = dest_dir.join(&backup.filename);
    if !dest_archive.exists() {
        fs::copy(archive_path, &dest_archive)?;
    }
    write_sidecars(&dest_archive, backup)?;
    Ok(())
}

/// Upload a specific local `.age` backup (`backup_id_opt`), create a new encrypted snapshot
/// (`passphrase_opt`), or upload the newest existing encrypted snapshot to Google Drive.
pub async fn sync_gdrive_with_target(
    vault_dir: &Path,
    passphrase_opt: Option<&str>,
    backup_id_opt: Option<&str>,
    config: &BackupConfig,
) -> Result<SyncResult> {
    let vault_dir_owned = vault_dir.to_path_buf();
    let passphrase_owned = passphrase_opt
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let target_id_owned = backup_id_opt
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let config_owned = config.clone();

    let mut backup = tokio::task::spawn_blocking(move || {
        let mut created_new = false;
        let selected = if let Some(target_id) = target_id_owned {
            let existing = list_backups(&vault_dir_owned)?;
            existing
                .into_iter()
                .find(|b| b.id == target_id || b.filename == target_id)
                .ok_or_else(|| SyncError::NotFound(target_id))?
        } else if let Some(passphrase) = passphrase_owned {
            created_new = true;
            create_backup(&vault_dir_owned, &passphrase, &config_owned)?
        } else {
            let mut existing = list_backups(&vault_dir_owned)?;
            if existing.is_empty() {
                return Err(SyncError::InvalidInput(
                    "No encrypted backup exists yet; provide a passphrase to create one."
                        .to_string(),
                ));
            }
            existing.remove(0)
        };

        if !created_new {
            let backups_dir = vault_dir_owned.join("backups");
            let archive_path = backups_dir.join(&selected.filename);
            if archive_path.is_file() {
                let _ = mirror_archive_to_destination(
                    &backups_dir,
                    &archive_path,
                    &selected,
                    &config_owned,
                );
            }
        }
        Ok(selected)
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

    // 1. LOG_GDRIVE_TOKEN env var.
    if let Ok(token) = std::env::var("LOG_GDRIVE_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            match upload_via_rest(token, folder_name, &archive_path, &backup.filename).await {
                Ok(remote_id) => {
                    let msg = format!(
                        "Encrypted archive '{}' uploaded to Google Drive folder '{folder_name}' (id: {remote_id}).",
                        backup.filename
                    );
                    return finish_remote_sync(vault_dir, &mut backup, Some(remote_id), msg).await;
                }
                Err(err) => {
                    return Ok(local_only_sync_result(
                        backup.clone(),
                        format!(
                            "Local encrypted backup '{}' saved; Google Drive upload unavailable ({}).",
                            backup.filename,
                            err.into_message()
                        ),
                    ));
                }
            }
        }
    }

    // 2. Saved OAuth 2.0 / direct token credentials in gdrive-auth.json.
    let mut stored_auth = load_stored_gdrive_auth();
    let has_refresh = stored_auth
        .refresh_token
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
        && stored_auth
            .client_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
    let stored_access = stored_auth
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if has_refresh || stored_access.is_some() {
        let client = match shared_gdrive_client() {
            Ok(c) => c,
            Err(err) => {
                return Ok(local_only_sync_result(
                    backup.clone(),
                    format!(
                        "Local encrypted backup '{}' saved; Google Drive client unavailable ({err}).",
                        backup.filename
                    ),
                ));
            }
        };

        let needs_refresh = has_refresh
            && (stored_access.is_none()
                || stored_auth
                    .expires_at
                    .map(|exp| exp <= Utc::now() + ChronoDuration::seconds(60))
                    .unwrap_or(false));

        let token_res = if needs_refresh {
            refresh_oauth_access_token(client, &mut stored_auth).await
        } else if let Some(token) = stored_access {
            Ok(token)
        } else {
            refresh_oauth_access_token(client, &mut stored_auth).await
        };

        match token_res {
            Ok(token) => {
                let mut upload_res =
                    upload_via_rest(&token, folder_name, &archive_path, &backup.filename).await;
                if matches!(upload_res, Err(RestUploadError::Unauthorized(_))) && has_refresh {
                    if let Ok(fresh_token) =
                        refresh_oauth_access_token(client, &mut stored_auth).await
                    {
                        upload_res = upload_via_rest(
                            &fresh_token,
                            folder_name,
                            &archive_path,
                            &backup.filename,
                        )
                        .await;
                    }
                }
                match upload_res {
                    Ok(remote_id) => {
                        let msg = format!(
                            "Encrypted archive '{}' uploaded to Google Drive folder '{folder_name}' (id: {remote_id}).",
                            backup.filename
                        );
                        return finish_remote_sync(vault_dir, &mut backup, Some(remote_id), msg)
                            .await;
                    }
                    Err(err) => {
                        return Ok(local_only_sync_result(
                            backup.clone(),
                            format!(
                                "Local encrypted backup '{}' saved; Google Drive upload unavailable ({}).",
                                backup.filename,
                                err.into_message()
                            ),
                        ));
                    }
                }
            }
            Err(err) => {
                return Ok(local_only_sync_result(
                    backup.clone(),
                    format!(
                        "Local encrypted backup '{}' saved; Google Drive OAuth refresh failed ({err}).",
                        backup.filename
                    ),
                ));
            }
        }
    }

    // 3. rclone Google Drive remote.
    let (has_rclone, rclone_remotes) = list_rclone_drive_remotes().await;
    if has_rclone {
        if let Some(remote) = select_rclone_remote(&rclone_remotes, config) {
            match upload_via_rclone(&remote, folder_name, &archive_path, &backup.filename).await {
                Ok(remote_id) => {
                    let msg = format!(
                        "Encrypted archive '{}' synced to Google Drive '{folder_name}' via rclone ({remote}:).",
                        backup.filename
                    );
                    return finish_remote_sync(vault_dir, &mut backup, Some(remote_id), msg).await;
                }
                Err(err) => {
                    return Ok(local_only_sync_result(
                        backup.clone(),
                        format!(
                            "Local encrypted backup '{}' saved; rclone upload unavailable ({err}).",
                            backup.filename
                        ),
                    ));
                }
            }
        }
    }

    // 4. Auto-detected gdrive CLI (when not disabled).
    if let Some(cli) = find_gdrive_cli() {
        let force_cli = std::env::var("LOG_GDRIVE_USE_CLI")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        if force_cli || is_gdrive_cli_authenticated(&cli).await {
            match upload_via_cli(&cli, folder_name, &archive_path, &backup.filename).await {
                Ok(remote_id_opt) => {
                    let msg = format!(
                        "Encrypted archive '{}' synced to Google Drive folder '{folder_name}' via gdrive CLI.",
                        backup.filename
                    );
                    return finish_remote_sync(vault_dir, &mut backup, remote_id_opt, msg).await;
                }
                Err(err) => {
                    return Ok(local_only_sync_result(
                        backup.clone(),
                        format!(
                            "Local encrypted backup '{}' saved; gdrive CLI sync skipped ({err}).",
                            backup.filename
                        ),
                    ));
                }
            }
        }
    }

    Ok(local_only_sync_result(
        backup.clone(),
        format!(
            "Local encrypted backup '{}' saved. Connect Google Drive in Backup Settings (via Google OAuth, rclone, or access token) to upload remotely.",
            backup.filename
        ),
    ))
}

fn local_only_sync_result(backup: BackupInfo, message: String) -> SyncResult {
    SyncResult {
        success: true,
        backup: Some(backup),
        uploaded_remote: false,
        message,
        synced_at: Utc::now(),
    }
}

async fn finish_remote_sync(
    vault_dir: &Path,
    backup: &mut BackupInfo,
    remote_id: Option<String>,
    message: String,
) -> Result<SyncResult> {
    backup.location = "gdrive".to_string();
    backup.remote_id = remote_id;
    let sidecar_vault = vault_dir.to_path_buf();
    let sidecar_backup = backup.clone();
    let _ =
        tokio::task::spawn_blocking(move || update_backup_sidecar(&sidecar_vault, &sidecar_backup))
            .await;
    Ok(SyncResult {
        success: true,
        backup: Some(backup.clone()),
        uploaded_remote: true,
        message,
        synced_at: Utc::now(),
    })
}

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

fn escape_drive_query(input: &str) -> String {
    input.replace('\\', "\\\\").replace('\'', "\\'")
}

fn classify_http_error(status: reqwest::StatusCode, context: &str, body: &str) -> RestUploadError {
    if status == reqwest::StatusCode::FORBIDDEN
        && (body.contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT")
            || body.contains("insufficientPermissions")
            || body.contains("insufficient authentication scopes"))
    {
        return RestUploadError::Other(
            "Google Drive permission was not granted for this sign-in (insufficient scope). Click Disconnect in Backup Settings, sign in again, and allow Google Drive file access.".to_string(),
        );
    }
    let msg = format!("{context} HTTP {status}: {body}");
    if status == reqwest::StatusCode::UNAUTHORIZED {
        RestUploadError::Unauthorized(msg)
    } else {
        RestUploadError::Other(msg)
    }
}

async fn find_existing_drive_file(
    client: &reqwest::Client,
    bearer_token: &str,
    folder_id: &str,
    filename: &str,
) -> std::result::Result<Option<String>, RestUploadError> {
    let escaped_name = escape_drive_query(filename);
    let escaped_parent = escape_drive_query(folder_id);
    let q =
        format!("name = '{escaped_name}' and '{escaped_parent}' in parents and trashed = false");
    let resp = client
        .get(GDRIVE_FILES_API)
        .bearer_auth(bearer_token)
        .query(&[
            ("q", q.as_str()),
            ("fields", "files(id,name)"),
            ("pageSize", "10"),
        ])
        .send()
        .await
        .map_err(|e| RestUploadError::Other(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(classify_http_error(status, "Drive file search", &body));
    }

    let list: DriveFileList = resp
        .json()
        .await
        .map_err(|e| RestUploadError::Other(e.to_string()))?;
    Ok(list
        .files
        .into_iter()
        .find(|f| f.name == filename)
        .map(|f| f.id))
}

async fn upload_via_rest(
    bearer_token: &str,
    folder_name: &str,
    archive_path: &Path,
    filename: &str,
) -> std::result::Result<String, RestUploadError> {
    let client = shared_gdrive_client().map_err(RestUploadError::Other)?;
    let folder_id = ensure_drive_folder(client, bearer_token, folder_name).await?;

    if let Some(existing_id) =
        find_existing_drive_file(client, bearer_token, &folder_id, filename).await?
    {
        return Ok(existing_id);
    }

    let total_len = tokio::fs::metadata(archive_path)
        .await
        .map_err(|e| RestUploadError::Other(format!("failed to stat archive: {e}")))?
        .len();

    if total_len <= MULTIPART_UPLOAD_THRESHOLD_BYTES {
        let bytes = tokio::fs::read(archive_path)
            .await
            .map_err(|e| RestUploadError::Other(format!("failed to read archive: {e}")))?;

        let metadata = serde_json::json!({
            "name": filename,
            "parents": [folder_id],
        });

        let metadata_part = Part::text(metadata.to_string())
            .mime_str("application/json; charset=UTF-8")
            .map_err(|e| RestUploadError::Other(e.to_string()))?;
        let media_part = Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| RestUploadError::Other(e.to_string()))?;

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
            .map_err(|e| RestUploadError::Other(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(classify_http_error(status, "Drive upload", &body));
        }

        let uploaded: DriveFileEntry = resp
            .json()
            .await
            .map_err(|e| RestUploadError::Other(e.to_string()))?;
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
) -> std::result::Result<String, RestUploadError> {
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
        .map_err(|e| RestUploadError::Other(e.to_string()))?;

    if !init_resp.status().is_success() {
        let status = init_resp.status();
        let body = init_resp.text().await.unwrap_or_default();
        return Err(classify_http_error(status, "Drive resumable init", &body));
    }

    let session_uri = init_resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| {
            RestUploadError::Other("Drive resumable init missing Location header".to_string())
        })?;

    let mut file = tokio::fs::File::open(archive_path)
        .await
        .map_err(|e| RestUploadError::Other(format!("failed to open archive: {e}")))?;
    let mut offset: u64 = 0;

    loop {
        let mut buf = vec![0u8; RESUMABLE_CHUNK_BYTES];
        let mut filled = 0usize;
        while filled < buf.len() {
            let n = file.read(&mut buf[filled..]).await.map_err(|e| {
                RestUploadError::Other(format!("failed to read archive chunk: {e}"))
            })?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            return Err(RestUploadError::Other(
                "Drive resumable upload ended before server returned file id".to_string(),
            ));
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
            .map_err(|e| RestUploadError::Other(e.to_string()))?;

        let status = chunk_resp.status();
        if status.as_u16() == 308 {
            offset = end + 1;
            continue;
        }
        if !status.is_success() {
            let body = chunk_resp.text().await.unwrap_or_default();
            return Err(classify_http_error(status, "Drive resumable chunk", &body));
        }

        let uploaded: DriveFileEntry = chunk_resp
            .json()
            .await
            .map_err(|e| RestUploadError::Other(e.to_string()))?;
        return Ok(uploaded.id);
    }
}

async fn ensure_drive_folder(
    client: &reqwest::Client,
    bearer_token: &str,
    folder_name: &str,
) -> std::result::Result<String, RestUploadError> {
    let escaped = escape_drive_query(folder_name);
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
        .map_err(|e| RestUploadError::Other(e.to_string()))?;

    if !search_resp.status().is_success() {
        let status = search_resp.status();
        let body = search_resp.text().await.unwrap_or_default();
        return Err(classify_http_error(status, "Drive folder search", &body));
    }

    let list: DriveFileList = search_resp
        .json()
        .await
        .map_err(|e| RestUploadError::Other(e.to_string()))?;
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
        .map_err(|e| RestUploadError::Other(e.to_string()))?;

    if !create_resp.status().is_success() {
        let status = create_resp.status();
        let body = create_resp.text().await.unwrap_or_default();
        return Err(classify_http_error(status, "Drive mkdir", &body));
    }

    let created: DriveFileEntry = create_resp
        .json()
        .await
        .map_err(|e| RestUploadError::Other(e.to_string()))?;
    Ok(created.id)
}

async fn upload_via_rclone(
    remote: &str,
    folder_name: &str,
    archive_path: &Path,
    filename: &str,
) -> std::result::Result<String, String> {
    let rclone_bin = find_binary_in_path("rclone")
        .ok_or_else(|| "rclone binary not found in PATH".to_string())?;
    let bin_str = rclone_bin.to_string_lossy().into_owned();
    let archive_str = archive_path.to_string_lossy().into_owned();
    let remote_dest = format!("{remote}:{folder_name}/{filename}");

    let out = run_cli_with_timeout(
        &bin_str,
        &["copyto", &archive_str, &remote_dest],
        REMOTE_UPLOAD_TIMEOUT,
        "copyto",
    )
    .await?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("rclone copyto failed: {}", err.trim()));
    }

    Ok(format!("rclone:{remote}:{folder_name}/{filename}"))
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
    cli: &str,
    folder_name: &str,
    archive_path: &Path,
    filename: &str,
) -> std::result::Result<Option<String>, String> {
    let search_out = run_cli_with_timeout(
        cli,
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

    let mut folder_id = parse_cli_first_id(&search_out.stdout).unwrap_or_default();
    if folder_id.is_empty() {
        let mkdir_out = run_cli_with_timeout(
            cli,
            &["mutate", "mkdir", folder_name],
            CLI_QUERY_TIMEOUT,
            "mkdir",
        )
        .await?;
        if !mkdir_out.status.success() {
            let err = String::from_utf8_lossy(&mkdir_out.stderr);
            return Err(format!("{cli} mkdir failed: {}", err.trim()));
        }
        let lookup_out = run_cli_with_timeout(
            cli,
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
            "folder lookup",
        )
        .await?;
        if !lookup_out.status.success() {
            let err = String::from_utf8_lossy(&lookup_out.stderr);
            return Err(format!("{cli} folder lookup failed: {}", err.trim()));
        }
        folder_id = parse_cli_first_id(&lookup_out.stdout)
            .ok_or_else(|| "gdrive mkdir succeeded but folder id was not found".to_string())?;
    }

    let existing_out = run_cli_with_timeout(
        cli,
        &[
            "readonly",
            "search",
            "--parent-id",
            &folder_id,
            "--name-exact",
            filename,
            "--json",
        ],
        CLI_QUERY_TIMEOUT,
        "file check",
    )
    .await?;
    if existing_out.status.success() {
        if let Some(existing_id) = parse_cli_first_id(&existing_out.stdout) {
            return Ok(Some(existing_id));
        }
    }

    let archive_str = archive_path.to_string_lossy().into_owned();
    let upload_out = run_cli_with_timeout(
        cli,
        &[
            "mutate",
            "upload",
            &archive_str,
            "--parent",
            &folder_id,
            "--mime-type",
            "application/octet-stream",
        ],
        REMOTE_UPLOAD_TIMEOUT,
        "upload",
    )
    .await?;

    if !upload_out.status.success() {
        let err = String::from_utf8_lossy(&upload_out.stderr);
        return Err(format!("{cli} upload failed: {}", err.trim()));
    }

    let verify_out = run_cli_with_timeout(
        cli,
        &[
            "readonly",
            "search",
            "--parent-id",
            &folder_id,
            "--name-exact",
            filename,
            "--json",
        ],
        CLI_QUERY_TIMEOUT,
        "upload verify",
    )
    .await?;
    if verify_out.status.success() {
        return Ok(parse_cli_first_id(&verify_out.stdout));
    }
    Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_error_formatter_explains_missing_client_secret() {
        let msg = format_oauth_token_exchange_error(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_request","error_description":"client_secret is missing."}"#,
        );
        assert!(msg.contains("Client Secret"));
        assert!(msg.contains("Google Cloud Console"));

        let invalid = format_oauth_token_exchange_error(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":"invalid_client","error_description":"Unauthorized"}"#,
        );
        assert!(invalid.contains("invalid_client"));
    }

    #[test]
    fn html_escape_sanitizes_special_characters() {
        assert_eq!(
            escape_html_text(r#"<script>alert("x" & 'y')</script>"#),
            "&lt;script&gt;alert(&quot;x&quot; &amp; &#39;y&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn drive_scope_check_and_403_scope_classification() {
        assert!(has_required_drive_scope(
            "https://www.googleapis.com/auth/drive.file"
        ));
        assert!(has_required_drive_scope(
            "email https://www.googleapis.com/auth/drive"
        ));
        assert!(!has_required_drive_scope(
            "email https://www.googleapis.com/auth/userinfo.email"
        ));

        let err = classify_http_error(
            reqwest::StatusCode::FORBIDDEN,
            "Drive folder search",
            r#"{"error":{"code":403,"details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#,
        );
        let msg = err.into_message();
        assert!(msg.contains("Disconnect"));
        assert!(msg.contains("insufficient scope"));
    }
}
