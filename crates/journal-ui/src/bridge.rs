//! Typed HTTP / Tauri IPC bridge to `journal-server`.

use journal_types::VaultStats;

/// Fetch vault statistics from the local API.
pub async fn fetch_stats() -> Result<VaultStats, String> {
    Err("bridge skeleton".to_string())
}
