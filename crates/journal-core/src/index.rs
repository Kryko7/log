use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use journal_types::{
    BacklinkRef, CalendarDaySummary, Entry, EntrySummary, GeoLocation, Notebook, OnThisDayGroup,
    SearchHit, SearchQuery, StampPlacement, TagCount, WeatherInfo,
};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Row};
use uuid::Uuid;

use crate::error::{Result, VaultError};
use crate::markdown::{escape_html_text, extract_wikilinks, slugify};

/// Initialize all SQLite tables and FTS5 virtual table if they do not exist.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA busy_timeout = 5000;
        PRAGMA temp_store = MEMORY;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS notebooks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            cover_asset TEXT NOT NULL,
            accent_color TEXT NOT NULL,
            created_at TEXT NOT NULL,
            is_default INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS entries (
            id TEXT PRIMARY KEY,
            slug TEXT NOT NULL,
            title TEXT NOT NULL,
            date TEXT NOT NULL,
            date_ymd TEXT NOT NULL,
            year INTEGER NOT NULL,
            month INTEGER NOT NULL,
            day INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            notebook_id TEXT,
            mood TEXT,
            starred INTEGER NOT NULL DEFAULT 0,
            source TEXT,
            source_id TEXT,
            path TEXT NOT NULL UNIQUE,
            snippet TEXT NOT NULL,
            word_count INTEGER NOT NULL DEFAULT 0,
            reading_minutes INTEGER NOT NULL DEFAULT 0,
            cover_photo TEXT,
            has_audio INTEGER NOT NULL DEFAULT 0,
            has_photos INTEGER NOT NULL DEFAULT 0,
            photo_count INTEGER NOT NULL DEFAULT 0,
            audio_count INTEGER NOT NULL DEFAULT 0,
            tags_json TEXT NOT NULL DEFAULT '[]',
            weather_json TEXT,
            location_json TEXT,
            stamps_json TEXT NOT NULL DEFAULT '[]',
            place_name TEXT,
            file_mtime INTEGER NOT NULL DEFAULT 0,
            file_size INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_entries_date ON entries(date DESC);
        CREATE INDEX IF NOT EXISTS idx_entries_notebook ON entries(notebook_id, date DESC);
        CREATE INDEX IF NOT EXISTS idx_entries_ymd ON entries(year, month, day);
        CREATE INDEX IF NOT EXISTS idx_entries_mdy ON entries(month, day, year);
        CREATE INDEX IF NOT EXISTS idx_entries_date_ymd ON entries(date_ymd);
        CREATE INDEX IF NOT EXISTS idx_entries_slug ON entries(slug);
        CREATE INDEX IF NOT EXISTS idx_entries_slug_nocase ON entries(LOWER(slug));
        CREATE INDEX IF NOT EXISTS idx_entries_title_nocase ON entries(LOWER(title));
        CREATE INDEX IF NOT EXISTS idx_entries_source ON entries(source, source_id);

        CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
            title,
            body,
            tags,
            place_name,
            mood
        );

        CREATE TABLE IF NOT EXISTS entry_tags (
            entry_id TEXT NOT NULL,
            tag TEXT NOT NULL,
            PRIMARY KEY (entry_id, tag)
        );
        CREATE INDEX IF NOT EXISTS idx_entry_tags_tag ON entry_tags(tag);
        CREATE INDEX IF NOT EXISTS idx_entry_tags_tag_nocase ON entry_tags(LOWER(tag), entry_id);

        CREATE TABLE IF NOT EXISTS entry_links (
            source_id TEXT NOT NULL,
            target_ref TEXT NOT NULL,
            PRIMARY KEY (source_id, target_ref)
        );
        CREATE INDEX IF NOT EXISTS idx_entry_links_target ON entry_links(target_ref);
        CREATE INDEX IF NOT EXISTS idx_entry_links_target_nocase ON entry_links(LOWER(target_ref), source_id);
        "#,
    )?;

    ensure_column(conn, "entries", "file_mtime", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(conn, "entries", "file_size", "INTEGER NOT NULL DEFAULT 0")?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    let mut exists = false;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            exists = true;
            break;
        }
    }
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

/// Sync the `notebooks` table from `notebooks.json` contents.
pub fn sync_notebooks_table(conn: &Connection, notebooks: &[Notebook]) -> Result<()> {
    conn.execute("DELETE FROM notebooks", [])?;
    let mut stmt = conn.prepare(
        "INSERT INTO notebooks (id, name, description, cover_asset, accent_color, created_at, is_default)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for nb in notebooks {
        stmt.execute(params![
            nb.id,
            nb.name,
            nb.description,
            nb.cover_asset,
            nb.accent_color,
            nb.created_at.to_rfc3339(),
            i32::from(nb.is_default),
        ])?;
    }
    Ok(())
}

/// Insert or update one entry across `entries`, `entries_fts`, `entry_tags`, and `entry_links`.
pub fn upsert_entry(conn: &Connection, entry: &Entry) -> Result<()> {
    upsert_entry_with_stat(conn, entry, 0, 0)
}

/// Insert or update one entry along with its source file `(mtime_ns, size)` fingerprint.
pub fn upsert_entry_with_stat(
    conn: &Connection,
    entry: &Entry,
    file_mtime: i64,
    file_size: i64,
) -> Result<()> {
    let id_str = entry.id.to_string();
    let slug = slug_from_path_or_title(&entry.path, &entry.frontmatter.title);
    let summary = entry.to_summary();
    let date_utc = entry.frontmatter.date;
    let date_ymd = date_utc.date_naive().format("%Y-%m-%d").to_string();

    let tags_json = serde_json::to_string(&entry.frontmatter.tags)?;
    let weather_json = entry
        .frontmatter
        .weather
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let location_json = entry
        .frontmatter
        .location
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let stamps_json = serde_json::to_string(&entry.frontmatter.stamps)?;
    let place_name = entry
        .frontmatter
        .location
        .as_ref()
        .map(|l| l.place_name.clone());

    // If a different entry ID previously occupied `entry.path`, clean up all its index rows first.
    if let Some((other_rowid, other_id)) = conn
        .query_row(
            "SELECT rowid, id FROM entries WHERE path = ?1 AND id != ?2",
            params![&entry.path, &id_str],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        delete_index_rows(conn, other_rowid, &other_id)?;
    }

    conn.execute(
        "INSERT INTO entries (
            id, slug, title, date, date_ymd, year, month, day, updated_at,
            notebook_id, mood, starred, source, source_id, path, snippet,
            word_count, reading_minutes, cover_photo, has_audio, has_photos,
            photo_count, audio_count, tags_json, weather_json, location_json,
            stamps_json, place_name, file_mtime, file_size
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
            ?10, ?11, ?12, ?13, ?14, ?15, ?16,
            ?17, ?18, ?19, ?20, ?21,
            ?22, ?23, ?24, ?25, ?26,
            ?27, ?28, ?29, ?30
        )
        ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            title = excluded.title,
            date = excluded.date,
            date_ymd = excluded.date_ymd,
            year = excluded.year,
            month = excluded.month,
            day = excluded.day,
            updated_at = excluded.updated_at,
            notebook_id = excluded.notebook_id,
            mood = excluded.mood,
            starred = excluded.starred,
            source = excluded.source,
            source_id = excluded.source_id,
            path = excluded.path,
            snippet = excluded.snippet,
            word_count = excluded.word_count,
            reading_minutes = excluded.reading_minutes,
            cover_photo = excluded.cover_photo,
            has_audio = excluded.has_audio,
            has_photos = excluded.has_photos,
            photo_count = excluded.photo_count,
            audio_count = excluded.audio_count,
            tags_json = excluded.tags_json,
            weather_json = excluded.weather_json,
            location_json = excluded.location_json,
            stamps_json = excluded.stamps_json,
            place_name = excluded.place_name,
            file_mtime = excluded.file_mtime,
            file_size = excluded.file_size",
        params![
            id_str,
            slug,
            entry.frontmatter.title,
            date_utc.to_rfc3339(),
            date_ymd,
            date_utc.year(),
            date_utc.month() as i64,
            date_utc.day() as i64,
            entry.frontmatter.updated_at.to_rfc3339(),
            entry.frontmatter.notebook_id,
            entry.frontmatter.mood,
            i32::from(entry.frontmatter.starred),
            entry.frontmatter.source,
            entry.frontmatter.source_id,
            entry.path,
            summary.snippet,
            entry.word_count as i64,
            entry.reading_minutes as i64,
            summary.cover_photo,
            i32::from(summary.has_audio),
            i32::from(summary.has_photos),
            entry.frontmatter.photos.len() as i64,
            entry.frontmatter.audio.len() as i64,
            tags_json,
            weather_json,
            location_json,
            stamps_json,
            place_name,
            file_mtime,
            file_size,
        ],
    )?;

    let rowid: i64 = conn.query_row(
        "SELECT rowid FROM entries WHERE id = ?1",
        params![&id_str],
        |row| row.get(0),
    )?;
    let tags_joined = entry.frontmatter.tags.join(" ");
    conn.execute("DELETE FROM entries_fts WHERE rowid = ?1", params![rowid])?;
    conn.execute(
        "INSERT INTO entries_fts (rowid, title, body, tags, place_name, mood)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            rowid,
            entry.frontmatter.title,
            entry.body,
            tags_joined,
            place_name.unwrap_or_default(),
            entry.frontmatter.mood.clone().unwrap_or_default(),
        ],
    )?;

    conn.execute(
        "DELETE FROM entry_tags WHERE entry_id = ?1",
        params![&id_str],
    )?;
    if !entry.frontmatter.tags.is_empty() {
        let mut tag_stmt =
            conn.prepare("INSERT OR IGNORE INTO entry_tags (entry_id, tag) VALUES (?1, ?2)")?;
        for tag in &entry.frontmatter.tags {
            let trimmed = tag.trim();
            if !trimmed.is_empty() {
                tag_stmt.execute(params![&id_str, trimmed])?;
            }
        }
    }

    conn.execute(
        "DELETE FROM entry_links WHERE source_id = ?1",
        params![&id_str],
    )?;
    let mut all_links = entry.frontmatter.links.clone();
    for extracted in extract_wikilinks(&entry.body) {
        if !all_links
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&extracted))
        {
            all_links.push(extracted);
        }
    }
    if !all_links.is_empty() {
        let mut link_stmt = conn
            .prepare("INSERT OR IGNORE INTO entry_links (source_id, target_ref) VALUES (?1, ?2)")?;
        for target in all_links {
            let trimmed = target.trim();
            if !trimmed.is_empty() {
                link_stmt.execute(params![&id_str, trimmed])?;
            }
        }
    }

    Ok(())
}

fn delete_index_rows(conn: &Connection, rowid: i64, id_str: &str) -> Result<()> {
    conn.execute("DELETE FROM entries_fts WHERE rowid = ?1", params![rowid])?;
    conn.execute(
        "DELETE FROM entry_tags WHERE entry_id = ?1",
        params![id_str],
    )?;
    conn.execute(
        "DELETE FROM entry_links WHERE source_id = ?1",
        params![id_str],
    )?;
    conn.execute("DELETE FROM entries WHERE id = ?1", params![id_str])?;
    Ok(())
}

/// Remove an entry from all SQLite index tables by UUID.
pub fn delete_entry_from_index(conn: &Connection, id: Uuid) -> Result<Option<String>> {
    let id_str = id.to_string();
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT rowid, path FROM entries WHERE id = ?1",
            params![&id_str],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let Some((rowid, path)) = existing else {
        return Ok(None);
    };

    delete_index_rows(conn, rowid, &id_str)?;
    Ok(Some(path))
}

/// Remove an entry from all SQLite index tables by vault-relative path.
pub fn delete_entry_by_path(conn: &Connection, rel_path: &str) -> Result<()> {
    if let Some((rowid, id_str)) = conn
        .query_row(
            "SELECT rowid, id FROM entries WHERE path = ?1",
            params![rel_path],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        delete_index_rows(conn, rowid, &id_str)?;
    }
    Ok(())
}

/// Return indexed `path -> (file_mtime, file_size)` map for incremental startup sync.
pub fn list_indexed_files(conn: &Connection) -> Result<HashMap<String, (i64, i64)>> {
    let mut stmt = conn.prepare("SELECT path, file_mtime, file_size FROM entries")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?),
        ))
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let (path, stat) = r?;
        map.insert(path, stat);
    }
    Ok(map)
}

/// List notebooks from SQLite enriched with live entry counts in a single query.
pub fn list_notebooks_with_counts(conn: &Connection) -> Result<Vec<Notebook>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.name, n.description, n.cover_asset, n.accent_color,
                n.created_at, n.is_default, COUNT(e.id) AS entry_count
         FROM notebooks n
         LEFT JOIN entries e ON COALESCE(e.notebook_id, 'daily') = n.id
         GROUP BY n.id
         ORDER BY n.rowid ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let created_str: String = row.get(5)?;
        let created_at = DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
        Ok(Notebook {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            cover_asset: row.get(3)?,
            accent_color: row.get(4)?,
            created_at,
            is_default: row.get::<_, i64>(6)? != 0,
            entry_count: row.get::<_, i64>(7)? as usize,
        })
    })?;
    let mut notebooks = Vec::new();
    for r in rows {
        notebooks.push(r?);
    }
    Ok(notebooks)
}

/// List `(id, path)` pairs for all entries currently belonging to `notebook_id`.
pub fn list_entry_ids_and_paths_for_notebook(
    conn: &Connection,
    notebook_id: &str,
) -> Result<Vec<(Uuid, String)>> {
    let mut stmt = conn.prepare("SELECT id, path FROM entries WHERE notebook_id = ?1")?;
    let rows = stmt.query_map(params![notebook_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id_str, path) = r?;
        let uuid = Uuid::parse_str(&id_str)
            .map_err(|e| VaultError::InvalidInput(format!("Corrupt UUID in index: {e}")))?;
        out.push((uuid, path));
    }
    Ok(out)
}

/// Count entries per `notebook_id`.
pub fn notebook_entry_counts(conn: &Connection) -> Result<BTreeMap<String, usize>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(notebook_id, 'daily') AS nb, COUNT(*) FROM entries GROUP BY nb",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
    })?;
    let mut counts = BTreeMap::new();
    for r in rows {
        let (nb, count) = r?;
        counts.insert(nb, count);
    }
    Ok(counts)
}

/// List entry summaries filtered by optional `notebook_id` and `tag`, newest first.
pub fn list_entries(
    conn: &Connection,
    notebook_id: Option<&str>,
    tag: Option<&str>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<EntrySummary>> {
    let mut sql = String::from(
        "SELECT e.id, e.title, e.date, e.updated_at, e.notebook_id, e.mood, e.starred,
                e.snippet, e.word_count, e.reading_minutes, e.path, e.cover_photo,
                e.has_audio, e.has_photos, e.tags_json, e.weather_json, e.location_json, e.stamps_json
         FROM entries e WHERE 1=1",
    );
    let mut bind: Vec<Value> = Vec::new();

    if let Some(nb) = notebook_id.filter(|s| !s.trim().is_empty()) {
        sql.push_str(" AND e.notebook_id = ?");
        bind.push(Value::Text(nb.to_string()));
    }
    if let Some(t) = tag.filter(|s| !s.trim().is_empty()) {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM entry_tags et WHERE et.entry_id = e.id AND LOWER(et.tag) = LOWER(?))",
        );
        bind.push(Value::Text(t.to_string()));
    }

    sql.push_str(" ORDER BY e.date DESC, e.updated_at DESC");

    let lim = limit.map(|l| l as i64).unwrap_or(-1);
    let off = offset.unwrap_or(0) as i64;
    sql.push_str(" LIMIT ? OFFSET ?");
    bind.push(Value::Integer(lim));
    bind.push(Value::Integer(off));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(bind), row_to_entry_summary)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Lookup an entry's vault-relative path by UUID, slug, or title.
pub fn resolve_entry_path(
    conn: &Connection,
    id_or_slug_or_title: &str,
) -> Result<Option<(Uuid, String)>> {
    let needle = id_or_slug_or_title.trim();
    if needle.is_empty() {
        return Ok(None);
    }

    if let Ok(uuid) = Uuid::parse_str(needle) {
        if let Some(path) = conn
            .query_row(
                "SELECT path FROM entries WHERE id = ?1",
                params![uuid.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(Some((uuid, path)));
        }
    }

    let like_pattern = format!("%/{}.md", escape_sql_like(&needle.to_ascii_lowercase()));
    let found: Option<(String, String)> = conn
        .query_row(
            "SELECT id, path FROM entries
             WHERE LOWER(slug) = LOWER(?1)
                OR LOWER(title) = LOWER(?1)
                OR path = ?1
                OR LOWER(path) LIKE ?2 ESCAPE '\\'
             ORDER BY date DESC
             LIMIT 1",
            params![needle, like_pattern],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    match found {
        Some((id_str, path)) => {
            let uuid = Uuid::parse_str(&id_str)
                .map_err(|e| VaultError::InvalidInput(format!("Corrupt UUID in index: {e}")))?;
            Ok(Some((uuid, path)))
        }
        None => Ok(None),
    }
}

/// Existing entry metadata `(path, date, notebook_id, source, source_id)` tuple.
pub type ExistingEntryMeta = (
    String,
    DateTime<Utc>,
    Option<String>,
    Option<String>,
    Option<String>,
);

type RawEntryMetaRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Lookup existing entry metadata `(path, date, notebook_id, source, source_id)` by UUID.
pub fn find_existing_entry_meta(conn: &Connection, id: Uuid) -> Result<Option<ExistingEntryMeta>> {
    let row: Option<RawEntryMetaRow> = conn
        .query_row(
            "SELECT path, date, notebook_id, source, source_id FROM entries WHERE id = ?1",
            params![id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;

    let Some((path, date_str, notebook_id, source, source_id)) = row else {
        return Ok(None);
    };

    let date = DateTime::parse_from_rfc3339(&date_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| VaultError::InvalidInput(format!("Corrupt timestamp in index: {e}")))?;

    Ok(Some((path, date, notebook_id, source, source_id)))
}

/// Find an entry by `(source, source_id)` for idempotent imports.
pub fn find_by_source_id(
    conn: &Connection,
    source: &str,
    source_id: &str,
) -> Result<Option<EntrySummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, date, updated_at, notebook_id, mood, starred,
                snippet, word_count, reading_minutes, path, cover_photo,
                has_audio, has_photos, tags_json, weather_json, location_json, stamps_json
         FROM entries
         WHERE source = ?1 AND source_id = ?2
         LIMIT 1",
    )?;
    stmt.query_row(params![source, source_id], row_to_entry_summary)
        .optional()
        .map_err(Into::into)
}

/// Execute full-text FTS5 + structured metadata search.
pub fn search_entries(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>> {
    let fts_expr = query.text.as_deref().and_then(build_fts5_query);

    let mut sql = if fts_expr.is_some() {
        String::from(
            "SELECT e.id, e.title, e.date, e.updated_at, e.notebook_id, e.mood, e.starred,
                    e.snippet, e.word_count, e.reading_minutes, e.path, e.cover_photo,
                    e.has_audio, e.has_photos, e.tags_json, e.weather_json, e.location_json, e.stamps_json,
                    snippet(entries_fts, 1, char(2), char(3), '…', 24) AS body_snip,
                    snippet(entries_fts, -1, char(2), char(3), '…', 24) AS any_snip,
                    bm25(entries_fts) AS rank
             FROM entries_fts
             JOIN entries e ON e.rowid = entries_fts.rowid
             WHERE entries_fts MATCH ?",
        )
    } else {
        String::from(
            "SELECT e.id, e.title, e.date, e.updated_at, e.notebook_id, e.mood, e.starred,
                    e.snippet, e.word_count, e.reading_minutes, e.path, e.cover_photo,
                    e.has_audio, e.has_photos, e.tags_json, e.weather_json, e.location_json, e.stamps_json,
                    e.snippet AS body_snip,
                    '' AS any_snip,
                    0.0 AS rank
             FROM entries e
             WHERE 1=1",
        )
    };

    let mut bind: Vec<Value> = Vec::new();
    if let Some(expr) = fts_expr.clone() {
        bind.push(Value::Text(expr));
    }

    if let Some(nb) = query
        .notebook_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        sql.push_str(" AND e.notebook_id = ?");
        bind.push(Value::Text(nb.to_string()));
    }
    if let Some(tag) = query.tag.as_deref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM entry_tags et WHERE et.entry_id = e.id AND LOWER(et.tag) = LOWER(?))",
        );
        bind.push(Value::Text(tag.to_string()));
    }
    if let Some(mood) = query.mood.as_deref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(" AND LOWER(e.mood) = LOWER(?)");
        bind.push(Value::Text(mood.to_string()));
    }
    if query.starred_only {
        sql.push_str(" AND e.starred = 1");
    }
    if let Some(from) = query.from_date {
        sql.push_str(" AND e.date_ymd >= ?");
        bind.push(Value::Text(from.format("%Y-%m-%d").to_string()));
    }
    if let Some(to) = query.to_date {
        sql.push_str(" AND e.date_ymd <= ?");
        bind.push(Value::Text(to.format("%Y-%m-%d").to_string()));
    }

    if fts_expr.is_some() {
        sql.push_str(" ORDER BY rank ASC, e.date DESC");
    } else {
        sql.push_str(" ORDER BY e.date DESC, e.updated_at DESC");
    }

    let limit = query.limit.unwrap_or(50) as i64;
    let offset = query.offset.unwrap_or(0) as i64;
    sql.push_str(" LIMIT ? OFFSET ?");
    bind.push(Value::Integer(limit));
    bind.push(Value::Integer(offset));

    let mut stmt = conn.prepare(&sql)?;
    let is_fts = fts_expr.is_some();
    let rows = stmt.query_map(params_from_iter(bind), |row| {
        let summary = row_to_entry_summary(row)?;
        let body_snip: String = row.get(18)?;
        let any_snip: String = row.get(19)?;
        let highlighted_snippet = if is_fts {
            let chosen = if body_snip.contains('\x02') {
                body_snip
            } else if any_snip.contains('\x02') {
                any_snip
            } else if !body_snip.trim().is_empty() {
                body_snip
            } else {
                summary.snippet.clone()
            };
            format_safe_fts_snippet(&chosen)
        } else {
            escape_html_text(&body_snip)
        };
        let rank: f64 = row.get(20)?;
        Ok(SearchHit {
            summary,
            highlighted_snippet,
            rank,
        })
    })?;

    let mut hits = Vec::new();
    for r in rows {
        hits.push(r?);
    }
    Ok(hits)
}

/// Convert arbitrary user search text into a safe FTS5 MATCH expression.
fn build_fts5_query(raw: &str) -> Option<String> {
    let mut terms = Vec::new();
    for token in raw.split_whitespace() {
        let has_prefix = token.ends_with('*') && token.len() > 1;
        let core = if has_prefix {
            &token[..token.len() - 1]
        } else {
            token
        };
        let cleaned: String = core
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '\'')
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        let escaped = cleaned.replace('"', "\"\"");
        if has_prefix {
            terms.push(format!("\"{escaped}\"*"));
        } else {
            terms.push(format!("\"{escaped}\""));
        }
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

fn escape_sql_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn format_safe_fts_snippet(raw: &str) -> String {
    escape_html_text(raw)
        .replace('\x02', "<mark>")
        .replace('\x03', "</mark>")
}

/// Return all tags sorted by descending frequency, then alphabetically (case-insensitive grouping).
pub fn list_tags(conn: &Connection) -> Result<Vec<TagCount>> {
    let mut stmt = conn.prepare(
        "SELECT MIN(tag) AS tag, COUNT(DISTINCT entry_id) AS cnt
         FROM entry_tags
         GROUP BY LOWER(tag)
         ORDER BY cnt DESC, LOWER(tag) ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(TagCount {
            tag: row.get(0)?,
            count: row.get::<_, i64>(1)? as usize,
        })
    })?;

    let mut tags = Vec::new();
    for r in rows {
        tags.push(r?);
    }
    Ok(tags)
}

/// Build per-day calendar summaries for a given `(year, month)` and optional `notebook_id`.
pub fn get_calendar_month(
    conn: &Connection,
    year: i32,
    month: u32,
    notebook_id: Option<&str>,
) -> Result<Vec<CalendarDaySummary>> {
    let mut sql = String::from(
        "SELECT date_ymd, id, starred, mood, has_photos, has_audio
         FROM entries
         WHERE year = ? AND month = ?",
    );
    let mut bind: Vec<Value> = vec![Value::Integer(year as i64), Value::Integer(month as i64)];
    if let Some(nb) = notebook_id.filter(|s| !s.trim().is_empty()) {
        sql.push_str(" AND notebook_id = ?");
        bind.push(Value::Text(nb.to_string()));
    }
    sql.push_str(" ORDER BY date ASC, id ASC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(bind), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? != 0,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)? != 0,
            row.get::<_, i64>(5)? != 0,
        ))
    })?;

    let mut by_day: BTreeMap<NaiveDate, CalendarDaySummary> = BTreeMap::new();
    for r in rows {
        let (ymd_str, id_str, starred, mood, has_photos, has_audio) = r?;
        let date = NaiveDate::parse_from_str(&ymd_str, "%Y-%m-%d")
            .map_err(|e| VaultError::InvalidInput(format!("Corrupt date_ymd in index: {e}")))?;
        let entry_id = Uuid::parse_str(&id_str).ok();

        let summary = by_day.entry(date).or_insert_with(|| CalendarDaySummary {
            date,
            entry_count: 0,
            starred_count: 0,
            moods: Vec::new(),
            primary_entry_id: entry_id,
            has_photos: false,
            has_audio: false,
        });

        summary.entry_count += 1;
        if starred {
            summary.starred_count += 1;
            if summary.primary_entry_id.is_none() || summary.starred_count == 1 {
                summary.primary_entry_id = entry_id;
            }
        }
        if let Some(m) = mood.filter(|s| !s.trim().is_empty()) {
            if !summary
                .moods
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&m))
            {
                summary.moods.push(m);
            }
        }
        summary.has_photos |= has_photos;
        summary.has_audio |= has_audio;
    }

    Ok(by_day.into_values().collect())
}

/// Retrieve entries written on `(month, day)` in years prior to `current_year`.
pub fn get_on_this_day(
    conn: &Connection,
    month: u32,
    day: u32,
    current_year: i32,
) -> Result<Vec<OnThisDayGroup>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, date, updated_at, notebook_id, mood, starred,
                snippet, word_count, reading_minutes, path, cover_photo,
                has_audio, has_photos, tags_json, weather_json, location_json, stamps_json,
                year
         FROM entries
         WHERE month = ?1 AND day = ?2 AND year < ?3
         ORDER BY year DESC, date DESC",
    )?;

    let rows = stmt.query_map(
        params![month as i64, day as i64, current_year as i64],
        |row| {
            let summary = row_to_entry_summary(row)?;
            let year: i32 = row.get(18)?;
            Ok((year, summary))
        },
    )?;

    let mut groups: BTreeMap<i32, Vec<EntrySummary>> = BTreeMap::new();
    for r in rows {
        let (year, summary) = r?;
        groups.entry(year).or_default().push(summary);
    }

    // Order closest year first (1 year ago, 2 years ago, ...).
    let mut result: Vec<OnThisDayGroup> = groups
        .into_iter()
        .rev()
        .map(|(year, entries)| OnThisDayGroup {
            years_ago: current_year - year,
            year,
            entries,
        })
        .collect();
    result.sort_by_key(|g| g.years_ago);
    Ok(result)
}

/// Find all entries linking to `entry_id` via `[[wikilinks]]` matching its UUID, slug, or title.
pub fn get_backlinks(conn: &Connection, entry_id: Uuid) -> Result<Vec<BacklinkRef>> {
    let id_str = entry_id.to_string();
    let target: Option<(String, String)> = conn
        .query_row(
            "SELECT slug, title FROM entries WHERE id = ?1",
            params![&id_str],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let Some((slug, title)) = target else {
        return Ok(Vec::new());
    };

    let mut stmt = conn.prepare(
        "SELECT DISTINCT src.id, src.title, src.date, src.snippet
         FROM entry_links l
         JOIN entries src ON src.id = l.source_id
         WHERE l.source_id != ?1
           AND (
               LOWER(l.target_ref) = LOWER(?1)
               OR LOWER(l.target_ref) = LOWER(?2)
               OR LOWER(l.target_ref) = LOWER(?3)
           )
         ORDER BY src.date DESC",
    )?;

    let rows = stmt.query_map(params![&id_str, &slug, &title], |row| {
        let src_id_str: String = row.get(0)?;
        let src_title: String = row.get(1)?;
        let date_str: String = row.get(2)?;
        let snippet: String = row.get(3)?;
        Ok((src_id_str, src_title, date_str, snippet))
    })?;

    let mut backlinks = Vec::new();
    for r in rows {
        let (src_id_str, src_title, date_str, snippet) = r?;
        let src_uuid = Uuid::parse_str(&src_id_str)
            .map_err(|e| VaultError::InvalidInput(format!("Corrupt UUID in index: {e}")))?;
        let date = DateTime::parse_from_rfc3339(&date_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| VaultError::InvalidInput(format!("Corrupt timestamp in index: {e}")))?;
        backlinks.push(BacklinkRef {
            entry_id: src_uuid,
            title: src_title,
            date,
            snippet,
        });
    }
    Ok(backlinks)
}

/// Compute aggregate vault metrics and writing streaks.
pub fn compute_vault_metrics(conn: &Connection, today: NaiveDate) -> Result<VaultMetrics> {
    let (total_entries, total_words, total_photos, total_audio_memos): (i64, i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(word_count), 0),
                    COALESCE(SUM(photo_count), 0),
                    COALESCE(SUM(audio_count), 0)
             FROM entries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;

    let entries_this_month: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE year = ?1 AND month = ?2",
        params![today.year(), today.month() as i64],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare("SELECT DISTINCT date_ymd FROM entries ORDER BY date_ymd ASC")?;
    let date_rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut unique_dates = BTreeSet::new();
    for r in date_rows {
        let s = r?;
        if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            unique_dates.insert(d);
        }
    }

    let (current_streak_days, longest_streak_days) = compute_streaks(&unique_dates, today);
    let mut top_tags = list_tags(conn)?;
    top_tags.truncate(10);

    Ok(VaultMetrics {
        total_entries: total_entries as usize,
        total_words: total_words as usize,
        total_photos: total_photos as usize,
        total_audio_memos: total_audio_memos as usize,
        current_streak_days,
        longest_streak_days,
        entries_this_month: entries_this_month as usize,
        top_tags,
    })
}

pub struct VaultMetrics {
    pub total_entries: usize,
    pub total_words: usize,
    pub total_photos: usize,
    pub total_audio_memos: usize,
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
    pub entries_this_month: usize,
    pub top_tags: Vec<TagCount>,
}

fn compute_streaks(dates: &BTreeSet<NaiveDate>, today: NaiveDate) -> (u32, u32) {
    if dates.is_empty() {
        return (0, 0);
    }

    let mut longest = 0_u32;
    let mut current_run = 0_u32;
    let mut prev: Option<NaiveDate> = None;

    for &d in dates {
        if prev.is_some_and(|p| p + Duration::days(1) == d) {
            current_run += 1;
        } else {
            current_run = 1;
        }
        longest = longest.max(current_run);
        prev = Some(d);
    }

    // Active streak counts backwards from today if written today, else from yesterday.
    let anchor = if dates.contains(&today) {
        Some(today)
    } else {
        today
            .pred_opt()
            .filter(|&yesterday| dates.contains(&yesterday))
    };

    let current = match anchor {
        Some(mut cursor) => {
            let mut count = 0_u32;
            while dates.contains(&cursor) {
                count += 1;
                match cursor.pred_opt() {
                    Some(prev_day) => cursor = prev_day,
                    None => break,
                }
            }
            count
        }
        None => 0,
    };

    (current, longest)
}

fn slug_from_path_or_title(path: &str, title: &str) -> String {
    if let Some(stem) = Path::new(path).file_stem().and_then(|s| s.to_str()) {
        // Strip leading `YYYY-MM-DD-` if present so `[[slug]]` matches cleanly.
        if stem.len() > 11
            && stem.as_bytes()[4] == b'-'
            && stem.as_bytes()[7] == b'-'
            && stem.as_bytes()[10] == b'-'
            && stem[..4].bytes().all(|b| b.is_ascii_digit())
        {
            return stem[11..].to_string();
        }
        return stem.to_string();
    }
    slugify(title)
}

fn row_to_entry_summary(row: &Row<'_>) -> rusqlite::Result<EntrySummary> {
    let id_str: String = row.get(0)?;
    let title: String = row.get(1)?;
    let date_str: String = row.get(2)?;
    let updated_str: String = row.get(3)?;
    let notebook_id: Option<String> = row.get(4)?;
    let mood: Option<String> = row.get(5)?;
    let starred: i64 = row.get(6)?;
    let snippet: String = row.get(7)?;
    let word_count: i64 = row.get(8)?;
    let reading_minutes: i64 = row.get(9)?;
    let path: String = row.get(10)?;
    let cover_photo: Option<String> = row.get(11)?;
    let has_audio: i64 = row.get(12)?;
    let has_photos: i64 = row.get(13)?;
    let tags_json: String = row.get(14)?;
    let weather_json: Option<String> = row.get(15)?;
    let location_json: Option<String> = row.get(16)?;
    let stamps_json: String = row.get(17)?;

    let id = Uuid::parse_str(&id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let date = DateTime::parse_from_rfc3339(&date_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let updated_at = DateTime::parse_from_rfc3339(&updated_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?;

    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let weather: Option<WeatherInfo> = weather_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let location: Option<GeoLocation> = location_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let stamps: Vec<StampPlacement> = serde_json::from_str(&stamps_json).unwrap_or_default();

    Ok(EntrySummary {
        id,
        title,
        date,
        updated_at,
        notebook_id,
        tags,
        mood,
        starred: starred != 0,
        snippet,
        word_count: word_count as usize,
        reading_minutes: reading_minutes as u32,
        path,
        cover_photo,
        has_audio: has_audio != 0,
        has_photos: has_photos != 0,
        weather,
        location,
        stamps,
    })
}
