//! Client-side `age` passphrase-encrypted vault archive creation, listing, and restoration.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use age::secrecy::SecretString;
use chrono::{DateTime, NaiveDateTime, Utc};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use journal_types::{BackupConfig, BackupInfo, RestoreBackupRequest, RestoreBackupResponse};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::error::{Result, SyncError};

/// Vault subdirectories and files included in encrypted snapshots.
const VAULT_PACK_ITEMS: &[&str] = &["entries", "notebooks.json", "media", "templates"];

/// Writer wrapper that computes SHA-256 and counts bytes while delegating writes.
struct HashingWriter<W: Write> {
    inner: W,
    hasher: Sha256,
    bytes_written: u64,
}

impl<W: Write> HashingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes_written: 0,
        }
    }

    fn finish_hash(mut self) -> io::Result<(W, String, u64)> {
        self.inner.flush()?;
        let digest = hex::encode(self.hasher.finalize());
        Ok((self.inner, digest, self.bytes_written))
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        if written > 0 {
            self.hasher.update(&buf[..written]);
            self.bytes_written += written as u64;
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Expand a leading `~/` against the user's home directory.
pub(crate) fn expand_tilde(path_str: &str) -> PathBuf {
    if let Some(rest) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if path_str == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path_str)
}

/// Pack `entries/`, `notebooks.json`, `media/`, and `templates/` into a `.tar.gz`,
/// encrypt it client-side with `age` passphrase encryption, write `.sha256` and
/// `.meta.json` sidecars, and prune older backups beyond `config.keep_last_n`.
pub fn create_backup(
    vault_dir: &Path,
    passphrase: &str,
    config: &BackupConfig,
) -> Result<BackupInfo> {
    if passphrase.trim().is_empty() {
        return Err(SyncError::InvalidInput(
            "Backup passphrase must not be empty".to_string(),
        ));
    }
    if !vault_dir.exists() {
        return Err(SyncError::NotFound(format!(
            "Vault directory does not exist: {}",
            vault_dir.display()
        )));
    }

    let backups_dir = vault_dir.join("backups");
    fs::create_dir_all(&backups_dir)?;

    let now = Utc::now();
    let base_stem = format!("log-vault-{}", now.format("%Y%m%d-%H%M%S"));
    let (id, filename, archive_path) = unique_backup_path(&backups_dir, &base_stem);

    let out_file = File::create(&archive_path)?;
    let hashing_writer = HashingWriter::new(BufWriter::new(out_file));

    let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(passphrase.to_owned()));
    let age_writer = encryptor
        .wrap_output(hashing_writer)
        .map_err(|e| SyncError::Crypto(format!("Failed to initialize age encryptor: {e}")))?;
    let gz_encoder = GzEncoder::new(age_writer, Compression::default());
    let mut tar_builder = tar::Builder::new(gz_encoder);

    let mut entry_count = 0usize;
    for item_name in VAULT_PACK_ITEMS {
        let item_path = vault_dir.join(item_name);
        if !item_path.exists() {
            continue;
        }
        if item_path.is_file() {
            tar_builder.append_path_with_name(&item_path, item_name)?;
        } else if item_path.is_dir() {
            for walk_res in WalkDir::new(&item_path).follow_links(false) {
                let dir_entry = walk_res.map_err(|e| {
                    SyncError::Io(io::Error::new(io::ErrorKind::Other, e.to_string()))
                })?;
                let abs_path = dir_entry.path();
                let rel_path = abs_path.strip_prefix(vault_dir).map_err(|e| {
                    SyncError::Io(io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
                })?;
                if rel_path.as_os_str().is_empty() {
                    continue;
                }
                if abs_path.is_file()
                    && *item_name == "entries"
                    && abs_path.extension().and_then(|s| s.to_str()) == Some("md")
                {
                    entry_count += 1;
                }
                tar_builder.append_path_with_name(abs_path, rel_path)?;
            }
        }
    }

    let gz_encoder = tar_builder.into_inner()?;
    let age_writer = gz_encoder.finish()?;
    let hashing_writer = age_writer
        .finish()
        .map_err(|e| SyncError::Crypto(format!("Failed to finalize age stream: {e}")))?;
    let (_buf_writer, sha256, size_bytes) = hashing_writer.finish_hash()?;

    let info = BackupInfo {
        id,
        filename: filename.clone(),
        created_at: now,
        size_bytes,
        entry_count,
        sha256: sha256.clone(),
        encrypted: true,
        location: "local".to_string(),
        remote_id: None,
    };

    write_sidecars(&archive_path, &info)?;

    // Mirror encrypted archive & sidecars to optional custom destination directory.
    if let Some(dest_str) = config.destination_dir.as_deref() {
        let trimmed = dest_str.trim();
        if !trimmed.is_empty() {
            let dest_dir = expand_tilde(trimmed);
            if dest_dir != backups_dir {
                fs::create_dir_all(&dest_dir)?;
                let dest_archive = dest_dir.join(&filename);
                fs::copy(&archive_path, &dest_archive)?;
                write_sidecars(&dest_archive, &info)?;
            }
        }
    }

    if config.keep_last_n > 0 {
        prune_old_backups(&backups_dir, config.keep_last_n)?;
    }

    Ok(info)
}

/// List available encrypted `.tar.gz.age` backups in `<vault>/backups/`, newest first.
pub fn list_backups(vault_dir: &Path) -> Result<Vec<BackupInfo>> {
    let backups_dir = vault_dir.join("backups");
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups = Vec::new();
    for entry in fs::read_dir(&backups_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with(".tar.gz.age") {
            continue;
        }
        backups.push(load_backup_info(&path, name)?);
    }

    backups.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.filename.cmp(&a.filename))
    });
    Ok(backups)
}

/// Decrypt, integrity-verify, and unpack an `age`-encrypted vault archive.
pub fn restore_backup(
    vault_dir: &Path,
    req: &RestoreBackupRequest,
) -> Result<RestoreBackupResponse> {
    if req.passphrase.trim().is_empty() {
        return Err(SyncError::InvalidInput(
            "Restore passphrase must not be empty".to_string(),
        ));
    }

    let archive_path = resolve_backup_path(vault_dir, &req.backup_id_or_path)?;
    verify_sha256_sidecar_if_present(&archive_path)?;

    let file = File::open(&archive_path)?;
    let decryptor = age::Decryptor::new_buffered(BufReader::new(file)).map_err(|e| {
        SyncError::Crypto(format!(
            "Failed to parse age archive header for {}: {e}",
            archive_path.display()
        ))
    })?;

    let identity = age::scrypt::Identity::new(SecretString::from(req.passphrase.clone()));
    let decrypted_reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| SyncError::Crypto(format!("Decryption failed (wrong passphrase?): {e}")))?;

    let target_dir = req
        .target_vault_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(expand_tilde)
        .unwrap_or_else(|| vault_dir.to_path_buf());
    fs::create_dir_all(&target_dir)?;

    let gz_decoder = GzDecoder::new(decrypted_reader);
    let mut archive = tar::Archive::new(gz_decoder);

    let mut entries_restored = 0usize;
    let mut media_restored = 0usize;

    for tar_entry_res in archive.entries()? {
        let mut tar_entry = tar_entry_res?;
        let rel_path = tar_entry.path()?.into_owned();

        // Guard against path traversal inside archives.
        if !is_safe_relative_path(&rel_path) {
            return Err(SyncError::Crypto(format!(
                "Refusing unsafe archive path: {}",
                rel_path.display()
            )));
        }
        if rel_path.as_os_str().is_empty() {
            continue;
        }

        let dest_path = target_dir.join(&rel_path);
        let entry_type = tar_entry.header().entry_type();

        if entry_type.is_dir() {
            fs::create_dir_all(&dest_path)?;
            continue;
        }
        if !entry_type.is_file() {
            continue;
        }

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if dest_path.exists() && !req.overwrite_existing {
            continue;
        }

        let mut out = File::create(&dest_path)?;
        io::copy(&mut tar_entry, &mut out)?;
        out.flush()?;

        if rel_path.starts_with("entries")
            && rel_path.extension().and_then(|s| s.to_str()) == Some("md")
        {
            entries_restored += 1;
        } else if rel_path.starts_with("media") {
            media_restored += 1;
        }
    }

    let target_display = target_dir.to_string_lossy().into_owned();
    Ok(RestoreBackupResponse {
        restored: true,
        entries_restored,
        media_restored,
        target_vault_dir: target_display.clone(),
        message: format!(
            "Decrypted and restored {entries_restored} entries and {media_restored} media files into {target_display}"
        ),
    })
}

pub(crate) fn update_backup_sidecar(vault_dir: &Path, info: &BackupInfo) -> Result<()> {
    let archive_path = vault_dir.join("backups").join(&info.filename);
    if archive_path.exists() {
        write_sidecars(&archive_path, info)?;
    }
    Ok(())
}

fn unique_backup_path(backups_dir: &Path, base_stem: &str) -> (String, String, PathBuf) {
    let first_name = format!("{base_stem}.tar.gz.age");
    let first_path = backups_dir.join(&first_name);
    if !first_path.exists() {
        return (base_stem.to_string(), first_name, first_path);
    }
    for suffix in 2..=1000 {
        let id = format!("{base_stem}-{suffix}");
        let filename = format!("{id}.tar.gz.age");
        let path = backups_dir.join(&filename);
        if !path.exists() {
            return (id, filename, path);
        }
    }
    let id = format!("{base_stem}-{}", uuid::Uuid::now_v7());
    let filename = format!("{id}.tar.gz.age");
    let path = backups_dir.join(&filename);
    (id, filename, path)
}

fn sha256_sidecar_path(archive_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sha256", archive_path.display()))
}

fn meta_sidecar_path(archive_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta.json", archive_path.display()))
}

fn write_sidecars(archive_path: &Path, info: &BackupInfo) -> Result<()> {
    let sha_path = sha256_sidecar_path(archive_path);
    fs::write(&sha_path, format!("{}  {}\n", info.sha256, info.filename))?;

    let meta_path = meta_sidecar_path(archive_path);
    let json = serde_json::to_string_pretty(info)?;
    fs::write(&meta_path, json)?;
    Ok(())
}

fn load_backup_info(archive_path: &Path, filename: &str) -> Result<BackupInfo> {
    let meta_path = meta_sidecar_path(archive_path);
    let actual_size = fs::metadata(archive_path)?.len();
    if meta_path.exists() {
        if let Ok(raw) = fs::read_to_string(&meta_path) {
            if let Ok(mut info) = serde_json::from_str::<BackupInfo>(&raw) {
                info.size_bytes = actual_size;
                return Ok(info);
            }
        }
    }

    let sha_path = sha256_sidecar_path(archive_path);
    let sha256 = if sha_path.exists() {
        fs::read_to_string(&sha_path)
            .ok()
            .and_then(|s| s.split_whitespace().next().map(str::to_string))
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| compute_file_sha256(archive_path).unwrap_or_default())
    } else {
        compute_file_sha256(archive_path)?
    };

    let id = filename
        .strip_suffix(".tar.gz.age")
        .unwrap_or(filename)
        .to_string();
    let created_at = parse_timestamp_from_id(&id).unwrap_or_else(|| {
        fs::metadata(archive_path)
            .and_then(|m| m.modified())
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| Utc::now())
    });

    Ok(BackupInfo {
        id,
        filename: filename.to_string(),
        created_at,
        size_bytes: actual_size,
        entry_count: 0,
        sha256,
        encrypted: true,
        location: "local".to_string(),
        remote_id: None,
    })
}

fn parse_timestamp_from_id(id: &str) -> Option<DateTime<Utc>> {
    let rest = id.strip_prefix("log-vault-")?;
    if rest.len() < 15 {
        return None;
    }
    let dt_part = &rest[..15];
    let naive = NaiveDateTime::parse_from_str(dt_part, "%Y%m%d-%H%M%S").ok()?;
    Some(DateTime::from_naive_utc_and_offset(naive, Utc))
}

fn compute_file_sha256(path: &Path) -> Result<String> {
    let mut file = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn verify_sha256_sidecar_if_present(archive_path: &Path) -> Result<()> {
    let sha_path = sha256_sidecar_path(archive_path);
    if !sha_path.exists() {
        return Ok(());
    }
    let expected = fs::read_to_string(&sha_path)?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if expected.is_empty() {
        return Ok(());
    }
    let actual = compute_file_sha256(archive_path)?;
    if actual != expected {
        return Err(SyncError::Crypto(format!(
            "Backup integrity check failed for {}: expected sha256 {expected}, got {actual}",
            archive_path.display()
        )));
    }
    Ok(())
}

fn resolve_backup_path(vault_dir: &Path, backup_id_or_path: &str) -> Result<PathBuf> {
    let trimmed = backup_id_or_path.trim();
    if trimmed.is_empty() {
        return Err(SyncError::InvalidInput(
            "Backup identifier or path must not be empty".to_string(),
        ));
    }

    let direct = expand_tilde(trimmed);
    if direct.is_file() {
        return Ok(direct);
    }

    let backups_dir = vault_dir.join("backups");
    let candidate_exact = backups_dir.join(trimmed);
    if candidate_exact.is_file() {
        return Ok(candidate_exact);
    }

    let candidate_ext = backups_dir.join(format!("{trimmed}.tar.gz.age"));
    if candidate_ext.is_file() {
        return Ok(candidate_ext);
    }

    for info in list_backups(vault_dir)? {
        if info.id == trimmed || info.filename == trimmed {
            let p = backups_dir.join(&info.filename);
            if p.is_file() {
                return Ok(p);
            }
        }
    }

    Err(SyncError::NotFound(trimmed.to_string()))
}

fn prune_old_backups(backups_dir: &Path, keep_last_n: usize) -> Result<()> {
    if keep_last_n == 0 {
        return Ok(());
    }
    let mut items = Vec::new();
    for entry in fs::read_dir(backups_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.ends_with(".tar.gz.age") {
            if let Ok(info) = load_backup_info(&path, name) {
                items.push((info.created_at, info.filename, path));
            }
        }
    }
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    for (_, _, old_path) in items.into_iter().skip(keep_last_n) {
        let _ = fs::remove_file(&old_path);
        let _ = fs::remove_file(sha256_sidecar_path(&old_path));
        let _ = fs::remove_file(meta_sidecar_path(&old_path));
    }
    Ok(())
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}
