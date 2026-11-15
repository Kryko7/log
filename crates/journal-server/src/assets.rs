//! Asset manifest and static directory path resolution for `journal-server`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use journal_types::AssetManifest;

/// Locate a repository-relative directory or file (e.g., `assets/manifest.json`,
/// `assets`, `crates/journal-ui/dist`) using executable-relative and CWD-relative lookup.
pub fn resolve_repo_path(relative: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(relative));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join(relative));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join(relative));
            if let Some(p1) = exe_dir.parent() {
                candidates.push(p1.join(relative));
                if let Some(p2) = p1.parent() {
                    candidates.push(p2.join(relative));
                }
            }
        }
    }

    // Compile-time fallback for cargo run / tests.
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_root.parent().and_then(Path::parent) {
        candidates.push(workspace_root.join(relative));
    }

    candidates.into_iter().find(|p| p.exists())
}

/// Load `assets/manifest.json` from exe-relative or CWD-relative paths.
pub fn load_asset_manifest() -> Result<AssetManifest> {
    let path = resolve_repo_path("assets/manifest.json")
        .context("could not locate assets/manifest.json relative to CWD or executable")?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let manifest: AssetManifest = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(manifest)
}
