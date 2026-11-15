//! Error types for client-side `age` encryption, Google Drive sync, and daily context cues.

use thiserror::Error;

/// Errors returned by `journal-sync` operations.
#[derive(Debug, Error)]
pub enum SyncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Encryption/decryption error: {0}")]
    Crypto(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Backup archive not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, SyncError>;
