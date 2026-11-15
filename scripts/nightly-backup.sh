#!/usr/bin/env bash
# Nightly / scheduled client-side age-encrypted backup runner for Log.
# Safe for cron, systemd timers, or /schedule automation.
#
# Environment variables:
#   LOG_VAULT_DIR              Vault path (default: "$HOME/Personal/JournalVault")
#   LOG_BACKUP_PASSPHRASE      Passphrase used for age encryption
#   LOG_BACKUP_PASSPHRASE_FILE Optional file containing passphrase (read if env var unset)
#   LOG_SYNC_GDRIVE            Set to "1" (or pass --upload-gdrive) to upload .age archive

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VAULT_DIR="${LOG_VAULT_DIR:-$HOME/Personal/JournalVault}"

if [[ -z "${LOG_BACKUP_PASSPHRASE:-}" && -n "${LOG_BACKUP_PASSPHRASE_FILE:-}" ]]; then
    if [[ ! -r "$LOG_BACKUP_PASSPHRASE_FILE" ]]; then
        echo "Error: passphrase file not readable: $LOG_BACKUP_PASSPHRASE_FILE" >&2
        exit 1
    fi
    LOG_BACKUP_PASSPHRASE="$(tr -d '\r\n' < "$LOG_BACKUP_PASSPHRASE_FILE")"
    export LOG_BACKUP_PASSPHRASE
fi

if [[ -z "${LOG_BACKUP_PASSPHRASE:-}" ]]; then
    echo "Error: set LOG_BACKUP_PASSPHRASE or LOG_BACKUP_PASSPHRASE_FILE before running nightly-backup.sh" >&2
    exit 1
fi

BIN="${LOG_JOURNAL_BIN:-}"
if [[ -z "$BIN" ]]; then
    if [[ -x "$REPO_ROOT/log-journal" ]]; then
        BIN="$REPO_ROOT/log-journal"
    elif [[ -x "/tmp/log-cargo-target/release/log-journal" ]]; then
        BIN="/tmp/log-cargo-target/release/log-journal"
    elif command -v log-journal >/dev/null 2>&1; then
        BIN="$(command -v log-journal)"
    else
        echo "Error: log-journal binary not found. Build with: CARGO_TARGET_DIR=/tmp/log-cargo-target cargo build --manifest-path src-tauri/Cargo.toml --release" >&2
        exit 1
    fi
fi

ARGS=("--vault" "$VAULT_DIR" "backup" "--passphrase-env" "LOG_BACKUP_PASSPHRASE")
if [[ "${LOG_SYNC_GDRIVE:-0}" == "1" || "${1:-}" == "--upload-gdrive" ]]; then
    ARGS+=("--upload-gdrive")
fi

"$BIN" "${ARGS[@]}"
