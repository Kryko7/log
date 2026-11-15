#!/usr/bin/env bash
# Batch migration helper: imports Day One (.zip/.json) first, then Notion delta (.zip/.json/folder),
# using the Tauri CLI binary (`log-journal`).
#
# Usage:
#   scripts/import-all.sh [--vault <PATH>] [--dayone <ZIP_OR_JSON>] [--notion <PATH>] [--dry-run]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VAULT_DIR="${LOG_VAULT_DIR:-$HOME/Personal/JournalVault}"
DAYONE_PATH=""
NOTION_PATH=""
DRY_RUN=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --vault)
            VAULT_DIR="$2"
            shift 2
            ;;
        --dayone)
            DAYONE_PATH="$2"
            shift 2
            ;;
        --notion)
            NOTION_PATH="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN="--dry-run"
            shift
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

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

if [[ -n "$DAYONE_PATH" ]]; then
    echo "Importing Day One archive: $DAYONE_PATH"
    "$BIN" --vault "$VAULT_DIR" import-dayone --zip "$DAYONE_PATH" ${DRY_RUN:+$DRY_RUN}
fi

if [[ -n "$NOTION_PATH" ]]; then
    echo "Importing Notion delta: $NOTION_PATH"
    "$BIN" --vault "$VAULT_DIR" import-notion --path "$NOTION_PATH" ${DRY_RUN:+$DRY_RUN}
fi

if [[ -z "$DAYONE_PATH" && -z "$NOTION_PATH" ]]; then
    echo "Nothing to import. Pass --dayone <PATH> and/or --notion <PATH>."
fi
