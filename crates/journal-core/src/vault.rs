use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Datelike, Utc};
use journal_types::{
    BacklinkRef, CalendarDaySummary, CreateNotebookRequest, DeleteEntryResponse, Entry,
    EntryFrontmatter, EntrySummary, Notebook, OnThisDayGroup, PolaroidMeta, SaveEntryRequest,
    SearchHit, SearchQuery, TagCount, Template, VaultStats, VoiceMemoMeta,
};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::error::{Result, VaultError};
use crate::index;
use crate::markdown::{extract_wikilinks, parse_entry_file, serialize_entry, slugify};
use crate::media::{self, write_bytes_atomic};
use crate::templates;

const DEFAULT_NOTEBOOK_ID: &str = "daily";
const DEFAULT_NOTEBOOK_NAME: &str = "Daily Sanctuary";
const DEFAULT_NOTEBOOK_COVER: &str = "/assets/covers/rainy-tea-desk.png";
const DEFAULT_NOTEBOOK_ACCENT: &str = "#546E5A";

/// Handle keeping a [`notify::RecommendedWatcher`] alive for a vault directory.
pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
}

/// Primary local-first journal vault engine backed by Markdown + YAML files and SQLite FTS5.
pub struct Vault {
    root: PathBuf,
    conn: Mutex<Connection>,
}

impl Vault {
    /// Resolve the default vault directory (`LOG_VAULT_DIR` env var or `~/Personal/JournalVault`).
    pub fn default_path() -> PathBuf {
        if let Ok(custom) = std::env::var("LOG_VAULT_DIR") {
            let trimmed = custom.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Personal")
            .join("JournalVault")
    }

    /// Open (or initialize) the default journal vault resolved via [`Vault::default_path`].
    pub fn open_default() -> Result<Self> {
        Self::open(Self::default_path())
    }

    /// Open (or initialize) a journal vault at `root`.
    ///
    /// Ensures the directory layout, default `notebooks.json`, starter templates,
    /// and `.index/index.sqlite` exist. Automatically rebuilds the SQLite FTS5 index
    /// when `.index/index.sqlite` was missing.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        ensure_vault_dirs(&root)?;

        let notebooks = ensure_notebooks_file(&root)?;
        templates::seed_templates(&root.join("templates"))?;

        let index_path = root.join(".index").join("index.sqlite");
        let index_existed = index_path.exists();

        let conn = Connection::open(&index_path)?;
        index::init_schema(&conn)?;
        index::sync_notebooks_table(&conn, &notebooks)?;

        let vault = Self {
            root,
            conn: Mutex::new(conn),
        };

        if !index_existed {
            vault.rebuild_index()?;
        }

        Ok(vault)
    }

    /// Return the root filesystem path of this vault.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Scan all `.md` files in `entries/` and rebuild `.index/index.sqlite` from scratch.
    pub fn rebuild_index(&self) -> Result<usize> {
        let notebooks = self.read_notebooks_file()?;
        let entries_dir = self.root.join("entries");

        let mut parsed_entries = Vec::new();
        if entries_dir.exists() {
            for item in WalkDir::new(&entries_dir).follow_links(false) {
                let item = item.map_err(|e| {
                    VaultError::Io(std::io::Error::other(format!("Walkdir error: {e}")))
                })?;
                let path = item.path();
                if !item.file_type().is_file() {
                    continue;
                }
                let is_md = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
                let is_hidden = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|name| name.starts_with('.'));
                if !is_md || is_hidden {
                    continue;
                }

                let rel_path = to_vault_relative_posix(&self.root, path)?;
                let raw = fs::read_to_string(path)?;
                let entry = parse_entry_file(&raw, &rel_path)?;
                parsed_entries.push(entry);
            }
        }

        let mut conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        let tx = conn.transaction()?;

        tx.execute("DELETE FROM entry_tags", [])?;
        tx.execute("DELETE FROM entry_links", [])?;
        tx.execute("DELETE FROM entries_fts", [])?;
        tx.execute("DELETE FROM entries", [])?;
        index::sync_notebooks_table(&tx, &notebooks)?;

        for entry in &parsed_entries {
            index::upsert_entry(&tx, entry)?;
        }

        tx.commit()?;
        Ok(parsed_entries.len())
    }

    /// Start a recursive `notify` filesystem watcher on `entries/` and `notebooks.json`.
    pub fn watch_vault<F>(&self, on_change: F) -> Result<VaultWatcher>
    where
        F: Fn(Event) + Send + 'static,
    {
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                on_change(event);
            }
        })?;

        let entries_dir = self.root.join("entries");
        if entries_dir.exists() {
            watcher.watch(&entries_dir, RecursiveMode::Recursive)?;
        }
        let notebooks_path = self.root.join("notebooks.json");
        if notebooks_path.exists() {
            watcher.watch(&notebooks_path, RecursiveMode::NonRecursive)?;
        }

        Ok(VaultWatcher { _watcher: watcher })
    }

    /// List all notebooks from `notebooks.json` enriched with live `entry_count`.
    pub fn list_notebooks(&self) -> Result<Vec<Notebook>> {
        let mut notebooks = self.read_notebooks_file()?;
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        let counts = index::notebook_entry_counts(&conn)?;
        for nb in &mut notebooks {
            nb.entry_count = counts.get(&nb.id).copied().unwrap_or(0);
        }
        Ok(notebooks)
    }

    /// Create and persist a new notebook in `notebooks.json` and the SQLite index.
    pub fn create_notebook(&self, req: CreateNotebookRequest) -> Result<Notebook> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(VaultError::InvalidInput(
                "Notebook name cannot be empty".to_string(),
            ));
        }

        let mut notebooks = self.read_notebooks_file()?;
        let base_id = slugify(name);
        let mut id = base_id.clone();
        let mut suffix = 2_usize;
        while notebooks.iter().any(|n| n.id.eq_ignore_ascii_case(&id)) {
            id = format!("{base_id}-{suffix}");
            suffix += 1;
        }

        let mut nb = Notebook::new(id, name);
        nb.description = req.description.filter(|d| !d.trim().is_empty());
        if let Some(cover) = req.cover_asset.filter(|c| !c.trim().is_empty()) {
            nb.cover_asset = cover;
        }
        if let Some(accent) = req.accent_color.filter(|c| !c.trim().is_empty()) {
            nb.accent_color = accent;
        }

        notebooks.push(nb.clone());
        self.write_notebooks_file(&notebooks)?;

        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::sync_notebooks_table(&conn, &notebooks)?;

        Ok(nb)
    }

    /// List entry summaries filtered by optional `notebook_id`, `tag`, `limit`, and `offset`.
    pub fn list_entries(
        &self,
        notebook_id: Option<&str>,
        tag: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<EntrySummary>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::list_entries(&conn, notebook_id, tag, limit, offset)
    }

    /// Load a full entry (with `html_preview` and `backlinks`) by UUID, slug, or title.
    pub fn get_entry(&self, id_or_slug_or_title: &str) -> Result<Entry> {
        let (entry_id, rel_path) = {
            let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            index::resolve_entry_path(&conn, id_or_slug_or_title)?.ok_or_else(|| {
                VaultError::NotFound(format!("Entry not found: {id_or_slug_or_title}"))
            })?
        };

        let abs_path = self.root.join(&rel_path);
        if !abs_path.exists() {
            return Err(VaultError::NotFound(format!(
                "Entry file missing on disk: {rel_path}"
            )));
        }
        let raw = fs::read_to_string(&abs_path)?;
        let mut entry = parse_entry_file(&raw, &rel_path)?;

        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        entry.backlinks = index::get_backlinks(&conn, entry_id)?;
        Ok(entry)
    }

    /// Create or update an entry from a [`SaveEntryRequest`].
    pub fn save_entry(&self, req: SaveEntryRequest) -> Result<Entry> {
        let now = Utc::now();
        let existing = if let Some(id) = req.id {
            let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            index::find_existing_entry_meta(&conn, id)?
        } else {
            None
        };

        let id = req.id.unwrap_or_else(Uuid::now_v7);
        let date = req
            .date
            .or_else(|| existing.as_ref().map(|m| m.1))
            .unwrap_or(now);
        let notebook_id = req
            .notebook_id
            .filter(|s| !s.trim().is_empty())
            .or_else(|| existing.as_ref().and_then(|m| m.2.clone()))
            .or_else(|| Some(DEFAULT_NOTEBOOK_ID.to_string()));
        let source = existing
            .as_ref()
            .and_then(|m| m.3.clone())
            .or_else(|| Some("log".to_string()));
        let source_id = existing.as_ref().and_then(|m| m.4.clone());

        let links = extract_wikilinks(&req.body);
        let title = if req.title.trim().is_empty() {
            "Untitled Entry".to_string()
        } else {
            req.title
        };

        let fm = EntryFrontmatter {
            id,
            title,
            date,
            updated_at: now,
            notebook_id,
            tags: req.tags,
            mood: req.mood.filter(|m| !m.trim().is_empty()),
            starred: req.starred,
            source,
            source_id,
            weather: req.weather,
            location: req.location,
            stamps: req.stamps,
            photos: req.photos,
            audio: req.audio,
            links,
        };

        self.save_entry_with_frontmatter(fm, &req.body)
    }

    /// Atomically persist an entry with explicit [`EntryFrontmatter`] (used by editors and importers).
    pub fn save_entry_with_frontmatter(
        &self,
        mut frontmatter: EntryFrontmatter,
        body: &str,
    ) -> Result<Entry> {
        let notebook_id = frontmatter
            .notebook_id
            .as_deref()
            .map(slugify)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_NOTEBOOK_ID.to_string());
        frontmatter.notebook_id = Some(notebook_id.clone());

        for extracted in extract_wikilinks(body) {
            if !frontmatter
                .links
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&extracted))
            {
                frontmatter.links.push(extracted);
            }
        }

        let old_path = {
            let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            index::find_existing_entry_meta(&conn, frontmatter.id)?.map(|m| m.0)
        };

        let rel_path =
            self.allocate_entry_rel_path(&notebook_id, &frontmatter, old_path.as_deref());
        let abs_path = self.root.join(&rel_path);

        let serialized = serialize_entry(&frontmatter, body)?;
        write_bytes_atomic(&abs_path, serialized.as_bytes())?;

        if let Some(prev_rel) = old_path {
            if prev_rel != rel_path {
                let prev_abs = self.root.join(&prev_rel);
                if prev_abs.exists() {
                    let _ = fs::remove_file(prev_abs);
                }
            }
        }

        let mut entry = parse_entry_file(&serialized, &rel_path)?;
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::upsert_entry(&conn, &entry)?;
        entry.backlinks = index::get_backlinks(&conn, entry.id)?;

        Ok(entry)
    }

    /// Look up an entry by external `(source, source_id)` pair for idempotent imports.
    pub fn find_by_source_id(&self, source: &str, source_id: &str) -> Result<Option<EntrySummary>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::find_by_source_id(&conn, source, source_id)
    }

    /// Delete an entry from disk and remove it from the SQLite FTS5 index.
    pub fn delete_entry(&self, id: Uuid) -> Result<DeleteEntryResponse> {
        let removed_path = {
            let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            index::delete_entry_from_index(&conn, id)?
        };

        let Some(rel_path) = removed_path else {
            return Ok(DeleteEntryResponse {
                deleted: false,
                id,
                path: None,
            });
        };

        let abs_path = self.root.join(&rel_path);
        if abs_path.exists() {
            fs::remove_file(&abs_path)?;
        }

        Ok(DeleteEntryResponse {
            deleted: true,
            id,
            path: Some(rel_path),
        })
    }

    /// Execute an FTS5 + metadata search query.
    pub fn search_entries(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::search_entries(&conn, query)
    }

    /// List all tags across the vault with entry counts.
    pub fn list_tags(&self) -> Result<Vec<TagCount>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::list_tags(&conn)
    }

    /// Summarize entries per day for a calendar month.
    pub fn get_calendar_month(
        &self,
        year: i32,
        month: u32,
        notebook_id: Option<&str>,
    ) -> Result<Vec<CalendarDaySummary>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::get_calendar_month(&conn, year, month, notebook_id)
    }

    /// Fetch entries written on `(month, day)` in previous years relative to `current_year`.
    pub fn get_on_this_day(
        &self,
        month: u32,
        day: u32,
        current_year: i32,
    ) -> Result<Vec<OnThisDayGroup>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::get_on_this_day(&conn, month, day, current_year)
    }

    /// Retrieve entries that link to `entry_id` via `[[wikilinks]]`.
    pub fn get_backlinks(&self, entry_id: Uuid) -> Result<Vec<BacklinkRef>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::get_backlinks(&conn, entry_id)
    }

    /// Compute overall vault statistics, streaks, and top tags.
    pub fn get_vault_stats(&self) -> Result<VaultStats> {
        let notebooks = self.read_notebooks_file()?;
        let today = Utc::now().date_naive();
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        let metrics = index::compute_vault_metrics(&conn, today)?;

        Ok(VaultStats {
            total_entries: metrics.total_entries,
            total_words: metrics.total_words,
            total_notebooks: notebooks.len(),
            total_photos: metrics.total_photos,
            total_audio_memos: metrics.total_audio_memos,
            current_streak_days: metrics.current_streak_days,
            longest_streak_days: metrics.longest_streak_days,
            entries_this_month: metrics.entries_this_month,
            top_tags: metrics.top_tags,
            vault_path: self.root.display().to_string(),
        })
    }

    /// Ingest a photo into `media/photos/`, extract EXIF metadata, and generate a 520px thumbnail.
    pub fn ingest_photo(
        &self,
        filename: &str,
        bytes: &[u8],
        caption: Option<String>,
    ) -> Result<PolaroidMeta> {
        media::ingest_photo(&self.root, filename, bytes, caption)
    }

    /// Ingest an audio clip into `media/audio/` and compute 64 normalized `[0.08..=1.0]` peaks.
    pub fn ingest_audio(
        &self,
        filename: &str,
        bytes: &[u8],
        duration_hint: Option<f32>,
        mime_type: Option<String>,
    ) -> Result<VoiceMemoMeta> {
        media::ingest_audio(&self.root, filename, bytes, duration_hint, mime_type)
    }

    /// List editorial Markdown templates stored in `templates/`.
    pub fn list_templates(&self) -> Result<Vec<Template>> {
        templates::load_templates(&self.root.join("templates"))
    }

    fn read_notebooks_file(&self) -> Result<Vec<Notebook>> {
        let path = self.root.join("notebooks.json");
        let raw = fs::read_to_string(&path)?;
        let notebooks: Vec<Notebook> = serde_json::from_str(&raw)?;
        Ok(notebooks)
    }

    fn write_notebooks_file(&self, notebooks: &[Notebook]) -> Result<()> {
        let path = self.root.join("notebooks.json");
        let json = serde_json::to_string_pretty(notebooks)?;
        write_bytes_atomic(&path, json.as_bytes())
    }

    fn allocate_entry_rel_path(
        &self,
        notebook_id: &str,
        frontmatter: &EntryFrontmatter,
        old_rel_path: Option<&str>,
    ) -> String {
        let d = frontmatter.date;
        let year = format!("{:04}", d.year());
        let month = format!("{:02}", d.month());
        let ymd = d.format("%Y-%m-%d").to_string();
        let base_slug = slugify(&frontmatter.title);

        let mut candidate_rel =
            format!("entries/{notebook_id}/{year}/{month}/{ymd}-{base_slug}.md");
        if old_rel_path == Some(candidate_rel.as_str()) {
            return candidate_rel;
        }

        let mut suffix = 2_usize;
        while self.root.join(&candidate_rel).exists() {
            if let Ok(existing_raw) = fs::read_to_string(self.root.join(&candidate_rel)) {
                if let Ok(existing_entry) = parse_entry_file(&existing_raw, &candidate_rel) {
                    if existing_entry.id == frontmatter.id {
                        break;
                    }
                }
            }
            candidate_rel =
                format!("entries/{notebook_id}/{year}/{month}/{ymd}-{base_slug}-{suffix}.md");
            suffix += 1;
        }
        candidate_rel
    }
}

fn ensure_vault_dirs(root: &Path) -> Result<()> {
    let dirs = [
        root.to_path_buf(),
        root.join("entries"),
        root.join("media").join("photos"),
        root.join("media").join("audio"),
        root.join("templates"),
        root.join(".index"),
        root.join(".index").join("thumbs"),
    ];
    for dir in dirs {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn ensure_notebooks_file(root: &Path) -> Result<Vec<Notebook>> {
    let path = root.join("notebooks.json");
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        let parsed: Vec<Notebook> = serde_json::from_str(&raw)?;
        if !parsed.is_empty() {
            return Ok(parsed);
        }
    }

    let default_nb = Notebook {
        id: DEFAULT_NOTEBOOK_ID.to_string(),
        name: DEFAULT_NOTEBOOK_NAME.to_string(),
        description: Some("Daily reflections, quiet moments, and ink on cream paper.".to_string()),
        cover_asset: DEFAULT_NOTEBOOK_COVER.to_string(),
        accent_color: DEFAULT_NOTEBOOK_ACCENT.to_string(),
        created_at: Utc::now(),
        entry_count: 0,
        is_default: true,
    };

    let notebooks = vec![default_nb];
    let json = serde_json::to_string_pretty(&notebooks)?;
    write_bytes_atomic(&path, json.as_bytes())?;
    Ok(notebooks)
}

fn to_vault_relative_posix(root: &Path, abs_path: &Path) -> Result<String> {
    let rel = abs_path
        .strip_prefix(root)
        .map_err(|e| VaultError::InvalidInput(format!("Path outside vault: {e}")))?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}
