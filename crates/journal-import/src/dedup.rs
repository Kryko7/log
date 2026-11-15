//! Normalization and idempotent deduplication helpers for Day One and Notion imports.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};
use std::sync::OnceLock;

use chrono::NaiveDate;
use journal_core::{markdown::split_frontmatter, Vault};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::error::Result;

fn img_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"!\[[^\]]*\]\([^)]*\)").expect("valid image regex"))
}

fn link_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").expect("valid link regex"))
}

/// In-memory index of entries already present in a [`Vault`] used for fast,
/// idempotent deduplication across Day One and Notion delta imports.
#[derive(Debug, Default)]
pub struct DeduplicationIndex {
    source_ids: HashSet<String>,
    dates_in_vault: HashSet<NaiveDate>,
    date_body_hashes: HashSet<(NaiveDate, String)>,
    date_titles_and_leads: HashSet<(NaiveDate, String)>,
}

impl DeduplicationIndex {
    /// Populate the deduplication index from all entries currently stored in `vault`.
    ///
    /// Reads raw vault `.md` files directly instead of calling `Vault::get_entry`
    /// to avoid redundant SQLite locks, HTML rendering, and backlink queries.
    pub fn from_vault(vault: &Vault) -> Result<Self> {
        let mut idx = Self::default();
        let summaries = vault.list_entries(None, None, None, None)?;
        let root = vault.root();

        for summary in summaries {
            let entry_date = summary.date.date_naive();
            let rel_path = Path::new(&summary.path);
            let is_safe = !summary.path.is_empty()
                && !rel_path.components().any(|c| {
                    matches!(
                        c,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                });

            if is_safe {
                if let Ok(raw) = fs::read_to_string(root.join(rel_path)) {
                    if let Ok((yaml, body)) = split_frontmatter(&raw) {
                        let source_id = extract_yaml_source_id(yaml);
                        idx.insert(source_id.as_deref(), entry_date, &summary.title, body);
                        continue;
                    }
                }
            }

            idx.dates_in_vault.insert(entry_date);
            let norm_title = normalize_text(&summary.title);
            if !norm_title.is_empty() {
                idx.date_titles_and_leads.insert((entry_date, norm_title));
            }
        }
        Ok(idx)
    }

    /// Record an existing or newly imported entry in the deduplication index.
    pub fn insert(&mut self, source_id: Option<&str>, date: NaiveDate, title: &str, body: &str) {
        if let Some(sid) = source_id.map(str::trim).filter(|s| !s.is_empty()) {
            self.source_ids.insert(sid.to_ascii_lowercase());
        }
        self.dates_in_vault.insert(date);

        let norm_body = normalize_text(body);
        if !norm_body.is_empty() {
            self.date_body_hashes.insert((date, sha256_hex(&norm_body)));
        }

        let norm_title = normalize_text(title);
        if !norm_title.is_empty() {
            self.date_titles_and_leads.insert((date, norm_title));
        }

        let norm_lead = normalized_lead_paragraph(body);
        if !norm_lead.is_empty() {
            self.date_titles_and_leads.insert((date, norm_lead));
        }
    }

    pub fn has_source_id(&self, source_id: &str) -> bool {
        let trimmed = source_id.trim();
        !trimmed.is_empty() && self.source_ids.contains(&trimmed.to_ascii_lowercase())
    }

    pub fn has_date(&self, date: NaiveDate) -> bool {
        self.dates_in_vault.contains(&date)
    }

    pub fn has_date_body_hash(&self, date: NaiveDate, body: &str) -> bool {
        let norm_body = normalize_text(body);
        if norm_body.is_empty() {
            return false;
        }
        self.date_body_hashes
            .contains(&(date, sha256_hex(&norm_body)))
    }

    pub fn has_date_title_or_lead(&self, date: NaiveDate, title: &str, body: &str) -> bool {
        let norm_title = normalize_text(title);
        if !norm_title.is_empty() && self.date_titles_and_leads.contains(&(date, norm_title)) {
            return true;
        }
        let norm_lead = normalized_lead_paragraph(body);
        !norm_lead.is_empty() && self.date_titles_and_leads.contains(&(date, norm_lead))
    }
}

fn extract_yaml_source_id(yaml: &str) -> Option<String> {
    for line in yaml.lines() {
        // Only match top-level `source_id:` (ignore nested keys under photos/weather/etc.).
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim_end_matches('\r');
        if let Some(rest) = trimmed.strip_prefix("source_id:") {
            let val = rest.trim();
            if val.is_empty() || val == "null" || val == "~" {
                return None;
            }
            let unquoted = val
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| val.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(val)
                .trim();
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

fn sha256_hex(norm: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(norm.as_bytes());
    hex::encode(hasher.finalize())
}

/// Normalize prose for comparison by stripping Markdown image/link URLs, formatting tokens,
/// and collapsing whitespace into single lowercase spaces.
pub fn normalize_text(input: &str) -> String {
    let without_imgs = img_regex().replace_all(input, " ");
    let without_links = link_regex().replace_all(&without_imgs, "$1");

    let mut cleaned = String::with_capacity(without_links.len());
    for ch in without_links.chars() {
        if ch.is_alphanumeric() || ch.is_whitespace() {
            for lower in ch.to_lowercase() {
                cleaned.push(lower);
            }
        } else if matches!(ch, '#' | '*' | '_' | '`' | '>' | '-' | '\\') {
            cleaned.push(' ');
        } else {
            cleaned.push(ch);
        }
    }

    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Compute a deterministic SHA-256 hex digest of normalized Markdown body text.
pub fn normalized_body_hash(body: &str) -> String {
    sha256_hex(&normalize_text(body))
}

/// Extract and normalize the first non-empty prose paragraph (skipping pure image lines
/// and leading `#` heading markers if a prose paragraph follows).
pub fn normalized_lead_paragraph(body: &str) -> String {
    let mut first_heading: Option<String> = None;
    for raw_line in body.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("![") && trimmed.ends_with(')') {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix('#') {
            let h = normalize_text(heading.trim_start_matches('#').trim());
            if first_heading.is_none() && !h.is_empty() {
                first_heading = Some(h);
            }
            continue;
        }
        let norm = normalize_text(trimmed);
        if !norm.is_empty() {
            return norm;
        }
    }
    first_heading.unwrap_or_default()
}

/// Derive a clean human title from a Markdown body or fallback to the entry date.
pub fn derive_title_from_body(body: &str, fallback_date: NaiveDate) -> String {
    for raw_line in body.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("![") {
            continue;
        }
        let candidate = trimmed
            .trim_start_matches('#')
            .trim_start_matches('>')
            .trim()
            .replace(['*', '_', '`'], "");
        let collapsed = candidate.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            continue;
        }
        if collapsed.chars().count() <= 64 {
            return collapsed;
        }
        let mut truncated: String = collapsed.chars().take(64).collect();
        if let Some(last_space) = truncated.rfind(' ') {
            if last_space > 20 {
                truncated.truncate(last_space);
            }
        }
        return format!("{truncated}…");
    }
    format!("Journal Entry — {}", fallback_date.format("%Y-%m-%d"))
}
