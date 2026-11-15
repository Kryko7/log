use std::fs;
use std::path::PathBuf;

use chrono::NaiveDate;
use journal_core::Vault;
use journal_sync::{
    create_backup, get_daily_context, list_backups, parse_calendar_json, restore_backup,
    sync_gdrive,
};
use journal_types::{BackupConfig, RestoreBackupRequest, SaveEntryRequest};
use uuid::Uuid;

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

    // Ensure no token/CLI env var interferes in this test.
    std::env::remove_var("LOG_GDRIVE_TOKEN");
    std::env::remove_var("LOG_GDRIVE_USE_CLI");

    let res = sync_gdrive(
        &vault_dir,
        Some("offline-sync-passphrase"),
        &BackupConfig::default(),
    )
    .await
    .expect("sync_gdrive should succeed locally when remote unconfigured");

    assert!(res.success);
    assert!(!res.uploaded_remote);
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

    std::env::set_var("LOG_OFFLINE", "1");
    std::env::set_var("LOG_CALENDAR_CACHE_PATH", &cache_path);

    let ctx = get_daily_context(&vault, date, None, None)
        .await
        .expect("get daily context");

    std::env::remove_var("LOG_OFFLINE");
    std::env::remove_var("LOG_CALENDAR_CACHE_PATH");

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
