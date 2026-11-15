//! Integration tests for `journal-server` demo seeder, HTTP API commands,
//! multipart uploads, and encrypted backup/restore endpoints.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Datelike, Utc};
use journal_core::Vault;
use journal_server::api::build_router;
use journal_server::media_gen::{generate_acoustic_chord_wav, generate_veranda_polaroid_png};
use journal_server::seed::seed_demo_vault;
use journal_types::{
    AssetManifest, BackupInfo, CalendarDaySummary, DailyContext, DeleteEntryResponse, Entry,
    EntrySummary, MigrationReport, Notebook, OnThisDayGroup, PolaroidMeta,
    RestoreBackupResponse, SaveEntryRequest, SearchHit, TagCount, Template, VaultStats,
    VoiceMemoMeta,
};
use reqwest::multipart::{Form, Part};
use serde_json::json;
use tokio::net::TcpListener;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

async fn spawn_test_server(vault: Arc<Vault>) -> String {
    let app = build_router(vault);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test router");
    });
    format!("http://{addr}")
}

#[test]
fn seed_demo_creates_rich_idempotent_showcase_vault() {
    let dir = TempDirGuard::new("log-server-seed");
    let vault = Vault::open(&dir.path).expect("open vault");

    let first = seed_demo_vault(&vault).expect("first seed");
    assert_eq!(first.notebooks_created, 1);
    assert_eq!(first.entries_seeded, 4);

    // Idempotent when called again on the same vault.
    let second = seed_demo_vault(&vault).expect("second seed");
    assert_eq!(second.notebooks_created, 0);
    assert_eq!(second.entries_seeded, 0);

    let notebooks = vault.list_notebooks().expect("list notebooks");
    assert_eq!(notebooks.len(), 2);
    assert!(notebooks.iter().any(|n| n.name == "Daily Sanctuary"));
    assert!(notebooks.iter().any(|n| n.name == "Field & Botanical Notes"));

    let entries = vault
        .list_entries(None, None, Some(20), None)
        .expect("list entries");
    assert_eq!(entries.len(), 4);

    let today_entry = vault
        .get_entry("Rain on the Teak Sill & Unhurried Ink")
        .expect("get entry 1");
    assert!(today_entry.frontmatter.starred);
    assert_eq!(today_entry.frontmatter.photos.len(), 1);
    assert_eq!(today_entry.frontmatter.audio.len(), 1);
    assert_eq!(today_entry.frontmatter.audio[0].peaks.len(), 64);
    assert!(
        !today_entry.backlinks.is_empty(),
        "Entry 2 must link back to Entry 1"
    );

    let linked_entry = vault
        .get_entry("First Rain of November")
        .expect("get entry 2");
    assert!(
        !linked_entry.backlinks.is_empty(),
        "Entry 1 must link to Entry 2"
    );

    let today = Utc::now().date_naive();
    let otd = vault
        .get_on_this_day(today.month(), today.day(), today.year())
        .expect("on this day");
    assert_eq!(otd.len(), 1);
    assert_eq!(otd[0].years_ago, 1);
    assert_eq!(otd[0].entries[0].title, "Autumn Tea in Kandy — One Year Ago");
}

#[tokio::test]
async fn http_invoke_upload_and_static_routes_work_end_to_end() {
    std::env::set_var("LOG_OFFLINE", "1");
    let dir = TempDirGuard::new("log-server-http");
    let restore_dir = TempDirGuard::new("log-server-restore");

    let vault = Arc::new(Vault::open(&dir.path).expect("open vault"));
    seed_demo_vault(&vault).expect("seed demo");

    let base = spawn_test_server(Arc::clone(&vault)).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // 1. Core read invoke commands (including empty `{}` and zero-byte bodies).
    let stats_resp = client
        .post(format!("{base}/api/invoke/get_vault_stats"))
        .send()
        .await
        .unwrap();
    let status = stats_resp.status();
    let text = stats_resp.text().await.unwrap();
    assert_eq!(status, 200, "get_vault_stats failed: {text}");
    let stats: VaultStats = serde_json::from_str(&text).unwrap();
    assert_eq!(stats.total_entries, 4);
    assert_eq!(stats.total_notebooks, 2);

    let notebooks: Vec<Notebook> = client
        .post(format!("{base}/api/invoke/list_notebooks"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(notebooks.len(), 2);

    let entries: Vec<EntrySummary> = client
        .post(format!("{base}/api/invoke/list_entries"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(entries.len(), 4);

    let tags: Vec<TagCount> = client
        .post(format!("{base}/api/invoke/list_tags"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!tags.is_empty());

    let templates: Vec<Template> = client
        .post(format!("{base}/api/invoke/list_templates"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!templates.is_empty());

    let manifest: AssetManifest = client
        .post(format!("{base}/api/invoke/get_asset_manifest"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!manifest.covers.is_empty());
    assert!(!manifest.stamps.is_empty());

    let otd: Vec<OnThisDayGroup> = client
        .post(format!("{base}/api/invoke/get_on_this_day"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(otd.len(), 1);

    let today = Utc::now().date_naive();
    let cal: Vec<CalendarDaySummary> = client
        .post(format!("{base}/api/invoke/get_calendar_month"))
        .json(&json!({ "year": today.year(), "month": today.month() }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!cal.is_empty());

    let ctx: DailyContext = client
        .post(format!("{base}/api/invoke/get_daily_context"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ctx.on_this_day_count, 1);

    // 2. Static asset, media, and thumbnail routes.
    let manifest_resp = client
        .get(format!("{base}/assets/manifest.json"))
        .send()
        .await
        .unwrap();
    assert_eq!(manifest_resp.status(), 200);

    let full_entry: Entry = client
        .post(format!("{base}/api/invoke/get_entry"))
        .json(&json!({ "id": entries[0].id.to_string() }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    if let Some(photo) = full_entry.frontmatter.photos.first() {
        let media_resp = client.get(format!("{base}{}", photo.url)).send().await.unwrap();
        assert_eq!(media_resp.status(), 200);
        if let Some(thumb) = &photo.thumbnail_url {
            let thumb_resp = client.get(format!("{base}{thumb}")).send().await.unwrap();
            assert_eq!(thumb_resp.status(), 200);
        }
    }

    // 3. Backlinks & FTS5 Search.
    let hits: Vec<SearchHit> = client
        .post(format!("{base}/api/invoke/search_entries"))
        .json(&json!({ "text": "cardamom" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].highlighted_snippet.contains("<mark>"));

    // 4. Save & Delete Entry round-trip.
    let created: Entry = client
        .post(format!("{base}/api/invoke/save_entry"))
        .json(&SaveEntryRequest {
            id: None,
            title: "Ephemeral Test Folio".to_string(),
            date: None,
            notebook_id: Some("daily".to_string()),
            tags: vec!["qa".to_string()],
            mood: Some("calm".to_string()),
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            body: "Temporary test body.".to_string(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let deleted: DeleteEntryResponse = client
        .post(format!("{base}/api/invoke/delete_entry"))
        .json(&json!({ "id": created.id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(deleted.deleted);

    // 5. Multipart uploads: photo, audio, and dayone zip.
    let png_bytes = generate_veranda_polaroid_png().unwrap();
    let photo_form = Form::new().part(
        "file",
        Part::bytes(png_bytes)
            .file_name("upload.png")
            .mime_str("image/png")
            .unwrap(),
    );
    let uploaded_photo: PolaroidMeta = client
        .post(format!("{base}/api/upload/photo"))
        .multipart(photo_form)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(uploaded_photo.url.starts_with("/vault-media/photos/"));

    let wav_bytes = generate_acoustic_chord_wav().unwrap();
    let audio_form = Form::new().part(
        "file",
        Part::bytes(wav_bytes)
            .file_name("chord.wav")
            .mime_str("audio/wav")
            .unwrap(),
    );
    let uploaded_audio: VoiceMemoMeta = client
        .post(format!("{base}/api/upload/audio"))
        .multipart(audio_form)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(uploaded_audio.peaks.len(), 64);

    // Build minimal Day One zip fixture in memory and upload via /api/upload/dayone.
    let mut zip_buf = Vec::new();
    {
        let mut zw = ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        zw.start_file("Journal.json", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(
            br#"{
                "entries": [{
                    "uuid": "D1-UPLOAD-001",
                    "creationDate": "2026-10-12T08:00:00Z",
                    "text": "Morning Train to Galle\n\nSea breeze through the open window.",
                    "tags": ["travel"]
                }]
            }"#,
        )
        .unwrap();
        zw.finish().unwrap();
    }
    let dayone_form = Form::new().part(
        "file",
        Part::bytes(zip_buf)
            .file_name("dayone.zip")
            .mime_str("application/zip")
            .unwrap(),
    );
    let import_report: MigrationReport = client
        .post(format!("{base}/api/upload/dayone"))
        .multipart(dayone_form)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(import_report.imported_count, 1);

    let notion_json = br#"[{
        "Entry": "Rainy Sketchbook Study",
        "Date": "2026-10-14",
        "Source": "Notion",
        "Starred": false,
        "body": "Sketched the courtyard roof in charcoal."
    }]"#;
    let notion_form = Form::new().part(
        "file",
        Part::bytes(notion_json.to_vec())
            .file_name("notion.json")
            .mime_str("application/json")
            .unwrap(),
    );
    let notion_report: MigrationReport = client
        .post(format!("{base}/api/upload/notion"))
        .multipart(notion_form)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(notion_report.imported_count, 1);

    // 6. Encrypted backup & restore round-trip.
    let backup: BackupInfo = client
        .post(format!("{base}/api/invoke/create_backup"))
        .json(&json!({ "passphrase": "smoke-test-passphrase", "upload_gdrive": false }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(backup.encrypted);

    let backups: Vec<BackupInfo> = client
        .post(format!("{base}/api/invoke/list_backups"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);

    let restored: RestoreBackupResponse = client
        .post(format!("{base}/api/invoke/restore_backup"))
        .json(&json!({
            "backup_id_or_path": backup.id,
            "passphrase": "smoke-test-passphrase",
            "target_vault_dir": restore_dir.path.to_string_lossy(),
            "overwrite_existing": true
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(restored.restored);
    assert!(restored.entries_restored >= 5);

    // 7. Unknown command returns 404 JSON { "error": ... }.
    let bad_cmd = client
        .post(format!("{base}/api/invoke/does_not_exist"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_cmd.status(), 404);
}
