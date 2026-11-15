//! Client-side `age`-encrypted vault backups, Google Drive sync, and daily context cues.

pub mod context;
pub mod crypto;
pub mod error;
pub mod gdrive;

pub use context::{get_daily_context, map_wmo_weather_code, parse_calendar_json};
pub use crypto::{create_backup, list_backups, restore_backup};
pub use error::{Result, SyncError};
pub use gdrive::sync_gdrive;
