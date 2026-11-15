//! Core vault storage, Markdown + YAML frontmatter serialization,
//! SQLite FTS5 indexing, templates, media thumbnails, backlinks, and On This Day queries.

use std::path::PathBuf;

/// Resolve the default journal vault path (`~/Personal/JournalVault`),
/// respecting the `LOG_VAULT_DIR` environment variable override.
pub fn default_vault_dir() -> PathBuf {
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
