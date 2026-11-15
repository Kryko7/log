//! Error types for Day One and Notion journal importers.

use thiserror::Error;

/// Errors returned by `journal-import` operations.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Vault error: {0}")]
    Vault(#[from] journal_core::VaultError),

    #[error("Invalid import source: {0}")]
    InvalidSource(String),
}

pub type Result<T> = std::result::Result<T, ImportError>;
