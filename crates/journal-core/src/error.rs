use thiserror::Error;

/// Convenience result alias for all `journal-core` vault operations.
pub type Result<T> = std::result::Result<T, VaultError>;

/// Errors produced by vault storage, indexing, and media processing.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML serialization error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SQLite index error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Image processing error: {0}")]
    Image(#[from] image::ImageError),

    #[error("WAV audio decoding error: {0}")]
    Audio(#[from] hound::Error),

    #[error("Filesystem watcher error: {0}")]
    Notify(#[from] notify::Error),

    #[error("Entry or resource not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Internal vault lock poisoned")]
    LockPoisoned,
}
