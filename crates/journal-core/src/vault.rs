use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use chrono::{Datelike, Local, Utc};
use journal_types::{
    BacklinkRef, CalendarDaySummary, CreateNotebookRequest, DeleteEntryResponse,
    DeleteNotebookResponse, Entry, EntryFrontmatter, EntrySummary, Notebook, OnThisDayGroup,
    PolaroidMeta, SaveEntryRequest, SaveTemplateRequest, SearchHit, SearchQuery, TagCount,
    Template, UpdateNotebookRequest, VaultStats, VoiceMemoMeta,
};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::error::{Result, VaultError};
use crate::index;
use crate::markdown::{
    extract_wikilinks, parse_entry_file, parse_entry_file_no_html, render_html_preview,
    serialize_entry, slugify,
};
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
        } else {
            vault.sync_index_incremental()?;
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
        let disk_files = self.collect_entry_files()?;

        let mut conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        let tx = conn.transaction()?;

        tx.execute("DELETE FROM entry_tags", [])?;
        tx.execute("DELETE FROM entry_links", [])?;
        tx.execute("DELETE FROM entries_fts", [])?;
        tx.execute("DELETE FROM entries", [])?;
        index::sync_notebooks_table(&tx, &notebooks)?;

        let mut indexed = 0usize;
        for (abs_path, rel_path, mtime, size) in disk_files {
            let raw = fs::read_to_string(&abs_path)?;
            let entry = parse_entry_file_no_html(&raw, &rel_path)?;
            index::upsert_entry_with_stat(&tx, &entry, mtime, size)?;
            indexed += 1;
        }

        tx.commit()?;
        Ok(indexed)
    }

    /// Reconcile added, modified, and deleted `.md` files incrementally using `(mtime, size)`.
    pub fn sync_index_incremental(&self) -> Result<usize> {
        let disk_files = self.collect_entry_files()?;
        let mut disk_paths = HashSet::with_capacity(disk_files.len());
        for (_, rel_path, _, _) in &disk_files {
            disk_paths.insert(rel_path.clone());
        }

        let indexed = {
            let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            index::list_indexed_files(&conn)?
        };

        let deleted_paths: Vec<String> = indexed
            .keys()
            .filter(|p| !disk_paths.contains(*p))
            .cloned()
            .collect();

        let changed_files: Vec<(PathBuf, String, i64, i64)> = disk_files
            .into_iter()
            .filter(|(_, rel_path, mtime, size)| {
                indexed.get(rel_path).copied() != Some((*mtime, *size))
            })
            .collect();

        if deleted_paths.is_empty() && changed_files.is_empty() {
            return Ok(0);
        }

        let mut updated_count = 0usize;
        let mut conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        let tx = conn.transaction()?;

        for del_path in deleted_paths {
            index::delete_entry_by_path(&tx, &del_path)?;
            updated_count += 1;
        }

        for (abs_path, rel_path, mtime, size) in changed_files {
            let raw = fs::read_to_string(&abs_path)?;
            let entry = parse_entry_file_no_html(&raw, &rel_path)?;
            index::upsert_entry_with_stat(&tx, &entry, mtime, size)?;
            updated_count += 1;
        }

        tx.commit()?;
        Ok(updated_count)
    }

    fn collect_entry_files(&self) -> Result<Vec<(PathBuf, String, i64, i64)>> {
        let entries_dir = self.root.join("entries");
        let mut files = Vec::new();
        if !entries_dir.exists() {
            return Ok(files);
        }

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
            let (mtime, size) = file_stat(path)?;
            files.push((path.to_path_buf(), rel_path, mtime, size));
        }
        Ok(files)
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

    /// List all notebooks from SQLite enriched with live `entry_count`.
    pub fn list_notebooks(&self) -> Result<Vec<Notebook>> {
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::list_notebooks_with_counts(&conn)
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

    /// Update an existing notebook's name, description, cover asset, or accent color.
    pub fn update_notebook(&self, req: UpdateNotebookRequest) -> Result<Notebook> {
        let target_id = req.id.trim();
        if target_id.is_empty() {
            return Err(VaultError::InvalidInput(
                "Notebook id cannot be empty".to_string(),
            ));
        }
        let name = req.name.trim();
        if name.is_empty() {
            return Err(VaultError::InvalidInput(
                "Notebook name cannot be empty".to_string(),
            ));
        }

        let mut notebooks = self.read_notebooks_file()?;
        let Some(nb) = notebooks.iter_mut().find(|n| n.id == target_id) else {
            return Err(VaultError::NotFound(format!(
                "Notebook not found: {target_id}"
            )));
        };

        nb.name = name.to_string();
        if let Some(desc) = req.description {
            let trimmed = desc.trim();
            nb.description = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        if let Some(cover) = req.cover_asset {
            nb.cover_asset = cover.trim().to_string();
        }
        if let Some(accent) = req.accent_color {
            let trimmed = accent.trim();
            if !trimmed.is_empty() {
                nb.accent_color = trimmed.to_string();
            }
        }
        let mut updated = nb.clone();

        self.write_notebooks_file(&notebooks)?;

        let mut conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        let tx = conn.transaction()?;
        index::sync_notebooks_table(&tx, &notebooks)?;
        tx.commit()?;

        let counts = index::notebook_entry_counts(&conn)?;
        updated.entry_count = counts.get(target_id).copied().unwrap_or(0);
        Ok(updated)
    }

    /// Delete a notebook and either delete its entries or reassign them to the first remaining notebook.
    pub fn delete_notebook(
        &self,
        id: &str,
        delete_entries: bool,
    ) -> Result<DeleteNotebookResponse> {
        let target_id = id.trim();
        if target_id.is_empty() {
            return Ok(DeleteNotebookResponse {
                deleted: false,
                id: String::new(),
                deleted_entries: 0,
                reassigned_to: None,
            });
        }

        let mut notebooks = self.read_notebooks_file()?;
        let orig_len = notebooks.len();
        notebooks.retain(|n| n.id != target_id);
        let had_notebook = notebooks.len() != orig_len;

        let entries_in_nb = {
            let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            index::list_entry_ids_and_paths_for_notebook(&conn, target_id)?
        };

        if !had_notebook && entries_in_nb.is_empty() {
            return Ok(DeleteNotebookResponse {
                deleted: false,
                id: target_id.to_string(),
                deleted_entries: 0,
                reassigned_to: None,
            });
        }

        if delete_entries {
            let mut conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            let tx = conn.transaction()?;
            for (entry_id, _) in &entries_in_nb {
                index::delete_entry_from_index(&tx, *entry_id)?;
            }
            index::sync_notebooks_table(&tx, &notebooks)?;
            tx.commit()?;
            drop(conn);

            self.write_notebooks_file(&notebooks)?;

            let mut deleted_count = 0usize;
            for (_, rel_path) in &entries_in_nb {
                if is_safe_vault_rel_path(rel_path) {
                    let abs_path = self.root.join(rel_path);
                    if abs_path.exists() {
                        fs::remove_file(&abs_path)?;
                    }
                }
                deleted_count += 1;
            }

            return Ok(DeleteNotebookResponse {
                deleted: true,
                id: target_id.to_string(),
                deleted_entries: deleted_count,
                reassigned_to: None,
            });
        }

        let fallback_nb = notebooks.first().map(|n| n.id.clone());
        let mut updated_entries = Vec::with_capacity(entries_in_nb.len());

        for (_, rel_path) in &entries_in_nb {
            if !is_safe_vault_rel_path(rel_path) {
                return Err(VaultError::InvalidInput(format!(
                    "Unsafe vault entry path: {rel_path}"
                )));
            }
            let abs_path = self.root.join(rel_path);
            let raw = fs::read_to_string(&abs_path)?;
            let mut entry = parse_entry_file_no_html(&raw, rel_path)?;
            entry.frontmatter.notebook_id = fallback_nb.clone();

            let new_rel_path = match fallback_nb.as_deref() {
                Some(fb_id) => self.allocate_entry_rel_path(
                    &slugify(fb_id),
                    &entry.frontmatter,
                    Some(rel_path),
                ),
                None => rel_path.clone(),
            };
            let new_abs_path = self.root.join(&new_rel_path);
            let serialized = serialize_entry(&entry.frontmatter, &entry.body)?;
            write_bytes_atomic(&new_abs_path, serialized.as_bytes())?;

            if new_rel_path != *rel_path && abs_path.exists() {
                let _ = fs::remove_file(&abs_path);
            }

            let (mtime, size) = file_stat(&new_abs_path)?;
            entry.path = new_rel_path;
            updated_entries.push((entry, mtime, size));
        }

        {
            let mut conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            let tx = conn.transaction()?;
            for (entry, mtime, size) in &updated_entries {
                index::upsert_entry_with_stat(&tx, entry, *mtime, *size)?;
            }
            index::sync_notebooks_table(&tx, &notebooks)?;
            tx.commit()?;
        }

        self.write_notebooks_file(&notebooks)?;

        Ok(DeleteNotebookResponse {
            deleted: true,
            id: target_id.to_string(),
            deleted_entries: 0,
            reassigned_to: fallback_nb,
        })
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

        if !is_safe_vault_rel_path(&rel_path) {
            return Err(VaultError::InvalidInput(format!(
                "Unsafe vault entry path: {rel_path}"
            )));
        }

        let abs_path = self.root.join(&rel_path);
        if !abs_path.exists() {
            return Err(VaultError::NotFound(format!(
                "Entry file missing on disk: {rel_path}"
            )));
        }
        let canon_root = fs::canonicalize(&self.root)?;
        let canon_abs = fs::canonicalize(&abs_path)?;
        if !canon_abs.starts_with(&canon_root) {
            return Err(VaultError::InvalidInput(format!(
                "Entry path escapes vault root: {rel_path}"
            )));
        }

        let raw = fs::read_to_string(&canon_abs)?;
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
            cover_asset: req.cover_asset.filter(|c| !c.trim().is_empty()),
            links: Vec::new(),
        };

        self.save_entry_internal(fm, &req.body, existing.map(|m| m.0))
    }

    /// Atomically persist an entry with explicit [`EntryFrontmatter`] (used by editors and importers).
    pub fn save_entry_with_frontmatter(
        &self,
        frontmatter: EntryFrontmatter,
        body: &str,
    ) -> Result<Entry> {
        let old_path = {
            let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
            index::find_existing_entry_meta(&conn, frontmatter.id)?.map(|m| m.0)
        };
        self.save_entry_internal(frontmatter, body, old_path)
    }

    fn save_entry_internal(
        &self,
        mut frontmatter: EntryFrontmatter,
        body: &str,
        old_path: Option<String>,
    ) -> Result<Entry> {
        let notebook_id = frontmatter
            .notebook_id
            .as_deref()
            .map(slugify)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_NOTEBOOK_ID.to_string());
        frontmatter.notebook_id = Some(notebook_id.clone());

        // Keep frontmatter.links synchronized with the current Markdown prose.
        frontmatter.links = extract_wikilinks(body);

        let rel_path =
            self.allocate_entry_rel_path(&notebook_id, &frontmatter, old_path.as_deref());
        let abs_path = self.root.join(&rel_path);

        let serialized = serialize_entry(&frontmatter, body)?;
        write_bytes_atomic(&abs_path, serialized.as_bytes())?;

        if let Some(prev_rel) = old_path {
            if prev_rel != rel_path && is_safe_vault_rel_path(&prev_rel) {
                let prev_abs = self.root.join(&prev_rel);
                if prev_abs.exists() {
                    let _ = fs::remove_file(prev_abs);
                }
            }
        }

        let (mtime, size) = file_stat(&abs_path)?;
        let mut entry = Entry::new(frontmatter, body, &rel_path);
        entry.html_preview = Some(render_html_preview(&entry.body));

        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        index::upsert_entry_with_stat(&conn, &entry, mtime, size)?;
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
        let today = Local::now().date_naive();
        let conn = self.conn.lock().map_err(|_| VaultError::LockPoisoned)?;
        let total_notebooks: i64 =
            conn.query_row("SELECT COUNT(*) FROM notebooks", [], |row| row.get(0))?;
        let metrics = index::compute_vault_metrics(&conn, today)?;

        Ok(VaultStats {
            total_entries: metrics.total_entries,
            total_words: metrics.total_words,
            total_notebooks: total_notebooks as usize,
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

    /// Save or update a custom editorial Markdown template in `templates/`.
    pub fn save_template(&self, req: SaveTemplateRequest) -> Result<Template> {
        templates::save_template(&self.root.join("templates"), req)
    }

    /// Delete a template by ID from `templates/`.
    pub fn delete_template(&self, id: &str) -> Result<bool> {
        templates::delete_template(&self.root.join("templates"), id)
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
                if let Ok(existing_entry) = parse_entry_file_no_html(&existing_raw, &candidate_rel)
                {
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

fn file_stat(path: &Path) -> Result<(i64, i64)> {
    let meta = fs::metadata(path)?;
    let size = meta.len() as i64;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0);
    Ok((mtime_ns, size))
}

fn is_safe_vault_rel_path(rel_path: &str) -> bool {
    let path = Path::new(rel_path);
    !rel_path.is_empty()
        && !path.is_absolute()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
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
