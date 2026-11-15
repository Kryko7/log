//! Core vault storage, Markdown + YAML frontmatter serialization,
//! SQLite FTS5 indexing, templates, media thumbnails, backlinks, and On This Day queries.

pub mod error;
pub mod index;
pub mod markdown;
pub mod media;
pub mod templates;
pub mod vault;

use std::path::PathBuf;

pub use error::{Result, VaultError};
pub use markdown::{
    extract_wikilinks, parse_entry_file, parse_entry_file_no_html, render_html_preview,
    serialize_entry, slugify,
};
pub use vault::{Vault, VaultWatcher};

/// Resolve the default journal vault path (`~/Personal/JournalVault`),
/// respecting the `LOG_VAULT_DIR` environment variable override.
pub fn default_vault_dir() -> PathBuf {
    Vault::default_path()
}
