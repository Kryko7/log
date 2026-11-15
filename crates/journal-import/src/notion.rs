//! Notion delta importer supporting Notion `.zip` / directory exports (Markdown + CSV)
//! and Notion JSON snapshots matching the `📓 Journal` database schema.
//!
//! Policy: **Day One is Primary** — any Notion entry matching a primary Day One entry
//! (`Source == "Day One"` with an entry on that date, or matching `(YYYY-MM-DD, normalized_title_or_lead)`)
//! is skipped with [`MigrationStatus::Skipped`].

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use journal_core::Vault;
use journal_types::{
    EntryFrontmatter, ImportNotionRequest, MigrationAuditItem, MigrationReport, MigrationStatus,
    PolaroidMeta,
};
use regex::Regex;
use serde_json::Value;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::dedup::{derive_title_from_body, normalize_text, DeduplicationIndex};
use crate::error::{ImportError, Result};

const MAX_ZIP_TEXT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ZIP_MEDIA_BYTES: u64 = 128 * 1024 * 1024;

fn md_image_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"!\[([^\]]*)\]\(([^)\s]+)\)").expect("valid md image regex"))
}

fn notion_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.*?)\s+([0-9a-fA-F]{32})$").expect("valid notion id regex"))
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

fn is_safe_relative_ref(candidate: &str) -> bool {
    if candidate.starts_with('/') || candidate.starts_with('\\') {
        return false;
    }
    let normalized = candidate.replace('\\', "/");
    !Path::new(&normalized).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

#[derive(Debug, Clone)]
pub struct ParsedNotionEntry {
    pub source_id: String,
    pub title: String,
    pub date: DateTime<Utc>,
    pub notion_source: Option<String>,
    pub starred: bool,
    pub tags: Vec<String>,
    pub body: String,
    pub base_dir_in_source: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct CsvRowMeta {
    title: String,
    date: Option<DateTime<Utc>>,
    notion_source: Option<String>,
    starred: bool,
    tags: Vec<String>,
    body: Option<String>,
}

enum NotionAssetStore {
    None,
    Dir {
        canon_root: Option<PathBuf>,
    },
    Zip {
        archive: ZipArchive<BufReader<File>>,
        paths: HashMap<String, usize>,
    },
}

impl NotionAssetStore {
    fn from_dir(root: &Path) -> Self {
        Self::Dir {
            canon_root: fs::canonicalize(root).ok(),
        }
    }

    fn read_relative(
        &mut self,
        base_dir: Option<&str>,
        rel_ref: &str,
    ) -> Option<(String, Vec<u8>)> {
        let decoded = percent_decode(rel_ref.trim());
        if decoded.starts_with("http://")
            || decoded.starts_with("https://")
            || decoded.starts_with("data:")
            || decoded.starts_with("/vault-media/")
            || !is_safe_relative_ref(&decoded)
        {
            return None;
        }

        let mut candidates = Vec::new();
        if let Some(base) = base_dir.filter(|b| !b.is_empty() && is_safe_relative_ref(b)) {
            candidates.push(format!(
                "{}/{}",
                base.trim_end_matches('/'),
                decoded.trim_start_matches("./")
            ));
        }
        candidates.push(decoded.trim_start_matches("./").to_string());
        if let Some(fname) = Path::new(&decoded).file_name().and_then(|s| s.to_str()) {
            candidates.push(fname.to_string());
        }

        match self {
            NotionAssetStore::None => None,
            NotionAssetStore::Dir { canon_root } => {
                let canon_root = canon_root.as_ref()?;
                for cand in candidates {
                    if !is_safe_relative_ref(&cand) {
                        continue;
                    }
                    let candidate_path = canon_root.join(&cand);
                    let Ok(canon_cand) = fs::canonicalize(&candidate_path) else {
                        continue;
                    };
                    if !canon_cand.starts_with(canon_root) || !canon_cand.is_file() {
                        continue;
                    }
                    if let Ok(bytes) = fs::read(&canon_cand) {
                        let filename = canon_cand
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("photo.png")
                            .to_string();
                        if !bytes.is_empty() {
                            return Some((filename, bytes));
                        }
                    }
                }
                None
            }
            NotionAssetStore::Zip { archive, paths } => {
                for cand in candidates {
                    if !is_safe_relative_ref(&cand) {
                        continue;
                    }
                    let norm = cand.replace('\\', "/").to_ascii_lowercase();
                    if let Some(&idx) = paths.get(&norm) {
                        if let Ok(zf) = archive.by_index(idx) {
                            let zf_name = zf.name().map(|c| c.into_owned()).unwrap_or_default();
                            let filename = Path::new(&zf_name)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("photo.png")
                                .to_string();
                            if let Some(buf) = read_bounded_bytes(zf, MAX_ZIP_MEDIA_BYTES) {
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

/// Import Notion-only delta entries from a Notion `.zip` / directory export (Markdown + CSV)
/// or a Notion JSON snapshot matching the `📓 Journal` schema.
pub fn import_notion(vault: &Vault, req: &ImportNotionRequest) -> Result<MigrationReport> {
    let started_at = Utc::now();
    let source_path = Path::new(req.source_path.trim());
    if !source_path.exists() {
        return Err(ImportError::InvalidSource(format!(
            "Notion source path does not exist: {}",
            source_path.display()
        )));
    }

    let (entries, mut assets) = load_notion_entries(source_path)?;
    let mut dedup = DeduplicationIndex::from_vault(vault)?;

    let mut report = MigrationReport {
        source_kind: "Notion".to_string(),
        started_at,
        completed_at: started_at,
        total_found: entries.len(),
        imported_count: 0,
        skipped_count: 0,
        failed_count: 0,
        media_copied: 0,
        items: Vec::with_capacity(entries.len()),
    };

    for entry in entries {
        let sid = entry.source_id.clone();
        match process_notion_entry(vault, req, &mut dedup, &mut assets, entry) {
            Ok((item, media_delta)) => {
                match item.status {
                    MigrationStatus::Imported | MigrationStatus::NeedsReview => {
                        report.imported_count += 1;
                    }
                    MigrationStatus::Skipped => report.skipped_count += 1,
                    MigrationStatus::Failed => report.failed_count += 1,
                }
                report.media_copied += media_delta;
                report.items.push(item);
            }
            Err(err) => {
                report.failed_count += 1;
                report.items.push(MigrationAuditItem {
                    source_id: sid,
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

fn process_notion_entry(
    vault: &Vault,
    req: &ImportNotionRequest,
    dedup: &mut DeduplicationIndex,
    assets: &mut NotionAssetStore,
    entry: ParsedNotionEntry,
) -> Result<(MigrationAuditItem, usize)> {
    let entry_date = entry.date.date_naive();
    let date_str = entry.date.to_rfc3339();
    let title = if entry.title.trim().is_empty() {
        derive_title_from_body(&entry.body, entry_date)
    } else {
        entry.title.trim().to_string()
    };

    // 1. Exact Notion source_id deduplication (preloaded from Vault + batch dedup index)
    if dedup.has_source_id(&entry.source_id) {
        return Ok((
            MigrationAuditItem {
                source_id: entry.source_id,
                title: Some(title),
                date: Some(date_str),
                status: MigrationStatus::Skipped,
                reason: Some("Already imported (matching Notion ID)".to_string()),
                target_entry_id: None,
                target_path: None,
            },
            0,
        ));
    }

    // 2. Day One is Primary:
    // Skip if Source == "Day One" AND an entry on that date exists in the vault,
    // OR if (YYYY-MM-DD, normalized_title_or_lead_paragraph) matches an existing vault entry.
    let is_marked_dayone = entry
        .notion_source
        .as_deref()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("day one"));

    if (is_marked_dayone && dedup.has_date(entry_date))
        || dedup.has_date_title_or_lead(entry_date, &title, &entry.body)
        || dedup.has_date_body_hash(entry_date, &entry.body)
    {
        return Ok((
            MigrationAuditItem {
                source_id: entry.source_id,
                title: Some(title),
                date: Some(date_str),
                status: MigrationStatus::Skipped,
                reason: Some("Duplicate of primary Day One entry".to_string()),
                target_entry_id: None,
                target_path: None,
            },
            0,
        ));
    }

    // 3. Ingest relative Markdown images if present in zip/dir
    let (rewritten_body, photos, media_copied) = ingest_notion_images(
        vault,
        req.dry_run,
        assets,
        entry.base_dir_in_source.as_deref(),
        &entry.body,
    );

    dedup.insert(Some(&entry.source_id), entry_date, &title, &rewritten_body);

    if req.dry_run {
        return Ok((
            MigrationAuditItem {
                source_id: entry.source_id,
                title: Some(title),
                date: Some(date_str),
                status: MigrationStatus::Imported,
                reason: Some("Dry run: would import Notion-only entry".to_string()),
                target_entry_id: None,
                target_path: None,
            },
            media_copied,
        ));
    }

    let fm = EntryFrontmatter {
        id: Uuid::now_v7(),
        title: title.clone(),
        date: entry.date,
        updated_at: entry.date,
        notebook_id: req.notebook_id.clone(),
        tags: entry.tags,
        mood: None,
        starred: entry.starred,
        source: Some("Notion".to_string()),
        source_id: Some(entry.source_id.clone()),
        weather: None,
        location: None,
        stamps: Vec::new(),
        photos,
        audio: Vec::new(),
        links: Vec::new(),
    };

    let saved = vault.save_entry_with_frontmatter(fm, &rewritten_body)?;

    Ok((
        MigrationAuditItem {
            source_id: entry.source_id,
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

fn ingest_notion_images(
    vault: &Vault,
    dry_run: bool,
    assets: &mut NotionAssetStore,
    base_dir: Option<&str>,
    body: &str,
) -> (String, Vec<PolaroidMeta>, usize) {
    let mut photos = Vec::new();
    let mut media_copied = 0usize;

    let rewritten = md_image_regex()
        .replace_all(body, |caps: &regex::Captures| {
            let alt = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let target = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            if let Some((filename, bytes)) = assets.read_relative(base_dir, target) {
                if dry_run {
                    media_copied += 1;
                    return format!("![{alt}](/vault-media/photos/{filename})");
                }
                let caption = if alt.trim().is_empty() {
                    None
                } else {
                    Some(alt.trim().to_string())
                };
                if let Ok(meta) = vault.ingest_photo(&filename, &bytes, caption) {
                    let url = meta.url.clone();
                    photos.push(meta);
                    media_copied += 1;
                    return format!("![{alt}]({url})");
                }
            }
            caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
        })
        .into_owned();

    (rewritten, photos, media_copied)
}

fn load_notion_entries(path: &Path) -> Result<(Vec<ParsedNotionEntry>, NotionAssetStore)> {
    if path.is_dir() {
        return load_notion_from_dir(path);
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if ext == "zip" {
        return load_notion_from_zip(path);
    }
    if ext == "json" {
        let raw = fs::read_to_string(path)?;
        let entries = parse_notion_json_snapshot(&raw)?;
        return Ok((entries, NotionAssetStore::None));
    }
    if ext == "md" || ext == "markdown" {
        let raw = fs::read_to_string(path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("note.md");
        let parsed = parse_notion_markdown(name, "", &raw, &HashMap::new());
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        return Ok((vec![parsed], NotionAssetStore::from_dir(parent)));
    }
    if ext == "csv" {
        let raw = fs::read_to_string(path)?;
        let rows = parse_notion_csv(&raw);
        let entries = rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| csv_row_to_entry(i, row))
            .collect();
        return Ok((entries, NotionAssetStore::None));
    }

    // Fallback: try JSON first
    let raw = fs::read_to_string(path)?;
    let entries = parse_notion_json_snapshot(&raw)?;
    Ok((entries, NotionAssetStore::None))
}

fn load_notion_from_dir(dir: &Path) -> Result<(Vec<ParsedNotionEntry>, NotionAssetStore)> {
    let mut csv_by_title: HashMap<String, CsvRowMeta> = HashMap::new();
    let mut csv_rows: Vec<CsvRowMeta> = Vec::new();
    let mut md_files: Vec<(String, String, String)> = Vec::new();
    let mut json_entries: Vec<ParsedNotionEntry> = Vec::new();

    for item in WalkDir::new(dir).follow_links(false) {
        let item = item.map_err(|e| ImportError::InvalidSource(e.to_string()))?;
        if item.path_is_symlink() || !item.file_type().is_file() {
            continue;
        }
        let p = item.path();
        let rel = p
            .strip_prefix(dir)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/");
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();

        if ext == "csv" {
            if let Ok(raw) = fs::read_to_string(p) {
                for row in parse_notion_csv(&raw) {
                    let key = normalize_text(&row.title);
                    if !key.is_empty() {
                        csv_by_title.insert(key, row.clone());
                    }
                    csv_rows.push(row);
                }
            }
        } else if ext == "md" || ext == "markdown" {
            if let Ok(raw) = fs::read_to_string(p) {
                let fname = p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("note.md")
                    .to_string();
                let parent_rel = Path::new(&rel)
                    .parent()
                    .map(|pp| pp.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                md_files.push((fname, parent_rel, raw));
            }
        } else if ext == "json" {
            if let Ok(raw) = fs::read_to_string(p) {
                if let Ok(mut parsed) = parse_notion_json_snapshot(&raw) {
                    json_entries.append(&mut parsed);
                }
            }
        }
    }

    let entries = combine_md_csv_json(md_files, &csv_by_title, csv_rows, json_entries);
    if entries.is_empty() {
        return Err(ImportError::InvalidSource(format!(
            "No Notion Markdown, CSV, or JSON entries found in {}",
            dir.display()
        )));
    }
    Ok((entries, NotionAssetStore::from_dir(dir)))
}

fn load_notion_from_zip(zip_path: &Path) -> Result<(Vec<ParsedNotionEntry>, NotionAssetStore)> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;

    let mut csv_by_title: HashMap<String, CsvRowMeta> = HashMap::new();
    let mut csv_rows: Vec<CsvRowMeta> = Vec::new();
    let mut md_indices: Vec<(String, String, usize)> = Vec::new();
    let mut json_entries: Vec<ParsedNotionEntry> = Vec::new();
    let mut asset_paths: HashMap<String, usize> = HashMap::new();

    for i in 0..archive.len() {
        let zf = archive.by_index(i)?;
        if zf.is_dir() {
            continue;
        }
        let name = zf.name()?.replace('\\', "/");
        let lower = name.to_ascii_lowercase();
        if lower.contains("__macosx/") {
            continue;
        }

        if lower.ends_with(".csv") {
            if let Some(bytes) = read_bounded_bytes(zf, MAX_ZIP_TEXT_BYTES) {
                if let Ok(raw) = String::from_utf8(bytes) {
                    for row in parse_notion_csv(&raw) {
                        let key = normalize_text(&row.title);
                        if !key.is_empty() {
                            csv_by_title.insert(key, row.clone());
                        }
                        csv_rows.push(row);
                    }
                }
            }
        } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
            let fname = Path::new(&name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("note.md")
                .to_string();
            let parent_rel = Path::new(&name)
                .parent()
                .map(|pp| pp.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            md_indices.push((fname, parent_rel, i));
        } else if lower.ends_with(".json") {
            if let Some(bytes) = read_bounded_bytes(zf, MAX_ZIP_TEXT_BYTES) {
                if let Ok(raw) = String::from_utf8(bytes) {
                    if let Ok(mut parsed) = parse_notion_json_snapshot(&raw) {
                        json_entries.append(&mut parsed);
                    }
                }
            }
        } else {
            asset_paths.insert(lower.clone(), i);
            if let Some(fname) = Path::new(&lower).file_name().and_then(|s| s.to_str()) {
                asset_paths.entry(fname.to_string()).or_insert(i);
            }
        }
    }

    let mut entries = Vec::with_capacity(md_indices.len() + csv_rows.len() + json_entries.len());
    let mut seen_titles = HashSet::new();

    for (fname, parent_rel, idx) in md_indices {
        if let Ok(zf) = archive.by_index(idx) {
            if let Some(bytes) = read_bounded_bytes(zf, MAX_ZIP_TEXT_BYTES) {
                if let Ok(raw) = String::from_utf8(bytes) {
                    let entry = parse_notion_markdown(&fname, &parent_rel, &raw, &csv_by_title);
                    seen_titles.insert(normalize_text(&entry.title));
                    entries.push(entry);
                }
            }
        }
    }

    for (idx, row) in csv_rows.into_iter().enumerate() {
        let key = normalize_text(&row.title);
        if !key.is_empty() && seen_titles.contains(&key) {
            continue;
        }
        entries.push(csv_row_to_entry(idx, row));
    }

    entries.append(&mut json_entries);

    if entries.is_empty() {
        return Err(ImportError::InvalidSource(format!(
            "No Notion Markdown, CSV, or JSON entries found in {}",
            zip_path.display()
        )));
    }

    Ok((
        entries,
        NotionAssetStore::Zip {
            archive,
            paths: asset_paths,
        },
    ))
}

fn combine_md_csv_json(
    md_files: Vec<(String, String, String)>,
    csv_by_title: &HashMap<String, CsvRowMeta>,
    csv_rows: Vec<CsvRowMeta>,
    mut json_entries: Vec<ParsedNotionEntry>,
) -> Vec<ParsedNotionEntry> {
    let mut entries = Vec::new();
    let mut seen_titles = HashSet::new();

    for (fname, parent_rel, raw) in md_files {
        let entry = parse_notion_markdown(&fname, &parent_rel, &raw, csv_by_title);
        seen_titles.insert(normalize_text(&entry.title));
        entries.push(entry);
    }

    // Include CSV rows that had no dedicated .md page (e.g., Day One stubs or inline notes)
    for (idx, row) in csv_rows.into_iter().enumerate() {
        let key = normalize_text(&row.title);
        if !key.is_empty() && seen_titles.contains(&key) {
            continue;
        }
        entries.push(csv_row_to_entry(idx, row));
    }

    entries.append(&mut json_entries);
    entries
}

fn csv_row_to_entry(idx: usize, row: CsvRowMeta) -> ParsedNotionEntry {
    let date = row.date.unwrap_or_else(Utc::now);
    let body = row.body.unwrap_or_else(|| row.title.clone());
    let slug = normalize_text(&row.title).replace(' ', "-");
    let date_tag = row
        .date
        .map(|d| d.format("%Y%m%d").to_string())
        .unwrap_or_else(|| "undated".to_string());
    ParsedNotionEntry {
        source_id: format!("notion-csv-{idx}-{date_tag}-{slug}"),
        title: row.title,
        date,
        notion_source: row.notion_source,
        starred: row.starred,
        tags: row.tags,
        body,
        base_dir_in_source: None,
    }
}

/// Parse Notion JSON snapshots matching the `📓 Journal` schema (`Entry`, `Date` / `date:Date:start`,
/// `Day`, `Source`, `Starred`, `Photos`, `Words`, `body` / `markdown`) as well as Notion API `properties` pages.
pub fn parse_notion_json_snapshot(raw: &str) -> Result<Vec<ParsedNotionEntry>> {
    let val: Value = serde_json::from_str(raw)?;
    let arr = match val {
        Value::Array(items) => items,
        Value::Object(mut obj) => {
            if let Some(Value::Array(items)) = obj.remove("entries") {
                items
            } else if let Some(Value::Array(items)) = obj.remove("results") {
                items
            } else if let Some(Value::Array(items)) = obj.remove("pages") {
                items
            } else {
                vec![Value::Object(obj)]
            }
        }
        _ => {
            return Err(ImportError::InvalidSource(
                "Notion JSON snapshot must be an array or object with entries/results".to_string(),
            ))
        }
    };

    let mut out = Vec::with_capacity(arr.len());
    for (idx, item) in arr.into_iter().enumerate() {
        if let Some(parsed) = parse_notion_json_item(idx, &item) {
            out.push(parsed);
        }
    }
    Ok(out)
}

fn parse_notion_json_item(idx: usize, item: &Value) -> Option<ParsedNotionEntry> {
    let obj = item.as_object()?;
    let props = obj.get("properties").and_then(|v| v.as_object());

    let title = extract_str(obj, &["Entry", "entry", "title", "Title", "name", "Name"])
        .or_else(|| props.and_then(|p| extract_notion_prop_text(p, &["Entry", "Title", "Name"])))
        .unwrap_or_default();

    let body = extract_str(
        obj,
        &[
            "body", "Body", "markdown", "Markdown", "content", "Content", "text", "Text",
        ],
    )
    .unwrap_or_default();

    if title.trim().is_empty() && body.trim().is_empty() {
        return None;
    }

    let date_raw = extract_date_str(
        obj,
        &["date:Date:start", "Date", "date", "created_time", "Created"],
    )
    .or_else(|| props.and_then(|p| extract_notion_prop_date(p, &["Date", "Created"])));
    let date = date_raw
        .as_deref()
        .and_then(parse_flexible_date)
        .unwrap_or_else(Utc::now);

    let notion_source = extract_str(obj, &["Source", "source"])
        .or_else(|| props.and_then(|p| extract_notion_prop_select(p, &["Source"])));

    let starred = extract_bool(obj, &["Starred", "starred"])
        .or_else(|| props.and_then(|p| extract_notion_prop_checkbox(p, &["Starred"])))
        .unwrap_or(false);

    let mut tags = extract_tags(obj, &["Tags", "tags"]);
    if tags.is_empty() {
        if let Some(p) = props {
            tags = extract_notion_prop_multi_select(p, &["Tags"]);
        }
    }

    let source_id = extract_str(obj, &["source_id", "id", "page_id", "url"]).unwrap_or_else(|| {
        let date_tag = date_raw
            .as_deref()
            .and_then(parse_flexible_date)
            .map(|d| d.format("%Y%m%d").to_string())
            .unwrap_or_else(|| "undated".to_string());
        format!(
            "notion-json-{idx}-{date_tag}-{}",
            normalize_text(&title).replace(' ', "-")
        )
    });

    let final_title = if title.trim().is_empty() {
        derive_title_from_body(&body, date.date_naive())
    } else {
        title.trim().to_string()
    };

    Some(ParsedNotionEntry {
        source_id,
        title: final_title,
        date,
        notion_source,
        starred,
        tags,
        body,
        base_dir_in_source: None,
    })
}

fn extract_str(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if let Some(s) = v.as_str() {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            } else if let Some(inner) = v.get("title").or_else(|| v.get("rich_text")) {
                if let Some(s) = join_rich_text_array(inner) {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn extract_date_str(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if let Some(s) = v.as_str() {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            } else if let Some(start) = v.get("start").and_then(|s| s.as_str()) {
                let t = start.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn extract_bool(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if let Some(b) = v.as_bool() {
                return Some(b);
            }
            if let Some(s) = v.as_str() {
                return Some(parse_bool_str(s));
            }
        }
    }
    None
}

fn extract_tags(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Vec<String> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if let Some(arr) = v.as_array() {
                return arr
                    .iter()
                    .filter_map(|item| {
                        item.as_str()
                            .map(str::trim)
                            .or_else(|| item.get("name").and_then(|n| n.as_str()).map(str::trim))
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    })
                    .collect();
            }
            if let Some(s) = v.as_str() {
                return s
                    .split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect();
            }
        }
    }
    Vec::new()
}

fn extract_notion_prop_text(
    props: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    for k in keys {
        if let Some(prop) = props.get(*k) {
            if let Some(arr) = prop.get("title").or_else(|| prop.get("rich_text")) {
                if let Some(s) = join_rich_text_array(arr) {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn join_rich_text_array(val: &Value) -> Option<String> {
    let arr = val.as_array()?;
    let joined: String = arr
        .iter()
        .filter_map(|t| {
            t.get("plain_text").and_then(|v| v.as_str()).or_else(|| {
                t.get("text")
                    .and_then(|x| x.get("content"))
                    .and_then(|v| v.as_str())
            })
        })
        .collect();
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn extract_notion_prop_date(
    props: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    for k in keys {
        if let Some(prop) = props.get(*k) {
            if let Some(start) = prop
                .get("date")
                .and_then(|d| d.get("start"))
                .and_then(|s| s.as_str())
            {
                return Some(start.to_string());
            }
        }
    }
    None
}

fn extract_notion_prop_select(
    props: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    for k in keys {
        if let Some(prop) = props.get(*k) {
            if let Some(name) = prop
                .get("select")
                .and_then(|s| s.get("name"))
                .and_then(|n| n.as_str())
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn extract_notion_prop_checkbox(
    props: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<bool> {
    for k in keys {
        if let Some(prop) = props.get(*k) {
            if let Some(b) = prop.get("checkbox").and_then(|v| v.as_bool()) {
                return Some(b);
            }
        }
    }
    None
}

fn extract_notion_prop_multi_select(
    props: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Vec<String> {
    for k in keys {
        if let Some(prop) = props.get(*k) {
            if let Some(arr) = prop.get("multi_select").and_then(|v| v.as_array()) {
                return arr
                    .iter()
                    .filter_map(|item| item.get("name").and_then(|n| n.as_str()))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    Vec::new()
}

fn parse_notion_markdown(
    filename: &str,
    base_dir_in_source: &str,
    raw: &str,
    csv_by_title: &HashMap<String, CsvRowMeta>,
) -> ParsedNotionEntry {
    let (file_title, file_id) = strip_notion_filename_suffix(filename);
    let mut title = file_title;
    let mut date: Option<DateTime<Utc>> = None;
    let mut notion_source: Option<String> = None;
    let mut starred = false;
    let mut tags: Vec<String> = Vec::new();

    let normalized_raw = raw.replace("\r\n", "\n");
    let mut lines = normalized_raw.lines().peekable();
    let mut body_lines: Vec<&str> = Vec::new();

    // Parse optional YAML frontmatter or Notion `# Title` + `Key: Value` property header
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                apply_notion_header_kv(
                    k,
                    v,
                    &mut title,
                    &mut date,
                    &mut notion_source,
                    &mut starred,
                    &mut tags,
                );
            }
        }
        body_lines.extend(lines);
    } else {
        if let Some(&first) = lines.peek() {
            if let Some(h) = first.trim().strip_prefix("# ") {
                let heading = h.trim();
                if !heading.is_empty() {
                    title = heading.to_string();
                }
                lines.next();
            }
        }
        // Skip blank lines before Notion's property block
        while lines.peek().is_some_and(|l| l.trim().is_empty()) {
            lines.next();
        }
        // Parse `Key: Value` metadata block right under title
        let mut saw_kv = false;
        while let Some(&line) = lines.peek() {
            let t = line.trim();
            if t.is_empty() {
                if saw_kv {
                    lines.next();
                }
                break;
            }
            if let Some((k, v)) = t.split_once(':') {
                let key_lower = k.trim().to_ascii_lowercase();
                if matches!(
                    key_lower.as_str(),
                    "date"
                        | "day"
                        | "source"
                        | "starred"
                        | "tags"
                        | "photos"
                        | "words"
                        | "daily log"
                ) {
                    apply_notion_header_kv(
                        k,
                        v,
                        &mut title,
                        &mut date,
                        &mut notion_source,
                        &mut starred,
                        &mut tags,
                    );
                    saw_kv = true;
                    lines.next();
                    continue;
                }
            }
            break;
        }
        body_lines.extend(lines);
    }

    let body = body_lines.join("\n").trim().to_string();

    // Merge matching CSV metadata if available
    if let Some(csv) = csv_by_title.get(&normalize_text(&title)) {
        if date.is_none() {
            date = csv.date;
        }
        if notion_source.is_none() {
            notion_source = csv.notion_source.clone();
        }
        if !starred {
            starred = csv.starred;
        }
        if tags.is_empty() {
            tags = csv.tags.clone();
        }
    }

    let resolved_date = date.unwrap_or_else(Utc::now);
    let source_id = file_id.unwrap_or_else(|| {
        format!(
            "notion-md-{}-{}",
            resolved_date.format("%Y%m%d"),
            normalize_text(&title).replace(' ', "-")
        )
    });

    ParsedNotionEntry {
        source_id,
        title,
        date: resolved_date,
        notion_source,
        starred,
        tags,
        body,
        base_dir_in_source: if base_dir_in_source.is_empty() {
            None
        } else {
            Some(base_dir_in_source.to_string())
        },
    }
}

fn apply_notion_header_kv(
    raw_k: &str,
    raw_v: &str,
    title: &mut String,
    date: &mut Option<DateTime<Utc>>,
    notion_source: &mut Option<String>,
    starred: &mut bool,
    tags: &mut Vec<String>,
) {
    let key = raw_k.trim().to_ascii_lowercase();
    let val = raw_v.trim().trim_matches('"').trim_matches('\'').trim();
    match key.as_str() {
        "title" | "entry" => {
            if !val.is_empty() {
                *title = val.to_string();
            }
        }
        "date" | "date:date:start" => {
            if date.is_none() {
                *date = parse_flexible_date(val);
            }
        }
        "source" => {
            if !val.is_empty() {
                *notion_source = Some(val.to_string());
            }
        }
        "starred" => {
            *starred = parse_bool_str(val);
        }
        "tags" => {
            let trimmed = val.trim_start_matches('[').trim_end_matches(']');
            for part in trimmed.split(',') {
                let p = part.trim().trim_matches('"').trim_matches('\'').trim();
                if !p.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(p)) {
                    tags.push(p.to_string());
                }
            }
        }
        _ => {}
    }
}

fn strip_notion_filename_suffix(filename: &str) -> (String, Option<String>) {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    if let Some(caps) = notion_id_regex().captures(stem) {
        let title = caps.get(1).map(|m| m.as_str().trim()).unwrap_or(stem);
        let id = caps.get(2).map(|m| m.as_str().to_string());
        (title.to_string(), id)
    } else {
        (stem.trim().to_string(), None)
    }
}

fn parse_notion_csv(raw: &str) -> Vec<CsvRowMeta> {
    let cleaned = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let rows = parse_rfc4180_csv(cleaned);
    if rows.len() < 2 {
        return Vec::new();
    }

    let headers: Vec<String> = rows[0]
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();

    let col_title = find_col(&headers, &["entry", "title", "name"]);
    let col_date = find_col(&headers, &["date", "date:date:start", "created"]);
    let col_source = find_col(&headers, &["source"]);
    let col_starred = find_col(&headers, &["starred"]);
    let col_tags = find_col(&headers, &["tags"]);
    let col_body = find_col(&headers, &["body", "content", "markdown", "notes"]);

    let mut out = Vec::new();
    for row in rows.into_iter().skip(1) {
        let title = col_title
            .and_then(|i| row.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let date = col_date
            .and_then(|i| row.get(i))
            .and_then(|s| parse_flexible_date(s));
        let notion_source = col_source
            .and_then(|i| row.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let starred = col_starred
            .and_then(|i| row.get(i))
            .is_some_and(|s| parse_bool_str(s));
        let tags = col_tags
            .and_then(|i| row.get(i))
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let body = col_body
            .and_then(|i| row.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        out.push(CsvRowMeta {
            title,
            date,
            notion_source,
            starred,
            tags,
            body,
        });
    }
    out
}

fn find_col(headers: &[String], candidates: &[&str]) -> Option<usize> {
    for cand in candidates {
        if let Some(idx) = headers.iter().position(|h| h == cand) {
            return Some(idx);
        }
    }
    None
}

fn parse_rfc4180_csv(input: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else {
            match ch {
                '"' => in_quotes = true,
                ',' => {
                    row.push(std::mem::take(&mut field));
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut field));
                    if !row.iter().all(|c| c.is_empty()) {
                        rows.push(std::mem::take(&mut row));
                    } else {
                        row.clear();
                    }
                }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    if !row.iter().all(|c| c.is_empty()) {
                        rows.push(std::mem::take(&mut row));
                    } else {
                        row.clear();
                    }
                }
                _ => field.push(ch),
            }
        }
    }

    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if !row.iter().all(|c| c.is_empty()) {
            rows.push(row);
        }
    }
    rows
}

fn parse_bool_str(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "1" | "✓" | "checked"
    )
}

fn parse_flexible_date(raw: &str) -> Option<DateTime<Utc>> {
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
        "%B %d, %Y %I:%M %p",
        "%b %d, %Y %I:%M %p",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%B %d, %Y", "%b %d, %Y", "%d %B %Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(trimmed, fmt) {
            let naive = d.and_hms_opt(12, 0, 0)?;
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
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
