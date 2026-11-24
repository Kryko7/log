use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use chrono::{Datelike, TimeZone, Utc};
use image::{DynamicImage, GenericImageView, ImageFormat, Rgb, RgbImage};
use journal_core::{
    extract_wikilinks, parse_entry_file, render_html_preview, serialize_entry, Vault, VaultError,
};
use journal_types::{
    CreateNotebookRequest, EntryFrontmatter, GeoLocation, SaveEntryRequest, SearchQuery,
    StampPlacement, UpdateNotebookRequest, WeatherInfo,
};
use uuid::Uuid;

struct TempVault {
    path: PathBuf,
}

impl TempVault {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("log-core-test-{name}-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("create temp vault dir");
        Self { path }
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn default_path_respects_env_and_fallback() {
    let prev = std::env::var_os("LOG_VAULT_DIR");

    std::env::set_var("LOG_VAULT_DIR", "/tmp/custom-log-vault-test");
    assert_eq!(
        Vault::default_path(),
        PathBuf::from("/tmp/custom-log-vault-test")
    );

    std::env::remove_var("LOG_VAULT_DIR");
    let fallback = Vault::default_path();
    assert!(fallback.ends_with("Personal/JournalVault"));

    if let Some(val) = prev {
        std::env::set_var("LOG_VAULT_DIR", val);
    }
}

#[test]
fn vault_init_seeds_default_notebook_and_four_templates() {
    let tmp = TempVault::new("init");
    let vault = Vault::open(&tmp.path).expect("open vault");

    assert!(vault.root().join("entries").is_dir());
    assert!(vault.root().join("media/photos").is_dir());
    assert!(vault.root().join("media/audio").is_dir());
    assert!(vault.root().join("templates").is_dir());
    assert!(vault.root().join(".index/index.sqlite").is_file());
    assert!(vault.root().join(".index/thumbs").is_dir());
    assert!(vault.root().join("notebooks.json").is_file());

    let notebooks = vault.list_notebooks().expect("list notebooks");
    assert_eq!(notebooks.len(), 1);
    let daily = &notebooks[0];
    assert_eq!(daily.id, "daily");
    assert_eq!(daily.name, "Daily Sanctuary");
    assert_eq!(daily.cover_asset, "/assets/covers/rainy-tea-desk.png");
    assert_eq!(daily.accent_color, "#546E5A");
    assert!(daily.is_default);
    assert_eq!(daily.entry_count, 0);

    let templates = vault.list_templates().expect("list templates");
    assert_eq!(templates.len(), 4);
    let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Evening Reflection",
            "Morning Pages",
            "Field & Travel Notes",
            "Quiet Gratitude",
        ]
    );
    for tpl in &templates {
        assert!(!tpl.body_markdown.trim().is_empty());
        assert!(!tpl.prompts.is_empty());
    }
}

#[test]
fn markdown_yaml_round_trip_and_wikilinks() {
    let dt = Utc.with_ymd_and_hms(2026, 11, 15, 8, 15, 0).unwrap();
    let fm = EntryFrontmatter {
        id: Uuid::now_v7(),
        title: "Rain on the Veranda".into(),
        date: dt,
        updated_at: dt,
        notebook_id: Some("daily".into()),
        tags: vec!["rain".into(), "tea".into()],
        mood: Some("reflective".into()),
        starred: true,
        source: Some("log".into()),
        source_id: None,
        weather: Some(WeatherInfo {
            condition: "Light Rain".into(),
            temperature_c: Some(19.5),
            icon_key: "rain".into(),
            humidity_pct: Some(84),
            summary: Some("Warm tea and cool air".into()),
        }),
        location: Some(GeoLocation {
            place_name: "Colombo Study".into(),
            city: Some("Colombo".into()),
            country: Some("Sri Lanka".into()),
            latitude: Some(6.9271),
            longitude: Some(79.8612),
        }),
        stamps: vec![StampPlacement {
            id: "st-1".into(),
            asset_id: "ginkgo".into(),
            asset_path: "/assets/stamps/pressed-ginkgo.png".into(),
            x_pct: 80.0,
            y_pct: 15.0,
            rotation_deg: -5.0,
            scale: 1.0,
            label: Some("AUTUMN".into()),
        }],
        photos: vec![],
        audio: vec![],
        cover_asset: Some("/assets/covers/botanical-fern-wash.png".into()),
        links: vec!["Autumn Notes".into(), "Tea Ledger".into()],
    };

    let body = "First paragraph mentioning [[Autumn Notes|last autumn]] and [[Tea Ledger]].\n\n---\n\n`[[Code Ignored]]`\n\n```\n[[Fenced Ignored]]\n```";
    let serialized = serialize_entry(&fm, body).expect("serialize");
    let parsed = parse_entry_file(
        &serialized,
        "entries/daily/2026/11/2026-11-15-rain-on-the-veranda.md",
    )
    .expect("parse");

    assert_eq!(parsed.frontmatter, fm);
    assert_eq!(parsed.body, body);
    assert_eq!(
        extract_wikilinks(body),
        vec!["Autumn Notes".to_string(), "Tea Ledger".to_string()]
    );

    let html = render_html_preview(body);
    assert!(html.contains("<a class=\"wikilink\" data-target=\"Autumn Notes\""));
    assert!(html.contains(">last autumn</a>"));
    assert!(html.contains("<code>[[Code Ignored]]</code>"));
}

#[test]
fn crud_save_relocate_delete_and_notebooks() {
    let tmp = TempVault::new("crud");
    let vault = Vault::open(&tmp.path).expect("open vault");

    let field_nb = vault
        .create_notebook(CreateNotebookRequest {
            name: "Field Notes".into(),
            description: Some("Sketches from walks".into()),
            cover_asset: Some("/assets/covers/pressed-wildflowers.png".into()),
            accent_color: Some("#B5563E".into()),
        })
        .expect("create notebook");
    assert_eq!(field_nb.id, "field-notes");

    let d1 = Utc.with_ymd_and_hms(2026, 11, 10, 14, 0, 0).unwrap();
    let saved = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "First Walk".into(),
            date: Some(d1),
            notebook_id: Some("field-notes".into()),
            tags: vec!["botany".into(), "walk".into()],
            mood: Some("curious".into()),
            starred: true,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Noticed ferns near the creek.".into(),
        })
        .expect("save entry");

    assert_eq!(
        saved.path,
        "entries/field-notes/2026/11/2026-11-10-first-walk.md"
    );
    assert!(vault.root().join(&saved.path).is_file());

    let old_path = saved.path.clone();

    // Update title & move date -> old file should be removed, new file created.
    let d2 = Utc.with_ymd_and_hms(2026, 11, 12, 16, 30, 0).unwrap();
    let updated = vault
        .save_entry(SaveEntryRequest {
            id: Some(saved.id),
            title: "Creekside Ferns".into(),
            date: Some(d2),
            notebook_id: Some("field-notes".into()),
            tags: vec!["botany".into()],
            mood: Some("calm".into()),
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Updated prose with fresh observations.".into(),
        })
        .expect("update entry");

    assert_eq!(updated.id, saved.id);
    assert_eq!(
        updated.path,
        "entries/field-notes/2026/11/2026-11-12-creekside-ferns.md"
    );
    assert!(!vault.root().join(&old_path).exists());
    assert!(vault.root().join(&updated.path).is_file());

    // Lookup by UUID, slug, and title.
    assert_eq!(
        vault.get_entry(&updated.id.to_string()).unwrap().id,
        updated.id
    );
    assert_eq!(vault.get_entry("creekside-ferns").unwrap().id, updated.id);
    assert_eq!(vault.get_entry("Creekside Ferns").unwrap().id, updated.id);

    // Notebooks live count.
    let notebooks = vault.list_notebooks().unwrap();
    let nb_field = notebooks.iter().find(|n| n.id == "field-notes").unwrap();
    assert_eq!(nb_field.entry_count, 1);

    // List entries filters.
    assert_eq!(
        vault
            .list_entries(Some("field-notes"), Some("botany"), None, None)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        vault
            .list_entries(Some("daily"), None, None, None)
            .unwrap()
            .len(),
        0
    );

    // Delete entry.
    let del = vault.delete_entry(updated.id).unwrap();
    assert!(del.deleted);
    assert!(!vault.root().join(&updated.path).exists());
    let del_again = vault.delete_entry(updated.id).unwrap();
    assert!(!del_again.deleted);
}

#[test]
fn save_entry_with_frontmatter_and_source_dedup() {
    let tmp = TempVault::new("import-fm");
    let vault = Vault::open(&tmp.path).expect("open vault");

    let mut fm = EntryFrontmatter::new("Imported Day One Note");
    fm.source = Some("Day One".into());
    fm.source_id = Some("DO-ABC-123".into());
    fm.tags = vec!["imported".into()];

    let saved = vault
        .save_entry_with_frontmatter(fm.clone(), "Imported memory body.")
        .expect("save with frontmatter");

    assert_eq!(saved.frontmatter.source.as_deref(), Some("Day One"));
    assert_eq!(saved.frontmatter.source_id.as_deref(), Some("DO-ABC-123"));

    let found = vault
        .find_by_source_id("Day One", "DO-ABC-123")
        .expect("find by source_id")
        .expect("entry exists");
    assert_eq!(found.id, saved.id);
}

#[test]
fn fts5_search_filters_and_snippets() {
    let tmp = TempVault::new("search");
    let vault = Vault::open(&tmp.path).expect("open vault");

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Monsoon Afternoon".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 1, 15, 0, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec!["rain".into(), "tea".into()],
            mood: Some("reflective".into()),
            starred: true,
            weather: None,
            location: Some(GeoLocation {
                place_name: "Kandy Veranda".into(),
                city: None,
                country: None,
                latitude: None,
                longitude: None,
            }),
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Listening to heavy monsoon rainfall while steeping Ceylon black tea.".into(),
        })
        .unwrap();

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Dry Morning Walk".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 5, 7, 30, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec!["morning".into(), "tea".into()],
            mood: Some("calm".into()),
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Clear skies and cool pavement before sunrise.".into(),
        })
        .unwrap();

    let hits = vault
        .search_entries(&SearchQuery::text("monsoon"))
        .expect("search monsoon");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].summary.title, "Monsoon Afternoon");
    assert!(hits[0].highlighted_snippet.contains("<mark>"));
    assert!(hits[0].highlighted_snippet.contains("</mark>"));

    // Punctuation should not cause FTS5 syntax errors.
    let punct_hits = vault
        .search_entries(&SearchQuery::text("tea-skies? (calm)"))
        .expect("punct search");
    assert!(punct_hits.is_empty());

    // Structured filters without text.
    let starred_only = vault
        .search_entries(&SearchQuery {
            starred_only: true,
            tag: Some("tea".into()),
            mood: Some("reflective".into()),
            ..SearchQuery::default()
        })
        .expect("filtered search");
    assert_eq!(starred_only.len(), 1);
    assert_eq!(starred_only[0].summary.title, "Monsoon Afternoon");

    let tags = vault.list_tags().unwrap();
    assert_eq!(tags[0].tag, "tea");
    assert_eq!(tags[0].count, 2);
}

#[test]
fn backlinks_calendar_on_this_day_and_stats() {
    let tmp = TempVault::new("queries");
    let vault = Vault::open(&tmp.path).expect("open vault");

    let today = Utc::now();
    let yesterday = today - chrono::Duration::days(1);

    // Historical On This Day entries on Nov 15, 2024 and Nov 15, 2025.
    let anchor = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Lantern Workshop".into(),
            date: Some(Utc.with_ymd_and_hms(2025, 11, 15, 19, 0, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec!["craft".into()],
            mood: Some("warm".into()),
            starred: true,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Built paper lanterns for the courtyard.".into(),
        })
        .unwrap();

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "November Two Years Ago".into(),
            date: Some(Utc.with_ymd_and_hms(2024, 11, 15, 10, 0, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec!["memory".into()],
            mood: Some("nostalgic".into()),
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "First frost on the garden gate.".into(),
        })
        .unwrap();

    // Two linking entries (today and yesterday) to test backlinks & 2-day active streak.
    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Yesterday's Notes".into(),
            date: Some(yesterday),
            notebook_id: Some("daily".into()),
            tags: vec!["craft".into()],
            mood: Some("calm".into()),
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: format!("Remembered [[{}]] while sorting twine.", anchor.id),
        })
        .unwrap();

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Today's Continuation".into(),
            date: Some(today),
            notebook_id: Some("daily".into()),
            tags: vec!["craft".into()],
            mood: Some("grateful".into()),
            starred: true,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Revisited [[Lantern Workshop|last year's lantern build]].".into(),
        })
        .unwrap();

    let backlinks = vault.get_backlinks(anchor.id).unwrap();
    assert_eq!(backlinks.len(), 2);

    let loaded_anchor = vault.get_entry(&anchor.id.to_string()).unwrap();
    assert_eq!(loaded_anchor.backlinks.len(), 2);

    let otd = vault.get_on_this_day(11, 15, 2026).unwrap();
    assert_eq!(otd.len(), 2);
    assert_eq!(otd[0].years_ago, 1);
    assert_eq!(otd[0].year, 2025);
    assert_eq!(otd[1].years_ago, 2);
    assert_eq!(otd[1].year, 2024);

    let cal = vault
        .get_calendar_month(today.year(), today.month(), None)
        .unwrap();
    assert!(!cal.is_empty());
    assert!(cal
        .iter()
        .any(|d| d.date == today.date_naive() && d.starred_count >= 1));

    let stats = vault.get_vault_stats().unwrap();
    assert_eq!(stats.total_entries, 4);
    assert!(stats.total_words >= 20);
    assert!(stats.current_streak_days >= 2);
    assert!(stats.longest_streak_days >= 2);
    assert_eq!(stats.top_tags[0].tag, "craft");
    assert_eq!(stats.top_tags[0].count, 3);
}

#[test]
fn rebuild_index_recovers_from_deleted_sqlite() {
    let tmp = TempVault::new("rebuild");
    {
        let vault = Vault::open(&tmp.path).unwrap();
        vault
            .save_entry(SaveEntryRequest {
                id: None,
                title: "Persistent Ink".into(),
                date: None,
                notebook_id: None,
                tags: vec!["resilience".into()],
                mood: None,
                starred: false,
                weather: None,
                location: None,
                stamps: vec![],
                photos: vec![],
                audio: vec![],
                cover_asset: None,
                body: "Markdown on disk outlives every database cache.".into(),
            })
            .unwrap();
    }

    // Delete the SQLite index and verify automatic rebuild on open + explicit rebuild_index.
    fs::remove_file(tmp.path.join(".index/index.sqlite")).unwrap();
    let vault = Vault::open(&tmp.path).unwrap();

    let hits = vault
        .search_entries(&SearchQuery::text("outlives"))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].summary.title, "Persistent Ink");

    let count = vault.rebuild_index().unwrap();
    assert_eq!(count, 1);
}

#[test]
fn ingest_photo_thumbnail_and_exif() {
    let tmp = TempVault::new("photo");
    let vault = Vault::open(&tmp.path).unwrap();

    // 1. Plain 1200x800 PNG -> 520px proportional thumbnail.
    let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(1200, 800, Rgb([140, 160, 130])));
    let mut png_bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .unwrap();

    let meta = vault
        .ingest_photo("Garden Walk.png", &png_bytes, Some("Fern corner".into()))
        .expect("ingest png");
    assert_eq!(meta.width, Some(1200));
    assert_eq!(meta.height, Some(800));
    assert!(meta.url.starts_with("/vault-media/photos/"));
    let thumb_url = meta.thumbnail_url.as_deref().expect("thumb url");
    assert!(thumb_url.starts_with("/vault-thumbs/"));

    let saved_file = vault.root().join("media/photos").join(&meta.filename);
    assert!(saved_file.is_file());

    let thumb_name = thumb_url.strip_prefix("/vault-thumbs/").unwrap();
    let thumb_file = vault.root().join(".index/thumbs").join(thumb_name);
    assert!(thumb_file.is_file());

    let decoded_thumb = image::open(&thumb_file).expect("open generated webp thumb");
    let (tw, th) = decoded_thumb.dimensions();
    assert!(tw <= 520 && th <= 520);
    assert_eq!(tw, 520);

    // 2. Synthetic JPEG with EXIF DateTimeOriginal + GPS coordinates.
    let jpg_img = DynamicImage::ImageRgb8(RgbImage::from_pixel(320, 240, Rgb([210, 190, 170])));
    let mut raw_jpg = Vec::new();
    jpg_img
        .write_to(&mut Cursor::new(&mut raw_jpg), ImageFormat::Jpeg)
        .unwrap();
    let jpeg_with_exif = inject_minimal_exif_into_jpeg(&raw_jpg);

    let exif_meta = vault
        .ingest_photo("colombo.jpg", &jpeg_with_exif, None)
        .expect("ingest jpeg with exif");
    assert_eq!(
        exif_meta.taken_at,
        Some(Utc.with_ymd_and_hms(2025, 8, 14, 17, 45, 30).unwrap())
    );
    let loc = exif_meta.location.expect("extracted GPS location");
    let lat = loc.latitude.unwrap();
    let lon = loc.longitude.unwrap();
    assert!((lat - 6.9271).abs() < 0.001, "lat={lat}");
    assert!((lon - 79.8612).abs() < 0.001, "lon={lon}");
}

#[test]
fn ingest_audio_wav_and_compressed_fallback() {
    let tmp = TempVault::new("audio");
    let vault = Vault::open(&tmp.path).unwrap();

    // Synthesize a 1.0s 16kHz mono WAV with rising & falling envelope.
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav_buf = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut wav_buf, spec).unwrap();
        for i in 0..16_000 {
            let t = i as f32 / 16_000.0;
            let env = (std::f32::consts::PI * t).sin();
            let sample = (env * (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 28000.0) as i16;
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }
    let wav_bytes = wav_buf.into_inner();

    let wav_meta = vault
        .ingest_audio("Rain Porch.wav", &wav_bytes, None, None)
        .expect("ingest wav");
    assert_eq!(wav_meta.peaks.len(), 64);
    assert!(wav_meta.peaks.iter().all(|&p| (0.08..=1.0).contains(&p)));
    assert!((wav_meta.duration_secs - 1.0).abs() < 0.05);
    assert_eq!(wav_meta.mime_type, "audio/wav");
    assert!(wav_meta.url.starts_with("/vault-media/audio/"));
    assert!(vault
        .root()
        .join("media/audio")
        .join(&wav_meta.filename)
        .is_file());

    // Compressed MediaRecorder fallback (deterministic 64 peaks).
    let fake_webm: Vec<u8> = (0..4096).map(|i| ((i * 37) % 251) as u8).collect();
    let webm_meta = vault
        .ingest_audio("voice.webm", &fake_webm, Some(9.25), None)
        .expect("ingest webm");
    assert_eq!(webm_meta.peaks.len(), 64);
    assert!(webm_meta.peaks.iter().all(|&p| (0.08..=1.0).contains(&p)));
    assert_eq!(webm_meta.duration_secs, 9.25);
    assert_eq!(webm_meta.mime_type, "audio/webm");

    let webm_meta_2 = vault
        .ingest_audio("voice.webm", &fake_webm, Some(9.25), None)
        .expect("ingest webm again");
    assert_eq!(webm_meta.peaks, webm_meta_2.peaks);
}

#[test]
fn watch_vault_detects_entry_change() {
    let tmp = TempVault::new("watch");
    let vault = Vault::open(&tmp.path).unwrap();

    let (tx, rx) = mpsc::channel();
    let _watcher = vault
        .watch_vault(move |event| {
            let _ = tx.send(event);
        })
        .expect("start watcher");

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Watched Note".into(),
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
            cover_asset: None,
            body: "Testing inotify watcher.".into(),
        })
        .unwrap();

    let received = rx.recv_timeout(Duration::from_secs(3));
    assert!(received.is_ok(), "expected notify event on entry write");
}

#[test]
fn list_entries_handles_more_than_500_entries() {
    let tmp = TempVault::new("over-500");
    let vault = Vault::open(&tmp.path).unwrap();

    // Seed 510 entries directly via save_entry_with_frontmatter + fast disk writes + rebuild_index.
    let entries_dir = vault.root().join("entries/daily/2026/11");
    fs::create_dir_all(&entries_dir).unwrap();
    for i in 0..510 {
        let mut fm = EntryFrontmatter::new(format!("Bulk Note {i:03}"));
        fm.notebook_id = Some("daily".into());
        let raw = serialize_entry(&fm, &format!("Body line for entry {i}.")).unwrap();
        let file = entries_dir.join(format!("2026-11-15-bulk-note-{i:03}.md"));
        fs::write(file, raw).unwrap();
    }

    let rebuilt = vault.rebuild_index().unwrap();
    assert_eq!(rebuilt, 510);

    let all = vault.list_entries(None, None, None, None).unwrap();
    assert_eq!(all.len(), 510);

    let capped = vault.list_entries(None, None, Some(25), Some(10)).unwrap();
    assert_eq!(capped.len(), 25);
}

#[test]
fn incremental_open_syncs_external_add_modify_delete() {
    let tmp = TempVault::new("incremental-sync");
    let (id_a, path_a, path_b);
    {
        let vault = Vault::open(&tmp.path).unwrap();
        let a = vault
            .save_entry(SaveEntryRequest {
                id: None,
                title: "Entry Alpha".into(),
                date: Some(Utc.with_ymd_and_hms(2026, 11, 10, 9, 0, 0).unwrap()),
                notebook_id: Some("daily".into()),
                tags: vec![],
                mood: None,
                starred: false,
                weather: None,
                location: None,
                stamps: vec![],
                photos: vec![],
                audio: vec![],
                cover_asset: None,
                body: "Original alpha text.".into(),
            })
            .unwrap();
        let b = vault
            .save_entry(SaveEntryRequest {
                id: None,
                title: "Entry Beta".into(),
                date: Some(Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap()),
                notebook_id: Some("daily".into()),
                tags: vec![],
                mood: None,
                starred: false,
                weather: None,
                location: None,
                stamps: vec![],
                photos: vec![],
                audio: vec![],
                cover_asset: None,
                body: "Beta text to delete.".into(),
            })
            .unwrap();
        id_a = a.id;
        path_a = vault.root().join(&a.path);
        path_b = vault.root().join(&b.path);
    }

    // Mutate A, delete B, and add C externally while vault is closed.
    let mut fm_a = parse_entry_file(&fs::read_to_string(&path_a).unwrap(), "a.md")
        .unwrap()
        .frontmatter;
    fm_a.updated_at = Utc::now();
    let new_a = serialize_entry(&fm_a, "Externally revised cardamom tea reflection.").unwrap();
    fs::write(&path_a, new_a).unwrap();

    fs::remove_file(&path_b).unwrap();

    let fm_c = EntryFrontmatter::new("External Gamma");
    let path_c = tmp
        .path
        .join("entries/daily/2026/11/2026-11-12-external-gamma.md");
    fs::create_dir_all(path_c.parent().unwrap()).unwrap();
    fs::write(
        &path_c,
        serialize_entry(&fm_c, "Freshly synced from external tool.").unwrap(),
    )
    .unwrap();

    let reopened = Vault::open(&tmp.path).unwrap();
    let entries = reopened.list_entries(None, None, None, None).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|e| e.id == id_a));
    assert!(entries.iter().any(|e| e.title == "External Gamma"));

    let hits = reopened
        .search_entries(&SearchQuery::text("cardamom"))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].summary.id, id_a);

    // Calling incremental sync again immediately should do 0 work.
    assert_eq!(reopened.sync_index_incremental().unwrap(), 0);
}

#[test]
fn fts5_escapes_html_in_snippets_and_handles_hyphenated_dates() {
    let tmp = TempVault::new("fts-xss-date");
    let vault = Vault::open(&tmp.path).unwrap();

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Solstice Ledger".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 15, 10, 0, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec!["Botany".into()],
            mood: Some("quiet".into()),
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Logged <script>alert(1)</script> on 2026-11-15 during monsoon rain.".into(),
        })
        .unwrap();

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Second Tag Case".into(),
            date: None,
            notebook_id: Some("daily".into()),
            tags: vec!["botany".into()],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Plain moss notes.".into(),
        })
        .unwrap();

    // Hyphenated date search must not parse `-11` as FTS5 NOT 11.
    let date_hits = vault
        .search_entries(&SearchQuery::text("2026-11-15"))
        .unwrap();
    assert_eq!(date_hits.len(), 1);
    assert!(!date_hits[0].highlighted_snippet.contains("<script>"));
    assert!(date_hits[0].highlighted_snippet.contains("&lt;script&gt;"));
    assert!(date_hits[0].highlighted_snippet.contains("<mark>"));

    // Title/tag match not present in body still highlights via fallback column.
    let tag_hits = vault.search_entries(&SearchQuery::text("Botany")).unwrap();
    assert_eq!(tag_hits.len(), 2);
    assert!(tag_hits.iter().all(|h| h
        .highlighted_snippet
        .to_ascii_lowercase()
        .contains("<mark>botany</mark>")));

    // Case-insensitive tag grouping combines "Botany" and "botany".
    let tags = vault.list_tags().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].count, 2);
}

#[test]
fn render_html_preview_sanitizes_raw_html_and_unsafe_urls() {
    let md = "\
Hello <script>alert(1)</script> and <img src=x onerror=alert(1)>

[Evil](javascript:alert(1))
![BadImg](data:text/html,<script>alert(1)</script>)
[Good](https://example.com/notes)
[[Target Note|Label <b>Bold</b>]]";

    let html = render_html_preview(md);
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<img src=x"));
    assert!(!html.contains("javascript:"));
    assert!(!html.contains("data:text/html"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(html.contains("href=\"#blocked\""));
    assert!(html.contains("src=\"#blocked\""));
    assert!(html.contains("href=\"https://example.com/notes\""));
    assert!(html.contains("Label &lt;b&gt;Bold&lt;/b&gt;"));
}

#[test]
fn editing_entry_removes_deleted_wikilinks_from_backlinks() {
    let tmp = TempVault::new("backlink-sync");
    let vault = Vault::open(&tmp.path).unwrap();

    let target = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Target Hearth".into(),
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
            cover_asset: None,
            body: "Target body.".into(),
        })
        .unwrap();

    let mut fm = EntryFrontmatter::new("Linker Note");
    let saved = vault
        .save_entry_with_frontmatter(fm.clone(), "Linking to [[Target Hearth]].")
        .unwrap();
    assert_eq!(vault.get_backlinks(target.id).unwrap().len(), 1);

    fm.id = saved.id;
    vault
        .save_entry_with_frontmatter(fm, "Removed the link now.")
        .unwrap();
    assert_eq!(vault.get_backlinks(target.id).unwrap().len(), 0);
}

#[test]
fn update_notebook_updates_name_cover_accent_and_preserves_entry_count() {
    let tmp = TempVault::new("update-notebook");
    let vault = Vault::open(&tmp.path).unwrap();

    let created = vault
        .create_notebook(CreateNotebookRequest {
            name: "Field Notes".into(),
            description: Some("Initial description".into()),
            cover_asset: None,
            accent_color: Some("#546E5A".into()),
        })
        .unwrap();
    assert_eq!(created.id, "field-notes");

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Mossy Stone".into(),
            date: None,
            notebook_id: Some(created.id.clone()),
            tags: vec![],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Observing lichen.".into(),
        })
        .unwrap();

    let updated = vault
        .update_notebook(UpdateNotebookRequest {
            id: created.id.clone(),
            name: "Botanical Field Notes".into(),
            description: Some("Pressed specimens and rain walks".into()),
            cover_asset: Some("/vault-thumbs/custom-cover.webp".into()),
            accent_color: Some("#B5563E".into()),
        })
        .unwrap();
    assert_eq!(updated.name, "Botanical Field Notes");
    assert_eq!(
        updated.description.as_deref(),
        Some("Pressed specimens and rain walks")
    );
    assert_eq!(updated.cover_asset, "/vault-thumbs/custom-cover.webp");
    assert_eq!(updated.accent_color, "#B5563E");
    assert_eq!(updated.entry_count, 1);

    let listed = vault
        .list_notebooks()
        .unwrap()
        .into_iter()
        .find(|n| n.id == created.id)
        .unwrap();
    assert_eq!(listed.name, "Botanical Field Notes");
    assert_eq!(listed.cover_asset, "/vault-thumbs/custom-cover.webp");
    assert_eq!(listed.accent_color, "#B5563E");
    assert_eq!(listed.entry_count, 1);

    let cleared = vault
        .update_notebook(UpdateNotebookRequest {
            id: created.id.clone(),
            name: "Botanical Field Notes".into(),
            description: Some("   ".into()),
            cover_asset: Some("".into()),
            accent_color: None,
        })
        .unwrap();
    assert_eq!(cleared.description, None);
    assert_eq!(cleared.cover_asset, "");
    assert_eq!(cleared.accent_color, "#B5563E");

    let missing_err = vault
        .update_notebook(UpdateNotebookRequest {
            id: "does-not-exist".into(),
            name: "Missing".into(),
            description: None,
            cover_asset: None,
            accent_color: None,
        })
        .unwrap_err();
    assert!(matches!(missing_err, VaultError::NotFound(_)));

    let invalid_err = vault
        .update_notebook(UpdateNotebookRequest {
            id: created.id,
            name: "   ".into(),
            description: None,
            cover_asset: None,
            accent_color: None,
        })
        .unwrap_err();
    assert!(matches!(invalid_err, VaultError::InvalidInput(_)));
}

#[test]
fn delete_notebook_reassign_and_delete_entries() {
    let tmp = TempVault::new("delete-notebook");
    let vault = Vault::open(&tmp.path).unwrap();

    let keep_nb = vault
        .create_notebook(CreateNotebookRequest {
            name: "Keep Entries".into(),
            description: None,
            cover_asset: None,
            accent_color: None,
        })
        .unwrap();
    let drop_nb = vault
        .create_notebook(CreateNotebookRequest {
            name: "Drop Entries".into(),
            description: None,
            cover_asset: None,
            accent_color: None,
        })
        .unwrap();

    let kept_entry = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Reassigned Note".into(),
            date: None,
            notebook_id: Some(keep_nb.id.clone()),
            tags: vec!["migration".into()],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Should move to daily.".into(),
        })
        .unwrap();
    let old_kept_abs = tmp.path.join(&kept_entry.path);
    assert!(old_kept_abs.exists());

    let dropped_entry = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Deleted Note".into(),
            date: None,
            notebook_id: Some(drop_nb.id.clone()),
            tags: vec![],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Should be deleted with notebook.".into(),
        })
        .unwrap();
    let dropped_abs = tmp.path.join(&dropped_entry.path);
    assert!(dropped_abs.exists());

    let reassign_res = vault.delete_notebook(&keep_nb.id, false).unwrap();
    assert!(reassign_res.deleted);
    assert_eq!(reassign_res.id, keep_nb.id);
    assert_eq!(reassign_res.deleted_entries, 0);
    assert_eq!(reassign_res.reassigned_to.as_deref(), Some("daily"));
    assert!(!old_kept_abs.exists());

    let moved_entry = vault.get_entry(&kept_entry.id.to_string()).unwrap();
    assert_eq!(
        moved_entry.frontmatter.notebook_id.as_deref(),
        Some("daily")
    );
    assert!(moved_entry.path.starts_with("entries/daily/"));
    assert!(tmp.path.join(&moved_entry.path).exists());

    let drop_res = vault.delete_notebook(&drop_nb.id, true).unwrap();
    assert!(drop_res.deleted);
    assert_eq!(drop_res.id, drop_nb.id);
    assert_eq!(drop_res.deleted_entries, 1);
    assert_eq!(drop_res.reassigned_to, None);
    assert!(!dropped_abs.exists());
    assert!(matches!(
        vault.get_entry(&dropped_entry.id.to_string()),
        Err(VaultError::NotFound(_))
    ));

    let missing_res = vault.delete_notebook("nonexistent", false).unwrap();
    assert!(!missing_res.deleted);
    assert_eq!(missing_res.deleted_entries, 0);
    assert_eq!(missing_res.reassigned_to, None);

    let remaining_ids: Vec<String> = vault
        .list_notebooks()
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(remaining_ids, vec!["daily".to_string()]);
}

#[test]
fn entry_cover_asset_persists_and_clears_across_reloads() {
    let tmp = TempVault::new("cover-asset");
    let vault = Vault::open(&tmp.path).unwrap();

    let saved = vault
        .save_entry(SaveEntryRequest {
            title: "Pressed Leaves".into(),
            cover_asset: Some("/assets/covers/pressed-wildflowers.png".into()),
            body: "A quiet afternoon with pressed meadow flowers.".into(),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(
        saved.frontmatter.cover_asset.as_deref(),
        Some("/assets/covers/pressed-wildflowers.png")
    );

    let reloaded = vault.get_entry(&saved.id.to_string()).unwrap();
    assert_eq!(
        reloaded.frontmatter.cover_asset.as_deref(),
        Some("/assets/covers/pressed-wildflowers.png")
    );

    let cleared = vault
        .save_entry(SaveEntryRequest {
            id: Some(saved.id),
            title: "Pressed Leaves".into(),
            cover_asset: Some("   ".into()),
            body: "Cover removed cleanly.".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.frontmatter.cover_asset, None);
    let reloaded2 = vault.get_entry(&saved.id.to_string()).unwrap();
    assert_eq!(reloaded2.frontmatter.cover_asset, None);
}

#[test]
fn custom_templates_can_be_created_loaded_and_deleted() {
    let tmp = TempVault::new("custom-templates");
    let vault = Vault::open(&tmp.path).unwrap();

    let initial = vault.list_templates().unwrap();
    assert_eq!(initial.len(), 4);

    let created = vault
        .save_template(journal_types::SaveTemplateRequest {
            id: None,
            name: "Weekly Review".into(),
            description: Some("Sunday evening lookback".into()),
            category: Some("Review".into()),
            default_tags: vec!["Weekly".into(), "Review".into(), "weekly".into()],
            default_mood: Some("reflective".into()),
            body_markdown: "## Highlights\n\n- \n\n## Next Week\n\n- [ ] \n".into(),
            prompts: vec!["What stood out most this week?".into()],
        })
        .unwrap();

    assert_eq!(created.id, "weekly-review");
    assert_eq!(created.default_tags, vec!["weekly", "review"]);

    let after_create = vault.list_templates().unwrap();
    assert_eq!(after_create.len(), 5);
    assert!(after_create.iter().any(|t| t.id == "weekly-review"));

    assert!(vault.delete_template("weekly-review").unwrap());
    assert!(!vault.delete_template("weekly-review").unwrap());
    assert_eq!(vault.list_templates().unwrap().len(), 4);
}

#[test]
fn stamps_weather_location_and_mood_round_trip_across_rebuild() {
    let tmp = TempVault::new("stamps-weather-roundtrip");
    let vault = Vault::open(&tmp.path).unwrap();

    let saved = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Rainy Veranda Afternoon".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 20, 14, 15, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec!["rain".into(), "tea".into()],
            mood: Some("calm".into()),
            starred: true,
            weather: Some(WeatherInfo {
                condition: "Light Rain".into(),
                temperature_c: Some(22.5),
                icon_key: "rain".into(),
                humidity_pct: Some(78),
                summary: Some("Light Rain · 23°C".into()),
            }),
            location: Some(GeoLocation {
                place_name: "Kandy".into(),
                city: Some("Kandy".into()),
                country: Some("Sri Lanka".into()),
                latitude: Some(7.2906),
                longitude: Some(80.6337),
            }),
            stamps: vec![StampPlacement {
                id: "st-1".into(),
                asset_id: "pressed-lavender-sprig".into(),
                asset_path: "/assets/stamps/pressed-lavender-sprig.png".into(),
                x_pct: 89.0,
                y_pct: 18.0,
                rotation_deg: -4.5,
                scale: 1.1,
                label: Some("Pressed Lavender Sprig".into()),
            }],
            photos: vec![],
            audio: vec![],
            cover_asset: Some("/assets/covers/anim-rainy-window.svg".into()),
            body: "Listening to rain on the garden glass.".into(),
        })
        .expect("save richly annotated entry");

    assert_eq!(vault.rebuild_index().unwrap(), 1);
    let loaded = vault.get_entry(&saved.id.to_string()).unwrap();
    assert_eq!(loaded.frontmatter.stamps.len(), 1);
    assert_eq!(
        loaded.frontmatter.stamps[0].asset_id,
        "pressed-lavender-sprig"
    );
    assert_eq!(
        loaded.frontmatter.cover_asset.as_deref(),
        Some("/assets/covers/anim-rainy-window.svg")
    );
    assert_eq!(
        loaded.frontmatter.weather.as_ref().unwrap().condition,
        "Light Rain"
    );
    assert_eq!(
        loaded.frontmatter.location.as_ref().unwrap().place_name,
        "Kandy"
    );
    assert_eq!(loaded.frontmatter.mood.as_deref(), Some("calm"));
}

#[test]
fn search_entries_supports_filter_only_queries_without_text() {
    let tmp = TempVault::new("filter-only-search");
    let vault = Vault::open(&tmp.path).unwrap();

    vault
        .create_notebook(CreateNotebookRequest {
            name: "Travel".into(),
            description: None,
            cover_asset: None,
            accent_color: None,
        })
        .unwrap();

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Starred Calm Note".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 10, 9, 0, 0).unwrap()),
            notebook_id: Some("travel".into()),
            tags: vec!["coast".into()],
            mood: Some("calm".into()),
            starred: true,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Sea breeze at dusk.".into(),
        })
        .unwrap();

    vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Plain Daily Note".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec!["work".into()],
            mood: Some("energized".into()),
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Shipping tests.".into(),
        })
        .unwrap();

    let starred_hits = vault
        .search_entries(&SearchQuery {
            text: None,
            notebook_id: None,
            tag: None,
            mood: None,
            starred_only: true,
            from_date: None,
            to_date: None,
            limit: None,
            offset: None,
        })
        .unwrap();
    assert_eq!(starred_hits.len(), 1);
    assert_eq!(starred_hits[0].summary.title, "Starred Calm Note");

    let mood_and_nb_hits = vault
        .search_entries(&SearchQuery {
            text: Some("   ".into()),
            notebook_id: Some("travel".into()),
            tag: Some("coast".into()),
            mood: Some("calm".into()),
            starred_only: false,
            from_date: None,
            to_date: None,
            limit: None,
            offset: None,
        })
        .unwrap();
    assert_eq!(mood_and_nb_hits.len(), 1);
    assert_eq!(mood_and_nb_hits[0].summary.title, "Starred Calm Note");
}

#[test]
fn sync_index_incremental_reconciles_external_delete_and_edit() {
    let tmp = TempVault::new("sync-incremental-live");
    let vault = Vault::open(&tmp.path).unwrap();

    let a = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Keep Me".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 18, 9, 0, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec![],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Before external edit.".into(),
        })
        .unwrap();
    let b = vault
        .save_entry(SaveEntryRequest {
            id: None,
            title: "Remove Externally".into(),
            date: Some(Utc.with_ymd_and_hms(2026, 11, 19, 9, 0, 0).unwrap()),
            notebook_id: Some("daily".into()),
            tags: vec![],
            mood: None,
            starred: false,
            weather: None,
            location: None,
            stamps: vec![],
            photos: vec![],
            audio: vec![],
            cover_asset: None,
            body: "Will disappear from disk.".into(),
        })
        .unwrap();

    // Edit A and remove B directly on disk while Vault stays open.
    let path_a = vault.root().join(&a.path);
    let mut parsed = parse_entry_file(&fs::read_to_string(&path_a).unwrap(), &a.path).unwrap();
    parsed.frontmatter.updated_at = Utc::now();
    let updated_raw =
        serialize_entry(&parsed.frontmatter, "After external obsidian edit.").unwrap();
    fs::write(&path_a, updated_raw).unwrap();
    fs::remove_file(vault.root().join(&b.path)).unwrap();

    let changed = vault.sync_index_incremental().unwrap();
    assert_eq!(changed, 2); // 1 externally modified + 1 externally deleted
    assert_eq!(vault.sync_index_incremental().unwrap(), 0);
    assert_eq!(vault.list_entries(None, None, None, None).unwrap().len(), 1);

    let hits = vault
        .search_entries(&SearchQuery {
            text: Some("obsidian".into()),
            notebook_id: None,
            tag: None,
            mood: None,
            starred_only: false,
            from_date: None,
            to_date: None,
            limit: None,
            offset: None,
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].summary.id, a.id);
    assert!(vault.get_entry(&b.id.to_string()).is_err());
}

/// Build a valid JPEG APP1 EXIF segment (little-endian TIFF) with DateTimeOriginal & GPS.
fn inject_minimal_exif_into_jpeg(jpeg: &[u8]) -> Vec<u8> {
    assert!(jpeg.starts_with(&[0xFF, 0xD8]));

    // Build TIFF body (little-endian "II*\0", offset 8).
    let mut tiff: Vec<u8> = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());

    // IFD0 at offset 8: 2 entries -> ExifIFDPointer (0x8769) & GPSInfoIFDPointer (0x8825)
    // Size of IFD0 = 2 + 2*12 + 4 = 30 bytes -> ends at offset 38.
    let exif_ifd_offset: u32 = 38;
    // ExifIFD has 1 entry (DateTimeOriginal 0x9003) -> 2 + 12 + 4 = 18 bytes -> ends at 56.
    let dt_str_offset: u32 = 56; // "2025:08:14 17:45:30\0" (20 bytes) -> ends at 76.
    let gps_ifd_offset: u32 = 76;
    // GPS IFD has 4 entries (LatRef, Lat, LonRef, Lon) -> 2 + 4*12 + 4 = 54 bytes -> ends at 130.
    let lat_rat_offset: u32 = 130; // 3 rationals * 8 = 24 bytes -> ends at 154.
    let lon_rat_offset: u32 = 154; // 3 rationals * 8 = 24 bytes -> ends at 178.

    let push_ifd_entry = |buf: &mut Vec<u8>, tag: u16, typ: u16, count: u32, val: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
    };

    // IFD0
    tiff.extend_from_slice(&2_u16.to_le_bytes());
    push_ifd_entry(&mut tiff, 0x8769, 4, 1, exif_ifd_offset);
    push_ifd_entry(&mut tiff, 0x8825, 4, 1, gps_ifd_offset);
    tiff.extend_from_slice(&0_u32.to_le_bytes());

    // ExifIFD at 38
    assert_eq!(tiff.len(), exif_ifd_offset as usize);
    tiff.extend_from_slice(&1_u16.to_le_bytes());
    push_ifd_entry(&mut tiff, 0x9003, 2, 20, dt_str_offset);
    tiff.extend_from_slice(&0_u32.to_le_bytes());

    // DateTimeOriginal ASCII at 56
    assert_eq!(tiff.len(), dt_str_offset as usize);
    tiff.extend_from_slice(b"2025:08:14 17:45:30\0");

    // GPS IFD at 76
    assert_eq!(tiff.len(), gps_ifd_offset as usize);
    tiff.extend_from_slice(&4_u16.to_le_bytes());
    push_ifd_entry(&mut tiff, 0x0001, 2, 2, u32::from_le_bytes(*b"N\0\0\0")); // GPSLatitudeRef
    push_ifd_entry(&mut tiff, 0x0002, 5, 3, lat_rat_offset); // GPSLatitude
    push_ifd_entry(&mut tiff, 0x0003, 2, 2, u32::from_le_bytes(*b"E\0\0\0")); // GPSLongitudeRef
    push_ifd_entry(&mut tiff, 0x0004, 5, 3, lon_rat_offset); // GPSLongitude
    tiff.extend_from_slice(&0_u32.to_le_bytes());

    // 6.9271 deg = 6 deg, 55 min, 37.56 sec
    assert_eq!(tiff.len(), lat_rat_offset as usize);
    for (num, den) in [(6_u32, 1_u32), (55, 1), (3756, 100)] {
        tiff.extend_from_slice(&num.to_le_bytes());
        tiff.extend_from_slice(&den.to_le_bytes());
    }
    // 79.8612 deg = 79 deg, 51 min, 40.32 sec
    assert_eq!(tiff.len(), lon_rat_offset as usize);
    for (num, den) in [(79_u32, 1_u32), (51, 1), (4032, 100)] {
        tiff.extend_from_slice(&num.to_le_bytes());
        tiff.extend_from_slice(&den.to_le_bytes());
    }

    // Wrap in APP1 marker: FF E1 <len> "Exif\0\0" <tiff>
    let app1_payload_len = 2 + 6 + tiff.len();
    let mut out = Vec::with_capacity(jpeg.len() + app1_payload_len + 2);
    out.extend_from_slice(&jpeg[..2]); // FF D8
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&(app1_payload_len as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    out.extend_from_slice(&jpeg[2..]);
    out
}
