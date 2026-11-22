//! Day One `.zip` and `.json` primary journal importer.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use journal_core::Vault;
use journal_types::{
    EntryFrontmatter, GeoLocation, ImportDayOneRequest, MigrationAuditItem, MigrationReport,
    MigrationStatus, PolaroidMeta, VoiceMemoMeta, WeatherInfo,
};
use regex::Regex;
use serde::Deserialize;
use uuid::Uuid;
use zip::ZipArchive;

use crate::dedup::{derive_title_from_body, DeduplicationIndex};
use crate::error::{ImportError, Result};

const MAX_ZIP_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ZIP_MEDIA_BYTES: u64 = 128 * 1024 * 1024;

fn unescape_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\\([.!()\[\]-])").expect("valid unescape regex"))
}

fn moment_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"!\[([^\]]*)\]\(dayone-moment://(?:(audio|photo)/)?([^)\s]+)\)")
            .expect("valid moment regex")
    })
}

fn read_bounded_bytes<R: Read>(reader: R, max_bytes: u64) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    if reader.take(max_bytes + 1).read_to_end(&mut buf).is_err() {
        return None;
    }
    if buf.len() as u64 > max_bytes {
        return None;
    }
    Some(buf)
}

fn is_safe_relative(path: &Path) -> bool {
    !path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DayOneRoot {
    Wrapped {
        #[serde(default)]
        entries: Vec<DayOneEntry>,
    },
    Array(Vec<DayOneEntry>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOneEntry {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    creation_date: Option<String>,
    #[serde(default)]
    modified_date: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    rich_text: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    starred: bool,
    #[serde(default)]
    weather: Option<DayOneWeather>,
    #[serde(default)]
    location: Option<DayOneLocation>,
    #[serde(default)]
    photos: Vec<DayOnePhoto>,
    #[serde(default)]
    audios: Vec<DayOneAudio>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOneWeather {
    #[serde(default)]
    conditions_description: Option<String>,
    #[serde(default)]
    temperature_celsius: Option<f32>,
    #[serde(default)]
    relative_humidity: Option<f32>,
    #[serde(default)]
    weather_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOneLocation {
    #[serde(default)]
    place_name: Option<String>,
    #[serde(default)]
    locality_name: Option<String>,
    #[serde(default)]
    administrative_area: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOnePhoto {
    #[serde(default)]
    identifier: Option<String>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default, rename = "type")]
    media_type: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    location: Option<DayOneLocation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOneAudio {
    #[serde(default)]
    identifier: Option<String>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    duration: Option<f32>,
}

/// Abstracted media store reading either from a `.zip` archive or sibling directories on disk.
enum MediaSource {
    Zip {
        archive: ZipArchive<BufReader<File>>,
        /// Map from lowercase archive path / stem / filename to zip entry index.
        lookup: HashMap<String, usize>,
    },
    Dir {
        canon_root: Option<PathBuf>,
        root: PathBuf,
        lookup: HashMap<String, PathBuf>,
    },
}

impl MediaSource {
    fn from_dir(root: &Path) -> Self {
        let canon_root = fs::canonicalize(root).ok();
        let mut lookup = HashMap::new();
        for sub in ["photos", "audios", "Photo", "Audio"] {
            let dir = root.join(sub);
            if dir.is_dir() {
                if let Ok(read_dir) = fs::read_dir(&dir) {
                    for item in read_dir.flatten() {
                        let Ok(ft) = item.file_type() else {
                            continue;
                        };
                        if ft.is_symlink() || !ft.is_file() {
                            continue;
                        }
                        let p = item.path();
                        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                            lookup.insert(name.to_ascii_lowercase(), p.clone());
                        }
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            lookup.insert(stem.to_ascii_lowercase(), p.clone());
                        }
                    }
                }
            }
        }
        Self::Dir {
            canon_root,
            root: root.to_path_buf(),
            lookup,
        }
    }

    fn read_media(&mut self, keys: &[String], ext_hint: Option<&str>) -> Option<(String, Vec<u8>)> {
        let mut candidates = Vec::new();
        for k in keys {
            let trimmed = k.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            candidates.push(lower.clone());
            if let Some(ext) = ext_hint {
                let ext_lower = ext.trim_start_matches('.').to_ascii_lowercase();
                candidates.push(format!("{lower}.{ext_lower}"));
            }
        }

        match self {
            MediaSource::Zip { archive, lookup } => {
                for cand in &candidates {
                    if let Some(&idx) = lookup.get(cand) {
                        if let Ok(entry) = archive.by_index(idx) {
                            let entry_name =
                                entry.name().map(|c| c.into_owned()).unwrap_or_default();
                            let filename = Path::new(&entry_name)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("media.bin")
                                .to_string();
                            if let Some(buf) = read_bounded_bytes(entry, MAX_ZIP_MEDIA_BYTES) {
                                if !buf.is_empty() {
                                    return Some((filename, buf));
                                }
                            }
                        }
                    }
                }
                None
            }
            MediaSource::Dir {
                canon_root,
                root,
                lookup,
            } => {
                for cand in &candidates {
                    if !is_safe_relative(Path::new(cand)) {
                        continue;
                    }
                    if let Some(path) = lookup.get(cand) {
                        if let Ok(buf) = fs::read(path) {
                            let filename = path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("media.bin")
                                .to_string();
                            if !buf.is_empty() {
                                return Some((filename, buf));
                            }
                        }
                    }
                    for sub in ["photos", "audios"] {
                        let direct = root.join(sub).join(cand);
                        if let (Some(cr), Ok(canon_direct)) =
                            (canon_root.as_ref(), fs::canonicalize(&direct))
                        {
                            if !canon_direct.starts_with(cr) || !canon_direct.is_file() {
                                continue;
                            }
                            if let Ok(buf) = fs::read(&canon_direct) {
                                let filename = canon_direct
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("media.bin")
                                    .to_string();
                                if !buf.is_empty() {
                                    return Some((filename, buf));
                                }
                            }
                        }
                    }
                }
                None
            }
        }
    }
}

/// Import a Day One `.zip` archive (containing `Journal.json` or `*.json` + `photos/` + `audios/`)
/// or raw `.json` export into `vault`.
///
/// Deduplicates idempotently by `source_id` (`uuid`) and `(YYYY-MM-DD, normalized_body_hash)`,
/// extracts photos & audio into the vault, and rewrites `dayone-moment://` references.
pub fn import_dayone(vault: &Vault, req: &ImportDayOneRequest) -> Result<MigrationReport> {
    let started_at = Utc::now();
    let source_path = Path::new(req.zip_path.trim());
    if !source_path.exists() {
        return Err(ImportError::InvalidSource(format!(
            "Day One export path does not exist: {}",
            source_path.display()
        )));
    }

    let (raw_entries, mut media_source) = load_dayone_source(source_path)?;
    let mut dedup = DeduplicationIndex::from_vault(vault)?;

    let mut report = MigrationReport {
        source_kind: "Day One".to_string(),
        started_at,
        completed_at: started_at,
        total_found: raw_entries.len(),
        imported_count: 0,
        skipped_count: 0,
        failed_count: 0,
        media_copied: 0,
        items: Vec::with_capacity(raw_entries.len()),
    };

    for (idx, raw) in raw_entries.into_iter().enumerate() {
        let source_id = raw
            .uuid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("dayone-row-{idx}"));

        match process_dayone_entry(vault, req, &mut dedup, &mut media_source, &source_id, raw) {
            Ok((item, media_delta)) => {
                match item.status {
                    MigrationStatus::Imported => report.imported_count += 1,
                    MigrationStatus::Skipped => report.skipped_count += 1,
                    MigrationStatus::Failed => report.failed_count += 1,
                    MigrationStatus::NeedsReview => report.imported_count += 1,
                }
                report.media_copied += media_delta;
                report.items.push(item);
            }
            Err(err) => {
                report.failed_count += 1;
                report.items.push(MigrationAuditItem {
                    source_id,
                    title: None,
                    date: None,
                    status: MigrationStatus::Failed,
                    reason: Some(err.to_string()),
                    target_entry_id: None,
                    target_path: None,
                });
            }
        }
    }

    report.completed_at = Utc::now();
    Ok(report)
}

fn load_dayone_source(path: &Path) -> Result<(Vec<DayOneEntry>, MediaSource)> {
    if path.is_dir() {
        let mut entries = Vec::new();
        for json_file in collect_json_files_in_dir(path)? {
            let raw = fs::read_to_string(&json_file)?;
            if let Ok(mut parsed) = parse_dayone_json_str(&raw) {
                entries.append(&mut parsed);
            }
        }
        if entries.is_empty() {
            return Err(ImportError::InvalidSource(format!(
                "No Day One JSON entries found in directory {}",
                path.display()
            )));
        }
        return Ok((entries, MediaSource::from_dir(path)));
    }

    let is_zip = path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));

    if is_zip {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(BufReader::new(file))?;

        let mut json_indices = Vec::new();
        let mut lookup = HashMap::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            if file.is_dir() {
                continue;
            }
            let name = file.name()?.into_owned();
            let lower = name.to_ascii_lowercase();

            if lower.ends_with(".json") && !lower.contains("__macosx/") {
                let is_primary = Path::new(&lower)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|f| f == "journal.json");
                json_indices.push((is_primary, i));
            } else {
                lookup.insert(lower.clone(), i);
                if let Some(fname) = Path::new(&lower).file_name().and_then(|s| s.to_str()) {
                    lookup.insert(fname.to_string(), i);
                }
                if let Some(stem) = Path::new(&lower).file_stem().and_then(|s| s.to_str()) {
                    lookup.insert(stem.to_string(), i);
                }
            }
        }

        // Parse Journal.json first, then any other *.json files.
        json_indices.sort_by_key(|(is_primary, idx)| (!*is_primary, *idx));

        let mut all_entries = Vec::new();
        for (_, idx) in json_indices {
            let zf = archive.by_index(idx)?;
            if let Some(bytes) = read_bounded_bytes(zf, MAX_ZIP_JSON_BYTES) {
                if let Ok(raw) = String::from_utf8(bytes) {
                    if let Ok(mut parsed) = parse_dayone_json_str(&raw) {
                        all_entries.append(&mut parsed);
                    }
                }
            }
        }

        if all_entries.is_empty() {
            return Err(ImportError::InvalidSource(format!(
                "No valid Day One JSON entries found inside {}",
                path.display()
            )));
        }

        return Ok((all_entries, MediaSource::Zip { archive, lookup }));
    }

    // Raw .json file
    let raw = fs::read_to_string(path)?;
    let entries = parse_dayone_json_str(&raw)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    Ok((entries, MediaSource::from_dir(parent)))
}

fn collect_json_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let Ok(ft) = item.file_type() else {
            continue;
        };
        if ft.is_symlink() || !ft.is_file() {
            continue;
        }
        let p = item.path();
        if p.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            let is_primary = p
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|f| f.eq_ignore_ascii_case("Journal.json"));
            files.push((!is_primary, p));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    Ok(files.into_iter().map(|(_, p)| p).collect())
}

fn parse_dayone_json_str(raw: &str) -> Result<Vec<DayOneEntry>> {
    let root: DayOneRoot = serde_json::from_str(raw)?;
    Ok(match root {
        DayOneRoot::Wrapped { entries } => entries,
        DayOneRoot::Array(entries) => entries,
    })
}

fn process_dayone_entry(
    vault: &Vault,
    req: &ImportDayOneRequest,
    dedup: &mut DeduplicationIndex,
    media_source: &mut MediaSource,
    source_id: &str,
    raw: DayOneEntry,
) -> Result<(MigrationAuditItem, usize)> {
    let created_at = raw
        .creation_date
        .as_deref()
        .and_then(parse_dayone_timestamp)
        .unwrap_or_else(Utc::now);
    let updated_at = raw
        .modified_date
        .as_deref()
        .and_then(parse_dayone_timestamp)
        .unwrap_or(created_at);
    let entry_date = created_at.date_naive();
    let date_str = created_at.to_rfc3339();

    let raw_body = extract_dayone_body(&raw);
    let title = derive_title_from_body(&raw_body, entry_date);

    // 1. Deduplicate by source_id (preloaded from Vault + batch dedup index)
    if dedup.has_source_id(source_id) {
        return Ok((
            MigrationAuditItem {
                source_id: source_id.to_string(),
                title: Some(title),
                date: Some(date_str),
                status: MigrationStatus::Skipped,
                reason: Some("Already imported (matching Day One UUID)".to_string()),
                target_entry_id: None,
                target_path: None,
            },
            0,
        ));
    }

    // 2. Deduplicate by (YYYY-MM-DD, normalized_body_hash)
    if dedup.has_date_body_hash(entry_date, &raw_body) {
        return Ok((
            MigrationAuditItem {
                source_id: source_id.to_string(),
                title: Some(title),
                date: Some(date_str),
                status: MigrationStatus::Skipped,
                reason: Some("Duplicate entry on same date with matching body".to_string()),
                target_entry_id: None,
                target_path: None,
            },
            0,
        ));
    }

    // 3. Extract & ingest photos / audio and build moment-ID -> URL rewrite map
    let mut photos: Vec<PolaroidMeta> = Vec::new();
    let mut audio: Vec<VoiceMemoMeta> = Vec::new();
    let mut moment_urls: HashMap<String, (String, bool)> = HashMap::new(); // id -> (url, is_audio)
    let mut media_copied = 0usize;

    for photo in &raw.photos {
        let keys = collect_media_keys(
            photo.filename.as_deref(),
            photo.md5.as_deref(),
            photo.identifier.as_deref(),
        );
        let ext = photo.media_type.as_deref().unwrap_or("jpg");
        if let Some((filename, bytes)) = media_source.read_media(&keys, Some(ext)) {
            if req.dry_run {
                let placeholder_url = format!("/vault-media/photos/{filename}");
                register_moment_keys(&mut moment_urls, &keys, &placeholder_url, false);
                media_copied += 1;
            } else {
                match vault.ingest_photo(&filename, &bytes, None) {
                    Ok(mut meta) => {
                        if meta.taken_at.is_none() {
                            meta.taken_at = photo.date.as_deref().and_then(parse_dayone_timestamp);
                        }
                        if meta.width.is_none() {
                            meta.width = photo.width;
                        }
                        if meta.height.is_none() {
                            meta.height = photo.height;
                        }
                        if meta.location.is_none() {
                            meta.location = photo.location.as_ref().and_then(map_location);
                        }
                        register_moment_keys(&mut moment_urls, &keys, &meta.url, false);
                        photos.push(meta);
                        media_copied += 1;
                    }
                    Err(_) => {
                        // Continue entry import even if an individual exotic image fails decoding.
                    }
                }
            }
        }
    }

    for aud in &raw.audios {
        let keys = collect_media_keys(
            aud.filename.as_deref(),
            aud.md5.as_deref(),
            aud.identifier.as_deref(),
        );
        let ext = aud.format.as_deref().unwrap_or("m4a");
        if let Some((filename, bytes)) = media_source.read_media(&keys, Some(ext)) {
            if req.dry_run {
                let placeholder_url = format!("/vault-media/audio/{filename}");
                register_moment_keys(&mut moment_urls, &keys, &placeholder_url, true);
                media_copied += 1;
            } else if let Ok(meta) = vault.ingest_audio(&filename, &bytes, aud.duration, None) {
                register_moment_keys(&mut moment_urls, &keys, &meta.url, true);
                audio.push(meta);
                media_copied += 1;
            }
        }
    }

    let rewritten_body = rewrite_dayone_moments(&raw_body, &moment_urls);

    let mut clean_tags: Vec<String> = Vec::new();
    for tag in raw.tags {
        let t = tag.trim().to_string();
        if !t.is_empty() && !clean_tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
            clean_tags.push(t);
        }
    }

    dedup.insert(Some(source_id), entry_date, &title, &rewritten_body);

    if req.dry_run {
        return Ok((
            MigrationAuditItem {
                source_id: source_id.to_string(),
                title: Some(title),
                date: Some(date_str),
                status: MigrationStatus::Imported,
                reason: Some("Dry run: would import Day One entry".to_string()),
                target_entry_id: None,
                target_path: None,
            },
            media_copied,
        ));
    }

    let fm = EntryFrontmatter {
        id: Uuid::now_v7(),
        title: title.clone(),
        date: created_at,
        updated_at,
        notebook_id: req.notebook_id.clone(),
        tags: clean_tags,
        mood: None,
        starred: raw.starred,
        source: Some("Day One".to_string()),
        source_id: Some(source_id.to_string()),
        weather: raw.weather.as_ref().and_then(map_weather),
        location: raw.location.as_ref().and_then(map_location),
        stamps: Vec::new(),
        photos,
        audio,
        cover_asset: None,
        links: Vec::new(),
    };

    let saved = vault.save_entry_with_frontmatter(fm, &rewritten_body)?;

    Ok((
        MigrationAuditItem {
            source_id: source_id.to_string(),
            title: Some(saved.frontmatter.title),
            date: Some(date_str),
            status: MigrationStatus::Imported,
            reason: None,
            target_entry_id: Some(saved.id),
            target_path: Some(saved.path),
        },
        media_copied,
    ))
}

fn extract_dayone_body(raw: &DayOneEntry) -> String {
    if let Some(text) = raw.text.as_deref() {
        if !text.trim().is_empty() {
            return unescape_dayone_markdown(text);
        }
    }
    if let Some(rich) = raw.rich_text.as_deref() {
        if let Some(extracted) = parse_dayone_richtext(rich) {
            if !extracted.trim().is_empty() {
                return extracted;
            }
        }
    }
    String::new()
}

fn unescape_dayone_markdown(input: &str) -> String {
    unescape_regex().replace_all(input, "$1").into_owned()
}

fn parse_dayone_richtext(raw_json: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(raw_json).ok()?;
    let contents = val.get("contents")?.as_array()?;
    let mut out = String::new();
    for item in contents {
        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
            out.push_str(text);
        } else if let Some(embedded) = item.get("embeddedObjects").and_then(|v| v.as_array()) {
            for obj in embedded {
                let kind = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(id) = obj.get("identifier").and_then(|v| v.as_str()) {
                    if kind.eq_ignore_ascii_case("audio") {
                        out.push_str(&format!("\n![](dayone-moment://audio/{id})\n"));
                    } else {
                        out.push_str(&format!("\n![](dayone-moment://{id})\n"));
                    }
                }
            }
        }
    }
    Some(out)
}

fn collect_media_keys(
    filename: Option<&str>,
    md5: Option<&str>,
    identifier: Option<&str>,
) -> Vec<String> {
    let mut keys = Vec::new();
    for opt in [filename, md5, identifier] {
        if let Some(val) = opt.map(str::trim).filter(|s| !s.is_empty()) {
            keys.push(val.to_string());
        }
    }
    keys
}

fn register_moment_keys(
    map: &mut HashMap<String, (String, bool)>,
    keys: &[String],
    url: &str,
    is_audio: bool,
) {
    for k in keys {
        map.insert(k.to_ascii_lowercase(), (url.to_string(), is_audio));
        if let Some(stem) = Path::new(k).file_stem().and_then(|s| s.to_str()) {
            map.insert(stem.to_ascii_lowercase(), (url.to_string(), is_audio));
        }
    }
}

/// Rewrite `![alt](dayone-moment://<id>)` and `![alt](dayone-moment://audio/<id>)` into clean vault links.
pub fn rewrite_dayone_moments(body: &str, moment_urls: &HashMap<String, (String, bool)>) -> String {
    moment_regex()
        .replace_all(body, |caps: &regex::Captures| {
            let alt = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let kind_prefix = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let raw_id = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            let key = raw_id.trim().to_ascii_lowercase();

            if let Some((url, is_audio)) = moment_urls.get(&key) {
                if *is_audio || kind_prefix.eq_ignore_ascii_case("audio") {
                    let label = if alt.trim().is_empty() {
                        "Voice Memo"
                    } else {
                        alt.trim()
                    };
                    format!("[{label}]({url})")
                } else {
                    format!("![{alt}]({url})")
                }
            } else {
                caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
            }
        })
        .into_owned()
}

fn parse_dayone_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let naive = d.and_hms_opt(12, 0, 0)?;
        return Some(Utc.from_utc_datetime(&naive));
    }
    None
}

fn map_weather(w: &DayOneWeather) -> Option<WeatherInfo> {
    let condition = w
        .conditions_description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            w.weather_code
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })?
        .to_string();

    let icon_key = infer_weather_icon_key(&condition, w.weather_code.as_deref());
    let humidity_pct = w.relative_humidity.map(|h| {
        let pct = if h <= 1.0 && h > 0.0 { h * 100.0 } else { h };
        pct.round().clamp(0.0, 100.0) as u8
    });
    let summary = w
        .temperature_celsius
        .map(|t| format!("{condition}, {t:.1}°C"));

    Some(WeatherInfo {
        condition,
        temperature_c: w.temperature_celsius,
        icon_key: icon_key.to_string(),
        humidity_pct,
        summary,
    })
}

fn infer_weather_icon_key(condition: &str, code: Option<&str>) -> &'static str {
    let combined = format!(
        "{} {}",
        condition.to_ascii_lowercase(),
        code.unwrap_or("").to_ascii_lowercase()
    );
    if combined.contains("thunder") || combined.contains("storm") {
        "thunderstorm"
    } else if combined.contains("drizzle") {
        "drizzle"
    } else if combined.contains("rain") || combined.contains("shower") {
        "rain"
    } else if combined.contains("snow") || combined.contains("sleet") {
        "snow"
    } else if combined.contains("fog") || combined.contains("mist") || combined.contains("haze") {
        "fog"
    } else if combined.contains("partly") || combined.contains("mostly") {
        "partly-cloudy"
    } else if combined.contains("cloud") || combined.contains("overcast") {
        "cloudy"
    } else {
        "clear-day"
    }
}

fn map_location(loc: &DayOneLocation) -> Option<GeoLocation> {
    let place = loc
        .place_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            loc.locality_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            loc.administrative_area
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            loc.country
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })?;

    Some(GeoLocation {
        place_name: place.to_string(),
        city: loc
            .locality_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        country: loc
            .country
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        latitude: loc.latitude,
        longitude: loc.longitude,
    })
}
