use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use journal_core::Vault;
use journal_import::{import_dayone, import_notion};
use journal_types::{ImportDayOneRequest, ImportNotionRequest, MigrationStatus};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// Valid 2x2 RGB PNG bytes (Botanical Sage & Terracotta pixels).
const TEST_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0,
    0, 0, 253, 212, 154, 115, 0, 0, 0, 19, 73, 68, 65, 84, 120, 156, 99, 8, 201, 139, 2, 34, 134,
    173, 97, 118, 64, 4, 0, 33, 59, 4, 203, 251, 163, 6, 31, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
    96, 130,
];

/// Minimal valid 1-channel 8kHz PCM WAV (8 samples).
const TEST_WAV: &[u8] = &[
    b'R', b'I', b'F', b'F', 52, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ', 16, 0, 0,
    0, 1, 0, 1, 0, 0x40, 0x1F, 0, 0, 0x80, 0x3E, 0, 0, 2, 0, 16, 0, b'd', b'a', b't', b'a', 16, 0,
    0, 0, 0, 0, 0x20, 0x10, 0x40, 0x20, 0x20, 0x10, 0, 0, 0xE0, 0xEF, 0xC0, 0xDF, 0xE0, 0xEF,
];

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("log-import-test-{name}-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("create temp test dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_dayone_zip_fixture(zip_path: &PathBuf) {
    let journal_json = serde_json::json!({
        "metadata": { "version": "1.0" },
        "entries": [
            {
                "uuid": "D1-ENTRY-001",
                "creationDate": "2025-11-15T08:45:00Z",
                "modifiedDate": "2025-11-15T09:10:00Z",
                "text": "# Rain on the Veranda\n\nTea cooled beside the open notebook while the monsoon settled in.\n\n![](dayone-moment://PHOTO-ID-001)\n\n![](dayone-moment://audio/AUDIO-ID-001)",
                "tags": ["monsoon", "tea"],
                "starred": true,
                "weather": {
                    "conditionsDescription": "Light Rain",
                    "temperatureCelsius": 24.5,
                    "relativeHumidity": 0.84,
                    "weatherCode": "rain"
                },
                "location": {
                    "placeName": "Colombo Study",
                    "localityName": "Colombo",
                    "country": "Sri Lanka",
                    "latitude": 6.9271,
                    "longitude": 79.8612
                },
                "photos": [
                    {
                        "identifier": "PHOTO-ID-001",
                        "md5": "abc123photo",
                        "type": "png",
                        "width": 2,
                        "height": 2
                    }
                ],
                "audios": [
                    {
                        "identifier": "AUDIO-ID-001",
                        "md5": "aud999wav",
                        "format": "wav",
                        "duration": 1.2
                    }
                ]
            },
            {
                "uuid": "D1-ENTRY-002",
                "creationDate": "2025-12-03T19:30:00Z",
                "richText": "{\"contents\":[{\"text\":\"Evening Walk Along Galle Face\\n\\nSalt wind and lanterns along the promenade.\"}]}",
                "tags": ["evening", "walk"],
                "starred": false
            }
        ]
    });

    let file = File::create(zip_path).expect("create dayone zip");
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default();

    zip.start_file("Journal.json", opts).unwrap();
    zip.write_all(serde_json::to_string_pretty(&journal_json).unwrap().as_bytes())
        .unwrap();

    zip.start_file("photos/abc123photo.png", opts).unwrap();
    zip.write_all(TEST_PNG).unwrap();

    zip.start_file("audios/aud999wav.wav", opts).unwrap();
    zip.write_all(TEST_WAV).unwrap();

    zip.finish().unwrap();
}

#[test]
fn dayone_zip_import_rewrites_moments_and_is_idempotent() {
    let tmp = TempDir::new("dayone-zip");
    let vault_dir = tmp.path.join("vault");
    let zip_path = tmp.path.join("DayOneExport.zip");
    create_dayone_zip_fixture(&zip_path);

    let vault = Vault::open(&vault_dir).expect("open vault");
    let req = ImportDayOneRequest {
        zip_path: zip_path.to_string_lossy().into_owned(),
        notebook_id: None,
        dry_run: false,
    };

    let report = import_dayone(&vault, &req).expect("import dayone zip");
    assert_eq!(report.source_kind, "Day One");
    assert_eq!(report.total_found, 2);
    assert_eq!(report.imported_count, 2);
    assert_eq!(report.skipped_count, 0);
    assert_eq!(report.failed_count, 0);
    assert_eq!(report.media_copied, 2);

    let entry_1 = vault.get_entry("Rain on the Veranda").expect("load entry 1");
    assert_eq!(entry_1.frontmatter.source.as_deref(), Some("Day One"));
    assert_eq!(entry_1.frontmatter.source_id.as_deref(), Some("D1-ENTRY-001"));
    assert!(entry_1.frontmatter.starred);
    assert_eq!(entry_1.frontmatter.photos.len(), 1);
    assert_eq!(entry_1.frontmatter.audio.len(), 1);
    assert_eq!(
        entry_1.frontmatter.weather.as_ref().map(|w| w.condition.as_str()),
        Some("Light Rain")
    );
    assert_eq!(
        entry_1.frontmatter.location.as_ref().map(|l| l.place_name.as_str()),
        Some("Colombo Study")
    );
    assert!(
        !entry_1.body.contains("dayone-moment://"),
        "expected dayone-moment links rewritten, got: {}",
        entry_1.body
    );
    assert!(
        entry_1.body.contains("/vault-media/photos/"),
        "expected vault photo link, got: {}",
        entry_1.body
    );
    assert!(
        entry_1.body.contains("/vault-media/audio/"),
        "expected vault audio link, got: {}",
        entry_1.body
    );

    let entry_2 = vault
        .get_entry("Evening Walk Along Galle Face")
        .expect("load entry 2 from richText");
    assert_eq!(entry_2.frontmatter.source_id.as_deref(), Some("D1-ENTRY-002"));
    assert!(entry_2.body.contains("Salt wind and lanterns"));

    // Second import of same Day One ZIP must skip both entries idempotently.
    let second = import_dayone(&vault, &req).expect("re-import dayone zip");
    assert_eq!(second.total_found, 2);
    assert_eq!(second.imported_count, 0);
    assert_eq!(second.skipped_count, 2);
    assert!(second
        .items
        .iter()
        .all(|item| item.status == MigrationStatus::Skipped));
}

#[test]
fn overlapping_dayone_and_notion_delta_skips_dayone_duplicates_and_imports_notion_only() {
    let tmp = TempDir::new("dayone-notion-delta");
    let vault_dir = tmp.path.join("vault");
    let dayone_zip = tmp.path.join("DayOne.zip");
    let notion_json = tmp.path.join("notion-snapshot.json");
    create_dayone_zip_fixture(&dayone_zip);

    let vault = Vault::open(&vault_dir).expect("open vault");
    let do_report = import_dayone(
        &vault,
        &ImportDayOneRequest {
            zip_path: dayone_zip.to_string_lossy().into_owned(),
            notebook_id: None,
            dry_run: false,
        },
    )
    .expect("import dayone");
    assert_eq!(do_report.imported_count, 2);

    // Notion snapshot matching the user's `📓 Journal` schema:
    // 1) Same date + Source == "Day One" -> MUST skip ("Duplicate of primary Day One entry")
    // 2) Same date + Source == "Notion" + matching title/lead paragraph -> MUST skip
    // 3) Unique Notion-only entry -> MUST import!
    let snapshot = serde_json::json!([
        {
            "id": "notion-page-dup-1",
            "Entry": "Veranda Tea Notes (Mirrored)",
            "date:Date:start": "2025-11-15",
            "Day": "Saturday",
            "Source": "Day One",
            "Starred": true,
            "Photos": 1,
            "Words": 24,
            "body": "Synced copy of Day One entry."
        },
        {
            "id": "notion-page-dup-2",
            "Entry": "Evening Walk Along Galle Face",
            "Date": "2025-12-03",
            "Day": "Wednesday",
            "Source": "Notion",
            "Starred": false,
            "Photos": 0,
            "Words": 8,
            "body": "Salt wind and lanterns along the promenade."
        },
        {
            "id": "notion-page-unique-3",
            "Entry": "Rust Ownership & Afternoon Espresso",
            "date:Date:start": "2026-01-10T15:20:00Z",
            "Day": "Saturday",
            "Source": "Notion",
            "Starred": true,
            "Photos": 0,
            "Words": 18,
            "Tags": ["rust", "notes"],
            "body": "Refactored the parser into clean zero-copy slices before dusk."
        }
    ]);
    fs::write(&notion_json, serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();

    let notion_report = import_notion(
        &vault,
        &ImportNotionRequest {
            source_path: notion_json.to_string_lossy().into_owned(),
            notebook_id: None,
            dry_run: false,
        },
    )
    .expect("import notion delta");

    assert_eq!(notion_report.source_kind, "Notion");
    assert_eq!(notion_report.total_found, 3);
    assert_eq!(notion_report.skipped_count, 2);
    assert_eq!(notion_report.imported_count, 1);
    assert_eq!(notion_report.failed_count, 0);

    assert_eq!(notion_report.items[0].status, MigrationStatus::Skipped);
    assert_eq!(
        notion_report.items[0].reason.as_deref(),
        Some("Duplicate of primary Day One entry")
    );
    assert_eq!(notion_report.items[1].status, MigrationStatus::Skipped);
    assert_eq!(
        notion_report.items[1].reason.as_deref(),
        Some("Duplicate of primary Day One entry")
    );
    assert_eq!(notion_report.items[2].status, MigrationStatus::Imported);

    let imported = vault
        .get_entry("Rust Ownership & Afternoon Espresso")
        .expect("find imported notion-only entry");
    assert_eq!(imported.frontmatter.source.as_deref(), Some("Notion"));
    assert_eq!(
        imported.frontmatter.source_id.as_deref(),
        Some("notion-page-unique-3")
    );
    assert!(imported.frontmatter.starred);
}

#[test]
fn notion_export_folder_markdown_and_csv_delta_imports_with_embedded_photo() {
    let tmp = TempDir::new("notion-dir");
    let vault_dir = tmp.path.join("vault");
    let export_dir = tmp.path.join("notion-export");
    fs::create_dir_all(export_dir.join("Garden Sketchbook")).unwrap();

    fs::write(export_dir.join("Garden Sketchbook/leaf.png"), TEST_PNG).unwrap();
    fs::write(
        export_dir.join("Journal.csv"),
        "Entry,Date,Day,Source,Starred,Photos,Words\n\"Garden Sketchbook\",\"2026-02-14\",\"Saturday\",\"Notion\",\"Yes\",1,12\n",
    )
    .unwrap();
    fs::write(
        export_dir.join("Garden Sketchbook 0123456789abcdef0123456789abcdef.md"),
        "# Garden Sketchbook\n\nDate: February 14, 2026\nSource: Notion\n\nSketched pressed leaves under the mango tree.\n\n![Leaf sketch](Garden%20Sketchbook/leaf.png)\n",
    )
    .unwrap();

    let vault = Vault::open(&vault_dir).expect("open vault");
    let report = import_notion(
        &vault,
        &ImportNotionRequest {
            source_path: export_dir.to_string_lossy().into_owned(),
            notebook_id: None,
            dry_run: false,
        },
    )
    .expect("import notion directory");

    assert_eq!(report.imported_count, 1);
    assert_eq!(report.media_copied, 1);

    let entry = vault
        .get_entry("Garden Sketchbook")
        .expect("get imported markdown+csv notion entry");
    assert!(entry.frontmatter.starred);
    assert_eq!(entry.frontmatter.photos.len(), 1);
    assert!(entry.body.contains("/vault-media/photos/"));
}
