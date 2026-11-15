use std::fs;
use std::path::PathBuf;

use chrono::NaiveDate;
use journal_core::Vault;
use journal_sync::{
    connect_gdrive_oauth, create_backup, disconnect_gdrive, get_daily_context, get_gdrive_status,
    list_backups, load_backup_config, parse_calendar_json, restore_backup, save_backup_config,
    save_gdrive_token, sync_gdrive, sync_gdrive_with_target,
};
use journal_types::{
    BackupConfig, ConnectGdriveOauthRequest, RestoreBackupRequest, SaveEntryRequest,
};
use uuid::Uuid;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct EnvVarGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvVarGuard {
    fn set(pairs: &[(&'static str, Option<&str>)]) -> Self {
        let mut saved = Vec::with_capacity(pairs.len());
        for (key, val) in pairs {
            saved.push((*key, std::env::var(key).ok()));
            match val {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        Self { saved }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        for (key, prev) in self.saved.drain(..).rev() {
            match prev {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

const TEST_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0,
    0, 0, 253, 212, 154, 115, 0, 0, 0, 19, 73, 68, 65, 84, 120, 156, 99, 8, 201, 139, 2, 34, 134,
    173, 97, 118, 64, 4, 0, 33, 59, 4, 203, 251, 163, 6, 31, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
    96, 130,
];

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("log-sync-test-{name}-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("create temp test dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn age_encrypt_decrypt_restore_round_trip() {
    let tmp = TempDir::new("age-roundtrip");
    let src_vault_dir = tmp.path.join("src-vault");
    let restore_dir = tmp.path.join("restored-vault");

    let vault = Vault::open(&src_vault_dir).expect("open source vault");
    let photo = vault
        .ingest_photo("veranda.png", TEST_PNG, Some("Morning veranda".into()))
        .expect("ingest photo");

    let secret_phrase = "Secret diary sentence about Cinnamon Gardens tea";
    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Tea & Monsoon Notes".into(),
            date: None,
            notebook_id: None,
            tags: vec!["rain".into(), "tea".into()],
            mood: Some("calm".into()),
            starred: true,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![photo.clone()],
            audio: vec![],
            body: format!("{secret_phrase}.\n\n![Veranda]({})", photo.url),
        })
        .expect("save entry");

    let cfg = BackupConfig {
        keep_last_n: 5,
        ..BackupConfig::default()
    };
    let passphrase = "sanctuary-correct-horse-battery";
    let backup = create_backup(&src_vault_dir, passphrase, &cfg).expect("create encrypted backup");

    assert!(backup.encrypted);
    assert!(backup.filename.ends_with(".tar.gz.age"));
    assert_eq!(backup.entry_count, 1);
    assert!(!backup.sha256.is_empty());

    let archive_path = src_vault_dir.join("backups").join(&backup.filename);
    assert!(archive_path.is_file());
    let sha_sidecar = PathBuf::from(format!("{}.sha256", archive_path.display()));
    assert!(sha_sidecar.is_file());

    // Ensure zero plaintext leakage inside the .age encrypted archive.
    let raw_bytes = fs::read(&archive_path).expect("read encrypted archive");
    let raw_lossy = String::from_utf8_lossy(&raw_bytes);
    assert!(!raw_lossy.contains("Tea & Monsoon Notes"));
    assert!(!raw_lossy.contains("Cinnamon Gardens"));

    // Wrong passphrase must fail decryption cleanly.
    let bad_restore = restore_backup(
        &src_vault_dir,
        &RestoreBackupRequest {
            backup_id_or_path: backup.id.clone(),
            passphrase: "wrong-passphrase-attempt".into(),
            target_vault_dir: Some(restore_dir.to_string_lossy().into_owned()),
            overwrite_existing: true,
        },
    );
    assert!(bad_restore.is_err());

    // Listed backups include our newly created archive.
    let listed = list_backups(&src_vault_dir).expect("list backups");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, backup.id);

    // Correct passphrase restores entries, notebooks.json, templates, and media.
    let restored = restore_backup(
        &src_vault_dir,
        &RestoreBackupRequest {
            backup_id_or_path: backup.id.clone(),
            passphrase: passphrase.into(),
            target_vault_dir: Some(restore_dir.to_string_lossy().into_owned()),
            overwrite_existing: true,
        },
    )
    .expect("restore backup");

    assert!(restored.restored);
    assert_eq!(restored.entries_restored, 1);
    assert!(restored.media_restored >= 1);
    assert!(restore_dir.join("notebooks.json").is_file());
    assert!(restore_dir
        .join("media/photos")
        .join(&photo.filename)
        .is_file());

    let reopened = Vault::open(&restore_dir).expect("reopen restored vault");
    let loaded = reopened
        .get_entry("Tea & Monsoon Notes")
        .expect("read restored entry");
    assert!(loaded.body.contains(secret_phrase));
}

#[tokio::test]
async fn gdrive_sync_offline_gracefully_reports_local_backup() {
    let tmp = TempDir::new("gdrive-offline");
    let vault_dir = tmp.path.join("vault");
    let vault = Vault::open(&vault_dir).expect("open vault");
    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Quiet Page".into(),
            date: None,
            notebook_id: None,
            tags: vec![],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            body: "Offline backup check.".into(),
        })
        .unwrap();

    let _lock = ENV_LOCK.lock().await;
    let config_dir = tmp.path.join("config");
    let config_str = config_dir.to_string_lossy().into_owned();
    let _env = EnvVarGuard::set(&[
        ("LOG_CONFIG_DIR", Some(&config_str)),
        ("LOG_GDRIVE_DISABLE_CLI", Some("1")),
        ("LOG_GDRIVE_TOKEN", None),
        ("LOG_GDRIVE_USE_CLI", None),
        ("PATH", Some("")),
    ]);

    let res = sync_gdrive(
        &vault_dir,
        Some("offline-sync-passphrase"),
        &BackupConfig::default(),
    )
    .await
    .expect("sync_gdrive should succeed locally when remote unconfigured");

    assert!(res.success);
    assert!(!res.uploaded_remote);
    assert!(!res.message.contains("scripts/sync-gdrive.sh"));
    assert!(res.backup.is_some());
    assert!(res.backup.unwrap().filename.ends_with(".tar.gz.age"));
}

#[tokio::test]
async fn daily_context_loads_calendar_cache_and_on_this_day_count() {
    let tmp = TempDir::new("daily-context");
    let vault_dir = tmp.path.join("vault");
    let cache_path = tmp.path.join("today-events.json");

    let cal_json = serde_json::json!([
        {
            "id": "evt-garden",
            "summary": "Peradeniya Botanical Walk",
            "location": "Kandy",
            "start": { "dateTime": "2026-11-15T15:30:00+05:30" },
            "end": { "dateTime": "2026-11-15T17:00:00+05:30" }
        }
    ]);
    fs::write(&cache_path, serde_json::to_string(&cal_json).unwrap()).unwrap();

    let vault = Vault::open(&vault_dir).expect("open vault");
    let past_dt = chrono::DateTime::parse_from_rfc3339("2025-11-15T09:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "One Year Ago Today".into(),
            date: Some(past_dt),
            notebook_id: None,
            tags: vec![],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            body: "Autumn tea by the window.".into(),
        })
        .unwrap();

    let date = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
    let parsed_direct = parse_calendar_json(&fs::read_to_string(&cache_path).unwrap(), date);
    assert_eq!(parsed_direct.len(), 1);
    assert!(parsed_direct[0]
        .suggested_prompt
        .contains("Peradeniya Botanical Walk"));
    assert!(parsed_direct[0].suggested_prompt.contains("this afternoon"));

    let _lock = ENV_LOCK.lock().await;
    let cache_str = cache_path.to_string_lossy().into_owned();
    let _env = EnvVarGuard::set(&[
        ("LOG_OFFLINE", Some("1")),
        ("LOG_CALENDAR_CACHE_PATH", Some(&cache_str)),
    ]);

    let ctx = get_daily_context(&vault, date, None, None)
        .await
        .expect("get daily context");

    assert_eq!(ctx.date, date);
    assert_eq!(ctx.on_this_day_count, 1);
    assert_eq!(ctx.events.len(), 1);
    assert_eq!(ctx.events[0].title, "Peradeniya Botanical Walk");
    assert!(ctx.suggested_prompts.len() >= 2);
}

#[test]
fn restore_backup_rejects_unexpected_archive_prefix() {
    let tmp = TempDir::new("reject-prefix");
    let archive_path = tmp.path.join("crafted.tar.gz.age");
    let restore_dir = tmp.path.join("restore-target");
    fs::create_dir_all(&restore_dir).unwrap();

    let passphrase = age::secrecy::SecretString::from("crafted-passphrase".to_string());
    let encryptor = age::Encryptor::with_user_passphrase(passphrase);
    let out_file = fs::File::create(&archive_path).unwrap();
    let age_writer = encryptor.wrap_output(out_file).unwrap();
    let gz = flate2::write::GzEncoder::new(age_writer, flate2::Compression::fast());
    let mut tar_builder = tar::Builder::new(gz);

    let payload = b"evil outside payload";
    let mut header = tar::Header::new_gnu();
    header.set_path(".index/evil.txt").unwrap();
    header.set_size(payload.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    tar_builder.append(&header, &payload[..]).unwrap();
    let gz = tar_builder.into_inner().unwrap();
    let age_writer = gz.finish().unwrap();
    age_writer.finish().unwrap();

    let err = restore_backup(
        &tmp.path,
        &RestoreBackupRequest {
            backup_id_or_path: archive_path.to_string_lossy().into_owned(),
            passphrase: "crafted-passphrase".into(),
            target_vault_dir: Some(restore_dir.to_string_lossy().into_owned()),
            overwrite_existing: true,
        },
    )
    .expect_err("restore_backup must reject unexpected archive path prefix");

    assert!(
        err.to_string().contains("unexpected archive path"),
        "unexpected error: {err}"
    );
    assert!(!restore_dir.join(".index/evil.txt").exists());
}

#[tokio::test]
async fn backup_config_persists_with_0600_and_defaults_keep_last_3() {
    let _lock = ENV_LOCK.lock().await;
    let tmp = TempDir::new("backup-config");
    let config_dir = tmp.path.join("cfg");
    let config_str = config_dir.to_string_lossy().into_owned();
    let _env = EnvVarGuard::set(&[
        ("LOG_CONFIG_DIR", Some(&config_str)),
        ("LOG_GDRIVE_DISABLE_CLI", Some("1")),
        ("LOG_GDRIVE_TOKEN", None),
        ("PATH", Some("")),
    ]);

    let default_cfg = load_backup_config();
    assert_eq!(default_cfg.keep_last_n, 3);
    assert_eq!(default_cfg.gdrive_folder, "LogJournal");

    let updated = BackupConfig {
        destination_dir: Some("~/DriveMirror".into()),
        gdrive_folder: "MyLogBackups".into(),
        auto_sync_enabled: true,
        keep_last_n: 5,
        has_passphrase_hint: Some("tea".into()),
        rclone_remote: Some("logjournal:".into()),
    };
    let saved = save_backup_config(&updated).expect("save backup config");
    assert_eq!(saved.rclone_remote.as_deref(), Some("logjournal"));
    assert_eq!(saved.keep_last_n, 5);

    let reloaded = load_backup_config();
    assert_eq!(reloaded, saved);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(config_dir.join("backup-config.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let status = get_gdrive_status(&reloaded).await;
    assert!(!status.connected);
    assert_eq!(status.method, "none");
    assert!(!status.has_rclone);
    assert!(!status.has_gdrive_cli);
}

#[tokio::test]
async fn sync_gdrive_with_target_reuses_existing_archive_and_mirrors_destination() {
    let _lock = ENV_LOCK.lock().await;
    let tmp = TempDir::new("sync-target");
    let vault_dir = tmp.path.join("vault");
    let config_dir = tmp.path.join("cfg");
    let mirror_dir = tmp.path.join("mirror");
    let config_str = config_dir.to_string_lossy().into_owned();
    let _env = EnvVarGuard::set(&[
        ("LOG_CONFIG_DIR", Some(&config_str)),
        ("LOG_GDRIVE_DISABLE_CLI", Some("1")),
        ("LOG_GDRIVE_TOKEN", None),
        ("PATH", Some("")),
    ]);

    let vault = Vault::open(&vault_dir).expect("open vault");
    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Existing Backup Target".into(),
            date: None,
            notebook_id: None,
            tags: vec![],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            body: "Verify no duplicate archive is created.".into(),
        })
        .unwrap();

    let created = create_backup(&vault_dir, "target-passphrase", &BackupConfig::default())
        .expect("create initial backup");

    let cfg = BackupConfig {
        destination_dir: Some(mirror_dir.to_string_lossy().into_owned()),
        ..BackupConfig::default()
    };
    let res = sync_gdrive_with_target(&vault_dir, None, Some(&created.id), &cfg)
        .await
        .expect("sync existing backup by id");

    assert!(res.success);
    assert!(!res.uploaded_remote);
    assert_eq!(res.backup.as_ref().map(|b| &b.id), Some(&created.id));
    assert_eq!(list_backups(&vault_dir).unwrap().len(), 1);
    assert!(mirror_dir.join(&created.filename).is_file());
    assert!(res.message.contains("Backup Settings"));
    assert!(!res.message.contains("scripts/sync-gdrive.sh"));
}

#[tokio::test]
async fn disconnect_gdrive_clears_tokens_and_preserves_client_id() {
    let _lock = ENV_LOCK.lock().await;
    let tmp = TempDir::new("disconnect-gdrive");
    let config_dir = tmp.path.join("cfg");
    fs::create_dir_all(&config_dir).unwrap();
    let config_str = config_dir.to_string_lossy().into_owned();
    let _env = EnvVarGuard::set(&[
        ("LOG_CONFIG_DIR", Some(&config_str)),
        ("LOG_GDRIVE_DISABLE_CLI", Some("1")),
        ("LOG_GDRIVE_TOKEN", None),
        ("PATH", Some("")),
    ]);

    fs::write(
        config_dir.join("gdrive-auth.json"),
        serde_json::json!({
            "client_id": "123456.apps.googleusercontent.com",
            "access_token": "ya29.fake",
            "refresh_token": "1//refresh-fake",
            "account_email": "writer@example.com"
        })
        .to_string(),
    )
    .unwrap();

    let before = get_gdrive_status(&BackupConfig::default()).await;
    assert!(before.connected);
    assert_eq!(before.method, "oauth");
    assert_eq!(before.account_hint.as_deref(), Some("writer@example.com"));
    assert_eq!(
        before.saved_client_id.as_deref(),
        Some("123456.apps.googleusercontent.com")
    );

    let after = disconnect_gdrive().await.expect("disconnect gdrive");
    assert!(!after.connected);
    assert_eq!(after.method, "none");
    assert_eq!(
        after.saved_client_id.as_deref(),
        Some("123456.apps.googleusercontent.com")
    );

    let empty_oauth = connect_gdrive_oauth(&ConnectGdriveOauthRequest {
        client_id: "   ".into(),
        client_secret: None,
    })
    .await;
    assert!(empty_oauth.is_err());

    let empty_tok = save_gdrive_token("   ").await;
    assert!(empty_tok.is_err());
}
